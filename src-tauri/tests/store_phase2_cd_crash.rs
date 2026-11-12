//! CNB #26 C5 / C7: real SIGKILL injection into the phase-C/D windows.
//!
//! The two earlier crash cases (C1–C3, #28) covered the commit log. Here the
//! external DYLD-interpose hook freezes the real CLI just before the paired
//! index rename (C5) and just before `secrets.enc` is replaced during an
//! explicit paired restore (C7). macOS only.

#![cfg(target_os = "macos")]

use std::{
    fs,
    io::Write,
    os::unix::process::ExitStatusExt,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

use agent2ssh::{config_pair, secrets};

const MASTER: &str = "cdcrash-master-only";
const SECRET_A: &str = "cdcrash-secret-a-marker";
const SECRET_B: &str = "cdcrash-secret-b-marker";
const HOST: &str = "cdcrash-host";
const ACCOUNT: &str = "host:cdcrash-host";
const HOOK_SRC: &str = include_str!("fixtures/crash_hook.c");

fn cli(dir: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_agent2ssh"));
    command
        .env("AGENT2SSH_CONFIG_DIR", dir)
        .env("AGENT2SSH_SECRETS_BACKEND", "encrypted")
        .env_remove("AGENT2SSH_MASTER_PASSWORD")
        .env_remove("DYLD_INSERT_LIBRARIES")
        .env_remove("A2S_HOOK_STAGE")
        .env_remove("A2S_HOOK_DIR")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

fn run(dir: &Path, args: &[&str], stdin: Option<&[u8]>, master: bool) -> Output {
    let mut command = cli(dir);
    command.args(args);
    if master {
        command.env("AGENT2SSH_MASTER_PASSWORD", MASTER);
    }
    if stdin.is_some() {
        command.stdin(Stdio::piped());
    }
    let mut child = command.spawn().unwrap();
    if let Some(bytes) = stdin {
        child.stdin.take().unwrap().write_all(bytes).unwrap();
    }
    child.wait_with_output().unwrap()
}

fn seed(dir: &Path) {
    let init = run(
        dir,
        &["secrets", "set-password"],
        Some(format!("{MASTER}\n").as_bytes()),
        false,
    );
    assert!(
        init.status.success(),
        "{}",
        String::from_utf8_lossy(&init.stderr)
    );
    let add = run(
        dir,
        &[
            "host",
            "add",
            HOST,
            "--host",
            "127.0.0.1",
            "--password-stdin",
        ],
        Some(SECRET_A.as_bytes()),
        true,
    );
    assert!(
        add.status.success(),
        "{}",
        String::from_utf8_lossy(&add.stderr)
    );
}

fn build_hook(root: &Path) -> PathBuf {
    let src = root.join("crash_hook.c");
    let dylib = root.join("liba2s_crashhook.dylib");
    fs::write(&src, HOOK_SRC).unwrap();
    let status = Command::new("clang")
        .arg("-dynamiclib")
        .arg("-o")
        .arg(&dylib)
        .arg(&src)
        .status()
        .expect("clang is required for the macOS crash-injection fixture");
    assert!(status.success(), "failed to compile the injection hook");
    dylib
}

/// Freeze the real CLI at `stage` and SIGKILL it from this (external) process.
fn crash_at(
    dir: &Path,
    dylib: &Path,
    stage: &str,
    hook_dir: &Path,
    args: &[&str],
    stdin: Option<&[u8]>,
    master: bool,
) -> Output {
    fs::create_dir_all(hook_dir).unwrap();
    let mut command = cli(dir);
    command.args(args);
    if master {
        command.env("AGENT2SSH_MASTER_PASSWORD", MASTER);
    }
    if stdin.is_some() {
        command.stdin(Stdio::piped());
    }
    command
        .env("DYLD_INSERT_LIBRARIES", dylib)
        .env("A2S_HOOK_STAGE", stage)
        .env("A2S_HOOK_DIR", hook_dir);
    let mut child = command.spawn().unwrap();
    if let Some(bytes) = stdin {
        child.stdin.take().unwrap().write_all(bytes).unwrap();
    }
    let fired = hook_dir.join(format!("{stage}.fired"));
    let deadline = Instant::now() + Duration::from_secs(20);
    while !fired.exists() {
        assert!(
            Instant::now() < deadline,
            "hook stage {stage} never fired; DYLD_INSERT_LIBRARIES may have been ignored"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    child.kill().unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(
        output.status.signal(),
        Some(9),
        "{stage} must die from SIGKILL, got {:?}",
        output.status
    );
    output
}

/// Owns the process-level config dir for library calls in serial tests.
struct ScopedConfig;
impl ScopedConfig {
    fn new(dir: &Path) -> Self {
        std::env::set_var("AGENT2SSH_CONFIG_DIR", dir);
        std::env::set_var("AGENT2SSH_SECRETS_BACKEND", "encrypted");
        std::env::remove_var("AGENT2SSH_MASTER_PASSWORD");
        secrets::lock();
        ScopedConfig
    }
}
impl Drop for ScopedConfig {
    fn drop(&mut self) {
        secrets::lock();
        std::env::remove_var("AGENT2SSH_CONFIG_DIR");
        std::env::remove_var("AGENT2SSH_SECRETS_BACKEND");
    }
}

#[test]
#[serial_test::serial]
fn c5_sigkill_before_pair_index_leaves_writes_recoverable() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("config");
    let _scope = ScopedConfig::new(&dir);
    let dylib = build_hook(root.path());
    seed(&dir);

    let index_before = config_pair::read_index().pairs.len();
    assert!(index_before >= 1, "seed must record a paired snapshot");

    // A second two-file commit freezes just before the index rename.
    crash_at(
        &dir,
        &dylib,
        "before-pair-index",
        &root.path().join("hook-c5"),
        &[
            "host",
            "add",
            "cdcrash-second",
            "--host",
            "127.0.0.2",
            "--password-stdin",
        ],
        Some(SECRET_B.as_bytes()),
        true,
    );

    // Crash state: hosts.json already renamed, .commit.json still present,
    // index still describes the previous state.
    assert!(dir.join(".commit.json").exists());
    let hosts = fs::read_to_string(dir.join("hosts.json")).unwrap();
    assert!(hosts.contains("cdcrash-second"));
    assert_eq!(
        config_pair::read_index().pairs.len(),
        index_before,
        "index must not have advanced before the freeze"
    );
    assert_eq!(config_pair::index_status(), config_pair::IndexStatus::Stale);

    // Restart: recover() clears the log, ordinary writes resume and the tag
    // verdict stays authoritative despite the stale index.
    let restarted = run(
        &dir,
        &["host", "add", "cdcrash-after", "--host", "127.0.0.3"],
        None,
        true,
    );
    assert!(
        restarted.status.success(),
        "writes must resume after C5: {}",
        String::from_utf8_lossy(&restarted.stderr)
    );
    assert!(!dir.join(".commit.json").exists());
    let hosts_bytes = fs::read(dir.join("hosts.json")).unwrap();
    assert!(String::from_utf8_lossy(&hosts_bytes).contains("cdcrash-after"));
    assert_eq!(
        config_pair::verify_hosts_restore(&hosts_bytes),
        config_pair::RestoreVerdict::Allowed
    );
    secrets::lock();
    secrets::unlock_or_init(MASTER).unwrap();
    assert_eq!(
        secrets::get_secret("host:cdcrash-second").as_deref(),
        Some(SECRET_B)
    );
}

#[test]
#[serial_test::serial]
fn c7_sigkill_during_paired_restore_stays_fail_closed_and_repairable() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("config");
    let _scope = ScopedConfig::new(&dir);
    let dylib = build_hook(root.path());
    seed(&dir);
    let pair = config_pair::read_index().pairs.last().unwrap().clone();

    // Change the ciphertext behind hosts.json without a config save.
    secrets::unlock_or_init(MASTER).unwrap();
    secrets::store_secret(ACCOUNT, SECRET_B).unwrap();
    let hosts_before = fs::read(dir.join("hosts.json")).unwrap();
    assert!(matches!(
        config_pair::verify_hosts_restore(&hosts_before),
        config_pair::RestoreVerdict::Refused(_)
    ));

    // Freeze the restore after hosts.json was replaced but before secrets.enc.
    crash_at(
        &dir,
        &dylib,
        "before-restore-secrets",
        &root.path().join("hook-c7"),
        &["config-repair", "--restore-pair", "0", "--confirm"],
        None,
        false,
    );

    let hosts_bytes = fs::read(dir.join("hosts.json")).unwrap();
    assert_eq!(
        agent2ssh::store_commit::sha256_bytes(&hosts_bytes),
        pair.hosts_sha256,
        "hosts.json should already be rolled back"
    );
    assert_ne!(
        agent2ssh::store_commit::sha256_bytes(&fs::read(dir.join(secrets::SECRETS_FILE)).unwrap()),
        pair.secrets_sha256,
        "secrets.enc must NOT have been replaced before the freeze"
    );
    // The half-finished restore must not be usable as a credential.
    assert!(
        matches!(
            config_pair::verify_hosts_restore(&hosts_bytes),
            config_pair::RestoreVerdict::Refused(_)
        ),
        "a half-rolled-back pair must stay fail-closed"
    );

    // Restart: re-running the same explicit restore completes the pair.
    let repaired = run(
        &dir,
        &["config-repair", "--restore-pair", "0", "--confirm"],
        None,
        false,
    );
    assert!(
        repaired.status.success(),
        "restore must be repeatable: {}",
        String::from_utf8_lossy(&repaired.stderr)
    );
    assert_eq!(
        config_pair::verify_hosts_restore(&fs::read(dir.join("hosts.json")).unwrap()),
        config_pair::RestoreVerdict::Allowed
    );
    secrets::lock();
    secrets::unlock_or_init(MASTER).unwrap();
    assert_eq!(secrets::get_secret(ACCOUNT).as_deref(), Some(SECRET_A));

    let audit = fs::read_to_string(dir.join("app.log")).unwrap_or_default();
    assert!(
        audit.contains("config repair restore-pair applied"),
        "missing audit entry: {audit}"
    );
}
