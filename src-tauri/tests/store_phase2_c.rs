//! CNB #26 phase C: paired snapshots and restore-time tag verification.
//!
//! Real CLI writes plus the four restore paths' shared guard. Every case uses
//! an isolated `AGENT2SSH_CONFIG_DIR` and synthetic markers only.

use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
};

use agent2ssh::{
    config_pair::{self, IndexStatus, RestoreVerdict},
    secrets,
    store_commit::sha256_bytes,
    webdav_sync,
};

const MASTER: &str = "phase2c-master-only";
const SECRET_A: &str = "phase2c-secret-a-marker";
const SECRET_B: &str = "phase2c-secret-b-marker";
const HOST: &str = "phase2c-host";
const ACCOUNT: &str = "host:phase2c-host";

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

fn hosts_bytes(dir: &Path) -> Vec<u8> {
    fs::read(dir.join("hosts.json")).unwrap()
}

/// Owns the process-level config dir for one test. Every config_pair/secrets
/// call reads `AGENT2SSH_CONFIG_DIR`, so no test may touch the real ~/.agent2ssh.
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

fn with_config<F: FnOnce()>(dir: &Path, f: F) {
    std::env::set_var("AGENT2SSH_CONFIG_DIR", dir);
    std::env::set_var("AGENT2SSH_SECRETS_BACKEND", "encrypted");
    secrets::lock();
    f();
    secrets::lock();
}

#[test]
#[serial_test::serial]
fn a_two_file_commit_records_a_consistent_paired_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let _scope = ScopedConfig::new(dir.path());
    seed(dir.path());

    assert!(dir.path().join(config_pair::PAIR_INDEX_FILE).exists());
    assert!(dir.path().join(config_pair::HOSTS_PAIR_BACKUP).exists());
    assert!(dir.path().join(config_pair::SECRETS_PAIR_BACKUP).exists());

    let index = config_pair::read_index();
    assert_eq!(index.schema_version, 1);
    let latest = index.pairs.last().expect("at least one pair record");
    assert_eq!(latest.hosts_sha256, sha256_bytes(&hosts_bytes(dir.path())));
    assert_eq!(
        latest.secrets_sha256,
        sha256_bytes(&fs::read(dir.path().join(secrets::SECRETS_FILE)).unwrap())
    );
    assert_eq!(latest.tag_digest, config_pair::tag_digest());
    assert_eq!(latest.hosts_backup, config_pair::HOSTS_PAIR_BACKUP);
    assert_eq!(latest.secrets_backup, config_pair::SECRETS_PAIR_BACKUP);

    with_config(dir.path(), || {
        assert_eq!(config_pair::index_status(), IndexStatus::Consistent);
    });
}

#[test]
#[serial_test::serial]
fn restoring_hosts_json_with_a_mismatched_tag_is_refused_before_any_write() {
    let dir = tempfile::tempdir().unwrap();
    let _scope = ScopedConfig::new(dir.path());
    seed(dir.path());
    let old_hosts = hosts_bytes(dir.path());

    // Change the ciphertext for the same account without touching hosts.json:
    // precisely the "restored older hosts.json" failure mode from #26.
    with_config(dir.path(), || {
        secrets::unlock_or_init(MASTER).unwrap();
        secrets::store_secret(ACCOUNT, SECRET_B).unwrap();
    });
    // The guard must leave the *changed* store untouched, not the original one.
    let secrets_before = fs::read(dir.path().join(secrets::SECRETS_FILE)).unwrap();

    let result = with_config_result(dir.path(), || {
        webdav_sync::apply_config_template(&[(
            "hosts.json".to_string(),
            String::from_utf8(old_hosts.clone()).unwrap(),
        )])
    });
    let error = result.expect_err("a restore with a stale tag must be refused");
    let message = format!("{error:#}");
    assert!(message.contains("expected tag"), "{message}");
    assert!(message.contains(ACCOUNT), "{message}");
    assert!(
        message.contains("restore hosts.json and secrets.enc as a pair"),
        "missing actionable guidance: {message}"
    );

    assert_eq!(
        hosts_bytes(dir.path()),
        old_hosts,
        "hosts.json was modified"
    );
    assert_eq!(
        fs::read(dir.path().join(secrets::SECRETS_FILE)).unwrap(),
        secrets_before,
        "secrets.enc was modified"
    );
}

fn with_config_result<T, F: FnOnce() -> T>(dir: &Path, f: F) -> T {
    std::env::set_var("AGENT2SSH_CONFIG_DIR", dir);
    std::env::set_var("AGENT2SSH_SECRETS_BACKEND", "encrypted");
    secrets::lock();
    let result = f();
    secrets::lock();
    result
}

