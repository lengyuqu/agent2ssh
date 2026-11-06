//! Real CLI coverage for redirected host credentials, isolated from user data.
use std::io::Write;
use std::process::{Command, Output, Stdio};

fn run(dir: &std::path::Path, args: &[&str], input: &[u8], master: bool) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_agent2ssh"));
    command
        .args(args)
        .env("AGENT2SSH_CONFIG_DIR", dir)
        .env("AGENT2SSH_SECRETS_BACKEND", "encrypted")
        .env_remove("AGENT2SSH_MASTER_PASSWORD")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if master {
        command.env("AGENT2SSH_MASTER_PASSWORD", "test-master-only");
    }
    let mut child = command.spawn().unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

fn no_marker(output: &Output, marker: &str) {
    assert!(!String::from_utf8_lossy(&output.stdout).contains(marker));
    assert!(!String::from_utf8_lossy(&output.stderr).contains(marker));
}

fn run_with_blocked_config_temp(dir: &std::path::Path, name: &str, regular_file: bool) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_agent2ssh"))
        .args([
            "host",
            "add",
            name,
            "--host",
            "127.0.0.7",
            "--password-stdin",
        ])
        .env("AGENT2SSH_CONFIG_DIR", dir)
        .env("AGENT2SSH_SECRETS_BACKEND", "encrypted")
        .env("AGENT2SSH_MASTER_PASSWORD", "test-master-only")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // The CLI blocks reading stdin while we make its per-process temp path
    // unopenable, deterministically exercising the config-open failure.
    let blocked = dir.join(format!("hosts.json.tmp.{}", child.id()));
    if regular_file {
        std::fs::write(&blocked, b"preexisting-file-marker").unwrap();
    } else {
        std::fs::create_dir(&blocked).unwrap();
    }
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"failed-write-marker")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    if regular_file {
        assert_eq!(std::fs::read(&blocked).unwrap(), b"preexisting-file-marker");
        std::fs::remove_file(&blocked).unwrap();
    } else {
        std::fs::remove_dir(&blocked).unwrap();
    }
    output
}

