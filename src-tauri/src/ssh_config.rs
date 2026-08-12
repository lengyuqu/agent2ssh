//! B38: SSH Config `Include` directive — recursive expansion.
//!
//! OpenSSH's `Include` directive lets users split `~/.ssh/config` into
//! multiple files (e.g. `Include config.d/*`). Without expansion, those
//! hosts are invisible to Agent2SSH's import and sync flows.
//!
//! This module reads a root config file, splices `Include` directives
//! in place (recursively, with cycle detection and depth limits matching
//! OpenSSH's `MAX_INCLUDE_DEPTH = 16`), and returns the fully expanded
//! text. The existing line-by-line parser in `core.rs` then works as-is.
//!
//! Design adapted from rssh's `ssh/config.rs`:
//! - Relative patterns resolve against the config file's directory
//!   (OpenSSH semantics: `~/.ssh` for user configs).
//! - `~` is expanded via `dirs::home_dir()`.
//! - Glob patterns use `std::fs` (no external `glob` crate needed).
//! - Cycle detection: canonicalized paths in a DFS chain.
//! - Missing files / empty globs are silently skipped (OpenSSH behavior).

use std::path::{Path, PathBuf};

/// Maximum `Include` nesting depth — matches OpenSSH's `MAX_INCLUDE_DEPTH`.
const MAX_INCLUDE_DEPTH: usize = 16;

/// Read an ssh config file and splice the contents of `Include` directives
/// in place, recursively. `base` is the directory non-absolute patterns
/// resolve against (OpenSSH semantics: `~/.ssh` for user configs).
///
/// Missing or unreadable included files are skipped, like a glob with no
/// matches. A file already on the active include chain is skipped too, so
/// cycles don't duplicate content. IO errors on the root file propagate.
pub fn load_with_includes(path: &Path, base: &Path) -> std::io::Result<String> {
    let content = std::fs::read_to_string(path)?;
    let mut out = String::new();
    let mut chain = vec![canonical(path)];
    splice_includes(&content, base, &mut chain, &mut out);
    Ok(out)
}

/// `chain` holds the canonicalized files currently being expanded, root
/// first — membership detects cycles, length bounds the depth.
fn splice_includes(content: &str, base: &Path, chain: &mut Vec<PathBuf>, out: &mut String) {
    for line in content.lines() {
        match include_patterns(line) {
            Some(patterns) if chain.len() <= MAX_INCLUDE_DEPTH => {
                for pat in split_include_patterns(patterns) {
                    splice_pattern(&pat, base, chain, out);
                }
            }
            _ => {
                out.push_str(line);
                out.push('\n');
            }
        }
    }
}

/// `Include p1 p2 ...` → Some("p1 p2 ..."); anything else → None.
fn include_patterns(line: &str) -> Option<&str> {
    let (key, value) = line.trim().split_once(char::is_whitespace)?;
    key.eq_ignore_ascii_case("include").then_some(value.trim())
}

