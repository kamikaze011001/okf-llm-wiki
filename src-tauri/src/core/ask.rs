use crate::core::index_store::Chunk;
use crate::core::provider::LlmProvider;
use anyhow::Result;

pub struct Answer {
    pub text: String,
    pub citations: Vec<String>,
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
