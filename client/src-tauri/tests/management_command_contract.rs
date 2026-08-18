use provider_relay_client::commands::{
    interfaces::InterfaceSaveInput, providers::ProviderSaveInput,
};
use provider_relay_protocol::{RotateCredentialRequest, TestProviderProtocolRequest};

#[test]
fn management_command_inputs_never_accept_identity_or_device_credentials() {
    let provider = serde_json::to_value(ProviderSaveInput::default()).expect("serialize provider");
    let interface =
        serde_json::to_value(InterfaceSaveInput::default()).expect("serialize interface");
    let protocol = serde_json::to_value(TestProviderProtocolRequest {
        protocol: String::new(),
        model: None,
    })
    .expect("serialize protocol test");

    for value in [provider, interface, protocol] {
        assert!(value.get("identity_id").is_none());
        assert!(value.get("device_credential").is_none());
        assert!(value.get("api_key_masked").is_none());
        assert!(value.get("current").is_none());
        assert!(value.get("pending").is_none());
    }
}

#[test]
fn rotation_request_only_carries_the_next_credential() {
    let request = serde_json::to_value(RotateCredentialRequest {
        new_credential: "next-device-credential".into(),
    })
    .expect("serialize rotation request");

    assert_eq!(request["new_credential"], "next-device-credential");
    assert!(request.get("credential").is_none());
    assert!(request.get("current").is_none());
    assert!(request.get("pending").is_none());
}
