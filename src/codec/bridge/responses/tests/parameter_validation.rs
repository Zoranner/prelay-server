use axum::http::StatusCode;
use serde_json::json;

use super::super::decode_responses_request;
use crate::error::AppError;

#[test]
fn rejects_previous_response_and_conversation_together() {
    let error = decode_responses_request(&json!({
        "model": "gpt-test",
        "previous_response_id": "resp_parent",
        "conversation": "conv_1",
        "input": "hello"
    }))
    .expect_err("mutually exclusive parameters must fail");

    let AppError::OpenAi {
        status,
        error_type,
        code,
        param,
        ..
    } = error
    else {
        panic!("expected OpenAI-style error");
    };
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error_type, "invalid_request_error");
    assert_eq!(code, "invalid_request_error");
    assert_eq!(param, Some("conversation"));
}

#[test]
fn rejects_conversation_string_and_object_as_unsupported() {
    for conversation in [json!("conv_1"), json!({ "id": "conv_1" })] {
        let error = decode_responses_request(&json!({
            "model": "gpt-test",
            "conversation": conversation,
            "input": "hello"
        }))
        .expect_err("conversation is not implemented yet");

        let AppError::OpenAi { code, param, .. } = error else {
            panic!("expected OpenAI-style error");
        };
        assert_eq!(code, "unsupported_parameter");
        assert_eq!(param, Some("conversation"));
    }
}

#[test]
fn defaults_store_to_true_and_rejects_non_boolean_store() {
    let request = decode_responses_request(&json!({
        "model": "gpt-test",
        "input": "hello"
    }))
    .expect("decode request");
    assert!(request.store);

    let error = decode_responses_request(&json!({
        "model": "gpt-test",
        "input": "hello",
        "store": "false"
    }))
    .expect_err("store must be a boolean or null");
    assert!(matches!(
        error,
        AppError::OpenAi {
            status: StatusCode::BAD_REQUEST,
            param: Some("store"),
            ..
        }
    ));
}

#[test]
fn rejects_invalid_previous_response_id_and_stream_types() {
    let error = decode_responses_request(&json!({
        "model": "gpt-test",
        "input": "hello",
        "previous_response_id": 42
    }))
    .expect_err("invalid previous_response_id type must fail");
    assert!(matches!(
        error,
        AppError::OpenAi {
            status: StatusCode::BAD_REQUEST,
            param: Some("previous_response_id"),
            ..
        }
    ));

    let error = decode_responses_request(&json!({
        "model": "gpt-test",
        "input": "hello",
        "stream": "true"
    }))
    .expect_err("invalid stream type must fail");
    assert!(matches!(
        error,
        AppError::OpenAi {
            status: StatusCode::BAD_REQUEST,
            param: Some("stream"),
            ..
        }
    ));

    let error = decode_responses_request(&json!({
        "model": "gpt-test",
        "input": "hello",
        "previous_response_id": ""
    }))
    .expect_err("empty previous_response_id must fail");
    assert!(matches!(
        error,
        AppError::OpenAi {
            param: Some("previous_response_id"),
            ..
        }
    ));
}
