//! γ.2 通道层端到端加密 — 意志传导通道的 E2E (§14.2: 默认端本地钥)
//!
//! 解决 R1 风险: "持 key 路径可见意志摘要"。服务端只在 register 时收到公钥,
//! signal description 在传输层加密, 私钥永不离开端侧。
//!
//! 格式 (base64): RSA-OAEP-SHA256(pub, aes_key_32B) || nonce_12B || AES-256-GCM(ct||tag)
//! 算法与 Node crypto 对齐: privateDecrypt(RSA_OAEP_SHA256) + createDecipheriv('aes-256-gcm')

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};

const RSA_OAEP_OVERHEAD: usize = 256; // 2048-bit modulus

/// 用端侧公钥(PEM SPKI)加密 payload, 返回 base64 密文; 失败返回 None(调用方降级明文+警告)
pub fn encrypt_for(payload: &[u8], pem_public: &str) -> Result<String, String> {
    use rsa::{Oaep, RsaPublicKey, pkcs8::DecodePublicKey};
    use rsa::sha2::Sha256;

    let pub_key = RsaPublicKey::from_public_key_pem(pem_public)
        .map_err(|e| format!("bad e2e pubkey pem: {}", e))?;

    // 1. 随机 AES-256 key + nonce
    let mut aes_key = [0u8; 32];
    let mut nonce = [0u8; 12];
    use rand::RngCore;
    rand::thread_rng().fill_bytes(&mut aes_key);
    rand::thread_rng().fill_bytes(&mut nonce);

    // 2. AES-256-GCM 加密 payload (输出 ct||tag, 与 Node authTag 拼接语义一致)
    use aes_gcm::{Aes256Gcm, KeyInit, aead::Aead};
    let cipher = Aes256Gcm::new_from_slice(&aes_key).map_err(|e| format!("{}", e))?;
    let ct = cipher.encrypt(&nonce.into(), payload)
        .map_err(|e| format!("aes encrypt: {}", e))?;

    // 3. RSA-OAEP-SHA256 包 AES key
    let wrapped = pub_key.encrypt(&mut rand::thread_rng(), Oaep::new::<Sha256>(), &aes_key)
        .map_err(|e| format!("rsa wrap: {}", e))?;

    // 4. 拼装: wrapped || nonce || ct
    let mut out = Vec::with_capacity(RSA_OAEP_OVERHEAD + 12 + ct.len());
    out.extend_from_slice(&wrapped);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    Ok(B64.encode(out))
}

/// 端侧解密 (SDK/adapter 对称实现; 服务端提供此函数供测试自证)
pub fn decrypt_with(payload_b64: &str, pem_private: &str) -> Result<Vec<u8>, String> {
    use rsa::{Oaep, RsaPrivateKey, pkcs8::DecodePrivateKey};
    use rsa::sha2::Sha256;

    let raw = B64.decode(payload_b64).map_err(|e| format!("b64: {}", e))?;
    if raw.len() < RSA_OAEP_OVERHEAD + 12 + 16 {
        return Err("payload too short".into());
    }
    let (wrapped, rest) = raw.split_at(RSA_OAEP_OVERHEAD);
    let (nonce, ct) = rest.split_at(12);

    let priv_key = RsaPrivateKey::from_pkcs8_pem(pem_private)
        .map_err(|e| format!("bad e2e privkey: {}", e))?;
    let aes_key = priv_key.decrypt(Oaep::new::<Sha256>(), wrapped)
        .map_err(|e| format!("rsa unwrap: {}", e))?;

    use aes_gcm::{Aes256Gcm, KeyInit, aead::Aead};
    let cipher = Aes256Gcm::new_from_slice(&aes_key).map_err(|e| format!("{}", e))?;
    cipher.decrypt(nonce.into(), ct).map_err(|e| format!("aes decrypt: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsa::{RsaPrivateKey, pkcs8::EncodePrivateKey, pkcs8::EncodePublicKey};

    #[test]
    fn roundtrip() {
        let priv_key = RsaPrivateKey::new(&mut rand::thread_rng(), 2048).unwrap();
        let priv_pem = priv_key.to_pkcs8_pem(rsa::pkcs8::LineEnding::LF).unwrap().to_string();
        let pub_pem = priv_key.to_public_key().to_public_key_pem(rsa::pkcs8::LineEnding::LF).unwrap();
        let msg = "意志通道加密测试: identity 不可触碰";
        let ct = encrypt_for(msg.as_bytes(), &pub_pem).unwrap();
        let pt = decrypt_with(&ct, &priv_pem).unwrap();
        assert_eq!(pt, msg.as_bytes());
    }
}
