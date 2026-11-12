//! Real SIGKILL injection regressions for CNB issue #28.
//!
//! `tests/store_transaction.rs` builds a `.commit.json` and a leftover temp
//! file by hand. That proves the recovery branches, but not that a **real
//! process death** at those two points leaves the state the recovery code
//! expects. This suite drives the real CLI, freezes it at the exact libc call
//! with an external DYLD-interpose library (no production code is touched),
//! sends SIGKILL from outside the process, then restarts a fresh CLI and
//! asserts both files still agree and every secret reference resolves.
//!
//! macOS only: the injection mechanism is DYLD_INSERT_LIBRARIES + clang, and
//! the Linux hidepid/ptrace matrix belongs to a different evidence batch.
//! Every case uses a throwaway `AGENT2SSH_CONFIG_DIR` and synthetic markers.
#![cfg(target_os = "macos")]

use std::{
    fs,
    io::Write,
    os::unix::process::ExitStatusExt,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

use agent2ssh::store_commit::sha256_file;

const MASTER: &str = "txn-crash-master-only";
const SEED_MARKER: &str = "A2S_CRASH_SEED_MARKER_41A7";
const NEW_MARKER: &str = "A2S_CRASH_NEW_MARKER_6C29";
const HOOK_SRC: &str = include_str!("fixtures/crash_hook.c");

fn cli(config: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_agent2ssh"));
    command
        .env("AGENT2SSH_CONFIG_DIR", config)
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

fn run(config: &Path, args: &[&str], stdin: Option<&[u8]>, master: bool) -> Output {
    let mut command = cli(config);
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

fn sha(path: &Path) -> Option<String> {
    sha256_file(path).unwrap()
}

fn json(path: &Path) -> serde_json::Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn host_names(config: &Path) -> Vec<String> {
    json(&config.join("hosts.json"))["hosts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["name"].as_str().unwrap().to_string())
        .collect()
}

fn no_marker(output: &Output, marker: &str) {
    assert!(!String::from_utf8_lossy(&output.stdout).contains(marker));
    assert!(!String::from_utf8_lossy(&output.stderr).contains(marker));
}

fn no_marker_on_disk(config: &Path, marker: &str) {
    for entry in fs::read_dir(config).unwrap() {
        let path = entry.unwrap().path();
        if path.is_file() {
            assert!(
                !fs::read(&path)
                    .unwrap()
                    .windows(marker.len())
                    .any(|w| w == marker.as_bytes()),
                "marker leaked into {}",
                path.display()
            );
        }
    }
}

fn copy_tree(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let path = entry.unwrap().path();
        if path.is_file() {
            fs::copy(&path, dst.join(path.file_name().unwrap())).unwrap();
        }
    }
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

fn seed(config: &Path) {
    fs::create_dir_all(config).unwrap();
    let set = run(
        config,
        &["secrets", "set-password"],
        Some(format!("{MASTER}\n").as_bytes()),
        false,
    );
    assert!(
        set.status.success(),
        "{}",
        String::from_utf8_lossy(&set.stderr)
    );
    let add = run(
        config,
        &[
            "host",
            "add",
            "seed",
            "--host",
            "127.0.0.1",
            "--password-stdin",
        ],
        Some(SEED_MARKER.as_bytes()),
        true,
    );
    assert!(
        add.status.success(),
        "{}",
        String::from_utf8_lossy(&add.stderr)
    );
    no_marker(&add, SEED_MARKER);
    no_marker_on_disk(config, SEED_MARKER);
    assert!(
        !config.join(".commit.json").exists(),
        "seed left a commit log"
    );
}

/// Freeze the real CLI at `stage` and SIGKILL it from this (external) process.
fn crash_at(root: &Path, config: &Path, dylib: &Path, stage: &str, case: &str) {
    let hook_dir = root.join(format!("hook-{case}"));
    fs::create_dir_all(&hook_dir).unwrap();
    let mut command = cli(config);
    command
        .args([
            "host",
            "add",
            "crash-target",
            "--host",
            "127.0.0.2",
            "--password-stdin",
        ])
        .env("AGENT2SSH_MASTER_PASSWORD", MASTER)
        .env("DYLD_INSERT_LIBRARIES", dylib)
        .env("A2S_HOOK_STAGE", stage)
        .env("A2S_HOOK_DIR", &hook_dir)
        .stdin(Stdio::piped());
    let mut child = command.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(NEW_MARKER.as_bytes())
        .unwrap();

    let fired = hook_dir.join(format!("{stage}.fired"));
    let deadline = Instant::now() + Duration::from_secs(20);
    while !fired.exists() {
        assert!(
            Instant::now() < deadline,
            "hook stage {stage} never fired for {case}; DYLD_INSERT_LIBRARIES may have been ignored"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    child.kill().unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(
        output.status.signal(),
        Some(9),
        "{case} must die from SIGKILL, got {:?}",
        output.status
    );
    no_marker(&output, NEW_MARKER);
    no_marker_on_disk(config, NEW_MARKER);
}

/// Decrypt the store and prove every reference resolves to the right value.
/// Mutates process env, so callers must be `#[serial_test::serial]`.
fn assert_references_resolve(config: &Path, pairs: &[(&str, &str)]) {
    std::env::set_var("AGENT2SSH_CONFIG_DIR", config);
    std::env::set_var("AGENT2SSH_SECRETS_BACKEND", "encrypted");
    agent2ssh::secrets::lock();
    agent2ssh::secrets::unlock_or_init(MASTER).unwrap();
    for (name, expected) in pairs {
        assert_eq!(
            agent2ssh::secrets::get_secret(&agent2ssh::secrets::host_account(name)).as_deref(),
            Some(*expected),
            "secret for {name} did not resolve"
        );
    }
    agent2ssh::secrets::lock();
    std::env::remove_var("AGENT2SSH_CONFIG_DIR");
}

fn assert_aligned(config: &Path) {
    let hosts = json(&config.join("hosts.json"));
    let secrets = json(&config.join("secrets.enc"));
    assert_eq!(
        hosts["secrets_generation"], secrets["generation"],
        "hosts.json and secrets.enc generations drifted"
    );
}

fn setup() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let dylib = build_hook(root.path());
    let seed_config = root.path().join("seed");
    seed(&seed_config);
    (root, dylib, seed_config)
}

fn restart(config: &Path, name: &str) -> Output {
    run(
        config,
        &["host", "add", name, "--host", "127.0.0.3"],
        None,
        true,
    )
}

#[test]
#[serial_test::serial]
fn sigkill_before_rename_replays_the_prepared_hosts_and_resolves_every_secret() {
    let (root, dylib, seed_config) = setup();
    let config = root.path().join("case-before-rename");
    copy_tree(&seed_config, &config);

    crash_at(
        root.path(),
        &config,
        &dylib,
        "before-rename",
        "before-rename",
    );

    // Crash state: log present, hosts still old, prepared temp file intact.
    let log = json(&config.join(".commit.json"));
    let tmp = config.join(log["tmp_name"].as_str().unwrap());
    assert!(
        tmp.exists(),
        "prepared hosts file vanished before the crash"
    );
    assert_eq!(sha(&tmp).as_deref(), log["hosts_sha256"].as_str());
    assert!(!host_names(&config).contains(&"crash-target".to_string()));
    assert_eq!(
        sha(&config.join("secrets.enc")).as_deref(),
        log["secrets_sha256"].as_str()
    );

    let output = restart(&config, "after-recovery");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    no_marker(&output, NEW_MARKER);

    let names = host_names(&config);
    assert!(
        names.contains(&"crash-target".to_string()),
        "replay lost the committed host"
    );
    assert!(
        names.contains(&"after-recovery".to_string()),
        "restart did not build on the replayed config"
    );
    assert!(!config.join(".commit.json").exists(), "log was not cleared");
    assert!(!tmp.exists(), "replayed temp file was not removed");
    assert_aligned(&config);
    assert_references_resolve(
        &config,
        &[("seed", SEED_MARKER), ("crash-target", NEW_MARKER)],
    );
    no_marker_on_disk(&config, NEW_MARKER);
}

#[test]
#[serial_test::serial]
fn sigkill_before_finish_clears_the_log_without_a_double_replay() {
    let (root, dylib, seed_config) = setup();
    let config = root.path().join("case-before-finish");
    copy_tree(&seed_config, &config);

    crash_at(
        root.path(),
        &config,
        &dylib,
        "before-finish",
        "before-finish",
    );

    // Crash state: the hosts rename already happened, the log is still there.
    let log = json(&config.join(".commit.json"));
    assert_eq!(
        sha(&config.join("hosts.json")).as_deref(),
        log["hosts_sha256"].as_str()
    );
    assert!(host_names(&config).contains(&"crash-target".to_string()));
    let tmp = config.join(log["tmp_name"].as_str().unwrap());
    assert!(
        !tmp.exists(),
        "prepared temp file should have been consumed by the rename"
    );

    let output = restart(&config, "after-recovery");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let names = host_names(&config);
    assert!(names.contains(&"crash-target".to_string()));
    assert!(names.contains(&"after-recovery".to_string()));
    assert!(
        !config.join(".commit.json").exists(),
        "case 1 of recover() must clear the log"
    );
    assert_aligned(&config);
    assert_references_resolve(
        &config,
        &[("seed", SEED_MARKER), ("crash-target", NEW_MARKER)],
    );
    no_marker_on_disk(&config, NEW_MARKER);
}

#[test]
#[serial_test::serial]
fn sigkill_with_a_lost_temp_file_refuses_writes_and_keeps_the_log() {
    let (root, dylib, seed_config) = setup();
    let config = root.path().join("case-lost-tmp");
    copy_tree(&seed_config, &config);

    crash_at(root.path(), &config, &dylib, "before-rename", "lost-tmp");
    let log = json(&config.join(".commit.json"));
    fs::remove_file(config.join(log["tmp_name"].as_str().unwrap())).unwrap();
    let before = fs::read(config.join("hosts.json")).unwrap();

    let output = restart(&config, "must-not-land");
    assert!(
        !output.status.success(),
        "an unreplayable log must refuse writes"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("cannot be replayed"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read(config.join("hosts.json")).unwrap(), before);
    assert!(
        config.join(".commit.json").exists(),
        "log must be kept for inspection"
    );
    assert!(!host_names(&config).contains(&"must-not-land".to_string()));
    no_marker_on_disk(&config, NEW_MARKER);
}
