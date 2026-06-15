use okf_llm_wiki_lib::core::{store::OkfStore, digest::digest, retrieval::build_index, ask::ask};
use okf_llm_wiki_lib::core::provider::fake::FakeProvider;

fn unique_tmp() -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static C: AtomicU64 = AtomicU64::new(0);
    let n = C.fetch_add(1, Ordering::SeqCst);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("okf-int-{}-{}-{}", std::process::id(), nanos, n));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[tokio::test]
async fn full_loop_digest_then_ask() {
    let dir = unique_tmp();
    let store = OkfStore::new(&dir);

    let reply = r#"{"title":"Vitamin D & Sleep","description":"d","tags":["sleep"],"body":"**TL;DR.** morning."}"#;
    let dp = FakeProvider { reply: reply.into() };
    let r = digest(&dp, "source about sleep", Some("https://x"), Some("note")).await.unwrap();
    store.write_page(&r.page).unwrap();
    store.append_log(&r.log_entry).unwrap();

    let index = build_index(&store).unwrap();
    let ap = FakeProvider { reply: "Morning dose [concepts/vitamin-d-sleep.md]".into() };
    let a = ask(&ap, "vitamin d sleep", &index).await.unwrap();
    assert!(a.text.contains("Morning"));
    assert_eq!(a.citations, vec!["concepts/vitamin-d-sleep.md".to_string()]);
}
