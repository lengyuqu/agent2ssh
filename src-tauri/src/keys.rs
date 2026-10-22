use anyhow::{anyhow, Result};
use base64::Engine as _;
use rand_core::{CryptoRng, RngCore};
use rsa::pkcs1::{EncodeRsaPrivateKey, LineEnding as PemLineEnding};
use rsa::traits::PublicKeyParts;
use rsa::RsaPrivateKey;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use zeroize::Zeroizing;

use crate::{
    path_resolver::expand_tilde,
    store::{config_dir, restrict_file_to_owner},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshKeyInfo {
    /// Filename of the private key (e.g. "id_ed25519_work")
    pub name: String,
    /// Full path to the private key file
    pub private_path: String,
    /// Full path to the public key file (name + ".pub")
    pub public_path: String,
    /// The public key content (e.g. "ssh-ed25519 AAAA... user@host")
    #[serde(default)]
    pub public_key: String,
    /// Key type (e.g. "ed25519", "rsa")
    #[serde(default)]
    pub key_type: String,
    /// Creation timestamp
    #[serde(default)]
    pub created_at: Option<String>,
}

fn keys_dir() -> Result<PathBuf> {
    let dir = config_dir()?.join("keys");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn restrict_private_key_permissions(path: impl AsRef<std::path::Path>) -> Result<()> {
    restrict_file_to_owner(path)
}

/// S1: Validate that a name is a safe filename component — no path traversal,
/// no absolute paths, no separators.
///
/// Rejects both Unix (`/`) and Windows (`\`) path separators explicitly.
/// Relying on platform `Path` semantics is inconsistent across OSes: on Unix,
/// `Path::new("nested\\key").file_name()` returns the whole string (backslash
/// is a plain character), letting `\`-separated names through that would
/// traverse directories on Windows. Also rejects "." and "..".
pub fn is_safe_filename(name: &str) -> bool {
    !name.contains('/') && !name.contains('\\') && name != "." && name != ".."
}

fn validate_key_name(name: &str) -> Result<()> {
    if name.trim().is_empty() {
        return Err(anyhow!("key name is required"));
    }
    if !is_safe_filename(name) {
        return Err(anyhow!(
            "invalid key name: path separators or traversal detected"
        ));
    }
    Ok(())
}

/// List all key pairs in ~/.agent2ssh/keys/
pub fn list_keys_core() -> Result<Vec<SshKeyInfo>> {
    let dir = keys_dir()?;
    let mut keys = Vec::new();

    if !dir.exists() {
        return Ok(keys);
    }

    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        // Skip .pub files and hidden files
        if name.ends_with(".pub") || name.starts_with('.') {
            continue;
        }

        let pub_path = path.with_extension(format!(
            "{}.pub",
            path.extension()
                .map(|e| e.to_string_lossy().to_string())
                .unwrap_or_default()
        ));
        // Handle keys without extension
        let pub_path = if !pub_path.exists() {
            PathBuf::from(format!("{}.pub", path.display()))
        } else {
            pub_path
        };

        let public_key = if pub_path.exists() {
            std::fs::read_to_string(&pub_path)
                .unwrap_or_default()
                .trim()
                .to_string()
        } else {
            String::new()
        };

        let key_type = if name.contains("ed25519") {
            "ed25519"
        } else if name.contains("rsa") {
            "rsa"
        } else if name.contains("ecdsa") {
            "ecdsa"
        } else if public_key.starts_with("ssh-ed25519") {
            "ed25519"
        } else if public_key.starts_with("ssh-rsa") {
            "rsa"
        } else {
            "unknown"
        }
        .to_string();

        let created_at = std::fs::metadata(&path)
            .ok()
            .and_then(|m| m.created().ok())
            .map(|t| {
                let dt: chrono::DateTime<chrono::Utc> = t.into();
                dt.to_rfc3339()
            });

        keys.push(SshKeyInfo {
            name: name.clone(),
            private_path: path.display().to_string(),
            public_path: pub_path.display().to_string(),
            public_key,
            key_type,
            created_at,
        });
    }

    keys.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(keys)
}

/// Bits used for generated RSA keys.
///
/// 3072 is the smallest size that reaches a 128-bit security level, and is what
/// `ssh-keygen -t rsa` has defaulted to since OpenSSH 9.5. It is also the size
/// verified end to end against a real `sshd` in the Windows container fixture,
/// so the number in the UI is a number that has actually authenticated.
pub const RSA_BITS: usize = 3072;

/// Below this, an RSA key no longer reaches the 128-bit security level the app
/// advertises, so generation refuses rather than producing a weak key.
const RSA_BITS_MIN: usize = 2048;

/// A key algorithm this app can generate.
///
/// The set is deliberately tiny. The default used to be derived from what the
/// linked libssh2 could authenticate with; the russh transport reads every
/// format, so it is now simply Ed25519. See [`KeyAlgorithm::default_for_build`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KeyAlgorithm {
    /// Ed25519, written in the OpenSSH private-key container.
    ///
    /// The better key: smaller, faster, and not dependent on prime generation.
    Ed25519,
    /// RSA, written as traditional PKCS#1 PEM (`-----BEGIN RSA PRIVATE KEY-----`).
    ///
    /// The old WinCNG-backed transport could not read the OpenSSH container, so
    /// this was the only form that authenticated there; that constraint is gone
    /// and the variant survives because RSA is still a choice a user can make.
    Rsa,
}

