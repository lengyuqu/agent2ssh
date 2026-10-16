//! Shared integration-test setup: keep every test off the developer's real
//! `~/.agent2ssh`.
//!
//! The integration tests call core functions in-process (approvals, hosts,
//! webhooks, ...) and spawn the CLI/MCP binaries, all of which resolve their
//! state from `AGENT2SSH_CONFIG_DIR` and otherwise fall back to the real config
//! directory. Running them unisolated has rewritten real `approvals.jsonl`,
//! `hosts.json` and friends. This constructor runs before `main`, while the
//! process is still single-threaded, so setting the environment is safe and
//! every child process inherits it.
//!
//! A test that wants its own directory still sets `AGENT2SSH_CONFIG_DIR`
//! itself and calls `restore_default_config_env` afterwards.

#![allow(dead_code)]

use agent2ssh::store::CONFIG_DIR_ENV;
use std::path::PathBuf;
use std::sync::OnceLock;

/// The isolated directory chosen at process start (or the caller's own, if
/// `AGENT2SSH_CONFIG_DIR` was already set to something other than the real dir).
static DEFAULT_CONFIG_DIR: OnceLock<std::ffi::OsString> = OnceLock::new();

const SECRETS_BACKEND_ENV: &str = "AGENT2SSH_SECRETS_BACKEND";

#[ctor::ctor]
unsafe fn isolate_config_dir() {
    match std::env::var_os(CONFIG_DIR_ENV) {
        Some(dir) => assert!(
            !is_real_config_dir(&PathBuf::from(&dir)),
            "refusing to run tests against the real config dir {dir:?}; unset {CONFIG_DIR_ENV}"
        ),
        None => {
            let dir = std::env::temp_dir().join(format!(
                "agent2ssh-it-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_nanos())
            ));
            std::fs::create_dir_all(&dir).expect("create isolated test config dir");
            std::env::set_var(CONFIG_DIR_ENV, dir);
        }
    }
    if let Some(dir) = std::env::var_os(CONFIG_DIR_ENV) {
        let _ = DEFAULT_CONFIG_DIR.set(dir);
    }
    if std::env::var_os(SECRETS_BACKEND_ENV).is_none() {
        std::env::set_var(SECRETS_BACKEND_ENV, "memory");
    }
}

fn is_real_config_dir(dir: &std::path::Path) -> bool {
    let Some(home) = dirs_home() else {
        return false;
    };
    let real = home.join(".agent2ssh");
    dir == real || dir.canonicalize().ok() == real.canonicalize().ok() && real.exists()
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// Undo a test's own `set_var(AGENT2SSH_CONFIG_DIR, ..)` by restoring the
/// isolated default. Removing the variable instead would send every later test
/// in the same binary to the real config directory.
pub fn restore_default_config_env() {
    let dir = DEFAULT_CONFIG_DIR
        .get()
        .expect("isolate_config_dir constructor did not run");
    std::env::set_var(CONFIG_DIR_ENV, dir);
}
