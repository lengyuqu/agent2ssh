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

/// Git install roots the beyond-PATH prober checks, ordered by likelihood.
///
/// Git for Windows can be installed per-user (under `LocalAppData\Programs`)
/// or into a non-default drive; the per-machine default locations come first.
#[cfg(windows)]
fn git_bash_candidate_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(pf) = std::env::var_os("ProgramFiles") {
        roots.push(PathBuf::from(pf).join("Git"));
    }
    if let Some(pf) = std::env::var_os("ProgramFiles(x86)") {
        roots.push(PathBuf::from(pf).join("Git"));
    }
    if let Some(lap) = std::env::var_os("LocalAppData") {
        roots.push(PathBuf::from(lap).join("Programs").join("Git"));
    }
    roots
}

/// Extract the `InstallPath` value from `reg query` output.
///
/// A matching line looks like `    InstallPath    REG_SZ    C:\Program Files\Git`.
/// The first hit wins; `None` when no line carries a non-empty value.
#[cfg(windows)]
fn parse_reg_install_path(text: &str) -> Option<String> {
    for line in text.lines() {
        let Some(idx) = line.find("InstallPath") else {
            continue;
        };
        let value = line[idx + "InstallPath".len()..].trim();
        let value = value.strip_prefix("REG_SZ").map(str::trim).unwrap_or(value);
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    None
}

/// Read Git for Windows' registered install root from the registry.
///
/// The installer records its (possibly custom) target under
/// `SOFTWARE\GitForWindows\InstallPath` in HKLM or HKCU; that covers installs a
/// fixed-path probe cannot see. `reg.exe` is a console program, so it goes
/// through `win_proc::no_window` like every other spawned console helper.
#[cfg(windows)]
fn git_bash_registry_root() -> Option<PathBuf> {
    use std::process::Command;
    for hive in ["HKEY_LOCAL_MACHINE", "HKEY_CURRENT_USER"] {
        let key = format!("{hive}\\SOFTWARE\\GitForWindows");
        let output = crate::win_proc::no_window(Command::new("reg.exe"))
            .args(["query", &key, "/v", "InstallPath"])
            .output();
        let text = match output {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
            _ => continue,
        };
        if let Some(path) = parse_reg_install_path(&text) {
            return Some(PathBuf::from(path));
        }
    }
    None
}

/// Locate Git Bash outside PATH: fixed install roots first, then the registry.
///
/// Only called when the PATH scan produced no Git-flavoured bash, so a machine
/// whose PATH already carries Git's `bin` directory pays nothing.
#[cfg(windows)]
fn find_git_bash_beyond_path() -> Option<PathBuf> {
    let mut roots = git_bash_candidate_roots();
    if let Some(root) = git_bash_registry_root() {
        roots.push(root);
    }
    for root in roots {
        let bash = root.join("bin").join("bash.exe");
        if bash.is_file() {
            return Some(bash);
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

    // 1.5. On Windows, a Git install is not necessarily on PATH — the
    // installer often adds only `Git\cmd` — while the one `bash.exe` PATH does
    // carry may be the WSL launcher stub. Probe Git's own install locations
    // (fixed roots + the registry's InstallPath) so `local:gitbash` resolves on
    // machines whose PATH never mentions Git's bin directory.
    #[cfg(windows)]
    if !shells
        .iter()
        .any(|s| s.path.to_ascii_lowercase().contains("git"))
    {
        if let Some(path) = find_git_bash_beyond_path() {
            let path_str = path.to_string_lossy().into_owned();
            if seen_paths.insert(path_str.clone()) {
                shells.push(ShellInfo {
                    name: "Git Bash".into(),
                    path: path_str,
                    family: ShellFamily::Posix,
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

    #[cfg(windows)]
    #[test]
    fn reg_query_install_path_parsing_handles_the_real_shape() {
        // reg.exe prints the value name, the type and the value on one line,
        // padded with spaces; paths contain spaces so splitting on whitespace
        // is not an option.
        let text = "\r\nHKEY_LOCAL_MACHINE\\SOFTWARE\\GitForWindows\r\n    InstallPath    REG_SZ    C:\\Program Files\\Git\r\n\r\n";
        assert_eq!(
            parse_reg_install_path(text).as_deref(),
            Some(r"C:\Program Files\Git")
        );

        assert_eq!(parse_reg_install_path("no match here"), None);
        assert_eq!(
            parse_reg_install_path("    InstallPath    REG_SZ"),
            None,
            "a valueless entry must not resolve"
        );
    }
}
