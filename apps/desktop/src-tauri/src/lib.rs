mod commands;
mod local_bridge;
mod mcp_access;
mod mcp_activity;
mod state;

use std::path::PathBuf;

use tauri::Manager;

/// Resolves the application data directory. Desktop smoke tests run a debug build with an
/// explicit isolated directory so they cannot read or modify the user's real settings.
fn app_data_dir(app: &tauri::AppHandle) -> Result<PathBuf, Box<dyn std::error::Error>> {
    #[cfg(debug_assertions)]
    if let Some(value) = std::env::var_os("FURYBUILDER_APP_DATA_DIR") {
        let path = PathBuf::from(value);
        std::fs::create_dir_all(&path)?;
        return Ok(path);
    }
    Ok(app.path().app_data_dir()?)
}

/// Starts the Tauri application. All IPC handlers are thin adapters over
/// `world_application` and `project_store`; no world or storage logic lives in this crate.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = app_data_dir(app.handle())?;
            let projects_root = data_dir.join("projects");
            std::fs::create_dir_all(&projects_root)?;
            app.manage(state::AppState::new(projects_root));
            app.manage(asset_source::AssetSettingsStore::new(
                data_dir.join("assets.json"),
            ));
            app.manage(asset_source::AssetBundleService::new());
            app.manage(runtime_manager::RuntimeManager::new(
                data_dir.join("runtime.json"),
                data_dir.join("runtimes").join("java"),
            ));
            app.manage(runtime_manager::ServerManager::new(
                data_dir.join("server.json"),
                data_dir.join("servers"),
            ));
            match local_bridge::start(app.handle().clone(), data_dir.clone()) {
                Ok(runtime) => {
                    app.manage(runtime);
                }
                Err(error) => {
                    // The editor stays usable; the MCP bridge reports that the desktop
                    // endpoint is unavailable instead of pretending to be connected.
                    eprintln!("FuryBuilder MCP bridge endpoint unavailable: {error}");
                }
            }
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
            commands::build_dsl_preview,
            commands::apply_operation,
            commands::undo_operation,
            commands::redo_operation,
            commands::operation_history,
            commands::history_state,
            commands::read_region,
            commands::get_block,
            commands::get_biome,
            commands::find_structure_sites,
            commands::workspace_policy,
            commands::workspace_set_policy,
            commands::scene_objects,
            commands::scene_inspect,
            commands::scene_create,
            commands::scene_delete,
            commands::parametric_list,
            commands::parametric_inspect,
            commands::parametric_create,
            commands::parametric_update,
            commands::parametric_bake,
            commands::parametric_rollback,
            commands::template_list,
            commands::template_save_selection,
            commands::template_import,
            commands::template_delete,
            commands::template_preview,
            commands::template_insert,
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
            commands::world_import_preview,
            commands::project_import_world,
            commands::project_import_generated_world,
            commands::project_verify_vanilla_parity,
            commands::structure_import_preview,
            commands::structure_import_project,
            commands::structure_insert,
            commands::structure_export_preview,
            commands::structure_export,
            commands::assets_discover,
            commands::assets_inspect_path,
            commands::assets_saved_source,
            commands::assets_save_source,
            commands::assets_clear_source,
            commands::assets_save_pack_stack,
            commands::assets_clear_pack_stack,
            commands::assets_render_bundle,
            commands::assets_clear_cache,
            commands::runtime_discover,
            commands::runtime_saved,
            commands::runtime_select_java,
            commands::runtime_clear_selection,
            commands::runtime_temurin_latest,
            commands::runtime_install_temurin,
            commands::runtime_managed_list,
            commands::runtime_clear_managed,
            commands::server_discover,
            commands::server_saved,
            commands::server_select_jar,
            commands::server_clear_selection,
            commands::server_release,
            commands::server_install,
            commands::server_managed_list,
            commands::server_clear_managed,
            commands::server_accept_eula,
            commands::server_clear_eula,
            commands::server_start_generation,
            commands::server_generation_status,
            commands::server_cancel_generation,
            commands::server_generation_runs,
            commands::server_clear_generation_runs,
            commands::server_inspect_datapacks,
            commands::vanilla_setup_check,
            local_bridge::renderer_ready,
            local_bridge::set_mcp_access,
            local_bridge::mcp_access_status,
            local_bridge::mcp_activity,
            local_bridge::submit_mcp_confirmation,
            local_bridge::submit_renderer_response
        ])
        .build(tauri::generate_context!())
        .expect("error while building FuryBuilder")
        .run(|app, event| {
            if matches!(event, tauri::RunEvent::Exit) {
                if let Some(runtime) = app.try_state::<local_bridge::LocalBridgeRuntime>() {
                    runtime.cleanup();
                }
            }
        });
}
