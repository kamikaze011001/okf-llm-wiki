use crate::core::config::ConfigStore;
use crate::core::retrieval::{build_index, IndexEntry};
use crate::core::settings::Settings;
use crate::core::store::OkfStore;
use std::sync::Mutex;

pub struct AppState {
    pub settings: Mutex<Settings>,
    pub index: Mutex<Vec<IndexEntry>>,
    pub config: ConfigStore,
}

/// Build the retrieval index from a wiki path, returning empty for an unset path
/// or any read failure (the app should still launch).
pub fn initial_index(wiki_path: &str) -> Vec<IndexEntry> {
    if wiki_path.is_empty() {
        return Vec::new();
    }
    build_index(&OkfStore::new(wiki_path)).unwrap_or_default()
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
    fn empty_wiki_path_yields_empty_index() {
        assert!(initial_index("").is_empty());
    }

    #[test]
    fn populated_wiki_path_builds_index() {
        let dir = tmp();
        let store = OkfStore::new(dir.clone());
        store
            .write_page(&Page {
                path: "concepts/x.md".into(),
                frontmatter: Frontmatter {
                    type_: "Concept".into(),
                    title: Some("X".into()),
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
        assert_eq!(initial_index(dir.to_str().unwrap()).len(), 1);
    }
}
