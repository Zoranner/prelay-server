use base64::{engine::general_purpose::STANDARD, Engine as _};
use sha2::{Digest, Sha256};

use crate::store::recording_journal::{
    frame::{decode_frame, encode_frame, Frame, FrameType, JournalBinding},
    JournalError,
};

use super::test_cipher;

fn binding() -> JournalBinding {
    JournalBinding {
        node_id: "5c3e0f3b-1e8d-4dd2-9aa7-6d92c4d3f101".to_string(),
        epoch: "125a9f99-7dc8-4dae-a62c-8f50a3de0bb3".to_string(),
    }
}

fn frame() -> Frame {
    Frame {
        sequence: 7,
        frame_type: FrameType::Data,
        activity_id: "activity-1".to_string(),
        kind: "turn_completed".to_string(),
        part_index: 0,
        part_count: 1,
        payload: br#"{"message":"hello"}"#.to_vec(),
    }
}

#[test]
fn round_trips_a_frame() {
    let cipher = test_cipher();
    let encoded = encode_frame(&frame(), &cipher, &binding()).expect("encode frame");
    let decoded = decode_frame(&encoded, &cipher, &binding()).expect("decode frame");
    assert_eq!(decoded, frame());
    assert!(!String::from_utf8_lossy(&encoded).contains("hello"));
}

#[test]
fn rejects_a_different_node_or_epoch_binding() {
    let cipher = test_cipher();
    let encoded = encode_frame(&frame(), &cipher, &binding()).expect("encode frame");
    let mut other = binding();
    other.epoch = "eb0e25eb-30a0-4fef-89ad-650064175bdf".to_string();
    assert!(matches!(
        decode_frame(&encoded, &cipher, &other),
        Err(JournalError::Crypto(_))
    ));
}

#[test]
fn rejects_ciphertext_changes_even_with_a_recomputed_checksum() {
    let cipher = test_cipher();
    let encoded = encode_frame(&frame(), &cipher, &binding()).expect("encode frame");
    let mut stored: serde_json::Value =
        serde_json::from_slice(&encoded).expect("parse encoded frame");
    let mut ciphertext = STANDARD
        .decode(stored["ciphertext"].as_str().expect("ciphertext"))
        .expect("decode ciphertext");
    ciphertext[0] ^= 0x55;
    let nonce = STANDARD
        .decode(stored["nonce"].as_str().expect("nonce"))
        .expect("decode nonce");
    let mut checksum_input = nonce;
    checksum_input.extend_from_slice(&ciphertext);
    stored["ciphertext"] = serde_json::Value::String(STANDARD.encode(ciphertext));
    stored["checksum"] = serde_json::Value::String(STANDARD.encode(Sha256::digest(checksum_input)));
    let tampered = serde_json::to_vec(&stored).expect("serialize tampered frame");
    assert!(matches!(
        decode_frame(&tampered, &cipher, &binding()),
        Err(JournalError::Crypto(_))
    ));
}

#[test]
fn rejects_checksum_changes() {
    let cipher = test_cipher();
    let encoded = encode_frame(&frame(), &cipher, &binding()).expect("encode frame");
    let mut stored: serde_json::Value =
        serde_json::from_slice(&encoded).expect("parse encoded frame");
    stored["checksum"] = serde_json::Value::String(STANDARD.encode([0_u8; 32]));
    let tampered = serde_json::to_vec(&stored).expect("serialize tampered frame");
    assert!(matches!(
        decode_frame(&tampered, &cipher, &binding()),
        Err(JournalError::Corrupt(_))
    ));
}

#[test]
fn rejects_invalid_part_ranges() {
    let cipher = test_cipher();
    let mut invalid = frame();
    invalid.part_index = 1;
    assert!(matches!(
        encode_frame(&invalid, &cipher, &binding()),
        Err(JournalError::Corrupt(_))
    ));
}
