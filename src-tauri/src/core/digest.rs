use crate::core::page::{Frontmatter, Page};
use crate::core::provider::LlmProvider;
use crate::core::slug::slugify;
use anyhow::{anyhow, Result};
use serde::Deserialize;
use std::collections::BTreeMap;

/// JSON contract the LLM must return.
#[derive(Deserialize)]
struct DigestJson {
    title: String,
    description: String,
    tags: Vec<String>,
    body: String,
}

pub struct DigestResult {
    pub page: Page,
    pub log_entry: String,
}

pub async fn digest(
    provider: &dyn LlmProvider,
    source_text: &str,
    resource: Option<&str>,
    note: Option<&str>,
) -> Result<DigestResult> {
    let system = "You write one OKF wiki page from a source. \
        Respond ONLY with JSON: {\"title\":..,\"description\":..,\"tags\":[..],\"body\":..}. \
        The body is Markdown beginning with a bold TL;DR line, then '## Key points'.";
    let user = format!(
        "SOURCE:\n{source_text}\n\nUSER NOTE: {}",
        note.unwrap_or("")
    );
    let raw = provider.complete(system, &user).await?;
    let parsed: DigestJson = serde_json::from_str(extract_json(&raw))
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
    Ok(DigestResult {
        log_entry: format!("Added page: {}", parsed.title),
        page,
    })
}

/// Pull a JSON object out of an LLM reply that may wrap it in ```fences``` or prose.
fn extract_json(raw: &str) -> &str {
    let s = raw.trim();
    if let Some(start) = s.find("```") {
        let after = &s[start + 3..];
        let after = after.strip_prefix("json").unwrap_or(after);
        let after = after.trim_start_matches(['\n', '\r', ' ']);
        if let Some(end) = after.find("```") {
            return after[..end].trim();
        }
    }
    first_json_object(s).unwrap_or(s)
}

/// Return the first balanced `{...}` span, ignoring braces inside JSON strings.
fn first_json_object(s: &str) -> Option<&str> {
    let bytes = s.as_bytes();
    let start = s.find('{')?;
    let mut depth = 0usize;
    let mut in_str = false;
    let mut escaped = false;
    for i in start..bytes.len() {
        let c = bytes[i];
        if in_str {
            match c {
                _ if escaped => escaped = false,
                b'\\' => escaped = true,
                b'"' => in_str = false,
                _ => {}
            }
        } else {
            match c {
                b'"' => in_str = true,
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(&s[start..=i]);
                    }
                }
                _ => {}
            }
        }
    }
    None
}

fn now_iso() -> String {
    use time::format_description::well_known::Rfc3339;
    use time::OffsetDateTime;
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::provider::fake::FakeProvider;
    #[tokio::test]
    async fn produces_concept_page_from_llm_json() {
        let reply = r#"{"title":"Vitamin D & Sleep","description":"d","tags":["sleep"],"body":"**TL;DR.** morning."}"#;
        let p = FakeProvider {
            reply: reply.into(),
        };
        let r = digest(&p, "some source", Some("https://x"), Some("winter"))
            .await
            .unwrap();
        assert_eq!(r.page.path, "concepts/vitamin-d-sleep.md");
        assert_eq!(r.page.frontmatter.title, Some("Vitamin D & Sleep".into()));
        assert_eq!(r.page.frontmatter.note, Some("winter".into()));
        assert!(r.page.body.contains("TL;DR"));
    }

    #[tokio::test]
    async fn errors_on_malformed_json() {
        let p = FakeProvider {
            reply: "not json".into(),
        };
        assert!(digest(&p, "some source", None, None).await.is_err());
    }

    #[tokio::test]
    async fn parses_json_wrapped_in_code_fence() {
        let reply = "Here you go:\n```json\n{\"title\":\"T\",\"description\":\"d\",\"tags\":[],\"body\":\"**TL;DR.** x\"}\n```";
        let p = FakeProvider {
            reply: reply.into(),
        };
        let r = digest(&p, "src", None, None).await.unwrap();
        assert_eq!(r.page.frontmatter.title, Some("T".into()));
    }

    #[tokio::test]
    async fn parses_json_with_surrounding_prose() {
        let reply = "Sure! {\"title\":\"P\",\"description\":\"d\",\"tags\":[\"a\"],\"body\":\"b\"} Hope that helps.";
        let p = FakeProvider {
            reply: reply.into(),
        };
        let r = digest(&p, "src", None, None).await.unwrap();
        assert_eq!(r.page.frontmatter.title, Some("P".into()));
    }

    #[test]
    fn extract_json_handles_braces_inside_strings() {
        let raw = "noise {\"body\":\"a } b\",\"x\":1} trailing";
        assert_eq!(extract_json(raw), "{\"body\":\"a } b\",\"x\":1}");
    }

    #[test]
    fn now_iso_is_rfc3339() {
        let ts = now_iso();
        // RFC-3339 looks like 2026-06-16T12:34:56...Z — 4-digit year then '-', 'T' at index 10.
        assert_eq!(ts.as_bytes()[4], b'-', "expected YYYY- prefix, got {ts}");
        assert_eq!(
            ts.as_bytes()[10],
            b'T',
            "expected date/time 'T' separator, got {ts}"
        );
        assert!(
            !ts.starts_with("unixtime"),
            "should not be the old placeholder, got {ts}"
        );
    }
}
