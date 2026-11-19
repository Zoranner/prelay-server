use std::sync::{Arc, Mutex};

use futures::future::BoxFuture;
use serde_json::json;
use tempfile::tempdir;

use crate::store::recording_journal::{
    checkpoint::JournalCheckpoint,
    journal::JournalRecord,
    layout::{list_segments, read_node_id},
    materializer::{MaterializeError, MaterializeSink, MaterializeSummary, Materializer},
    record::{RecordEnvelope, RecordKind},
};

use super::{open_writer, read_batches, test_cipher};

#[derive(Default)]
struct SinkState {
    applied: Vec<String>,
    fail_on: Option<String>,
}

#[derive(Clone, Default)]
struct TestSink {
    state: Arc<Mutex<SinkState>>,
}

impl TestSink {
    fn fail_on(&self, record_id: impl Into<String>) {
        self.state.lock().expect("sink state").fail_on = Some(record_id.into());
    }

    fn applied(&self) -> Vec<String> {
        self.state.lock().expect("sink state").applied.clone()
    }
}

impl MaterializeSink for TestSink {
    fn apply(&self, record: RecordEnvelope) -> BoxFuture<'static, Result<(), MaterializeError>> {
        let state = Arc::clone(&self.state);
        Box::pin(async move {
            let fail_on = state.lock().expect("sink state").fail_on.clone();
            if fail_on.as_deref() == Some(record.record_id.as_str()) {
                return Err(MaterializeError::Sink("injected failure".to_string()));
            }
            state
                .lock()
                .expect("sink state")
                .applied
                .push(record.record_id);
            Ok(())
        })
    }
}

fn envelope(activity_id: &str, value: i64) -> RecordEnvelope {
    RecordEnvelope::new(
        activity_id,
        "identity-1",
        RecordKind::TurnCompleted,
        "2026-11-17T12:00:00+08:00",
        json!({"value": value}),
    )
}

fn record(activity_id: &str, value: i64) -> JournalRecord {
    JournalRecord::from_envelope(envelope(activity_id, value), 7).expect("encode record")
}

fn large_record(activity_id: &str) -> JournalRecord {
    JournalRecord::from_envelope(
        RecordEnvelope::new(
            activity_id,
            "identity-1",
            RecordKind::TurnCompleted,
            "2026-11-17T12:00:00+08:00",
            json!({"value": "x".repeat(900)}),
        ),
        2048,
    )
    .expect("encode large record")
}

#[tokio::test]
async fn applies_records_in_order_and_persists_a_checkpoint() {
    let directory = tempdir().expect("temporary directory");
    let receipt = {
        let mut writer = open_writer(directory.path(), 1024 * 1024);
        writer
            .append_and_commit(&[record("activity-1", 1), record("activity-2", 2)])
            .expect("append records")
    };
    let sink = Arc::new(TestSink::default());
    let mut materializer =
        Materializer::open(directory.path(), Arc::clone(&sink)).expect("open materializer");
    let summary = materializer
        .replay_all(&test_cipher())
        .await
        .expect("materialize records");

    assert_eq!(
        summary,
        MaterializeSummary {
            batches: 1,
            applied_records: 2,
            skipped_records: 0
        }
    );
    assert_eq!(
        sink.applied(),
        vec![
            "activity-1:turn_completed".to_string(),
            "activity-2:turn_completed".to_string()
        ]
    );
    let checkpoint = materializer.checkpoint().expect("checkpoint");
    assert_eq!(checkpoint.last_applied_sequence, receipt.last_sequence);
    assert_eq!(checkpoint.segment, 1);
    assert!(checkpoint.offset > 0);
    let persisted = JournalCheckpoint::load(directory.path(), &checkpoint.node_id)
        .expect("load checkpoint")
        .expect("checkpoint exists");
    assert_eq!(persisted, *checkpoint);
}

#[tokio::test]
async fn replay_skips_records_already_applied() {
    let directory = tempdir().expect("temporary directory");
    {
        let mut writer = open_writer(directory.path(), 1024 * 1024);
        writer
            .append_and_commit(&[record("activity-1", 1), record("activity-2", 2)])
            .expect("append records");
    }
    let sink = Arc::new(TestSink::default());
    let mut first =
        Materializer::open(directory.path(), Arc::clone(&sink)).expect("open first materializer");
    first
        .replay_all(&test_cipher())
        .await
        .expect("first replay");
    let mut second =
        Materializer::open(directory.path(), Arc::clone(&sink)).expect("open second materializer");
    let summary = second
        .replay_all(&test_cipher())
        .await
        .expect("second replay");

    assert_eq!(summary.applied_records, 0);
    assert_eq!(summary.skipped_records, 0);
    assert_eq!(sink.applied().len(), 2);
}

