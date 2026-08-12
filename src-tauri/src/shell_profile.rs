use std::path::Path;

/// Sentinel comment that marks agent2ssh-managed PATH entries in shell profiles.
pub const SHELL_PROFILE_SENTINEL: &str = "# agent2ssh-cli-path";

fn escape_shell_double_quoted(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$")
        .replace('`', "\\`")
}

/// Build a PATH assignment using Fish syntax for `config.fish` and POSIX
/// syntax for Bash/Zsh profiles.
pub fn build_shell_profile_line(profile: &Path, dir: &Path) -> String {
    let escaped_dir = escape_shell_double_quoted(&dir.to_string_lossy());
    if profile.file_name().and_then(|name| name.to_str()) == Some("config.fish") {
        format!("set -gx PATH $PATH \"{escaped_dir}\"  {SHELL_PROFILE_SENTINEL}")
    } else {
        format!("export PATH=\"$PATH:{escaped_dir}\"  {SHELL_PROFILE_SENTINEL}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_target_shell_syntax_and_escapes_paths() {
        let dir = Path::new(r#"/opt/agent $tools/"bin""#);
        let sh_line = build_shell_profile_line(Path::new(".zshrc"), dir);
        assert!(sh_line.starts_with("export PATH=\"$PATH:"));
        assert!(sh_line.contains(r#"\$tools/\"bin\""#));
        assert!(sh_line.ends_with(SHELL_PROFILE_SENTINEL));

        let fish_line = build_shell_profile_line(Path::new("config.fish"), dir);
        assert!(fish_line.starts_with("set -gx PATH $PATH \""));
        assert!(!fish_line.contains("export PATH="));
        assert!(fish_line.ends_with(SHELL_PROFILE_SENTINEL));
    }
}
