use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Barrier},
    thread,
    time::Duration,
};

use tempfile::tempdir;

use crate::store::recording_journal::{
    frame::MAX_FRAME_BYTES,
    group_commit::{GroupCommitConfig, GroupCommitter},
    journal::JournalRecord,
    JournalError,
};

use super::{open_writer, read_batches};

fn config(max_wait_ms: u64, max_records: usize, max_bytes: usize) -> GroupCommitConfig {
    GroupCommitConfig {
        max_wait: Duration::from_millis(max_wait_ms),
        max_records,
        max_bytes,
        queue_capacity: 64,
    }
}

fn record(activity_id: &str, payload: &[u8]) -> JournalRecord {
    JournalRecord::new(activity_id, "turn_completed", vec![payload.to_vec()])
}

#[test]
fn groups_concurrent_submissions_into_one_commit() {
    let directory = tempdir().expect("temporary directory");
    let writer = open_writer(directory.path(), 1024 * 1024);
    let committer =
        GroupCommitter::start(writer, config(100, 64, 8 * 1024 * 1024)).expect("start committer");
    let handle = committer.handle();
    let barrier = Arc::new(Barrier::new(8));
    let workers = (0..8)
        .map(|index| {
            let handle = handle.clone();
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                handle
                    .submit_blocking(vec![record(
                        &format!("activity-{index}"),
                        format!("payload-{index}").as_bytes(),
                    )])
                    .expect("submit record")
                    .wait_timeout(Duration::from_secs(5))
                    .expect("wait for commit")
            })
        })
        .collect::<Vec<_>>();
    let receipts = workers
        .into_iter()
        .map(|worker| worker.join().expect("join submitter"))
        .collect::<Vec<_>>();
    let group_ids = receipts
        .iter()
        .map(|receipt| receipt.group_id.clone())
        .collect::<HashSet<_>>();
    let durable_sequences = receipts
        .iter()
        .map(|receipt| receipt.durable_sequence)
        .collect::<HashSet<_>>();
    let mut first_sequences = receipts
        .iter()
        .map(|receipt| receipt.first_sequence)
        .collect::<Vec<_>>();
    first_sequences.sort_unstable();

    assert_eq!(group_ids.len(), 1);
    assert_eq!(durable_sequences.len(), 1);
    assert_eq!(first_sequences, (1..=8).collect::<Vec<_>>());
    assert_eq!(read_batches(directory.path()).len(), 1);

    drop(handle);
    committer.shutdown();
}

#[test]
fn splits_groups_at_the_record_limit() {
    let directory = tempdir().expect("temporary directory");
    let writer = open_writer(directory.path(), 1024 * 1024);
    let committer =
        GroupCommitter::start(writer, config(100, 2, 8 * 1024 * 1024)).expect("start committer");
    let handle = committer.handle();
    let waiters = (0..5)
        .map(|index| {
            handle
                .submit_blocking(vec![record(
                    &format!("activity-{index}"),
                    format!("payload-{index}").as_bytes(),
                )])
                .expect("submit record")
        })
        .collect::<Vec<_>>();
    let receipts = waiters
        .into_iter()
        .map(|waiter| waiter.wait_blocking().expect("wait for commit"))
        .collect::<Vec<_>>();
    let mut group_sizes = HashMap::<String, usize>::new();
    for receipt in receipts {
        *group_sizes.entry(receipt.group_id).or_default() += 1;
    }
    let mut sizes = group_sizes.into_values().collect::<Vec<_>>();
    sizes.sort_unstable();
    assert_eq!(sizes, vec![1, 2, 2]);
    assert_eq!(read_batches(directory.path()).len(), 3);

    drop(handle);
    committer.shutdown();
}

#[test]
fn commits_a_single_record_larger_than_the_group_byte_limit() {
    let directory = tempdir().expect("temporary directory");
    let writer = open_writer(directory.path(), 1024 * 1024);
    let committer = GroupCommitter::start(writer, config(5, 64, 1024)).expect("start committer");
    let handle = committer.handle();
    let receipt = handle
        .submit_blocking(vec![record("activity-large", &vec![0_u8; 4096])])
        .expect("submit large record")
        .wait_timeout(Duration::from_secs(5))
        .expect("wait for commit");
    assert_eq!(receipt.first_sequence, 1);
    assert_eq!(receipt.last_sequence, 1);
    assert_eq!(read_batches(directory.path()).len(), 1);

    drop(handle);
    committer.shutdown();
}

#[test]
fn rejects_empty_submissions_before_they_enter_the_queue() {
    let directory = tempdir().expect("temporary directory");
    let writer = open_writer(directory.path(), 1024 * 1024);
    let committer =
        GroupCommitter::start(writer, config(5, 64, 8 * 1024 * 1024)).expect("start committer");
    let handle = committer.handle();
    assert!(matches!(
        handle.submit_blocking(Vec::new()),
        Err(JournalError::EmptyRecord)
    ));
    assert!(matches!(
        handle.submit_blocking(vec![JournalRecord::new(
            "activity-empty",
            "turn_completed",
            Vec::new(),
        )]),
        Err(JournalError::EmptyRecord)
    ));

    drop(handle);
    committer.shutdown();
}

#[test]
fn rejects_an_invalid_group_configuration() {
    let directory = tempdir().expect("temporary directory");
    let writer = open_writer(directory.path(), 1024 * 1024);
    let result = GroupCommitter::start(writer, config(5, 0, 1024));
    assert!(matches!(result, Err(JournalError::InvalidLayout(_))));
}

#[test]
fn keeps_the_journal_healthy_after_rejecting_an_oversized_frame() {
    let directory = tempdir().expect("temporary directory");
    let writer = open_writer(directory.path(), 32 * 1024 * 1024);
    let committer =
        GroupCommitter::start(writer, config(5, 64, 8 * 1024 * 1024)).expect("start committer");
    let handle = committer.handle();
    let oversized = vec![0_u8; MAX_FRAME_BYTES];
    let error = handle
        .submit_blocking(vec![record("activity-huge", &oversized)])
        .expect("submit oversized record")
        .wait_timeout(Duration::from_secs(60))
        .expect_err("oversized frame must fail");
    assert!(matches!(error, JournalError::Corrupt(_)), "{error:?}");

    let receipt = handle
        .submit_blocking(vec![record("activity-small", b"ok")])
        .expect("submit next record")
        .wait_timeout(Duration::from_secs(60))
        .expect("commit next record");
    assert_eq!(receipt.first_sequence, 1);
    assert_eq!(read_batches(directory.path()).len(), 1);

    drop(handle);
    committer.shutdown();
}
