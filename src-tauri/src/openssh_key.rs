//! The OpenSSH private-key container and the single-line public-key form.
//!
//! ## Why this exists
//!
//! Key generation used to go through the `ssh-key` crate. That crate pins exact
//! RustCrypto pre-releases (`curve25519-dalek`, `ed25519-dalek`, `pkcs8`) which
//! cannot coexist with the ones russh 0.60 pins — the two sets have no common
//! version, so simply having both crates in the graph fails to resolve. Since
//! this crate only ever needed three things from `ssh-key` — derive the public
//! key from a seed, write the container, read it back — and the container format
//! is a short, fully specified structure (`PROTOCOL.key` in OpenSSH), it is
//! cheaper to own those 150 lines than to own that dependency conflict.
//!
//! The derivation itself is *not* reimplemented: [`ed25519_dalek`] computes the
//! public key from the seed. Only the encoding is ours.
//!
//! ## Format
//!
//! ```text
//! "openssh-key-v1\0"
//! string  ciphername     "none"
//! string  kdfname        "none"
//! string  kdfoptions     ""
//! uint32  nkeys          1
//! string  publickey      wire format: string "ssh-ed25519" + string pubkey
//! string  private        uint32 checkint, uint32 checkint (again)
//!                        string "ssh-ed25519", string pubkey, string seed||pubkey
//!                        string comment, then 1,2,3… padding to an 8-byte block
//! ```
//!
//! `string` is a `u32` big-endian length followed by that many bytes.

use base64::Engine as _;
use ed25519_dalek::SigningKey;

/// Magic that distinguishes the container from a PEM PKCS#8 file.
const MAGIC: &[u8] = b"openssh-key-v1\0";
/// Cipher block size used to compute the padding length. `none` has no cipher,
/// and the specification still pads to the 8-byte block of the old default.
const BLOCK_SIZE: usize = 8;
/// The only key type this module writes.
const KEY_TYPE: &str = "ssh-ed25519";

/// An Ed25519 pair in the form the container stores: the seed, the public key it
/// derives, and the comment that is written into the private section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ed25519KeyMaterial {
    /// The 32-byte secret seed. The container stores this, not the expanded key.
    pub seed: [u8; 32],
    /// `SigningKey::verifying_key().to_bytes()`, i.e. the 32-byte public key.
    pub public: [u8; 32],
    pub comment: String,
}

/// Build the material for `seed`, deriving the public half.
pub fn ed25519_material(seed: [u8; 32], comment: &str) -> Ed25519KeyMaterial {
    let public = SigningKey::from_bytes(&seed).verifying_key().to_bytes();
    Ed25519KeyMaterial {
        seed,
        public,
        comment: comment.to_string(),
    }
}

fn put_string(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    out.extend_from_slice(bytes);
}

fn read_string<'a>(input: &'a [u8], pos: &mut usize) -> Option<&'a [u8]> {
    let len_end = pos.checked_add(4)?;
    let len_bytes = input.get(*pos..len_end)?;
    let len = u32::from_be_bytes(len_bytes.try_into().ok()?) as usize;
    let start = len_end;
    let end = start.checked_add(len)?;
    let bytes = input.get(start..end)?;
    *pos = end;
    Some(bytes)
}

fn read_u32(input: &[u8], pos: &mut usize) -> Option<u32> {
    let end = pos.checked_add(4)?;
    let bytes = input.get(*pos..end)?;
    *pos = end;
    Some(u32::from_be_bytes(bytes.try_into().ok()?))
}

/// The wire form of an Ed25519 public key: `string "ssh-ed25519" + string key`.
fn public_blob(public: &[u8; 32]) -> Vec<u8> {
    let mut blob = Vec::with_capacity(4 + KEY_TYPE.len() + 4 + public.len());
    put_string(&mut blob, KEY_TYPE.as_bytes());
    put_string(&mut blob, public);
    blob
}

/// The `authorized_keys` line: `<type> <base64> <comment>`.
pub fn encode_public(public: &[u8; 32], comment: &str) -> String {
    let encoded = base64::engine::general_purpose::STANDARD.encode(public_blob(public));
    if comment.is_empty() {
        format!("{KEY_TYPE} {encoded}")
    } else {
        format!("{KEY_TYPE} {encoded} {comment}")
    }
}

