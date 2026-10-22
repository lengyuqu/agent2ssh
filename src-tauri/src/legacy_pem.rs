//! Legacy OpenSSL PEM encryption: `Proc-Type: 4,ENCRYPTED` with `DEK-Info`.
//!
//! ## Why this exists
//!
//! `ssh-keygen -m PEM -N <pass>` writes an encrypted ECDSA key as a SEC1 body
//! under `-----BEGIN EC PRIVATE KEY-----`, encrypted the way OpenSSL's
//! `PEM_write_bio_PrivateKey` has always done it: AES-128-CBC with a key derived
//! from the passphrase and the IV's first eight bytes, both named in a
//! `DEK-Info:` header.
//!
//! The transport decrypts this container, but then parses the plaintext as
//! PKCS#1 regardless of the label, so an EC key — whose plaintext is SEC1, not
//! PKCS#1 — fails with
//! `Pkcs1: PKCS#1 ASN.1 error: expected INTEGER, got OCTET STRING`. An RSA key
//! in the same container is fine, and so is a *plaintext* EC key, because that
//! path goes through the loader's SEC1-aware branch. Only the encrypted EC case
//! falls into the RSA-only one.
//!
//! ## What this module does instead
//!
//! It decrypts the container itself and returns a *plaintext* PEM with the same
//! label, which `russh::keys::decode_secret_key` already parses for every key
//! type. Nothing is written to disk and no key material is copied into a second
//! parser: the decrypted bytes are re-armoured and handed straight back.
//!
//! ## What it refuses
//!
//! Ciphers other than AES-128/192/256-CBC (OpenSSH only emits AES-128-CBC; other
//! tools use AES-256-CBC) and 3DES get a named error rather than a guess. A
//! wrong passphrase is reported as such, which the transport's own loader
//! cannot do for this container — it fails with an ASN.1 error that says
//! nothing about the passphrase.

use aes::cipher::{BlockDecrypt, BlockSizeUser, KeyInit};
use aes::{Aes128, Aes192, Aes256};
use base64::Engine as _;
use md5::{Digest, Md5};

const PEM_LINE: usize = 64;

/// Decrypt a legacy-encrypted PEM if this is one.
///
/// `Ok(None)` means the text is not legacy-encrypted and the caller should hand
/// it to the normal loader unchanged. `Err` means it *is* one and decryption
/// failed — a wrong or missing passphrase, or an unsupported cipher.
pub(crate) fn decrypt(text: &str, passphrase: Option<&str>) -> Result<Option<String>, String> {
    let mut lines = text.lines();
    let Some(label) = lines.next().and_then(pem_label) else {
        return Ok(None);
    };

    let mut encryption = None;
    let mut body = String::new();
    let mut body_started = false;
    for line in lines {
        let line = line.trim();
        if line.starts_with("-----END") {
            break;
        }
        if !body_started {
            if line.is_empty() {
                body_started = true;
                continue;
            }
            if let Some(value) = line.strip_prefix("DEK-Info:") {
                encryption = Some(value.trim().to_string());
            }
            // `Proc-Type:` and any other header is consumed here and not part of
            // the base64 body.
            continue;
        }
        body.push_str(line);
    }

    let Some(encryption) = encryption else {
        return Ok(None);
    };
    let (algorithm, iv) = encryption
        .split_once(',')
        .ok_or_else(|| format!("the DEK-Info header is malformed: {encryption}"))?;
    let iv = decode_hex(iv)?;
    let ciphertext = base64::engine::general_purpose::STANDARD
        .decode(body.as_bytes())
        .map_err(|error| format!("the encrypted body is not base64: {error}"))?;
    let passphrase = passphrase.ok_or_else(|| {
        format!("{label} is encrypted with {algorithm} and no passphrase was given")
    })?;

    let plain = match algorithm.to_ascii_uppercase().as_str() {
        "AES-128-CBC" => {
            let (key, iv) = derive_key_and_iv::<Aes128>(passphrase, &iv)?;
            cbc_decrypt::<Aes128>(&key, &iv, &ciphertext)?
        }
        "AES-192-CBC" => {
            let (key, iv) = derive_key_and_iv::<Aes192>(passphrase, &iv)?;
            cbc_decrypt::<Aes192>(&key, &iv, &ciphertext)?
        }
        "AES-256-CBC" => {
            let (key, iv) = derive_key_and_iv::<Aes256>(passphrase, &iv)?;
            cbc_decrypt::<Aes256>(&key, &iv, &ciphertext)?
        }
        other => {
            return Err(format!(
                "{label} uses {other}, which this build cannot decrypt; \
                 convert it with `ssh-keygen -p -m OpenSSH -f <key>`"
            ))
        }
    };

    Ok(Some(armour(&label, &plain)))
}