#[test]
#[serial_test::serial]
fn restoring_hosts_json_with_matching_tags_is_allowed() {
    let dir = tempfile::tempdir().unwrap();
    let _scope = ScopedConfig::new(dir.path());
    seed(dir.path());
    let current = hosts_bytes(dir.path());

    let result = with_config_result(dir.path(), || {
        webdav_sync::apply_config_template(&[(
            "hosts.json".to_string(),
            String::from_utf8(current.clone()).unwrap(),
        )])
    });
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(hosts_bytes(dir.path()), current);
}

#[test]
#[serial_test::serial]
fn index_states_are_diagnosed_without_blocking_ordinary_writes() {
    let dir = tempfile::tempdir().unwrap();
    let _scope = ScopedConfig::new(dir.path());
    seed(dir.path());
    let index_path = dir.path().join(config_pair::PAIR_INDEX_FILE);

    fs::remove_file(&index_path).unwrap();
    with_config(dir.path(), || {
        assert_eq!(config_pair::index_status(), IndexStatus::Missing);
    });
    let write = run(
        dir.path(),
        &["host", "add", "phase2c-extra", "--host", "127.0.0.2"],
        None,
        true,
    );
    assert!(
        write.status.success(),
        "a missing index must not block writes: {}",
        String::from_utf8_lossy(&write.stderr)
    );

    // Keep a valid copy before corrupting: read_index() on a corrupt file
    // returns an empty default, which cannot be used to build the C6 state.
    let mut index = config_pair::read_index();
    fs::write(&index_path, b"not json").unwrap();
    with_config(dir.path(), || {
        assert_eq!(config_pair::index_status(), IndexStatus::Unreadable);
    });

    // C6: hashes still match but the recorded tag digest does not.
    index.pairs.last_mut().unwrap().tag_digest = "stale".to_string();
    fs::write(&index_path, serde_json::to_string_pretty(&index).unwrap()).unwrap();
    with_config(dir.path(), || {
        assert_eq!(config_pair::index_status(), IndexStatus::Stale);
    });
}

#[test]
#[serial_test::serial]
fn legacy_and_non_json_hosts_content_is_allowed_with_a_warning() {
    let dir = tempfile::tempdir().unwrap();
    let _scope = ScopedConfig::new(dir.path());
    assert!(matches!(
        config_pair::verify_hosts_restore(b"not json"),
        RestoreVerdict::LegacyOnly(_)
    ));
    assert!(matches!(
        config_pair::verify_hosts_restore(
            br#"{"hosts":[{"name":"old","password":"$agent2ssh-secret$"}]}"#
        ),
        RestoreVerdict::LegacyOnly(_)
    ));
    assert_eq!(
        config_pair::verify_hosts_restore(br#"{"hosts":[],"proxies":[]}"#),
        RestoreVerdict::Allowed
    );
    assert!(matches!(
        config_pair::verify_hosts_restore(
            br#"{"hosts":[{"name":"old","password":"$agent2ssh-secret$:bad"}]}"#
        ),
        RestoreVerdict::LegacyOnly(_)
    ));
    assert!(config_pair::guard_restore(b"not json", "test restore").is_ok());
}

#[test]
#[serial_test::serial]
fn c4_to_c6_states_keep_tag_verification_authoritative() {
    let dir = tempfile::tempdir().unwrap();
    let _scope = ScopedConfig::new(dir.path());
    seed(dir.path());
    let current = hosts_bytes(dir.path());
    let mut index = config_pair::read_index();

    // C4/C5: paired files exist but the index is missing (crash before the
    // index rename). Verification is based on tags, not on the index.
    fs::remove_file(dir.path().join(config_pair::PAIR_INDEX_FILE)).unwrap();
    with_config(dir.path(), || {
        assert_eq!(config_pair::index_status(), IndexStatus::Missing);
        assert_eq!(
            config_pair::verify_hosts_restore(&current),
            RestoreVerdict::Allowed
        );
    });

    // C6: a stale index must not turn into a write blocker either.
    index.pairs.last_mut().unwrap().tag_digest = "stale".to_string();
    fs::write(
        dir.path().join(config_pair::PAIR_INDEX_FILE),
        serde_json::to_string_pretty(&index).unwrap(),
    )
    .unwrap();
    with_config(dir.path(), || {
        assert_eq!(config_pair::index_status(), IndexStatus::Stale);
        assert_eq!(
            config_pair::verify_hosts_restore(&current),
            RestoreVerdict::Allowed
        );
    });

    let write = run(
        dir.path(),
        &["host", "add", "phase2c-after-crash", "--host", "127.0.0.3"],
        None,
        true,
    );
    assert!(
        write.status.success(),
        "{}",
        String::from_utf8_lossy(&write.stderr)
    );
}
