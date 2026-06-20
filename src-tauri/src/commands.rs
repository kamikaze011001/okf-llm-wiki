use crate::core::{
    ask::ask,
    digest::digest,
    embed::make_embedder,
    fetch::fetch_clean,
    index_store::{self, rebuild_index},
    links::{build_link_graph, segment_body, Segment},
    retrieval::search,
    settings::{make_provider, Settings},
    store::OkfStore,
};
use crate::state::AppState;
use serde::Serialize;
use tauri::State;

#[derive(Serialize)]
pub struct PageDto {
    pub path: String,
    pub title: String,
    pub body: String,
    pub tags: Vec<String>,
    pub note: Option<String>,
    pub resource: Option<String>,
}

#[derive(Serialize)]
pub struct SegmentDto {
    pub kind: String, // "text" | "link"
    pub text: String,
    pub target_path: Option<String>,
    pub exists: bool,
}

#[derive(Serialize)]
pub struct RefDto {
    pub path: String,
    pub title: String,
}

#[derive(Serialize)]
pub struct PageViewDto {
    pub path: String,
    pub title: String,
    pub tags: Vec<String>,
    pub note: Option<String>,
    pub resource: Option<String>,
    pub segments: Vec<SegmentDto>,
    pub backlinks: Vec<RefDto>,
}

fn store(state: &State<AppState>) -> OkfStore {
    OkfStore::new(state.settings.lock().unwrap().wiki_path.clone())
}

#[tauri::command]
pub fn get_settings(state: State<AppState>) -> Settings {
    state.settings.lock().unwrap().clone()
}

/// Persist settings, then rebuild + persist the retrieval index for the new config.
///
/// `config.save()` runs first so a persistence failure leaves in-memory state untouched.
/// The index is rebuilt with the newly-selected embedder (a changed embedder id forces a
/// full re-embed inside `rebuild_index`). No `MutexGuard` is held across the `.await`.
///
/// Partial-success caveat: if `config.save` succeeds but the rebuild then fails (e.g. the
/// newly-selected Ollama endpoint is unreachable), the new settings are already on disk
/// while in-memory `settings`/`index` stay on the old values, and the command returns
/// `Err`. The session keeps working with the previous embedder; the user can fix the
/// endpoint and re-save, or call `reindex`, to recover. Accepted trade-off for a
/// single-user local app.
#[tauri::command]
pub async fn set_settings(state: State<'_, AppState>, settings: Settings) -> Result<(), String> {
    state.config.save(&settings).map_err(|e| e.to_string())?;
    let embedder = make_embedder(&settings).map_err(|e| e.to_string())?;
    let store = OkfStore::new(settings.wiki_path.clone());
    let links = crate::state::initial_links(&settings.wiki_path);

    let prev = state.index.lock().unwrap().clone();
    let next = rebuild_index(&store, embedder.as_ref(), &prev)
        .await
        .map_err(|e| e.to_string())?;
    index_store::save(&state.index_path, &next).map_err(|e| e.to_string())?;

    *state.index.lock().unwrap() = next;
    *state.links.lock().unwrap() = links;
    *state.settings.lock().unwrap() = settings;
    Ok(())
}

#[tauri::command]
pub fn list_pages(state: State<AppState>) -> Result<Vec<PageDto>, String> {
    let s = store(&state);
    let mut out = Vec::new();
    for path in s.list_pages().map_err(|e| e.to_string())? {
        let p = s.read_page(&path).map_err(|e| e.to_string())?;
        out.push(PageDto {
            path: p.path,
            title: p.frontmatter.title.unwrap_or_default(),
            body: p.body,
            tags: p.frontmatter.tags,
            note: p.frontmatter.note,
            resource: p.frontmatter.resource,
        });
    }
    Ok(out)
}

