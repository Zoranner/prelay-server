mod checkpoint;
mod frame;
mod group_commit;
mod journal;
mod materializer;
mod payload;
mod reader;
mod record;
mod sink;
mod sink_stream;

use std::{fs::OpenOptions, io::Write, path::Path};

use crate::store::storage::MasterKey;

use super::{
    cipher::JournalCipher,
    frame::{encode_frame, Frame, JournalBinding},
    journal::RecordingJournalWriter,
    layout::{list_segments, read_node_id},
    reader::RecordingJournalReader,
};

fn test_cipher() -> JournalCipher {
    JournalCipher::new(&MasterKey::from_bytes([0x42; 32]))
}

fn open_writer(root: &Path, segment_bytes: usize) -> RecordingJournalWriter {
    RecordingJournalWriter::open(root, test_cipher(), segment_bytes).expect("open journal writer")
}

fn read_batches(root: &Path) -> Vec<super::frame::CommittedBatch> {
    RecordingJournalReader::read_committed(root, &test_cipher()).expect("read journal")
}

fn append_raw_frame(root: &Path, frame: Frame) {
    let cipher = test_cipher();
    let node_id = read_node_id(root).expect("read node ID");
    let segments = list_segments(root, &node_id).expect("list segments");
    let segment = segments.last().expect("journal segment");
    let binding = JournalBinding {
        node_id,
        epoch: segment.epoch.clone(),
    };
    let encoded = encode_frame(&frame, &cipher, &binding).expect("encode raw frame");
    let mut file = OpenOptions::new()
        .append(true)
        .open(&segment.path)
        .expect("open segment");
    file.write_all(&(encoded.len() as u32).to_be_bytes())
        .expect("write frame length");
    file.write_all(&encoded).expect("write frame");
    file.sync_all().expect("sync raw frame");
}

fn append_raw_bytes(root: &Path, bytes: &[u8]) {
    let node_id = read_node_id(root).expect("read node ID");
    let segments = list_segments(root, &node_id).expect("list segments");
    let segment = segments.last().expect("journal segment");
    let mut file = OpenOptions::new()
        .append(true)
        .open(&segment.path)
        .expect("open segment");
    file.write_all(bytes).expect("write raw bytes");
    file.sync_all().expect("sync raw bytes");
}

fn segment_count(root: &Path) -> usize {
    let node_id = read_node_id(root).expect("read node ID");
    list_segments(root, &node_id).expect("list segments").len()
}