impl KeyAlgorithm {
    /// The algorithm to use when the caller expresses no preference.
    ///
    /// Always Ed25519 now. It used to consult a capability probe because the old
    /// transport could only read whichever key formats its crypto backend
    /// supplied — a WinCNG build had to fall back to the RSA-PEM form. The russh
    /// transport reads every format this crate writes, so there is nothing left
    /// to probe and no build where the answer differs.
    ///
    /// Kept as a named function rather than folded into call sites: the choice
    /// is still a product decision worth locating in one place.
    pub fn default_for_build() -> Self {
        Self::Ed25519
    }

    /// Parse an algorithm name from a UI or IPC payload, case-insensitively.
    pub fn from_name(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "ed25519" => Some(Self::Ed25519),
            "rsa" => Some(Self::Rsa),
            _ => None,
        }
    }

    /// The value carried over IPC and shown in the UI (`SshKeyInfo::key_type`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ed25519 => "ed25519",
            Self::Rsa => "rsa",
        }
    }
}

/// A freshly generated key pair, still in memory.
///
/// The private half stays in a [`Zeroizing`] buffer right up to the `write`, so
/// the only plain copy on the heap is the one `std::fs::write` is handed.
struct GeneratedKeyPair {
    private_pem: Zeroizing<String>,
    /// The single-line `ssh-…` form an `authorized_keys` file expects, without
    /// a trailing newline.
    public_openssh: String,
    key_type: &'static str,
}

/// The raw entropy source: fills `dest` entirely, or fails.
type EntropySource = fn(&mut [u8]) -> Result<()>;

/// The message used when generation aborts because entropy is unavailable. The
/// caller normally replaces it with the recorded, more specific reason; it only
/// reaches the user if the panic escapes [`generate_rsa_pair`], which would
/// itself be a bug worth making loud.
const ENTROPY_ABORT_MESSAGE: &str =
    "aborted key generation because the operating system entropy source failed";

/// Adapts [`fill_entropy`] to the infallible `RngCore` interface `rsa` needs.
///
/// `RngCore::fill_bytes` cannot report an error, and neither quiet option is
/// acceptable:
///
/// * leaving the buffer zeroed or untouched makes `num-bigint-dig`'s `gen_prime`
///   loop forever — it forces a fixed candidate (top two bits and the low bit
///   set) and retries until *that same number* is prime, which for a composite
///   is never;
/// * substituting bytes from a deterministic generator risks emitting a key
///   whose factors are recoverable.
///
/// So a failed read aborts instead: the reason is recorded, and `fill_bytes`
/// panics rather than returning unusable bytes. `fill_bytes` is documented as
/// allowed to panic when it cannot fill the buffer, and this is the same
/// contract `rand_core::OsRng` has. [`generate_rsa_pair`] is the only caller,
/// and it runs the generator under `catch_unwind` so the panic becomes an error
/// rather than a crash.
struct EntropyRng {
    source: EntropySource,
    failure: Option<String>,
}