/// The label of a `-----BEGIN <label>-----` line.
fn pem_label(line: &str) -> Option<String> {
    line.trim()
        .strip_prefix("-----BEGIN ")?
        .strip_suffix("-----")
        .map(str::to_string)
}

/// OpenSSL's `EVP_BytesToKey` with MD5 and one iteration: hash the previous
/// digest (empty for the first round) together with the passphrase and the salt,
/// concatenating digests until there is enough material.
fn derive_key_and_iv<C: KeyInit + BlockSizeUser>(
    passphrase: &str,
    iv: &[u8],
) -> Result<(Vec<u8>, Vec<u8>), String> {
    // The salt is the first eight bytes of the IV; the IV itself comes from
    // `DEK-Info` verbatim.
    let salt = iv.get(..8).ok_or("the IV in DEK-Info is too short")?;
    let want = C::key_size();
    let mut material = Vec::with_capacity(want + 16);
    let mut previous: Vec<u8> = Vec::new();
    while material.len() < want {
        let mut hasher = Md5::new();
        hasher.update(&previous);
        hasher.update(passphrase.as_bytes());
        hasher.update(salt);
        previous = hasher.finalize().to_vec();
        material.extend_from_slice(&previous);
    }
    material.truncate(want);
    Ok((material, iv.to_vec()))
}

