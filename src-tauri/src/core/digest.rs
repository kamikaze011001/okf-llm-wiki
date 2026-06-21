use crate::core::links::{validate_links, ConceptRef};
use crate::core::page::{Frontmatter, Page};
use crate::core::provider::LlmProvider;
use crate::core::slug::slugify;
use anyhow::{anyhow, Result};
use serde::Deserialize;
use std::collections::{BTreeMap, HashSet};

/// JSON contract the LLM must return.
#[derive(Deserialize, Debug)]
struct DigestJson {
    title: String,
    description: String,
    tags: Vec<String>,
    body: String,
}

/// Why a raw LLM reply could not be turned into a usable digest.
#[derive(Debug)]
enum DigestFailure {
    /// The reply did not contain parseable digest JSON; carries the parser error.
    Unparseable(String),
    /// Parsed, but a required field was blank after trimming.
    EmptyField(&'static str),
}

impl std::fmt::Display for DigestFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DigestFailure::Unparseable(e) => write!(f, "reply was not valid digest JSON: {e}"),
            DigestFailure::EmptyField(field) => write!(f, "the \"{field}\" field was empty"),
        }
    }
}

/// Parse + validate a raw LLM reply into a `DigestJson`, or report why it failed.
/// Reuses `extract_json` to peel ```fences```/prose, then requires non-empty
/// `title` and `body` after trimming.
fn evaluate(raw: &str) -> std::result::Result<DigestJson, DigestFailure> {
    let parsed: DigestJson = serde_json::from_str(extract_json(raw))
        .map_err(|e| DigestFailure::Unparseable(e.to_string()))?;
    if parsed.title.trim().is_empty() {
        return Err(DigestFailure::EmptyField("title"));
    }
    if parsed.body.trim().is_empty() {
        return Err(DigestFailure::EmptyField("body"));
    }
    Ok(parsed)
}

pub struct DigestResult {
    pub page: Page,
    pub log_entry: String,
}

/// Build the system prompt, adding an allow-list + linking instruction when concepts exist.
fn build_system_prompt(existing: &[ConceptRef]) -> String {
    let mut system = String::from(
        "You write one OKF wiki page from a source. \
        Respond ONLY with JSON: {\"title\":..,\"description\":..,\"tags\":[..],\"body\":..}. \
        The body is Markdown beginning with a bold TL;DR line, then '## Key points'.",
    );
    if !existing.is_empty() {
        system.push_str(
            " When the body mentions any of these existing concepts by name, wrap that \
            mention in [[double brackets]]. Use ONLY these exact titles: ",
        );
        let titles: Vec<&str> = existing.iter().map(|c| c.title.as_str()).collect();
        system.push_str(&titles.join(", "));
        system.push('.');
    }
    system
}

