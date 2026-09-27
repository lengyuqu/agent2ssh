//! Open a discovered container target in the user's own terminal.
//!
//! Discovery hands the UI a ready-made argv (`exec_binary` + `exec_args`, e.g.
//! `docker exec -it abc123 /bin/sh`). Running it inside the desktop process
//! would be useless — a GUI app has no console for an interactive shell — so the
//! window is handed to the platform's terminal instead.
//!
//! ## Why this validates before it spawns
//!
//! The argv arrives from the frontend, and the frontend is the least trusted
//! link in the chain: a renderer bug or a compromised webview must not be able
//! to turn this command into "run any binary with any arguments". So the argv is
//! checked against what discovery itself can produce:
//!
//! 1. the binary must be one *this process* resolved (a known container runtime
//!    or kubectl) — not an arbitrary path the caller picked;
//! 2. every argument must be a single safe token. Container ids, names,
//!    namespaces and flags all live in `[A-Za-z0-9._:/@=-]`, which is what
//!    Docker itself restricts container names to, so the rule costs no real
//!    target while making shell metacharacters unrepresentable.
//!
//! What is left is that a caller could ask for a *different* container than the
//! one discovery found. That is the same authority the feature grants by design
//! (the exec argv is shown to the user as a copyable command), so it is not a
//! further escalation.
//!
//! ## Per-platform launch
//!
//! * **Windows** — `cmd /C start "Agent2SSH" <argv>`. `start` needs a window
//!   title before the command, otherwise a quoted first argument is consumed as
//!   the title.
//! * **macOS** — a `0700` `.command` script in the temp dir, opened with
//!   `open -a Terminal`. Terminal.app runs a document, not an argv, so the
//!   command has to exist as a file; the script `exec`s the argv and is deleted
//!   once Terminal has read it.
//! * **Linux** — the first terminal emulator found, each with its own
//!   "run this argv" convention.

use std::path::PathBuf;

#[cfg(target_os = "macos")]
use std::io::Write;

use crate::container_runtime;
use crate::path_resolver::resolve_executable_in;

/// Longest accepted single argument. Container ids, names and namespaces are far
/// shorter; the cap only bounds the damage of a malformed caller.
const MAX_ARG_LEN: usize = 256;

/// Most arguments a target argv may carry. The longest shape this app builds is
/// the Kubernetes one (11 entries).
const MAX_ARGS: usize = 16;

/// Terminal emulators to try on Linux, with the flag that means "run this argv".
///
/// Only the Linux path reads this table, so unlike the argv builders below it is
/// not compiled under `test` — there the two conventions are asserted directly.
#[cfg(all(unix, not(target_os = "macos")))]
const LINUX_EMULATORS: [(&str, LinuxEmulatorArg); 4] = [
    ("x-terminal-emulator", LinuxEmulatorArg::DashE),
    ("gnome-terminal", LinuxEmulatorArg::DoubleDash),
    ("konsole", LinuxEmulatorArg::DashE),
    ("xterm", LinuxEmulatorArg::DashE),
];

#[cfg(any(all(unix, not(target_os = "macos")), test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LinuxEmulatorArg {
    DashE,
    DoubleDash,
}

/// Is this a single safe argv token?
///
/// Deliberately narrower than "anything without a space": no quoting style has
/// to be trusted anywhere in this module if metacharacters cannot get in, and
/// the allowed set covers every character Docker, wslc and Apple allow in the
/// container names, ids, namespaces and flags that appear in a target argv.
fn is_safe_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_ARG_LEN
        && value.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':' | '/' | '@' | '=')
        })
}

/// The binaries this process is willing to launch: every installed container
/// runtime plus kubectl, each as the absolute path it resolved to.
fn launchable_binaries() -> Vec<PathBuf> {
    let mut binaries: Vec<PathBuf> = container_runtime::available()
        .into_iter()
        .map(|resolved| resolved.binary)
        .collect();
    if let Some(kubectl) = resolve_executable_in("kubectl") {
        binaries.push(kubectl);
    }
    binaries
}

fn normalize_path(path: &str) -> String {
    let unified = path.replace('\\', "/");
    if cfg!(windows) {
        unified.to_ascii_lowercase()
    } else {
        unified
    }
}

