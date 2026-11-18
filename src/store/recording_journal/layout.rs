use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use uuid::Uuid;

use super::JournalError;

const NODE_ID_FILE: &str = "node-id";
const LOCK_FILE: &str = "writer.lock";
const SEGMENT_PREFIX: &str = "wal-";
const SEGMENT_SUFFIX: &str = ".log";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct JournalSegment {
    pub(crate) path: PathBuf,
    pub(crate) epoch: String,
    pub(crate) index: u32,
}

pub(super) fn acquire_writer_lock(root: &Path) -> Result<File, JournalError> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join(LOCK_FILE))?;
    file.try_lock()
        .map_err(|error| JournalError::Locked(error.to_string()))?;
    Ok(file)
}

pub(super) fn open_node_id(root: &Path) -> Result<String, JournalError> {
    let path = root.join(NODE_ID_FILE);
    if path.exists() {
        return read_node_id(root);
    }
    if contains_segment(root)? {
        return Err(JournalError::InvalidLayout(
            "node-id is missing while journal segments exist".to_string(),
        ));
    }

    let node_id = Uuid::new_v4().to_string();
    write_new_file_atomically(&path, format!("{node_id}\n").as_bytes())?;
    Ok(node_id)
}

pub(super) fn read_node_id(root: &Path) -> Result<String, JournalError> {
    let path = root.join(NODE_ID_FILE);
    let value = fs::read_to_string(&path).map_err(|error| {
        JournalError::InvalidLayout(format!("unable to read {}: {error}", path.display()))
    })?;
    let node_id = value.trim();
    Uuid::parse_str(node_id)
        .map_err(|_| JournalError::InvalidLayout("node-id is not a valid UUID".to_string()))?;
    Ok(node_id.to_string())
}

pub(super) fn new_epoch() -> String {
    Uuid::now_v7().to_string()
}

pub(super) fn list_segments(
    root: &Path,
    node_id: &str,
) -> Result<Vec<JournalSegment>, JournalError> {
    let prefix = segment_prefix(node_id);
    let mut segments = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                JournalError::InvalidLayout(format!(
                    "journal entry name is not valid UTF-8: {}",
                    path.display()
                ))
            })?;
        if !name.starts_with(SEGMENT_PREFIX) || !name.ends_with(SEGMENT_SUFFIX) {
            continue;
        }
        let remainder = name.strip_prefix(&prefix).ok_or_else(|| {
            JournalError::InvalidLayout(format!("journal segment belongs to another node: {name}"))
        })?;
        let stem = remainder.strip_suffix(SEGMENT_SUFFIX).ok_or_else(|| {
            JournalError::InvalidLayout(format!("invalid journal segment name: {name}"))
        })?;
        let (epoch, index) = stem.rsplit_once('-').ok_or_else(|| {
            JournalError::InvalidLayout(format!("invalid journal segment name: {name}"))
        })?;
        Uuid::parse_str(epoch).map_err(|_| {
            JournalError::InvalidLayout(format!("invalid journal segment epoch: {name}"))
        })?;
        let index = index.parse::<u32>().map_err(|_| {
            JournalError::InvalidLayout(format!("invalid journal segment index: {name}"))
        })?;
        if index == 0 {
            return Err(JournalError::InvalidLayout(format!(
                "journal segment index must be positive: {name}"
            )));
        }
        let epoch = epoch.to_string();
        segments.push(JournalSegment { path, epoch, index });
    }
    segments.sort_by(|left, right| {
        left.epoch
            .cmp(&right.epoch)
            .then_with(|| left.index.cmp(&right.index))
    });
    Ok(segments)
}

pub(super) fn segment_path(root: &Path, node_id: &str, epoch: &str, index: u32) -> PathBuf {
    root.join(format!(
        "{}{node_id}-{epoch}-{index:06}{SEGMENT_SUFFIX}",
        SEGMENT_PREFIX
    ))
}

fn contains_segment(root: &Path) -> Result<bool, JournalError> {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if path.is_file() && name.starts_with(SEGMENT_PREFIX) && name.ends_with(SEGMENT_SUFFIX) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn segment_prefix(node_id: &str) -> String {
    format!("{SEGMENT_PREFIX}{node_id}-")
}

fn write_new_file_atomically(path: &Path, contents: &[u8]) -> Result<(), JournalError> {
    let parent = path.parent().ok_or_else(|| {
        JournalError::InvalidLayout(format!("path has no parent: {}", path.display()))
    })?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            JournalError::InvalidLayout(format!("path has no file name: {}", path.display()))
        })?;
    let temporary = parent.join(format!(".{file_name}.{}.tmp", Uuid::new_v4()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)?;
        file.write_all(contents)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)?;
        Ok::<(), JournalError>(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
