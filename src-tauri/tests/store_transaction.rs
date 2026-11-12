//! Cross-file commit regressions for CNB issue #6 (phase 0/1).
//!
//! Every case drives the real CLI against its own `AGENT2SSH_CONFIG_DIR` and a
//! throwaway master password. No real credential is used, no test touches
//! `~/.agent2ssh`, and every marker is a synthetic string so a leak is
//! detectable in stdout, stderr and every file left in the config directory.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use agent2ssh::store_commit::{sha256_bytes, sha256_file, CommitLog};

const MASTER: &str = "txn-master-only";
const COMMIT_FILE: &str = ".commit.json";
const HOSTS_FILE: &str = "hosts.json";
const SECRETS_FILE: &str = "secrets.enc";

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

fn run(dir: &Path, args: &[&str], master: bool) -> Output {
    let mut command = cli(dir);
    command.args(args);
    if master {
        command.env("AGENT2SSH_MASTER_PASSWORD", MASTER);
    }
    command.output().unwrap()
}

fn run_with_stdin(dir: &Path, args: &[&str], input: &[u8], master: bool) -> Output {
    let mut command = cli(dir);
    command.args(args).stdin(Stdio::piped());
    if master {
        command.env("AGENT2SSH_MASTER_PASSWORD", MASTER);
    }
    let mut child = command.spawn().unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

fn stderr_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn no_marker(output: &Output, marker: &str) {
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains(marker),
        "marker leaked into stdout"
    );
    assert!(
        !String::from_utf8_lossy(&output.stderr).contains(marker),
        "marker leaked into stderr"
    );
}

/// Grep every regular file in the isolated config dir for a synthetic marker.
/// Covers hosts.json, its .bak, secrets.enc, the commit log and any audit/log
/// file a failed path might have written.
fn no_marker_on_disk(dir: &Path, marker: &str) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if !path.is_file() {
            continue;
        }
        if let Ok(bytes) = std::fs::read(&path) {
            assert!(
                !bytes
                    .windows(marker.len())
                    .any(|window| window == marker.as_bytes()),
                "marker leaked into {}",
                path.display()
            );
        }
    }
}

/// Establish a valid hosts/secrets pair with one sealed credential.
fn seed(dir: &Path) {
    let output = run_with_stdin(
        dir,
        &[
            "host",
            "add",
            "seed",
            "--host",
            "127.0.0.1",
            "--password-stdin",
        ],
        b"txn-seed-secret-marker",
        true,
    );
    assert!(
        output.status.success(),
        "seed failed: {}",
        stderr_of(&output)
    );
    assert!(dir.join(HOSTS_FILE).exists());
    assert!(dir.join(SECRETS_FILE).exists());
    no_marker(&output, "txn-seed-secret-marker");
    no_marker_on_disk(dir, "txn-seed-secret-marker");
}

fn write_log(dir: &Path, log: &CommitLog) {
    std::fs::write(
        dir.join(COMMIT_FILE),
        serde_json::to_string_pretty(log).unwrap(),
    )
    .unwrap();
}

/// A target hosts body that differs from the seeded one without changing the
/// shape of `AppConfig` (so a replay produces a parseable config).
fn renamed_hosts(dir: &Path) -> String {
    std::fs::read_to_string(dir.join(HOSTS_FILE))
        .unwrap()
        .replace("\"seed\"", "\"seed-after-crash\"")
}

// ── Phase 0: concurrent writers ───────────────────────────────────────────