/// Validate the argv the frontend asked to launch.
///
/// Returns the resolved binary to spawn with, so the caller cannot accidentally
/// spawn a different (unvalidated) string than the one that was checked.
pub fn validate_target(exec_binary: &str, exec_args: &[String]) -> Result<PathBuf, String> {
    if exec_binary.trim().is_empty() {
        return Err("no exec binary was given for this target".into());
    }
    let matched = launchable_binaries()
        .into_iter()
        .find(|candidate| normalize_path(&candidate.display().to_string()) == normalize_path(exec_binary))
        .ok_or_else(|| {
            format!(
                "{exec_binary} is not a container runtime or kubectl installed on this host, so it will not be launched"
            )
        })?;

    if exec_args.is_empty() {
        return Err("the target has no exec arguments".into());
    }
    if exec_args.len() > MAX_ARGS {
        return Err(format!(
            "the target has {} exec arguments, more than the {MAX_ARGS} this command accepts",
            exec_args.len()
        ));
    }
    for arg in exec_args {
        if !is_safe_token(arg) {
            return Err(format!(
                "refusing to launch: {arg:?} is not a plain container argument"
            ));
        }
    }
    Ok(matched)
}

/// Quote a string for a POSIX shell, single-quote style.
///
/// Only used for the macOS launcher script. Embedded single quotes are closed,
/// escaped and reopened (`'` → `'\''`), which is the one sequence that cannot be
/// broken out of.
#[cfg(any(target_os = "macos", test))]
fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

/// Spawn the target in a new terminal window.
pub fn launch(exec_binary: &str, exec_args: &[String]) -> Result<(), String> {
    let binary = validate_target(exec_binary, exec_args)?;
    let binary = binary.display().to_string();

    #[cfg(windows)]
    {
        let argv = windows_start_argv(&binary, exec_args);
        spawn_detached("cmd", &argv)
    }

    #[cfg(target_os = "macos")]
    {
        let script = write_macos_launcher(&binary, exec_args)?;
        // `open` is a short-lived helper: it exits once LaunchServices has the
        // document, and Terminal reads the script afterwards. Two ordering
        // rules follow. First, wait for `open` to exit — spawning it detached
        // and deleting the file right away raced the helper's own path
        // resolution and Terminal silently never appeared. Second, keep the
        // file alive for a grace period after `open` succeeds, because the
        // Terminal-side read is asynchronous. A failed `open` needs no grace
        // period; the script is removed immediately.
        let opened = std::process::Command::new("open")
            .args(["-a".to_string(), "Terminal".to_string()])
            .arg(&script)
            .status()
            .map_err(|e| format!("failed to start open: {e}"))
            .and_then(|status| {
                if status.success() {
                    Ok(())
                } else {
                    Err(format!("open -a Terminal failed: {status}"))
                }
            });
        if opened.is_ok() {
            let cleanup = script.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(10));
                let _ = std::fs::remove_file(&cleanup);
            });
        } else {
            let _ = std::fs::remove_file(&script);
        }
        opened
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let Some((emulator, flag)) = LINUX_EMULATORS
            .iter()
            .find_map(|(name, flag)| resolve_executable_in(name).map(|path| (path, *flag)))
        else {
            return Err(
                "no terminal emulator was found; copy the command and run it in your terminal instead"
                    .into(),
            );
        };
        let argv = linux_emulator_argv(flag, &binary, exec_args);
        spawn_detached(&emulator.display().to_string(), &argv)
    }

    #[cfg(not(any(unix, windows)))]
    {
        Err("launching a terminal is not supported on this platform".into())
    }
}

/// Start a detached child, reporting a spawn failure as a readable message.
///
/// Only the Windows and Linux branches use it: on macOS the launcher is the
/// short-lived `open` helper, whose completion `launch` must wait for.
#[cfg(any(windows, all(unix, not(target_os = "macos"))))]
fn spawn_detached(program: &str, args: &[String]) -> Result<(), String> {
    std::process::Command::new(program)
        .args(args)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("failed to start {program}: {e}"))
}

/// `cmd /C start <title> <argv>` — the title keeps a quoted first argument from
/// being swallowed as the window title.
#[cfg(any(windows, test))]
fn windows_start_argv(binary: &str, args: &[String]) -> Vec<String> {
    let mut argv = vec![
        "/C".to_string(),
        "start".to_string(),
        "Agent2SSH".to_string(),
        binary.to_string(),
    ];
    argv.extend(args.iter().cloned());
    argv
}

/// `[emulator flag, argv…]` for a Linux terminal emulator.
#[cfg(any(all(unix, not(target_os = "macos")), test))]
fn linux_emulator_argv(flag: LinuxEmulatorArg, binary: &str, args: &[String]) -> Vec<String> {
    let mut argv = vec![match flag {
        LinuxEmulatorArg::DashE => "-e".to_string(),
        LinuxEmulatorArg::DoubleDash => "--".to_string(),
    }];
    argv.push(binary.to_string());
    argv.extend(args.iter().cloned());
    argv
}