#[test]
fn redirected_credentials_are_encrypted_and_fail_closed() {
    let dir = tempfile::tempdir().unwrap();
    let password = "  stdin-password-marker\nsecond line  ";
    let passphrase = "stdin-passphrase-marker  ";
    let output = run(
        dir.path(),
        &[
            "host",
            "add",
            "a",
            "--host",
            "127.0.0.1",
            "--password-stdin",
            "--json",
        ],
        format!("{password}\r\n").as_bytes(),
        true,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    no_marker(&output, "stdin-password-marker");
    let output = run(
        dir.path(),
        &[
            "host",
            "add",
            "b",
            "--host",
            "127.0.0.2",
            "--passphrase-stdin",
            "--json",
        ],
        format!("{passphrase}\n").as_bytes(),
        true,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    no_marker(&output, "stdin-passphrase-marker");
    for file in ["hosts.json", "hosts.json.bak", "secrets.enc"] {
        let bytes = std::fs::read(dir.path().join(file)).unwrap();
        assert!(!bytes
            .windows(b"stdin-password-marker".len())
            .any(|v| v == b"stdin-password-marker"));
        assert!(!bytes
            .windows(b"stdin-passphrase-marker".len())
            .any(|v| v == b"stdin-passphrase-marker"));
    }
    let before_hosts = std::fs::read(dir.path().join("hosts.json")).unwrap();
    let before_secrets = std::fs::read(dir.path().join("secrets.enc")).unwrap();
    for (name, regular_file) in [
        ("a", false),
        ("new-host", false),
        ("a", true),
        ("new-host", true),
    ] {
        let output = run_with_blocked_config_temp(dir.path(), name, regular_file);
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("failed to open temp config"));
        no_marker(&output, "failed-write-marker");
        assert_eq!(
            std::fs::read(dir.path().join("hosts.json")).unwrap(),
            before_hosts
        );
        assert_eq!(
            std::fs::read(dir.path().join("secrets.enc")).unwrap(),
            before_secrets
        );
    }
    std::env::set_var("AGENT2SSH_CONFIG_DIR", dir.path());
    std::env::set_var("AGENT2SSH_SECRETS_BACKEND", "encrypted");
    agent2ssh::secrets::unlock_or_init("test-master-only").unwrap();
    assert_eq!(
        agent2ssh::secrets::get_secret(&agent2ssh::secrets::host_account("a")).as_deref(),
        Some(password)
    );
    assert_eq!(
        agent2ssh::secrets::get_secret(&agent2ssh::secrets::host_passphrase_account("b"))
            .as_deref(),
        Some(passphrase)
    );
    agent2ssh::secrets::lock();

    let locked = tempfile::tempdir().unwrap();
    let output = run(
        locked.path(),
        &[
            "host",
            "add",
            "x",
            "--host",
            "127.0.0.1",
            "--password-stdin",
        ],
        b"stdin-locked-marker",
        false,
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr)
        .contains("refusing to persist an unencrypted password"));
    no_marker(&output, "stdin-locked-marker");
    assert!(!locked.path().join("hosts.json").exists());
    let output = run(
        locked.path(),
        &[
            "host",
            "add",
            "x",
            "--host",
            "127.0.0.1",
            "--passphrase-stdin",
        ],
        b"stdin-locked-passphrase-marker",
        false,
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr)
        .contains("refusing to persist an unencrypted key passphrase"));
    no_marker(&output, "stdin-locked-passphrase-marker");
    assert!(!locked.path().join("hosts.json").exists());
    let output = run(
        locked.path(),
        &[
            "host",
            "add",
            "x",
            "--host",
            "127.0.0.1",
            "--password-stdin",
        ],
        b"",
        false,
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("secret input is empty"));
    let output = run(
        locked.path(),
        &[
            "host",
            "add",
            "x",
            "--host",
            "127.0.0.1",
            "--password-stdin",
            "--passphrase-stdin",
        ],
        b"stdin-conflict-marker",
        false,
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot be used with"));
    no_marker(&output, "stdin-conflict-marker");
    let output = run(
        locked.path(),
        &[
            "host",
            "add",
            "x",
            "--host",
            "127.0.0.1",
            "--password",
            "argv-conflict-marker",
            "--password-stdin",
        ],
        b"stdin-conflict-marker",
        false,
    );
    assert!(!output.status.success());
    no_marker(&output, "argv-conflict-marker");
    no_marker(&output, "stdin-conflict-marker");
    let output = run(
        locked.path(),
        &[
            "host",
            "add",
            "x",
            "--host",
            "127.0.0.1",
            "--password-stdin",
        ],
        b"\xff",
        false,
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("UTF-8"));
    let output = run(
        locked.path(),
        &[
            "host",
            "add",
            "x",
            "--host",
            "127.0.0.1",
            "--password-stdin",
        ],
        &vec![b'x'; 65537],
        false,
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("exceeds 65536"));
    let broken = tempfile::tempdir().unwrap();
    std::fs::create_dir(broken.path().join("secrets.enc")).unwrap();
    let output = run(
        broken.path(),
        &[
            "host",
            "add",
            "x",
            "--host",
            "127.0.0.1",
            "--password-stdin",
        ],
        b"stdin-broken-marker",
        true,
    );
    assert!(!output.status.success());
    no_marker(&output, "stdin-broken-marker");
    assert!(!broken.path().join("hosts.json").exists());
}

#[cfg(unix)]
#[test]
fn terminal_stdin_is_refused_before_secret_entry() {
    use std::os::fd::FromRawFd;
    let dir = tempfile::tempdir().unwrap();
    let mut master = -1;
    let mut slave = -1;
    let rc = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    assert_eq!(rc, 0, "openpty failed: {}", std::io::Error::last_os_error());
    let _master = unsafe { std::fs::File::from_raw_fd(master) };
    let slave = unsafe { std::fs::File::from_raw_fd(slave) };
    let output = Command::new(env!("CARGO_BIN_EXE_agent2ssh"))
        .env("AGENT2SSH_CONFIG_DIR", dir.path())
        .args([
            "host",
            "add",
            "x",
            "--host",
            "127.0.0.1",
            "--password-stdin",
        ])
        .stdin(Stdio::from(slave))
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("requires redirected stdin"));
    assert!(!dir.path().join("hosts.json").exists());
}
