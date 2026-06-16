use crate::core::page::{Page, Frontmatter};
use crate::core::provider::LlmProvider;
use crate::core::slug::slugify;
use anyhow::{Result, anyhow};
use serde::Deserialize;
use std::collections::BTreeMap;

/// JSON contract the LLM must return.
#[derive(Deserialize)]
struct DigestJson { title: String, description: String, tags: Vec<String>, body: String }

pub struct DigestResult { pub page: Page, pub log_entry: String }

pub async fn digest(
    provider: &dyn LlmProvider,
    source_text: &str,
    resource: Option<&str>,
    note: Option<&str>,
) -> Result<DigestResult> {
    let system = "You write one OKF wiki page from a source. \
        Respond ONLY with JSON: {\"title\":..,\"description\":..,\"tags\":[..],\"body\":..}. \
        The body is Markdown beginning with a bold TL;DR line, then '## Key points'.";
    let user = format!("SOURCE:\n{source_text}\n\nUSER NOTE: {}", note.unwrap_or(""));
    let raw = provider.complete(system, &user).await?;
    let parsed: DigestJson = serde_json::from_str(raw.trim())
        .map_err(|e| anyhow!("LLM did not return valid digest JSON: {e}; got: {raw}"))?;
    let slug = slugify(&parsed.title);
    let page = Page {
        path: format!("concepts/{slug}.md"),
        frontmatter: Frontmatter {
            type_: "Concept".into(),
            title: Some(parsed.title.clone()),
            description: Some(parsed.description),
            tags: parsed.tags,
            resource: resource.map(|s| s.to_string()),
            timestamp: Some(now_iso()),
            note: note.map(|s| s.to_string()),
            extra: BTreeMap::new(),
        },
        body: parsed.body,
    };
    Ok(DigestResult { log_entry: format!("Added page: {}", parsed.title), page })
}

fn now_iso() -> String {
    // Minimal RFC3339-ish stamp without extra deps.
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    format!("unixtime:{secs}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::provider::fake::FakeProvider;
    #[tokio::test]
    async fn produces_concept_page_from_llm_json() {
        let reply = r#"{"title":"Vitamin D & Sleep","description":"d","tags":["sleep"],"body":"**TL;DR.** morning."}"#;
        let p = FakeProvider { reply: reply.into() };
        let r = digest(&p, "some source", Some("https://x"), Some("winter")).await.unwrap();
        assert_eq!(r.page.path, "concepts/vitamin-d-sleep.md");
        assert_eq!(r.page.frontmatter.title, Some("Vitamin D & Sleep".into()));
        assert_eq!(r.page.frontmatter.note, Some("winter".into()));
        assert!(r.page.body.contains("TL;DR"));
    }

    #[tokio::test]
    async fn errors_on_malformed_json() {
        let p = FakeProvider { reply: "not json".into() };
        assert!(digest(&p, "some source", None, None).await.is_err());
    }
}
