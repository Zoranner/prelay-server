use std::{
    collections::{BTreeMap, HashMap},
    fs::{self, File},
    io::{self, Read, Seek, SeekFrom},
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use super::{
    cache::{FileStamp, ThreadState, WindowState},
    read::thread_candidate_paths,
    support::ContentHash,
    RolloutStore, RolloutWriter,
};
use crate::domain::activity::rollout::{ConversationFormat, RolloutError};

const THREAD_INDEX_VERSION: u8 = 3;

#[derive(Clone, Copy, Deserialize, PartialEq, Eq, Serialize)]
struct PersistedStamp {
    len: u64,
    modified_nanos: Option<u64>,
}

impl PersistedStamp {
    fn from_file_stamp(stamp: FileStamp) -> Self {
        Self {
            len: stamp.len,
            modified_nanos: stamp.modified.and_then(system_time_nanos),
        }
    }

    fn into_file_stamp(self) -> FileStamp {
        FileStamp {
            len: self.len,
            modified: self.modified_nanos.and_then(system_time_from_nanos),
        }
    }
}

#[derive(Deserialize, Serialize)]
struct PersistedThreadIndex {
    version: u8,
    thread_path: String,
    relative_path: String,
    format: ConversationFormat,
    source_stamp: PersistedStamp,
    base_stamp: Option<PersistedStamp>,
    line_count: u64,
    seq: u64,
    window_id: Option<String>,
    generation: Option<u64>,
    content_count: u64,
    content_sha256: String,
    content_event_count: u64,
    event_refs: BTreeMap<String, Vec<u64>>,
    item_refs: BTreeMap<String, u64>,
}

impl RolloutStore {
    pub(super) fn load_persisted_thread(
        &self,
        identity_id: &str,
        thread_id: &str,
    ) -> Result<Option<ThreadState>, RolloutError> {
        let thread_relative = thread_relative(identity_id, thread_id);
        let index_relative = index_relative(&thread_relative);
        let index_path = self.checked_absolute_path(&index_relative)?;
        let bytes = match fs::read(&index_path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let index = match serde_json::from_slice::<PersistedThreadIndex>(&bytes) {
            Ok(index) => index,
            Err(_) => return Ok(None),
        };
        if index.version != THREAD_INDEX_VERSION || index.thread_path != thread_relative {
            return Ok(None);
        }
        if !thread_candidate_paths(identity_id, thread_id)
            .iter()
            .any(|candidate| candidate == &index.relative_path)
        {
            return Ok(None);
        }

        let source_path = self.checked_absolute_path(&index.relative_path)?;
        let source_metadata = match fs::metadata(&source_path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let source_stamp = FileStamp::from_metadata(&source_metadata);
        if source_stamp != index.source_stamp.into_file_stamp() {
            return Ok(None);
        }

        let base_path = self.checked_absolute_path(&thread_relative)?;
        let base_stamp = metadata_stamp(&base_path)?;
        if base_stamp != index.base_stamp.map(PersistedStamp::into_file_stamp) {
            return Ok(None);
        }

        let Some(digest) = decode_digest(&index.content_sha256) else {
            return Ok(None);
        };
        let mut event_refs = HashMap::new();
        for (identity, refs) in index.event_refs {
            let Some(identity) = decode_digest(&identity) else {
                return Ok(None);
            };
            if refs.is_empty() || refs.iter().any(|seq| *seq == 0 || *seq > index.seq) {
                return Ok(None);
            }
            event_refs.insert(identity, refs);
        }
        if index
            .item_refs
            .values()
            .any(|seq| *seq == 0 || *seq > index.seq)
        {
            return Ok(None);
        }
        let window = match (index.window_id, index.generation) {
            (Some(window_id), Some(generation)) => Some(WindowState {
                window_id: Some(window_id),
                generation,
            }),
            (None, Some(generation)) => Some(WindowState {
                window_id: None,
                generation,
            }),
            (None, None) => None,
            (Some(_), None) => return Ok(None),
        };
        Ok(Some(ThreadState {
            relative_path: index.relative_path,
            format: index.format,
            stamp: source_stamp,
            line_count: index.line_count,
            seq: index.seq,
            window,
            content: ContentHash::from_parts(index.content_count, digest),
            event_refs,
            item_refs: index.item_refs.into_iter().collect(),
            content_event_count: index.content_event_count,
        }))
    }

    pub(super) fn persist_thread_index(
        &self,
        identity_id: &str,
        thread_id: &str,
        state: &ThreadState,
    ) {
        let thread_relative = thread_relative(identity_id, thread_id);
        let index_relative = index_relative(&thread_relative);
        let paths = self
            .checked_absolute_path(&index_relative)
            .and_then(|index_path| {
                self.checked_absolute_path(&thread_relative)
                    .and_then(|thread_path| {
                        self.checked_absolute_path(&state.relative_path)
                            .map(|source_path| (index_path, thread_path, source_path))
                    })
            });
        let (index_path, thread_path, source_path) = match paths {
            Ok(paths) => paths,
            Err(error) => {
                tracing::warn!(
                    thread = %thread_relative,
                    error = %error,
                    "failed to resolve thread index paths"
                );
                return;
            }
        };
        if let Err(error) = write_thread_index(
            &index_path,
            &thread_relative,
            &thread_path,
            &source_path,
            state,
        ) {
            tracing::warn!(
                index_path = %index_path.display(),
                error = %error,
                "failed to write thread index"
            );
        }
    }

    pub(crate) fn remove_thread_index(&self, relative_path: &str) {
        let mut index_relatives = vec![index_relative(relative_path)];
        if let Some(thread_relative) = logical_thread_relative(relative_path) {
            let logical_index = index_relative(&thread_relative);
            if logical_index != index_relatives[0] {
                index_relatives.push(logical_index);
            }
        }
        for index_relative in index_relatives {
            let Ok(path) = self.checked_absolute_path(&index_relative) else {
                continue;
            };
            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => {
                    tracing::warn!(
                        index_path = %path.display(),
                        error = %error,
                        "failed to remove thread index during identity cleanup"
                    );
                }
            }
        }
    }
}

impl RolloutWriter {
    pub(super) fn persist_index(&self, state: &ThreadState) {
        if let Err(error) = write_thread_index(
            &self.index_path,
            &self.thread_relative,
            &self.thread_path,
            &self.final_path,
            state,
        ) {
            tracing::warn!(
                rollout_path = %self.final_path.display(),
                error = %error,
                "failed to persist thread index after finish"
            );
        }
    }
}

pub(super) fn write_thread_index(
    index_path: &Path,
    thread_relative: &str,
    thread_path: &Path,
    source_path: &Path,
    state: &ThreadState,
) -> Result<(), RolloutError> {
    if state.is_empty() {
        return Ok(());
    }
    let source_metadata = fs::metadata(source_path)?;
    let source_stamp = FileStamp::from_metadata(&source_metadata);
    if source_stamp != state.stamp
        || !has_complete_tail(source_path, source_metadata.len())?
        || source_stamp.modified.and_then(system_time_nanos).is_none()
    {
        return Ok(());
    }
    let base_stamp = if source_path == thread_path {
        Some(source_stamp)
    } else {
        metadata_stamp(thread_path)?
    };
    let index = PersistedThreadIndex {
        version: THREAD_INDEX_VERSION,
        thread_path: thread_relative.to_string(),
        relative_path: state.relative_path.clone(),
        format: state.format,
        source_stamp: PersistedStamp::from_file_stamp(source_stamp),
        base_stamp: base_stamp.map(PersistedStamp::from_file_stamp),
        line_count: state.line_count,
        seq: state.seq,
        window_id: state
            .window
            .as_ref()
            .and_then(|window| window.window_id.clone()),
        generation: state.window.as_ref().map(|window| window.generation),
        content_count: state.content.count,
        content_sha256: encode_digest(&state.content.digest_bytes()),
        content_event_count: state.content_event_count,
        event_refs: state
            .event_refs
            .iter()
            .map(|(identity, refs)| (encode_digest(identity), refs.clone()))
            .collect(),
        item_refs: state
            .item_refs
            .iter()
            .map(|(id, seq)| (id.clone(), *seq))
            .collect(),
    };
    let bytes = serde_json::to_vec(&index)?;
    fs::write(index_path, bytes)?;
    Ok(())
}

fn thread_relative(identity_id: &str, thread_id: &str) -> String {
    format!("{identity_id}/{thread_id}.jsonl")
}

fn index_relative(thread_relative: &str) -> String {
    format!("{thread_relative}.index")
}

fn logical_thread_relative(relative_path: &str) -> Option<String> {
    let (directory, file) = relative_path.rsplit_once('/')?;
    let stem = file.strip_suffix(".jsonl")?;
    let thread = if let Some(thread) = stem.strip_suffix(".recovered") {
        thread
    } else {
        let (thread, segment) = stem.rsplit_once(".recovered-")?;
        if segment.is_empty() || !segment.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        thread
    };
    Some(format!("{directory}/{thread}.jsonl"))
}

fn metadata_stamp(path: &Path) -> Result<Option<FileStamp>, RolloutError> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(Some(FileStamp::from_metadata(&metadata))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn has_complete_tail(path: &Path, len: u64) -> Result<bool, io::Error> {
    if len == 0 {
        return Ok(true);
    }
    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(len - 1))?;
    let mut last = [0_u8; 1];
    file.read_exact(&mut last)?;
    Ok(last[0] == b'\n')
}

fn system_time_nanos(time: SystemTime) -> Option<u64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_nanos()).ok())
}