pub async fn digest(
    provider: &dyn LlmProvider,
    source_text: &str,
    resource: Option<&str>,
    note: Option<&str>,
    existing: &[ConceptRef],
) -> Result<DigestResult> {
    let system = build_system_prompt(existing);
    let user = format!(
        "SOURCE:\n{source_text}\n\nUSER NOTE: {}",
        note.unwrap_or("")
    );
    let raw = provider.complete(&system, &user).await?;
    let parsed: DigestJson = serde_json::from_str(extract_json(&raw))
        .map_err(|e| anyhow!("LLM did not return valid digest JSON: {e}; got: {raw}"))?;
    let slug = slugify(&parsed.title);
    // Keep only links to existing concepts or this page itself; drop hallucinated ones.
    let mut known: HashSet<String> = existing.iter().map(|c| c.slug.clone()).collect();
    known.insert(slug.clone());
    let body = validate_links(&parsed.body, &known);
    let page = Page {
        path: format!("concepts/{slug}.md"),
        frontmatter: Frontmatter {
            type_: "Concept".into(),
            title: Some(parsed.title.clone()),
            description: Some(parsed.description),
            tags: parsed.tags,
            resource: resource.map(|s| s.to_string()),
            timestamp: Some(crate::core::clock::now_iso()),
            note: note.map(|s| s.to_string()),
            extra: BTreeMap::new(),
        },
        body,
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
///
/// Known limitation: this scans from the *first* `{` in the input. If prose before the
/// JSON contains a stray `{`, the span starts there and will likely be unbalanced or
/// unparseable — `extract_json` then falls back to the raw string and `digest` surfaces
/// the existing "did not return valid digest JSON" error. It cannot panic: the byte scan
/// only matches ASCII `{`/`}`/`"`/`\`, which never collide with UTF-8 continuation bytes.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::links::ConceptRef;
    use crate::core::provider::fake::FakeProvider;

    #[test]
    fn system_prompt_lists_existing_titles_with_link_instruction() {
        let existing = vec![
            ConceptRef {
                slug: "alpha".into(),
                title: "Alpha".into(),
            },
            ConceptRef {
                slug: "beta".into(),
                title: "Beta".into(),
            },
        ];
        let p = build_system_prompt(&existing);
        assert!(p.contains("Alpha"));
        assert!(p.contains("Beta"));
        assert!(p.contains("[[double brackets]]"));
    }

    #[test]
    fn system_prompt_has_no_link_instruction_when_empty() {
        let p = build_system_prompt(&[]);
        assert!(!p.contains("[[double brackets]]"));
    }

    #[tokio::test]
    async fn drops_hallucinated_links_keeps_valid_ones() {
        let reply = r#"{"title":"Sleep Hygiene","description":"d","tags":[],"body":"See [[Vitamin D & Sleep]] and [[Nonexistent Concept]]."}"#;
        let p = FakeProvider {
            reply: reply.into(),
        };
        let existing = vec![ConceptRef {
            slug: "vitamin-d-sleep".into(),
            title: "Vitamin D & Sleep".into(),
        }];
        let r = digest(&p, "src", None, None, &existing).await.unwrap();
        assert!(r.page.body.contains("[[Vitamin D & Sleep]]"));
        assert!(!r.page.body.contains("[[Nonexistent Concept]]"));
        assert!(r.page.body.contains("Nonexistent Concept"));
    }
    #[tokio::test]
    async fn produces_concept_page_from_llm_json() {
        let reply = r#"{"title":"Vitamin D & Sleep","description":"d","tags":["sleep"],"body":"**TL;DR.** morning."}"#;
        let p = FakeProvider {
            reply: reply.into(),
        };
        let r = digest(&p, "some source", Some("https://x"), Some("winter"), &[])
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
        assert!(digest(&p, "some source", None, None, &[]).await.is_err());
    }

    #[tokio::test]
    async fn parses_json_wrapped_in_code_fence() {
        let reply = "Here you go:\n```json\n{\"title\":\"T\",\"description\":\"d\",\"tags\":[],\"body\":\"**TL;DR.** x\"}\n```";
        let p = FakeProvider {
            reply: reply.into(),
        };
        let r = digest(&p, "src", None, None, &[]).await.unwrap();
        assert_eq!(r.page.frontmatter.title, Some("T".into()));
    }

    #[tokio::test]
    async fn parses_json_with_surrounding_prose() {
        let reply = "Sure! {\"title\":\"P\",\"description\":\"d\",\"tags\":[\"a\"],\"body\":\"b\"} Hope that helps.";
        let p = FakeProvider {
            reply: reply.into(),
        };
        let r = digest(&p, "src", None, None, &[]).await.unwrap();
        assert_eq!(r.page.frontmatter.title, Some("P".into()));
    }

    #[test]
    fn extract_json_handles_braces_inside_strings() {
        let raw = "noise {\"body\":\"a } b\",\"x\":1} trailing";
        assert_eq!(extract_json(raw), "{\"body\":\"a } b\",\"x\":1}");
    }

    #[test]
    fn evaluate_accepts_valid_json() {
        let raw = r#"{"title":"T","description":"d","tags":["a"],"body":"b"}"#;
        let parsed = evaluate(raw).unwrap();
        assert_eq!(parsed.title, "T");
        assert_eq!(parsed.body, "b");
    }

    #[test]
    fn evaluate_rejects_unparseable() {
        let f = evaluate("not json").unwrap_err();
        assert!(matches!(f, DigestFailure::Unparseable(_)));
    }

    #[test]
    fn evaluate_rejects_blank_title() {
        let raw = r#"{"title":"   ","description":"d","tags":[],"body":"b"}"#;
        let f = evaluate(raw).unwrap_err();
        assert!(matches!(f, DigestFailure::EmptyField("title")));
    }

    #[test]
    fn evaluate_rejects_blank_body() {
        let raw = r#"{"title":"T","description":"d","tags":[],"body":""}"#;
        let f = evaluate(raw).unwrap_err();
        assert!(matches!(f, DigestFailure::EmptyField("body")));
    }

    #[test]
    fn digest_failure_display_mentions_field() {
        let f = DigestFailure::EmptyField("title");
        assert!(format!("{f}").contains("title"));
    }
}
