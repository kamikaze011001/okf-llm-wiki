mod commands;
pub mod core;
mod state;

use crate::core::config::{ConfigStore, KeyringSecretStore};
use crate::state::{initial_index, initial_links, AppState};
use std::sync::Mutex;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let dir = app
                .path()
                .app_config_dir()
                .expect("resolving app config dir");
            let index_path = crate::core::index_store::index_path(&dir);
            let config = ConfigStore::new(dir, Box::new(KeyringSecretStore::new()));
            let settings = config.load();
            let index = initial_index(&index_path);
            let links = initial_links(&settings.wiki_path);
            app.manage(AppState {
                settings: Mutex::new(settings),
                index: Mutex::new(index),
                index_path,
                links: Mutex::new(links),
                config,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::set_settings,
            commands::list_pages,
            commands::get_page_view,
            commands::submit_source,
            commands::ask_question,
            commands::reindex,
            commands::update_page,
            commands::delete_page,
            commands::create_page,
            commands::get_graph,
            commands::list_openrouter_models
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
