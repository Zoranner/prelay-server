use sha2::{Digest, Sha256};

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

#[cfg(test)]
mod tests {
    use super::media_metadata_from_bytes;

    #[test]
    fn image_metadata_contains_only_type_size_and_hash() {
        let media =
            media_metadata_from_bytes("image/png; endpoint_token=must-not-be-captured", b"abc");

        assert_eq!(media.media_type, "image/png");
        assert_eq!(media.size_bytes, 3);
        assert_eq!(
            media.sha256,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert!(media.extracted_text.is_none());
    }
}
