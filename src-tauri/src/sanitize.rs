//! Command sanitization: AST-based canonical command head extraction.
//!
//! This module replaces the old glob/substring matching in `risk_config.rs`
//! with a tree-sitter-bash AST parser. Instead of checking whether a command
//! string *contains* a pattern, we parse the command into a syntax tree and
//! extract the **canonical command head** — the actual program being run after
//! stripping wrappers (sudo, env, timeout, nohup) and normalizing aliases
//! (gcp -> cp, gawk -> awk).
//!
//! ## Why AST instead of glob?
//!
//! Glob matching (`command.contains("docker system prune")`) is trivially
//! bypassed:
//! - Flag reordering: `docker system prune -af` != pattern `docker system prune -a`
//! - Wrapper stacking: `sudo docker system prune` doesn't start with `docker`
//! - Alias variation: `gcp file dest` is `cp` on macOS (brew coreutils)
//! - Encoding tricks: `$'\x72\x6d'` decodes to `rm` but contains neither
//!
//! ## fail-closed principle
//!
//! If the parser fails (syntax error, unknown node type, empty tree), the
//! function returns `None`. Callers **must** treat `None` as high-risk
//! (typically `RiskLevel::High` or `RiskLevel::Blocked`), never as "safe".
//! This is the fail-closed principle: better to over-classify a safe command
//! as risky than to let a dangerous one through.

use std::collections::HashSet;

use tree_sitter::{Node, Parser};

/// Initialize the bash parser. The grammar is compiled into the binary at
/// build time, so no external files are needed.
fn bash_parser() -> Result<Parser, tree_sitter::LanguageError> {
    let mut parser = Parser::new();
    let language = tree_sitter_bash::LANGUAGE.into();
    parser.set_language(&language)?;
    Ok(parser)
}

/// Wrappers that prepend another command. We strip these to find the real
/// command head. Each entry is the bare command name (lowercase).
///
/// These wrappers have the form: `wrapper [wrapper-options...] real-command ...`
/// so we skip the wrapper name and its flag arguments (starting with `-`) to
/// find the real command.
const WRAPPERS: &[&str] = &[
    "sudo", "env", "timeout", "nohup", "doas", "su", "pkexec", "nice", "ionice", "stdbuf", "time",
    "command", "builtin", "\\source", ".",
];

/// macOS brew coreutils aliases: `g<cmd>` maps to `<cmd>`.
const BREW_PREFIXES: &[(&str, &str)] = &[
    ("gcp", "cp"),
    ("gmv", "mv"),
    ("grm", "rm"),
    ("gls", "ls"),
    ("gcat", "cat"),
    ("ghead", "head"),
    ("gtail", "tail"),
    ("gchmod", "chmod"),
    ("gchown", "chown"),
    ("gmkdir", "mkdir"),
    ("grmdir", "rmdir"),
    ("gdd", "dd"),
    ("gtruncate", "truncate"),
    ("gstat", "stat"),
    ("gwc", "wc"),
    ("gsort", "sort"),
    ("guniq", "uniq"),
    ("gcut", "cut"),
    ("gpaste", "paste"),
    ("gtr", "tr"),
    ("gtee", "tee"),
    ("gfind", "find"),
    ("gxargs", "xargs"),
    ("grealpath", "realpath"),
    ("greadlink", "readlink"),
    ("gbasename", "basename"),
    ("gdirname", "dirname"),
    ("gmktemp", "mktemp"),
    ("gdu", "du"),
    ("gdf", "df"),
    ("gln", "ln"),
    ("gtouch", "touch"),
    ("guptime", "uptime"),
    ("gwhoami", "whoami"),
    ("gid", "id"),
    ("ggroups", "groups"),
    ("gdate", "date"),
    ("ghostname", "hostname"),
    ("garch", "arch"),
    ("gnproc", "nproc"),
    ("gexpr", "expr"),
    ("gfactor", "factor"),
    ("gseq", "seq"),
    ("gshuf", "shuf"),
];

