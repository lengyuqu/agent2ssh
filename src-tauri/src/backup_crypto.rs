//! Encrypted backup for WebDAV sync (P2 #10).
//!
//! ## Why this module exists
//!
//! Before this module, `webdav_push` uploaded config files (including
//! `hosts.json`, `secrets.enc`, `policy.toml`) to the WebDAV server as
//! raw bytes. While `secrets.enc` is already encrypted at rest, the
//! other files — especially `hosts.json` — contain host addresses,
//! usernames, proxy configurations, and policy rules that reveal
//! infrastructure topology. An attacker with read access to the WebDAV
//! server (shared hosting, compromised credentials, MITM) could extract
//! this information.
//!
//! This module provides envelope encryption: each sync upload is
//! encrypted with AES-256-GCM before being sent to the WebDAV server.
//! The encryption key is derived from a user-supplied sync password
//! via Argon2id, so the server never sees the key.
//!
//! ## Wire format
//!
//! ```text
//!   "AGENT2SSH_ENCRYPTED_BACKUP_V1" (32 bytes, ASCII, no null terminator)
//!   salt (16 bytes, random per backup)
//!   nonce (12 bytes, random per backup)
//!   ciphertext + GCM tag (variable length)
//! ```
//!
//! The magic prefix allows the pull side to detect whether a remote
//! file is encrypted or plaintext (for backward compatibility with
//! pre-encryption sync state).

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use anyhow::{anyhow, Result};
use argon2::{Algorithm, Argon2, Params, Version};
use rand::RngCore;

/// Magic prefix for encrypted backup files.
pub const ENCRYPTED_MAGIC: &[u8] = b"AGENT2SSH_ENCRYPTED_BACKUP_V1";

/// Salt length for Argon2id key derivation (16 bytes = recommended).
const SALT_LEN: usize = 16;

/// Nonce length for AES-256-GCM (12 bytes = standard).
const NONCE_LEN: usize = 12;

/// Argon2id parameters: 64 MiB memory, 3 iterations, 4 lanes.
/// This is deliberately heavier than typical interactive auth to
/// make brute-force of the sync password expensive.
const ARGON2_MEMORY_KIB: u32 = 65536;
const ARGON2_ITERATIONS: u32 = 3;
const ARGON2_PARALLELISM: u32 = 4;

/// Derive a 256-bit key from a password and salt using Argon2id.
fn derive_key(password: &[u8], salt: &[u8]) -> Result<[u8; 32]> {
    let params = Params::new(
        ARGON2_MEMORY_KIB,
        ARGON2_ITERATIONS,
        ARGON2_PARALLELISM,
        Some(32),
    )
    .map_err(|e| anyhow!("Argon2 params error: {e}"))?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);

    let mut key = [0u8; 32];
    argon
        .hash_password_into(password, salt, &mut key)
        .map_err(|e| anyhow!("Argon2id key derivation failed: {e}"))?;
    Ok(key)
}

/// Encrypt `plaintext` using AES-256-GCM with a key derived from `password`.
///
/// Returns a wire-format blob (see module docs).
pub fn encrypt_backup(password: &[u8], plaintext: &[u8]) -> Result<Vec<u8>> {
    let mut salt = [0u8; SALT_LEN];
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut salt);
    rand::thread_rng().fill_bytes(&mut nonce_bytes);

    let key_bytes = derive_key(password, &salt)?;
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);
    let nonce = Nonce::from_slice(&nonce_bytes);

    // AAD binds the magic prefix to the ciphertext, so a file that
    // doesn't start with the right magic can't be swapped in.
    let ciphertext = cipher
        .encrypt(
            nonce,
            Payload {
                msg: plaintext,
                aad: ENCRYPTED_MAGIC,
            },
        )
        .map_err(|e| anyhow!("AES-GCM encryption failed: {e}"))?;

    let mut output =
        Vec::with_capacity(ENCRYPTED_MAGIC.len() + SALT_LEN + NONCE_LEN + ciphertext.len());
    output.extend_from_slice(ENCRYPTED_MAGIC);
    output.extend_from_slice(&salt);
    output.extend_from_slice(&nonce_bytes);
    output.extend_from_slice(&ciphertext);
    Ok(output)
}

