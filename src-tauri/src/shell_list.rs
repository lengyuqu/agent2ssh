//! Local shell enumeration — scans the system for installed shells.
//!
//! Mirrors rssh's `terminal/pty.rs:available_shells()` pattern, adapted
//! for agent2ssh's architecture (no portable-pty dependency, pure detection).
//!
//! On Unix, reads `/etc/shells` and scans PATH. On Windows, looks for
//! PowerShell, cmd, and Git Bash.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// An installed shell found on the system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellInfo {
    /// Display name (e.g. "bash", "PowerShell 7", "Command Prompt").
    pub name: String,
    /// Absolute path to the shell binary.
    pub path: String,
    /// Shell family for sentinel template selection.
    pub family: ShellFamily,
}

/// Broad shell categories for protocol/sentinel selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ShellFamily {
    /// POSIX-compatible (bash, zsh, fish, sh, dash).
    Posix,
    /// Windows cmd.exe.
    Cmd,
    /// PowerShell (5.1 or 7+).
    PowerShell,
}

/// Well-known Unix shell binary names to search for in PATH.
const UNIX_SHELLS: &[(&str, ShellFamily)] = &[
    ("bash", ShellFamily::Posix),
    ("zsh", ShellFamily::Posix),
    ("fish", ShellFamily::Posix),
    ("sh", ShellFamily::Posix),
    ("dash", ShellFamily::Posix),
    ("ksh", ShellFamily::Posix),
    ("tcsh", ShellFamily::Posix),
    ("csh", ShellFamily::Posix),
    ("pwsh", ShellFamily::PowerShell),
    ("pwsh-preview", ShellFamily::PowerShell),
];

/// Well-known Windows shell binary names to search for in PATH.
const WINDOWS_SHELLS: &[(&str, ShellFamily)] = &[
    ("pwsh.exe", ShellFamily::PowerShell),
    ("powershell.exe", ShellFamily::PowerShell),
    ("cmd.exe", ShellFamily::Cmd),
    ("bash.exe", ShellFamily::Posix), // Git Bash
    ("wsl.exe", ShellFamily::Posix),
];

/// Search for a binary in PATH, returning the first match.
fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
        // On Windows, try with .exe extension if not already present.
        #[cfg(windows)]
        {
            if !name.ends_with(".exe") {
                let candidate_exe = dir.join(format!("{name}.exe"));
                if candidate_exe.is_file() {
                    return Some(candidate_exe);
                }
            }
        }
    }
    None
}

/// Parse `/etc/shells` to get additional shell paths on Unix.
#[cfg(unix)]
fn parse_etc_shells() -> Vec<String> {
    let content = match std::fs::read_to_string("/etc/shells") {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    content
        .lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .map(|line| line.trim().to_string())
        .collect()
}

/// Enumerate all installed shells on the system.
///
/// Returns shells in priority order: well-known shells found in PATH first,
/// then any additional entries from `/etc/shells` (Unix only).
pub fn list_shells() -> Vec<ShellInfo> {
    let mut shells = Vec::new();
    let mut seen_paths: std::collections::HashSet<String> = std::collections::HashSet::new();

    let candidates: &[(&str, ShellFamily)] = if cfg!(windows) {
        WINDOWS_SHELLS
    } else {
        UNIX_SHELLS
    };

    // 1. Scan PATH for well-known shells.
    for (name, family) in candidates {
        if let Some(path) = which(name) {
            let path_str = path.to_string_lossy().into_owned();
            if seen_paths.insert(path_str.clone()) {
                shells.push(ShellInfo {
                    name: pretty_name(name, &path),
                    path: path_str,
                    family: *family,
                });
            }
        }
    }

    // 2. On Unix, also check /etc/shells for system-registered shells.
    #[cfg(unix)]
    {
        for shell_path in parse_etc_shells() {
            let path = PathBuf::from(&shell_path);
            if path.is_file() {
                let path_str = path.to_string_lossy().into_owned();
                if seen_paths.insert(path_str.clone()) {
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| shell_path.clone());
                    shells.push(ShellInfo {
                        name,
                        path: path_str,
                        family: ShellFamily::Posix, // /etc/shells entries are POSIX
                    });
                }
            }
        }
    }

    // 3. Fallback: if nothing found, at least report the default shell.
    if shells.is_empty() {
        #[cfg(unix)]
        {
            if let Ok(shell) = std::env::var("SHELL") {
                if !shell.is_empty() {
                    let path = PathBuf::from(&shell);
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| shell.clone());
                    shells.push(ShellInfo {
                        name,
                        path: shell,
                        family: ShellFamily::Posix,
                    });
                }
            }
        }
        #[cfg(windows)]
        {
            // cmd.exe should always exist on Windows.
            if let Ok(system_root) = std::env::var("SystemRoot") {
                let cmd = format!("{system_root}\\System32\\cmd.exe");
                shells.push(ShellInfo {
                    name: "Command Prompt".into(),
                    path: cmd,
                    family: ShellFamily::Cmd,
                });
            }
        }
    }

    shells
}