impl EntropyRng {
    fn new() -> Self {
        Self {
            source: fill_entropy,
            failure: None,
        }
    }

    /// Constructor for tests, so the failure path is reachable without breaking
    /// the machine's entropy source.
    #[cfg(test)]
    fn with_source(source: EntropySource) -> Self {
        Self {
            source,
            failure: None,
        }
    }

    /// Take the recorded failure, if the source ever failed.
    fn take_failure(&mut self) -> Option<String> {
        self.failure.take()
    }

    fn read(&mut self, dest: &mut [u8]) {
        // Once the source has failed there is nothing to gain from calling it
        // again — one key search hits it hundreds of times — so abort at once.
        if self.failure.is_some() {
            panic!("{ENTROPY_ABORT_MESSAGE}");
        }
        if let Err(error) = (self.source)(dest) {
            self.failure = Some(error.to_string());
            panic!("{ENTROPY_ABORT_MESSAGE}");
        }
    }
}

impl RngCore for EntropyRng {
    fn next_u32(&mut self) -> u32 {
        let mut bytes = [0u8; 4];
        self.read(&mut bytes);
        u32::from_le_bytes(bytes)
    }

    fn next_u64(&mut self) -> u64 {
        let mut bytes = [0u8; 8];
        self.read(&mut bytes);
        u64::from_le_bytes(bytes)
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        self.read(dest);
    }

    /// Delegates to [`RngCore::fill_bytes`], which the trait explicitly allows.
    ///
    /// A `rand_core::Error` cannot be built here without pulling in a second
    /// `getrandom` major version, and inventing one would hide the reason the
    /// source gave. The panic is caught by [`generate_rsa_pair`] anyway.
    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> std::result::Result<(), rand_core::Error> {
        self.fill_bytes(dest);
        Ok(())
    }
}

impl CryptoRng for EntropyRng {}

/// The message inside a panic payload, for reporting a caught panic.
fn panic_payload_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|text| text.to_string()))
        .unwrap_or_else(|| "the generator panicked".to_string())
}

/// Generate an Ed25519 pair in the OpenSSH container format.
///
/// The container is written by [`crate::openssh_key`] rather than a library:
/// the only crate that could do it here (`ssh-key`) pins RustCrypto
/// pre-releases that cannot coexist with russh's, and the format is short
/// enough to own. The public key is still derived by `ed25519-dalek`, so no key
/// arithmetic is hand-rolled.
fn generate_ed25519_pair(comment: &str) -> Result<GeneratedKeyPair> {
    let mut seed = [0u8; 32];
    fill_entropy(&mut seed)?;
    let material = crate::openssh_key::ed25519_material(seed, comment);

    Ok(GeneratedKeyPair {
        private_pem: Zeroizing::new(crate::openssh_key::encode_private(&material)),
        public_openssh: crate::openssh_key::encode_public(&material.public, comment),
        key_type: KeyAlgorithm::Ed25519.as_str(),
    })
}

