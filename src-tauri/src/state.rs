use crate::core::config::ConfigStore;
use crate::core::index_store::{self, PersistedIndex};
use crate::core::links::{build_link_graph, LinkGraph};
use crate::core::settings::Settings;
use crate::core::store::OkfStore;
use std::path::Path;
use std::sync::Mutex;

pub struct AppState {
    pub settings: Mutex<Settings>,
    pub index: Mutex<PersistedIndex>,
    /// Where `index.json` lives (app-config dir). Needed to persist after rebuilds.
    pub index_path: std::path::PathBuf,
    pub links: Mutex<LinkGraph>,
    pub config: ConfigStore,
}

/// Load the persisted retrieval index from disk. NEVER embeds and NEVER hits the
/// network — a missing/corrupt file yields an empty index so the app always launches.
pub fn initial_index(index_path: &Path) -> PersistedIndex {
    index_store::load(index_path)
}

/// Build the link graph from a wiki path, returning empty for an unset path
/// or any read failure (mirrors `initial_index`'s fail-soft behavior).
pub fn initial_links(wiki_path: &str) -> LinkGraph {
    if wiki_path.is_empty() {
        return LinkGraph::default();
    }
    build_link_graph(&OkfStore::new(wiki_path)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::page::{Frontmatter, Page};
    use crate::core::store::OkfStore;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn tmp() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let d = std::env::temp_dir().join(format!("okf-state-{}-{}", std::process::id(), n));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn empty_wiki_path_yields_empty_link_graph() {
        let g = initial_links("");
        assert!(!g.exists("anything"));
    }

    #[test]
    fn populated_wiki_path_builds_link_graph() {
        let dir = tmp();
        let store = OkfStore::new(dir.clone());
        store
            .write_page(&Page {
                path: "concepts/alpha.md".into(),
                frontmatter: Frontmatter {
                    type_: "Concept".into(),
                    title: Some("Alpha".into()),
                    description: None,
                    tags: vec![],
                    resource: None,
                    timestamp: None,
                    note: None,
                    extra: BTreeMap::new(),
                },
                body: "body".into(),
            })
            .unwrap();
        let g = initial_links(dir.to_str().unwrap());
        assert!(g.exists("alpha"));
    }

    #[test]
    fn missing_index_file_yields_empty_index() {
        let dir = tmp();
        let idx = initial_index(&crate::core::index_store::index_path(&dir));
        assert!(idx.pages.is_empty());
    }

    #[test]
    fn loads_persisted_index_from_disk() {
        use crate::core::index_store::{index_path, save, Chunk, PageEntry, PersistedIndex};
        let dir = tmp();
        let path = index_path(&dir);
        let mut idx = PersistedIndex {
            embedder_id: "hash-fnv-256".into(),
            ..Default::default()
        };
        idx.pages.insert(
            "concepts/x.md".into(),
            PageEntry {
                content_hash: 1,
                chunks: vec![Chunk {
                    path: "concepts/x.md".into(),
                    chunk_id: 0,
                    text: "x".into(),
                    vector: vec![0.0],
                }],
            },
        );
        save(&path, &idx).unwrap();
        assert_eq!(initial_index(&path).pages.len(), 1);
    }
}