/// The body of the macOS launcher script.
///
/// Compiled off macOS too (under `test`): this is the one place the module
/// builds a shell string, so its quoting is asserted wherever the suite runs,
/// not only on the platform that ships it.
#[cfg(any(target_os = "macos", test))]
fn macos_script_contents(binary: &str, args: &[String]) -> String {
    let mut command = format!("exec {}", shell_single_quote(binary));
    for arg in args {
        command.push(' ');
        command.push_str(&shell_single_quote(arg));
    }
    // `exec` replaces the shell, so closing the Terminal window ends the shell
    // inside the container rather than leaving a stray login shell behind.
    format!("#!/bin/sh\n{command}\n")
}

/// Write the macOS launcher script with owner-only permissions.
#[cfg(target_os = "macos")]
fn write_macos_launcher(binary: &str, args: &[String]) -> Result<PathBuf, String> {
    use std::os::unix::fs::PermissionsExt;

    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let path = std::env::temp_dir().join(format!(
        "agent2ssh-container-{}-{stamp}.command",
        std::process::id()
    ));
    let mut file = std::fs::File::create(&path)
        .map_err(|e| format!("failed to create the launcher script: {e}"))?;
    file.write_all(macos_script_contents(binary, args).as_bytes())
        .map_err(|e| format!("failed to write the launcher script: {e}"))?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
        .map_err(|e| format!("failed to secure the launcher script: {e}"))?;
    Ok(path)
}