/// Return a page with its body pre-segmented into text/link runs and its backlinks resolved.
#[tauri::command]
pub fn get_page_view(state: State<AppState>, path: String) -> Result<PageViewDto, String> {
    let s = store(&state);
    let page = s.read_page(&path).map_err(|e| e.to_string())?;
    let graph = state.links.lock().unwrap();
    let segments = segment_body(&page.body)
        .into_iter()
        .map(|seg| match seg {
            Segment::Text(t) => SegmentDto {
                kind: "text".into(),
                text: t,
                target_path: None,
                exists: false,
            },
            Segment::Link { text, target_slug } => {
                let target_path = graph.path_for(&target_slug).map(|s| s.to_string());
                let exists = target_path.is_some();
                SegmentDto {
                    kind: "link".into(),
                    text,
                    target_path,
                    exists,
                }
            }
        })
        .collect();
    let backlinks = graph
        .backlinks(&path)
        .into_iter()
        .map(|b| RefDto {
            path: b.path,
            title: b.title,
        })
        .collect();
    Ok(PageViewDto {
        path: page.path,
        title: page.frontmatter.title.unwrap_or_default(),
        tags: page.frontmatter.tags,
        note: page.frontmatter.note,
        resource: page.frontmatter.resource,
        segments,
        backlinks,
    })
}

#[tauri::command]
pub async fn submit_source(
    state: State<'_, AppState>,
    input: String,
    note: Option<String>,
) -> Result<PageDto, String> {
    let settings = state.settings.lock().unwrap().clone();
    let provider = make_provider(&settings).map_err(|e| e.to_string())?;
    let clean = fetch_clean(&input).await.map_err(|e| e.to_string())?;
    let resource = input.starts_with("http").then(|| input.clone());
    let existing = crate::core::links::concept_refs(&OkfStore::new(settings.wiki_path.clone()))
        .map_err(|e| e.to_string())?;
    let r = digest(
        provider.as_ref(),
        &clean,
        resource.as_deref(),
        note.as_deref(),
        &existing,
    )
    .await
    .map_err(|e| e.to_string())?;
    let s = OkfStore::new(settings.wiki_path.clone());
    s.write_page(&r.page).map_err(|e| e.to_string())?;
    s.append_log(&r.log_entry).map_err(|e| e.to_string())?;
    let embedder = make_embedder(&settings).map_err(|e| e.to_string())?;
    let prev = state.index.lock().unwrap().clone();
    let next = rebuild_index(&s, embedder.as_ref(), &prev)
        .await
        .map_err(|e| e.to_string())?;
    index_store::save(&state.index_path, &next).map_err(|e| e.to_string())?;
    *state.index.lock().unwrap() = next;
    *state.links.lock().unwrap() = build_link_graph(&s).map_err(|e| e.to_string())?;
    Ok(PageDto {
        path: r.page.path,
        title: r.page.frontmatter.title.unwrap_or_default(),
        body: r.page.body,
        tags: r.page.frontmatter.tags,
        note: r.page.frontmatter.note,
        resource: r.page.frontmatter.resource,
    })
}

/// Force a full rebuild of the retrieval index from scratch (ignores any reuse).
/// Passing a default `PersistedIndex` (empty `embedder_id`) guarantees an id mismatch,
/// so every page is re-embedded with the currently-selected embedder.
#[tauri::command]
pub async fn reindex(state: State<'_, AppState>) -> Result<(), String> {
    let settings = state.settings.lock().unwrap().clone();
    let embedder = make_embedder(&settings).map_err(|e| e.to_string())?;
    let store = OkfStore::new(settings.wiki_path.clone());

    let next = rebuild_index(
        &store,
        embedder.as_ref(),
        &index_store::PersistedIndex::default(),
    )
    .await
    .map_err(|e| e.to_string())?;
    index_store::save(&state.index_path, &next).map_err(|e| e.to_string())?;
    *state.index.lock().unwrap() = next;
    Ok(())
}

#[derive(Serialize)]
pub struct AnswerDto {
    pub text: String,
    pub citations: Vec<String>,
}

#[tauri::command]
pub async fn ask_question(
    state: State<'_, AppState>,
    question: String,
) -> Result<AnswerDto, String> {
    let settings = state.settings.lock().unwrap().clone();
    let provider = make_provider(&settings).map_err(|e| e.to_string())?;
    let embedder = make_embedder(&settings).map_err(|e| e.to_string())?;
    let index = state.index.lock().unwrap().clone();

    let hits = search(embedder.as_ref(), &question, &index, 4)
        .await
        .map_err(|e| e.to_string())?;
    let a = ask(provider.as_ref(), &question, &hits)
        .await
        .map_err(|e| e.to_string())?;
    Ok(AnswerDto {
        text: a.text,
        citations: a.citations,
    })
}
