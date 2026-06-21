use crate::core::page::{Frontmatter, Page};
use crate::core::slug::slugify;
use std::collections::BTreeMap;

/// Build a minimal "stub" page for a concept that does not exist yet.
///
/// The path is `concepts/{slugify(title)}.md`, so the slug matches what a `[[title]]`
/// red-link resolves against — creating the stub turns that link blue. The body is a
/// visible placeholder the user replaces via the edit flow; no LLM is involved.
pub fn new_stub_page(title: &str) -> Page {
    let slug = slugify(title);
    Page {
        path: format!("concepts/{slug}.md"),
        frontmatter: Frontmatter {
            type_: "Concept".into(),
            title: Some(title.to_string()),
            description: None,
            tags: vec![],
            resource: None,
            timestamp: Some(crate::core::clock::now_iso()),
            note: None,
            extra: BTreeMap::new(),
        },
        body: "_Stub — fill this in._".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_stub_with_slugified_path_and_concept_frontmatter() {
        let p = new_stub_page("Vitamin D & Sleep");
        assert_eq!(p.path, "concepts/vitamin-d-sleep.md");
        assert_eq!(p.frontmatter.type_, "Concept");
        assert_eq!(p.frontmatter.title, Some("Vitamin D & Sleep".into()));
        assert!(p.frontmatter.tags.is_empty());
        assert_eq!(p.frontmatter.description, None);
        assert_eq!(p.frontmatter.resource, None);
        assert_eq!(p.frontmatter.note, None);
        assert_eq!(p.body, "_Stub — fill this in._");
    }

    #[test]
    fn stub_has_rfc3339_timestamp() {
        let p = new_stub_page("Alpha");
        let ts = p.frontmatter.timestamp.unwrap();
        // RFC-3339: 4-digit year then '-', 'T' separator at index 10.
        assert_eq!(ts.as_bytes()[4], b'-', "expected YYYY- prefix, got {ts}");
        assert_eq!(ts.as_bytes()[10], b'T', "expected 'T' separator, got {ts}");
    }
}
