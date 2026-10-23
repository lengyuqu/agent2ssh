//! Offline recovery of encrypted desktop WebDAV backups through the real CLI.

mod common;

use agent2ssh::store::CONFIG_DIR_ENV;
use std::path::Path;
use std::process::{Command, Output};
use std::sync::OnceLock;

const PASSWORD: &str = "desktop-backup-regression-password";
const CUSTOM_PASSWORD_ENV: &str = "AGENT2SSH_TEST_BACKUP_PASSWORD";
const PRIVATE_MARKER: &str = "offline-private-config-marker";

fn fixture() -> &'static (Vec<u8>, Vec<u8>) {
    static FIXTURE: OnceLock<(Vec<u8>, Vec<u8>)> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let content = format!("# {PRIVATE_MARKER}\nmode = \"deny\"\n");
        let plaintext = serde_json::to_vec(&serde_json::json!({
            "version": 1,
            "exportedAt": "2026-10-23T00:00:00Z",
            "files": [{
                "path": "policy.toml",
                "bytes": content.len(),
                "content": content,
            }],
        }))
        .unwrap();
        let encrypted = agent2ssh::encrypt_backup(PASSWORD.as_bytes(), &plaintext).unwrap();
        (plaintext, encrypted)
    })
}

struct BackupFixture {
    dir: tempfile::TempDir,
}

impl BackupFixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("desktop-backup.a2s"), &fixture().1).unwrap();
        Self { dir }
    }

    fn input(&self) -> std::path::PathBuf {
        self.dir.path().join("desktop-backup.a2s")
    }

    fn output(&self) -> std::path::PathBuf {
        self.dir.path().join("recovered.json")
    }

    fn command(&self, output: &Path) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_agent2ssh"));
        command
            .args(["webdav", "decrypt"])
            .arg(self.input())
            .arg(output)
            .env(CONFIG_DIR_ENV, self.dir.path().join("untouched-config"))
            .env_remove("AGENT2SSH_SYNC_PASSWORD")
            .env_remove(CUSTOM_PASSWORD_ENV)
            .env_remove("AGENT2SSH_MASTER_PASSWORD")
            .env_remove("COMPLETE")
            .env("AGENT2SSH_QUIET", "1");
        command
    }

    fn assert_private(&self, result: &Output) {
        for output in [&result.stdout, &result.stderr] {
            let text = String::from_utf8_lossy(output);
            assert!(!text.contains(PASSWORD), "password was printed");
            assert!(!text.contains(PRIVATE_MARKER), "backup content was printed");
        }
        assert!(
            !self.dir.path().join("untouched-config").exists(),
            "offline decryption must not initialize or modify local configuration"
        );
        assert_eq!(std::fs::read(self.input()).unwrap(), fixture().1);
    }
}

#[test]
fn desktop_backup_decrypt_exports_exact_json_with_default_password_env() {
    let backup = BackupFixture::new();
    let result = backup
        .command(&backup.output())
        .env("AGENT2SSH_SYNC_PASSWORD", PASSWORD)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "decryption failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(std::fs::read(backup.output()).unwrap(), fixture().0);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(backup.output())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    backup.assert_private(&result);
}

#[test]
fn desktop_backup_decrypt_accepts_an_explicit_password_env() {
    let backup = BackupFixture::new();
    let result = backup
        .command(&backup.output())
        .args(["--sync-password-env", CUSTOM_PASSWORD_ENV])
        .env(CUSTOM_PASSWORD_ENV, PASSWORD)
        .env("AGENT2SSH_SYNC_PASSWORD", "unused-wrong-default")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "decryption failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(std::fs::read(backup.output()).unwrap(), fixture().0);
    backup.assert_private(&result);
}

#[test]
fn desktop_backup_decrypt_wrong_password_creates_no_output() {
    let backup = BackupFixture::new();
    let result = backup
        .command(&backup.output())
        .env("AGENT2SSH_SYNC_PASSWORD", "incorrect-password")
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("wrong password or tampered"));
    assert!(!backup.output().exists());
    backup.assert_private(&result);
}

#[test]
fn desktop_backup_decrypt_never_overwrites_existing_output_or_input() {
    let backup = BackupFixture::new();
    let existing = b"keep this existing output";
    std::fs::write(backup.output(), existing).unwrap();
    for output in [backup.output(), backup.input()] {
        let result = backup
            .command(&output)
            .env("AGENT2SSH_SYNC_PASSWORD", PASSWORD)
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert!(
            String::from_utf8_lossy(&result.stderr).contains("failed to create new output file")
        );
        assert_eq!(std::fs::read(backup.output()).unwrap(), existing);
        backup.assert_private(&result);
    }
}

#[test]
fn desktop_backup_decrypt_requires_a_nonempty_password_env() {
    let backup = BackupFixture::new();
    for password in [None, Some("")] {
        let mut command = backup.command(&backup.output());
        if let Some(password) = password {
            command.env("AGENT2SSH_SYNC_PASSWORD", password);
        }
        let result = command.output().unwrap();
        assert!(!result.status.success());
        let stderr = String::from_utf8_lossy(&result.stderr);
        assert!(stderr.contains("AGENT2SSH_SYNC_PASSWORD"));
        assert!(stderr.contains("sync password"));
        assert!(!backup.output().exists());
        backup.assert_private(&result);
    }
}