/// Serialize the private key as the `-----BEGIN OPENSSH PRIVATE KEY-----` PEM.
pub fn encode_private(material: &Ed25519KeyMaterial) -> String {
    let blob = public_blob(&material.public);

    let mut private = Vec::new();
    // Two equal check words: the reader uses them to tell a wrong passphrase
    // from a corrupt file. With no cipher there is no passphrase, so any value
    // works as long as both halves agree.
    let check = u32::from_be_bytes([0xA5, 0x5A, 0xC3, 0x3C]);
    private.extend_from_slice(&check.to_be_bytes());
    private.extend_from_slice(&check.to_be_bytes());
    put_string(&mut private, KEY_TYPE.as_bytes());
    put_string(&mut private, &material.public);
    // The secret is seed || public, not the expanded secret key.
    let mut secret = [0u8; 64];
    secret[..32].copy_from_slice(&material.seed);
    secret[32..].copy_from_slice(&material.public);
    put_string(&mut private, &secret);
    put_string(&mut private, material.comment.as_bytes());

    let mut padding = 1u8;
    while private.len() % BLOCK_SIZE != 0 {
        private.push(padding);
        padding = padding.wrapping_add(1);
    }

    let mut out = Vec::with_capacity(MAGIC.len() + 64 + private.len());
    out.extend_from_slice(MAGIC);
    put_string(&mut out, b"none"); // ciphername
    put_string(&mut out, b"none"); // kdfname
    put_string(&mut out, b""); // kdfoptions
    out.extend_from_slice(&1u32.to_be_bytes()); // nkeys
    put_string(&mut out, &blob);
    put_string(&mut out, &private);

    let body = base64::engine::general_purpose::STANDARD.encode(&out);
    let mut pem = String::from("-----BEGIN OPENSSH PRIVATE KEY-----\n");
    for chunk in body.as_bytes().chunks(70) {
        pem.push_str(std::str::from_utf8(chunk).expect("base64 is ASCII"));
        pem.push('\n');
    }
    pem.push_str("-----END OPENSSH PRIVATE KEY-----\n");
    pem
}

/// Read a container this module wrote, or any unencrypted Ed25519 one.
///
/// Returns `None` for anything else: a different key type, an encrypted
/// container, a mismatched check word, or trailing bytes that are not the
/// 1,2,3… padding. Callers use this as a verifier, so "did not understand it" is
/// deliberately indistinguishable from "malformed".
pub fn parse_private(pem: &str) -> Option<Ed25519KeyMaterial> {
    let body = pem
        .trim()
        .strip_prefix("-----BEGIN OPENSSH PRIVATE KEY-----")?
        .strip_suffix("-----END OPENSSH PRIVATE KEY-----")?
        .trim();
    let raw = base64::engine::general_purpose::STANDARD
        .decode(body.split_whitespace().collect::<String>())
        .ok()?;
    let raw = raw.strip_prefix(MAGIC)?;

    let mut pos = 0usize;
    if read_string(raw, &mut pos)? != b"none" {
        return None;
    }
    if read_string(raw, &mut pos)? != b"none" {
        return None;
    }
    read_string(raw, &mut pos)?; // kdfoptions, empty for `none`
    if read_u32(raw, &mut pos)? != 1 {
        return None;
    }
    let blob = read_string(raw, &mut pos)?;
    let private = read_string(raw, &mut pos)?;

    let mut key_pos = 0usize;
    if read_string(blob, &mut key_pos)? != KEY_TYPE.as_bytes() {
        return None;
    }
    let public: [u8; 32] = read_string(blob, &mut key_pos)?.try_into().ok()?;

    let mut pos = 0usize;
    let first = read_u32(private, &mut pos)?;
    if read_u32(private, &mut pos)? != first {
        return None;
    }
    if read_string(private, &mut pos)? != KEY_TYPE.as_bytes() {
        return None;
    }
    if read_string(private, &mut pos)? != public {
        return None;
    }
    let secret = read_string(private, &mut pos)?;
    if secret.len() != 64 || secret[32..] != public {
        return None;
    }
    let seed: [u8; 32] = secret[..32].try_into().ok()?;
    let comment = String::from_utf8(read_string(private, &mut pos)?.to_vec()).ok()?;

    // Whatever is left must be the ascending padding bytes.
    for (index, byte) in private[pos..].iter().enumerate() {
        if *byte != (index as u8).wrapping_add(1) {
            return None;
        }
    }

    Some(Ed25519KeyMaterial {
        seed,
        public,
        comment,
    })
}

/// A parsed `authorized_keys` line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicKeyLine {
    /// The algorithm name, e.g. `ssh-ed25519` or `ssh-rsa`.
    pub algorithm: String,
    /// Everything after the algorithm name in the base64 payload, i.e. the
    /// remaining strings of the wire format.
    pub blob: Vec<u8>,
    /// The trailing comment, empty when absent.
    pub comment: String,
}

/// Parse a single-line public key.
///
/// The point of this parser is to check the hand-rolled encoders against
/// independent key material, so it reads the structure and hands the payload
/// back rather than interpreting it.
pub fn parse_public_key_line(line: &str) -> Option<PublicKeyLine> {
    let mut fields = line.split_whitespace();
    let algorithm = fields.next()?.to_string();
    let encoded = fields.next()?;
    let comment = fields.collect::<Vec<_>>().join(" ");
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()?;

    let mut pos = 0usize;
    if read_string(&decoded, &mut pos)? != algorithm.as_bytes() {
        return None;
    }
    Some(PublicKeyLine {
        algorithm,
        blob: decoded[pos..].to_vec(),
        comment,
    })
}

