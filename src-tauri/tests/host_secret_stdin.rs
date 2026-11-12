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

#[test]
fn legacy_password_fails_closed_when_locked() {
    let dir = tempfile::tempdir().unwrap();
    let marker = "legacy-locked-password-marker";
    let output = run(
        dir.path(),
        &[
            "host",
            "add",
            "locked",
            "--host",
            "127.0.0.1",
            "--password",
            marker,
        ],
        b"",
        false,
    );
    assert!(
        !output.status.success(),
        "locked vault must reject legacy password"
    );
    assert!(String::from_utf8_lossy(&output.stderr)
        .contains("refusing to persist an unencrypted password"));
    no_marker(&output, marker);
    for entry in std::fs::read_dir(dir.path()).unwrap() {
        let path = entry.unwrap().path();
        if path.is_file() {
            assert!(!std::fs::read(&path)
                .unwrap()
                .windows(marker.len())
                .any(|b| b == marker.as_bytes()));
        }
    }
    let initial = run(
        dir.path(),
        &[
            "host",
            "add",
            "locked",
            "--host",
            "original.example",
            "--password",
            "original-secret-marker",
        ],
        b"",
        true,
    );
    assert!(initial.status.success());
    let before_hosts = std::fs::read(dir.path().join("hosts.json")).unwrap();
    let before_secrets = std::fs::read(dir.path().join("secrets.enc")).unwrap();
    let overwrite = run(
        dir.path(),
        &[
            "host",
            "add",
            "locked",
            "--host",
            "replacement.example",
            "--password",
            marker,
        ],
        b"",
        false,
    );
    assert!(!overwrite.status.success());
    no_marker(&overwrite, marker);
    assert_eq!(
        std::fs::read(dir.path().join("hosts.json")).unwrap(),
        before_hosts
    );
    assert_eq!(
        std::fs::read(dir.path().join("secrets.enc")).unwrap(),
        before_secrets
    );
}

#[test]
fn legacy_plaintext_requires_unlock_before_a_save_and_then_migrates() {
    let dir = tempfile::tempdir().unwrap();
    let first = run(
        dir.path(),
        &["host", "add", "old", "--host", "old.example"],
        b"",
        false,
    );
    assert!(first.status.success());
    let path = dir.path().join("hosts.json");
    let mut config: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    config["hosts"][0]["password"] = "historical-password-marker".into();
    let original = serde_json::to_vec_pretty(&config).unwrap();
    std::fs::write(&path, &original).unwrap();
    let args = ["host", "add", "new", "--host", "new.example"];
    let locked = run(dir.path(), &args, b"", false);
    assert!(!locked.status.success());
    no_marker(&locked, "historical-password-marker");
    assert_eq!(std::fs::read(&path).unwrap(), original);
    let unlocked = run(dir.path(), &args, b"", true);
    assert!(
        unlocked.status.success(),
        "{}",
        String::from_utf8_lossy(&unlocked.stderr)
    );
    no_marker(&unlocked, "historical-password-marker");
    let saved = std::fs::read_to_string(path).unwrap();
    assert!(!saved.contains("historical-password-marker"));
    let saved: serde_json::Value = serde_json::from_str(&saved).unwrap();
    assert_eq!(
        saved["hosts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|h| h["name"] == "old")
            .unwrap()["password"],
        agent2ssh::secrets::SECRET_REF
    );
}

#[test]
#[serial_test::serial]
fn import_config_skips_existing_credentials_with_locked_or_unlocked_vault() {
    let dir = tempfile::tempdir().unwrap();
    let ssh = dir.path().join("ssh-config");
    std::fs::write(&ssh, "Host existing\n  HostName replacement.example\nHost new\n  HostName new.example\nHost new\n  HostName duplicate.example\n").unwrap();
    let seeded = run(
        dir.path(),
        &[
            "host",
            "add",
            "existing",
            "--host",
            "original.example",
            "--password",
            "import-password-marker",
            "--passphrase",
            "import-passphrase-marker",
        ],
        b"",
        true,
    );
    assert!(
        seeded.status.success(),
        "{}",
        String::from_utf8_lossy(&seeded.stderr)
    );
    let before_secrets = std::fs::read(dir.path().join("secrets.enc")).unwrap();
    for unlocked in [false, true] {
        let output = run(
            dir.path(),
            &[
                "host",
                "import-config",
                "--path",
                ssh.to_str().unwrap(),
                "--json",
            ],
            b"",
            unlocked,
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        no_marker(&output, "import-password-marker");
        no_marker(&output, "import-passphrase-marker");
        let parsed: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.path().join("hosts.json")).unwrap()).unwrap();
        let hosts = parsed["hosts"].as_array().unwrap();
        let existing = hosts.iter().find(|h| h["name"] == "existing").unwrap();
        assert_eq!(existing["host"], "original.example");
        assert_eq!(existing["password"], agent2ssh::secrets::SECRET_REF);
        assert_eq!(existing["passphrase"], agent2ssh::secrets::SECRET_REF);
        assert_eq!(hosts.iter().filter(|h| h["name"] == "new").count(), 1);
        assert_eq!(
            hosts.iter().find(|h| h["name"] == "new").unwrap()["host"],
            "new.example"
        );
        // Saving an unlocked configuration may reseal unchanged secrets, so
        // compare bytes only for the locked import.
        if !unlocked {
            assert_eq!(
                std::fs::read(dir.path().join("secrets.enc")).unwrap(),
                before_secrets
            );
        }
    }
    std::env::set_var("AGENT2SSH_CONFIG_DIR", dir.path());
    std::env::set_var("AGENT2SSH_SECRETS_BACKEND", "encrypted");
    agent2ssh::secrets::unlock_or_init("test-master-only").unwrap();
    assert_eq!(
        agent2ssh::secrets::get_secret(&agent2ssh::secrets::host_account("existing")).as_deref(),
        Some("import-password-marker")
    );
    assert_eq!(
        agent2ssh::secrets::get_secret(&agent2ssh::secrets::host_passphrase_account("existing"))
            .as_deref(),
        Some("import-passphrase-marker")
    );
    agent2ssh::secrets::lock();
}

