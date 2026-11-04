use std::{
    fs::{self, OpenOptions},
    io,
    path::Path,
};

use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::{
    cache::{FileStamp, ThreadState},
    RolloutFile, RolloutSlice, RolloutStore,
};
use crate::domain::activity::rollout::{
    ConversationEvent, ConversationFormat, ConversationMeta, RolloutError,
};

pub(super) const MAX_RECOVERED_SEGMENTS: usize = 10;

enum ThreadCandidate {
    Missing,
    Corrupt,
    Events(ParsedConversation, FileStamp),
}

pub(super) struct ParsedConversation {
    pub(super) format: ConversationFormat,
    pub(super) meta: Option<ConversationMeta>,
    pub(super) events: Vec<ConversationEvent>,
    pub(super) line_count: u64,
    pub(super) last_seq: u64,
}

#[derive(Deserialize)]
struct StoredLine {
    seq: u64,
    #[serde(alias = "timestamp")]
    time: String,
    #[serde(rename = "type")]
    event_type: String,
    payload: Value,
}

impl RolloutStore {
    pub fn read_events(&self, relative_path: &str) -> Result<Vec<ConversationEvent>, RolloutError> {
        self.read_verified(relative_path).map(|file| file.events)
    }

    pub fn read_verified(&self, relative_path: &str) -> Result<RolloutFile, RolloutError> {
        let path = self.checked_absolute_path(relative_path)?;
        let contents = fs::read_to_string(&path)?;
        if !completed_contents(&contents) {
            let valid_events = valid_prefix_lines(&contents);
            return Err(RolloutError::PartialLine { path, valid_events });
        }
        let parsed = parse_conversation_contents(&contents)?;
        Ok(RolloutFile {
            format: parsed.format,
            meta: parsed.meta,
            event_count: parsed.line_count,
            byte_size: contents.len() as u64,
            sha256: format!("{:x}", Sha256::digest(contents.as_bytes())),
            events: parsed.events,
        })
    }

    pub fn read_slice(
        &self,
        relative_path: &str,
        start_line: u64,
        end_line: u64,
    ) -> Result<RolloutSlice, RolloutError> {
        let path = self.checked_absolute_path(relative_path)?;
        let contents = fs::read_to_string(&path)?;
        if !completed_contents(&contents) {
            let valid_events = valid_prefix_lines(&contents);
            return Err(RolloutError::PartialLine { path, valid_events });
        }
        let lines = contents.lines().collect::<Vec<_>>();
        if start_line == 0 {
            if end_line == 0 {
                return Ok(empty_slice());
            }
            return Err(RolloutError::InvalidPath(format!(
                "invalid rollout line range {start_line}..{end_line}"
            )));
        }
        if start_line > end_line {
            if start_line == end_line.saturating_add(1) && start_line <= lines.len() as u64 + 1 {
                return Ok(empty_slice());
            }
            return Err(RolloutError::InvalidPath(format!(
                "invalid rollout line range {start_line}..{end_line}"
            )));
        }
        if end_line > lines.len() as u64 || start_line > lines.len() as u64 + 1 {
            return Err(RolloutError::InvalidPath(format!(
                "rollout line range {start_line}..{end_line} exceeds {} lines",
                lines.len()
            )));
        }

        let selected = &lines[start_line as usize - 1..end_line as usize];
        let mut bytes = Vec::new();
        let mut events = Vec::new();
        for line in selected {
            bytes.extend_from_slice(line.as_bytes());
            bytes.push(b'\n');
            if let Some(event) = parse_event_line(line)? {
                events.push(event);
            }
        }
        Ok(RolloutSlice {
            event_count: selected.len() as u64,
            byte_size: bytes.len() as u64,
            sha256: format!("{:x}", Sha256::digest(&bytes)),
            events,
        })
    }

