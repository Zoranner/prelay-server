use std::fs;

use tempfile::tempdir;

use crate::store::recording_journal::{checkpoint::JournalCheckpoint, JournalError};

fn checkpoint(last_applied_sequence: u64) -> JournalCheckpoint {
    JournalCheckpoint {
        node_id: "5c3e0f3b-1e8d-4dd2-9aa7-6d92c4d3f101".to_string(),
        epoch: "125a9f99-7dc8-4dae-a62c-8f50a3de0bb3".to_string(),
        segment: 2,
        offset: 1048576,
        last_applied_sequence,
        updated_at: "2026-11-16T12:00:00Z".to_string(),
    }
}

#[test]
fn saves_and_replaces_a_checkpoint_atomically() {
    let directory = tempdir().expect("temporary directory");
    checkpoint(10)
        .save(directory.path())
        .expect("save first checkpoint");
    checkpoint(11)
        .save(directory.path())
        .expect("replace checkpoint");

    let loaded = JournalCheckpoint::load(directory.path(), "5c3e0f3b-1e8d-4dd2-9aa7-6d92c4d3f101")
        .expect("load checkpoint")
        .expect("checkpoint exists");
    assert_eq!(loaded.last_applied_sequence, 11);
}

#[test]
fn returns_none_when_a_checkpoint_is_absent() {
    let directory = tempdir().expect("temporary directory");
    assert!(
        JournalCheckpoint::load(directory.path(), "5c3e0f3b-1e8d-4dd2-9aa7-6d92c4d3f101")
            .expect("load checkpoint")
            .is_none()
    );
}

#[test]
fn rejects_a_checkpoint_from_another_node() {
    let directory = tempdir().expect("temporary directory");
    checkpoint(10)
        .save(directory.path())
        .expect("save checkpoint");
    assert!(matches!(
        JournalCheckpoint::load(directory.path(), "eb0e25eb-30a0-4fef-89ad-650064175bdf"),
        Err(JournalError::InvalidLayout(_))
    ));
}

#[test]
fn rejects_a_corrupt_checkpoint() {
    let directory = tempdir().expect("temporary directory");
    fs::write(directory.path().join("checkpoint.json"), b"{not-json")
        .expect("write corrupt checkpoint");
    assert!(matches!(
        JournalCheckpoint::load(directory.path(), "5c3e0f3b-1e8d-4dd2-9aa7-6d92c4d3f101"),
        Err(JournalError::Corrupt(_))
    ));
}
