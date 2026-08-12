//! Desktop PATH resolution for external executables (A5).
//!
//! When resolving an executable name (e.g., "code", "vim", "git") to an
//! absolute path, the standard `which`-style approach only searches the
//! `PATH` environment variable. On desktop systems, this misses executables
//! installed in well-known locations that may not be on `PATH`:
//!
//! - **macOS**: `/opt/homebrew/bin` (Apple Silicon Homebrew),
//!   `/usr/local/bin` (Intel Homebrew), `/Applications/...`
//! - **Linux**: `/snap/bin` (Snap packages), `/usr/local/bin`
//! - **Windows**: Chocolatey (`C:\ProgramData\chocolatey\bin`),
//!   Scoop (`%USERPROFILE%\scoop\shims`), VS Code CLI
//!
//! `resolve_executable_in` combines the inherited `PATH` with these
//! platform-specific fallback directories, then checks each candidate
//! for existence and executability.
//!
//! Design borrowed from rssh's `resolve_executable_in` and
//! `discovery_cli_fallback_dirs` functions.

use std::path::{Path, PathBuf};

/// Resolve an executable name to an absolute path.
///
/// Searches the inherited `PATH` first, then falls back to platform-specific
/// directories. Returns `None` if the executable is not found.
///
/// # Arguments
/// * `name` — The executable name (without path). On Windows, `.exe` is
///   appended if the name doesn't already have an extension.
pub fn resolve_executable_in(name: &str) -> Option<PathBuf> {
    // On Windows, ensure the name has an extension.
    let name = if cfg!(windows) && Path::new(name).extension().is_none() {
        format!("{}.exe", name)
    } else {
        name.to_string()
    };

    // Collect all candidate directories: inherited PATH + platform fallbacks.
    let mut search_dirs: Vec<PathBuf> = Vec::new();

    // 1. Inherited PATH from the environment.
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            search_dirs.push(dir);
        }
    }

    // 2. Platform-specific fallback directories.
    for dir in platform_fallback_dirs() {
        if !search_dirs.contains(&dir) {
            search_dirs.push(dir);
        }
    }

    // 3. Check each candidate.
    for dir in &search_dirs {
        let candidate = dir.join(&name);
        if is_executable(&candidate) {
            return Some(candidate);
        }
    }

    None
}

/// Platform-specific directories that are not always on `PATH` but
/// commonly contain user-installed executables.
fn platform_fallback_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    #[cfg(unix)]
    {
        // macOS Homebrew (Apple Silicon)
        dirs.push(PathBuf::from("/opt/homebrew/bin"));
        // macOS Homebrew (Intel) / generic local
        dirs.push(PathBuf::from("/usr/local/bin"));
        // Linux Snap
        dirs.push(PathBuf::from("/snap/bin"));
        // Linux Flatpak
        dirs.push(PathBuf::from("/var/lib/flatpak/exports/bin"));
    }

    #[cfg(windows)]
    {
        // Chocolatey
        dirs.push(PathBuf::from("C:\\ProgramData\\chocolatey\\bin"));

        // Scoop (user-local)
        if let Some(home) = dirs::home_dir() {
            dirs.push(home.join("scoop").join("shims"));
        }

        // Common Windows tool locations
        dirs.push(PathBuf::from("C:\\Windows\\System32"));
        dirs.push(PathBuf::from("C:\\Windows"));
    }

    // User's local bin (cross-platform, XDG-style on Linux)
    if let Some(home) = dirs::home_dir() {
        if cfg!(unix) {
            dirs.push(home.join(".local").join("bin"));
        }
    }

    dirs
}

/// Check if a path exists and is executable.
fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        match std::fs::metadata(path) {
            Ok(meta) => meta.is_file() && (meta.permissions().mode() & 0o111 != 0),
            Err(_) => false,
        }
    }

    #[cfg(windows)]
    {
        // On Windows, just check that the file exists.
        // Executability is determined by the file extension (.exe, .bat, .cmd).
        path.exists() && path.is_file()
    }

    #[cfg(not(any(unix, windows)))]
    {
        path.exists() && path.is_file()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_known_system_executable() {
        // On any system, there should be at least one well-known executable.
        let name = if cfg!(windows) { "cmd" } else { "ls" };

        let result = resolve_executable_in(name);
        assert!(result.is_some(), "expected to find '{}' on PATH", name);
    }

    #[test]
    fn resolve_nonexistent_returns_none() {
        let result = resolve_executable_in("this-executable-should-not-exist-12345");
        assert!(result.is_none());
    }

    #[test]
    fn resolve_appends_exe_on_windows() {
        if !cfg!(windows) {
            return;
        }
        // "cmd" should resolve to "cmd.exe" on Windows.
        let result = resolve_executable_in("cmd");
        assert!(result.is_some());
        let path = result.unwrap();
        assert!(path.to_string_lossy().ends_with("cmd.exe"));
    }

    #[test]
    fn platform_fallback_dirs_not_empty() {
        let dirs = platform_fallback_dirs();
        assert!(
            !dirs.is_empty(),
            "platform should have at least one fallback dir"
        );
    }

    #[test]
    fn platform_fallback_dirs_contain_expected_paths() {
        let dirs = platform_fallback_dirs();

        #[cfg(unix)]
        {
            assert!(dirs.contains(&PathBuf::from("/opt/homebrew/bin")));
            assert!(dirs.contains(&PathBuf::from("/snap/bin")));
        }

        #[cfg(windows)]
        {
            assert!(dirs.contains(&PathBuf::from("C:\\ProgramData\\chocolatey\\bin")));
        }
    }

    #[test]
    fn is_executable_rejects_nonexistent() {
        assert!(!is_executable(Path::new("/nonexistent/path/to/binary")));
    }

    #[test]
    fn is_executable_accepts_known_binary() {
        let name = if cfg!(windows) { "cmd" } else { "ls" };
        if let Some(path) = resolve_executable_in(name) {
            assert!(is_executable(&path));
        }
    }
}
