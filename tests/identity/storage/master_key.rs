use prelay_server::storage::MasterKey;

use crate::support;

#[test]
fn master_key_requires_base64_encoded_32_bytes() {
    assert!(MasterKey::from_base64("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=").is_ok());
    assert!(MasterKey::from_base64("not base64").is_err());
    assert!(MasterKey::from_base64("AAAA").is_err());
}

#[test]
fn master_key_environment_requires_a_valid_base64_encoded_32_byte_value() {
    let _restore = support::EnvironmentVariableRestore::capture("ENCRYPTION_KEY");

    std::env::remove_var("ENCRYPTION_KEY");
    assert!(MasterKey::from_environment().is_err());
    std::env::set_var("ENCRYPTION_KEY", "not base64");
    assert!(MasterKey::from_environment().is_err());
    std::env::set_var("ENCRYPTION_KEY", "AAAA");
    assert!(MasterKey::from_environment().is_err());
}
