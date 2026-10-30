use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use prelay_protocol::identity::{
    DEVICE_CREDENTIAL_BYTES, DEVICE_CREDENTIAL_URL_SAFE_BASE64_LENGTH,
};
use rand::RngCore;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

pub fn generate_credential() -> String {
    let mut bytes = [0_u8; DEVICE_CREDENTIAL_BYTES];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn is_valid_device_credential(credential: &str) -> bool {
    if credential.len() != DEVICE_CREDENTIAL_URL_SAFE_BASE64_LENGTH {
        return false;
    }

    let Ok(bytes) = URL_SAFE_NO_PAD.decode(credential) else {
        return false;
    };
    bytes.len() == DEVICE_CREDENTIAL_BYTES && URL_SAFE_NO_PAD.encode(bytes) == credential
}

pub fn hash_credential(credential: &str) -> String {
    let digest = Sha256::digest(credential.as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}

pub fn credential_hashes_match(expected: &str, supplied: &str) -> bool {
    let expected = expected.as_bytes();
    let supplied = supplied.as_bytes();
    expected.len() == supplied.len() && bool::from(expected.ct_eq(supplied))
}
