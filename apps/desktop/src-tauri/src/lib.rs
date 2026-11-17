mod commands;

/// Starts the Tauri application. All IPC handlers are thin adapters over
/// `world_application`; no world logic lives in this crate.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![commands::app_info, commands::ping])
        .run(tauri::generate_context!())
        .expect("error while running FuryBuilder");
}