/// awk variants that should be normalized to `awk`.
const AWK_VARIANTS: &[&str] = &["gawk", "mawk", "nawk"];

/// The result of analyzing a command: the canonical head (or `None` if parsing
/// failed — **fail-closed**), and whether the command had any parse errors.
#[derive(Debug, Clone)]
pub struct CommandAnalysis {
    /// The canonical command name (lowercase, wrapper-stripped, alias-normalized).
    /// `None` means the parser could not extract a reliable head — the caller
    /// must treat this as high-risk (fail-closed).
    pub canonical_head: Option<String>,
    /// True if the tree-sitter parse produced any ERROR or MISSING nodes.
    /// Even if we extracted a head, errors suggest the command may have
    /// obfuscation attempts.
    pub had_parse_errors: bool,
}

/// Parse a command string and extract its canonical command head.
///
/// Returns `CommandAnalysis { canonical_head, had_parse_errors }`.
///
/// ## Algorithm
///
/// 1. Parse the command with tree-sitter-bash.
/// 2. Walk the top-level node to find the first `command` node (skipping
///    pipeline segments, `&&`, `||`, etc.).
/// 3. From the `command` node, extract the command name (first child that is
///    a `word` or `raw_string` or `string` or `concatenation`).
/// 4. If the name is a known wrapper (sudo, env, timeout, ...), skip it and
///    its flag arguments (tokens starting with `-`), then try the next word.
/// 5. Normalize the name: lowercase, strip brew `g` prefix, map awk variants.
/// 6. If at any point we encounter an ERROR node or can't find a command name,
///    set `had_parse_errors = true`. If we can't find any command name at all,
///    return `canonical_head = None` (fail-closed).
pub fn analyze_command(command: &str) -> CommandAnalysis {
    let source = command.as_bytes();
    let mut parser = match bash_parser() {
        Ok(p) => p,
        Err(_) => {
            return CommandAnalysis {
                canonical_head: None,
                had_parse_errors: true,
            }
        }
    };

    let tree = match parser.parse(command, None) {
        Some(t) => t,
        None => {
            return CommandAnalysis {
                canonical_head: None,
                had_parse_errors: true,
            }
        }
    };
    let root = tree.root_node();

    let had_errors = has_errors(root);

    // Walk the top-level to find the first executable command.
    let head = find_first_command_head(&root, source);

    CommandAnalysis {
        canonical_head: head,
        had_parse_errors: had_errors,
    }
}

/// Recursively check if a node (or any descendant) has ERROR or MISSING nodes.
fn has_errors(node: Node) -> bool {
    if node.is_error() || node.is_missing() {
        return true;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if has_errors(child) {
            return true;
        }
    }
    false
}

/// Walk the tree to find the first `command` node, then extract and normalize
/// its head. Returns `None` if no command node is found or the head can't be
/// extracted.
fn find_first_command_head(root: &Node, source: &[u8]) -> Option<String> {
    // The root is typically `program` or `ERROR`. We need to find the first
    // `command` node in the tree.
    let cmd_node = find_first_node_of_type(root, "command")?;
    extract_head_from_command(&cmd_node, source)
}

/// Depth-first search for the first node of a given type.
fn find_first_node_of_type<'a>(node: &Node<'a>, type_name: &str) -> Option<Node<'a>> {
    if node.kind() == type_name {
        return Some(*node);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(found) = find_first_node_of_type(&child, type_name) {
            return Some(found);
        }
    }
    None
}

