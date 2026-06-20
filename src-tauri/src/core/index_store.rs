use crate::core::embed::Embedder;
use crate::core::retrieval::chunk_body;
use crate::core::store::OkfStore;
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

/// Rebuild the persisted index from the store, reusing unchanged pages when the
/// embedder is unchanged.
///
/// For each page: compute `content_hash(title, body)`. If `prev` was built by the
/// same embedder and holds a matching hash for this path, reuse its chunks verbatim
/// (no embedding). Otherwise chunk the body (`chunk_body`); an empty body falls back
/// to a single title-only chunk. Each chunk is embedded as `"{title}\n\n{chunk}"`,
/// but the RAW chunk text is what we store. Pages absent from the store are dropped.
pub async fn rebuild_index(
    store: &OkfStore,
    embedder: &dyn Embedder,
    prev: &PersistedIndex,
) -> Result<PersistedIndex> {
    let reuse = prev.embedder_id == embedder.id();
    let mut pages: BTreeMap<String, PageEntry> = BTreeMap::new();

    for path in store.list_pages()? {
        let page = store.read_page(&path)?;
        let title = page.frontmatter.title.clone().unwrap_or_default();
        let hash = content_hash(&title, &page.body);

        if reuse {
            if let Some(existing) = prev.pages.get(&path) {
                if existing.content_hash == hash {
                    pages.insert(path.clone(), existing.clone());
                    continue;
                }
            }
        }

        let mut texts = chunk_body(&page.body);
        if texts.is_empty() {
            texts.push(title.clone());
        }
        let mut chunks = Vec::with_capacity(texts.len());
        for (chunk_id, text) in texts.into_iter().enumerate() {
            let embed_input = format!("{title}\n\n{text}");
            let vector = embedder.embed(&embed_input).await?;
            chunks.push(Chunk {
                path: path.clone(),
                chunk_id,
                text,
                vector,
            });
        }
        pages.insert(
            path.clone(),
            PageEntry {
                content_hash: hash,
                chunks,
            },
        );
    }

    Ok(PersistedIndex {
        embedder_id: embedder.id(),
        pages,
    })
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

    // ── rebuild_index tests ────────────────────────────────────────────────
    use crate::core::embed::Embedder;
    use crate::core::page::{Frontmatter, Page};
    use crate::core::store::OkfStore;
    use async_trait::async_trait;
    use std::collections::BTreeMap as TestBTreeMap;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
    use std::sync::Arc;

    struct CountingEmbedder {
        id: String,
        calls: Arc<AtomicUsize>,
    }
    #[async_trait]
    impl Embedder for CountingEmbedder {
        async fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>> {
            self.calls.fetch_add(1, AtomicOrdering::SeqCst);
            let n = text.chars().count() as f32;
            Ok(vec![n, n / 2.0])
        }
        fn id(&self) -> String {
            self.id.clone()
        }
    }

    fn write_page(store: &OkfStore, path: &str, title: &str, body: &str) {
        store
            .write_page(&Page {
                path: path.into(),
                frontmatter: Frontmatter {
                    type_: "Concept".into(),
                    title: Some(title.into()),
                    description: None,
                    tags: vec![],
                    resource: None,
                    timestamp: None,
                    note: None,
                    extra: TestBTreeMap::new(),
                },
                body: body.into(),
            })
            .unwrap();
    }

    #[tokio::test]
    async fn rebuild_embeds_each_chunk_of_each_page() {
        let dir = idx_tmp();
        let store = OkfStore::new(&dir);
        write_page(&store, "concepts/a.md", "Alpha", "one\n\ntwo");
        let calls = Arc::new(AtomicUsize::new(0));
        let e = CountingEmbedder {
            id: "count".into(),
            calls: calls.clone(),
        };
        let idx = rebuild_index(&store, &e, &PersistedIndex::default())
            .await
            .unwrap();
        assert_eq!(idx.embedder_id, "count");
        assert_eq!(idx.pages.len(), 1);
        let entry = idx.pages.get("concepts/a.md").unwrap();
        assert_eq!(entry.chunks.len(), 1);
        assert!(calls.load(AtomicOrdering::SeqCst) >= 1);
    }

    #[tokio::test]
    async fn rebuild_reuses_unchanged_pages_when_embedder_matches() {
        let dir = idx_tmp();
        let store = OkfStore::new(&dir);
        write_page(&store, "concepts/a.md", "Alpha", "body text");
        let calls = Arc::new(AtomicUsize::new(0));
        let e = CountingEmbedder {
            id: "count".into(),
            calls: calls.clone(),
        };
        let first = rebuild_index(&store, &e, &PersistedIndex::default())
            .await
            .unwrap();
        let after_first = calls.load(AtomicOrdering::SeqCst);
        let second = rebuild_index(&store, &e, &first).await.unwrap();
        assert_eq!(calls.load(AtomicOrdering::SeqCst), after_first);
        assert_eq!(first, second);
    }

    #[tokio::test]
    async fn rebuild_full_when_embedder_id_changes() {
        let dir = idx_tmp();
        let store = OkfStore::new(&dir);
        write_page(&store, "concepts/a.md", "Alpha", "body text");
        let calls = Arc::new(AtomicUsize::new(0));
        let e1 = CountingEmbedder {
            id: "v1".into(),
            calls: calls.clone(),
        };
        let first = rebuild_index(&store, &e1, &PersistedIndex::default())
            .await
            .unwrap();
        let baseline = calls.load(AtomicOrdering::SeqCst);
        let e2 = CountingEmbedder {
            id: "v2".into(),
            calls: calls.clone(),
        };
        let _ = rebuild_index(&store, &e2, &first).await.unwrap();
        assert!(calls.load(AtomicOrdering::SeqCst) > baseline);
    }

    #[tokio::test]
    async fn empty_body_page_embeds_the_title() {
        let dir = idx_tmp();
        let store = OkfStore::new(&dir);
        write_page(&store, "concepts/e.md", "OnlyTitle", "");
        let calls = Arc::new(AtomicUsize::new(0));
        let e = CountingEmbedder {
            id: "count".into(),
            calls: calls.clone(),
        };
        let idx = rebuild_index(&store, &e, &PersistedIndex::default())
            .await
            .unwrap();
        let entry = idx.pages.get("concepts/e.md").unwrap();
        assert_eq!(entry.chunks.len(), 1);
        assert_eq!(entry.chunks[0].text, "OnlyTitle");
    }

    #[tokio::test]
    async fn rebuild_drops_removed_pages() {
        let dir = idx_tmp();
        let store = OkfStore::new(&dir);
        write_page(&store, "concepts/a.md", "Alpha", "a body");
        write_page(&store, "concepts/b.md", "Beta", "b body");
        let calls = Arc::new(AtomicUsize::new(0));
        let e = CountingEmbedder {
            id: "count".into(),
            calls: calls.clone(),
        };
        let first = rebuild_index(&store, &e, &PersistedIndex::default())
            .await
            .unwrap();
        assert_eq!(first.pages.len(), 2);
        std::fs::remove_file(dir.join("concepts/b.md")).unwrap();
        let second = rebuild_index(&store, &e, &first).await.unwrap();
        assert_eq!(second.pages.len(), 1);
        assert!(second.pages.contains_key("concepts/a.md"));
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
