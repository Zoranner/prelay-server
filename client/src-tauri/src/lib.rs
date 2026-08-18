//! Provider Relay desktop client crate.

use tauri::Manager;

pub mod api_client;
pub mod autostart;
pub mod commands;
pub mod credential_store;
pub mod identity;
pub mod tray;

pub struct NativeState {
    pub identity: identity::WindowsIdentitySource,
    pub credentials: credential_store::FileCredentialStore,
    pub registration_gate: api_client::RegistrationGate,
}

impl NativeState {
    pub fn for_app_data_dir(app_data_dir: std::path::PathBuf) -> Self {
        Self {
            identity: identity::WindowsIdentitySource,
            credentials: credential_store::FileCredentialStore::at(
                app_data_dir
                    .join("Provider Relay")
                    .join("device-credential.json"),
            ),
            registration_gate: api_client::RegistrationGate::default(),
        }
    }
}

impl Default for NativeState {
    fn default() -> Self {
        Self::for_app_data_dir(std::env::temp_dir())
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .invoke_handler(tauri::generate_handler![
            commands::bootstrap::bootstrap,
            commands::providers::providers_list,
            commands::providers::providers_save,
            commands::providers::providers_delete,
            commands::providers::providers_ping,
            commands::providers::providers_discover_models,
            commands::providers::providers_test_protocol,
            commands::interfaces::interfaces_list,
            commands::interfaces::interfaces_save,
            commands::interfaces::interfaces_delete,
            commands::interfaces::interfaces_regenerate_token,
            commands::stats::stats_overview,
            commands::stats::stats_requests,
            commands::stats::stats_models,
            commands::stats::stats_providers,
            commands::credential_rotate
        ])
        .setup(|app| {
            let app_data_dir = app.path().app_data_dir()?;
            app.manage(NativeState::for_app_data_dir(app_data_dir));
            autostart::enable(app.handle()).map_err(std::io::Error::other)?;
            tray::install(app.handle())?;
            Ok(())
        })
        .on_window_event(tray::hide_on_close)
        .run(tauri::generate_context!())
        .expect("failed to run Provider Relay desktop client");
}
