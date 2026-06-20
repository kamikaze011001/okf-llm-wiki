use okf_llm_wiki_lib::core::edit::apply_page_edits;
use okf_llm_wiki_lib::core::embed::HashEmbedder;
use okf_llm_wiki_lib::core::index_store::{rebuild_index, PersistedIndex};
use okf_llm_wiki_lib::core::links::{
    build_link_graph, concept_refs, segment_body, BacklinkRef, Segment,
};
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

#[tokio::test]
async fn edit_round_trip_preserves_hidden_frontmatter_and_updates_index() {
    let dir = unique_tmp();
    let store = OkfStore::new(&dir);

    // Seed a page with a resource + body, via digest (so frontmatter is realistic).
    let r = digest(
        &FakeProvider {
            reply: r#"{"title":"Alpha","description":"d","tags":["old"],"body":"**TL;DR.** old."}"#
                .into(),
        },
        "src",
        Some("https://src"),
        Some("orig note"),
        &[],
    )
    .await
    .unwrap();
    store.write_page(&r.page).unwrap();
    let path = r.page.path.clone();

    let embedder = HashEmbedder;
    let before = rebuild_index(&store, &embedder, &PersistedIndex::default())
        .await
        .unwrap();
    let hash_before = before.pages.get(&path).unwrap().content_hash;

    // Simulate update_page's core flow.
    let existing = store.read_page(&path).unwrap();
    let edited = apply_page_edits(
        existing,
        Some("Alpha".into()),
        vec!["new".into()],
        Some("orig note".into()),
        "**TL;DR.** brand new body.".into(),
    );
    store.write_page(&edited).unwrap();

    // Read back: edited fields applied, hidden frontmatter (resource) preserved.
    let read = store.read_page(&path).unwrap();
    assert_eq!(read.frontmatter.tags, vec!["new".to_string()]);
    assert!(read.body.contains("brand new body"));
    assert_eq!(read.frontmatter.resource, Some("https://src".into()));

    // The command appends an "edited <title>" line to log.md.
    let log_title = read.frontmatter.title.clone().unwrap_or_default();
    store.append_log(&format!("edited {log_title}")).unwrap();
    let log = std::fs::read_to_string(dir.join("log.md")).unwrap();
    assert!(log.contains(&format!("- edited {log_title}")));

    // Index reflects the changed body (content hash differs -> re-embedded).
    let after = rebuild_index(&store, &embedder, &before).await.unwrap();
    let hash_after = after.pages.get(&path).unwrap().content_hash;
    assert_ne!(hash_before, hash_after);
}

#[tokio::test]
async fn delete_drops_page_from_listing_and_index() {
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
    let path = r.page.path.clone();

    let embedder = HashEmbedder;
    let before = rebuild_index(&store, &embedder, &PersistedIndex::default())
        .await
        .unwrap();
    assert!(before.pages.contains_key(&path));

    store.delete_page(&path).unwrap();

    assert!(!store.list_pages().unwrap().contains(&path));
    // rebuild_index drops pages no longer on disk.
    let after = rebuild_index(&store, &embedder, &before).await.unwrap();
    assert!(!after.pages.contains_key(&path));

    // The command appends a "deleted <title>" line to log.md.
    store.append_log("deleted Alpha").unwrap();
    let log = std::fs::read_to_string(dir.join("log.md")).unwrap();
    assert!(log.contains("- deleted Alpha"));
}

#[tokio::test]
async fn link_to_deleted_page_renders_as_red_link() {
    let dir = unique_tmp();
    let store = OkfStore::new(&dir);

    // Alpha exists; Beta links to it.
    let a = digest(
        &FakeProvider {
            reply: r#"{"title":"Alpha","description":"d","tags":[],"body":"**TL;DR.** a."}"#.into(),
        },
        "src a",
        None,
        None,
        &[],
    )
    .await
    .unwrap();
    store.write_page(&a.page).unwrap();
    let existing = concept_refs(&store).unwrap();
    let b = digest(
        &FakeProvider {
            reply:
                r#"{"title":"Beta","description":"d","tags":[],"body":"Beta builds on [[Alpha]]."}"#
                    .into(),
        },
        "src b",
        None,
        None,
        &existing,
    )
    .await
    .unwrap();
    store.write_page(&b.page).unwrap();

    // Delete Alpha, rebuild the graph.
    store.delete_page("concepts/alpha.md").unwrap();
    let graph = build_link_graph(&store).unwrap();

    // Beta's [[Alpha]] link now resolves to nothing -> red-link (exists == false).
    let beta = store.read_page("concepts/beta.md").unwrap();
    let has_unresolved_link = segment_body(&beta.body).iter().any(|seg| {
        matches!(seg, Segment::Link { target_slug, .. } if graph.path_for(target_slug).is_none())
    });
    assert!(has_unresolved_link, "deleted target should be unresolved");
}