/// Check a path without requiring it to exist (used by the tests).
#[cfg(test)]
fn same_path(a: &str, b: &str) -> bool {
    normalize_path(a) == normalize_path(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn docker_args() -> Vec<String> {
        vec![
            "exec".into(),
            "-it".into(),
            "abc123def456".into(),
            "/bin/sh".into(),
        ]
    }

    fn k8s_args() -> Vec<String> {
        vec![
            "--context".into(),
            "prod".into(),
            "exec".into(),
            "-n".into(),
            "default".into(),
            "-it".into(),
            "my-pod".into(),
            "-c".into(),
            "app".into(),
            "--".into(),
            "/bin/sh".into(),
        ]
    }

    #[test]
    fn safe_tokens_cover_every_shape_discovery_produces() {
        for token in [
            "exec",
            "-it",
            "--context",
            "--",
            "-n",
            "/bin/sh",
            "abc123def456",
            "488611800bc0",
            "a1b2c3d4-0000-4000-8000-000000000001",
            "my-pod.v2",
            "default",
            "docker.io/library/alpine:latest",
            "user=root",
        ] {
            assert!(is_safe_token(token), "{token} should be accepted");
        }
    }

    #[test]
    fn unsafe_tokens_are_rejected() {
        for token in [
            "",
            "a b",
            "$(rm -rf /)",
            "`id`",
            "a;b",
            "a|b",
            "a&b",
            "a>b",
            "'quoted'",
            "\"quoted\"",
            "new\nline",
            // Over the length cap.
            "x".repeat(MAX_ARG_LEN + 1).as_str(),
        ] {
            assert!(
                !is_safe_token(token),
                "{token:?} must not be accepted as an argv token"
            );
        }
    }

    #[test]
    fn validate_rejects_an_empty_argv_and_an_empty_binary() {
        assert!(validate_target("", &docker_args()).is_err());
        // The binary check runs before the argv check, so use a real binary for
        // the empty-argv case.
        if let Some(resolved) = container_runtime::available().first() {
            let binary = resolved.binary.display().to_string();
            assert!(validate_target(&binary, &[]).is_err());
        }
    }

    #[test]
    fn validate_rejects_a_binary_this_process_did_not_resolve() {
        // A path that is certainly not a container runtime or kubectl.
        let error = validate_target("C:\\Windows\\System32\\cmd.exe", &docker_args())
            .unwrap_err()
            .to_string();
        assert!(error.contains("cmd.exe"), "{error}");
        assert!(error.contains("will not be launched"), "{error}");
    }

    #[test]
    fn validate_accepts_a_real_runtime_when_one_is_installed() {
        if let Some(resolved) = container_runtime::available().first() {
            let binary = resolved.binary.display().to_string();
            let path = validate_target(&binary, &docker_args()).expect("installed runtime");
            assert_eq!(
                normalize_path(&path.display().to_string()),
                normalize_path(&binary)
            );
            // Case differences on the same path must still match on Windows.
            assert!(
                validate_target(&binary.to_uppercase(), &docker_args()).is_ok() == cfg!(windows)
            );
        }
    }

    #[test]
    fn validate_rejects_a_dangerous_argument_on_a_real_runtime() {
        if let Some(resolved) = container_runtime::available().first() {
            let binary = resolved.binary.display().to_string();
            let mut args = docker_args();
            args[2] = "abc; rm -rf /".into();
            let error = validate_target(&binary, &args).unwrap_err().to_string();
            assert!(error.contains("refusing to launch"), "{error}");
        }
    }

    #[test]
    fn validate_bounds_the_argument_count() {
        if let Some(resolved) = container_runtime::available().first() {
            let binary = resolved.binary.display().to_string();
            let args = vec!["exec".to_string(); MAX_ARGS + 1];
            assert!(validate_target(&binary, &args).is_err());
            assert!(validate_target(&binary, &vec!["exec".to_string(); MAX_ARGS]).is_ok());
        }
    }

    #[test]
    fn single_quote_escaping_cannot_be_broken_out_of() {
        assert_eq!(shell_single_quote("plain"), "'plain'");
        assert_eq!(shell_single_quote("it's"), r"'it'\''s'");
        // A path with a quote and a command substitution stays one literal word.
        assert_eq!(
            shell_single_quote("a'; rm -rf /; echo '"),
            r"'a'\''; rm -rf /; echo '\'''"
        );
    }

    #[test]
    fn windows_argv_puts_a_title_before_the_command() {
        let argv = windows_start_argv("C:\\Program Files\\WSL\\wslc.exe", &docker_args());
        assert_eq!(argv[0], "/C");
        assert_eq!(argv[1], "start");
        assert_eq!(argv[2], "Agent2SSH", "start needs a title argument");
        assert_eq!(argv[3], "C:\\Program Files\\WSL\\wslc.exe");
        assert_eq!(&argv[4..], &["exec", "-it", "abc123def456", "/bin/sh"]);
    }

    #[test]
    fn linux_argv_matches_each_emulator_convention() {
        let dash_e = linux_emulator_argv(LinuxEmulatorArg::DashE, "docker", &docker_args());
        assert_eq!(dash_e[0], "-e");
        assert_eq!(
            &dash_e[1..],
            &["docker", "exec", "-it", "abc123def456", "/bin/sh"]
        );

        let double_dash =
            linux_emulator_argv(LinuxEmulatorArg::DoubleDash, "docker", &docker_args());
        assert_eq!(double_dash[0], "--");
        assert_eq!(double_dash[1], "docker");
    }

    #[test]
    fn macos_script_execs_the_argv_and_ends_in_one_line() {
        let script = macos_script_contents(
            "/usr/local/bin/container",
            &[
                "exec".into(),
                "-it".into(),
                "a1b2c3d4-0000-4000-8000-000000000001".into(),
                "/bin/sh".into(),
            ],
        );
        assert_eq!(
            script,
            "#!/bin/sh\nexec '/usr/local/bin/container' 'exec' '-it' 'a1b2c3d4-0000-4000-8000-000000000001' '/bin/sh'\n"
        );
        assert!(script.starts_with("#!/bin/sh\n"));
        assert!(script.ends_with('\n'));
        assert_eq!(script.lines().count(), 2);
    }

    #[test]
    fn k8s_argv_is_representable() {
        // The Kubernetes shape has more flags than the docker one; none of them
        // may trip the token rule.
        for arg in k8s_args() {
            assert!(is_safe_token(&arg), "{arg} should be accepted");
        }
        let script = macos_script_contents("/usr/local/bin/kubectl", &k8s_args());
        assert!(script.contains("'--context' 'prod'"));
        assert!(script.contains("'--' '/bin/sh'"));
    }

    #[test]
    fn same_path_ignores_separator_direction_and_case_on_windows() {
        assert!(same_path("/usr/bin/docker", "/usr/bin/docker"));
        assert!(!same_path("/usr/bin/docker", "/usr/bin/docker2"));
        if cfg!(windows) {
            assert!(same_path(
                "C:\\Program Files\\WSL\\wslc.exe",
                "c:/program files/wsl/wslc.exe"
            ));
        } else {
            assert!(!same_path("/usr/bin/Docker", "/usr/bin/docker"));
        }
    }

    #[test]
    fn launchable_binaries_are_absolute_paths() {
        for binary in launchable_binaries() {
            assert!(binary.is_absolute(), "{binary:?} should be absolute");
        }
    }
}