#[test]
fn password_reference_does_not_bypass_passphrase_encryption() {
    let dir = tempfile::tempdir().unwrap();
    let args = [
        "host",
        "add",
        "reference-host",
        "--host",
        "127.0.0.9",
        "--password",
        agent2ssh::secrets::SECRET_REF,
        "--passphrase-stdin",
    ];
    let marker = "private-key-passphrase-test-marker";
    let locked = run(dir.path(), &args, marker.as_bytes(), false);
    assert!(
        !locked.status.success(),
        "locked store must refuse passphrase"
    );
    no_marker(&locked, marker);
    assert!(!dir.path().join("hosts.json").exists());

    let unlocked = run(dir.path(), &args, marker.as_bytes(), true);
    assert!(
        unlocked.status.success(),
        "{}",
        String::from_utf8_lossy(&unlocked.stderr)
    );
    no_marker(&unlocked, marker);
    let hosts = std::fs::read_to_string(dir.path().join("hosts.json")).unwrap();
    assert!(!hosts.contains(marker));
    let parsed: serde_json::Value = serde_json::from_str(&hosts).unwrap();
    assert_eq!(
        parsed["hosts"][0]["passphrase"],
        agent2ssh::secrets::SECRET_REF
    );
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
#[serial_test::serial]
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

/// Issue #27 — what a real `cmd.exe` and PowerShell put on stdin.
///
/// The tests above define the CLI's own contract by writing bytes into a pipe
/// the test controls. These define the *shell* contract: neither shell's
/// quoting, line endings, nor encoding is visible from inside the CLI, and all
/// three differ between `cmd.exe`, PowerShell 7 and Windows PowerShell 5.1.
#[cfg(windows)]
mod windows_shells {
    use super::*;
    use std::os::windows::process::CommandExt;
    use std::path::{Path, PathBuf};

    /// The CLI is reached through `PATH` rather than a quoted absolute path:
    /// `cmd /c "<quoted exe>" … < "<quoted file>"` re-quotes differently
    /// depending on whether the executable path needs quotes at all, and that
    /// difference belongs to cmd's parser, not to this contract.
    fn path_with_target_dir() -> std::ffi::OsString {
        let exe = Path::new(env!("CARGO_BIN_EXE_agent2ssh"));
        let mut path = std::ffi::OsString::from(exe.parent().unwrap());
        path.push(";");
        path.push(std::env::var_os("PATH").unwrap_or_default());
        path
    }

    fn shell_env(command: &mut Command, dir: &Path) {
        command
            .env("AGENT2SSH_CONFIG_DIR", dir)
            .env("AGENT2SSH_SECRETS_BACKEND", "encrypted")
            .env("AGENT2SSH_MASTER_PASSWORD", "test-master-only")
            .env("PATH", path_with_target_dir())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
    }

    /// `cmd /d /s /c <script>`: `/d` skips AutoRun, `/s` keeps the quoting
    /// rules a real `cmd /c "..."` uses.
    fn cmd(dir: &Path, script: &str) -> Output {
        let mut command = Command::new("cmd.exe");
        shell_env(&mut command, dir);
        command
            .raw_arg("/d")
            .raw_arg("/s")
            .raw_arg("/c")
            .raw_arg(script);
        command.output().unwrap()
    }

    fn powershell(shell: &Path, dir: &Path, script: &str) -> Output {
        let mut command = Command::new(shell);
        shell_env(&mut command, dir);
        command
            .args(["-NoLogo", "-NoProfile", "-Command", script])
            .output()
            .unwrap()
    }

    fn powershell51() -> PathBuf {
        Path::new(&std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into()))
            .join(r"System32\WindowsPowerShell\v1.0\powershell.exe")
    }

    fn pwsh_available() -> bool {
        Command::new("pwsh")
            .args(["-NoLogo", "-NoProfile", "-Command", "exit 0"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }

    /// Decrypt through the library: "the shell's bytes arrived intact" has to be
    /// measured, because the JSON output redacts the value by design. Callers
    /// hold `serial_test`'s lock; this only mutates the process environment.
    fn stored_password(dir: &Path, host: &str) -> String {
        std::env::set_var("AGENT2SSH_CONFIG_DIR", dir);
        std::env::set_var("AGENT2SSH_SECRETS_BACKEND", "encrypted");
        std::env::set_var("AGENT2SSH_MASTER_PASSWORD", "test-master-only");
        agent2ssh::secrets::unlock_or_init("test-master-only").unwrap();
        let value = agent2ssh::secrets::get_secret(&agent2ssh::secrets::host_account(host))
            .unwrap_or_else(|| panic!("no stored password for {host}"));
        agent2ssh::secrets::lock();
        value
    }

    #[test]
    #[serial_test::serial]
    fn cmd_echo_pipe_drops_the_crlf_it_adds() {
        let dir = tempfile::tempdir().unwrap();
        let marker = "windows-cmd-echo-marker";
        let output = cmd(
            dir.path(),
            &format!(
                "echo {marker}| agent2ssh.exe host add cmd-echo --host 127.0.0.1 --password-stdin --json"
            ),
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        no_marker(&output, marker);
        assert_eq!(stored_password(dir.path(), "cmd-echo"), marker);
    }

    #[test]
    #[serial_test::serial]
    fn cmd_redirect_trims_one_ending_and_keeps_the_interior() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("secret-crlf.txt");
        std::fs::write(&file, b"  windows-interior  \r\nsecond line  \r\n").unwrap();
        let output = cmd(
            dir.path(),
            &format!(
                "agent2ssh.exe host add cmd-redirect --host 127.0.0.1 --password-stdin --json < \"{}\"",
                file.display()
            ),
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            stored_password(dir.path(), "cmd-redirect"),
            "  windows-interior  \r\nsecond line  "
        );
    }

    #[test]
    fn cmd_pipes_reject_empty_and_invalid_utf8_input() {
        let dir = tempfile::tempdir().unwrap();
        let empty = cmd(
            dir.path(),
            "type NUL| agent2ssh.exe host add cmd-empty --host 127.0.0.1 --password-stdin",
        );
        assert!(!empty.status.success());
        assert!(String::from_utf8_lossy(&empty.stderr).contains("secret input is empty"));
        assert!(!dir.path().join("hosts.json").exists());

        // 0xC3 0x28 is invalid UTF-8 but not a UTF-16 BOM: a file starting with
        // FF FE makes `type` emit nothing at all, which would test cmd instead.
        // The marker rides along so the "nothing leaked" scan below has a string
        // that can only appear if the rejected bytes were written somewhere.
        let inputs = tempfile::tempdir().unwrap();
        let bad = inputs.path().join("bad.bin");
        std::fs::write(&bad, b"windows-invalid-marker\xc3\x28").unwrap();
        let invalid = cmd(
            dir.path(),
            &format!(
                "type \"{}\"| agent2ssh.exe host add cmd-bad --host 127.0.0.1 --password-stdin",
                bad.display()
            ),
        );
        assert!(!invalid.status.success());
        assert!(String::from_utf8_lossy(&invalid.stderr).contains("UTF-8"));
        assert!(!dir.path().join("hosts.json").exists());
        // The rejected bytes must not survive anywhere in the config directory
        // either — the input file lives outside it so this only sees output.
        for entry in std::fs::read_dir(dir.path()).unwrap() {
            let path = entry.unwrap().path();
            if path.is_file() {
                let bytes = std::fs::read(&path).unwrap();
                assert!(
                    !bytes
                        .windows(b"windows-invalid-marker".len())
                        .any(|window| window == b"windows-invalid-marker"),
                    "{} leaked the rejected marker",
                    path.display()
                );
            }
        }
    }

    #[test]
    #[serial_test::serial]
    fn powershell_pipes_arrive_without_a_bom() {
        let dir = tempfile::tempdir().unwrap();
        let marker = "windows-pwsh-pipe-marker";
        if pwsh_available() {
            let output = powershell(
                Path::new("pwsh"),
                dir.path(),
                &format!(
                    "\"{marker}\" | & agent2ssh.exe host add ps7 --host 127.0.0.1 --password-stdin --json"
                ),
            );
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(stored_password(dir.path(), "ps7"), marker);
        }

        // Windows PowerShell 5.1 precedes a native-command pipe with a UTF-8
        // BOM even when `$OutputEncoding` is set to a BOM-less encoder; the CLI
        // strips it so the stored password matches what the user typed.
        let marker51 = "windows-ps51-pipe-marker";
        let output = powershell(
            &powershell51(),
            dir.path(),
            &format!(
                "\"{marker51}\" | & agent2ssh.exe host add ps51 --host 127.0.0.1 --password-stdin --json"
            ),
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(stored_password(dir.path(), "ps51"), marker51);
    }

    #[test]
    fn powershell_refuses_angle_bracket_redirection() {
        let dir = tempfile::tempdir().unwrap();
        if pwsh_available() {
            let output = powershell(
                Path::new("pwsh"),
                dir.path(),
                "& agent2ssh.exe host add lt --host 127.0.0.1 --password-stdin < \"nope.txt\"",
            );
            assert!(!output.status.success());
            let stderr = String::from_utf8_lossy(&output.stderr).to_lowercase();
            assert!(
                stderr.contains("reserved") || stderr.contains("保留"),
                "unexpected error: {stderr}"
            );
            assert!(!dir.path().join("hosts.json").exists());
        }
    }

    /// D5 from #6: can a Windows test assert that the marker is absent from the
    /// process command line? `GetCommandLineW` reads the *calling* process, so it
    /// can only describe the test binary (where the marker never is, because it
    /// travels through a pipe). The child's command line needs a process-table
    /// read; `sysinfo` does that through the PEB and works for a same-user child
    /// that is blocked reading stdin.
    #[test]
    fn the_secret_never_reaches_the_child_command_line() {
        let dir = tempfile::tempdir().unwrap();
        let marker = "windows-argv-marker-4c";
        let mut child = Command::new(env!("CARGO_BIN_EXE_agent2ssh"))
            .args([
                "host",
                "add",
                "argv",
                "--host",
                "127.0.0.1",
                "--password-stdin",
                "--json",
            ])
            .env("AGENT2SSH_CONFIG_DIR", dir.path())
            .env("AGENT2SSH_SECRETS_BACKEND", "encrypted")
            .env("AGENT2SSH_MASTER_PASSWORD", "test-master-only")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();

        let line =
            child_command_line(child.id()).expect("sysinfo could not read the child cmdline");
        assert!(
            line.contains("--password-stdin"),
            "unexpected child command line: {line}"
        );
        assert!(!line.contains(marker), "the secret was in argv: {line}");
        assert!(
            !current_command_line().contains(marker),
            "GetCommandLineW reported the secret in the test process"
        );

        child
            .stdin
            .take()
            .unwrap()
            .write_all(marker.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// The raw command line of this process, straight from the Win32 API.
    fn current_command_line() -> String {
        use std::os::windows::ffi::OsStringExt;
        extern "system" {
            fn GetCommandLineW() -> *const u16;
        }
        unsafe {
            let pointer = GetCommandLineW();
            if pointer.is_null() {
                return String::new();
            }
            let mut len = 0usize;
            while *pointer.add(len) != 0 {
                len += 1;
            }
            std::ffi::OsString::from_wide(std::slice::from_raw_parts(pointer, len))
                .to_string_lossy()
                .into_owned()
        }
    }

    fn child_command_line(pid: u32) -> Option<String> {
        let mut system = sysinfo::System::new();
        let pid = sysinfo::Pid::from_u32(pid);
        for _ in 0..20 {
            // The default refresh kind does not collect the command line; the
            // daemon's process checks ask for it explicitly and so must this.
            system.refresh_processes_specifics(
                sysinfo::ProcessesToUpdate::Some(&[pid]),
                true,
                sysinfo::ProcessRefreshKind::nothing().with_cmd(sysinfo::UpdateKind::OnlyIfNotSet),
            );
            if let Some(process) = system.process(pid) {
                let parts: Vec<String> = process
                    .cmd()
                    .iter()
                    .map(|part| part.to_string_lossy().into_owned())
                    .collect();
                if !parts.is_empty() {
                    return Some(parts.join(" "));
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        None
    }
}
