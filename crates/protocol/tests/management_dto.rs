use std::fmt::Debug;

use provider_relay_protocol::{
    interfaces::{InterfaceModelResponse, UpdateInterfaceRequest},
    providers::ProviderModelResponse,
    stats::{ModelStatsSummary, ProviderStatsSummary, RequestLogSummary, StatsOverview},
};
use provider_relay_protocol::{
    CreateIdentityRequest, CreateIdentityResponse, CreateInterfaceRequest, CreateProviderRequest,
    InterfaceModelInput, InterfaceResponse, ProtocolErrorCode, ProviderCapabilityOverrides,
    ProviderProtocolBaseUrls, ProviderResponse, RotateCredentialResponse, UpdateProviderRequest,
};
use serde::{de::DeserializeOwned, Serialize};

fn assert_json_round_trip<T>(value: T)
where
    T: Debug + DeserializeOwned + PartialEq + Serialize,
{
    let json = serde_json::to_value(&value).unwrap();
    assert_eq!(serde_json::from_value::<T>(json).unwrap(), value);
}

fn capabilities() -> ProviderCapabilityOverrides {
    ProviderCapabilityOverrides {
        upstream_protocols: Some(vec!["openai".into(), "anthropic".into()]),
        protocol_base_urls: Some(ProviderProtocolBaseUrls {
            responses: None,
            openai: Some("https://api.deepseek.com/v1".into()),
            anthropic: None,
        }),
        tool_calls: Some(true),
        ..Default::default()
    }
}

#[test]
fn management_requests_round_trip_without_client_identity_id() {
    let register = CreateIdentityRequest {
        machine_id: "machine-a".into(),
        account_sid: "S-1-5-21-100".into(),
    };
    let provider = CreateProviderRequest {
        name: "DeepSeek".into(),
        provider_type: "openai_compatible".into(),
        base_url: "https://api.deepseek.com".into(),
        api_key: "sk-test".into(),
        capabilities: Some(capabilities()),
        models: vec!["deepseek-chat".into()],
    };
    let update = UpdateProviderRequest {
        name: Some("DeepSeek Production".into()),
        capabilities: Some(capabilities()),
        ..Default::default()
    };
    let interface = CreateInterfaceRequest {
        name: "OpenAI tools".into(),
        protocol: Some("openai".into()),
        models: vec![InterfaceModelInput {
            provider_id: "provider-a".into(),
            upstream_model: "deepseek-chat".into(),
            model_name: Some("assistant".into()),
        }],
    };
    let interface_update = UpdateInterfaceRequest {
        name: Some("OpenAI tools production".into()),
        protocol: Some("responses".into()),
        models: Some(vec![InterfaceModelInput {
            provider_id: "provider-a".into(),
            upstream_model: "deepseek-reasoner".into(),
            model_name: Some("reasoner".into()),
        }]),
    };
    let empty_interface_update = UpdateInterfaceRequest::default();

    assert_json_round_trip(register.clone());
    assert_json_round_trip(provider.clone());
    assert_json_round_trip(update.clone());
    assert_json_round_trip(interface);
    assert_json_round_trip(interface_update);
    assert_json_round_trip(empty_interface_update.clone());

    assert!(serde_json::to_value(register)
        .unwrap()
        .get("identity_id")
        .is_none());
    assert!(serde_json::to_value(&provider)
        .unwrap()
        .get("identity_id")
        .is_none());
    assert_eq!(
        serde_json::to_value(provider).unwrap()["capabilities"]["protocol_base_urls"]["openai"],
        "https://api.deepseek.com/v1"
    );
    assert_eq!(
        serde_json::to_value(update).unwrap()["api_key"],
        serde_json::Value::Null
    );
    let empty_interface_update_json = serde_json::to_value(empty_interface_update).unwrap();
    assert_eq!(empty_interface_update_json["name"], serde_json::Value::Null);
    assert_eq!(
        empty_interface_update_json["protocol"],
        serde_json::Value::Null
    );
    assert_eq!(
        empty_interface_update_json["models"],
        serde_json::Value::Null
    );
    assert_eq!(
        InterfaceModelInput::default_model_name("upstream"),
        "upstream"
    );
}

#[test]
fn management_responses_and_stats_round_trip() {
    assert_json_round_trip(CreateIdentityResponse {
        identity_id: "identity-a".into(),
        credential: "credential-once".into(),
    });
    assert_json_round_trip(RotateCredentialResponse {
        credential: "rotated-credential-once".into(),
    });
    assert_json_round_trip(ProviderResponse {
        id: "provider-a".into(),
        name: "DeepSeek".into(),
        provider_type: "openai_compatible".into(),
        base_url: "https://api.deepseek.com".into(),
        api_key_masked: "sk-t...test".into(),
        capabilities: capabilities(),
        models: vec![ProviderModelResponse {
            id: "model-a".into(),
            provider_id: "provider-a".into(),
            model_name: "deepseek-chat".into(),
            created_at: "2026-08-13T00:00:00Z".into(),
        }],
        created_at: "2026-08-13T00:00:00Z".into(),
    });
    assert_json_round_trip(InterfaceResponse {
        id: "interface-a".into(),
        name: "OpenAI tools".into(),
        protocol: "openai".into(),
        token: "interface-token".into(),
        models: vec![InterfaceModelResponse {
            id: "interface-model-a".into(),
            interface_id: "interface-a".into(),
            model_name: "assistant".into(),
            provider_id: "provider-a".into(),
            upstream_model: "deepseek-chat".into(),
            created_at: "2026-08-13T00:00:00Z".into(),
        }],
        created_at: "2026-08-13T00:00:00Z".into(),
    });
    assert_json_round_trip(StatsOverview {
        total_requests: 12,
        successful_requests: 10,
        failed_requests: 2,
        input_tokens: 123,
        output_tokens: 456,
    });
    assert_json_round_trip(RequestLogSummary {
        id: "request-a".into(),
        created_at: "2026-08-13T00:00:00Z".into(),
        protocol_in: Some("responses".into()),
        protocol_upstream: None,
        provider_name: Some("DeepSeek".into()),
        model_requested: Some("assistant".into()),
        status: "success".into(),
        http_status: Some(200),
        error_code: None,
        error_message: None,
        input_tokens: Some(123),
        output_tokens: Some(456),
        latency_ms: Some(789),
        upstream_request_id: None,
        metadata_json: None,
    });
    assert_json_round_trip(ModelStatsSummary {
        model_requested: Some("assistant".into()),
        total_requests: 12,
        successful_requests: 10,
        failed_requests: 2,
        input_tokens: 123,
        output_tokens: 456,
        estimated_cost: Some(0.12),
        average_latency_ms: Some(789.0),
    });
    assert_json_round_trip(ProviderStatsSummary {
        provider_id: Some("provider-a".into()),
        provider_name: Some("DeepSeek".into()),
        total_requests: 12,
        successful_requests: 10,
        failed_requests: 2,
        input_tokens: 123,
        output_tokens: 456,
        estimated_cost: Some(0.12),
        average_latency_ms: Some(789.0),
        average_first_token_ms: None,
    });
}

#[test]
fn protocol_error_code_uses_stable_snake_case_json() {
    let code = ProtocolErrorCode::IdentityAlreadyRegistered;

    assert_eq!(
        serde_json::to_value(code).unwrap(),
        "identity_already_registered"
    );
    assert_eq!(
        serde_json::from_value::<ProtocolErrorCode>("identity_already_registered".into()).unwrap(),
        code
    );
    assert_eq!(code.as_str(), "identity_already_registered");
}
