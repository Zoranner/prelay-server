use crate::store::storage::{crypto::KeyCipher, StorageError};

const SESSION_CIPHERTEXT_PREFIX: &str = "enc:v1:";

pub(super) fn encrypt_session_value(
    crypto: &KeyCipher,
    value: &str,
) -> Result<String, StorageError> {
    let ciphertext = crypto.encrypt(value)?;
    Ok(format!("{SESSION_CIPHERTEXT_PREFIX}{ciphertext}"))
}

pub(super) fn decrypt_session_value(
    crypto: &KeyCipher,
    value: &str,
) -> Result<String, StorageError> {
    let ciphertext = value
        .strip_prefix(SESSION_CIPHERTEXT_PREFIX)
        .ok_or_else(|| {
            StorageError::InvalidStoredState("response state value is not encrypted".to_string())
        })?;
    crypto.decrypt(ciphertext)
}

#[cfg(test)]
mod tests {
    use super::{decrypt_session_value, encrypt_session_value};
    use crate::store::storage::{crypto::KeyCipher, MasterKey, StorageError};

    fn cipher() -> KeyCipher {
        KeyCipher::new(MasterKey::from_bytes([0; 32]))
    }

    #[test]
    fn rejects_plaintext_session_values() {
        let error =
            decrypt_session_value(&cipher(), "plain-json").expect_err("plaintext must be rejected");

        assert!(matches!(error, StorageError::InvalidStoredState(_)));
    }

    #[test]
    fn decrypts_prefixed_session_values() {
        let cipher = cipher();
        let value = encrypt_session_value(&cipher, "secret").expect("encrypt value");

        assert_eq!(
            decrypt_session_value(&cipher, &value).expect("decrypt value"),
            "secret"
        );
    }
}