#[tokio::test]
async fn reads_only_records_after_the_persisted_checkpoint() {
    let directory = tempdir().expect("temporary directory");
    {
        let mut writer = open_writer(directory.path(), 1024 * 1024);
        writer
            .append_and_commit(&[record("activity-1", 1)])
            .expect("append first record");
    }
    let sink = Arc::new(TestSink::default());
    let mut first =
        Materializer::open(directory.path(), Arc::clone(&sink)).expect("open first materializer");
    first
        .replay_all(&test_cipher())
        .await
        .expect("first replay");
    {
        let mut writer = open_writer(directory.path(), 1024 * 1024);
        writer
            .append_and_commit(&[record("activity-2", 2)])
            .expect("append second record");
    }
    let mut second =
        Materializer::open(directory.path(), Arc::clone(&sink)).expect("open second materializer");
    let summary = second
        .replay_all(&test_cipher())
        .await
        .expect("second replay");

    assert_eq!(summary.applied_records, 1);
    assert_eq!(summary.skipped_records, 0);
    assert_eq!(
        sink.applied(),
        vec![
            "activity-1:turn_completed".to_string(),
            "activity-2:turn_completed".to_string()
        ]
    );
}

#[tokio::test]
async fn removes_segments_before_the_durable_checkpoint() {
    let directory = tempdir().expect("temporary directory");
    {
        let mut writer = open_writer(directory.path(), 1024);
        writer
            .append_and_commit(&[large_record("activity-1")])
            .expect("append first segment");
        writer
            .append_and_commit(&[large_record("activity-2")])
            .expect("append second segment");
    }
    assert!(super::segment_count(directory.path()) > 1);
    let sink = Arc::new(TestSink::default());
    let mut materializer =
        Materializer::open(directory.path(), Arc::clone(&sink)).expect("open materializer");
    materializer
        .replay_all(&test_cipher())
        .await
        .expect("materialize both segments");

    assert_eq!(super::segment_count(directory.path()), 1);
    assert_eq!(sink.applied().len(), 2);
}

#[tokio::test]
async fn a_sink_failure_does_not_advance_past_the_last_successful_batch() {
    let directory = tempdir().expect("temporary directory");
    let first_receipt = {
        let mut writer = open_writer(directory.path(), 1024 * 1024);
        let receipt = writer
            .append_and_commit(&[record("activity-1", 1)])
            .expect("append first record");
        writer
            .append_and_commit(&[record("activity-2", 2)])
            .expect("append second record");
        receipt
    };
    let sink = Arc::new(TestSink::default());
    sink.fail_on("activity-2:turn_completed");
    let mut materializer =
        Materializer::open(directory.path(), Arc::clone(&sink)).expect("open materializer");
    let error = materializer
        .replay_all(&test_cipher())
        .await
        .expect_err("second batch must fail");
    assert!(matches!(error, MaterializeError::Sink(_)));
    assert_eq!(
        sink.applied(),
        vec!["activity-1:turn_completed".to_string()]
    );
    let checkpoint = materializer.checkpoint().expect("checkpoint");
    assert_eq!(
        checkpoint.last_applied_sequence,
        first_receipt.last_sequence
    );
}

#[tokio::test]
async fn retries_the_whole_batch_after_a_mid_batch_sink_failure() {
    let directory = tempdir().expect("temporary directory");
    {
        let mut writer = open_writer(directory.path(), 1024 * 1024);
        writer
            .append_and_commit(&[record("activity-1", 1), record("activity-2", 2)])
            .expect("append batch");
    }
    let sink = Arc::new(TestSink::default());
    sink.fail_on("activity-2:turn_completed");
    let mut materializer =
        Materializer::open(directory.path(), Arc::clone(&sink)).expect("open materializer");
    let error = materializer
        .replay_all(&test_cipher())
        .await
        .expect_err("second logical record must fail");
    assert!(matches!(error, MaterializeError::Sink(_)));
    assert_eq!(
        sink.applied(),
        vec!["activity-1:turn_completed".to_string()]
    );
    assert!(materializer.checkpoint().is_none());

    sink.state.lock().expect("sink state").fail_on = None;
    materializer
        .replay_all(&test_cipher())
        .await
        .expect("retry batch");
    assert!(sink
        .applied()
        .contains(&"activity-2:turn_completed".to_string()));
    assert!(materializer
        .checkpoint()
        .is_some_and(|checkpoint| checkpoint.last_applied_sequence >= 2));
}

