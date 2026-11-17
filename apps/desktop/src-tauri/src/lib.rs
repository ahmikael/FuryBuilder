mod commands;
mod state;

/// Starts the Tauri application. All IPC handlers are thin adapters over
/// `world_application`; no world logic lives in this crate.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(state::AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::ping,
            commands::world_info,
            commands::load_demo_world,
            commands::render_snapshot,
            commands::render_chunks,
            commands::get_block
        ])
        .run(tauri::generate_context!())
        .expect("error while running FuryBuilder");
}
