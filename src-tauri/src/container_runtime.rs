//! Container runtime abstraction — the single source of truth for *which* local
//! container CLI agent2ssh can drive, whether this host supports it, where its
//! binary lives, and how to ask it for a container list or an exec.
//!
//! Before this module the discovery code hard-coded `docker`: one binary name,
//! one set of subcommands, one JSON dialect. That is now a table:
//!
//! | runtime | binary | list | exec (interactive) | exec (capture) | remove |
//! |---------|--------|------|--------------------|----------------|--------|
//! | Docker  | `docker`  | `ps [--all] --format {{json .}}` | `exec -it <id> <shell>` | `exec <id> <shell> -c <cmd>` | `rm -f <id>` |
//! | wslc    | `wslc`    | `ps [--a] --format json`         | `exec -it <id> <shell>` | `exec <id> <shell> -c <cmd>` | `rm -f <id>` |
//! | Apple   | `container` | `list [--all] --format json`   | `exec -it <id> <shell>` | `exec <id> <shell> -c <cmd>` | `rm -f <id>` |
//!
//! The two exec shapes are two semantics, not two spellings of one: `-it`
//! allocates a TTY for a human at a terminal, and a TTY corrupts captured
//! output (CRLF translation, prompt echoing), so the capture shape omits both
//! flags. See [`ContainerRuntime::exec_args`] and
//! [`ContainerRuntime::exec_capture_args`].
//!
//! `wslc rm` and `container rm` are documented aliases of `wslc remove` and
//! `container delete` (Microsoft's `wslc remove --help` lists `rm`/`delete` as
//! aliases; Apple's command reference titles the command "delete (rm)"), so the
//! remove spelling is shared and the callers that only need to stop a container
//! do not have to branch per runtime.
//!
//! ## Dialect notes (verified, not assumed)
//!
//! * **wslc 2.9.13** — `wslc ps --format json` emits **JSON-lines** with the
//!   docker-compatible keys `ID`/`Image`/`Names`/`Status` and additionally
//!   `State`/`RunningFor`. Verified on a real container: `create --name
//!   a2s-probe alpine:latest true` then `wslc ps -a --format json`. Its binary
//!   ships in `C:\Program Files\WSL`, which is *not* on `PATH` by default, hence
//!   the extra search dir below.
//! * **Apple `container`** — `container list --format json` renders
//!   `ManagedContainer` (`Sources/ContainerResource/Container/ManagedContainer.swift`),
//!   i.e. a JSON **array** of
//!   `{"id": …, "configuration": {"image": {"reference": …}}, "status": {"state": …}}`.
//!   Container names *are* ids on this runtime (`ManagedContainer.name` returns
//!   `configuration.id`), so the discovered name and id are the same string.
//!
//! ## Selection
//!
//! `select()` resolves one runtime for callers that need exactly one (the e2e
//! harness, `agent2ssh containers runtime`). Explicit request → `AGENT2SSH_CONTAINER_RUNTIME`
//! → first installed runtime in `ContainerRuntime::ALL` order. `available()`
//! returns all of them, which is what discovery iterates so a host running both
//! Docker and wslc lists containers from both.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::path_resolver::resolve_executable_in;

/// Environment variable that pins the container runtime by id, for hosts where
/// auto-detection would pick the wrong one.
pub const RUNTIME_ENV: &str = "AGENT2SSH_CONTAINER_RUNTIME";

/// When set to a truthy value (`1`, `true`, `yes`, `on`), discovery starts a
/// stopped runtime service itself instead of reporting that it is stopped.
/// Off by default: starting a system service is a side effect a listing
/// command should not have unasked. `agent2ssh containers start` is the
/// explicit way.
pub const AUTOSTART_ENV: &str = "AGENT2SSH_CONTAINER_AUTOSTART";

/// How long a service status probe may take before the service counts as
/// unresponsive. Apple's answers in well under 100 ms when it is up.
const SERVICE_PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// How long `start` waits for the service to come up.
const SERVICE_START_TIMEOUT: Duration = Duration::from_secs(60);

/// The shell used for `exec` targets. Every runtime here is a Linux container
/// runtime, so `sh` is the portable choice.
pub const CONTAINER_SHELL: &str = "/bin/sh";