#[test]
#[serial_test::serial]
fn concurrent_writers_keep_every_sealed_secret() {
    let dir = tempfile::tempdir().unwrap();
    let names: Vec<String> = (0..8).map(|i| format!("txn-host-{i}")).collect();
    let markers: Vec<String> = (0..8)
        .map(|i| format!("txn-concurrent-marker-{i}"))
        .collect();

    let mut children = Vec::new();
    for name in &names {
        let mut command = cli(dir.path());
        command
            .args([
                "host",
                "add",
                name.as_str(),
                "--host",
                "127.0.0.1",
                "--password-stdin",
            ])
            .env("AGENT2SSH_MASTER_PASSWORD", MASTER)
            .stdin(Stdio::piped());
        children.push(command.spawn().unwrap());
    }
    for (child, marker) in children.iter_mut().zip(markers.iter()) {
        child
            .stdin
            .take()
            .unwrap()
            .write_all(marker.as_bytes())
            .unwrap();
    }
    for child in children {
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "concurrent host add failed: {}",
            stderr_of(&output)
        );
        for marker in &markers {
            no_marker(&output, marker);
        }
    }
    for marker in &markers {
        no_marker_on_disk(dir.path(), marker);
    }

    // The point of `.secrets.lock`: no writer's ciphertext was overwritten by
    // another process's read-modify-write of the same map.
    std::env::set_var("AGENT2SSH_CONFIG_DIR", dir.path());
    std::env::set_var("AGENT2SSH_SECRETS_BACKEND", "encrypted");
    agent2ssh::secrets::unlock_or_init(MASTER).unwrap();
    for (name, marker) in names.iter().zip(markers.iter()) {
        assert_eq!(
            agent2ssh::secrets::get_secret(&agent2ssh::secrets::host_account(name)).as_deref(),
            Some(marker.as_str()),
            "secret for {name} was lost"
        );
    }
    agent2ssh::secrets::lock();

    // hosts.json and secrets.enc must agree on the content generation.
    let hosts: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.path().join(HOSTS_FILE)).unwrap()).unwrap();
    let secrets: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.path().join(SECRETS_FILE)).unwrap()).unwrap();
    assert_eq!(
        hosts["secrets_generation"], secrets["generation"],
        "hosts.json and secrets.enc generations drifted"
    );
    assert!(
        hosts["secrets_generation"].as_u64().unwrap_or(0) >= 8,
        "each of the 8 sealed credentials should have advanced the generation"
    );
}

// ── Phase 1: redo log ─────────────────────────────────────────────────────

#[test]
fn pending_commit_is_replayed_before_the_next_write() {
    let dir = tempfile::tempdir().unwrap();
    seed(dir.path());

    let target = renamed_hosts(dir.path());
    assert_ne!(
        target,
        std::fs::read_to_string(dir.path().join(HOSTS_FILE)).unwrap()
    );
    let tmp_name = "hosts.json.tmp.999999";
    std::fs::write(dir.path().join(tmp_name), target.as_bytes()).unwrap();
    write_log(
        dir.path(),
        &CommitLog {
            version: 1,
            hosts_sha256: sha256_bytes(target.as_bytes()),
            secrets_sha256: sha256_file(&dir.path().join(SECRETS_FILE))
                .unwrap()
                .unwrap(),
            secrets_generation: 1,
            tmp_name: tmp_name.to_string(),
        },
    );

    let output = run(
        dir.path(),
        &["host", "add", "after-recovery", "--host", "127.0.0.2"],
        true,
    );
    assert!(output.status.success(), "{}", stderr_of(&output));
    let hosts = std::fs::read_to_string(dir.path().join(HOSTS_FILE)).unwrap();
    assert!(
        hosts.contains("seed-after-crash"),
        "the replayed config must be the base for the next save"
    );
    assert!(hosts.contains("after-recovery"));
    assert!(!dir.path().join(COMMIT_FILE).exists());
    assert!(!dir.path().join(tmp_name).exists());
}

#[test]
fn unreplayable_commit_refuses_writes_and_keeps_the_previous_config() {
    let dir = tempfile::tempdir().unwrap();
    seed(dir.path());

    let target = renamed_hosts(dir.path());
    let tmp_name = "hosts.json.tmp.999998";
    // Simulate a crash that left the log but lost the prepared temp file.
    write_log(
        dir.path(),
        &CommitLog {
            version: 1,
            hosts_sha256: sha256_bytes(target.as_bytes()),
            secrets_sha256: sha256_file(&dir.path().join(SECRETS_FILE))
                .unwrap()
                .unwrap(),
            secrets_generation: 1,
            tmp_name: tmp_name.to_string(),
        },
    );
    let before = std::fs::read(dir.path().join(HOSTS_FILE)).unwrap();

    let output = run(
        dir.path(),
        &["host", "add", "must-not-land", "--host", "127.0.0.3"],
        true,
    );
    assert!(!output.status.success());
    assert!(
        stderr_of(&output).contains("cannot be replayed"),
        "unexpected error: {}",
        stderr_of(&output)
    );
    assert_eq!(std::fs::read(dir.path().join(HOSTS_FILE)).unwrap(), before);
    assert!(
        dir.path().join(COMMIT_FILE).exists(),
        "an unreplayable log must be kept for inspection"
    );
}