fn cbc_decrypt<C>(key: &[u8], iv: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, String>
where
    C: BlockDecrypt + KeyInit + BlockSizeUser,
{
    let block = C::block_size();
    debug_assert_eq!(block, 16, "AES block size");
    let cipher =
        C::new_from_slice(key).map_err(|_| format!("the derived key is {} bytes", key.len()))?;
    if iv.len() != block || ciphertext.is_empty() || !ciphertext.len().is_multiple_of(block) {
        return Err("the encrypted body is not a whole number of AES blocks".to_string());
    }

    let mut previous = iv.to_vec();
    let mut plain = Vec::with_capacity(ciphertext.len());
    for chunk in ciphertext.chunks_exact(block) {
        let mut block_bytes = chunk.to_vec();
        let block_ref = aes::cipher::generic_array::GenericArray::from_mut_slice(&mut block_bytes);
        cipher.decrypt_block(block_ref);
        for (index, byte) in block_bytes.iter().enumerate() {
            plain.push(byte ^ previous[index]);
        }
        previous = chunk.to_vec();
    }

    // PKCS#7: the last byte is the pad length, and that many bytes must repeat.
    let pad = *plain
        .last()
        .ok_or_else(|| "the decrypted body is empty".to_string())? as usize;
    if pad == 0 || pad > block || pad > plain.len() {
        return Err(
            "the passphrase did not decrypt this key (the padding is not valid)".to_string(),
        );
    }
    if plain[plain.len() - pad..]
        .iter()
        .any(|byte| *byte as usize != pad)
    {
        return Err(
            "the passphrase did not decrypt this key (the padding is not valid)".to_string(),
        );
    }
    plain.truncate(plain.len() - pad);
    Ok(plain)
}

/// Rebuild a plaintext PEM under `label` from DER bytes.
fn armour(label: &str, der: &[u8]) -> String {
    let encoded = base64::engine::general_purpose::STANDARD.encode(der);
    let mut out = format!("-----BEGIN {label}-----\n");
    for chunk in encoded.as_bytes().chunks(PEM_LINE) {
        out.push_str(&String::from_utf8_lossy(chunk));
        out.push('\n');
    }
    out.push_str(&format!("-----END {label}-----\n"));
    out
}

fn decode_hex(text: &str) -> Result<Vec<u8>, String> {
    let text = text.trim();
    if !text.len().is_multiple_of(2) {
        return Err(format!("the DEK-Info IV is not valid hex: {text}"));
    }
    (0..text.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&text[index..index + 2], 16)
                .map_err(|_| format!("the DEK-Info IV is not valid hex: {text}"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `ssh-keygen -t ecdsa -b 256 -m PEM -N fix-pass` output. Public half below.
    const ENCRYPTED_EC: &str = "\
-----BEGIN EC PRIVATE KEY-----
Proc-Type: 4,ENCRYPTED
DEK-Info: AES-128-CBC,E9C8B44E75471E6CEC54B8EE1C91FA15

vbvJ+MEdLWAVH2WHTgHMC2WKmTJVlqe6dmbMfJEDfR2wdwC8QQnyKVNpDrUT/xZH
QVfOJdbuP9gndpzMo+BmtO3rpVJFtPKh96Lkmr96ESkG0Rj5SZJJPhS4D9NazbCV
1zo2Mqf8g6Nyq/LjwyfGI7gHaAwn+RrEb7Cl/zTuvrc=
-----END EC PRIVATE KEY-----
";
    const ENCRYPTED_EC_PUB: &str = "ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTYAAAAIbmlzdHAyNTYAAABBBI8nLHD7b2XI7ZE2t6KU/9x8EBaY6gn02zzzNH7Wd5US840+ChILRHJYixpgpWcdpgKhnCjhrqe1V7RpbQZhbUQ=";

    #[test]
    fn decrypts_an_encrypted_ec_pem_into_a_key_the_loader_accepts() {
        let plain = decrypt(ENCRYPTED_EC, Some("fix-pass"))
            .expect("decrypt")
            .expect("this is a legacy-encrypted PEM");
        assert!(plain.starts_with("-----BEGIN EC PRIVATE KEY-----"));
        assert!(!plain.contains("Proc-Type"), "headers must not survive");

        let key = russh::keys::decode_secret_key(&plain, None).expect("parse");
        let public = key.public_key().to_openssh().expect("encode");
        assert_eq!(public.split(' ').nth(1), ENCRYPTED_EC_PUB.split(' ').nth(1));
    }

    #[test]
    fn a_wrong_passphrase_is_reported_as_such() {
        let error = decrypt(ENCRYPTED_EC, Some("wrong")).unwrap_err();
        assert!(error.contains("passphrase"), "{error}");
    }

    #[test]
    fn a_missing_passphrase_names_the_requirement() {
        let error = decrypt(ENCRYPTED_EC, None).unwrap_err();
        assert!(error.contains("no passphrase"), "{error}");
    }

    #[test]
    fn plaintext_pem_is_left_to_the_normal_loader() {
        let plain = "-----BEGIN EC PRIVATE KEY-----\nAAAA\n-----END EC PRIVATE KEY-----\n";
        assert!(decrypt(plain, None).unwrap().is_none());
    }

    #[test]
    fn the_openssh_container_is_left_alone() {
        let text = "-----BEGIN OPENSSH PRIVATE KEY-----\nAAAA\n-----END OPENSSH PRIVATE KEY-----\n";
        assert!(decrypt(text, Some("x")).unwrap().is_none());
    }

    #[test]
    fn evp_bytes_to_key_matches_openssl_for_a_known_iv() {
        // Derived by hand from the format: salt = first 8 IV bytes, one MD5
        // round for a 16-byte key.
        let iv = decode_hex("E9C8B44E75471E6CEC54B8EE1C91FA15").unwrap();
        let (key, _) = derive_key_and_iv::<Aes128>("fix-pass", &iv).unwrap();
        let mut hasher = Md5::new();
        hasher.update(b"fix-pass");
        hasher.update(&iv[..8]);
        assert_eq!(key, hasher.finalize().to_vec());
    }
}
