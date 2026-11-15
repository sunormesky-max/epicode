//! Per-user field decryption for the S1 shadow (read-only).
//!
//! Production `labels` columns are per-user AES-256-GCM ciphertext
//! (`backend/src/engine/storage.rs::encrypt_field` → `crypto.rs::encrypt_content`):
//!
//! ```text
//! master_key = TETRAMEM_MASTER_KEY env (base64, 32 bytes)
//! user_ctx   = HMAC-SHA256(master_key, user_id || ":user-context")
//! aes_key    = HMAC-SHA256(master_key, user_ctx   || ":aes-256-key")
//! stored     = base64( nonce[12] || AES-256-GCM(aes_key, nonce, labels_json) )
//! ```
//!
//! This module reproduces exactly that derivation (same crate versions as the
//! backend) so the shadow can decrypt labels WITHOUT touching the production
//! process. The master key is only ever read from the environment — never a
//! CLI argument (it would leak into process listings).
//!
//! Hard rule preserved: decrypt in memory, never write plaintext anywhere.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

pub struct ShadowCrypto {
    master_key: Vec<u8>,
}

impl ShadowCrypto {
    /// Load from `TETRAMEM_MASTER_KEY` (base64, 32 bytes) — same env var the
    /// production cloud binary reads. `Ok(None)` when unset: the shadow then
    /// runs in plaintext-labels mode and skips encrypted rows.
    pub fn from_env() -> Result<Option<Self>, String> {
        match std::env::var("TETRAMEM_MASTER_KEY") {
            Ok(b64) => {
                let key = STANDARD
                    .decode(b64.trim())
                    .map_err(|e| format!("TETRAMEM_MASTER_KEY base64: {e}"))?;
                if key.len() != 32 {
                    return Err(format!(
                        "TETRAMEM_MASTER_KEY must decode to 32 bytes, got {}",
                        key.len()
                    ));
                }
                Ok(Some(Self { master_key: key }))
            }
            Err(_) => Ok(None),
        }
    }

    #[cfg(test)]
    pub fn from_key(key: Vec<u8>) -> Self {
        assert_eq!(key.len(), 32);
        Self { master_key: key }
    }

    fn hmac(&self, parts: &[&[u8]]) -> Vec<u8> {
        let mut mac = <HmacSha256 as Mac>::new_from_slice(&self.master_key)
            .expect("HMAC accepts any key length");
        for p in parts {
            mac.update(p);
        }
        mac.finalize().into_bytes().to_vec()
    }

    /// Decrypt one stored field (labels) for `user_id`.
    /// Returns `None` for undecryptable input (wrong key, foreign format,
    /// corrupted row) — the caller skips and counts the row.
    pub fn decrypt_field(&self, stored_b64: &str, user_id: &str) -> Option<String> {
        let user_ctx = self.hmac(&[user_id.as_bytes(), b":user-context"]);
        let aes_key = self.hmac(&[user_ctx.as_slice(), b":aes-256-key"]);
        let data = STANDARD.decode(stored_b64.trim()).ok()?;
        if data.len() < 12 {
            return None;
        }
        let (nonce, ct) = data.split_at(12);
        let cipher = Aes256Gcm::new_from_slice(&aes_key).ok()?;
        let plain = cipher.decrypt(Nonce::from_slice(nonce), ct).ok()?;
        String::from_utf8(plain).ok()
    }

    /// Test/fixture helper: encrypt with the exact production chain so tests
    /// can build ciphertext fixtures without linking the backend.
    #[cfg(test)]
    pub fn encrypt_field(&self, plaintext: &str, user_id: &str) -> String {
        use aes_gcm::aead::OsRng as AeadOsRng;
        use aes_gcm::AeadCore;
        let user_ctx = self.hmac(&[user_id.as_bytes(), b":user-context"]);
        let aes_key = self.hmac(&[user_ctx.as_slice(), b":aes-256-key"]);
        let cipher = Aes256Gcm::new_from_slice(&aes_key).unwrap();
        let nonce = Aes256Gcm::generate_nonce(&mut AeadOsRng);
        let ct = cipher.encrypt(&nonce, plaintext.as_bytes()).unwrap();
        let mut out = nonce.to_vec();
        out.extend_from_slice(&ct);
        STANDARD.encode(out)
    }
}

impl Drop for ShadowCrypto {
    fn drop(&mut self) {
        self.master_key.iter_mut().for_each(|b| *b = 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> Vec<u8> {
        (0u8..32).collect()
    }

    #[test]
    fn roundtrip_matches_production_chain() {
        let c = ShadowCrypto::from_key(key());
        let ct = c.encrypt_field(r#"["空间记忆","rust"]"#, "sunorme");
        let pt = c.decrypt_field(&ct, "sunorme").expect("decrypt");
        assert_eq!(pt, r#"["空间记忆","rust"]"#);
    }

    #[test]
    fn wrong_user_or_garbage_is_none_not_panic() {
        let c = ShadowCrypto::from_key(key());
        let ct = c.encrypt_field("[]", "alice");
        assert!(c.decrypt_field(&ct, "bob").is_none(), "foreign user key");
        assert!(c.decrypt_field("not-base64!!!", "alice").is_none());
        assert!(c.decrypt_field("AAAA", "alice").is_none(), "too short");
    }
}