    pub(super) fn load_thread_source(
        &self,
        identity_id: &str,
        thread_id: &str,
    ) -> Result<(ThreadState, Vec<ConversationEvent>), RolloutError> {
        let base = format!("{identity_id}/{thread_id}.jsonl");
        match self.read_thread_candidate(&base)? {
            ThreadCandidate::Events(parsed, stamp) => {
                let state = ThreadState::from_events(
                    base,
                    stamp,
                    parsed.format,
                    &parsed.events,
                    parsed.line_count,
                    parsed.last_seq,
                )?;
                Ok((state, parsed.events))
            }
            ThreadCandidate::Missing => {
                for index in 1..=MAX_RECOVERED_SEGMENTS {
                    let candidate = recovered_thread_path(identity_id, thread_id, index);
                    match self.read_thread_candidate(&candidate)? {
                        ThreadCandidate::Events(parsed, stamp) => {
                            let state = ThreadState::from_events(
                                candidate,
                                stamp,
                                parsed.format,
                                &parsed.events,
                                parsed.line_count,
                                parsed.last_seq,
                            )?;
                            return Ok((state, parsed.events));
                        }
                        ThreadCandidate::Corrupt => continue,
                        ThreadCandidate::Missing if index == 1 => {
                            return Ok((ThreadState::empty(base), Vec::new()));
                        }
                        ThreadCandidate::Missing => {
                            return Ok((ThreadState::empty(candidate), Vec::new()));
                        }
                    }
                }
                Err(RolloutError::InvalidPath(format!(
                    "thread {thread_id} has too many corrupt recovered segments"
                )))
            }
            ThreadCandidate::Corrupt => {
                for index in 1..=MAX_RECOVERED_SEGMENTS {
                    let candidate = recovered_thread_path(identity_id, thread_id, index);
                    match self.read_thread_candidate(&candidate)? {
                        ThreadCandidate::Events(parsed, stamp) => {
                            let state = ThreadState::from_events(
                                candidate,
                                stamp,
                                parsed.format,
                                &parsed.events,
                                parsed.line_count,
                                parsed.last_seq,
                            )?;
                            return Ok((state, parsed.events));
                        }
                        ThreadCandidate::Corrupt => continue,
                        ThreadCandidate::Missing => {
                            return Ok((ThreadState::empty(candidate), Vec::new()));
                        }
                    }
                }
                Err(RolloutError::InvalidPath(format!(
                    "thread {thread_id} has too many corrupt segments"
                )))
            }
        }
    }

    fn read_thread_candidate(&self, relative: &str) -> Result<ThreadCandidate, RolloutError> {
        let path = self.checked_absolute_path(relative)?;
        match read_thread_events(&path) {
            Ok(Some(parsed)) => {
                let stamp = FileStamp::from_metadata(&fs::metadata(&path)?);
                Ok(ThreadCandidate::Events(parsed, stamp))
            }
            Ok(None) => Ok(ThreadCandidate::Missing),
            Err(RolloutError::Serialize(_)) => {
                tracing::warn!(
                    thread = %relative,
                    "activity rollout has a corrupt line; trying a recovered segment"
                );
                Ok(ThreadCandidate::Corrupt)
            }
            Err(error) => Err(error),
        }
    }
}

fn recovered_thread_path(identity_id: &str, thread_id: &str, index: usize) -> String {
    if index == 1 {
        format!("{identity_id}/{thread_id}.recovered.jsonl")
    } else {
        format!("{identity_id}/{thread_id}.recovered-{index}.jsonl")
    }
}

pub(super) fn thread_candidate_paths(identity_id: &str, thread_id: &str) -> Vec<String> {
    let mut paths = Vec::with_capacity(MAX_RECOVERED_SEGMENTS + 1);
    paths.push(format!("{identity_id}/{thread_id}.jsonl"));
    for index in 1..=MAX_RECOVERED_SEGMENTS {
        paths.push(recovered_thread_path(identity_id, thread_id, index));
    }
    paths
}

fn empty_slice() -> RolloutSlice {
    RolloutSlice {
        events: Vec::new(),
        event_count: 0,
        byte_size: 0,
        sha256: format!("{:x}", Sha256::digest(b"")),
    }
}

