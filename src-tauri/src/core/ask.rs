use crate::core::digest::extract_json;
use crate::core::index_store::Chunk;
use crate::core::provider::LlmProvider;
use anyhow::Result;
use serde::Deserialize;

pub struct Answer {
    pub text: String,
    pub citations: Vec<String>,
}

/// The judge's decision about a draft answer.
#[derive(Debug)]
enum Verdict {
    /// The draft is grounded in the context; use it.
    Accept,
    /// The draft has unsupported claims; the string is feedback for the redraft.
    Revise(String),
    /// The context does not support an answer; respond that we don't know.
    Abstain,
}

/// Why a raw judge reply could not be turned into a `Verdict`.
#[derive(Debug)]
struct VerdictError(String);

/// Raw JSON shape the judge must return.
#[derive(Deserialize)]
struct VerdictJson {
    verdict: String,
    #[serde(default)]
    feedback: String,
}

/// Parse a raw judge reply into a `Verdict`, reusing `extract_json` to peel
/// ```fences```/prose. An unparseable reply or unknown verdict word is an error;
/// the caller treats that as fail-closed (abstain).
fn parse_verdict(raw: &str) -> std::result::Result<Verdict, VerdictError> {
    let vj: VerdictJson =
        serde_json::from_str(extract_json(raw)).map_err(|e| VerdictError(e.to_string()))?;
    match vj.verdict.trim().to_ascii_lowercase().as_str() {
        "accept" => Ok(Verdict::Accept),
        "revise" => Ok(Verdict::Revise(vj.feedback)),
        "abstain" => Ok(Verdict::Abstain),
        other => Err(VerdictError(format!("unknown verdict {other:?}"))),
    }
}

/// The retrieved paths the answer actually cited, in hits-rank order, de-duplicated.
/// A `[path]` the answer cites that is not among `hits` is dropped (hallucinated).
fn filter_citations(answer: &str, hits: &[&Chunk]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for h in hits {
        if out.contains(&h.path) {
            continue;
        }
        if answer.contains(&format!("[{}]", h.path)) {
            out.push(h.path.clone());
        }
    }
    out
}

/// Ask the LLM to answer `question` grounded ONLY in the pre-retrieved `hits`.
/// Citations are the hit page paths, de-duplicated, preserving rank order.
pub async fn ask(provider: &dyn LlmProvider, question: &str, hits: &[&Chunk]) -> Result<Answer> {
    let context = hits
        .iter()
        .map(|h| format!("[{}]\n{}", h.path, h.text))
        .collect::<Vec<_>>()
        .join("\n\n");

    let mut citations: Vec<String> = Vec::new();
    for h in hits {
        if !citations.contains(&h.path) {
            citations.push(h.path.clone());
        }
    }

    let system = "Answer ONLY from the provided wiki context. Cite the page paths you used in [brackets]. If the context does not contain the answer, say you don't know.";
    let user = format!("QUESTION: {question}\n\nWIKI CONTEXT:\n{context}");
    let text = provider.complete(system, &user).await?;
    Ok(Answer { text, citations })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::index_store::Chunk;
    use crate::core::provider::fake::FakeProvider;

    fn chunk(path: &str, text: &str) -> Chunk {
        Chunk {
            path: path.into(),
            chunk_id: 0,
            text: text.into(),
            vector: vec![],
        }
    }

    #[test]
    fn filter_citations_keeps_only_cited_hits_in_rank_order() {
        let a = chunk("concepts/a.md", "");
        let b = chunk("concepts/b.md", "");
        let hits = vec![&a, &b];
        // Answer cites b before a, but output is ordered by hits rank (a, then b).
        let cites = filter_citations("see [concepts/b.md] and [concepts/a.md]", &hits);
        assert_eq!(
            cites,
            vec!["concepts/a.md".to_string(), "concepts/b.md".to_string()]
        );
    }

    #[test]
    fn filter_citations_drops_uncited_and_hallucinated() {
        let a = chunk("concepts/a.md", "");
        let hits = vec![&a];
        // Cites a path that is not in hits, and does not cite a.
        let cites = filter_citations("per [concepts/ghost.md]", &hits);
        assert!(cites.is_empty());
    }

    #[test]
    fn filter_citations_dedupes_repeated_path() {
        let a0 = chunk("concepts/a.md", "");
        let mut a1 = chunk("concepts/a.md", "");
        a1.chunk_id = 1;
        let hits = vec![&a0, &a1];
        let cites = filter_citations("[concepts/a.md]", &hits);
        assert_eq!(cites, vec!["concepts/a.md".to_string()]);
    }

    #[test]
    fn parse_verdict_accept() {
        assert!(matches!(
            parse_verdict(r#"{"verdict":"accept"}"#),
            Ok(Verdict::Accept)
        ));
    }

    #[test]
    fn parse_verdict_revise_carries_feedback() {
        match parse_verdict(r#"{"verdict":"revise","feedback":"claim X unsupported"}"#) {
            Ok(Verdict::Revise(f)) => assert_eq!(f, "claim X unsupported"),
            other => panic!("expected Revise, got {other:?}"),
        }
    }

    #[test]
    fn parse_verdict_abstain() {
        assert!(matches!(
            parse_verdict(r#"{"verdict":"abstain"}"#),
            Ok(Verdict::Abstain)
        ));
    }

    #[test]
    fn parse_verdict_handles_fenced_json() {
        assert!(matches!(
            parse_verdict("```json\n{\"verdict\":\"accept\"}\n```"),
            Ok(Verdict::Accept)
        ));
    }

    #[test]
    fn parse_verdict_unparseable_errs() {
        assert!(parse_verdict("not json").is_err());
    }

    #[test]
    fn parse_verdict_unknown_verdict_errs() {
        assert!(parse_verdict(r#"{"verdict":"maybe"}"#).is_err());
    }

    #[tokio::test]
    async fn answers_from_hits_and_dedupes_citations() {
        let c0 = Chunk {
            path: "concepts/vd.md".into(),
            chunk_id: 0,
            text: "Vitamin D helps sleep.".into(),
            vector: vec![],
        };
        let c1 = Chunk {
            path: "concepts/vd.md".into(),
            chunk_id: 1,
            text: "Take it in the morning.".into(),
            vector: vec![],
        };
        let hits: Vec<&Chunk> = vec![&c0, &c1];
        let p = FakeProvider {
            reply: "Morning dose [concepts/vd.md]".into(),
        };
        let a = ask(&p, "when to take vitamin d", &hits).await.unwrap();
        assert!(a.text.contains("Morning"));
        assert_eq!(a.citations, vec!["concepts/vd.md".to_string()]);
    }
}
