use provider_relay_protocol::{
    CreateIdentityRequest, CreateProviderRequest, InterfaceModelInput, ProtocolErrorCode,
    ProviderCapabilityOverrides, ProviderProtocolBaseUrls,
};

#[test]
fn management_dtos_round_trip_without_identity_id_from_client() {
    let register = CreateIdentityRequest {
        machine_id: "machine-a".into(),
        account_sid: "S-1-5-21-100".into(),
    };
    let provider = CreateProviderRequest {
        name: "DeepSeek".into(),
        provider_type: "openai_compatible".into(),
        base_url: "https://api.deepseek.com".into(),
        api_key: "sk-test".into(),
        capabilities: Some(ProviderCapabilityOverrides {
            upstream_protocols: Some(vec!["openai".into(), "anthropic".into()]),
            protocol_base_urls: Some(ProviderProtocolBaseUrls {
                responses: None,
                openai: Some("https://api.deepseek.com/v1".into()),
                anthropic: None,
            }),
            tool_calls: Some(true),
            ..Default::default()
        }),
        models: vec!["deepseek-chat".into()],
    };

    assert_eq!(
        serde_json::to_value(register).unwrap()["machine_id"],
        "machine-a"
    );
    assert!(serde_json::to_value(&provider)
        .unwrap()
        .get("identity_id")
        .is_none());
    assert_eq!(
        serde_json::to_value(&provider).unwrap()["capabilities"]["protocol_base_urls"]["openai"],
        "https://api.deepseek.com/v1"
    );
    assert_eq!(
        InterfaceModelInput::default_model_name("upstream"),
        "upstream"
    );
    assert_eq!(
        ProtocolErrorCode::IdentityAlreadyRegistered.as_str(),
        "identity_already_registered"
    );
}
