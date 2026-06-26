//! Integration tests for `epicode::engine::crypto`.
//!
//! These tests exercise the public surface of the crypto engine without
//! touching the network or filesystem. They run as part of `cargo test`.

use base64::Engine;
use epicode::engine::crypto::{constant_time_eq, constant_time_eq_bytes, CryptoEngine};

#[test]
fn constant_time_eq_matches_identical_strings() {
    assert!(constant_time_eq("secret", "secret"));
    assert!(!constant_time_eq("secret", "secre7"));
    assert!(!constant_time_eq("short", "longer-string"));
}

#[test]
fn constant_time_eq_bytes_handles_slices() {
    assert!(constant_time_eq_bytes(&[1, 2, 3], &[1, 2, 3]));
    assert!(!constant_time_eq_bytes(&[1, 2, 3], &[1, 2, 4]));
    assert!(!constant_time_eq_bytes(&[1, 2], &[1, 2, 3]));
}

#[test]
fn crypto_engine_round_trip_text() {
    let key = vec![0u8; 32];
    let engine = CryptoEngine::from_key(key).expect("32-byte key is valid");
    let user = "user-42";
    let plaintext = "the quick brown fox jumps over the lazy dog";

    let ciphertext = engine
        .encrypt_content(plaintext, user)
        .expect("encrypt succeeds");
    assert_ne!(
        ciphertext, plaintext,
        "ciphertext must differ from plaintext"
    );

    let recovered = engine
        .decrypt_content(&ciphertext, user)
        .expect("decrypt succeeds");
    assert_eq!(recovered, plaintext, "round trip must restore plaintext");
}

#[test]
fn crypto_engine_round_trip_embedding() {
    let key = vec![1u8; 32];
    let engine = CryptoEngine::from_key(key).expect("32-byte key is valid");
    let user = "user-embed";
    let embedding = vec![0.1, 0.2, 0.3, -0.4, 1.5];

    let encrypted = engine
        .encrypt_embedding(&embedding, user)
        .expect("encrypt embedding succeeds");
    assert!(
        encrypted.len() > 12,
        "encrypted payload must include nonce + ciphertext"
    );

    let recovered = engine
        .decrypt_embedding(&encrypted, user)
        .expect("decrypt embedding succeeds");
    assert_eq!(recovered.len(), embedding.len());
    for (a, b) in recovered.iter().zip(embedding.iter()) {
        assert!(
            (a - b).abs() < f64::EPSILON,
            "embedding value must survive round trip"
        );
    }
}

#[test]
fn crypto_engine_rejects_wrong_user() {
    let key = vec![2u8; 32];
    let engine = CryptoEngine::from_key(key).expect("32-byte key is valid");
    let ciphertext = engine
        .encrypt_content("payload", "alice")
        .expect("encrypt succeeds");

    let result = engine.decrypt_content(&ciphertext, "bob");
    assert!(
        result.is_err(),
        "decrypting with a different user must fail"
    );
}

#[test]
fn crypto_engine_rejects_short_data() {
    let key = vec![3u8; 32];
    let engine = CryptoEngine::from_key(key).expect("32-byte key is valid");
    let result = engine.decrypt(&[0u8, 1, 2], &[0u8; 32]);
    assert!(result.is_err(), "data shorter than nonce must be rejected");
}

#[test]
fn crypto_engine_rejects_invalid_key_length() {
    let result = CryptoEngine::from_key(vec![0u8; 16]);
    assert!(result.is_err(), "non-32-byte key must be rejected");
}

#[test]
fn generated_master_key_is_32_bytes_base64() {
    let b64 = CryptoEngine::generate_master_key();
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(&b64)
        .expect("generated key is valid base64");
    assert_eq!(decoded.len(), 32, "master key must decode to 32 bytes");
}

#[test]
fn derive_user_key_is_deterministic() {
    let key = vec![4u8; 32];
    let engine = CryptoEngine::from_key(key).expect("32-byte key is valid");
    let k1 = engine.derive_user_key("user-1");
    let k2 = engine.derive_user_key("user-1");
    // We can't read the bytes directly, but we can verify encrypt/decrypt round trips.
    let data = b"hello";
    let c1 = k1.encrypt_data(data).expect("encrypt");
    let c2 = k2.encrypt_data(data).expect("encrypt");
    // Both keys should decrypt each other's ciphertext because they are equal.
    assert!(
        k1.decrypt_data(&c2).is_ok(),
        "identical keys must decrypt each other's ciphertext"
    );
    assert!(k2.decrypt_data(&c1).is_ok());
}
