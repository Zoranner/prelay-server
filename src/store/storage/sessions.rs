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
    match value.strip_prefix(SESSION_CIPHERTEXT_PREFIX) {
        Some(ciphertext) => crypto.decrypt(ciphertext),
        None => Ok(value.to_string()),
    }
}

pub(super) fn is_encrypted_session_value(value: &str) -> bool {
    value.starts_with(SESSION_CIPHERTEXT_PREFIX)
}
