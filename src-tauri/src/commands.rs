use crate::core::{
    ask::ask,
    digest::digest,
    fetch::fetch_clean,
    retrieval::build_index,
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

fn store(state: &State<AppState>) -> OkfStore {
    OkfStore::new(state.settings.lock().unwrap().wiki_path.clone())
}

#[tauri::command]
pub fn get_settings(state: State<AppState>) -> Settings {
    state.settings.lock().unwrap().clone()
}

#[tauri::command]
pub fn set_settings(state: State<AppState>, settings: Settings) -> Result<(), String> {
    state.config.save(&settings).map_err(|e| e.to_string())?;
    *state.index.lock().unwrap() = crate::state::initial_index(&settings.wiki_path);
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
    let r = digest(
        provider.as_ref(),
        &clean,
        resource.as_deref(),
        note.as_deref(),
    )
    .await
    .map_err(|e| e.to_string())?;
    let s = OkfStore::new(settings.wiki_path.clone());
    s.write_page(&r.page).map_err(|e| e.to_string())?;
    s.append_log(&r.log_entry).map_err(|e| e.to_string())?;
    *state.index.lock().unwrap() = build_index(&s).map_err(|e| e.to_string())?;
    Ok(PageDto {
        path: r.page.path,
        title: r.page.frontmatter.title.unwrap_or_default(),
        body: r.page.body,
        tags: r.page.frontmatter.tags,
        note: r.page.frontmatter.note,
        resource: r.page.frontmatter.resource,
    })
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
    let index = state.index.lock().unwrap().clone();
    let a = ask(provider.as_ref(), &question, &index)
        .await
        .map_err(|e| e.to_string())?;
    Ok(AnswerDto {
        text: a.text,
        citations: a.citations,
    })
}
