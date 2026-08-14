//! Provider Relay desktop client crate.

pub mod autostart;
pub mod commands;
pub mod credential_store;
pub mod identity;
pub mod tray;

pub struct NativeState {
    pub identity: identity::WindowsIdentitySource,
    pub credentials: credential_store::WindowsCredentialStore,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(commands::bootstrap::native_state())
        .invoke_handler(tauri::generate_handler![commands::bootstrap::bootstrap])
        .setup(|app| {
            autostart::enable(app.handle()).map_err(std::io::Error::other)?;
            tray::install(app.handle())?;
            Ok(())
        })
        .on_window_event(tray::hide_on_close)
        .run(tauri::generate_context!())
        .expect("failed to run Provider Relay desktop client");
}