/// Generate an RSA pair.
///
/// The private key is traditional PKCS#1 PEM rather than the OpenSSH container
/// `ssh-keygen` would write, simply because that is what the `rsa` crate emits;
/// the transport reads both. The OpenSSH container would have to be hand-rolled,
/// and the previous reason for caring — a WinCNG backend that could not parse
/// it — no longer exists.
///
/// The public key is still the standard single-line `ssh-rsa …` form, which is
/// what every `authorized_keys` file and every server expects.
fn generate_rsa_pair(bits: usize, comment: &str) -> Result<GeneratedKeyPair> {
    if bits < RSA_BITS_MIN {
        return Err(anyhow!(
            "refusing to generate a {bits}-bit RSA key: the minimum is {RSA_BITS_MIN} bits"
        ));
    }

    let mut rng = EntropyRng::new();

    // Prime generation is driven by `rsa`, which only ever sees the infallible
    // `RngCore` interface; `EntropyRng` aborts by panicking when entropy is
    // unavailable. Catch that here so a broken entropy source surfaces as an
    // error instead of taking the app down. `AssertUnwindSafe` is sound: `rng`
    // is inspected immediately afterwards and neither `rsa` nor `rng` is left
    // in a state that is observed by anyone else.
    let generated = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        RsaPrivateKey::new(&mut rng, bits)
    }));

    // Consult the recorded reason before the panic payload: it names the real
    // problem, while the payload only repeats the abort message.
    if let Some(reason) = rng.take_failure() {
        let _ = crate::diagnostics::append_diagnostic_log(
            "error",
            "keys",
            "RSA key generation aborted: entropy unavailable",
            Some(serde_json::json!({ "bits": bits, "reason": reason })),
        );
        return Err(anyhow!(
            "could not generate an RSA key because the operating system entropy source failed: {reason}"
        ));
    }

    let private_key = match generated {
        Ok(Ok(key)) => key,
        Ok(Err(error)) => return Err(anyhow!("could not generate an RSA key: {error}")),
        // Reached when the generator itself panics — `num-bigint-dig` has an
        // internal `panic!` in its primality test — so the payload is the only
        // description of what happened.
        Err(payload) => {
            return Err(anyhow!(
                "could not generate an RSA key: {}",
                panic_payload_message(&*payload)
            ))
        }
    };

    Ok(GeneratedKeyPair {
        private_pem: private_key
            .to_pkcs1_pem(PemLineEnding::LF)
            .map_err(|error| anyhow!("could not encode the generated RSA key as PEM: {error}"))?,
        public_openssh: encode_ssh_rsa_public_key(&private_key, comment),
        key_type: KeyAlgorithm::Rsa.as_str(),
    })
}

/// Encode an RSA public key as the single-line `ssh-rsa <base64> [comment]` form
/// an `authorized_keys` file expects.
///
/// Hand-rolled because `ssh-key` cannot produce it for a key that came from
/// `rsa`: its `Mpint` type belongs to a pre-release `ssh-encoding` this crate
/// does not otherwise depend on. The wire format is fixed by RFC 4253 §6.6, and
/// `test_generate_key_core_with_rsa_writes_traditional_pem` checks this encoder
/// against `ssh-key`'s own parser — and the E2E against a real sshd — so the two
/// cannot silently diverge.
fn encode_ssh_rsa_public_key(key: &RsaPrivateKey, comment: &str) -> String {
    let mut blob = Vec::new();
    push_ssh_string(&mut blob, b"ssh-rsa");
    push_ssh_mpint(&mut blob, &key.e().to_bytes_be());
    push_ssh_mpint(&mut blob, &key.n().to_bytes_be());

    let encoded = base64::engine::general_purpose::STANDARD.encode(&blob);
    if comment.is_empty() {
        format!("ssh-rsa {encoded}")
    } else {
        format!("ssh-rsa {encoded} {comment}")
    }
}

/// Append a length-prefixed byte string (`string` in RFC 4251 §5).
fn push_ssh_string(buffer: &mut Vec<u8>, value: &[u8]) {
    buffer.extend_from_slice(&(value.len() as u32).to_be_bytes());
    buffer.extend_from_slice(value);
}

/// Append a length-prefixed non-negative integer (`mpint` in RFC 4251 §5).
///
/// `mpint` is signed, so a magnitude whose top bit is set needs a leading zero
/// byte to stay positive. `e` and `n` are always positive, so nothing else is
/// required: no sign byte for negatives, and no truncation of leading zeros
/// because `to_bytes_be` is already minimal.
fn push_ssh_mpint(buffer: &mut Vec<u8>, magnitude: &[u8]) {
    let pad = magnitude.first().is_some_and(|byte| byte & 0x80 != 0);
    let length = magnitude.len() + usize::from(pad);

    buffer.extend_from_slice(&(length as u32).to_be_bytes());
    if pad {
        buffer.push(0);
    }
    buffer.extend_from_slice(magnitude);
}

