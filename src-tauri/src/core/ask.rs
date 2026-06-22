use crate::core::digest::extract_json;
use crate::core::index_store::Chunk;
use crate::core::provider::LlmProvider;
use anyhow::Result;
use serde::Deserialize;

#[derive(Debug)]
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

/// Raw JSON shape the judge must return.
#[derive(Deserialize)]
struct VerdictJson {
    verdict: String,
    #[serde(default)]
    feedback: String,
}

/// Parse a raw judge reply into a `Verdict`, reusing `extract_json` to peel
/// ```fences```/prose. `None` means the reply was unparseable or named an unknown
/// verdict; the caller treats that as fail-closed (abstain). No error payload is
/// carried — the fail-closed path discards it and nothing is logged.
fn parse_verdict(raw: &str) -> Option<Verdict> {
    let vj: VerdictJson = serde_json::from_str(extract_json(raw)).ok()?;
    match vj.verdict.trim().to_ascii_lowercase().as_str() {
        "accept" => Some(Verdict::Accept),
        "revise" => Some(Verdict::Revise(vj.feedback)),
        "abstain" => Some(Verdict::Abstain),
        _ => None,
    }
}

const ANSWER_SYSTEM: &str = "Answer the question using ONLY the provided wiki context. \
Cite the page paths you used in [brackets], e.g. [concepts/foo.md]. \
If the context does not contain the answer, say you don't know.";

const JUDGE_SYSTEM: &str = "You are a strict grounding judge. Given a QUESTION, the \
WIKI CONTEXT, and a DRAFT ANSWER, decide whether the draft is fully supported by the \
context. Respond ONLY with JSON {\"verdict\":\"accept\"|\"revise\"|\"abstain\",\"feedback\":\"...\"}. \
Use \"accept\" if every claim is supported by the context and any [bracketed] citations \
refer to provided paths. Use \"abstain\" if the context does not contain the answer. \
Use \"revise\" if the draft makes claims not supported by the context; put what is wrong \
in \"feedback\".";

/// The first-attempt draft user message.
fn base_ask_prompt(question: &str, context: &str) -> String {
    format!("QUESTION: {question}\n\nWIKI CONTEXT:\n{context}")
}

/// The retry draft message: name the grounding problem, echo the previous draft,
/// restate the grounding rule, then re-append the question and context.
fn repair_ask_prompt(question: &str, context: &str, prev_draft: &str, feedback: &str) -> String {
    format!(
        "Your previous answer was not adequately grounded: {feedback}.\n\n\
         Previous answer:\n{prev_draft}\n\n\
         Revise it. Use ONLY the wiki context below and cite page paths in [brackets]. \
         Remove or correct any claim not supported by the context; if the context does \
         not support an answer, say you don't know.\n\n\
         QUESTION: {question}\n\nWIKI CONTEXT:\n{context}"
    )
}

/// The judge user message: question + context + the draft to evaluate.
fn judge_user_prompt(question: &str, context: &str, draft: &str) -> String {
    format!("QUESTION: {question}\n\nWIKI CONTEXT:\n{context}\n\nDRAFT ANSWER:\n{draft}")
}

