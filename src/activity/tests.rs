use serde_json::json;

use super::{
    activity_content_from_text, activity_content_from_text_with_media, anthropic_message_text,
    anthropic_request_text, chat_message_text, chat_response_text, media_metadata_from_bytes,
    ActivityMediaMetadata, RawStreamContentCapture, RawStreamProtocol,
};

#[test]
fn activity_content_redacts_known_credential_forms() {
    let draft = activity_content_from_text(
        "Authorization: Bearer sk-live-secret\napi_key=provider-secret\nendpoint_token: endpoint-secret\ndevice_credential=device-secret",
        "Bearer response-secret",
        4_096,
    )
    .expect("content draft");

    let content = format!("{}\n{}", draft.input_text, draft.output_text);
    assert!(!content.contains("sk-live-secret"));
    assert!(!content.contains("provider-secret"));
    assert!(!content.contains("endpoint-secret"));
    assert!(!content.contains("device-secret"));
    assert!(!content.contains("response-secret"));
    assert!(content.contains("[REDACTED]"));
}

#[test]
fn activity_content_redacts_unlabeled_and_extended_credential_forms() {
    let endpoint_token = "a".repeat(43);
    let input = format!(
        "password=hunter2\nclient_secret=client-secret\nsk-proj-live-secret-value\n{endpoint_token}"
    );
    let output = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJ1c2VyIn0.signature";
    let draft = activity_content_from_text(&input, output, 4_096).expect("content draft");
    let content = format!("{}\n{}", draft.input_text, draft.output_text);

    for secret in [
        "hunter2",
        "client-secret",
        "sk-proj-live-secret-value",
        endpoint_token.as_str(),
        output,
    ] {
        assert!(!content.contains(secret), "stored credential: {secret}");
    }
    assert!(content.matches("[REDACTED]").count() >= 5);
}

#[test]
fn activity_content_redacts_complete_multiline_private_key_blocks() {
    let private_key_body = "MIIEvQIBADANBgkqhkiG9w0BAQEFAASC";
    let input = format!(
        "private_key: -----BEGIN PRIVATE KEY-----\n{private_key_body}\n-----END PRIVATE KEY-----"
    );
    let draft = activity_content_from_text(&input, "", 4_096).expect("content draft");

    assert!(!draft.input_text.contains(private_key_body));
    assert!(!draft.input_text.contains("BEGIN PRIVATE KEY"));
    assert_eq!(draft.input_text, "private_key: [REDACTED]");
}

#[test]
fn activity_content_redacts_url_safe_token_edges_and_pgp_private_keys() {
    let leading_hyphen_token = format!("-{}", "a".repeat(42));
    let trailing_hyphen_token = format!("{}-", "b".repeat(42));
    let pgp_body = "mQINBGQAAAEBCAC7Yy4zQw7qfSxZk4hwEXAMPLEPRIVATEKEYBASE64";
    let input = format!(
        "{leading_hyphen_token}\n{trailing_hyphen_token}\n-----BEGIN PGP PRIVATE KEY BLOCK-----\n{pgp_body}\n-----END PGP PRIVATE KEY BLOCK-----"
    );
    let draft = activity_content_from_text(&input, "", 4_096).expect("content draft");

    for secret in [
        leading_hyphen_token.as_str(),
        trailing_hyphen_token.as_str(),
        pgp_body,
        "BEGIN PGP PRIVATE KEY BLOCK",
    ] {
        assert!(
            !draft.input_text.contains(secret),
            "stored credential: {secret}"
        );
    }
}

#[test]
fn activity_content_truncates_on_utf8_boundaries() {
    let draft = activity_content_from_text("hello", "你好世界", 11).expect("content draft");

    assert!(draft.is_truncated);
    assert_eq!(draft.input_text, "hello");
    assert_eq!(draft.output_text, "你好");
    assert!(draft.input_text.len() + draft.output_text.len() <= 11);
}

#[test]
fn activity_content_uses_a_stable_hash_for_normalized_text() {
    let first = activity_content_from_text("  hello\r\nworld  ", " answer ", 4_096)
        .expect("first content draft");
    let second =
        activity_content_from_text("hello\nworld", "answer", 4_096).expect("second content draft");

    assert_eq!(first.content_hash, second.content_hash);
    assert_eq!(first.input_text, "hello\nworld");
    assert_eq!(first.output_text, "answer");
}