fn system_time_from_nanos(nanos: u64) -> Option<SystemTime> {
    UNIX_EPOCH.checked_add(Duration::from_nanos(nanos))
}

fn encode_digest(digest: &[u8; 32]) -> String {
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn decode_digest(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64 {
        return None;
    }
    let mut digest = [0_u8; 32];
    for (index, chunk) in value.as_bytes().as_chunks::<2>().0.iter().enumerate() {
        let chunk = std::str::from_utf8(chunk).ok()?;
        digest[index] = u8::from_str_radix(chunk, 16).ok()?;
    }
    Some(digest)
}

#[cfg(test)]
mod tests {
    use super::logical_thread_relative;

    #[test]
    fn derives_the_logical_thread_from_recovered_segments() {
        assert_eq!(
            logical_thread_relative("identity-1/thread.recovered.jsonl").as_deref(),
            Some("identity-1/thread.jsonl")
        );
        assert_eq!(
            logical_thread_relative("identity-1/thread.recovered-10.jsonl").as_deref(),
            Some("identity-1/thread.jsonl")
        );
        assert_eq!(logical_thread_relative("identity-1/thread.jsonl"), None);
        assert_eq!(logical_thread_relative("identity-1/thread.txt"), None);
    }

    #[test]
    fn removes_the_logical_index_for_a_recovered_segment() {
        let root = tempfile::tempdir().expect("temp rollout root");
        let store = super::RolloutStore::new(root.path().to_path_buf());
        let index_path = root.path().join("identity-1/thread.jsonl.index");
        std::fs::create_dir_all(index_path.parent().expect("index parent"))
            .expect("create index parent");
        std::fs::write(&index_path, b"{}").expect("write index");

        store.remove_thread_index("identity-1/thread.recovered.jsonl");

        assert!(!index_path.exists());
    }
}