#[test]
fn a_completed_commit_only_clears_its_log() {
    let dir = tempfile::tempdir().unwrap();
    seed(dir.path());

    let target = renamed_hosts(dir.path());
    std::fs::write(dir.path().join(HOSTS_FILE), target.as_bytes()).unwrap();
    write_log(
        dir.path(),
        &CommitLog {
            version: 1,
            hosts_sha256: sha256_bytes(target.as_bytes()),
            secrets_sha256: sha256_file(&dir.path().join(SECRETS_FILE))
                .unwrap()
                .unwrap(),
            secrets_generation: 1,
            tmp_name: "hosts.json.tmp.999997".to_string(),
        },
    );

    let output = run(
        dir.path(),
        &["host", "add", "after-cleanup", "--host", "127.0.0.4"],
        true,
    );
    assert!(output.status.success(), "{}", stderr_of(&output));
    assert!(!dir.path().join(COMMIT_FILE).exists());
    assert!(std::fs::read_to_string(dir.path().join(HOSTS_FILE))
        .unwrap()
        .contains("after-cleanup"));
}

// ── P2 and the generation guard ───────────────────────────────────────────

#[test]
fn proxy_password_is_refused_when_the_store_is_locked() {
    let dir = tempfile::tempdir().unwrap();
    seed(dir.path());

    let hosts_path = dir.path().join(HOSTS_FILE);
    let mut config: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&hosts_path).unwrap()).unwrap();
    let proxy = agent2ssh::types::ProxyProfile {
        id: "txn-proxy".into(),
        name: "txn-proxy".into(),
        protocol: agent2ssh::types::ProxyProtocol::Http,
        host: "127.0.0.4".into(),
        port: 8080,
        username: None,
        password: Some("txn-proxy-plaintext-marker".into()),
    };
    config["proxies"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::to_value(proxy).unwrap());
    let bytes = serde_json::to_vec_pretty(&config).unwrap();
    std::fs::write(&hosts_path, &bytes).unwrap();

    // No master password: the store is locked. A hosts-only save still has to
    // seal the existing plaintext proxy password, and must refuse instead of
    // warning and writing it in the clear.
    let output = run(
        dir.path(),
        &["host", "add", "unrelated", "--host", "127.0.0.5"],
        false,
    );
    assert!(!output.status.success());
    assert!(
        stderr_of(&output).contains("refusing to persist an unencrypted password for proxy"),
        "unexpected error: {}",
        stderr_of(&output)
    );
    no_marker(&output, "txn-proxy-plaintext-marker");
    assert_eq!(
        std::fs::read(&hosts_path).unwrap(),
        bytes,
        "a refused save must not rewrite hosts.json"
    );
}

#[test]
fn save_is_refused_when_the_credential_store_is_behind() {
    let dir = tempfile::tempdir().unwrap();
    seed(dir.path());

    let hosts_path = dir.path().join(HOSTS_FILE);
    let mut config: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&hosts_path).unwrap()).unwrap();
    config["secrets_generation"] = serde_json::json!(9999);
    let bytes = serde_json::to_vec_pretty(&config).unwrap();
    std::fs::write(&hosts_path, &bytes).unwrap();

    let output = run(
        dir.path(),
        &["host", "add", "must-not-land", "--host", "127.0.0.6"],
        true,
    );
    assert!(!output.status.success());
    assert!(
        stderr_of(&output).contains("credential store is behind hosts.json"),
        "unexpected error: {}",
        stderr_of(&output)
    );
    assert_eq!(std::fs::read(&hosts_path).unwrap(), bytes);
}

// ── set-password input handling ───────────────────────────────────────────

