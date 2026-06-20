use okf_llm_wiki_lib::core::embed::HashEmbedder;
use okf_llm_wiki_lib::core::index_store::{rebuild_index, PersistedIndex};
use okf_llm_wiki_lib::core::links::{build_link_graph, concept_refs, BacklinkRef};
use okf_llm_wiki_lib::core::provider::fake::FakeProvider;
use okf_llm_wiki_lib::core::retrieval::search;
use okf_llm_wiki_lib::core::{ask::ask, digest::digest, store::OkfStore};

fn unique_tmp() -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static C: AtomicU64 = AtomicU64::new(0);
    let n = C.fetch_add(1, Ordering::SeqCst);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("okf-int-{}-{}-{}", std::process::id(), nanos, n));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[tokio::test]
async fn full_loop_digest_then_ask() {
    let dir = unique_tmp();
    let store = OkfStore::new(&dir);

    let reply = r#"{"title":"Vitamin D & Sleep","description":"d","tags":["sleep"],"body":"**TL;DR.** morning."}"#;
    let dp = FakeProvider {
        reply: reply.into(),
    };
    let r = digest(
        &dp,
        "source about sleep",
        Some("https://x"),
        Some("note"),
        &[],
    )
    .await
    .unwrap();
    store.write_page(&r.page).unwrap();
    store.append_log(&r.log_entry).unwrap();

    let embedder = HashEmbedder;
    let index = rebuild_index(&store, &embedder, &PersistedIndex::default())
        .await
        .unwrap();
    let hits = search(&embedder, "vitamin d sleep", &index, 4)
        .await
        .unwrap();
    let ap = FakeProvider {
        reply: "Morning dose [concepts/vitamin-d-sleep.md]".into(),
    };
    let a = ask(&ap, "vitamin d sleep", &hits).await.unwrap();
    assert!(a.text.contains("Morning"));
    assert_eq!(a.citations, vec!["concepts/vitamin-d-sleep.md".to_string()]);
}

#[tokio::test]
async fn backlinks_resolve_across_pages() {
    let dir = unique_tmp();
    let store = OkfStore::new(&dir);

    // Page A: an existing concept.
    let a_reply = r#"{"title":"Alpha","description":"d","tags":[],"body":"**TL;DR.** alpha."}"#;
    let a = digest(
        &FakeProvider {
            reply: a_reply.into(),
        },
        "src a",
        None,
        None,
        &[],
    )
    .await
    .unwrap();
    store.write_page(&a.page).unwrap();

    // Page B references Alpha by exact title; Alpha is in the allow-list.
    let existing = concept_refs(&store).unwrap();
    let b_reply =
        r#"{"title":"Beta","description":"d","tags":[],"body":"Beta builds on [[Alpha]]."}"#;
    let b = digest(
        &FakeProvider {
            reply: b_reply.into(),
        },
        "src b",
        None,
        None,
        &existing,
    )
    .await
    .unwrap();
    // The valid link to Alpha survives validation.
    assert!(b.page.body.contains("[[Alpha]]"));
    store.write_page(&b.page).unwrap();

    let graph = build_link_graph(&store).unwrap();
    assert_eq!(
        graph.backlinks("concepts/alpha.md"),
        vec![BacklinkRef {
            path: "concepts/beta.md".into(),
            title: "Beta".into()
        }]
    );
}

#[tokio::test]
async fn reindex_from_empty_prev_embeds_all_pages() {
    let dir = unique_tmp();
    let store = OkfStore::new(&dir);
    let r = digest(
        &FakeProvider {
            reply: r#"{"title":"Alpha","description":"d","tags":[],"body":"**TL;DR.** a."}"#.into(),
        },
        "src",
        None,
        None,
        &[],
    )
    .await
    .unwrap();
    store.write_page(&r.page).unwrap();

    // reindex == rebuild against a default (empty) index
    let embedder = HashEmbedder;
    let idx = rebuild_index(&store, &embedder, &PersistedIndex::default())
        .await
        .unwrap();
    assert_eq!(idx.pages.len(), 1);
    assert_eq!(idx.embedder_id, "hash-fnv-256");
}
