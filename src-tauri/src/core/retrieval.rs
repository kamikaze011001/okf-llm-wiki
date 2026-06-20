const DIM: usize = 256;

/// Deterministic local embedding: hashes word tokens into a fixed vector.
/// Keeps v1 fully offline and provider-independent for search.
pub fn hash_embed(text: &str) -> Vec<f32> {
    let mut v = vec![0f32; DIM];
    for word in text.to_lowercase().split(|c: char| !c.is_alphanumeric()) {
        if word.is_empty() {
            continue;
        }
        let mut h: u64 = 1469598103934665603;
        for b in word.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(1099511628211);
        }
        v[(h as usize) % DIM] += 1.0;
    }
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in &mut v {
            *x /= norm;
        }
    }
    v
}

pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

#[derive(Clone)]
pub struct IndexEntry {
    pub path: String,
    pub vector: Vec<f32>,
    pub snippet: String,
}

pub fn search<'a>(query: &str, entries: &'a [IndexEntry], k: usize) -> Vec<&'a IndexEntry> {
    let q = hash_embed(query);
    let mut scored: Vec<(f32, &IndexEntry)> =
        entries.iter().map(|e| (cosine(&q, &e.vector), e)).collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    scored.into_iter().take(k).map(|(_, e)| e).collect()
}

/// Target maximum size (in characters) for a single chunk.
pub const MAX_CHUNK_CHARS: usize = 800;

/// Split a page body into retrieval chunks.
///
/// Paragraphs are delimited by blank lines. Markdown headings (`#`-prefixed lines)
/// also start a new paragraph. Paragraphs are greedily packed into chunks up to
/// `MAX_CHUNK_CHARS`; a paragraph is never split across chunks. A single paragraph
/// longer than the limit becomes its own (oversized) chunk. An empty/whitespace-only
/// body yields no chunks.
pub fn chunk_body(body: &str) -> Vec<String> {
    let paragraphs = split_paragraphs(body);
    let mut chunks: Vec<String> = Vec::new();
    let mut current = String::new();
    for para in paragraphs {
        if current.is_empty() {
            current = para;
        } else if current.chars().count() + 2 + para.chars().count() <= MAX_CHUNK_CHARS {
            current.push_str("\n\n");
            current.push_str(&para);
        } else {
            chunks.push(std::mem::take(&mut current));
            current = para;
        }
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

/// Group raw lines into trimmed paragraphs. Blank lines separate paragraphs; a
/// heading line (starts with `#`) forces a boundary before itself.
fn split_paragraphs(body: &str) -> Vec<String> {
    let mut paras: Vec<String> = Vec::new();
    let mut buf: Vec<&str> = Vec::new();
    let flush = |buf: &mut Vec<&str>, paras: &mut Vec<String>| {
        if !buf.is_empty() {
            let joined = buf.join("\n");
            let trimmed = joined.trim();
            if !trimmed.is_empty() {
                paras.push(trimmed.to_string());
            }
            buf.clear();
        }
    };
    for line in body.lines() {
        if line.trim().is_empty() {
            flush(&mut buf, &mut paras);
        } else if line.trim_start().starts_with('#') {
            flush(&mut buf, &mut paras);
            buf.push(line);
            flush(&mut buf, &mut paras);
        } else {
            buf.push(line);
        }
    }
    flush(&mut buf, &mut paras);
    paras
}

use crate::core::store::OkfStore;
use anyhow::Result;

pub fn build_index(store: &OkfStore) -> Result<Vec<IndexEntry>> {
    let mut entries = Vec::new();
    for path in store.list_pages()? {
        let page = store.read_page(&path)?;
        let text = format!(
            "{} {}",
            page.frontmatter.title.clone().unwrap_or_default(),
            page.body
        );
        let snippet: String = page.body.chars().take(160).collect();
        entries.push(IndexEntry {
            path,
            vector: hash_embed(&text),
            snippet,
        });
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn finds_most_relevant_page() {
        let entries = vec![
            IndexEntry {
                path: "a.md".into(),
                vector: hash_embed("vitamin d sleep melatonin"),
                snippet: "".into(),
            },
            IndexEntry {
                path: "b.md".into(),
                vector: hash_embed("rust tauri desktop app"),
                snippet: "".into(),
            },
        ];
        let hits = search("how does vitamin d affect sleep", &entries, 1);
        assert_eq!(hits[0].path, "a.md");
    }

    fn tmp() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let d = std::env::temp_dir().join(format!("okf-ret-{}-{}", std::process::id(), n));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn empty_body_yields_no_chunks() {
        assert!(chunk_body("").is_empty());
        assert!(chunk_body("   \n\n  ").is_empty());
    }

    #[test]
    fn short_body_is_one_chunk() {
        let chunks = chunk_body("para one\n\npara two");
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].contains("para one"));
        assert!(chunks[0].contains("para two"));
    }

    #[test]
    fn packs_paragraphs_up_to_limit_without_splitting() {
        let p = "x".repeat(500);
        let chunks = chunk_body(&format!("{p}\n\n{p}"));
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].chars().count(), 500);
        assert_eq!(chunks[1].chars().count(), 500);
    }

    #[test]
    fn oversized_paragraph_becomes_its_own_chunk() {
        let big = "y".repeat(2000);
        let chunks = chunk_body(&big);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].chars().count(), 2000);
    }

    #[test]
    fn headings_start_new_paragraph_boundaries() {
        let body = "intro text\n\n## Section\n\nbody text";
        let chunks = chunk_body(body);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].contains("## Section"));
    }

    #[test]
    fn builds_index_from_store() {
        use crate::core::page::{Frontmatter, Page};
        use std::collections::BTreeMap;

        let store = OkfStore::new(tmp());

        let page1 = Page {
            path: "concepts/vitamin-d-sleep.md".into(),
            frontmatter: Frontmatter {
                type_: "Concept".into(),
                title: Some("Vitamin D and Sleep".into()),
                description: None,
                tags: vec![],
                resource: None,
                timestamp: None,
                note: None,
                extra: BTreeMap::new(),
            },
            body: "Vitamin D affects melatonin and sleep quality.".into(),
        };

        let page2 = Page {
            path: "concepts/rust-tauri.md".into(),
            frontmatter: Frontmatter {
                type_: "Concept".into(),
                title: Some("Rust Tauri Desktop App".into()),
                description: None,
                tags: vec![],
                resource: None,
                timestamp: None,
                note: None,
                extra: BTreeMap::new(),
            },
            body: "Tauri is a framework for building desktop apps with Rust.".into(),
        };

        store.write_page(&page1).unwrap();
        store.write_page(&page2).unwrap();

        let index = build_index(&store).unwrap();
        assert_eq!(index.len(), 2);
    }
}