/// Decrypt a wire-format blob using AES-256-GCM with a key derived
/// from `password`.
///
/// Returns the original plaintext. Returns an error if the data is
/// not an encrypted backup (missing magic), the password is wrong,
/// or the data has been tampered with (GCM tag verification fails).
pub fn decrypt_backup(password: &[u8], data: &[u8]) -> Result<Vec<u8>> {
    if data.len() < ENCRYPTED_MAGIC.len() + SALT_LEN + NONCE_LEN {
        return Err(anyhow!("data too short to be an encrypted backup"));
    }

    let (magic, rest) = data.split_at(ENCRYPTED_MAGIC.len());
    if magic != ENCRYPTED_MAGIC {
        return Err(anyhow!("missing encrypted backup magic prefix"));
    }

    let (salt, rest) = rest.split_at(SALT_LEN);
    let (nonce_bytes, ciphertext) = rest.split_at(NONCE_LEN);

    let key_bytes = derive_key(password, salt)?;
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);
    let nonce = Nonce::from_slice(nonce_bytes);

    cipher
        .decrypt(
            nonce,
            Payload {
                msg: ciphertext,
                aad: ENCRYPTED_MAGIC,
            },
        )
        .map_err(|e| anyhow!("decryption failed (wrong password or tampered data): {e}"))
}

/// Check if `data` starts with the encrypted backup magic prefix.
pub fn is_encrypted_backup(data: &[u8]) -> bool {
    data.len() >= ENCRYPTED_MAGIC.len() && &data[..ENCRYPTED_MAGIC.len()] == ENCRYPTED_MAGIC
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let password = b"my-sync-password";
        let plaintext = b"{\"hosts\":[{\"name\":\"prod-1\",\"addr\":\"10.0.0.5\"}]}";
        let encrypted = encrypt_backup(password, plaintext).unwrap();
        assert!(is_encrypted_backup(&encrypted));
        let decrypted = decrypt_backup(password, &encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn wrong_password_fails() {
        let encrypted = encrypt_backup(b"correct", b"secret data").unwrap();
        let result = decrypt_backup(b"wrong", &encrypted);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("wrong password or tampered"));
    }

    #[test]
    fn tampered_ciphertext_fails() {
        let encrypted = encrypt_backup(b"pw", b"secret data").unwrap();
        let mut tampered = encrypted.clone();
        // Flip a bit in the ciphertext (after magic + salt + nonce).
        let offset = ENCRYPTED_MAGIC.len() + SALT_LEN + NONCE_LEN;
        tampered[offset] ^= 0x01;
        let result = decrypt_backup(b"pw", &tampered);
        assert!(result.is_err());
    }

    #[test]
    fn tampered_magic_fails() {
        let mut data = encrypt_backup(b"pw", b"secret").unwrap();
        data[0] ^= 0x01;
        let result = decrypt_backup(b"pw", &data);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("magic prefix"));
    }

    #[test]
    fn too_short_data_fails() {
        let result = decrypt_backup(b"pw", b"short");
        assert!(result.is_err());
    }

    #[test]
    fn is_encrypted_backup_detects_magic() {
        assert!(!is_encrypted_backup(b"plain text data"));
        assert!(!is_encrypted_backup(b""));

        let encrypted = encrypt_backup(b"pw", b"data").unwrap();
        assert!(is_encrypted_backup(&encrypted));
    }

    #[test]
    fn different_encryptions_have_different_salts() {
        // Same password + plaintext should produce different ciphertext
        // because of random salt + nonce.
        let e1 = encrypt_backup(b"pw", b"same data").unwrap();
        let e2 = encrypt_backup(b"pw", b"same data").unwrap();
        assert_ne!(e1, e2);

        // Both should decrypt to the same plaintext.
        assert_eq!(decrypt_backup(b"pw", &e1).unwrap(), b"same data");
        assert_eq!(decrypt_backup(b"pw", &e2).unwrap(), b"same data");
    }

    #[test]
    fn empty_plaintext_roundtrip() {
        let encrypted = encrypt_backup(b"pw", b"").unwrap();
        let decrypted = decrypt_backup(b"pw", &encrypted).unwrap();
        assert!(decrypted.is_empty());
    }

    #[test]
    fn large_plaintext_roundtrip() {
        let plaintext = vec![0xAB; 100_000];
        let encrypted = encrypt_backup(b"pw", &plaintext).unwrap();
        let decrypted = decrypt_backup(b"pw", &encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn unicode_password_roundtrip() {
        // Passwords with non-ASCII should work.
        let password = "密码🔑Passw0rd".as_bytes();
        let encrypted = encrypt_backup(password, b"data").unwrap();
        let decrypted = decrypt_backup(password, &encrypted).unwrap();
        assert_eq!(decrypted, b"data");
    }
}
