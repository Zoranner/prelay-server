use aes_gcm::{
    aead::{Aead, KeyInit, Payload},
    Aes256Gcm, Nonce,
};
use hkdf::Hkdf;
use rand::RngCore;
use sha2::Sha256;

use super::JournalError;
use crate::store::storage::MasterKey;

const SALT: &[u8] = b"prelay-recording-wal-salt";
const INFO: &[u8] = b"prelay-recording-wal-v1";

#[derive(Clone)]
pub(crate) struct JournalCipher {
    cipher: Aes256Gcm,
}

pub(crate) struct SealedPayload {
    pub(crate) nonce: [u8; 12],
    pub(crate) ciphertext: Vec<u8>,
}

impl JournalCipher {
    pub(crate) fn new(master_key: &MasterKey) -> Self {
        let mut key = [0_u8; 32];
        Hkdf::<Sha256>::new(Some(SALT), master_key.as_bytes())
            .expand(INFO, &mut key)
            .expect("32-byte HKDF output is valid");
        Self {
            cipher: Aes256Gcm::new_from_slice(&key).expect("AES-256 key length is fixed"),
        }
    }

    pub(crate) fn seal(&self, plaintext: &[u8], aad: &[u8]) -> Result<SealedPayload, JournalError> {
        let mut nonce = [0_u8; 12];
        rand::rngs::OsRng.fill_bytes(&mut nonce);
        let ciphertext = self
            .cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: plaintext,
                    aad,
                },
            )
            .map_err(|_| JournalError::Crypto("unable to seal journal frame".to_string()))?;
        Ok(SealedPayload { nonce, ciphertext })
    }

    pub(crate) fn open(
        &self,
        nonce: &[u8],
        ciphertext: &[u8],
        aad: &[u8],
    ) -> Result<Vec<u8>, JournalError> {
        if nonce.len() != 12 {
            return Err(JournalError::Crypto(
                "journal frame nonce is not 96 bits".to_string(),
            ));
        }
        self.cipher
            .decrypt(
                Nonce::from_slice(nonce),
                Payload {
                    msg: ciphertext,
                    aad,
                },
            )
            .map_err(|_| JournalError::Crypto("unable to open journal frame".to_string()))
    }
}
