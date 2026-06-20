use aes_gcm::{
    aead::{rand_core::RngCore, Aead, KeyInit, OsRng},
    Aes256Gcm, Nonce,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use zeroize::Zeroize;

type HmacSha256 = Hmac<Sha256>;

pub fn constant_time_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut result: u8 = 0;
    for (x, y) in a.bytes().zip(b.bytes()) {
        result |= x ^ y;
    }
    result == 0
}

pub fn constant_time_eq_bytes(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut result: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        result |= x ^ y;
    }
    result == 0
}

pub struct CryptoEngine {
    master_key: Vec<u8>,
}

impl CryptoEngine {
    pub fn from_env() -> Result<Self, String> {
        let key_b64 = std::env::var("TETRAMEM_MASTER_KEY")
            .map_err(|_| "TETRAMEM_MASTER_KEY not set".to_string())?;
        let key = STANDARD
            .decode(&key_b64)
            .map_err(|e| format!("invalid master key base64: {}", e))?;
        if key.len() != 32 {
            return Err("master key must be 32 bytes (base64-encoded)".to_string());
        }
        Ok(Self { master_key: key })
    }

    pub fn from_key(key: Vec<u8>) -> Result<Self, String> {
        if key.len() != 32 {
            return Err("key must be 32 bytes".to_string());
        }
        Ok(Self { master_key: key })
    }

    pub fn generate_master_key() -> String {
        let mut key = vec![0u8; 32];
        OsRng.fill_bytes(&mut key);
        let b64 = STANDARD.encode(&key);
        key.zeroize();
        b64
    }

    pub fn derive_user_key(&self, user_id: &str) -> UserKey {
        let mut mac = <HmacSha256 as hmac::Mac>::new_from_slice(&self.master_key)
            .expect("HMAC key length is valid");
        mac.update(user_id.as_bytes());
        mac.update(b":epicode-user-key-v1");
        let result = mac.finalize().into_bytes();
        UserKey {
            key: result.to_vec(),
        }
    }

    pub fn encrypt(&self, plaintext: &[u8], context_key: &[u8]) -> Result<Vec<u8>, String> {
        let mut derived = self.derive_aes_key(context_key);
        let cipher =
            Aes256Gcm::new_from_slice(&derived).map_err(|e| format!("cipher init: {}", e))?;
        let mut nonce_bytes = [0u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let mut ciphertext = cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| format!("encrypt: {}", e))?;
        let mut output = Vec::with_capacity(12 + ciphertext.len());
        output.extend_from_slice(&nonce_bytes);
        output.append(&mut ciphertext);
        derived.zeroize();
        Ok(output)
    }

    pub fn decrypt(&self, data: &[u8], context_key: &[u8]) -> Result<Vec<u8>, String> {
        if data.len() < 12 {
            return Err("data too short".to_string());
        }
        let (nonce_bytes, ciphertext) = data.split_at(12);
        let mut derived = self.derive_aes_key(context_key);
        let cipher =
            Aes256Gcm::new_from_slice(&derived).map_err(|e| format!("cipher init: {}", e))?;
        let nonce = Nonce::from_slice(nonce_bytes);
        let result = cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| format!("decrypt: {}", e));
        derived.zeroize();
        result
    }

    pub fn encrypt_content(&self, content: &str, user_id: &str) -> Result<String, String> {
        let mut context = self.user_context(user_id);
        let encrypted = self.encrypt(content.as_bytes(), &context)?;
        context.zeroize();
        Ok(STANDARD.encode(&encrypted))
    }

    pub fn decrypt_content(&self, encrypted: &str, user_id: &str) -> Result<String, String> {
        let mut context = self.user_context(user_id);
        let data = STANDARD
            .decode(encrypted)
            .map_err(|e| format!("base64 decode: {}", e))?;
        let decrypted = self.decrypt(&data, &context)?;
        context.zeroize();
        String::from_utf8(decrypted).map_err(|e| format!("utf8: {}", e))
    }

    pub fn encrypt_embedding(&self, embedding: &[f64], user_id: &str) -> Result<Vec<u8>, String> {
        let mut context = self.user_context(user_id);
        let bytes: Vec<u8> = embedding.iter().flat_map(|f| f.to_le_bytes()).collect();
        let result = self.encrypt(&bytes, &context);
        context.zeroize();
        result
    }

    pub fn decrypt_embedding(&self, data: &[u8], user_id: &str) -> Result<Vec<f64>, String> {
        let mut context = self.user_context(user_id);
        let bytes = self.decrypt(data, &context)?;
        if bytes.len() % 8 != 0 {
            return Err("invalid embedding length".to_string());
        }
        let result = Ok(bytes
            .chunks_exact(8)
            .map(|chunk| {
                f64::from_le_bytes([
                    chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7],
                ])
            })
            .collect());
        context.zeroize();
        result
    }

    fn derive_aes_key(&self, context_key: &[u8]) -> Vec<u8> {
        let mut mac = <HmacSha256 as hmac::Mac>::new_from_slice(&self.master_key)
            .expect("HMAC key length is valid");
        mac.update(context_key);
        mac.update(b":aes-256-key");
        let result = mac.finalize().into_bytes();
        result.to_vec()
    }

    fn user_context(&self, user_id: &str) -> Vec<u8> {
        let mut mac = <HmacSha256 as hmac::Mac>::new_from_slice(&self.master_key)
            .expect("HMAC key length is valid");
        mac.update(user_id.as_bytes());
        mac.update(b":user-context");
        mac.finalize().into_bytes().to_vec()
    }
}