/// Generate a new key pair using the algorithm that fits this build.
///
/// See [`generate_key_core_with`] to name the algorithm explicitly.
pub fn generate_key_core(name: &str, comment: Option<&str>) -> Result<SshKeyInfo> {
    generate_key_core_with(name, comment, KeyAlgorithm::default_for_build())
}

/// Generate a new key pair in `~/.agent2ssh/keys/`.
///
/// `comment` becomes the OpenSSH comment for Ed25519 keys and the trailing
/// comment of the `ssh-rsa …` line for RSA keys — traditional PKCS#1 PEM has
/// nowhere to store one, so the `.pub` file is the only place it can live.
pub fn generate_key_core_with(
    name: &str,
    comment: Option<&str>,
    algorithm: KeyAlgorithm,
) -> Result<SshKeyInfo> {
    validate_key_name(name)?;

    let dir = keys_dir()?;
    let private_path = dir.join(name);
    let public_path = dir.join(format!("{}.pub", name));

    if private_path.exists() {
        return Err(anyhow!("key '{}' already exists", name));
    }

    let comment = comment.unwrap_or("agent2ssh");
    let pair = match algorithm {
        KeyAlgorithm::Ed25519 => generate_ed25519_pair(comment)?,
        KeyAlgorithm::Rsa => generate_rsa_pair(RSA_BITS, comment)?,
    };

    std::fs::write(&private_path, pair.private_pem.as_bytes())?;
    restrict_private_key_permissions(&private_path)?;
    std::fs::write(&public_path, format!("{}\n", pair.public_openssh))?;

    Ok(SshKeyInfo {
        name: name.to_string(),
        private_path: private_path.display().to_string(),
        public_path: public_path.display().to_string(),
        public_key: pair.public_openssh,
        key_type: pair.key_type.to_string(),
        created_at: Some(chrono::Utc::now().to_rfc3339()),
    })
}

fn fill_entropy(seed: &mut [u8]) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        let source = "linux-urandom";
        use std::io::Read;

        let mut file = std::fs::File::open("/dev/urandom").map_err(|e| {
            log_entropy_error(source, &e.to_string());
            anyhow!("failed to open /dev/urandom ({source}): {e}")
        })?;
        file.read_exact(seed).map_err(|e| {
            log_entropy_error(source, &e.to_string());
            anyhow!("failed to read entropy from /dev/urandom ({source}): {e}")
        })?;
        // Block-final expression (no `return`): on Linux this cfg block is the
        // function tail, so the block value becomes the return value. A bare
        // `return Ok(())` here trips clippy::needless_return on Linux builds.
        Ok(())
    }

    #[cfg(target_os = "macos")]
    {
        use std::ffi::c_void;

        unsafe {
            extern "C" {
                fn arc4random_buf(buf: *mut c_void, nbytes: usize);
            }

            arc4random_buf(seed.as_mut_ptr().cast::<c_void>(), seed.len());
        }
        Ok(())
    }

    #[cfg(target_os = "windows")]
    {
        let source = "windows-BCryptGenRandom";
        use std::ptr::null_mut;

        const BCRYPT_USE_SYSTEM_PREFERRED_RNG: u32 = 0x00000002;

        unsafe {
            extern "system" {
                fn BCryptGenRandom(
                    hAlgorithm: *mut std::ffi::c_void,
                    pbBuffer: *mut u8,
                    cbBuffer: u32,
                    dwFlags: u32,
                ) -> u32;
            }

            let status = BCryptGenRandom(
                null_mut(),
                seed.as_mut_ptr(),
                seed.len() as u32,
                BCRYPT_USE_SYSTEM_PREFERRED_RNG,
            );
            if status == 0 {
                return Ok(());
            }

            log_entropy_error(source, &format!("BCryptGenRandom status=0x{status:08x}"));
            Err(anyhow!(
                "failed to read entropy via BCryptGenRandom ({source}): status {status:#x}"
            ))
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        let source = "fallback-getrandom";
        getrandom::fill(seed).map_err(|e| {
            log_entropy_error(source, &e.to_string());
            anyhow!("failed to read entropy via getrandom ({source}): {e}")
        })?;
        return Ok(());
    }
}

