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
            commands::block_catalog,
            commands::world_info,
            commands::load_demo_world,
            commands::render_snapshot,
            commands::render_chunks,
            commands::preview_operation,
            commands::apply_operation,
            commands::undo_operation,
            commands::redo_operation,
            commands::operation_history,
            commands::history_state,
            commands::read_region,
            commands::get_block
        ])
        .run(tauri::generate_context!())
        .expect("error while running FuryBuilder");
}
