use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use rand_core::{OsRng, RngCore};

#[derive(Debug)]
pub enum CryptoError {
    Encrypt(String),
    Decrypt(String),
    Decode(String),
    Json(serde_json::Error),
}

impl std::fmt::Display for CryptoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CryptoError::Encrypt(e) => write!(f, "Encryption error: {e}"),
            CryptoError::Decrypt(e) => write!(f, "Decryption error: {e}"),
            CryptoError::Decode(e) => write!(f, "Base64 decode error: {e}"),
            CryptoError::Json(e) => write!(f, "JSON error: {e}"),
        }
    }
}

impl std::error::Error for CryptoError {}

// CryptoError is Send + Sync because all its fields are Send + Sync
unsafe impl Send for CryptoError {}
unsafe impl Sync for CryptoError {}

/// Encrypt a JSON value with AES-256-GCM.
/// Returns base64(12-byte nonce ‖ ciphertext+tag).
pub fn encrypt_json(data: &serde_json::Value, key: &[u8; 32]) -> Result<String, CryptoError> {
    let plaintext = serde_json::to_vec(data).map_err(CryptoError::Json)?;

    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let cipher = Aes256Gcm::new(key.into());
    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_ref())
        .map_err(|e| CryptoError::Encrypt(e.to_string()))?;

    let mut combined = nonce_bytes.to_vec();
    combined.extend_from_slice(&ciphertext);

    Ok(STANDARD.encode(&combined))
}

/// Decrypt a base64-encoded AES-256-GCM ciphertext back to a JSON value.
pub fn decrypt_json(encoded: &str, key: &[u8; 32]) -> Result<serde_json::Value, CryptoError> {
    let data = STANDARD
        .decode(encoded)
        .map_err(|e| CryptoError::Decode(e.to_string()))?;

    if data.len() < 12 {
        return Err(CryptoError::Decrypt(
            "Ciphertext too short (< 12 bytes)".to_string(),
        ));
    }

    let (nonce_bytes, ciphertext) = data.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);

    let cipher = Aes256Gcm::new(key.into());
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| CryptoError::Decrypt(e.to_string()))?;

    serde_json::from_slice(&plaintext).map_err(CryptoError::Json)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_key() -> [u8; 32] {
        [42u8; 32]
    }

    #[test]
    fn test_round_trip_object() {
        let key = test_key();
        let data = serde_json::json!({"host": "localhost", "port": 5432});
        let encrypted = encrypt_json(&data, &key).unwrap();
        let decrypted = decrypt_json(&encrypted, &key).unwrap();
        assert_eq!(data, decrypted);
    }

    #[test]
    fn test_round_trip_string() {
        let key = test_key();
        let data = serde_json::json!("secret password");
        let encrypted = encrypt_json(&data, &key).unwrap();
        let decrypted = decrypt_json(&encrypted, &key).unwrap();
        assert_eq!(data, decrypted);
    }

    #[test]
    fn test_round_trip_nested() {
        let key = test_key();
        let data = serde_json::json!({"a": {"b": [1, 2, 3]}, "c": null});
        let encrypted = encrypt_json(&data, &key).unwrap();
        let decrypted = decrypt_json(&encrypted, &key).unwrap();
        assert_eq!(data, decrypted);
    }

    #[test]
    fn test_round_trip_empty_object() {
        let key = test_key();
        let data = serde_json::json!({});
        let encrypted = encrypt_json(&data, &key).unwrap();
        let decrypted = decrypt_json(&encrypted, &key).unwrap();
        assert_eq!(data, decrypted);
    }

    #[test]
    fn test_wrong_key_fails() {
        let key1 = [1u8; 32];
        let key2 = [2u8; 32];
        let data = serde_json::json!({"secret": "value"});
        let encrypted = encrypt_json(&data, &key1).unwrap();
        let result = decrypt_json(&encrypted, &key2);
        assert!(result.is_err(), "Decryption with wrong key should fail");
    }

    #[test]
    fn test_corrupted_data_fails() {
        let key = test_key();
        let result = decrypt_json("not-valid-base64!!!", &key);
        assert!(result.is_err());
    }

    #[test]
    fn test_two_encryptions_differ() {
        let key = test_key();
        let data = serde_json::json!({"host": "localhost"});
        let enc1 = encrypt_json(&data, &key).unwrap();
        let enc2 = encrypt_json(&data, &key).unwrap();
        assert_ne!(
            enc1, enc2,
            "Random nonce should produce different ciphertext each time"
        );
    }
}