/// Local single-host container runtimes this build knows how to drive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContainerRuntime {
    /// Docker Engine (Docker Desktop / Colima / native dockerd).
    Docker,
    /// Windows Subsystem for Linux container CLI.
    Wslc,
    /// Apple's container runtime (`container`, macOS 26+ on Apple silicon).
    Apple,
}

impl ContainerRuntime {
    /// All known runtimes, in auto-detection priority order.
    pub const ALL: [ContainerRuntime; 3] = [Self::Docker, Self::Wslc, Self::Apple];

    /// Stable id: the string that appears in `--runtime`, in the environment
    /// variable, and on the wire in `ContainerRuntimeInfo`.
    pub const fn id(self) -> &'static str {
        match self {
            Self::Docker => "docker",
            Self::Wslc => "wslc",
            Self::Apple => "apple",
        }
    }

    /// Parse an id back to a runtime. Case-insensitive so `--runtime Apple` and
    /// `AGENT2SSH_CONTAINER_RUNTIME=Apple` both work.
    pub fn from_id(id: &str) -> Option<Self> {
        let needle = id.trim().to_ascii_lowercase();
        Self::ALL.into_iter().find(|rt| rt.id() == needle)
    }

    /// Human-facing name, used by the CLI and the desktop UI.
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Docker => "Docker",
            Self::Wslc => "WSL containers (wslc)",
            Self::Apple => "Apple container",
        }
    }

    /// Binary names to try, in order.
    pub const fn binary_candidates(self) -> &'static [&'static str] {
        match self {
            Self::Docker => &["docker"],
            Self::Wslc => &["wslc"],
            Self::Apple => &["container"],
        }
    }

    /// Directories to search in addition to the ones `path_resolver` already
    /// covers. Docker and Apple ship their CLI into locations that are on
    /// `PATH` or in the shared fallback list, so only wslc needs one.
    pub fn extra_search_dirs(self) -> Vec<PathBuf> {
        match self {
            // `wslc.exe` lives beside the WSL service; the installer does not
            // add the directory to PATH.
            Self::Wslc => vec![PathBuf::from("C:\\Program Files\\WSL")],
            Self::Docker | Self::Apple => Vec::new(),
        }
    }

    /// Whether this host's OS can run the runtime at all.
    ///
    /// This is a *platform* check, not an installation check — `resolve()`
    /// answers the latter. Apple container requires macOS; wslc requires
    /// Windows. Docker is the only one that exists on every platform.
    pub const fn supported_on_host(self) -> bool {
        match self {
            Self::Docker => true,
            Self::Wslc => cfg!(windows),
            Self::Apple => cfg!(target_os = "macos"),
        }
    }

    /// Why this host cannot run the runtime, when `supported_on_host` is false.
    pub const fn unsupported_reason(self) -> Option<&'static str> {
        match self {
            Self::Docker => None,
            Self::Wslc => {
                if cfg!(windows) {
                    None
                } else {
                    Some("wslc is the Windows Subsystem for Linux container CLI and only runs on Windows")
                }
            }
            Self::Apple => {
                if cfg!(target_os = "macos") {
                    None
                } else {
                    Some("Apple container requires macOS 26+ on Apple silicon")
                }
            }
        }
    }

    /// Resolve the runtime's binary, or `None` when it is not installed (or not
    /// supported on this host).
    pub fn resolve(self) -> Option<PathBuf> {
        if !self.supported_on_host() {
            return None;
        }
        for name in self.binary_candidates() {
            if let Some(path) = resolve_executable_in(name) {
                return Some(path);
            }
            if let Some(path) = resolve_in_dirs(name, &self.extra_search_dirs()) {
                return Some(path);
            }
        }
        None
    }

    /// Arguments that list containers. `all` includes non-running ones.
    ///
    /// Docker has no `json` value for `--format`, it takes a Go template that
    /// renders one JSON object per line; wslc and Apple take a literal `json`
    /// format. The parsers handle both shapes, so the difference stops here.
    pub const fn list_args(self, all: bool) -> &'static [&'static str] {
        match (self, all) {
            (Self::Docker, false) => &["ps", "--format", "{{json .}}"],
            (Self::Docker, true) => &["ps", "-a", "--format", "{{json .}}"],
            (Self::Wslc, false) => &["ps", "--format", "json"],
            (Self::Wslc, true) => &["ps", "-a", "--format", "json"],
            (Self::Apple, false) => &["list", "--format", "json"],
            (Self::Apple, true) => &["list", "--all", "--format", "json"],
        }
    }

    /// Arguments that open an interactive shell in a container.
    ///
    /// This is the human path (`container_terminal` launches it in a real
    /// terminal): `-i` keeps stdin open and `-t` allocates a TTY. It must not
    /// be used for captured execution — the TTY mangles the byte streams — see
    /// [`Self::exec_capture_args`] for that shape.
    pub fn exec_args(self, container_id: &str, shell: &str) -> Vec<String> {
        // All three runtimes accept the combined `-it` short form, and all three
        // put the container reference before the command.
        vec![
            "exec".into(),
            "-it".into(),
            container_id.into(),
            shell.into(),
        ]
    }

    /// Arguments that run *one command* inside a container with its output
    /// captured — the agent path, and the non-TTY counterpart of
    /// [`Self::exec_args`].
    ///
    /// No `-i` and no `-t`: there is no terminal to allocate, and a TTY would
    /// corrupt the captured streams with CRLF translation and prompt echoing.
    /// The shell is the last argument, so the command reaches it as
    /// `<shell> -c <command>` — the same shape a POSIX shell takes locally,
    /// which is what lets a container target share the local exec's invocation
    /// machinery unchanged. `docker exec` without a `-t` prints the container
    /// process's stdout and stderr as raw bytes and exits with its status.
    pub fn exec_capture_args(self, container_id: &str, shell: &str) -> Vec<String> {
        vec!["exec".into(), container_id.into(), shell.into()]
    }

    /// Arguments that force-remove a container.
    pub fn rm_args(self, container_id: &str) -> Vec<String> {
        vec!["rm".into(), "-f".into(), container_id.into()]
    }

    /// Arguments that ask a runtime's background service whether it is up, for
    /// the runtimes that have one. Apple's `container` is a CLI over an
    /// `apiserver` that must be started first (`container system start`); every
    /// command, `list` included, fails while it is down. Docker and wslc manage
    /// their daemon through their own desktop tooling and are not probed here.
    const fn service_status_args(self) -> Option<&'static [&'static str]> {
        match self {
            Self::Apple => Some(&["system", "status"]),
            Self::Docker | Self::Wslc => None,
        }
    }

    /// Arguments that start the service, for the runtimes that have one.
    const fn service_start_args(self) -> Option<&'static [&'static str]> {
        match self {
            Self::Apple => Some(&["system", "start"]),
            Self::Docker | Self::Wslc => None,
        }
    }

    /// The command a person types to start the service, for hints and the UI.
    pub fn service_start_command(self) -> Option<String> {
        let args = self.service_start_args()?;
        Some(format!(
            "{} {}",
            self.binary_candidates()[0],
            args.join(" ")
        ))
    }

    /// Whether the runtime's service answers: `Some(true)` up, `Some(false)`
    /// down or unresponsive, `None` when the runtime has no service to probe.
    pub fn service_running(self, binary: &Path) -> Option<bool> {
        let args = self.service_status_args()?;
        Some(run_to_exit(binary, args, SERVICE_PROBE_TIMEOUT).is_some_and(|ok| ok))
    }

    /// Start the runtime's service and wait until it answers. A no-op for
    /// runtimes without one, and for a service that is already up.
    pub fn start_service(self, binary: &Path) -> Result<()> {
        let Some(args) = self.service_start_args() else {
            return Ok(());
        };
        if self.service_running(binary) == Some(true) {
            return Ok(());
        }
        match run_to_exit(binary, args, SERVICE_START_TIMEOUT) {
            Some(true) => {}
            Some(false) => {
                return Err(anyhow!(
                    "`{}` failed; run it in a terminal to see its output or answer its prompts",
                    self.service_start_command().unwrap_or_default()
                ))
            }
            None => {
                return Err(anyhow!(
                    "`{}` did not finish within {}s",
                    self.service_start_command().unwrap_or_default(),
                    SERVICE_START_TIMEOUT.as_secs()
                ))
            }
        }
        if self.service_running(binary) == Some(true) {
            Ok(())
        } else {
            Err(anyhow!(
                "{} was started but its service is still not responding",
                self.display_name()
            ))
        }
    }

    /// Fail with an actionable message when the runtime is installed but its
    /// service is down, optionally starting it first (see [`AUTOSTART_ENV`]).
    pub fn ensure_ready(self, binary: &Path) -> Result<()> {
        self.ensure_ready_with(binary, autostart_enabled())
    }

    fn ensure_ready_with(self, binary: &Path, autostart: bool) -> Result<()> {
        if self.service_running(binary) != Some(false) {
            return Ok(());
        }
        let start = self.service_start_command().unwrap_or_default();
        if autostart {
            return self.start_service(binary).map_err(|e| {
                anyhow!(
                    "{} service was not running and could not be started: {e}",
                    self.display_name()
                )
            });
        }
        Err(anyhow!(
            "{} is installed but its service is not running; start it with `{start}` \
             (or `agent2ssh containers start --runtime {}`, or set {AUTOSTART_ENV}=1 to start it on demand)",
            self.display_name(),
            self.id()
        ))
    }

    /// Prefix of a discovered target's id, e.g. `docker_exec`.
    pub const fn exec_id_prefix(self) -> &'static str {
        match self {
            Self::Docker => "docker_exec",
            Self::Wslc => "wslc_exec",
            Self::Apple => "apple_exec",
        }
    }
}

