//! CNB #38: real SIGKILL injection into the `secrets retag` window.
//!
//! The retag command rewrites `secrets.enc` (fresh tags) and then `hosts.json`
//! (tagged references) under the phase-0/1 commit protocol. Here the external
//! DYLD-interpose hook freezes the real CLI at both renames and the driver
//! SIGKILLs it; a restarted retag must complete the pair idempotently. macOS
//! only; reuses `tests/fixtures/crash_hook.c` unchanged.

#![cfg(target_os = "macos")]

use std::{
    fs,
    io::Write,
    os::unix::process::ExitStatusExt,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

use agent2ssh::secrets::{parse_secret_ref, SecretRef, SECRET_REF};

const MASTER: &str = "retagcrash-master-only";
const SECRET_A: &str = "retagcrash-secret-a-marker";
const HOST: &str = "retagcrash-host";
const ACCOUNT: &str = "host:retagcrash-host";
const HOOK_SRC: &str = include_str!("fixtures/crash_hook.c");

fn cli(dir: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_agent2ssh"));
    command
        .env("AGENT2SSH_CONFIG_DIR", dir)
        .env("AGENT2SSH_SECRETS_BACKEND", "encrypted")
        // Locked has to mean locked: the OS credential store would otherwise
        // re-unlock this scratch store behind the test's back.
        .env("AGENT2SSH_AUTO_UNLOCK", "0")
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

fn hosts_json(dir: &Path) -> serde_json::Value {
    serde_json::from_slice(&fs::read(dir.join("hosts.json")).unwrap()).unwrap()
}

fn secrets_json(dir: &Path) -> serde_json::Value {
    serde_json::from_slice(&fs::read(dir.join("secrets.enc")).unwrap()).unwrap()
}

fn password_ref(dir: &Path) -> String {
    hosts_json(dir)["hosts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|host| host["name"].as_str() == Some(HOST))
        .and_then(|host| host["password"].as_str())
        .unwrap()
        .to_string()
}

fn make_legacy(dir: &Path) {
    let mut hosts = hosts_json(dir);
    for host in hosts["hosts"].as_array_mut().unwrap().iter_mut() {
        if host["name"].as_str() == Some(HOST) {
            host["password"] = serde_json::Value::String(SECRET_REF.to_string());
        }
    }
    fs::write(
        dir.join("hosts.json"),
        serde_json::to_string_pretty(&hosts).unwrap(),
    )
    .unwrap();
    let mut store = secrets_json(dir);
    for entry in store["entries"].as_object_mut().unwrap().values_mut() {
        entry["tag"] = serde_json::Value::String(String::new());
    }
    fs::write(
        dir.join("secrets.enc"),
        serde_json::to_string_pretty(&store).unwrap(),
    )
    .unwrap();
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
fn crash_at(dir: &Path, dylib: &Path, stage: &str, hook_dir: &Path, args: &[&str]) -> Output {
    fs::create_dir_all(hook_dir).unwrap();
    let mut command = cli(dir);
    command.args(args);
    command.env("AGENT2SSH_MASTER_PASSWORD", MASTER);
    command
        .env("DYLD_INSERT_LIBRARIES", dylib)
        .env("A2S_HOOK_STAGE", stage)
        .env("A2S_HOOK_DIR", hook_dir);
    let mut child = command.spawn().unwrap();
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

/// Assert the fully retagged end state: tagged reference matching the entry
/// tag, aligned generations, plaintext preserved.
fn assert_fully_retagged(dir: &Path) {
    let reference = password_ref(dir);
    let tag = match parse_secret_ref(&reference) {
        Some(SecretRef::Tagged(tag)) => tag.to_string(),
        other => panic!("expected a tagged reference, got {other:?} ({reference})"),
    };
    assert_eq!(
        secrets_json(dir)["entries"][ACCOUNT]["tag"].as_str(),
        Some(tag.as_str())
    );
    let hosts_generation = hosts_json(dir)["secrets_generation"].as_u64();
    let store_generation = secrets_json(dir)["generation"].as_u64();
    assert_eq!(hosts_generation, store_generation, "generations must align");
    assert!(!dir.join(".commit.json").exists());
}

/// Owns the process-level config dir for library calls in serial tests.
struct ScopedConfig;
impl ScopedConfig {
    fn new(dir: &Path) -> Self {
        std::env::set_var("AGENT2SSH_CONFIG_DIR", dir);
        std::env::set_var("AGENT2SSH_SECRETS_BACKEND", "encrypted");
        std::env::remove_var("AGENT2SSH_MASTER_PASSWORD");
        agent2ssh::secrets::lock();
        ScopedConfig
    }
}
impl Drop for ScopedConfig {
    fn drop(&mut self) {
        agent2ssh::secrets::lock();
        std::env::remove_var("AGENT2SSH_CONFIG_DIR");
        std::env::remove_var("AGENT2SSH_SECRETS_BACKEND");
    }
}

/// Crash after `secrets.enc` was retagged and `.commit.json` written, but
/// before `hosts.json` was renamed: the restarted command replays the
/// prepared temp file (or simply recompletes) and finishes the upgrade.
#[test]
#[serial_test::serial]
fn retag_sigkill_before_hosts_rename_replays_to_a_complete_pair() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("config");
    let _scope = ScopedConfig::new(&dir);
    let dylib = build_hook(root.path());
    seed(&dir);
    make_legacy(&dir);

    crash_at(
        &dir,
        &dylib,
        "before-rename",
        &root.path().join("hook-retag-hosts"),
        &["secrets", "retag", "--confirm"],
    );

    // Crash state: secrets already carry fresh tags, hosts.json still bare,
    // the redo log is still present.
    assert!(dir.join(".commit.json").exists());
    assert_eq!(password_ref(&dir), SECRET_REF);

    // Restart: the same explicit command completes the upgrade.
    let restarted = run(&dir, &["secrets", "retag", "--confirm"], None, true);
    assert!(
        restarted.status.success(),
        "retag must resume after the crash: {}",
        String::from_utf8_lossy(&restarted.stderr)
    );
    assert_fully_retagged(&dir);

    // And a further run stays a no-op.
    let hosts_bytes = fs::read(dir.join("hosts.json")).unwrap();
    let secrets_bytes = fs::read(dir.join("secrets.enc")).unwrap();
    let again = run(&dir, &["secrets", "retag", "--confirm"], None, true);
    assert!(again.status.success());
    assert_eq!(fs::read(dir.join("hosts.json")).unwrap(), hosts_bytes);
    assert_eq!(fs::read(dir.join("secrets.enc")).unwrap(), secrets_bytes);

    agent2ssh::secrets::lock();
    agent2ssh::secrets::unlock_or_init(MASTER).unwrap();
    assert_eq!(
        agent2ssh::secrets::get_secret(ACCOUNT).as_deref(),
        Some(SECRET_A)
    );
}

#[test]
#[serial_test::serial]
fn retag_replay_preserves_an_unresolved_bare_reference() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("config");
    let _scope = ScopedConfig::new(&dir);
    let dylib = build_hook(root.path());
    seed(&dir);
    make_legacy(&dir);
    let mut hosts = hosts_json(&dir);
    let mut orphan = hosts["hosts"][0].clone();
    orphan["name"] = serde_json::json!("orphan-host");
    orphan["host"] = serde_json::json!("127.0.0.2");
    orphan["password"] = serde_json::json!(SECRET_REF);
    hosts["hosts"].as_array_mut().unwrap().push(orphan);
    fs::write(
        dir.join("hosts.json"),
        serde_json::to_vec_pretty(&hosts).unwrap(),
    )
    .unwrap();
    crash_at(
        &dir,
        &dylib,
        "before-rename",
        &root.path().join("hook-orphan"),
        &["secrets", "retag", "--confirm"],
    );
    assert!(dir.join(".commit.json").exists());
    let restarted = run(&dir, &["secrets", "retag", "--confirm"], None, true);
    assert!(
        restarted.status.success(),
        "{}",
        String::from_utf8_lossy(&restarted.stderr)
    );
    assert_fully_retagged(&dir);
    let hosts = hosts_json(&dir);
    let orphan = hosts["hosts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|host| host["name"] == "orphan-host")
        .unwrap();
    assert_eq!(orphan["password"].as_str(), Some(SECRET_REF));
}

/// Crash before `secrets.enc` itself is replaced (the entry rewrite never
/// lands): nothing changed on disk, and a rerun retags from scratch.
#[test]
#[serial_test::serial]
fn retag_sigkill_before_secrets_rewrite_leaves_the_pair_untouched() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("config");
    let _scope = ScopedConfig::new(&dir);
    let dylib = build_hook(root.path());
    seed(&dir);
    make_legacy(&dir);
    let hosts_before = fs::read(dir.join("hosts.json")).unwrap();
    let secrets_before = fs::read(dir.join("secrets.enc")).unwrap();

    crash_at(
        &dir,
        &dylib,
        "before-restore-secrets",
        &root.path().join("hook-retag-secrets"),
        &["secrets", "retag", "--confirm"],
    );

    // Frozen before the entry rewrite: both files are byte-identical and no
    // commit log was started.
    assert_eq!(fs::read(dir.join("hosts.json")).unwrap(), hosts_before);
    assert_eq!(fs::read(dir.join("secrets.enc")).unwrap(), secrets_before);
    assert!(!dir.join(".commit.json").exists());

    let restarted = run(&dir, &["secrets", "retag", "--confirm"], None, true);
    assert!(
        restarted.status.success(),
        "retag must work after the early crash: {}",
        String::from_utf8_lossy(&restarted.stderr)
    );
    assert_fully_retagged(&dir);
}