/// Extract the command head from a `command` AST node.
///
/// A `command` node in tree-sitter-bash has children like:
///   command_name (word) | argument (word) | flag (word starting with -) | ...
///
/// We extract the first word as the command name, then check if it's a wrapper.
/// If so, we skip the wrapper and its flags to find the real command.
fn extract_head_from_command(cmd_node: &Node, source: &[u8]) -> Option<String> {
    let wrapper_set: HashSet<&str> = WRAPPERS.iter().copied().collect();

    // Collect the word-like children of the command node.
    let words = collect_command_words(cmd_node, source);
    if words.is_empty() {
        return None;
    }

    // Try the first word as the command name.
    let mut idx = 0;
    let mut head = words[idx].clone();

    // Strip wrappers: sudo, env, timeout, etc.
    // These have the form: wrapper [-flags...] real-command ...
    // We skip the wrapper name and any subsequent tokens starting with '-'.
    while wrapper_set.contains(&head.to_lowercase().as_str()) {
        let wrapper_lower = head.to_lowercase();
        idx += 1;

        // Special handling per wrapper type.
        match wrapper_lower.as_str() {
            "timeout" => {
                // `timeout` takes a duration argument before the command:
                //   timeout 30s cmd, timeout 5m cmd, timeout --signal=KILL 10 cmd
                // Skip flags (starting with -) and the first non-flag token (duration).
                let mut skipped_duration = false;
                while idx < words.len() {
                    if words[idx].starts_with('-') {
                        // Skip flag and possibly its value for long options.
                        if words[idx].contains('=') {
                            idx += 1;
                        } else {
                            idx += 1;
                        }
                    } else if !skipped_duration {
                        // Skip the duration token.
                        skipped_duration = true;
                        idx += 1;
                    } else {
                        break;
                    }
                }
            }
            "sudo" => {
                // sudo flags that take a value argument: -u/--user, -g/--group,
                // -C/--close-from, -D/--chdir, -R/--role, -T/--type, -P/--preserve-env
                let value_flags = [
                    "-u",
                    "--user",
                    "-g",
                    "--group",
                    "-C",
                    "--close-from",
                    "-D",
                    "--chdir",
                    "-R",
                    "-T",
                    "-P",
                ];
                while idx < words.len() {
                    if words[idx].starts_with('-') {
                        // Check if this is a long option with '=' (e.g. --user=root).
                        if words[idx].contains('=') {
                            idx += 1;
                        } else if value_flags.contains(&words[idx].as_str()) {
                            // Skip the flag and its value argument.
                            idx += 2;
                        } else {
                            // Short flag without value, or unknown flag.
                            idx += 1;
                        }
                    } else {
                        break;
                    }
                }
            }
            "doas" => {
                // doas -u user cmd — similar to sudo.
                let value_flags = ["-u", "--user"];
                while idx < words.len() {
                    if words[idx].starts_with('-') {
                        if words[idx].contains('=') {
                            idx += 1;
                        } else if value_flags.contains(&words[idx].as_str()) {
                            idx += 2;
                        } else {
                            idx += 1;
                        }
                    } else {
                        break;
                    }
                }
            }
            "su" => {
                // su [username] [-c command] — skip the username argument.
                // su takes an optional username, then possibly -c 'command'.
                if idx < words.len() && !words[idx].starts_with('-') {
                    idx += 1; // skip username
                }
                // Then skip flags.
                while idx < words.len() && words[idx].starts_with('-') {
                    idx += 1;
                }
            }
            _ => {
                // Generic wrapper: skip flag arguments (tokens starting with '-').
                while idx < words.len() && words[idx].starts_with('-') {
                    idx += 1;
                }
            }
        }

        // Also handle env VAR=value assignments.
        if wrapper_lower == "env" {
            while idx < words.len() && is_wrapper_value("env", &words[idx], idx) {
                idx += 1;
            }
            // Skip flags too.
            while idx < words.len() && words[idx].starts_with('-') {
                idx += 1;
            }
        }

        if idx >= words.len() {
            // The entire command was just wrappers + flags, no real command.
            return Some(normalize_name(&head));
        }
        head = words[idx].clone();
    }

    Some(normalize_name(&head))
}

/// For certain wrappers, the next non-flag argument is a value, not a command.
/// For example, `env VAR=value cmd` — `VAR=value` is a value assignment, not
/// a command. We should skip it.
fn is_wrapper_value(wrapper: &str, token: &str, _idx: usize) -> bool {
    // `env` accepts VAR=value assignments before the command.
    if wrapper == "env" {
        return token.contains('=');
    }
    // `su` takes a username argument: `su root -c cmd`.
    // We don't handle `-c` here; the `-c` will be skipped as a flag.
    false
}