fn autostart_enabled() -> bool {
    std::env::var(AUTOSTART_ENV)
        .map(|v| {
            matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

/// Run `binary args` with no stdin and discarded output. `Some(success)` when
/// it exited within `timeout`; `None` when it could not be spawned or had to be
/// killed for running over.
fn run_to_exit(binary: &Path, args: &[&str], timeout: Duration) -> Option<bool> {
    let mut child = Command::new(binary)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status.success()),
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(25));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

/// Resolve a bare binary name inside an explicit list of directories.
///
/// `resolve_executable_in` owns the `PATH` + shared-fallback search; this covers
/// the runtime-specific directories it does not know about. On Windows the
/// `.exe` suffix is appended when the caller did not supply an extension, to
/// match what `resolve_executable_in` does.
fn resolve_in_dirs(name: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    if dirs.is_empty() {
        return None;
    }
    let names: Vec<String> = if cfg!(windows) && Path::new(name).extension().is_none() {
        vec![name.to_string(), format!("{name}.exe")]
    } else {
        vec![name.to_string()]
    };
    for dir in dirs {
        for candidate in &names {
            let path = dir.join(candidate);
            if path.is_file() {
                return Some(path);
            }
        }
    }
    None
}

/// A runtime that is installed on this host, with the binary that was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRuntime {
    pub runtime: ContainerRuntime,
    pub binary: PathBuf,
}

impl ResolvedRuntime {
    /// Wire/UI description of this runtime.
    pub fn info(&self, selected: bool) -> ContainerRuntimeInfo {
        ContainerRuntimeInfo {
            runtime: self.runtime,
            display_name: self.runtime.display_name().to_string(),
            binary: Some(self.binary.display().to_string()),
            supported: true,
            available: true,
            selected,
            unsupported_reason: None,
            service_running: self.runtime.service_running(&self.binary),
            start_command: self.runtime.service_start_command(),
        }
    }
}

/// Serialized description of one runtime's state on this host. Shared by the
/// `containers runtime --json` CLI output and the `container_runtimes` Tauri
/// command, so the desktop UI and the e2e harness read the same decision.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContainerRuntimeInfo {
    pub runtime: ContainerRuntime,
    pub display_name: String,
    /// Absolute path of the CLI, when it was found.
    pub binary: Option<String>,
    /// Does this host's OS support the runtime at all?
    pub supported: bool,
    /// Is it supported *and* installed?
    pub available: bool,
    /// Would `select()` pick this runtime on this host?
    pub selected: bool,
    /// Platform-level explanation when `supported` is false.
    pub unsupported_reason: Option<String>,
    /// Is the runtime's background service up? `None` when the runtime is not
    /// installed or has no service to probe (Docker, wslc).
    #[serde(default)]
    pub service_running: Option<bool>,
    /// Command that starts the service, when it has one.
    #[serde(default)]
    pub start_command: Option<String>,
}

