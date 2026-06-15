use crate::core::page::{Page, Frontmatter};
use std::path::PathBuf;
use anyhow::{Result, Context};

pub struct OkfStore {
    root: PathBuf,
}

impl OkfStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        OkfStore { root: root.into() }
    }

    /// Write a page to disk atomically (temp file + rename).
    pub fn write_page(&self, page: &Page) -> Result<()> {
        let dest = self.root.join(&page.path);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating dirs for {}", dest.display()))?;
        }

        let yaml = serde_yaml::to_string(&page.frontmatter)
            .context("serializing frontmatter")?;
        let content = format!("---\n{}---\n\n{}\n", yaml, page.body);

        // Atomic write: write to a temp file next to the destination, then rename.
        let tmp_path = dest.with_extension("md.tmp");
        std::fs::write(&tmp_path, &content)
            .with_context(|| format!("writing temp file {}", tmp_path.display()))?;
        std::fs::rename(&tmp_path, &dest)
            .with_context(|| format!("renaming {} -> {}", tmp_path.display(), dest.display()))?;

        Ok(())
    }

    /// Read a page from disk. Parses the YAML frontmatter fence manually.
    pub fn read_page(&self, rel: &str) -> Result<Page> {
        let path = self.root.join(rel);
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;

        let (fm, body) = parse_frontmatter(&raw, rel)?;

        Ok(Page {
            path: rel.to_string(),
            frontmatter: fm,
            body,
        })
    }

    /// Recursively list *.md files relative to root, excluding index.md and log.md, sorted.
    pub fn list_pages(&self) -> Result<Vec<String>> {
        let mut pages = Vec::new();
        collect_md_files(&self.root, &self.root, &mut pages)?;
        pages.sort();
        Ok(pages)
    }

    /// Append `- {entry}\n` to {root}/log.md, creating the file if missing.
    pub fn append_log(&self, entry: &str) -> Result<()> {
        use std::io::Write;
        let log_path = self.root.join("log.md");
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .with_context(|| format!("opening log.md at {}", log_path.display()))?;
        writeln!(file, "- {}", entry)
            .context("writing to log.md")?;
        Ok(())
    }
}

/// Parse a `---\n...\n---\n` frontmatter fence using serde_yaml directly.
/// Returns (Frontmatter, body_string).
fn parse_frontmatter(raw: &str, rel: &str) -> Result<(Frontmatter, String)> {
    // Expect the file to start with "---\n"
    let without_leading = raw
        .strip_prefix("---\n")
        .with_context(|| format!("missing opening --- in {rel}"))?;

    // Find the closing "---" on its own line
    let close = without_leading
        .find("\n---\n")
        .with_context(|| format!("missing closing --- in {rel}"))?;

    let yaml_section = &without_leading[..close];
    // Content after the closing "---\n", skip optional blank line
    let after_close = &without_leading[close + 5..]; // skip "\n---\n"
    let body = after_close.strip_prefix('\n').unwrap_or(after_close).to_string();
    // Remove trailing newline that write_page appends
    let body = body.strip_suffix('\n').unwrap_or(&body).to_string();

    let fm: Frontmatter = serde_yaml::from_str(yaml_section)
        .with_context(|| format!("deserializing frontmatter in {rel}"))?;

    Ok((fm, body))
}

/// Recursively collect *.md files, skipping index.md and log.md.
fn collect_md_files(base: &std::path::Path, dir: &std::path::Path, out: &mut Vec<String>) -> Result<()> {
    for entry in std::fs::read_dir(dir)
        .with_context(|| format!("reading dir {}", dir.display()))?
    {
        let entry = entry?;
        let ft = entry.file_type()?;
        let path = entry.path();
        if ft.is_dir() {
            collect_md_files(base, &path, out)?;
        } else if ft.is_file() {
            if let Some(ext) = path.extension() {
                if ext == "md" {
                    let rel = path.strip_prefix(base)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"); // normalise on Windows too
                    let filename = path.file_name().unwrap().to_string_lossy();
                    if filename != "index.md" && filename != "log.md" {
                        out.push(rel);
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("okf-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn write_then_read_roundtrips() {
        let store = OkfStore::new(tmp());
        let page = Page {
            path: "concepts/vitamin-d-sleep.md".into(),
            frontmatter: Frontmatter {
                type_: "Concept".into(),
                title: Some("Vitamin D & Sleep".into()),
                description: None, tags: vec!["sleep".into()],
                resource: None, timestamp: None, note: Some("winter insomnia".into()),
                extra: BTreeMap::new(),
            },
            body: "**TL;DR.** Take it in the morning.".into(),
        };
        store.write_page(&page).unwrap();
        let read = store.read_page("concepts/vitamin-d-sleep.md").unwrap();
        assert_eq!(read.frontmatter.title, Some("Vitamin D & Sleep".into()));
        assert_eq!(read.frontmatter.tags, vec!["sleep".to_string()]);
        assert!(read.body.contains("morning"));
    }

    #[test]
    fn list_pages_excludes_index_and_log() {
        let root = tmp();
        let store = OkfStore::new(root.clone());
        let page = Page {
            path: "concepts/test-concept.md".into(),
            frontmatter: Frontmatter {
                type_: "Concept".into(),
                title: Some("Test Concept".into()),
                description: None, tags: vec![],
                resource: None, timestamp: None, note: None,
                extra: BTreeMap::new(),
            },
            body: "Body text.".into(),
        };
        store.write_page(&page).unwrap();
        // Write index.md and log.md directly
        std::fs::write(root.join("index.md"), "# Index\n").unwrap();
        std::fs::write(root.join("log.md"), "- entry\n").unwrap();
        let pages = store.list_pages().unwrap();
        assert!(pages.contains(&"concepts/test-concept.md".to_string()), "should include concept page");
        assert!(!pages.contains(&"index.md".to_string()), "should exclude index.md");
        assert!(!pages.contains(&"log.md".to_string()), "should exclude log.md");
    }

    #[test]
    fn append_log_creates_two_lines() {
        let root = tmp();
        let store = OkfStore::new(root.clone());
        store.append_log("first entry").unwrap();
        store.append_log("second entry").unwrap();
        let content = std::fs::read_to_string(root.join("log.md")).unwrap();
        assert!(content.contains("- first entry"), "log should contain first entry");
        assert!(content.contains("- second entry"), "log should contain second entry");
        assert_eq!(content.lines().count(), 2, "log should have exactly 2 lines");
    }

    #[test]
    fn extra_frontmatter_keys_survive_round_trip() {
        let store = OkfStore::new(tmp());
        let mut extra = BTreeMap::new();
        extra.insert("okf_version".to_string(), serde_yaml::Value::String("0.1".into()));
        let page = Page {
            path: "notes/extra-test.md".into(),
            frontmatter: Frontmatter {
                type_: "Note".into(),
                title: Some("Extra Test".into()),
                description: None, tags: vec![],
                resource: None, timestamp: None, note: None,
                extra,
            },
            body: "body with extras".into(),
        };
        store.write_page(&page).unwrap();
        let read = store.read_page("notes/extra-test.md").unwrap();
        assert_eq!(
            read.frontmatter.extra.get("okf_version"),
            Some(&serde_yaml::Value::String("0.1".into())),
            "extra key okf_version should survive round-trip"
        );
        // Second write->read to validate true double round-trip
        store.write_page(&read).unwrap();
        let read2 = store.read_page("notes/extra-test.md").unwrap();
        assert_eq!(read2.frontmatter.extra.get("okf_version"),
            Some(&serde_yaml::Value::String("0.1".into())));
    }
}
