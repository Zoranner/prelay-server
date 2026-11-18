use tempfile::tempdir;

use crate::store::recording_journal::{
    frame::{CommitMetadata, Frame, FrameType},
    journal::JournalRecord,
    JournalError,
};

use super::{append_raw_bytes, append_raw_frame, open_writer, read_batches, test_cipher};

#[test]
fn reports_the_commit_frame_position() {
    let directory = tempdir().expect("temporary directory");
    {
        let mut writer = open_writer(directory.path(), 1024 * 1024);
        writer
            .append_and_commit(&[JournalRecord::new(
                "activity-a",
                "turn_completed",
                vec![b"committed".to_vec()],
            )])
            .expect("append record");
    }
    let batch = read_batches(directory.path())
        .into_iter()
        .next()
        .expect("committed batch");
    assert_eq!(batch.position.segment, 1);
    assert!(batch.position.offset > 0);
    assert!(!batch.position.epoch.is_empty());
}

#[test]
fn ignores_an_uncommitted_data_tail() {
    let directory = tempdir().expect("temporary directory");
    let receipt = {
        let mut writer = open_writer(directory.path(), 1024 * 1024);
        writer
            .append_and_commit(&[JournalRecord::new(
                "activity-a",
                "turn_completed",
                vec![b"committed".to_vec()],
            )])
            .expect("append record")
    };
    append_raw_frame(
        directory.path(),
        Frame {
            sequence: receipt.durable_sequence + 1,
            frame_type: FrameType::Data,
            activity_id: "activity-b".to_string(),
            kind: "turn_finished".to_string(),
            part_index: 0,
            part_count: 1,
            payload: b"uncommitted".to_vec(),
        },
    );

    let batches = read_batches(directory.path());
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].records[0].payload, b"committed");
}

#[test]
fn ignores_a_truncated_tail() {
    let directory = tempdir().expect("temporary directory");
    {
        let mut writer = open_writer(directory.path(), 1024 * 1024);
        writer
            .append_and_commit(&[JournalRecord::new(
                "activity-a",
                "turn_completed",
                vec![b"committed".to_vec()],
            )])
            .expect("append record");
    }
    append_raw_bytes(directory.path(), &[0, 0, 0, 128, b'x']);
    assert_eq!(read_batches(directory.path()).len(), 1);
}

#[test]
fn continues_after_an_uncommitted_tail() {
    let directory = tempdir().expect("temporary directory");
    let first_durable_sequence = {
        let mut writer = open_writer(directory.path(), 1024 * 1024);
        writer
            .append_and_commit(&[JournalRecord::new(
                "activity-a",
                "turn_completed",
                vec![b"committed".to_vec()],
            )])
            .expect("append record")
            .durable_sequence
    };
    append_raw_frame(
        directory.path(),
        Frame {
            sequence: first_durable_sequence + 1,
            frame_type: FrameType::Data,
            activity_id: "activity-abandoned".to_string(),
            kind: "turn_finished".to_string(),
            part_index: 0,
            part_count: 1,
            payload: b"uncommitted".to_vec(),
        },
    );
    let next_receipt = {
        let mut writer = open_writer(directory.path(), 1024 * 1024);
        writer
            .append_and_commit(&[JournalRecord::new(
                "activity-b",
                "turn_completed",
                vec![b"next".to_vec()],
            )])
            .expect("append next record")
    };
    assert_eq!(next_receipt.first_sequence, first_durable_sequence + 2);

    let batches = read_batches(directory.path());
    assert_eq!(batches.len(), 2);
    assert_eq!(batches[1].records[0].activity_id, "activity-b");
}

#[test]
fn rejects_a_commit_with_a_missing_sequence() {
    let directory = tempdir().expect("temporary directory");
    let first_durable_sequence = {
        let mut writer = open_writer(directory.path(), 1024 * 1024);
        writer
            .append_and_commit(&[JournalRecord::new(
                "activity-a",
                "turn_completed",
                vec![b"committed".to_vec()],
            )])
            .expect("append record")
            .durable_sequence
    };
    append_raw_frame(
        directory.path(),
        Frame {
            sequence: first_durable_sequence + 1,
            frame_type: FrameType::Data,
            activity_id: "activity-b".to_string(),
            kind: "turn_completed".to_string(),
            part_index: 0,
            part_count: 1,
            payload: b"orphan".to_vec(),
        },
    );
    append_raw_frame(
        directory.path(),
        Frame {
            sequence: first_durable_sequence + 2,
            frame_type: FrameType::Commit,
            activity_id: String::new(),
            kind: "commit".to_string(),
            part_index: 0,
            part_count: 1,
            payload: serde_json::to_vec(&CommitMetadata {
                group_id: "group-invalid".to_string(),
                first_sequence: first_durable_sequence + 1,
                last_sequence: first_durable_sequence + 2,
                committed_at: "2026-11-16T12:00:00Z".to_string(),
            })
            .expect("serialize commit metadata"),
        },
    );

    assert!(matches!(
        crate::store::recording_journal::reader::RecordingJournalReader::read_committed(
            directory.path(),
            &test_cipher()
        ),
        Err(JournalError::Corrupt(_))
    ));
}

#[test]
fn rejects_a_corrupt_frame_in_the_middle() {
    let directory = tempdir().expect("temporary directory");
    {
        let mut writer = open_writer(directory.path(), 1024 * 1024);
        writer
            .append_and_commit(&[JournalRecord::new(
                "activity-a",
                "turn_completed",
                vec![b"committed".to_vec()],
            )])
            .expect("append record");
    }
    let node_id = crate::store::recording_journal::layout::read_node_id(directory.path())
        .expect("read node ID");
    let segment =
        crate::store::recording_journal::layout::list_segments(directory.path(), &node_id)
            .expect("list segments")
            .pop()
            .expect("segment");
    let mut bytes = std::fs::read(&segment.path).expect("read segment");
    let index = bytes.len() / 2;
    bytes[index] ^= 0x7f;
    std::fs::write(&segment.path, bytes).expect("write corrupt segment");

    assert!(
        crate::store::recording_journal::reader::RecordingJournalReader::read_committed(
            directory.path(),
            &test_cipher()
        )
        .is_err()
    );
}