#[cfg(not(target_os = "macos"))]
fn log_entropy_error(source: &str, details: &str) {
    let _ = crate::diagnostics::append_diagnostic_log(
        "error",
        "keys",
        "random source failed",
        Some(serde_json::json!({
            "source": source,
            "details": details,
        })),
    );
}

/// Import an existing private key file into the keys directory
pub fn import_key_core(source_path: &str, name: Option<&str>) -> Result<SshKeyInfo> {
    let source = PathBuf::from(expand_tilde(source_path));
    if !source.exists() {
        return Err(anyhow!("source key not found: {}", source.display()));
    }

    let key_name: String = match name {
        Some(n) => n.to_string(),
        None => source
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "imported_key".to_string()),
    };
    validate_key_name(&key_name)?;

    let dir = keys_dir()?;
    let dest = dir.join(&key_name);

    if dest.exists() {
        return Err(anyhow!(
            "key '{}' already exists in keys directory",
            key_name
        ));
    }

    std::fs::copy(&source, &dest)?;
    restrict_private_key_permissions(&dest)?;

    // Try to copy .pub file too
    let source_pub = PathBuf::from(format!("{}.pub", source.display()));
    let dest_pub = dir.join(format!("{}.pub", key_name));
    if source_pub.exists() {
        let _ = std::fs::copy(&source_pub, &dest_pub);
    }

    let public_key = if dest_pub.exists() {
        std::fs::read_to_string(&dest_pub)
            .unwrap_or_default()
            .trim()
            .to_string()
    } else {
        String::new()
    };

    let key_type = if public_key.starts_with("ssh-ed25519") {
        "ed25519"
    } else if public_key.starts_with("ssh-rsa") {
        "rsa"
    } else if public_key.starts_with("ecdsa") {
        "ecdsa"
    } else {
        "unknown"
    }
    .to_string();

    Ok(SshKeyInfo {
        name: key_name,
        private_path: dest.display().to_string(),
        public_path: dest_pub.display().to_string(),
        public_key,
        key_type,
        created_at: Some(chrono::Utc::now().to_rfc3339()),
    })
}