/// Generate a human-friendly display name for a shell.
fn pretty_name(binary_name: &str, path: &std::path::Path) -> String {
    let lower = binary_name.to_lowercase();
    match lower.as_str() {
        "pwsh" | "pwsh.exe" | "pwsh-preview" => "PowerShell 7".to_string(),
        "powershell.exe" => "Windows PowerShell".to_string(),
        "cmd.exe" => "Command Prompt".to_string(),
        "bash" | "bash.exe" => {
            // Check if this is Git Bash (path contains "Git")
            if path.to_string_lossy().to_lowercase().contains("git") {
                "Git Bash".to_string()
            } else {
                "Bash".to_string()
            }
        }
        "zsh" => "Zsh".to_string(),
        "fish" => "Fish".to_string(),
        "sh" => "POSIX Shell".to_string(),
        "dash" => "Dash".to_string(),
        "ksh" => "KornShell".to_string(),
        "tcsh" => "TC Shell".to_string(),
        "csh" => "C Shell".to_string(),
        "wsl.exe" => "WSL".to_string(),
        _ => binary_name.to_string(),
    }
}

/// Return the default shell for the current system.
///
/// On Unix, uses `$SHELL`. On Windows, prefers PowerShell 7, then
/// Windows PowerShell, then cmd.exe.
pub fn default_shell() -> Option<ShellInfo> {
    let shells = list_shells();
    if shells.is_empty() {
        return None;
    }

    #[cfg(unix)]
    {
        if let Ok(shell_var) = std::env::var("SHELL") {
            if let Some(found) = shells.iter().find(|s| s.path == shell_var) {
                return Some(found.clone());
            }
        }
    }

    // Prefer PowerShell on Windows, POSIX shells on Unix.
    #[cfg(windows)]
    {
        if let Some(pwsh) = shells.iter().find(|s| s.family == ShellFamily::PowerShell) {
            return Some(pwsh.clone());
        }
    }

    shells.first().cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_shells_returns_non_empty() {
        let shells = list_shells();
        assert!(!shells.is_empty(), "system should have at least one shell");
    }

    #[test]
    fn list_shells_no_duplicate_paths() {
        let shells = list_shells();
        let mut paths: Vec<&str> = shells.iter().map(|s| s.path.as_str()).collect();
        paths.sort();
        let before = paths.len();
        paths.dedup();
        assert_eq!(before, paths.len(), "no duplicate shell paths");
    }

    #[test]
    fn default_shell_returns_something() {
        let shell = default_shell();
        assert!(shell.is_some(), "default shell should be found");
    }

    #[test]
    fn pretty_name_recognizes_known_shells() {
        let path = PathBuf::from("/usr/bin/bash");
        assert_eq!(pretty_name("bash", &path), "Bash");

        let path = PathBuf::from("C:\\Program Files\\Git\\bin\\bash.exe");
        assert_eq!(pretty_name("bash.exe", &path), "Git Bash");

        let path = PathBuf::from("/usr/bin/zsh");
        assert_eq!(pretty_name("zsh", &path), "Zsh");
    }
}
