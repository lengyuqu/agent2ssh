//! CNB #26 phase D / #28: explicit, audited config repair.
//!
//! Real CLI invocations against isolated config dirs with synthetic markers.
//! No real credential and never `~/.agent2ssh`.

use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
};

use agent2ssh::{config_pair, secrets, store_commit::sha256_bytes};

const MASTER: &str = "repair-master-only";
const SECRET_A: &str = "repair-secret-a-marker";
const SECRET_B: &str = "repair-secret-b-marker";
const HOST: &str = "repair-host";
const ACCOUNT: &str = "host:repair-host";

fn cli(dir: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_agent2ssh"));
    command
        .env("AGENT2SSH_CONFIG_DIR", dir)
        .env("AGENT2SSH_SECRETS_BACKEND", "encrypted")
        .env_remove("AGENT2SSH_MASTER_PASSWORD")
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

fn files(dir: &Path) -> (Vec<u8>, Vec<u8>) {
    (
        fs::read(dir.join("hosts.json")).unwrap(),
        fs::read(dir.join(secrets::SECRETS_FILE)).unwrap(),
    )
}

/// Isolated config dir for library calls in `#[serial_test::serial]` tests.
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

fn write_unreplayable_commit(dir: &Path) {
    let secrets_bytes = fs::read(dir.join(secrets::SECRETS_FILE)).unwrap();
    let log = agent2ssh::store_commit::CommitLog {
        version: 1,
        hosts_sha256: "0".repeat(64),
        secrets_sha256: sha256_bytes(&secrets_bytes),
        secrets_generation: 1,
        tmp_name: "hosts.json.tmp.999999".to_string(),
    };
    fs::write(
        dir.join(".commit.json"),
        serde_json::to_string_pretty(&log).unwrap(),
    )
    .unwrap();
}

#[test]
fn list_and_previews_are_read_only_without_confirm() {
    let dir = tempfile::tempdir().unwrap();
    seed(dir.path());
    let before = files(dir.path());

    let list = run(dir.path(), &["config-repair", "--list"], None, false);
    assert!(
        list.status.success(),
        "{}",
        String::from_utf8_lossy(&list.stderr)
    );
    assert!(String::from_utf8_lossy(&list.stdout).contains("[0]"));

    // A pending log is needed for the discard preview to have an action to
    // decline; without one it correctly reports "No pending ..." instead.
    write_unreplayable_commit(dir.path());
    let discard = run(
        dir.path(),
        &["config-repair", "--discard-pending"],
        None,
        false,
    );
    assert!(discard.status.success());
    assert!(String::from_utf8_lossy(&discard.stdout).contains("Not applied"));

    let restore = run(
        dir.path(),
        &["config-repair", "--restore-pair", "0"],
        None,
        false,
    );
    assert!(restore.status.success());
    assert!(String::from_utf8_lossy(&restore.stdout).contains("Not applied"));
    assert_eq!(files(dir.path()), before, "preview changed the config");
}

#[test]
fn discard_pending_requires_confirm_then_resumes_writes() {
    let dir = tempfile::tempdir().unwrap();
    seed(dir.path());
    write_unreplayable_commit(dir.path());
    let before = files(dir.path());

    let blocked = run(
        dir.path(),
        &["host", "add", "blocked", "--host", "127.0.0.2"],
        None,
        true,
    );
    assert!(!blocked.status.success(), "recover() should refuse writes");
    assert!(dir.path().join(".commit.json").exists());

    let preview = run(
        dir.path(),
        &["config-repair", "--discard-pending"],
        None,
        false,
    );
    assert!(String::from_utf8_lossy(&preview.stdout).contains("Not applied"));
    assert!(dir.path().join(".commit.json").exists());

    let applied = run(
        dir.path(),
        &["config-repair", "--discard-pending", "--confirm"],
        None,
        false,
    );
    assert!(
        applied.status.success(),
        "{}",
        String::from_utf8_lossy(&applied.stderr)
    );
    assert!(!dir.path().join(".commit.json").exists());
    assert_eq!(
        files(dir.path()),
        before,
        "discard must not touch either file"
    );

    let resumed = run(
        dir.path(),
        &["host", "add", "resumed", "--host", "127.0.0.3"],
        None,
        true,
    );
    assert!(
        resumed.status.success(),
        "writes must resume: {}",
        String::from_utf8_lossy(&resumed.stderr)
    );

    let log = fs::read_to_string(dir.path().join("app.log")).unwrap_or_default();
    assert!(
        log.contains("config repair discard-pending applied"),
        "missing audit entry: {log}"
    );
}

#[test]
#[serial_test::serial]
fn restore_pair_recovers_a_mismatched_tag_state() {
    let dir = tempfile::tempdir().unwrap();
    let _scope = ScopedConfig::new(dir.path());
    seed(dir.path());
    let (hosts_before, _) = files(dir.path());

    // Change the ciphertext for the same account without a config save, so the
    // paired snapshot [0] still describes the state before the change.
    secrets::unlock_or_init(MASTER).unwrap();
    secrets::store_secret(ACCOUNT, SECRET_B).unwrap();
    // The guard compares the saved reference against the *current* store, so
    // the old hosts.json is now refused — exactly the state phase D repairs.
    assert!(matches!(
        config_pair::verify_hosts_restore(&hosts_before),
        config_pair::RestoreVerdict::Refused(_)
    ));

    let list = run(
        dir.path(),
        &["config-repair", "--list", "--json"],
        None,
        false,
    );
    assert!(list.status.success());
    let pairs: serde_json::Value =
        serde_json::from_slice(&list.stdout).expect("--list --json must be valid JSON");
    assert!(pairs.as_array().unwrap().len() >= 1);

    let applied = run(
        dir.path(),
        &["config-repair", "--restore-pair", "0", "--confirm"],
        None,
        false,
    );
    assert!(
        applied.status.success(),
        "restore failed: {}",
        String::from_utf8_lossy(&applied.stderr)
    );

    // Both files were rolled back together: the original secret is back.
    secrets::lock();
    secrets::unlock_or_init(MASTER).unwrap();
    assert_eq!(
        secrets::get_secret(ACCOUNT).as_deref(),
        Some(SECRET_A),
        "paired restore did not roll back secrets.enc"
    );
    assert_eq!(
        config_pair::verify_hosts_restore(&fs::read(dir.path().join("hosts.json")).unwrap()),
        config_pair::RestoreVerdict::Allowed
    );
    let log = fs::read_to_string(dir.path().join("app.log")).unwrap_or_default();
    assert!(
        log.contains("config repair restore-pair applied"),
        "missing audit entry: {log}"
    );
}

#[test]
fn restore_pair_refuses_when_paired_copies_are_corrupt() {
    let dir = tempfile::tempdir().unwrap();
    seed(dir.path());
    let before = files(dir.path());
    fs::write(
        dir.path().join(config_pair::SECRETS_PAIR_BACKUP),
        b"corrupt",
    )
    .unwrap();

    let result = run(
        dir.path(),
        &["config-repair", "--restore-pair", "0", "--confirm"],
        None,
        false,
    );
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("paired copies"));
    assert_eq!(files(dir.path()), before);
}

#[test]
fn restore_pair_refuses_while_a_pending_commit_exists() {
    let dir = tempfile::tempdir().unwrap();
    seed(dir.path());
    write_unreplayable_commit(dir.path());
    let before = files(dir.path());

    let result = run(
        dir.path(),
        &["config-repair", "--restore-pair", "0", "--confirm"],
        None,
        false,
    );
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("discard it first"));
    assert_eq!(files(dir.path()), before);
}

#[test]
fn actions_are_mutually_exclusive() {
    let dir = tempfile::tempdir().unwrap();
    seed(dir.path());
    let result = run(
        dir.path(),
        &["config-repair", "--list", "--discard-pending"],
        None,
        false,
    );
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("exactly one"));
}