/// The two `mpint`s an `ssh-rsa` public key carries: exponent then modulus.
pub fn rsa_exponent_and_modulus(blob: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    let mut pos = 0usize;
    let exponent = read_string(blob, &mut pos)?.to_vec();
    let modulus = read_string(blob, &mut pos)?.to_vec();
    Some((exponent, modulus))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn material() -> Ed25519KeyMaterial {
        ed25519_material([7u8; 32], "agent2ssh-test")
    }

    #[test]
    fn private_key_round_trips_through_the_container() {
        let original = material();
        let pem = encode_private(&original);
        let parsed = parse_private(&pem).expect("own container parses");
        assert_eq!(parsed, original);
    }

    #[test]
    fn private_key_has_the_openssh_armour_and_wraps_at_70_columns() {
        let pem = encode_private(&material());
        assert!(pem.starts_with("-----BEGIN OPENSSH PRIVATE KEY-----\n"));
        assert!(pem.ends_with("-----END OPENSSH PRIVATE KEY-----\n"));
        for line in pem.lines().filter(|l| !l.starts_with("-----")) {
            assert!(line.len() <= 70, "line too long: {line}");
        }
    }

    #[test]
    fn public_key_is_a_single_authorized_keys_line() {
        let material = material();
        let line = encode_public(&material.public, &material.comment);
        assert!(line.starts_with("ssh-ed25519 "));
        assert!(line.ends_with(" agent2ssh-test"));

        let parsed = parse_public_key_line(&line).expect("own line parses");
        assert_eq!(parsed.algorithm, "ssh-ed25519");
        // The payload keeps the wire framing: a length-prefixed key string.
        let mut expected = Vec::new();
        put_string(&mut expected, &material.public);
        assert_eq!(parsed.blob, expected);
        assert_eq!(parsed.comment, "agent2ssh-test");
    }

    #[test]
    fn public_key_without_a_comment_omits_the_trailing_space() {
        let line = encode_public(&material().public, "");
        assert_eq!(line.split_whitespace().count(), 2);
        assert_eq!(parse_public_key_line(&line).unwrap().comment, "");
    }

    #[test]
    fn the_public_half_matches_the_one_derived_from_the_seed() {
        // Guards against writing a container whose stored public key does not
        // belong to the stored seed -- an authentication failure that would
        // otherwise only show up against a real server.
        let material = ed25519_material([9u8; 32], "");
        let derived = SigningKey::from_bytes(&material.seed)
            .verifying_key()
            .to_bytes();
        assert_eq!(material.public, derived);
    }

    #[test]
    fn a_flipped_check_word_is_rejected() {
        let pem = encode_private(&material());
        let body: String = pem
            .lines()
            .filter(|l| !l.starts_with("-----"))
            .collect::<Vec<_>>()
            .join("");
        let mut raw = base64::engine::general_purpose::STANDARD
            .decode(body)
            .unwrap();

        // Layout up to the first check word: magic (15) + "none" (8) + "none"
        // (8) + "" (4) + nkeys (4) + public blob (4 + 51) + private length (4)
        // = 98. The check word is the first four bytes after that.
        const CHECK_WORD_OFFSET: usize = 98;
        raw[CHECK_WORD_OFFSET] ^= 0xFF;

        let body = base64::engine::general_purpose::STANDARD.encode(&raw);
        let tampered = format!(
            "-----BEGIN OPENSSH PRIVATE KEY-----\n{body}\n-----END OPENSSH PRIVATE KEY-----\n"
        );
        assert!(parse_private(&tampered).is_none());
    }

    #[test]
    fn foreign_and_truncated_input_are_rejected() {
        assert!(parse_private(
            "-----BEGIN RSA PRIVATE KEY-----\nAAAA\n-----END RSA PRIVATE KEY-----\n"
        )
        .is_none());
        assert!(parse_private("not a key").is_none());
        let pem = encode_private(&material());
        let truncated = &pem[..pem.len() / 2];
        assert!(parse_private(truncated).is_none());
    }

    #[test]
    fn rsa_public_key_parts_are_readable() {
        // One-byte exponent, small modulus: enough to prove the walk works.
        let mut blob = Vec::new();
        put_string(&mut blob, &[0x01, 0x00, 0x01]);
        put_string(&mut blob, &[0x00, 0xC5]);
        let (e, n) = rsa_exponent_and_modulus(&blob).unwrap();
        assert_eq!(e, vec![0x01, 0x00, 0x01]);
        assert_eq!(n, vec![0x00, 0xC5]);
    }
}