/// The canonical "couldn't ground it" answer, with no citations.
fn abstention() -> Answer {
    Answer {
        text: "I couldn't find a grounded answer for this in your wiki.".to_string(),
        citations: Vec::new(),
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

/// Ask the LLM to answer `question` grounded ONLY in the pre-retrieved `hits`,
/// verifying each draft with an LLM judge and retrying with feedback. Returns a
/// grounded answer (citing only retrieved paths it used) or a canonical
/// abstention when the wiki cannot ground an answer. Transport errors propagate.
pub async fn ask(provider: &dyn LlmProvider, question: &str, hits: &[&Chunk]) -> Result<Answer> {
    const MAX_ATTEMPTS: usize = 2;
    run_ask_attempts(provider, question, hits, MAX_ATTEMPTS).await
}

/// Draft -> judge -> repair loop. Each iteration makes one draft call and one judge
/// call (both propagate transport errors via `?`). `accept` returns the draft with
/// filtered citations; `abstain`, an unparseable verdict, or attempt exhaustion
/// returns the canonical abstention; `revise` re-drafts with feedback.
async fn run_ask_attempts(
    provider: &dyn LlmProvider,
    question: &str,
    hits: &[&Chunk],
    max_attempts: usize,
) -> Result<Answer> {
    let context = hits
        .iter()
        .map(|h| format!("[{}]\n{}", h.path, h.text))
        .collect::<Vec<_>>()
        .join("\n\n");

    let mut last: Option<(String, String)> = None; // (prev_draft, feedback)
    for _ in 1..=max_attempts {
        let user = match &last {
            None => base_ask_prompt(question, &context),
            Some((prev_draft, feedback)) => {
                repair_ask_prompt(question, &context, prev_draft, feedback)
            }
        };
        let draft = provider.complete(ANSWER_SYSTEM, &user).await?;

        let verdict_user = judge_user_prompt(question, &context, &draft);
        let verdict_raw = provider.complete(JUDGE_SYSTEM, &verdict_user).await?;

        match parse_verdict(&verdict_raw) {
            Some(Verdict::Accept) => {
                let citations = filter_citations(&draft, hits);
                return Ok(Answer {
                    text: draft,
                    citations,
                });
            }
            Some(Verdict::Abstain) => return Ok(abstention()),
            Some(Verdict::Revise(feedback)) => last = Some((draft, feedback)),
            // Fail-closed: we cannot certify groundedness, so abstain.
            None => return Ok(abstention()),
        }
    }
    Ok(abstention())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::index_store::Chunk;
    use crate::core::provider::fake::ScriptedProvider;

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
    fn filter_citations_empty_when_answer_has_no_citation_tokens() {
        let a = chunk("concepts/a.md", "text");
        let cites = filter_citations("no citations here", &[&a]);
        assert!(cites.is_empty());
    }

    #[test]
    fn parse_verdict_accept() {
        assert!(matches!(
            parse_verdict(r#"{"verdict":"accept"}"#),
            Some(Verdict::Accept)
        ));
    }

    #[test]
    fn parse_verdict_revise_carries_feedback() {
        match parse_verdict(r#"{"verdict":"revise","feedback":"claim X unsupported"}"#) {
            Some(Verdict::Revise(f)) => assert_eq!(f, "claim X unsupported"),
            other => panic!("expected Revise, got {other:?}"),
        }
    }

    #[test]
    fn parse_verdict_abstain() {
        assert!(matches!(
            parse_verdict(r#"{"verdict":"abstain"}"#),
            Some(Verdict::Abstain)
        ));
    }

    #[test]
    fn parse_verdict_handles_fenced_json() {
        assert!(matches!(
            parse_verdict("```json\n{\"verdict\":\"accept\"}\n```"),
            Some(Verdict::Accept)
        ));
    }

    #[test]
    fn parse_verdict_unparseable_none() {
        assert!(parse_verdict("not json").is_none());
    }

    #[test]
    fn parse_verdict_unknown_verdict_none() {
        assert!(parse_verdict(r#"{"verdict":"maybe"}"#).is_none());
    }

    #[test]
    fn parse_verdict_case_insensitive() {
        assert!(matches!(
            parse_verdict(r#"{"verdict":"Accept"}"#),
            Some(Verdict::Accept)
        ));
    }

    #[test]
    fn repair_prompt_includes_feedback_and_prev_draft() {
        let p = repair_ask_prompt("q", "CTXDATA", "OLD ANSWER", "claim X unsupported");
        assert!(p.contains("claim X unsupported"));
        assert!(p.contains("OLD ANSWER"));
        assert!(p.contains("CTXDATA"));
    }

    #[test]
    fn judge_prompt_includes_draft_and_context() {
        let p = judge_user_prompt("q", "CTXDATA", "DRAFTDATA");
        assert!(p.contains("CTXDATA"));
        assert!(p.contains("DRAFTDATA"));
    }

    #[test]
    fn abstention_has_empty_citations() {
        let a = abstention();
        assert!(a.citations.is_empty());
        assert!(a.text.to_lowercase().contains("couldn't find"));
    }

    #[tokio::test]
    async fn answers_from_hits_and_filters_citations() {
        let c0 = chunk("concepts/vd.md", "Vitamin D helps sleep.");
        let mut c1 = chunk("concepts/vd.md", "Take it in the morning.");
        c1.chunk_id = 1;
        let hits: Vec<&Chunk> = vec![&c0, &c1];
        let p = ScriptedProvider::new(vec![
            "Morning dose [concepts/vd.md]".into(),
            r#"{"verdict":"accept"}"#.into(),
        ]);
        let a = ask(&p, "when to take vitamin d", &hits).await.unwrap();
        assert!(a.text.contains("Morning"));
        assert_eq!(a.citations, vec!["concepts/vd.md".to_string()]);
        assert_eq!(p.calls(), 2);
    }

    #[tokio::test]
    async fn revise_then_accept() {
        let c = chunk("concepts/a.md", "Fact A.");
        let hits = vec![&c];
        let p = ScriptedProvider::new(vec![
            "ungrounded [concepts/a.md]".into(),
            r#"{"verdict":"revise","feedback":"claim unsupported"}"#.into(),
            "fixed answer [concepts/a.md]".into(),
            r#"{"verdict":"accept"}"#.into(),
        ]);
        let a = ask(&p, "q", &hits).await.unwrap();
        assert!(a.text.contains("fixed answer"));
        assert_eq!(a.citations, vec!["concepts/a.md".to_string()]);
        assert_eq!(p.calls(), 4);
    }

    #[tokio::test]
    async fn judge_abstains_returns_canonical() {
        let c = chunk("concepts/a.md", "Fact A.");
        let hits = vec![&c];
        let p = ScriptedProvider::new(vec![
            "guess".into(),
            r#"{"verdict":"abstain","feedback":"not in context"}"#.into(),
        ]);
        let a = ask(&p, "q", &hits).await.unwrap();
        assert!(a.text.contains("couldn't find a grounded answer"));
        assert!(a.citations.is_empty());
        assert_eq!(p.calls(), 2);
    }

    #[tokio::test]
    async fn exhaustion_abstains() {
        let c = chunk("concepts/a.md", "Fact A.");
        let hits = vec![&c];
        let p = ScriptedProvider::new(vec![
            "d1".into(),
            r#"{"verdict":"revise","feedback":"bad"}"#.into(),
            "d2".into(),
            r#"{"verdict":"revise","feedback":"still bad"}"#.into(),
        ]);
        let a = ask(&p, "q", &hits).await.unwrap();
        assert!(a.text.contains("couldn't find a grounded answer"));
        assert!(a.citations.is_empty());
        assert_eq!(p.calls(), 4);
    }

    #[tokio::test]
    async fn unparseable_judge_abstains() {
        let c = chunk("concepts/a.md", "Fact A.");
        let hits = vec![&c];
        let p = ScriptedProvider::new(vec!["answer [concepts/a.md]".into(), "not json".into()]);
        let a = ask(&p, "q", &hits).await.unwrap();
        assert!(a.text.contains("couldn't find a grounded answer"));
        assert!(a.citations.is_empty());
        assert_eq!(p.calls(), 2);
    }

    #[tokio::test]
    async fn transport_error_propagates_without_retry() {
        let c = chunk("concepts/a.md", "Fact A.");
        let hits = vec![&c];
        // Empty queue makes the first complete() call error, standing in for a
        // transport failure: it must propagate, not abstain.
        let p = ScriptedProvider::new(vec![]);
        let err = ask(&p, "q", &hits).await.unwrap_err();
        assert!(format!("{err}").contains("exhausted"));
        assert_eq!(p.calls(), 1);
    }

    #[tokio::test]
    async fn judge_transport_error_propagates() {
        let c = chunk("concepts/a.md", "Fact A.");
        let hits = vec![&c];
        // Draft succeeds, then the judge call exhausts the queue → transport-like Err
        // from the second complete(). It must propagate, not abstain.
        let p = ScriptedProvider::new(vec!["answer text".into()]);
        let err = ask(&p, "q", &hits).await.unwrap_err();
        assert!(format!("{err}").contains("exhausted"));
        assert_eq!(p.calls(), 2);
    }
}