/// Whitespace-separated patterns; double quotes keep embedded spaces
/// (`Include "My Keys/*.conf"`), matching OpenSSH's argument lexer.
fn split_include_patterns(value: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    for c in value.chars() {
        match c {
            '"' => in_quotes = !in_quotes,
            c if c.is_whitespace() && !in_quotes => {
                if !cur.is_empty() {
                    tokens.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        tokens.push(cur);
    }
    tokens
}

fn splice_pattern(pattern: &str, base: &Path, chain: &mut Vec<PathBuf>, out: &mut String) {
    let expanded = expand_tilde(pattern);
    let full = if Path::new(&expanded).is_absolute() {
        PathBuf::from(&expanded)
    } else {
        base.join(&expanded)
    };

    // Expand glob patterns manually (avoid external `glob` crate dependency).
    // If the path contains no glob metacharacters, treat it as a literal path.
    let full_str = full.to_string_lossy();
    if !has_glob_meta(&full_str) {
        expand_single_file(&full, base, chain, out);
        return;
    }

    // Glob expansion: list the parent directory and match.
    let parent = full.parent();
    let file_name = full.file_name();
    if let (Some(parent), Some(file_name)) = (parent, file_name) {
        if let Ok(entries) = std::fs::read_dir(parent) {
            let pattern_str = file_name.to_string_lossy();
            let mut matches: Vec<PathBuf> = entries
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.file_name()
                        .to_str()
                        .map(|name| glob_match(&pattern_str, name))
                        .unwrap_or(false)
                })
                .map(|e| e.path())
                .collect();
            // Sort for deterministic order (matches glob crate behavior).
            matches.sort();
            for path in matches {
                expand_single_file(&path, base, chain, out);
            }
        }
    }
}

fn expand_single_file(path: &Path, base: &Path, chain: &mut Vec<PathBuf>, out: &mut String) {
    let cpath = canonical(path);
    if chain.contains(&cpath) {
        return; // cycle — this file is an ancestor of itself
    }
    let Ok(content) = std::fs::read_to_string(path) else {
        return;
    };
    chain.push(cpath);
    splice_includes(&content, base, chain, out);
    chain.pop();
}

/// Check if a string contains glob metacharacters.
fn has_glob_meta(s: &str) -> bool {
    s.contains(['*', '?', '[', ']'])
}

/// Simple glob matcher supporting `*` (any sequence), `?` (single char),
/// and `[...]` (character class). Does NOT support `{a,b}` brace expansion
/// (OpenSSH's `Include` doesn't either — that's a separate PathSpec feature).
fn glob_match(pattern: &str, name: &str) -> bool {
    glob_match_bytes(pattern.as_bytes(), name.as_bytes())
}

fn glob_match_bytes(pattern: &[u8], name: &[u8]) -> bool {
    let mut p = 0usize;
    let mut n = 0usize;
    let mut star_p: Option<usize> = None;
    let mut star_n = 0usize;

    while n < name.len() {
        if p < pattern.len() {
            match pattern[p] {
                b'?' => {
                    p += 1;
                    n += 1;
                    continue;
                }
                b'*' => {
                    star_p = Some(p);
                    star_n = n;
                    p += 1;
                    continue;
                }
                b'[' => {
                    // Character class: [abc] or [a-z] or [!abc]
                    if let Some(end) = pattern[p..].iter().position(|&c| c == b']') {
                        let class = &pattern[p + 1..p + end];
                        let (negated, chars) = if !class.is_empty() && class[0] == b'!' {
                            (true, &class[1..])
                        } else {
                            (false, class)
                        };
                        let matched = char_class_match(chars, name[n]);
                        if matched != negated {
                            p += end + 1;
                            n += 1;
                            continue;
                        }
                    }
                    // Malformed class — treat `[` as literal
                    if pattern[p] == name[n] {
                        p += 1;
                        n += 1;
                        continue;
                    }
                }
                c => {
                    if c == name[n] {
                        p += 1;
                        n += 1;
                        continue;
                    }
                }
            }
        }
        // No match — backtrack to last `*`
        if let Some(sp) = star_p {
            p = sp + 1;
            star_n += 1;
            n = star_n;
        } else {
            return false;
        }
    }

    // Consume trailing `*` in pattern
    while p < pattern.len() && pattern[p] == b'*' {
        p += 1;
    }
    p == pattern.len()
}

/// Match a character class like `a-z` or `abc`.
fn char_class_match(class: &[u8], c: u8) -> bool {
    let mut i = 0;
    while i < class.len() {
        if i + 2 < class.len() && class[i + 1] == b'-' {
            if c >= class[i] && c <= class[i + 2] {
                return true;
            }
            i += 3;
        } else {
            if class[i] == c {
                return true;
            }
            i += 1;
        }
    }
    false
}

/// Symlink-stable identity for cycle detection; unresolvable paths fall
/// back to themselves (they failed read_to_string anyway).
fn canonical(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

fn expand_tilde(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest).to_string_lossy().to_string();
        }
    }
    path.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    fn write(dir: &Path, rel: &str, content: &str) -> PathBuf {
        let p = dir.join(rel);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&p, content).unwrap();
        p
    }

    #[test]
    fn no_include_content_passes_through_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let src = "Host one\n    HostName 1.example\n\n# comment\n";
        let root = write(dir.path(), "config", src);
        assert_eq!(load_with_includes(&root, dir.path()).unwrap(), src);
    }

    #[test]
    fn include_splices_glob_matches() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "config.d/alpha",
            "Host alpha\n    HostName 10.0.0.1\n",
        );
        write(
            dir.path(),
            "config.d/beta",
            "Host beta\n    HostName 10.0.0.2\n",
        );
        let root = write(
            dir.path(),
            "config",
            "Host main\n    HostName main.example\n\nInclude config.d/*\n",
        );
        let content = load_with_includes(&root, dir.path()).unwrap();
        assert!(content.contains("Host main"));
        assert!(content.contains("Host alpha"));
        assert!(content.contains("Host beta"));
    }

    #[test]
    fn include_nested_two_levels() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "level2",
            "Host deep\n    HostName deep.example\n",
        );
        write(
            dir.path(),
            "level1",
            "Include level2\nHost mid\n    HostName mid.example\n",
        );
        let root = write(dir.path(), "config", "Include level1\n");
        let content = load_with_includes(&root, dir.path()).unwrap();
        assert!(content.contains("Host deep"));
        assert!(content.contains("Host mid"));
    }

    #[test]
    fn include_missing_target_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let root = write(
            dir.path(),
            "config",
            "Include nope/*\nInclude absent-file\nHost real\n    HostName r.example\n",
        );
        let content = load_with_includes(&root, dir.path()).unwrap();
        assert!(content.contains("Host real"));
        assert!(!content.contains("Host nope"));
    }

    #[test]
    fn include_absolute_path() {
        let dir = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let abs = write(
            other.path(),
            "extra",
            "Host abs\n    HostName abs.example\n",
        );
        let root = write(
            dir.path(),
            "config",
            &format!("Include {}\n", abs.display()),
        );
        let content = load_with_includes(&root, dir.path()).unwrap();
        assert!(content.contains("Host abs"));
    }

    #[test]
    fn include_self_reference_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let root = write(
            dir.path(),
            "config",
            "Include config\nHost x\n    HostName x.example\n",
        );
        let content = load_with_includes(&root, dir.path()).unwrap();
        assert!(content.contains("Host x"));
        // Should not contain duplicated content from the cycle
        let count = content.matches("Host x").count();
        assert_eq!(count, 1);
    }

    #[test]
    fn include_mutual_cycle_expands_each_file_once() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "a",
            "Include b\nHost a\n    HostName a.example\n",
        );
        write(
            dir.path(),
            "b",
            "Include a\nHost b\n    HostName b.example\n",
        );
        let root = write(dir.path(), "config", "Include a\n");
        let content = load_with_includes(&root, dir.path()).unwrap();
        assert!(content.contains("Host a"));
        assert!(content.contains("Host b"));
        let a_count = content.matches("Host a").count();
        let b_count = content.matches("Host b").count();
        assert_eq!(a_count, 1);
        assert_eq!(b_count, 1);
    }

    #[test]
    fn include_quoted_pattern_with_spaces() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "my keys/host.conf",
            "Host quoted\n    HostName q.example\n",
        );
        write(dir.path(), "plain", "Host plain\n    HostName p.example\n");
        let root = write(dir.path(), "config", "Include \"my keys/*.conf\" plain\n");
        let content = load_with_includes(&root, dir.path()).unwrap();
        assert!(content.contains("Host quoted"));
        assert!(content.contains("Host plain"));
    }

    #[test]
    fn include_multiple_patterns_on_one_line() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "a", "Host a\n    HostName a.example\n");
        write(dir.path(), "b", "Host b\n    HostName b.example\n");
        let root = write(dir.path(), "config", "Include a b\n");
        let content = load_with_includes(&root, dir.path()).unwrap();
        assert!(content.contains("Host a"));
        assert!(content.contains("Host b"));
    }

    #[test]
    fn include_root_missing_is_io_error() {
        let dir = tempfile::tempdir().unwrap();
        let err = load_with_includes(&dir.path().join("config"), dir.path()).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }

    #[test]
    fn include_tilde_expansion() {
        // Tilde expansion requires a home directory; in test environments
        // dirs::home_dir() typically works. This test verifies the path
        // is expanded and not left as ~/...
        let dir = tempfile::tempdir().unwrap();
        let home = dirs::home_dir().unwrap();
        write(
            &home,
            ".ssh/test_include_file",
            "Host tilde\n    HostName t.example\n",
        );
        let root = write(dir.path(), "config", "Include ~/.ssh/test_include_file\n");
        let content = load_with_includes(&root, dir.path()).unwrap();
        assert!(content.contains("Host tilde"));
        // Cleanup
        let _ = fs::remove_file(home.join(".ssh/test_include_file"));
    }

    // ── glob_match unit tests ───────────────────────────────────────────

    #[test]
    fn glob_match_star() {
        assert!(glob_match("*.conf", "alpha.conf"));
        assert!(glob_match("*.conf", "beta.conf"));
        assert!(!glob_match("*.conf", "alpha.txt"));
    }

    #[test]
    fn glob_match_question() {
        assert!(glob_match("?.txt", "a.txt"));
        assert!(!glob_match("?.txt", "ab.txt"));
    }

    #[test]
    fn glob_match_literal() {
        assert!(glob_match("config", "config"));
        assert!(!glob_match("config", "other"));
    }

    #[test]
    fn glob_match_char_class() {
        assert!(glob_match("[ab].conf", "a.conf"));
        assert!(glob_match("[ab].conf", "b.conf"));
        assert!(!glob_match("[ab].conf", "c.conf"));
    }

    #[test]
    fn glob_match_star_matches_empty() {
        assert!(glob_match("*.conf", ".conf"));
    }

    #[test]
    fn glob_match_star_at_end() {
        assert!(glob_match("config*", "config"));
        assert!(glob_match("config*", "config.d"));
    }

    #[test]
    fn has_glob_meta_detects_metachars() {
        assert!(has_glob_meta("*.conf"));
        assert!(has_glob_meta("config?"));
        assert!(has_glob_meta("[abc]"));
        assert!(!has_glob_meta("plain"));
        assert!(!has_glob_meta("path/to/file"));
    }
}