/// Collect all word-like tokens from a `command` node.
///
/// The tree-sitter-bash `command` node has this structure:
/// ```text
/// command
///   command_name        ← contains the actual program name
///     word / raw_string / ansi_c_string / string
///   word                ← arguments
///   word
///   ...
/// ```
/// We must first extract the `command_name` child to get the program name,
/// then collect the remaining argument words.
fn collect_command_words(cmd_node: &Node, source: &[u8]) -> Vec<String> {
    let mut words = Vec::new();
    let mut cursor = cmd_node.walk();

    for child in cmd_node.children(&mut cursor) {
        match child.kind() {
            // The command_name node wraps the actual program name token.
            "command_name" => {
                if let Some(text) = extract_token_text(&child, source) {
                    words.push(text);
                }
            }
            // Direct argument tokens.
            "word" => {
                if let Ok(text) = child.utf8_text(source) {
                    words.push(text.to_string());
                }
            }
            // Numeric arguments (e.g. `timeout 30 cmd` — `30` is a `number` node).
            "number" => {
                if let Ok(text) = child.utf8_text(source) {
                    words.push(text.to_string());
                }
            }
            "raw_string" => {
                if let Ok(text) = child.utf8_text(source) {
                    let decoded = decode_ansi_c_escapes(text);
                    words.push(decoded);
                }
            }
            "string" => {
                if let Ok(text) = child.utf8_text(source) {
                    let stripped = strip_quotes(text);
                    words.push(stripped);
                }
            }
            "concatenation" => {
                if let Some(text) = extract_concatenation_text(&child, source) {
                    words.push(text);
                }
            }
            _ => {}
        }
    }

    words
}

/// Extract text from a `command_name` node by looking at its single child
/// (which may be `word`, `raw_string`, `ansi_c_string`, or `string`).
fn extract_token_text(node: &Node, source: &[u8]) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "word" => {
                return child.utf8_text(source).ok().map(|s| s.to_string());
            }
            "raw_string" | "ansi_c_string" => {
                // ANSI-C quoting like $'\x72\x6d' — decode escapes.
                return child
                    .utf8_text(source)
                    .ok()
                    .map(|s| decode_ansi_c_escapes(s));
            }
            "string" => {
                return child.utf8_text(source).ok().map(|s| strip_quotes(s));
            }
            "concatenation" => {
                return extract_concatenation_text(&child, source);
            }
            _ => {}
        }
    }
    None
}

