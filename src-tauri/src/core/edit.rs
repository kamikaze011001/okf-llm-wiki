use crate::core::page::Page;

/// Apply edited fields onto an existing page, preserving every frontmatter field
/// the editor does not expose (`type`, `description`, `resource`, `timestamp`, and any
/// flattened `extra` keys). The page path/slug is never changed here.
pub fn apply_page_edits(
    mut page: Page,
    title: Option<String>,
    tags: Vec<String>,
    note: Option<String>,
    body: String,
) -> Page {
    page.frontmatter.title = title;
    page.frontmatter.tags = tags;
    page.frontmatter.note = note;
    page.body = body;
    page
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::page::Frontmatter;
    use std::collections::BTreeMap;

    fn sample() -> Page {
        let mut extra = BTreeMap::new();
        extra.insert(
            "okf_version".to_string(),
            serde_yaml::Value::String("0.1".into()),
        );
        Page {
            path: "concepts/x.md".into(),
            frontmatter: Frontmatter {
                type_: "Concept".into(),
                title: Some("Old".into()),
                description: Some("desc".into()),
                tags: vec!["old".into()],
                resource: Some("https://src".into()),
                timestamp: Some("2026-01-01T00:00:00Z".into()),
                note: Some("old note".into()),
                extra,
            },
            body: "old body".into(),
        }
    }

    #[test]
    fn updates_exposed_fields() {
        let p = apply_page_edits(
            sample(),
            Some("New".into()),
            vec!["a".into(), "b".into()],
            Some("new note".into()),
            "new body".into(),
        );
        assert_eq!(p.frontmatter.title, Some("New".into()));
        assert_eq!(p.frontmatter.tags, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(p.frontmatter.note, Some("new note".into()));
        assert_eq!(p.body, "new body");
    }

    #[test]
    fn preserves_hidden_fields_and_path() {
        let p = apply_page_edits(sample(), Some("New".into()), vec![], None, "b".into());
        assert_eq!(p.path, "concepts/x.md");
        assert_eq!(p.frontmatter.type_, "Concept");
        assert_eq!(p.frontmatter.description, Some("desc".into()));
        assert_eq!(p.frontmatter.resource, Some("https://src".into()));
        assert_eq!(p.frontmatter.timestamp, Some("2026-01-01T00:00:00Z".into()));
        assert_eq!(
            p.frontmatter.extra.get("okf_version"),
            Some(&serde_yaml::Value::String("0.1".into()))
        );
    }
}
