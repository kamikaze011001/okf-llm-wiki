use crate::core::index_store::Chunk;
use crate::core::provider::LlmProvider;
use anyhow::Result;

pub struct Answer {
    pub text: String,
    pub citations: Vec<String>,
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