pub(super) fn completed_contents(contents: &str) -> bool {
    contents.ends_with('\n') || contents.is_empty()
}

pub(super) fn read_thread_events(path: &Path) -> Result<Option<ParsedConversation>, RolloutError> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if completed_contents(&contents) {
        return parse_conversation_contents(&contents).map(Some);
    }

    let valid_len = contents.rfind('\n').map_or(0, |index| index + 1);
    let parsed = parse_conversation_contents(&contents[..valid_len])?;
    let dropped_bytes = contents.len() - valid_len;
    let file = OpenOptions::new().write(true).open(path)?;
    file.set_len(valid_len as u64)?;
    file.sync_all()?;
    tracing::warn!(
        path = %path.display(),
        dropped_bytes,
        "repaired an incomplete activity rollout tail"
    );
    Ok(Some(parsed))
}

fn parse_conversation_contents(contents: &str) -> Result<ParsedConversation, RolloutError> {
    let lines = contents.lines().collect::<Vec<_>>();
    let mut format = ConversationFormat::Legacy;
    let mut meta = None;
    let mut start = 0;
    if let Some(first) = lines.first() {
        let value = serde_json::from_str::<Value>(first)?;
        if value.get("type").and_then(Value::as_str) == Some("meta") {
            if value.get("v").and_then(Value::as_u64) != Some(1) {
                return Err(RolloutError::InvalidPath(
                    "unsupported conversation file version".to_string(),
                ));
            }
            let parsed = serde_json::from_value::<ConversationMeta>(value)?;
            if parsed.kind != "meta" || parsed.thread_id.is_empty() {
                return Err(RolloutError::InvalidPath(
                    "invalid conversation file header".to_string(),
                ));
            }
            meta = Some(parsed);
            format = ConversationFormat::V1;
            start = 1;
        }
    }

    let mut events = Vec::new();
    let mut last_seq = 0;
    for line in &lines[start..] {
        if let Some(event) = parse_event_line(line)? {
            last_seq = last_seq.max(event.seq);
            events.push(event);
        }
    }
    Ok(ParsedConversation {
        format,
        meta,
        events,
        line_count: lines.len() as u64,
        last_seq,
    })
}

fn parse_event_line(line: &str) -> Result<Option<ConversationEvent>, RolloutError> {
    let value = serde_json::from_str::<Value>(line)?;
    if value.get("type").and_then(Value::as_str) == Some("meta") {
        return Ok(None);
    }
    normalize_event(serde_json::from_value::<StoredLine>(value)?)
}

fn normalize_event(line: StoredLine) -> Result<Option<ConversationEvent>, RolloutError> {
    let payload = match line.event_type.as_str() {
        "session_meta" | "media" => return Ok(None),
        "window_start" => json!({
            "window_id": line.payload.get("window_id").cloned(),
            "generation": line.payload.get("generation").cloned().unwrap_or(json!(0)),
        }),
        "session_end" => json!({
            "status": line.payload.get("status").cloned().unwrap_or(Value::Null),
            "http_status": line.payload.get("http_status").cloned(),
            "error_code": line.payload.get("error_code").cloned(),
        }),
        "meta" => {
            return Err(RolloutError::InvalidPath(
                "conversation header must be the first line".to_string(),
            ));
        }
        _ => line.payload,
    };
    let event_type = match line.event_type.as_str() {
        "window_start" => "window",
        "session_end" => "turn_end",
        other => other,
    };
    Ok(Some(ConversationEvent {
        seq: line.seq,
        time: line.time,
        event_type: event_type.to_string(),
        payload,
    }))
}

fn valid_prefix_lines(contents: &str) -> usize {
    contents
        .lines()
        .take_while(|line| {
            if line.contains("\"type\":\"meta\"") {
                return serde_json::from_str::<Value>(line).is_ok();
            }
            serde_json::from_str::<StoredLine>(line).is_ok()
        })
        .count()
}
