use serde::Serialize;
use sha2::{Digest, Sha256};

use super::redaction::redact_sensitive_text;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NormalizedActivityContent {
    pub input_text: String,
    pub output_text: String,
    pub media_metadata_json: Option<String>,
    pub is_truncated: bool,
    pub content_hash: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActivityContentDraft {
    pub activity_id: String,
    pub input_text: String,
    pub output_text: String,
    pub media_metadata_json: Option<String>,
    pub is_truncated: bool,
    pub content_hash: String,
}

impl NormalizedActivityContent {
    pub fn into_draft(self, activity_id: String) -> ActivityContentDraft {
        ActivityContentDraft {
            activity_id,
            input_text: self.input_text,
            output_text: self.output_text,
            media_metadata_json: self.media_metadata_json,
            is_truncated: self.is_truncated,
            content_hash: self.content_hash,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActivityMediaMetadata {
    pub media_type: String,
    pub size_bytes: usize,
    pub sha256: String,
    pub extracted_text: Option<String>,
}

pub fn media_metadata_from_bytes(media_type: &str, bytes: &[u8]) -> ActivityMediaMetadata {
    ActivityMediaMetadata {
        media_type: normalize_media_type(media_type),
        size_bytes: bytes.len(),
        sha256: format!("{:x}", Sha256::digest(bytes)),
        extracted_text: None,
    }
}

fn normalize_media_type(value: &str) -> String {
    let media_type = value
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let Some((type_name, subtype)) = media_type.split_once('/') else {
        return "application/octet-stream".to_string();
    };
    if type_name.is_empty()
        || subtype.is_empty()
        || media_type.matches('/').count() != 1
        || !media_type.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#' | b'$' | b'&' | b'^' | b'_' | b'.' | b'+' | b'-' | b'/'
                )
        })
    {
        return "application/octet-stream".to_string();
    }
    media_type
}

#[derive(Serialize)]
struct StoredMediaMetadata<'a> {
    media_type: &'a str,
    size_bytes: usize,
    sha256: &'a str,
}

pub fn activity_content_from_text(
    input_text: &str,
    output_text: &str,
    max_bytes: usize,
) -> Option<NormalizedActivityContent> {
    activity_content_from_text_with_media(input_text, output_text, None, max_bytes)
}

pub fn activity_content_from_text_with_media(
    input_text: &str,
    output_text: &str,
    media: Option<ActivityMediaMetadata>,
    max_bytes: usize,
) -> Option<NormalizedActivityContent> {
    let input_text = normalize_text(input_text);
    let mut output_text = normalize_text(output_text);
    let media_metadata_json = media.as_ref().map(|media| {
        if let Some(extracted_text) = media.extracted_text.as_deref() {
            output_text = join_text(&output_text, &normalize_text(extracted_text));
        }
        serde_json::to_string(&StoredMediaMetadata {
            media_type: &media.media_type,
            size_bytes: media.size_bytes,
            sha256: &media.sha256,
        })
        .expect("media metadata serializes")
    });

    if input_text.is_empty() && output_text.is_empty() {
        return None;
    }

    let (input_text, input_truncated, remaining) = truncate_utf8(&input_text, max_bytes);
    let (output_text, output_truncated, _) = truncate_utf8(&output_text, remaining);
    let is_truncated = input_truncated || output_truncated;
    let content_hash = content_hash(&input_text, &output_text, media_metadata_json.as_deref());

    Some(NormalizedActivityContent {
        input_text,
        output_text,
        media_metadata_json,
        is_truncated,
        content_hash,
    })
}

fn normalize_text(value: &str) -> String {
    let redacted = redact_sensitive_text(value);
    let normalized_lines = redacted
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .lines()
        .map(|line| redact_sensitive_text(line.trim()))
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    normalized_lines.join("\n")
}

fn join_text(left: &str, right: &str) -> String {
    match (left.is_empty(), right.is_empty()) {
        (true, _) => right.to_string(),
        (_, true) => left.to_string(),
        (false, false) => format!("{left}\n{right}"),
    }
}

fn truncate_utf8(value: &str, max_bytes: usize) -> (String, bool, usize) {
    if value.len() <= max_bytes {
        return (value.to_string(), false, max_bytes - value.len());
    }

    let mut end = 0;
    for (index, character) in value.char_indices() {
        let next_end = index + character.len_utf8();
        if next_end > max_bytes {
            break;
        }
        end = next_end;
    }
    (value[..end].to_string(), true, max_bytes - end)
}

fn content_hash(input_text: &str, output_text: &str, media_metadata_json: Option<&str>) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input_text.as_bytes());
    hasher.update([0]);
    hasher.update(output_text.as_bytes());
    hasher.update([0]);
    if let Some(media_metadata_json) = media_metadata_json {
        hasher.update(media_metadata_json.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}