/// Every installed runtime, in auto-detection priority order.
pub fn available() -> Vec<ResolvedRuntime> {
    ContainerRuntime::ALL
        .into_iter()
        .filter_map(|runtime| {
            runtime
                .resolve()
                .map(|binary| ResolvedRuntime { runtime, binary })
        })
        .collect()
}

/// The id of the runtime `select(None)` would pick, if any.
fn auto_selected_runtime() -> Option<ContainerRuntime> {
    available().first().map(|resolved| resolved.runtime)
}

/// Resolve one runtime: explicit request → environment override → first
/// installed runtime.
///
/// The explicit and environment paths are strict on purpose: a caller that
/// named a runtime wants to know that it is missing or unsupported rather than
/// silently getting a different one. Only the auto path falls back.
pub fn select(explicit: Option<&str>) -> Result<ResolvedRuntime> {
    let requested = match explicit {
        Some(id) if !id.trim().is_empty() => Some((id.trim().to_string(), "requested runtime")),
        _ => std::env::var(RUNTIME_ENV)
            .ok()
            .filter(|id| !id.trim().is_empty())
            .map(|id| (id, "runtime pinned by environment")),
    };

    if let Some((id, origin)) = requested {
        let runtime = ContainerRuntime::from_id(&id).ok_or_else(|| {
            anyhow!(
                "unknown {origin} {id:?}; expected one of: {}",
                ContainerRuntime::ALL
                    .iter()
                    .map(|rt| rt.id())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;
        if let Some(reason) = runtime.unsupported_reason() {
            return Err(anyhow!("{} is not available here: {reason}", runtime.id()));
        }
        let binary = runtime.resolve().ok_or_else(|| {
            anyhow!(
                "{} is supported on this host but its binary ({}) was not found on PATH or in the known install locations",
                runtime.display_name(),
                runtime.binary_candidates().join(", ")
            )
        })?;
        return Ok(ResolvedRuntime { runtime, binary });
    }

    available().into_iter().next().ok_or_else(|| {
        anyhow!(
            "no container runtime found; install one of: {}",
            ContainerRuntime::ALL
                .iter()
                .filter(|rt| rt.supported_on_host())
                .map(|rt| format!("{} ({})", rt.display_name(), rt.binary_candidates()[0]))
                .collect::<Vec<_>>()
                .join(", ")
        )
    })
}

/// The full availability matrix for this host, including the runtimes that are
/// missing or unsupported, with the auto-selected one flagged.
pub fn infos() -> Vec<ContainerRuntimeInfo> {
    let selected = auto_selected_runtime();
    ContainerRuntime::ALL
        .into_iter()
        .map(|runtime| {
            let reason = runtime.unsupported_reason();
            let binary = runtime.resolve();
            ContainerRuntimeInfo {
                runtime,
                display_name: runtime.display_name().to_string(),
                available: binary.is_some(),
                supported: reason.is_none(),
                binary: binary.as_ref().map(|path| path.display().to_string()),
                selected: binary.is_some() && selected == Some(runtime),
                unsupported_reason: reason.map(|r| r.to_string()),
                service_running: binary.as_deref().and_then(|b| runtime.service_running(b)),
                start_command: runtime.service_start_command(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip_and_are_case_insensitive() {
        for runtime in ContainerRuntime::ALL {
            assert_eq!(ContainerRuntime::from_id(runtime.id()), Some(runtime));
        }
        assert_eq!(
            ContainerRuntime::from_id("  Apple "),
            Some(ContainerRuntime::Apple)
        );
        assert_eq!(ContainerRuntime::from_id("podman"), None);
    }

    #[test]
    fn runtime_serializes_as_its_id() {
        assert_eq!(
            serde_json::to_string(&ContainerRuntime::Wslc).unwrap(),
            "\"wslc\""
        );
        let apple: ContainerRuntime = serde_json::from_str("\"apple\"").unwrap();
        assert_eq!(apple, ContainerRuntime::Apple);
    }

    #[test]
    fn list_args_differ_by_runtime_and_all_flag() {
        assert_eq!(
            ContainerRuntime::Docker.list_args(false),
            &["ps", "--format", "{{json .}}"]
        );
        assert_eq!(
            ContainerRuntime::Docker.list_args(true),
            &["ps", "-a", "--format", "{{json .}}"]
        );
        assert_eq!(
            ContainerRuntime::Wslc.list_args(true),
            &["ps", "-a", "--format", "json"]
        );
        assert_eq!(
            ContainerRuntime::Apple.list_args(false),
            &["list", "--format", "json"]
        );
        assert_eq!(
            ContainerRuntime::Apple.list_args(true),
            &["list", "--all", "--format", "json"]
        );
    }

    #[test]
    fn exec_and_rm_args_share_one_shape() {
        for runtime in ContainerRuntime::ALL {
            assert_eq!(
                runtime.exec_args("abc123", CONTAINER_SHELL),
                vec!["exec", "-it", "abc123", "/bin/sh"]
            );
            assert_eq!(
                runtime.rm_args("abc123"),
                vec!["rm", "-f", "abc123"],
                "{} must accept the shared rm spelling",
                runtime.id()
            );
        }
    }

    #[test]
    fn capture_exec_args_never_allocate_a_tty() {
        for runtime in ContainerRuntime::ALL {
            let args = runtime.exec_capture_args("abc123", CONTAINER_SHELL);
            assert_eq!(
                args,
                vec!["exec", "abc123", "/bin/sh"],
                "{} capture argv",
                runtime.id()
            );
            // The whole point of the capture shape: no `-t`, no `-it`, no `-i`.
            // A TTY would CRLF-translate and echo prompts into captured bytes;
            // `-i` would bind stdin this runner does not forward.
            for flag in ["-t", "-it", "-i"] {
                assert!(
                    !args.iter().any(|arg| arg == flag),
                    "{} capture argv must not contain {flag}: {args:?}",
                    runtime.id()
                );
            }
        }
    }

    #[test]
    fn exec_id_prefixes_are_distinct() {
        let prefixes: Vec<&str> = ContainerRuntime::ALL
            .iter()
            .map(|rt| rt.exec_id_prefix())
            .collect();
        let mut unique = prefixes.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(prefixes.len(), unique.len());
        assert_eq!(ContainerRuntime::Docker.exec_id_prefix(), "docker_exec");
        assert_eq!(ContainerRuntime::Wslc.exec_id_prefix(), "wslc_exec");
        assert_eq!(ContainerRuntime::Apple.exec_id_prefix(), "apple_exec");
    }

    #[test]
    fn host_support_matches_the_platform() {
        assert!(ContainerRuntime::Docker.supported_on_host());
        assert_eq!(ContainerRuntime::Wslc.supported_on_host(), cfg!(windows));
        assert_eq!(
            ContainerRuntime::Apple.supported_on_host(),
            cfg!(target_os = "macos")
        );
        // An unsupported runtime always explains itself; a supported one never does.
        for runtime in ContainerRuntime::ALL {
            assert_eq!(
                runtime.unsupported_reason().is_none(),
                runtime.supported_on_host(),
                "{} has an inconsistent support/reason pair",
                runtime.id()
            );
        }
    }

    #[test]
    fn unsupported_runtime_never_resolves() {
        for runtime in ContainerRuntime::ALL {
            if !runtime.supported_on_host() {
                assert!(
                    runtime.resolve().is_none(),
                    "{} resolved on a host that does not support it",
                    runtime.id()
                );
            }
        }
    }

    #[test]
    fn select_rejects_unknown_ids_with_the_valid_list() {
        let error = select(Some("podman")).unwrap_err().to_string();
        assert!(error.contains("podman"), "{error}");
        for runtime in ContainerRuntime::ALL {
            assert!(
                error.contains(runtime.id()),
                "{error} should list {}",
                runtime.id()
            );
        }
    }

    #[test]
    fn select_reports_an_unsupported_platform_runtime() {
        for runtime in ContainerRuntime::ALL {
            if runtime.supported_on_host() {
                continue;
            }
            let error = select(Some(runtime.id())).unwrap_err().to_string();
            assert!(
                error.contains(runtime.id()),
                "error should name the runtime: {error}"
            );
            assert!(
                error.contains(runtime.unsupported_reason().unwrap()),
                "error should carry the platform reason: {error}"
            );
        }
    }

    #[test]
    fn select_accepts_an_installed_runtime() {
        // This test machine has at least one runtime available in CI? No — a
        // clean runner has none, so only assert the shape when one exists.
        let installed = available();
        if let Some(first) = installed.first() {
            let selected = select(Some(first.runtime.id())).expect("explicit select");
            assert_eq!(selected.runtime, first.runtime);
            assert_eq!(selected.binary, first.binary);
            // `select(None)` must agree with the priority order.
            let auto = select(None).expect("auto select");
            assert_eq!(auto.runtime, installed[0].runtime);
        }
    }

    #[test]
    fn infos_flag_exactly_one_selection_and_never_an_unavailable_one() {
        let infos = infos();
        assert_eq!(infos.len(), ContainerRuntime::ALL.len());
        let selected: Vec<&ContainerRuntimeInfo> =
            infos.iter().filter(|info| info.selected).collect();
        assert!(selected.len() <= 1);
        for info in &infos {
            if info.selected {
                assert!(info.available);
            }
            assert_eq!(info.available, info.binary.is_some());
            assert_eq!(info.supported, info.unsupported_reason.is_none());
        }
    }

    #[test]
    fn available_keeps_priority_order() {
        let ids: Vec<&str> = available().iter().map(|r| r.runtime.id()).collect();
        // Priority is ALL order, so the observed order must be that subsequence.
        let mut expected = Vec::new();
        for runtime in ContainerRuntime::ALL {
            if ids.contains(&runtime.id()) {
                expected.push(runtime.id());
            }
        }
        assert_eq!(ids, expected);
    }

    #[test]
    fn resolve_in_dirs_finds_the_probe_binary() {
        // A directory we know exists, with a file created by the test.
        let dir = std::env::temp_dir().join("a2s-rt-resolve-test");
        std::fs::create_dir_all(&dir).unwrap();
        let name = if cfg!(windows) {
            "a2s-probe-rt.exe"
        } else {
            "a2s-probe-rt"
        };
        std::fs::write(dir.join(name), b"stub").unwrap();
        let bare = name.trim_end_matches(".exe");
        let found = resolve_in_dirs(bare, std::slice::from_ref(&dir));
        assert!(found.is_some(), "expected to find {name} in {dir:?}");
        assert_eq!(resolve_in_dirs(bare, &[]), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A stand-in `container` whose service is "up" once `started` exists.
    #[cfg(unix)]
    fn fake_apple_cli(dir: &Path) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let started = dir.join("started");
        let script = dir.join("container");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\ncase \"$1 $2\" in\n  \"system status\") [ -e '{0}' ];;\n  \"system start\") touch '{0}';;\n  *) exit 2;;\nesac\n",
                started.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        script
    }

    #[cfg(unix)]
    #[test]
    fn a_stopped_service_is_reported_with_the_command_that_starts_it() {
        let dir = tempfile_dir("svc-stopped");
        let cli = fake_apple_cli(&dir);
        assert_eq!(ContainerRuntime::Apple.service_running(&cli), Some(false));
        let err = ContainerRuntime::Apple
            .ensure_ready_with(&cli, false)
            .unwrap_err()
            .to_string();
        assert!(err.contains("container system start"), "{err}");
        assert!(err.contains(AUTOSTART_ENV), "{err}");
        // Reporting must not start anything.
        assert!(!dir.join("started").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn autostart_and_the_explicit_start_bring_the_service_up() {
        let dir = tempfile_dir("svc-start");
        let cli = fake_apple_cli(&dir);
        ContainerRuntime::Apple
            .ensure_ready_with(&cli, true)
            .unwrap();
        assert_eq!(ContainerRuntime::Apple.service_running(&cli), Some(true));
        // Idempotent once it is up.
        ContainerRuntime::Apple.start_service(&cli).unwrap();
        ContainerRuntime::Apple
            .ensure_ready_with(&cli, false)
            .unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn runtimes_without_a_service_are_never_probed() {
        let nowhere = Path::new("/nonexistent/docker");
        for runtime in [ContainerRuntime::Docker, ContainerRuntime::Wslc] {
            assert_eq!(runtime.service_running(nowhere), None);
            assert!(runtime.service_start_command().is_none());
            runtime.ensure_ready_with(nowhere, false).unwrap();
        }
        assert_eq!(
            ContainerRuntime::Apple.service_start_command().as_deref(),
            Some("container system start")
        );
    }

    #[test]
    fn a_missing_binary_counts_as_a_service_that_is_not_running() {
        assert_eq!(
            ContainerRuntime::Apple.service_running(Path::new("/nonexistent/container")),
            Some(false)
        );
    }

    fn tempfile_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("agent2ssh-{label}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
