use provider_relay_client::{
    commands::bootstrap::collect_bootstrap,
    credential_store::MemoryCredentialStore,
    identity::{IdentitySource, WindowsIdentity},
};

struct FakeWindowsIdentity {
    identity: WindowsIdentity,
}

impl FakeWindowsIdentity {
    fn new(machine_id: &str, account_sid: &str) -> Self {
        Self {
            identity: WindowsIdentity {
                machine_id: machine_id.into(),
                account_sid: account_sid.into(),
                username: "Ada".into(),
            },
        }
    }
}

impl IdentitySource for FakeWindowsIdentity {
    fn identity(&self) -> Result<WindowsIdentity, String> {
        Ok(self.identity.clone())
    }
}

#[test]
fn bootstrap_uses_windows_identity_and_never_exposes_credential() {
    let identity = FakeWindowsIdentity::new("machine-a", "S-1-5-21-100");
    let credentials = MemoryCredentialStore::with_secret("device-secret");

    let response = collect_bootstrap(&identity, &credentials).unwrap();

    assert_eq!(response.machine_id, "machine-a");
    assert_eq!(response.account_sid, "S-1-5-21-100");
    assert_eq!(response.relay_url, "https://relay.rd.kim");
    assert!(response.has_device_credential);
    assert!(serde_json::to_value(response)
        .unwrap()
        .get("device_credential")
        .is_none());
}
