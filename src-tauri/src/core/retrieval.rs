const DIM: usize = 256;

/// Deterministic local embedding: hashes word tokens into a fixed vector.
/// Keeps v1 fully offline and provider-independent for search.
pub fn hash_embed(text: &str) -> Vec<f32> {
    let mut v = vec![0f32; DIM];
    for word in text.to_lowercase().split(|c: char| !c.is_alphanumeric()) {
        if word.is_empty() { continue; }
        let mut h: u64 = 1469598103934665603;
        for b in word.bytes() { h ^= b as u64; h = h.wrapping_mul(1099511628211); }
        v[(h as usize) % DIM] += 1.0;
    }
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 { for x in &mut v { *x /= norm; } }
    v
}

pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

#[derive(Clone)]
pub struct IndexEntry { pub path: String, pub vector: Vec<f32>, pub snippet: String }

pub fn search<'a>(query: &str, entries: &'a [IndexEntry], k: usize) -> Vec<&'a IndexEntry> {
    let q = hash_embed(query);
    let mut scored: Vec<(f32, &IndexEntry)> =
        entries.iter().map(|e| (cosine(&q, &e.vector), e)).collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    scored.into_iter().take(k).map(|(_, e)| e).collect()
}

use crate::core::store::OkfStore;
use anyhow::Result;

pub fn build_index(store: &OkfStore) -> Result<Vec<IndexEntry>> {
    let mut entries = Vec::new();
    for path in store.list_pages()? {
        let page = store.read_page(&path)?;
        let text = format!("{} {}", page.frontmatter.title.clone().unwrap_or_default(), page.body);
        let snippet: String = page.body.chars().take(160).collect();
        entries.push(IndexEntry { path, vector: hash_embed(&text), snippet });
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn finds_most_relevant_page() {
        let entries = vec![
            IndexEntry { path: "a.md".into(), vector: hash_embed("vitamin d sleep melatonin"), snippet: "".into() },
            IndexEntry { path: "b.md".into(), vector: hash_embed("rust tauri desktop app"), snippet: "".into() },
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
    fn builds_index_from_store() {
        use crate::core::page::{Page, Frontmatter};
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
