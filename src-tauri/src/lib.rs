pub mod core;
mod state;
mod commands;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .manage(state::AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::get_settings, commands::set_settings, commands::list_pages,
            commands::submit_source, commands::ask_question
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