/// Delete a key pair from the keys directory
pub fn delete_key_core(name: &str) -> Result<()> {
    validate_key_name(name)?;
    let dir = keys_dir()?;
    let private = dir.join(name);
    let public = dir.join(format!("{}.pub", name));

    if !private.exists() {
        return Err(anyhow!("key '{}' not found", name));
    }

    std::fs::remove_file(&private)?;
    if public.exists() {
        let _ = std::fs::remove_file(&public);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::CONFIG_DIR_ENV;
    use rsa::pkcs1::DecodeRsaPrivateKey;

    #[cfg(test)]
    fn entropy_source_name() -> &'static str {
        #[cfg(target_os = "linux")]
        {
            "linux-urandom"
        }
        #[cfg(target_os = "macos")]
        {
            "macos-arc4random"
        }
        #[cfg(target_os = "windows")]
        {
            "windows-BCryptGenRandom"
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        {
            "fallback-getrandom"
        }
    }

    /// Redirects the keys directory at a fresh temp dir for the duration of
    /// `body`, restoring whatever `CONFIG_DIR_ENV` held before.
    ///
    /// `generate_key_core*` resolves `~/.agent2ssh/keys` through that variable,
    /// so a test that writes keys must not run concurrently with another one —
    /// every caller is `#[serial]`.
    fn with_temp_config_dir<T>(body: impl FnOnce() -> T) -> T {
        let original = std::env::var(CONFIG_DIR_ENV).ok();
        let config_dir =
            std::env::temp_dir().join(format!("agent2ssh-keygen-{}", uuid::Uuid::new_v4()));
        std::env::set_var(CONFIG_DIR_ENV, &config_dir);

        let outcome = body();

        let _ = std::fs::remove_dir_all(&config_dir);
        match original {
            Some(value) => std::env::set_var(CONFIG_DIR_ENV, value),
            None => std::env::remove_var(CONFIG_DIR_ENV),
        }
        outcome
    }

    /// An entropy source that always fails, so the abort path is reachable
    /// without touching the machine's real entropy source.
    fn failing_entropy(_seed: &mut [u8]) -> Result<()> {
        Err(anyhow!("synthetic entropy failure"))
    }

    #[test]
    #[serial_test::serial]
    fn test_generate_key_core_writes_openssh_ed25519_pair() {
        with_temp_config_dir(|| {
            let result = generate_key_core_with(
                "id_ed25519_test",
                Some("agent2ssh-test"),
                KeyAlgorithm::Ed25519,
            )
            .unwrap();
            let private_raw = std::fs::read_to_string(&result.private_path).unwrap();
            let public_raw = std::fs::read_to_string(&result.public_path).unwrap();

            let parsed = crate::openssh_key::parse_private(&private_raw)
                .expect("the container this app writes must parse back");
            assert_eq!(parsed.comment, "agent2ssh-test");
            assert!(private_raw.starts_with("-----BEGIN OPENSSH PRIVATE KEY-----"));
            assert!(public_raw.starts_with("ssh-ed25519 "));
            assert_eq!(
                public_raw.trim(),
                crate::openssh_key::encode_public(&parsed.public, &parsed.comment)
            );
            assert_eq!(result.key_type, "ed25519");
        });
    }

    #[test]
    #[serial_test::serial]
    fn test_generate_key_core_with_rsa_writes_traditional_pem() {
        with_temp_config_dir(|| {
            let result =
                generate_key_core_with("id_rsa_test", Some("agent2ssh-test"), KeyAlgorithm::Rsa)
                    .unwrap();
            let private_raw = std::fs::read_to_string(&result.private_path).unwrap();
            let public_raw = std::fs::read_to_string(&result.public_path).unwrap();

            // PKCS#1 PEM, not the OpenSSH container: that is what `rsa` writes
            // natively, and the transport reads it like any other format.
            assert!(
                private_raw.starts_with("-----BEGIN RSA PRIVATE KEY-----"),
                "unexpected private key header: {}",
                private_raw.lines().next().unwrap_or_default()
            );
            assert!(!private_raw.contains("OPENSSH PRIVATE KEY"));

            assert_eq!(result.key_type, "rsa");
            assert_eq!(public_raw.trim(), result.public_key);
            assert!(public_raw.starts_with("ssh-rsa "));
            assert!(public_raw.ends_with("agent2ssh-test\n"), "{public_raw}");

            // Cross-check the hand-rolled public encoding against the private
            // key the `rsa` crate produced: both sides decode independently, so
            // an error in either the exponent or the modulus shows up here
            // rather than at authentication time.
            let private = rsa::RsaPrivateKey::from_pkcs1_pem(&private_raw)
                .expect("the PEM we wrote must decode as PKCS#1");
            let parsed = crate::openssh_key::parse_public_key_line(public_raw.trim()).unwrap();
            assert_eq!(parsed.algorithm, "ssh-rsa");
            assert_eq!(parsed.comment, "agent2ssh-test");
            let (exponent, modulus) =
                crate::openssh_key::rsa_exponent_and_modulus(&parsed.blob).unwrap();
            let strip_leading_zeros = |bytes: &[u8]| {
                bytes
                    .iter()
                    .skip_while(|b| **b == 0)
                    .copied()
                    .collect::<Vec<u8>>()
            };
            assert_eq!(
                strip_leading_zeros(&exponent),
                strip_leading_zeros(&private.e().to_bytes_be())
            );
            assert_eq!(
                strip_leading_zeros(&modulus),
                strip_leading_zeros(&private.n().to_bytes_be())
            );
        });
    }

    /// The invariant that used to tie generation to the build's crypto backend:
    /// whatever the app generates by default must be readable by the transport
    /// that will authenticate with it. It is now checked against that transport's
    /// own parser instead of a capability probe, which is a stronger question —
    /// the previous version asked "does this build claim to understand the
    /// format", this one asks "can the code that logs in actually open the file".
    #[test]
    #[serial_test::serial]
    fn the_default_algorithm_produces_a_key_the_transport_can_read() {
        with_temp_config_dir(|| {
            let result = generate_key_core("id_default", None).unwrap();
            russh::keys::load_secret_key(&result.private_path, None)
                .expect("the transport must be able to read the key we generated");
            assert_eq!(result.key_type, KeyAlgorithm::default_for_build().as_str());
        });
    }

    #[test]
    fn rsa_generation_refuses_keys_below_the_minimum_size() {
        let error = match generate_rsa_pair(1024, "agent2ssh-test") {
            Ok(_) => panic!("a 1024-bit RSA key must be refused"),
            Err(error) => error.to_string(),
        };
        assert!(error.contains("1024"), "{error}");
        assert!(error.contains(&RSA_BITS_MIN.to_string()), "{error}");
    }

    #[test]
    fn entropy_failure_aborts_instead_of_returning_unusable_bytes() {
        let mut rng = EntropyRng::with_source(failing_entropy);

        // `RngCore::fill_bytes` cannot report an error, so the adapter aborts.
        // Returning zeroed bytes instead would be worse than a failure: a fixed
        // candidate makes the prime search spin forever, and a derivable one
        // would be a broken key.
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            rng.fill_bytes(&mut [0u8; 8])
        }));
        assert!(outcome.is_err(), "a failed read must not return bytes");
        assert_eq!(
            rng.take_failure().as_deref(),
            Some("synthetic entropy failure")
        );
        assert!(rng.take_failure().is_none(), "the reason is taken once");
    }

    #[test]
    fn algorithm_names_round_trip_through_the_ipc_form() {
        for algorithm in [KeyAlgorithm::Ed25519, KeyAlgorithm::Rsa] {
            assert_eq!(KeyAlgorithm::from_name(algorithm.as_str()), Some(algorithm));
        }
        assert_eq!(KeyAlgorithm::from_name("  RSA "), Some(KeyAlgorithm::Rsa));
        assert_eq!(KeyAlgorithm::from_name("dsa"), None);
    }

    #[test]
    fn test_entropy_source_is_expected_for_platform() {
        #[cfg(target_os = "linux")]
        assert_eq!(entropy_source_name(), "linux-urandom");
        #[cfg(target_os = "macos")]
        assert_eq!(entropy_source_name(), "macos-arc4random");
        #[cfg(target_os = "windows")]
        assert_eq!(entropy_source_name(), "windows-BCryptGenRandom");
        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        assert_eq!(entropy_source_name(), "fallback-getrandom");
    }

    #[test]
    fn test_fill_entropy_populates_nonzero_bytes() {
        let mut seed = [0u8; 32];
        fill_entropy(&mut seed).unwrap();
        assert!(seed.iter().any(|byte| *byte != 0));
    }

    #[cfg(unix)]
    #[test]
    fn test_restrict_private_key_permissions_sets_0600() {
        use std::os::unix::fs::PermissionsExt;

        let path =
            std::env::temp_dir().join(format!("agent2ssh-key-perms-{}", uuid::Uuid::new_v4()));
        std::fs::write(&path, "private-key").unwrap();

        restrict_private_key_permissions(&path).unwrap();

        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        let _ = std::fs::remove_file(&path);
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn test_validate_key_name_rejects_path_traversal() {
        assert!(validate_key_name("../id_ed25519").is_err());
        assert!(validate_key_name("nested/id_ed25519").is_err());
        assert!(validate_key_name("nested\\id_ed25519").is_err());
        assert!(validate_key_name("id_ed25519").is_ok());
    }
}
