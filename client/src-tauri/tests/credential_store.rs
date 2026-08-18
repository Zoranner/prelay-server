use std::{
    fs,
    sync::{Arc, Barrier},
    thread,
};

use provider_relay_client::credential_store::{
    CredentialRecord, CredentialStore, FileCredentialStore,
};
use tempfile::tempdir;
use tokio::sync::{oneshot, Notify};

#[test]
fn file_store_keeps_current_credential_until_pending_rotation_is_confirmed() {
    let directory = tempdir().unwrap();
    let store = FileCredentialStore::at(directory.path().join("device-credential.json"));

    store.save_initial("credential-old").unwrap();
    store.begin_rotation("credential-new").unwrap();
    assert_eq!(
        store.load().unwrap(),
        Some(CredentialRecord {
            current: "credential-old".into(),
            pending: Some("credential-new".into()),
        })
    );

    store.confirm_pending().unwrap();
    assert_eq!(store.load().unwrap().unwrap().current, "credential-new");
    assert!(store.load().unwrap().unwrap().pending.is_none());
}

#[test]
fn file_store_discards_pending_credential_without_replacing_current_credential() {
    let directory = tempdir().unwrap();
    let store = FileCredentialStore::at(directory.path().join("device-credential.json"));

    store.save_initial("credential-old").unwrap();
    store.begin_rotation("credential-new").unwrap();
    store.discard_pending().unwrap();

    assert_eq!(
        store.load().unwrap(),
        Some(CredentialRecord {
            current: "credential-old".into(),
            pending: None,
        })
    );
}

#[test]
fn file_store_rejects_a_second_rotation_while_a_credential_is_pending() {
    let directory = tempdir().unwrap();
    let store = FileCredentialStore::at(directory.path().join("device-credential.json"));

    store.save_initial("credential-old").unwrap();
    store.begin_rotation("credential-new").unwrap();
    let error = store
        .begin_rotation("credential-replacement")
        .expect_err("a pending credential must not be replaced");

    assert_eq!(error, "credential record already has a pending credential");
    assert_eq!(
        store.load().unwrap(),
        Some(CredentialRecord {
            current: "credential-old".into(),
            pending: Some("credential-new".into()),
        })
    );
}

#[test]
fn file_store_completes_the_expected_rotation_after_pending_is_discarded() {
    let directory = tempdir().unwrap();
    let store = FileCredentialStore::at(directory.path().join("device-credential.json"));

    store.save_initial("credential-old").unwrap();
    store.begin_rotation("credential-new").unwrap();
    store.discard_pending().unwrap();
    store.complete_rotation("credential-new").unwrap();

    assert_eq!(
        store.load().unwrap(),
        Some(CredentialRecord {
            current: "credential-new".into(),
            pending: None,
        })
    );
}

#[test]
fn file_store_lifecycle_lock_waits_until_the_other_store_releases_it() {
    tauri::async_runtime::block_on(async {
        let directory = tempdir().unwrap();
        let credential_path = directory.path().join("device-credential.json");
        let first = FileCredentialStore::at(&credential_path);
        let second = FileCredentialStore::at(&credential_path);
        let first_guard = first.acquire_lifecycle_lock().await.unwrap();
        let second_attempted = Arc::new(Notify::new());
        let (second_acquired, mut second_acquired_rx) = oneshot::channel();

        let second_task = tauri::async_runtime::spawn({
            let second_attempted = second_attempted.clone();
            async move {
                second_attempted.notify_one();
                let guard = second.acquire_lifecycle_lock().await.unwrap();
                second_acquired.send(()).unwrap();
                drop(guard);
            }
        });
        second_attempted.notified().await;
        tokio::task::yield_now().await;

        assert!(matches!(
            second_acquired_rx.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));
        drop(first_guard);
        second_task.await.unwrap();
        second_acquired_rx.await.unwrap();
    });
}

#[test]
fn file_store_writes_complete_json_without_leaving_temporary_files() {
    let directory = tempdir().unwrap();
    let credential_path = directory.path().join("device-credential.json");
    let store = FileCredentialStore::at(&credential_path);

    store.save_initial("credential-old").unwrap();
    store.begin_rotation("credential-new").unwrap();

    let record: CredentialRecord =
        serde_json::from_str(&fs::read_to_string(&credential_path).unwrap())
            .expect("credential file must contain a complete JSON record");
    assert_eq!(record.current, "credential-old");
    assert_eq!(record.pending.as_deref(), Some("credential-new"));

    let mut entries = fs::read_dir(directory.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    entries.sort();
    assert_eq!(
        entries,
        vec!["device-credential.json", "device-credential.json.lock"]
    );
}

#[test]
fn file_store_only_creates_the_first_credential_once_across_store_instances() {
    let directory = tempdir().unwrap();
    let credential_path = directory.path().join("device-credential.json");
    let first = Arc::new(FileCredentialStore::at(&credential_path));
    let second = Arc::new(FileCredentialStore::at(&credential_path));
    let start = Arc::new(Barrier::new(3));

    let threads = [(first, "credential-a"), (second, "credential-b")]
        .into_iter()
        .map(|(store, credential)| {
            let start = start.clone();
            thread::spawn(move || {
                start.wait();
                store.save_initial(credential).unwrap()
            })
        })
        .collect::<Vec<_>>();
    start.wait();

    let records = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(records[0], records[1]);
    assert!(matches!(
        records[0].current.as_str(),
        "credential-a" | "credential-b"
    ));
    assert_eq!(
        FileCredentialStore::at(credential_path).load().unwrap(),
        Some(records[0].clone())
    );
}
