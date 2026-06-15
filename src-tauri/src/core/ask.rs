use crate::core::provider::LlmProvider;
use crate::core::retrieval::{IndexEntry, search};
use anyhow::Result;

pub struct Answer { pub text: String, pub citations: Vec<String> }

pub async fn ask(
    provider: &dyn LlmProvider,
    question: &str,
    index: &[IndexEntry],
) -> Result<Answer> {
    let hits = search(question, index, 4);
    let citations: Vec<String> = hits.iter().map(|h| h.path.clone()).collect();
    let context = hits.iter()
        .map(|h| format!("[{}]\n{}", h.path, h.snippet))
        .collect::<Vec<_>>().join("\n\n");
    let system = "Answer ONLY from the provided wiki context. Cite page paths in [brackets]. \
        If the context is insufficient, say so.";
    let user = format!("QUESTION: {question}\n\nWIKI CONTEXT:\n{context}");
    let text = provider.complete(system, &user).await?;
    Ok(Answer { text, citations })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::provider::fake::FakeProvider;
    use crate::core::retrieval::hash_embed;
    #[tokio::test]
    async fn answers_and_returns_citations() {
        let index = vec![
            IndexEntry { path: "concepts/sleep.md".into(), vector: hash_embed("vitamin d sleep"), snippet: "morning dose".into() },
        ];
        let p = FakeProvider { reply: "Take it in the morning [concepts/sleep.md]".into() };
        let a = ask(&p, "vitamin d sleep timing", &index).await.unwrap();
        assert!(a.text.contains("morning"));
        assert_eq!(a.citations, vec!["concepts/sleep.md".to_string()]);
    }
}
