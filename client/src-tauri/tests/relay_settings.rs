use provider_relay_client::relay_settings::{FileRelaySettingsStore, RelaySettingsStore};
use tempfile::tempdir;

#[test]
fn relay_settings_persists_the_selected_management_url() {
    let directory = tempdir().expect("create settings directory");
    let store = FileRelaySettingsStore::at(directory.path().join("relay-settings.json"));

    assert_eq!(store.load().expect("load empty settings"), None);

    store
        .save("https://relay.example.test")
        .expect("save relay URL");

    assert_eq!(
        store.load().expect("load saved settings").as_deref(),
        Some("https://relay.example.test")
    );
}