#[cfg(unix)]
#[test]
fn interactive_initial_password_requires_matching_confirmation_without_echo() {
    use std::io::{Read, Write};
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::time::{Duration, Instant};

    fn read_until(pty: &mut std::fs::File, needle: &str, output: &mut String) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while !output.contains(needle) {
            assert!(Instant::now() < deadline, "missing prompt: {needle}");
            let mut pollfd = libc::pollfd {
                fd: pty.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            if unsafe { libc::poll(&mut pollfd, 1, 100) } <= 0 {
                continue;
            }
            let mut buf = [0; 1024];
            match pty.read(&mut buf) {
                Ok(0) | Err(_) => panic!("terminal closed before {needle}"),
                Ok(n) => output.push_str(&String::from_utf8_lossy(&buf[..n])),
            }
        }
    }

    for (matches, cancelled) in [(false, false), (true, false), (false, true)] {
        let dir = tempfile::tempdir().unwrap();
        let (mut master, mut slave) = (-1, -1);
        assert_eq!(
            unsafe {
                libc::openpty(
                    &mut master,
                    &mut slave,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            },
            0
        );
        let mut master = unsafe { std::fs::File::from_raw_fd(master) };
        let slave = unsafe { std::fs::File::from_raw_fd(slave) };
        let mut child = Command::new(env!("CARGO_BIN_EXE_agent2ssh"))
            .args(["secrets", "set-password"])
            .env("AGENT2SSH_CONFIG_DIR", dir.path())
            .env("AGENT2SSH_SECRETS_BACKEND", "encrypted")
            .env_remove("AGENT2SSH_MASTER_PASSWORD")
            .stdin(Stdio::from(slave.try_clone().unwrap()))
            .stdout(Stdio::from(slave.try_clone().unwrap()))
            .stderr(Stdio::from(slave))
            .spawn()
            .unwrap();
        let mut output = String::new();
        read_until(&mut master, "New master password: ", &mut output);
        // Backspace removes an entire UTF-8 character, not just its last byte.
        master
            .write_all("pty-first-password-marker中\x7f\n".as_bytes())
            .unwrap();
        read_until(&mut master, "Confirm master password: ", &mut output);
        master
            .write_all(if cancelled {
                b"\x03".as_slice()
            } else if matches {
                b"pty-first-password-marker\n"
            } else {
                b"pty-other-password-marker\n"
            })
            .unwrap();
        let expected = if cancelled {
            "master password input cancelled"
        } else if matches {
            "Master password set."
        } else {
            "master password confirmation does not match"
        };
        read_until(&mut master, expected, &mut output);
        assert_eq!(child.wait().unwrap().success(), matches);
        assert!(!output.contains("pty-first-password-marker"));
        assert!(!output.contains("pty-other-password-marker"));
        assert_eq!(dir.path().join(SECRETS_FILE).exists(), matches);
    }
}

#[test]
fn set_password_separates_the_current_password_from_the_new_one() {
    let dir = tempfile::tempdir().unwrap();

    // First-time setup from redirected stdin: no terminal to echo to, so no
    // prompt on stderr and no password in stdout/stderr.
    let output = run_with_stdin(
        dir.path(),
        &["secrets", "set-password"],
        b"txn-first-master\n",
        false,
    );
    assert!(output.status.success(), "{}", stderr_of(&output));
    assert!(!stderr_of(&output).contains("New master password:"));
    no_marker(&output, "txn-first-master");
    assert!(dir.path().join(SECRETS_FILE).exists());

    // Without --password, the env value is the CURRENT password and must not
    // be silently reused as the new one.
    let output = Command::new(env!("CARGO_BIN_EXE_agent2ssh"))
        .args(["secrets", "set-password"])
        .env("AGENT2SSH_CONFIG_DIR", dir.path())
        .env("AGENT2SSH_SECRETS_BACKEND", "encrypted")
        .env("AGENT2SSH_MASTER_PASSWORD", "txn-first-master")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(stderr_of(&output).contains("requires --password <NEW>"));

    // The documented form: current password in the env, new one in --password.
    let output = Command::new(env!("CARGO_BIN_EXE_agent2ssh"))
        .args(["secrets", "set-password", "--password", "txn-second-master"])
        .env("AGENT2SSH_CONFIG_DIR", dir.path())
        .env("AGENT2SSH_SECRETS_BACKEND", "encrypted")
        .env("AGENT2SSH_MASTER_PASSWORD", "txn-first-master")
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr_of(&output));
    no_marker(&output, "txn-first-master");
    no_marker(&output, "txn-second-master");
}
