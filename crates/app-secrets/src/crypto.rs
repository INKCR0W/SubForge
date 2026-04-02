use std::collections::HashMap;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use argon2::{Algorithm, Argon2, Params, Version};
use zeroize::Zeroizing;

use crate::constants::{
    ARGON2_ITERATIONS, ARGON2_MEMORY_KIB, ARGON2_PARALLELISM, FILE_FORMAT_VERSION, FILE_NONCE_LEN,
    FILE_SALT_LEN, FILE_TAG_LEN,
};
use crate::{SecretError, SecretResult};

pub(crate) fn encrypt_payload(
    data: &HashMap<String, HashMap<String, String>>,
    master_key: &str,
) -> SecretResult<Vec<u8>> {
    let plaintext = serde_json::to_vec(data)?;

    let mut salt = [0_u8; FILE_SALT_LEN];
    let mut nonce = [0_u8; FILE_NONCE_LEN];
    getrandom::fill(&mut salt)
        .map_err(|error| SecretError::Backend(format!("生成 salt 失败：{error}")))?;
    getrandom::fill(&mut nonce)
        .map_err(|error| SecretError::Backend(format!("生成 nonce 失败：{error}")))?;

    let key = Zeroizing::new(derive_key(master_key, &salt)?);
    let cipher = Aes256Gcm::new_from_slice(key.as_ref())
        .map_err(|error| SecretError::Backend(format!("AES 初始化失败：{error}")))?;

    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce), plaintext.as_ref())
        .map_err(|_| SecretError::Backend("AES-GCM 加密失败".to_string()))?;

    let mut payload = Vec::with_capacity(1 + FILE_SALT_LEN + FILE_NONCE_LEN + ciphertext.len());
    payload.push(FILE_FORMAT_VERSION);
    payload.extend_from_slice(&salt);
    payload.extend_from_slice(&nonce);
    payload.extend_from_slice(&ciphertext);
    Ok(payload)
}

pub(crate) fn decrypt_payload(
    payload: &[u8],
    master_key: &str,
) -> SecretResult<HashMap<String, HashMap<String, String>>> {
    let min_len = 1 + FILE_SALT_LEN + FILE_NONCE_LEN + FILE_TAG_LEN;
    if payload.len() < min_len {
        return Err(SecretError::SecretMissing(
            "密钥文件损坏或主密码错误".to_string(),
        ));
    }

    let version = payload[0];
    if version != FILE_FORMAT_VERSION {
        return Err(SecretError::Backend(format!("不支持的密文版本：{version}")));
    }

    let salt_start = 1;
    let nonce_start = salt_start + FILE_SALT_LEN;
    let cipher_start = nonce_start + FILE_NONCE_LEN;

    let mut salt = [0_u8; FILE_SALT_LEN];
    salt.copy_from_slice(&payload[salt_start..nonce_start]);

    let mut nonce = [0_u8; FILE_NONCE_LEN];
    nonce.copy_from_slice(&payload[nonce_start..cipher_start]);

    let key = Zeroizing::new(derive_key(master_key, &salt)?);
    let cipher = Aes256Gcm::new_from_slice(key.as_ref())
        .map_err(|error| SecretError::Backend(format!("AES 初始化失败：{error}")))?;

    let plaintext = cipher
        .decrypt(Nonce::from_slice(&nonce), &payload[cipher_start..])
        .map_err(|_| SecretError::SecretMissing("密钥文件解密失败（主密码错误）".to_string()))?;

    let data = serde_json::from_slice(&plaintext)?;
    Ok(data)
}

fn derive_key(master_key: &str, salt: &[u8; FILE_SALT_LEN]) -> SecretResult<[u8; 32]> {
    let params = Params::new(
        ARGON2_MEMORY_KIB,
        ARGON2_ITERATIONS,
        ARGON2_PARALLELISM,
        Some(32),
    )
    .map_err(|error| SecretError::Backend(format!("Argon2 参数无效：{error}")))?;

    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);

    let mut key = [0_u8; 32];
    argon2
        .hash_password_into(master_key.as_bytes(), salt, &mut key)
        .map_err(|error| SecretError::Backend(format!("Argon2 密钥派生失败：{error}")))?;
    Ok(key)
}
