mod commands;
mod state;

use tauri::Manager;

/// Starts the Tauri application. All IPC handlers are thin adapters over
/// `world_application` and `project_store`; no world or storage logic lives in this crate.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let projects_root = data_dir.join("projects");
            std::fs::create_dir_all(&projects_root)?;
            app.manage(state::AppState::new(projects_root));
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                let state = window.app_handle().state::<state::AppState>();
                state::autosave_on_exit(&state);
            }
        })
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
            commands::get_block,
            commands::project_status,
            commands::project_list,
            commands::project_create,
            commands::project_open,
            commands::project_save,
            commands::project_autosave,
            commands::project_checkpoint,
            commands::project_checkpoints,
            commands::project_open_checkpoint,
            commands::project_close,
            commands::project_rename,
            commands::structure_import_preview,
            commands::structure_import_project,
            commands::structure_insert,
            commands::structure_export_preview,
            commands::structure_export
        ])
        .run(tauri::generate_context!())
        .expect("error while running FuryBuilder");
}
