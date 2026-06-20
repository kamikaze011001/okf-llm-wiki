use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One embedded passage of a page.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Chunk {
    pub path: String,
    pub chunk_id: usize,
    /// The raw chunk text (what we show / feed to the LLM as context).
    pub text: String,
    /// The embedding vector (any dimension; mismatches are skipped at search time).
    pub vector: Vec<f32>,
}

/// All chunks for one page plus the content hash used to detect changes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PageEntry {
    pub content_hash: u64,
    pub chunks: Vec<Chunk>,
}

/// The full persisted retrieval index. `embedder_id` records which embedder built it
/// so a change of embedder forces a full rebuild.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PersistedIndex {
    pub embedder_id: String,
    pub pages: BTreeMap<String, PageEntry>,
}

/// FNV-1a hash of `title \0 body`. The NUL separator keeps `("ab","c")` distinct
/// from `("a","bc")`.
pub fn content_hash(title: &str, body: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325; // FNV offset basis
    let mut mix = |bytes: &[u8]| {
        for &b in bytes {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3); // FNV prime
        }
    };
    mix(title.as_bytes());
    mix(&[0u8]);
    mix(body.as_bytes());
    h
}

/// Path of the index file inside the app's data directory.
pub fn index_path(data_dir: &Path) -> PathBuf {
    data_dir.join("index.json")
}

/// Load the persisted index. A missing or corrupt file yields an empty index — the
/// app must always launch, and a bad index just means "no hits until next rebuild".
pub fn load(path: &Path) -> PersistedIndex {
    match std::fs::read_to_string(path) {
        Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
        Err(_) => PersistedIndex::default(),
    }
}

/// Atomically write the index (temp file + rename), creating the parent dir if needed.
pub fn save(path: &Path, idx: &PersistedIndex) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating index dir {}", parent.display()))?;
    }
    let json = serde_json::to_string(idx).context("serializing index")?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json.as_bytes()).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path)
        .with_context(|| format!("renaming {} -> {}", tmp.display(), path.display()))?;
    Ok(())
}

/// Borrow every chunk across all pages, in `BTreeMap` (path-sorted) order.
pub fn flatten(idx: &PersistedIndex) -> Vec<&Chunk> {
    idx.pages.values().flat_map(|p| p.chunks.iter()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idx_tmp() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static C: AtomicU64 = AtomicU64::new(0);
        let n = C.fetch_add(1, Ordering::SeqCst);
        let d = std::env::temp_dir().join(format!("okf-idx-{}-{}", std::process::id(), n));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn content_hash_is_stable_and_field_sensitive() {
        assert_eq!(content_hash("T", "body"), content_hash("T", "body"));
        assert_ne!(content_hash("T", "body"), content_hash("T", "body2"));
        assert_ne!(content_hash("T", "body"), content_hash("T2", "body"));
        assert_ne!(content_hash("ab", "c"), content_hash("a", "bc"));
    }

    #[test]
    fn index_path_is_index_json_in_dir() {
        let p = index_path(std::path::Path::new("/some/dir"));
        assert!(p.ends_with("index.json"));
    }

    #[test]
    fn load_missing_returns_default() {
        let dir = idx_tmp();
        let idx = load(&index_path(&dir));
        assert_eq!(idx.embedder_id, "");
        assert!(idx.pages.is_empty());
    }

    #[test]
    fn load_corrupt_returns_default() {
        let dir = idx_tmp();
        let path = index_path(&dir);
        std::fs::write(&path, b"{not json").unwrap();
        let idx = load(&path);
        assert_eq!(idx, PersistedIndex::default());
    }

    #[test]
    fn save_then_load_roundtrips() {
        let dir = idx_tmp();
        let path = index_path(&dir);
        let mut idx = PersistedIndex {
            embedder_id: "hash-fnv-256".into(),
            ..Default::default()
        };
        idx.pages.insert(
            "concepts/a.md".into(),
            PageEntry {
                content_hash: 42,
                chunks: vec![Chunk {
                    path: "concepts/a.md".into(),
                    chunk_id: 0,
                    text: "hello".into(),
                    vector: vec![0.1, 0.2],
                }],
            },
        );
        save(&path, &idx).unwrap();
        assert_eq!(load(&path), idx);
    }

    #[test]
    fn flatten_collects_all_chunks_in_page_order() {
        let mut idx = PersistedIndex::default();
        idx.pages.insert(
            "b.md".into(),
            PageEntry {
                content_hash: 1,
                chunks: vec![Chunk {
                    path: "b.md".into(),
                    chunk_id: 0,
                    text: "b0".into(),
                    vector: vec![],
                }],
            },
        );
        idx.pages.insert(
            "a.md".into(),
            PageEntry {
                content_hash: 1,
                chunks: vec![
                    Chunk {
                        path: "a.md".into(),
                        chunk_id: 0,
                        text: "a0".into(),
                        vector: vec![],
                    },
                    Chunk {
                        path: "a.md".into(),
                        chunk_id: 1,
                        text: "a1".into(),
                        vector: vec![],
                    },
                ],
            },
        );
        let flat = flatten(&idx);
        let texts: Vec<&str> = flat.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(texts, vec!["a0", "a1", "b0"]);
    }
}