/// Decode ANSI-C escape sequences like `$'\x72\x6d'` to their literal values.
/// This is a simple decoder for the most common escapes used in obfuscation.
fn decode_ansi_c_escapes(s: &str) -> String {
    // Strip the $'...' wrapper if present.
    let inner = if s.starts_with("$'") && s.ends_with('\'') {
        &s[2..s.len() - 1]
    } else if s.starts_with("'") && s.ends_with('\'') {
        &s[1..s.len() - 1]
    } else {
        return s.to_string();
    };

    let mut result = String::new();
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some(escape_char @ ('x' | 'u' | 'U')) => {
                    // Hex escape: \xNN, \uNNNN, \UNNNNNNNN
                    let hex_len = if escape_char == 'x' {
                        2
                    } else if escape_char == 'u' {
                        4
                    } else {
                        8
                    };
                    let mut hex = String::new();
                    for _ in 0..hex_len {
                        if let Some(h) = chars.next() {
                            hex.push(h);
                        }
                    }
                    if let Ok(code) = u32::from_str_radix(&hex, 16) {
                        if let Some(ch) = char::from_u32(code) {
                            result.push(ch);
                        }
                    }
                }
                Some('n') => result.push('\n'),
                Some('t') => result.push('\t'),
                Some('r') => result.push('\r'),
                Some('\\') => result.push('\\'),
                Some('\'') => result.push('\''),
                Some('"') => result.push('"'),
                Some(other) => {
                    result.push('\\');
                    result.push(other);
                }
                None => result.push('\\'),
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Strip surrounding quotes from a string.
fn strip_quotes(s: &str) -> String {
    if (s.starts_with('"') && s.ends_with('"') && s.len() >= 2)
        || (s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2)
    {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

/// Extract text from a concatenation node (e.g. "pre"$var"post").
fn extract_concatenation_text(node: &Node, source: &[u8]) -> Option<String> {
    let mut result = String::new();
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "word" | "raw_string" | "string" => {
                if let Ok(text) = child.utf8_text(source) {
                    result.push_str(&decode_ansi_c_escapes(text));
                }
            }
            _ => break, // Stop at the first non-word child.
        }
    }
    if result.is_empty() {
        None
    } else {
        Some(result)
    }
}

/// Normalize a command name: lowercase, strip brew `g` prefix, map awk variants.
fn normalize_name(name: &str) -> String {
    let lower = name.to_lowercase();

    // Map awk variants.
    if AWK_VARIANTS.contains(&lower.as_str()) {
        return "awk".to_string();
    }

    // Strip brew coreutils `g` prefix.
    for (brew, canonical) in BREW_PREFIXES {
        if lower == *brew {
            return canonical.to_string();
        }
    }

    // Strip leading path components: /usr/bin/rm -> rm.
    // This is important because `/usr/bin/rm -rf /` should match `rm`.
    let basename = lower.rsplit('/').next().unwrap_or(&lower).to_string();

    basename
}

/// Convenience function: extract just the canonical head, or `None` on failure.
/// This is the main entry point for risk classification.
pub fn canonical_head(command: &str) -> Option<String> {
    analyze_command(command).canonical_head
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Basic command extraction ──────────────────────────────────────────────

    #[test]
    fn extracts_simple_command() {
        assert_eq!(canonical_head("ls -la"), Some("ls".into()));
        assert_eq!(canonical_head("cat /etc/hosts"), Some("cat".into()));
        assert_eq!(canonical_head("rm -rf /tmp/stuff"), Some("rm".into()));
    }

    #[test]
    fn extracts_empty_command() {
        assert_eq!(canonical_head(""), None);
        assert_eq!(canonical_head("   "), None);
    }

    // ── Wrapper stripping ─────────────────────────────────────────────────────

    #[test]
    fn strips_sudo() {
        assert_eq!(canonical_head("sudo rm -rf /"), Some("rm".into()));
        assert_eq!(canonical_head("sudo shutdown"), Some("shutdown".into()));
        assert_eq!(canonical_head("sudo whoami"), Some("whoami".into()));
    }

    #[test]
    fn strips_sudo_with_flags() {
        assert_eq!(canonical_head("sudo -u root rm -rf /"), Some("rm".into()));
        assert_eq!(
            canonical_head("sudo --user=root systemctl stop nginx"),
            Some("systemctl".into())
        );
    }

    #[test]
    fn strips_env_wrapper() {
        assert_eq!(canonical_head("env VAR=value rm -rf /"), Some("rm".into()));
        assert_eq!(
            canonical_head("env FOO=bar BAR=baz kubectl delete pod x"),
            Some("kubectl".into())
        );
    }

    #[test]
    fn strips_timeout_wrapper() {
        assert_eq!(
            canonical_head("timeout 30s dd if=/dev/zero of=/dev/sda"),
            Some("dd".into())
        );
        assert_eq!(
            canonical_head("timeout --signal=KILL 10 rm -rf /"),
            Some("rm".into())
        );
    }

    #[test]
    fn strips_nohup_wrapper() {
        assert_eq!(canonical_head("nohup ./server &"), Some("server".into()));
    }

    #[test]
    fn strips_nested_wrappers() {
        assert_eq!(
            canonical_head("sudo env VAR=val timeout 10 rm -rf /"),
            Some("rm".into())
        );
        assert_eq!(
            canonical_head("sudo timeout 30 shutdown -h now"),
            Some("shutdown".into())
        );
    }

    // ── Path stripping ────────────────────────────────────────────────────────

    #[test]
    fn strips_path_prefix() {
        assert_eq!(canonical_head("/usr/bin/rm -rf /"), Some("rm".into()));
        assert_eq!(
            canonical_head("/bin/dd if=/dev/zero of=/dev/sda"),
            Some("dd".into())
        );
    }

    // ── Alias normalization ───────────────────────────────────────────────────

    #[test]
    fn normalizes_brew_prefixes() {
        assert_eq!(canonical_head("gcp file dest"), Some("cp".into()));
        assert_eq!(canonical_head("gchmod 777 /etc"), Some("chmod".into()));
        assert_eq!(
            canonical_head("sudo gdd if=/dev/zero of=/dev/sda"),
            Some("dd".into())
        );
    }

    #[test]
    fn normalizes_awk_variants() {
        assert_eq!(canonical_head("gawk '{print $1}'"), Some("awk".into()));
        assert_eq!(canonical_head("mawk '{print $1}'"), Some("awk".into()));
        assert_eq!(canonical_head("nawk '{print $1}'"), Some("awk".into()));
    }

    // ── ANSI-C hex escape decoding ────────────────────────────────────────────

    #[test]
    fn decodes_ansi_c_hex_escape() {
        // $'\x72\x6d' decodes to "rm"
        assert_eq!(canonical_head("$'\\x72\\x6d' -rf /"), Some("rm".into()));
    }

    // ── Pipeline handling ────────────────────────────────────────────────────

    #[test]
    fn extracts_first_command_in_pipeline() {
        assert_eq!(
            canonical_head("cat /etc/passwd | grep root"),
            Some("cat".into())
        );
    }

    // ── Logical operator handling ────────────────────────────────────────────

    #[test]
    fn extracts_first_command_in_chain() {
        assert_eq!(canonical_head("ls -la && rm -rf /tmp"), Some("ls".into()));
        assert_eq!(
            canonical_head("test -f /file || rm -rf /"),
            Some("test".into())
        );
    }

    // ── fail-closed: parse errors return None ────────────────────────────────

    #[test]
    fn fail_closed_on_unparseable() {
        // Completely unparseable garbage.
        assert_eq!(canonical_head(")))((("), None);
    }

    #[test]
    fn analysis_reports_parse_errors() {
        let analysis = analyze_command(")))(((");
        assert!(analysis.had_parse_errors);
        assert_eq!(analysis.canonical_head, None);
    }

    // ── Case insensitivity ──────────────────────────────────────────────────

    #[test]
    fn lowercases_command() {
        assert_eq!(canonical_head("RM -RF /"), Some("rm".into()));
        assert_eq!(canonical_head("SUDO REBOOT"), Some("reboot".into()));
    }

    // ── Subshell handling ───────────────────────────────────────────────────

    #[test]
    fn extracts_command_in_subshell() {
        assert_eq!(canonical_head("(rm -rf /tmp)"), Some("rm".into()));
    }

    // ── Complex real-world commands ──────────────────────────────────────────

    #[test]
    fn docker_prune() {
        assert_eq!(
            canonical_head("docker system prune -af"),
            Some("docker".into())
        );
    }

    #[test]
    fn kubectl_delete() {
        assert_eq!(
            canonical_head("kubectl delete namespace kube-system"),
            Some("kubectl".into())
        );
    }

    #[test]
    fn terraform_destroy() {
        assert_eq!(
            canonical_head("terraform destroy -auto-approve"),
            Some("terraform".into())
        );
    }

    #[test]
    fn fork_bomb() {
        // The fork bomb :(){ :|:& };:
        // tree-sitter-bash may parse this as a function definition + command.
        // We just verify it doesn't crash.
        let _ = canonical_head(":(){ :|:& };:");
    }
}
