use std::collections::HashSet;
use std::fs;

use tempfile::tempdir;

use crate::store::recording_journal::{
    frame::MAX_FRAME_BYTES,
    journal::{JournalRecord, RecordingJournalWriter},
    JournalError,
};

use super::{open_writer, read_batches, segment_count};

#[test]
fn commits_multiple_records_as_one_batch() {
    let directory = tempdir().expect("temporary directory");
    let mut writer = open_writer(directory.path(), 1024 * 1024);
    let receipt = writer
        .append_and_commit(&[
            JournalRecord::new(
                "activity-a",
                "turn_completed",
                vec![b"first".to_vec(), b"second".to_vec()],
            ),
            JournalRecord::new(
                "activity-b",
                "provider_observation",
                vec![b"third".to_vec()],
            ),
        ])
        .expect("append records");
    assert_eq!(receipt.first_sequence, 1);
    assert_eq!(receipt.last_sequence, 3);
    assert_eq!(receipt.durable_sequence, 4);

    let batches = read_batches(directory.path());
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].metadata, receipt_metadata(&receipt));
    assert_eq!(batches[0].records.len(), 3);
    assert_eq!(batches[0].records[0].payload, b"first");
    assert_eq!(batches[0].records[2].activity_id, "activity-b");
}

#[test]
fn continues_after_a_restart_without_reusing_sequences() {
    let directory = tempdir().expect("temporary directory");
    let first_receipt = {
        let mut writer = open_writer(directory.path(), 1024 * 1024);
        writer
            .append_and_commit(&[JournalRecord::new(
                "activity-a",
                "turn_completed",
                vec![b"first".to_vec()],
            )])
            .expect("append first record")
    };
    let second_receipt = {
        let mut writer = open_writer(directory.path(), 1024 * 1024);
        writer
            .append_and_commit(&[JournalRecord::new(
                "activity-b",
                "turn_completed",
                vec![b"second".to_vec()],
            )])
            .expect("append second record")
    };
    assert!(second_receipt.first_sequence > first_receipt.durable_sequence);

    let batches = read_batches(directory.path());
    assert_eq!(batches.len(), 2);
    assert_eq!(batches[1].records[0].activity_id, "activity-b");
    let node_id = crate::store::recording_journal::layout::read_node_id(directory.path())
        .expect("read node ID");
    let epochs = crate::store::recording_journal::layout::list_segments(directory.path(), &node_id)
        .expect("list segments")
        .into_iter()
        .map(|segment| segment.epoch)
        .collect::<HashSet<_>>();
    assert_eq!(epochs.len(), 2);
}

#[test]
fn rotates_segments_and_reads_across_them() {
    let directory = tempdir().expect("temporary directory");
    let mut writer = open_writer(directory.path(), 1024);
    for index in 0..5 {
        writer
            .append_and_commit(&[JournalRecord::new(
                format!("activity-{index}"),
                "turn_completed",
                vec![vec![index as u8; 700]],
            )])
            .expect("append record");
    }
    assert!(segment_count(directory.path()) >= 5);

    let batches = read_batches(directory.path());
    assert_eq!(batches.len(), 5);
    assert_eq!(batches[4].records[0].activity_id, "activity-4");
}

#[test]
fn rejects_a_second_writer() {
    let directory = tempdir().expect("temporary directory");
    let _writer = open_writer(directory.path(), 1024 * 1024);
    let second = RecordingJournalWriter::open(directory.path(), super::test_cipher(), 1024 * 1024);
    assert!(matches!(second, Err(JournalError::Locked(_))));
}

#[test]
fn rejects_a_missing_node_id_when_segments_exist() {
    let directory = tempdir().expect("temporary directory");
    fs::write(
        directory
            .path()
            .join("wal-5c3e0f3b-1e8d-4dd2-9aa7-6d92c4d3f101-125a9f99-7dc8-4dae-a62c-8f50a3de0bb3-000001.log"),
        [],
    )
    .expect("write orphan segment");
    assert!(matches!(
        open_result(directory.path()),
        Err(JournalError::InvalidLayout(_))
    ));
}

#[test]
fn rejects_frames_above_the_encoded_size_limit() {
    let directory = tempdir().expect("temporary directory");
    let mut writer = open_writer(directory.path(), 32 * 1024 * 1024);
    let result = writer.append_and_commit(&[JournalRecord::new(
        "activity-large",
        "turn_completed",
        vec![vec![0_u8; MAX_FRAME_BYTES]],
    )]);
    assert!(matches!(result, Err(JournalError::Corrupt(_))));
}

#[test]
fn rejects_empty_records_and_empty_parts() {
    let directory = tempdir().expect("temporary directory");
    let mut writer = open_writer(directory.path(), 1024 * 1024);
    assert!(matches!(
        writer.append_and_commit(&[]),
        Err(JournalError::EmptyRecord)
    ));
    assert!(matches!(
        writer.append_and_commit(&[JournalRecord::new(
            "activity-empty",
            "turn_completed",
            Vec::new(),
        )]),
        Err(JournalError::EmptyRecord)
    ));
}

fn open_result(path: &std::path::Path) -> Result<RecordingJournalWriter, JournalError> {
    RecordingJournalWriter::open(path, super::test_cipher(), 1024 * 1024)
}

fn receipt_metadata(
    receipt: &crate::store::recording_journal::journal::CommitReceipt,
) -> crate::store::recording_journal::frame::CommitMetadata {
    crate::store::recording_journal::frame::CommitMetadata {
        group_id: receipt.group_id.clone(),
        first_sequence: receipt.first_sequence,
        last_sequence: receipt.last_sequence,
        committed_at: receipt.committed_at.clone(),
    }
}
