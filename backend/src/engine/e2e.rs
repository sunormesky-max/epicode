//! γ.2 通道层端到端加密 — 意志传导通道的 E2E (§14.2: 默认端本地钥)
//!
//! 解决 R1 风险: "持 key 路径可见意志摘要"。服务端只在 register 时收到公钥,
//! signal description 在传输层加密, 私钥永不离开端侧。
//!
//! 格式 (base64): RSA-OAEP-SHA256(pub, aes_key_32B) || nonce_12B || AES-256-GCM(ct||tag)
//! 算法与 Node crypto 对齐: privateDecrypt(RSA_OAEP_SHA256) + createDecipheriv('aes-256-gcm')

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};

#[cfg(test)]
const RSA_OAEP_OVERHEAD: usize = 256; // 2048-bit modulus

fn pem_to_der(pem: &str, label: &str) -> Result<Vec<u8>, String> {
    let header = format!("-----BEGIN {label}-----");
    let footer = format!("-----END {label}-----");
    let mut lines = pem.lines().map(str::trim).filter(|line| !line.is_empty());
    if lines.next() != Some(header.as_str()) {
        return Err(format!("bad e2e {label} PEM header"));
    }

    let mut encoded = String::new();
    let mut footer_seen = false;
    for line in lines {
        if footer_seen {
            return Err(format!("unexpected data after e2e {label} PEM"));
        }
        if line == footer {
            footer_seen = true;
        } else if line.starts_with("-----") {
            return Err(format!("bad e2e {label} PEM boundary"));
        } else {
            encoded.push_str(line);
        }
    }
    if !footer_seen {
        return Err(format!("missing e2e {label} PEM footer"));
    }
    B64.decode(encoded)
        .map_err(|error| format!("bad e2e {label} PEM base64: {error}"))
}

/// 用端侧公钥(PEM SPKI)加密 payload, 返回 base64 密文; 失败返回 None(调用方降级明文+警告)
pub fn encrypt_for(payload: &[u8], pem_public: &str) -> Result<String, String> {
    use aws_lc_rs::rsa::{OaepPublicEncryptingKey, PublicEncryptingKey, OAEP_SHA256_MGF1SHA256};

    let public_der = pem_to_der(pem_public, "PUBLIC KEY")?;
    let public_key =
        PublicEncryptingKey::from_der(&public_der).map_err(|_| "bad e2e public key".to_string())?;
    let public_key = OaepPublicEncryptingKey::new(public_key)
        .map_err(|_| "unsupported e2e public key".to_string())?;

    // 1. 随机 AES-256 key + nonce
    let mut aes_key = [0u8; 32];
    let mut nonce = [0u8; 12];
    use rand::RngCore;
    rand::thread_rng().fill_bytes(&mut aes_key);
    rand::thread_rng().fill_bytes(&mut nonce);

    // 2. AES-256-GCM 加密 payload (输出 ct||tag, 与 Node authTag 拼接语义一致)
    use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit};
    let cipher = Aes256Gcm::new_from_slice(&aes_key).map_err(|e| format!("{}", e))?;
    let ct = cipher
        .encrypt(&nonce.into(), payload)
        .map_err(|e| format!("aes encrypt: {}", e))?;

    // 3. RSA-OAEP-SHA256 包 AES key
    let mut wrapped_buf = vec![0u8; public_key.ciphertext_size()];
    let wrapped = public_key
        .encrypt(&OAEP_SHA256_MGF1SHA256, &aes_key, &mut wrapped_buf, None)
        .map_err(|_| "rsa wrap failed".to_string())?;

    // 4. 拼装: wrapped || nonce || ct
    let mut out = Vec::with_capacity(wrapped.len() + 12 + ct.len());
    out.extend_from_slice(wrapped);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    Ok(B64.encode(out))
}

/// 端侧解密 (SDK/adapter 对称实现; 服务端提供此函数供测试自证)
#[cfg(test)]
pub(crate) fn decrypt_with(payload_b64: &str, pem_private: &str) -> Result<Vec<u8>, String> {
    use aws_lc_rs::rsa::{OaepPrivateDecryptingKey, PrivateDecryptingKey, OAEP_SHA256_MGF1SHA256};

    let raw = B64.decode(payload_b64).map_err(|e| format!("b64: {}", e))?;
    if raw.len() < RSA_OAEP_OVERHEAD + 12 + 16 {
        return Err("payload too short".into());
    }
    let (wrapped, rest) = raw.split_at(RSA_OAEP_OVERHEAD);
    let (nonce, ct) = rest.split_at(12);

    let private_der = pem_to_der(pem_private, "PRIVATE KEY")?;
    let private_key = PrivateDecryptingKey::from_pkcs8(&private_der)
        .map_err(|_| "bad e2e private key".to_string())?;
    let private_key = OaepPrivateDecryptingKey::new(private_key)
        .map_err(|_| "unsupported e2e private key".to_string())?;
    let mut aes_key_buf = vec![0u8; private_key.min_output_size()];
    let aes_key = private_key
        .decrypt(&OAEP_SHA256_MGF1SHA256, wrapped, &mut aes_key_buf, None)
        .map_err(|_| "rsa unwrap failed".to_string())?;

    use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit};
    let cipher = Aes256Gcm::new_from_slice(aes_key).map_err(|e| format!("{}", e))?;
    cipher
        .decrypt(nonce.into(), ct)
        .map_err(|e| format!("aes decrypt: {}", e))
}

#[cfg(test)]
pub(crate) fn test_keypair_pem() -> (String, String) {
    use aws_lc_rs::encoding::{AsDer, Pkcs8V1Der, PublicKeyX509Der};
    use aws_lc_rs::rsa::{KeySize, PrivateDecryptingKey};

    let private_key = PrivateDecryptingKey::generate(KeySize::Rsa2048).unwrap();
    let private_der = AsDer::<Pkcs8V1Der>::as_der(&private_key).unwrap();
    let public_der = AsDer::<PublicKeyX509Der>::as_der(&private_key.public_key()).unwrap();
    (
        der_to_pem("PUBLIC KEY", public_der.as_ref()),
        der_to_pem("PRIVATE KEY", private_der.as_ref()),
    )
}

#[cfg(test)]
fn der_to_pem(label: &str, der: &[u8]) -> String {
    let encoded = B64.encode(der);
    let body = encoded
        .as_bytes()
        .chunks(64)
        .map(|chunk| std::str::from_utf8(chunk).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    format!("-----BEGIN {label}-----\n{body}\n-----END {label}-----")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let (pub_pem, priv_pem) = test_keypair_pem();
        let msg = "意志通道加密测试: identity 不可触碰";
        let ct = encrypt_for(msg.as_bytes(), &pub_pem).unwrap();
        let pt = decrypt_with(&ct, &priv_pem).unwrap();
        assert_eq!(pt, msg.as_bytes());
    }
}
