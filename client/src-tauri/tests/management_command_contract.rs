use provider_relay_client::commands::{
    interfaces::InterfaceSaveInput,
    providers::{ProviderSaveInput, ProviderTestProtocolInput},
};

#[test]
fn management_command_inputs_never_accept_identity_or_device_credentials() {
    let provider = serde_json::to_value(ProviderSaveInput::default()).expect("serialize provider");
    let interface =
        serde_json::to_value(InterfaceSaveInput::default()).expect("serialize interface");
    let protocol = serde_json::to_value(ProviderTestProtocolInput::default())
        .expect("serialize protocol test");

    for value in [provider, interface, protocol] {
        assert!(value.get("identity_id").is_none());
        assert!(value.get("device_credential").is_none());
        assert!(value.get("api_key_masked").is_none());
    }
}
