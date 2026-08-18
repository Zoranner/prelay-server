use std::{fs, path::PathBuf};

fn source_file(relative_path: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative_path);
    fs::read_to_string(path).unwrap().replace("\r\n", "\n")
}

#[test]
fn registers_the_bootstrap_command_without_a_legacy_alias() {
    let library = source_file("src/lib.rs");
    let command = source_file("src/commands/bootstrap.rs");

    assert!(library.contains("commands::bootstrap::bootstrap"));
    assert!(!library.contains("bootstrap_client"));
    assert!(command.contains("#[tauri::command]\npub async fn bootstrap("));
    assert!(!command.contains("bootstrap_client"));
}

#[test]
fn bootstrap_does_not_access_the_credential_store_directly() {
    let command = source_file("src/commands/bootstrap.rs");

    assert!(!command.contains("credential_store"));
    assert!(!command.contains("CredentialStore"));
    assert!(command.contains("api_client::{ApiClient, ClientError}"));
    assert!(command.contains("has_stored_credential"));
}
