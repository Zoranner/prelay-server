use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use rand::RngCore;

use super::StorageError;

#[derive(Clone)]
pub struct MasterKey([u8; 32]);

impl MasterKey {
    pub fn from_environment() -> Result<Self, StorageError> {
        let value = std::env::var("PROVIDER_RELAY_MASTER_KEY").map_err(|_| {
            StorageError::InvalidMasterKey("PROVIDER_RELAY_MASTER_KEY is required".to_string())
        })?;
        Self::from_base64(&value)
    }

    pub fn from_base64(value: &str) -> Result<Self, StorageError> {
        let decoded = STANDARD
            .decode(value)
            .map_err(|_| StorageError::InvalidMasterKey("must be valid Base64".to_string()))?;
        let bytes: [u8; 32] = decoded.try_into().map_err(|_: Vec<u8>| {
            StorageError::InvalidMasterKey("must decode to exactly 32 bytes".to_string())
        })?;
        Ok(Self(bytes))
    }

    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

#[derive(Clone)]
pub(crate) struct KeyCipher {
    cipher: Aes256Gcm,
}

impl KeyCipher {
    pub(crate) fn new(master_key: MasterKey) -> Self {
        Self {
            cipher: Aes256Gcm::new_from_slice(&master_key.0).expect("AES-256 key length is fixed"),
        }
    }

    pub(crate) fn encrypt(&self, plaintext: &str) -> Result<String, StorageError> {
        let mut nonce_bytes = [0_u8; 12];
        rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);
        let ciphertext = self
            .cipher
            .encrypt(Nonce::from_slice(&nonce_bytes), plaintext.as_bytes())
            .map_err(|_| StorageError::Crypto("unable to encrypt provider key".to_string()))?;
        let mut payload = nonce_bytes.to_vec();
        payload.extend(ciphertext);
        Ok(STANDARD.encode(payload))
    }

    pub(crate) fn decrypt(&self, value: &str) -> Result<String, StorageError> {
        let payload = STANDARD
            .decode(value)
            .map_err(|_| StorageError::Crypto("stored ciphertext is not Base64".to_string()))?;
        let (nonce, ciphertext) = payload.split_at_checked(12).ok_or_else(|| {
            StorageError::Crypto("stored ciphertext does not include a 96-bit nonce".to_string())
        })?;
        let plaintext = self
            .cipher
            .decrypt(Nonce::from_slice(nonce), ciphertext)
            .map_err(|_| StorageError::Crypto("unable to decrypt provider key".to_string()))?;
        String::from_utf8(plaintext)
            .map_err(|_| StorageError::Crypto("provider key is not UTF-8".to_string()))
    }
}