#[test]
fn activity_content_keeps_only_image_metadata_and_extracted_text() {
    let draft = activity_content_from_text_with_media(
        "draw a lighthouse",
        "",
        Some(ActivityMediaMetadata {
            media_type: "image/png".to_string(),
            size_bytes: 1024,
            sha256: "a".repeat(64),
            extracted_text: Some("visible label".to_string()),
        }),
        4_096,
    )
    .expect("content draft");

    assert_eq!(draft.output_text, "visible label");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(
            draft
                .media_metadata_json
                .as_deref()
                .expect("media metadata"),
        )
        .expect("decode media metadata"),
        json!({
            "media_type": "image/png",
            "size_bytes": 1024,
            "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        })
    );
    assert!(!draft
        .media_metadata_json
        .as_deref()
        .expect("media metadata")
        .contains("visible label"));
}

#[test]
fn activity_content_skips_empty_text_without_media() {
    assert!(activity_content_from_text("  \n", "\t", 4_096).is_none());
}

#[test]
fn chat_text_extraction_uses_only_message_content() {
    let request = json!({
        "messages": [{
            "role": "user",
            "content": [
                { "type": "text", "text": "keep this" },
                { "type": "image_url", "image_url": { "url": "data:image/png;base64,secret" } }
            ]
        }],
        "api_key": "must-not-be-captured"
    });
    let response = json!({
        "choices": [{ "message": { "content": "answer" } }],
        "metadata": { "endpoint_token": "must-not-be-captured" }
    });

    assert_eq!(chat_message_text(&request), "keep this");
    assert_eq!(chat_response_text(&response), "answer");
}

#[test]
fn anthropic_text_extraction_uses_only_declared_text_parts() {
    let message = json!({
        "content": [
            { "type": "text", "text": "visible text" },
            { "type": "image", "source": { "data": "base64-secret" } }
        ],
        "x-api-key": "must-not-be-captured"
    });

    assert_eq!(anthropic_message_text(&message), "visible text");
}

#[test]
fn anthropic_request_text_uses_system_and_message_text_parts() {
    let request = json!({
        "system": "system rule",
        "messages": [{ "role": "user", "content": "user question" }],
        "metadata": { "api_key": "must-not-be-captured" }
    });

    assert_eq!(
        anthropic_request_text(&request),
        "system rule\nuser question"
    );
}

#[test]
fn image_metadata_contains_only_type_size_and_hash() {
    let media = media_metadata_from_bytes("image/png; endpoint_token=must-not-be-captured", b"abc");

    assert_eq!(media.media_type, "image/png");
    assert_eq!(media.size_bytes, 3);
    assert_eq!(
        media.sha256,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert!(media.extracted_text.is_none());
}

#[test]
fn raw_chat_stream_capture_joins_split_text_and_waits_for_done() {
    let mut capture = RawStreamContentCapture::new(RawStreamProtocol::ChatCompletions);

    capture.observe_chunk(b"data: {\"choices\":[{\"delta\":{\"content\":\"hel\"}}]}\n\n");
    capture.observe_chunk(
        b"data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\ndata: [DONE]\n\n",
    );
    capture.finish();

    assert!(capture.is_completed());
    assert_eq!(capture.output_text(), "hello");
}

#[test]
fn raw_anthropic_and_image_stream_captures_keep_only_text() {
    let mut anthropic = RawStreamContentCapture::new(RawStreamProtocol::AnthropicMessages);
    anthropic.observe_chunk(
        b"event: content_block_delta\ndata: {\"delta\":{\"type\":\"text_delta\",\"text\":\"hello\"}}\n\nevent: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
    );
    anthropic.finish();

    let mut image = RawStreamContentCapture::new(RawStreamProtocol::ImageGeneration);
    image.observe_chunk(b"data: {\"type\":\"image_generation.partial_image\",\"b64_json\":\"do-not-store\"}\n\ndata: {\"type\":\"image_generation.completed\"}\n\n");
    image.finish();

    assert!(anthropic.is_completed());
    assert_eq!(anthropic.output_text(), "hello");
    assert!(image.is_completed());
    assert!(image.output_text().is_empty());
}
