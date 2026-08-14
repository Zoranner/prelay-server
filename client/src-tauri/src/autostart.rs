use tauri::AppHandle;
use tauri_plugin_autostart::ManagerExt;

pub fn enable(app: &AppHandle) -> Result<(), String> {
    let manager = app.autolaunch();
    if !manager
        .is_enabled()
        .map_err(|error| format!("unable to read autostart state: {error}"))?
    {
        manager
            .enable()
            .map_err(|error| format!("unable to enable autostart: {error}"))?;
    }
    Ok(())
}