impl Drop for CryptoEngine {
    fn drop(&mut self) {
        self.master_key.zeroize();
    }
}

pub struct UserKey {
    key: Vec<u8>,
}

impl UserKey {
    pub fn encrypt_data(&self, plaintext: &[u8]) -> Result<Vec<u8>, String> {
        let cipher =
            Aes256Gcm::new_from_slice(&self.key).map_err(|e| format!("cipher init: {}", e))?;
        let mut nonce_bytes = [0u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let mut ciphertext = cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| format!("encrypt: {}", e))?;
        let mut output = Vec::with_capacity(12 + ciphertext.len());
        output.extend_from_slice(&nonce_bytes);
        output.append(&mut ciphertext);
        Ok(output)
    }

    pub fn decrypt_data(&self, data: &[u8]) -> Result<Vec<u8>, String> {
        if data.len() < 12 {
            return Err("data too short".to_string());
        }
        let (nonce_bytes, ciphertext) = data.split_at(12);
        let cipher =
            Aes256Gcm::new_from_slice(&self.key).map_err(|e| format!("cipher init: {}", e))?;
        let nonce = Nonce::from_slice(nonce_bytes);
        cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| format!("decrypt: {}", e))
    }
}

impl Drop for UserKey {
    fn drop(&mut self) {
        self.key.zeroize();
    }
}

pub fn compute_integrity_hash(data: &[u8], key: &[u8]) -> String {
    let mut mac = <HmacSha256 as hmac::Mac>::new_from_slice(key).expect("HMAC key");
    mac.update(data);
    let result = mac.finalize().into_bytes();
    STANDARD.encode(result)
}

pub fn verify_integrity(data: &[u8], key: &[u8], expected: &str) -> bool {
    let actual = compute_integrity_hash(data, key);
    let mut diff = 0u8;
    for (a, b) in actual.bytes().zip(expected.bytes()) {
        diff |= a ^ b;
    }
    diff |= (actual.len() != expected.len()) as u8;
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_key_is_32_bytes() {
        let b64 = CryptoEngine::generate_master_key();
        let key = STANDARD.decode(&b64).unwrap();
        assert_eq!(key.len(), 32);
    }

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let b64 = CryptoEngine::generate_master_key();
        let engine = CryptoEngine::from_key(STANDARD.decode(&b64).unwrap()).unwrap();
        let ctx = b"test-context";
        let plaintext = b"hello world secret data";
        let encrypted = engine.encrypt(plaintext, ctx).unwrap();
        let decrypted = engine.decrypt(&encrypted, ctx).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn content_encrypt_decrypt() {
        let b64 = CryptoEngine::generate_master_key();
        let engine = CryptoEngine::from_key(STANDARD.decode(&b64).unwrap()).unwrap();
        let original = "这是一条秘密记忆，包含敏感信息";
        let encrypted = engine.encrypt_content(original, "user123").unwrap();
        let decrypted = engine.decrypt_content(&encrypted, "user123").unwrap();
        assert_eq!(decrypted, original);
    }

    #[test]
    fn embedding_encrypt_decrypt() {
        let b64 = CryptoEngine::generate_master_key();
        let engine = CryptoEngine::from_key(STANDARD.decode(&b64).unwrap()).unwrap();
        let original: Vec<f64> = vec![0.1, -0.5, 0.99, 0.0, 1e-10];
        let encrypted = engine.encrypt_embedding(&original, "user456").unwrap();
        let decrypted = engine.decrypt_embedding(&encrypted, "user456").unwrap();
        for (a, b) in original.iter().zip(decrypted.iter()) {
            assert!((a - b).abs() < 1e-15);
        }
    }

    #[test]
    fn different_users_cannot_decrypt() {
        let b64 = CryptoEngine::generate_master_key();
        let engine = CryptoEngine::from_key(STANDARD.decode(&b64).unwrap()).unwrap();
        let encrypted = engine.encrypt_content("secret", "alice").unwrap();
        let result = engine.decrypt_content(&encrypted, "bob");
        assert!(result.is_err());
    }

    #[test]
    fn user_key_roundtrip() {
        let key = UserKey {
            key: vec![42u8; 32],
        };
        let data = b"per-user encrypted data";
        let encrypted = key.encrypt_data(data).unwrap();
        let decrypted = key.decrypt_data(&encrypted).unwrap();
        assert_eq!(decrypted, data);
    }

    #[test]
    fn integrity_hash_verify() {
        let key = b"integrity-key-123";
        let data = b"some important data";
        let hash = compute_integrity_hash(data, key);
        assert!(verify_integrity(data, key, &hash));
        assert!(!verify_integrity(b"tampered data", key, &hash));
    }
}