#[tokio::test]
async fn malformed_logical_records_fail_before_advancing_the_checkpoint() {
    let directory = tempdir().expect("temporary directory");
    {
        let mut writer = open_writer(directory.path(), 1024 * 1024);
        writer
            .append_and_commit(&[JournalRecord::new(
                "activity-bad",
                "turn_completed",
                vec![b"not-json".to_vec()],
            )])
            .expect("append malformed record");
    }
    let sink = Arc::new(TestSink::default());
    let mut materializer =
        Materializer::open(directory.path(), Arc::clone(&sink)).expect("open materializer");
    let error = materializer
        .replay_all(&test_cipher())
        .await
        .expect_err("malformed record must fail");
    assert!(matches!(error, MaterializeError::Journal(_)));
    assert!(materializer.checkpoint().is_none());
    assert!(sink.applied().is_empty());
}

#[tokio::test]
async fn rejects_a_checkpoint_that_splits_a_logical_record() {
    let directory = tempdir().expect("temporary directory");
    {
        let mut writer = open_writer(directory.path(), 1024 * 1024);
        writer
            .append_and_commit(&[record("activity-1", 1)])
            .expect("append record");
    }
    let batch = read_batches(directory.path())
        .into_iter()
        .next()
        .expect("committed batch");
    let checkpoint = JournalCheckpoint {
        node_id: read_node_id(directory.path()).expect("read node ID"),
        epoch: batch.position.epoch,
        segment: batch.position.segment,
        offset: batch.position.offset,
        last_applied_sequence: batch.metadata.first_sequence,
        updated_at: "2026-11-17T12:00:00Z".to_string(),
    };
    checkpoint.save(directory.path()).expect("save checkpoint");

    let sink = Arc::new(TestSink::default());
    let mut materializer =
        Materializer::open(directory.path(), Arc::clone(&sink)).expect("open materializer");
    let error = materializer
        .replay_all(&test_cipher())
        .await
        .expect_err("split checkpoint must fail");
    assert!(matches!(error, MaterializeError::Journal(_)));
    assert!(sink.applied().is_empty());
}

#[tokio::test]
async fn bounds_how_much_is_replayed_in_one_pass() {
    let directory = tempdir().expect("temporary directory");
    {
        let mut writer = open_writer(directory.path(), 1024 * 1024);
        for index in 0..8 {
            writer
                .append_and_commit(&[large_record(&format!("activity-{index}"))])
                .expect("append record");
        }
    }
    let sink = Arc::new(TestSink::default());
    let mut materializer =
        Materializer::open(directory.path(), Arc::clone(&sink)).expect("open materializer");
    materializer.set_replay_budget(4 * 1024);

    let first = materializer
        .replay_all(&test_cipher())
        .await
        .expect("first pass");
    assert!(
        first.applied_records >= 1 && first.applied_records < 8,
        "one pass must stop at the byte budget instead of replaying the whole backlog"
    );

    let mut applied = first.applied_records;
    for _ in 0..8 {
        let pass = materializer
            .replay_all(&test_cipher())
            .await
            .expect("next pass");
        if pass.applied_records == 0 {
            break;
        }
        applied += pass.applied_records;
    }
    assert_eq!(applied, 8);
    assert_eq!(sink.applied().len(), 8);
}

#[tokio::test]
async fn reclaims_applied_segments_when_a_later_batch_fails() {
    let directory = tempdir().expect("temporary directory");
    {
        let mut writer = open_writer(directory.path(), 1024);
        writer
            .append_and_commit(&[large_record("activity-1")])
            .expect("append first segment");
        writer
            .append_and_commit(&[large_record("activity-2")])
            .expect("append second segment");
    }
    let node_id = read_node_id(directory.path()).expect("read node ID");
    let before = list_segments(directory.path(), &node_id).expect("list segments before replay");
    assert!(before.len() > 1);
    let sink = Arc::new(TestSink::default());
    sink.fail_on("activity-2:turn_completed");
    let mut materializer =
        Materializer::open(directory.path(), Arc::clone(&sink)).expect("open materializer");

    let error = materializer
        .replay_all(&test_cipher())
        .await
        .expect_err("second batch must fail");
    assert!(matches!(error, MaterializeError::Sink(_)));
    let after = list_segments(directory.path(), &node_id).expect("list segments after replay");
    assert!(
        !before[0].path.exists(),
        "a segment covered by the durable checkpoint must be reclaimed even when a later batch fails"
    );
    assert!(after.len() < before.len());
    assert_eq!(
        after.last().map(|segment| &segment.path),
        before.last().map(|segment| &segment.path)
    );
}
