//! Local (on-this-machine) command execution — the non-SSH exec target.
//!
//! Every other command this crate runs goes over SSH. This module runs one in a
//! shell on the machine the app itself is running on. It exists because an agent
//! driving agent2ssh sometimes needs the local host, and the alternative was
//! worse: a separate shell MCP server would bypass the risk classifier, the
//! approval policy and the audit log, which is the opposite of what this app is
//! for. The execution lands here; the authorization for it lives in
//! `execution_control`, the same place every SSH exec is authorized.
//!
//! ## Why this is not just `std::process::Command`
//!
//! * **The process tree is actually reclaimed on timeout.** `embedded_ssh`
//!   documents the opposite limitation on its side: a pty-less exec channel
//!   cannot be reaped remotely, so its timeout returns promptly while the remote
//!   command keeps running, and stopping it for real needs a server-side
//!   `timeout(1)` wrapper. Locally that limitation does not exist. The child is
//!   put in its own process group and killed as a *group*, so a timed-out
//!   `sh -c 'sleep 30'` leaves nothing behind — see [`kill_process_tree`].
//! * **Both streams are drained concurrently.** A child that fills the stderr
//!   pipe while its stdout is still open blocks on its own write if only one
//!   stream is being read, so each stream gets its own reader thread.
//! * **What is kept is bounded identically to SSH**, through
//!   [`crate::exec_capture::BoundedCapture`], so `ExecResult::truncated` and
//!   `dropped_bytes` mean the same thing whichever target produced the result.
//!
//! ## Named environments, not just "a shell"
//!
//! The reason this target exists is not "run a command on this machine" but "run
//! a command in the terminal environment the agent asked for". Which environment
//! is asked for matters more than the command does: MSVC cannot compile without
//! its developer shell, a POSIX pipeline needs a POSIX shell's PATH, and the
//! difference between those two is entirely in *how the environment is
//! established*, not in the command string.
//!
//! So an environment is data — an id, the binary to spawn, and an optional
//! prelude that runs inside that same shell first — and every environment shares
//! one execution path, one timeout and process-tree story, and one audit record.
//! See [`LocalEnvironment`] for the set, addressed as `local:<id>`.
//!
//! A container is an environment under the same rule: `local:<runtime>:<id>`
//! (`local:docker:abc123`) runs one command inside that container through the
//! runtime CLI's non-TTY exec, so the capture contract, the authorization chain
//! and the audit record are the ones every other local target already has. See
//! [`container_plan`].
//!
//! ## WSL is an environment, not a shell
//!
//! `wsl.exe` is a launcher. Handing it a command through a shell flag is the
//! mistake this target exists to avoid, and it is not even valid: WSL takes
//! `-e/--exec`, `--shell-type`, `-d` and `--`, and has no `-c`. Picking a shell
//! here would defeat the point anyway — what a caller naming WSL wants is the
//! *distribution's* environment, and only WSL knows which shell that is. The plan
//! therefore asks WSL for its own login shell (`--shell-type login`) and hands it
//! nothing but the command, which is the one form that gets the distribution's
//! `PATH`, profile and toolchains.
//!
//! ## Recursion guard
//!
//! An agent can drive agent2ssh, which can run a local command, which can be
//! another agent driving agent2ssh. That is a fork bomb with extra steps, so
//! every local child is handed `AGENT2SSH_LOCAL_EXEC_DEPTH` set to its depth,
//! and a process already at [`MAX_LOCAL_EXEC_DEPTH`] refuses to run one.
//!
//! ## Known limitation (Windows)
//!
//! If a command exits while a *descendant* still holds the inherited output pipe
//! open — `cmd /c "start /b server"` — the readers cannot see EOF. The collector
//! bounds that wait and then kills the process group to release the pipe, which
//! works on Unix. On Windows `taskkill /T` is addressed by the parent's pid, so
//! once the parent has been reaped it can no longer find the tree; the call then
//! returns the output captured so far instead of hanging. Closing that gap needs
//! a Job Object, which has to be assigned at spawn time.
//!
//! Separately, `wsl.exe` writes its *own* diagnostics as UTF-16, so a failure
//! coming from WSL itself — an unknown distribution, say — reaches the caller as
//! lossy text, while output from the Linux command is ordinary UTF-8. Telling the
//! two apart is not reliable (a CJK message carries no NUL bytes to key off), so
//! this is documented rather than guessed at.

use anyhow::{anyhow, Result};
use std::{
    io::{ErrorKind, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use crate::{
    container_runtime::{ContainerRuntime, CONTAINER_SHELL},
    exec_capture::{BoundedCapture, CapturedExec},
    shell_list::{default_shell, list_shells, ShellFamily, ShellInfo},
};

/// Reserved target id, re-exported from `types` so the target that emits it and
/// the audit reader that classifies it share one definition.
pub use crate::types::LOCAL_TARGET_ID;

/// Environment variable holding how deep this process already sits in a local
/// exec chain. See the recursion-guard note above.
pub const DEPTH_ENV: &str = "AGENT2SSH_LOCAL_EXEC_DEPTH";

/// Depth at which a process refuses to start another local exec. One level is
/// enough: an agent that drives agent2ssh gets a shell, and a shell that tries
/// to drive agent2ssh again is the loop this exists to stop.
pub const MAX_LOCAL_EXEC_DEPTH: u32 = 1;

/// Overrides the shell for the *default* local environment. Accepts a path or a
/// bare name resolved from `PATH`; unset means "whatever the app would open as a
/// terminal", i.e. [`crate::shell_list::default_shell`].
///
/// Deliberately does not apply to a named environment (`local:gitbash`): naming
/// one is an explicit request, and substituting a different shell for it would
/// defeat the point of having named it.
pub const SHELL_ENV: &str = "AGENT2SSH_LOCAL_SHELL";

/// How often the wait loop checks whether the child has exited.
const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// How long a reader may keep waiting for EOF after the command itself is over
/// before the process group is killed to release the pipe. Only reached when a
/// descendant outlives the command, so the common path never pays it.
const STREAM_SETTLE_GRACE: Duration = Duration::from_millis(500);

/// Marks a local command that outlived its own deadline.
///
/// Deliberately separate from `embedded_ssh::ExecTimeout`: that one reports
/// itself as an SSH timeout, and a local command deserves wording that says so.
#[derive(Debug)]
pub(crate) struct LocalExecTimeout {
    pub(crate) timeout_secs: u64,
}

impl std::fmt::Display for LocalExecTimeout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "local command timed out after {}s", self.timeout_secs)
    }
}

impl std::error::Error for LocalExecTimeout {}

// ── Recursion guard ─────────────────────────────────────────────────────────

fn depth_from(value: Option<&str>) -> u32 {
    value
        .and_then(|raw| raw.trim().parse::<u32>().ok())
        .unwrap_or(0)
}

/// How deep this process already is in a local exec chain.
pub(crate) fn current_exec_depth() -> u32 {
    depth_from(std::env::var(DEPTH_ENV).ok().as_deref())
}

fn depth_exceeded(depth: u32) -> bool {
    depth >= MAX_LOCAL_EXEC_DEPTH
}

/// The depth to stamp on a child, or an error when this process is already too
/// deep to start one.
pub(crate) fn check_exec_depth() -> Result<u32> {
    let depth = current_exec_depth();
    if depth_exceeded(depth) {
        return Err(anyhow!(
            "local exec refused: this process is already {depth} level(s) deep in a \
             local exec chain (limit {MAX_LOCAL_EXEC_DEPTH})"
        ));
    }
    Ok(depth + 1)
}

// ── Shell selection ─────────────────────────────────────────────────────────

/// Lowercased file name with a trailing `.exe` removed, so `pwsh.exe` and
/// `pwsh` classify the same way.
fn file_stem_lower(path: &Path) -> String {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    name.strip_suffix(".exe").unwrap_or(&name).to_string()
}

/// Which family a shell belongs to, judged from its binary name. Only used for
/// shells the scanner did not report, so the common path takes the scanner's own
/// classification instead of re-deriving it.
fn family_for(path: &Path) -> ShellFamily {
    match file_stem_lower(path).as_str() {
        "pwsh" | "pwsh-preview" | "powershell" => ShellFamily::PowerShell,
        "cmd" => ShellFamily::Cmd,
        _ => ShellFamily::Posix,
    }
}

#[cfg(windows)]
fn platform_fallback() -> PathBuf {
    PathBuf::from("cmd.exe")
}

#[cfg(unix)]
fn platform_fallback() -> PathBuf {
    PathBuf::from("/bin/sh")
}

fn shell_override() -> Option<PathBuf> {
    let raw = std::env::var(SHELL_ENV).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(PathBuf::from(trimmed))
    }
}

/// Describe a requested shell, preferring the scanner's entry when it knows the
/// binary so the recorded path and family match what the terminal feature shows.
fn describe_shell(requested: &Path, installed: &[ShellInfo]) -> ShellInfo {
    let requested_stem = file_stem_lower(requested);
    if let Some(found) = installed.iter().find(|shell| {
        Path::new(&shell.path) == requested
            || file_stem_lower(Path::new(&shell.path)) == requested_stem
    }) {
        return found.clone();
    }
    ShellInfo {
        name: requested
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("shell")
            .to_string(),
        path: requested.to_string_lossy().into_owned(),
        family: family_for(requested),
    }
}

/// Resolve the shell for this exec.
///
/// Resolved per call rather than cached, on purpose: `AGENT2SSH_LOCAL_SHELL` has
/// to take effect without a restart, and that is also what lets the tests pin a
/// shell whose quoting rules they can assert on.
fn resolve_shell() -> ShellInfo {
    match shell_override() {
        Some(requested) => {
            let installed = list_shells();
            describe_shell(&requested, &installed)
        }
        None => default_shell().unwrap_or_else(|| describe_shell(&platform_fallback(), &[])),
    }
}

// ── Named terminal environments ─────────────────────────────────────────────

/// A local terminal environment an agent can name by host id.
///
/// Addressed as `local:<id>` on the wire — see [`crate::types::LOCAL_ENV_PREFIX`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalEnvironment {
    /// Whatever the app would open as a terminal. Addressed as plain `local`.
    Default,
    /// PowerShell, preferring PowerShell 7 and falling back to Windows
    /// PowerShell. Aliases: `pwsh`, `powershell`.
    Pwsh,
    /// Windows `cmd.exe`.
    Cmd,
    /// The MSYS shell shipped with Git for Windows, so `git`, `grep`, `sed` and
    /// POSIX path handling behave as they do in Git Bash. Aliases: `gitbash`,
    /// `bash`.
    GitBash,
    /// A Visual Studio developer shell: MSVC's `cl.exe`/`link.exe` on `PATH` and
    /// `INCLUDE`/`LIB`/`LIBPATH` populated, so a compile works without the user
    /// having opened a developer prompt first. Aliases: `vs`, `vsdev`.
    VsDevShell,
    /// A WSL distribution, entered through its own login shell so the caller gets
    /// the distribution's `PATH`, profile and toolchains. Addressed as `local:wsl`
    /// for the default distribution, or `local:wsl:<distribution>` for a named
    /// one. Windows only, in practice.
    Wsl,
}

impl LocalEnvironment {
    /// The ids a caller may name after `local:`, in the order an error message
    /// should list them.
    pub const SELECTABLE_IDS: [&'static str; 5] = ["pwsh", "cmd", "gitbash", "vs", "wsl"];

    /// The id that follows `local:` on the wire.
    pub const fn id(self) -> &'static str {
        match self {
            Self::Default => "local",
            Self::Pwsh => "pwsh",
            Self::Cmd => "cmd",
            Self::GitBash => "gitbash",
            Self::VsDevShell => "vs",
            Self::Wsl => "wsl",
        }
    }

    /// Parse the part after `local:`.
    ///
    /// Returns `None` for anything unrecognized instead of falling back to a
    /// shell: silently running a command in an environment the caller did not ask
    /// for is worse than refusing it, because the caller has no way to notice.
    pub fn from_id(id: &str) -> Option<Self> {
        match id.trim().to_ascii_lowercase().as_str() {
            "pwsh" | "powershell" => Some(Self::Pwsh),
            "cmd" => Some(Self::Cmd),
            "gitbash" | "bash" => Some(Self::GitBash),
            "vs" | "vsdev" => Some(Self::VsDevShell),
            "wsl" => Some(Self::Wsl),
            _ => None,
        }
    }

    /// How this environment is described in prose and in error messages.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Default => "the default shell",
            Self::Pwsh => "PowerShell",
            Self::Cmd => "cmd.exe",
            Self::GitBash => "Git Bash",
            Self::VsDevShell => "the Visual Studio developer shell",
            Self::Wsl => "a WSL distribution",
        }
    }
}

/// A local environment as requested, decoded from a `local[:...]` host id.
///
/// The distribution is carried beside the kind rather than inside the enum,
/// because it is data rather than a variant: every WSL distribution is the same
/// environment under a different name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalEnvironmentRequest {
    pub environment: LocalEnvironment,
    /// The WSL distribution to enter. `None` means WSL's default distribution,
    /// and it is only ever set for [`LocalEnvironment::Wsl`].
    pub distro: Option<String>,
}

impl LocalEnvironmentRequest {
    const fn plain(environment: LocalEnvironment) -> Self {
        Self {
            environment,
            distro: None,
        }
    }
}

/// The environment a local host id names, plus anything the id parameterizes.
///
/// The caller must already have established that `host` is a local id (see
/// [`crate::types::is_local_host`]); this only decodes what follows the colon.
pub fn environment_for_host(host: &str) -> Result<LocalEnvironmentRequest> {
    let Some(id) = host.strip_prefix(crate::types::LOCAL_ENV_PREFIX) else {
        return Ok(LocalEnvironmentRequest::plain(LocalEnvironment::Default));
    };

    let (kind, argument) = match id.split_once(':') {
        Some((kind, argument)) => (kind, Some(argument)),
        None => (id, None),
    };
    let environment = LocalEnvironment::from_id(kind).ok_or_else(|| {
        anyhow!(
            "unknown local environment '{kind}': expected one of {}, a WSL distribution as \
             'local:wsl:<distribution>', or a container as 'local:<runtime>:<container-id>' \
             (runtime: {})",
            LocalEnvironment::SELECTABLE_IDS.join(", "),
            ContainerRuntime::ALL
                .iter()
                .map(|runtime| runtime.id())
                .collect::<Vec<_>>()
                .join("/")
        )
    })?;

    match (environment, argument) {
        (LocalEnvironment::Wsl, None) => Ok(LocalEnvironmentRequest::plain(environment)),
        (LocalEnvironment::Wsl, Some("")) => Err(anyhow!(
            "'local:wsl:' names no distribution: use 'local:wsl' for the default one, or \
             'local:wsl:<distribution>' for a specific one"
        )),
        (LocalEnvironment::Wsl, Some(distro)) => Ok(LocalEnvironmentRequest {
            environment,
            distro: Some(distro.to_string()),
        }),
        // A suffix is only meaningful where something can consume it. Refusing
        // beats ignoring it, which would leave `local:cmd:ls` looking as though it
        // had asked for something.
        (_, Some(argument)) => Err(anyhow!(
            "'{}' takes no argument, but got '{argument}'",
            environment.id()
        )),
        // With no argument it is simply itself. `from_id` never yields `Default` —
        // that environment is addressed by the absence of a suffix, handled before
        // this match — so this arm exists only to keep the match total.
        (_, None) => Ok(LocalEnvironmentRequest::plain(environment)),
    }
}

/// A resolved environment: the binary to spawn, how it wants to be told to run a
/// command, and whatever has to happen inside that environment first.
#[derive(Debug, Clone)]
pub(crate) struct EnvironmentPlan {
    pub(crate) program: PathBuf,
    /// Arguments that come before any command, e.g. WSL's `-d <distribution>`
    /// or a container's `exec <id> <shell>`.
    pub(crate) prefix: Vec<String>,
    pub(crate) style: CommandStyle,
    /// Setup that runs in the same environment before the command, e.g. Visual
    /// Studio's developer-shell bootstrap. `None` for a plain shell.
    pub(crate) prelude: Option<String>,
    /// Whether the runner may forward a request's `stdin` payload. Containers
    /// run without `-i` — its stdin would be bound to a pipe this runner has
    /// no honest contract for — so a container request that carries stdin is
    /// refused by the caller rather than silently dropped.
    pub(crate) forwards_stdin: bool,
    /// Human label used in error messages.
    pub(crate) label: String,
}

impl EnvironmentPlan {
    fn plain(shell: &ShellInfo) -> Self {
        Self {
            program: PathBuf::from(&shell.path),
            prefix: Vec::new(),
            style: CommandStyle::for_family(shell.family),
            prelude: None,
            forwards_stdin: true,
            label: shell.name.clone(),
        }
    }
}

/// The first installed shell whose binary stem is one of `stems`, in the order
/// `stems` gives them — so callers express preference by ordering, not by
/// hoping the scanner did.
fn shell_with_stem(installed: &[ShellInfo], stems: &[&str]) -> Option<ShellInfo> {
    stems.iter().find_map(|stem| {
        installed
            .iter()
            .find(|shell| file_stem_lower(Path::new(&shell.path)) == *stem)
            .cloned()
    })
}

/// Git Bash, preferred over any other `bash.exe`.
///
/// `bash.exe` also ships with WSL and with standalone MSYS installs, and those
/// are different animals: Git Bash is the one whose PATH carries the Git
/// utilities a caller would be asking for by name.
fn git_bash(installed: &[ShellInfo]) -> Option<ShellInfo> {
    installed
        .iter()
        .find(|shell| {
            file_stem_lower(Path::new(&shell.path)) == "bash"
                && shell.path.to_ascii_lowercase().contains("git")
        })
        .cloned()
        .or_else(|| shell_with_stem(installed, &["bash"]))
}

/// The MSVC architecture name for the host this build runs on.
///
/// Used for both `-Arch` and `-HostArch`: a developer shell is for building
/// natively, and choosing a cross target is the caller's decision rather than a
/// sensible default.
fn msvc_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        "x86" => "x86",
        other => other,
    }
}

/// Locate Visual Studio's developer-shell bootstrap script.
///
/// `vswhere.exe` is the supported way to ask where Visual Studio lives; the
/// version-directory probe behind it exists because a BuildTools-only install is
/// not always reported by an older `vswhere`, and BuildTools is exactly the
/// install a machine that only needs to compile things tends to have.
#[cfg(windows)]
fn find_vs_dev_shell_script() -> Option<PathBuf> {
    let program_files_x86 = std::env::var("ProgramFiles(x86)").ok()?;
    let root = Path::new(&program_files_x86).join("Microsoft Visual Studio");

    if let Some(install) = vs_install_path_from_vswhere(&root) {
        if let Some(script) = vs_dev_shell_script_in(&install) {
            return Some(script);
        }
    }

    // 18 is the version ship name for the release after 17; probe both rather
    // than deriving one from the other.
    for version in ["18", "17"] {
        for edition in ["BuildTools", "Community", "Professional", "Enterprise"] {
            if let Some(script) = vs_dev_shell_script_in(&root.join(version).join(edition)) {
                return Some(script);
            }
        }
    }
    None
}

#[cfg(not(windows))]
fn find_vs_dev_shell_script() -> Option<PathBuf> {
    None
}

#[cfg(windows)]
fn vs_install_path_from_vswhere(vs_root: &Path) -> Option<PathBuf> {
    let vswhere = vs_root.join("Installer").join("vswhere.exe");
    if !vswhere.is_file() {
        return None;
    }
    let output = Command::new(&vswhere)
        .args([
            "-latest",
            "-products",
            "*",
            "-requires",
            "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
            "-property",
            "installationPath",
        ])
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        None
    } else {
        Some(PathBuf::from(path))
    }
}

/// Locate `VsDevShell.bat` under one Visual Studio install root.
///
/// Windows-only by construction — every caller is behind `#[cfg(windows)]` —
/// so the function is gated to match and does not compile (and cannot be
/// flagged dead) on other platforms.
#[cfg(windows)]
fn vs_dev_shell_script_in(install: &Path) -> Option<PathBuf> {
    let script = install
        .join("Common7")
        .join("Tools")
        .join("Launch-VsDevShell.ps1");
    script.is_file().then_some(script)
}

/// Build the plan for Visual Studio's developer shell.
///
/// MSVC needs `cl.exe`/`link.exe` on `PATH` plus `INCLUDE`/`LIB`/`LIBPATH`, and
/// Visual Studio's own bootstrap is the only supported way to get them. It sets
/// them as *process* environment variables, which is why calling it with `&`
/// inside the same PowerShell that then runs the command is enough — no
/// dot-sourcing needed — and `-SkipAutomaticLocation` keeps the caller's working
/// directory instead of moving to the install root.
fn vs_dev_shell_plan(installed: &[ShellInfo]) -> Result<EnvironmentPlan> {
    let script = find_vs_dev_shell_script().ok_or_else(|| {
        anyhow!(
            "Visual Studio developer shell not found: no Launch-VsDevShell.ps1 under a \
             Visual Studio installation (looked via vswhere, then the 18 and 17 \
             version directories)"
        )
    })?;
    // The bootstrap is a PowerShell script, so it needs a PowerShell to run in.
    let shell =
        shell_with_stem(installed, &["pwsh", "pwsh-preview", "powershell"]).ok_or_else(|| {
            anyhow!("the Visual Studio developer shell needs PowerShell, which is not on PATH")
        })?;

    let arch = msvc_arch();
    let escaped = script.display().to_string().replace('\'', "''");
    Ok(EnvironmentPlan {
        program: PathBuf::from(&shell.path),
        prefix: Vec::new(),
        style: CommandStyle::for_family(shell.family),
        prelude: Some(format!(
            "& '{escaped}' -SkipAutomaticLocation -Arch {arch} -HostArch {arch}"
        )),
        forwards_stdin: true,
        label: format!("{} + Visual Studio developer shell", shell.name),
    })
}

/// Build the plan for entering a WSL distribution.
///
/// `--shell-type login` rather than a `/bin/sh -c` this crate picks: the entire
/// value of this environment is that the caller gets the *distribution's* own
/// environment — its `PATH`, its profile, its toolchains — and only WSL knows
/// which shell a given distribution uses or how to start it as a login shell.
///
/// A named distribution is deliberately not verified here. `wsl.exe` reports an
/// unknown one itself, with a better message than this crate could invent, and
/// checking the installed list would mean parsing the UTF-16 output of
/// `wsl.exe -l`.
fn wsl_plan(distro: Option<&str>, installed: &[ShellInfo]) -> Result<EnvironmentPlan> {
    let wsl = shell_with_stem(installed, &["wsl"]).ok_or_else(|| {
        anyhow!("WSL is not available: wsl.exe is not on PATH (install one with 'wsl --install')")
    })?;

    Ok(EnvironmentPlan {
        program: PathBuf::from(&wsl.path),
        prefix: match distro {
            Some(distro) => vec!["-d".to_string(), distro.to_string()],
            None => Vec::new(),
        },
        style: CommandStyle::WslLoginShell,
        prelude: None,
        forwards_stdin: true,
        label: match distro {
            Some(distro) => format!("WSL distribution '{distro}'"),
            None => "the default WSL distribution".to_string(),
        },
    })
}

/// Resolve a requested environment into something spawnable.
///
/// Resolved per call rather than cached: the shell scan has to see `PATH`
/// changes, and a Visual Studio installation can appear or be removed between
/// runs.
pub(crate) fn resolve_environment(request: LocalEnvironmentRequest) -> Result<EnvironmentPlan> {
    let LocalEnvironmentRequest {
        environment,
        distro,
    } = request;

    // The default shell has its own resolution path — the explicit override, then
    // the terminal feature's own preference — so it does not need the scan the
    // named environments do.
    if let LocalEnvironment::Default = environment {
        let shell = resolve_shell();
        // `wsl.exe` is not a shell. It launches a distribution, and a command
        // handed to it through some shell flag this crate chose would never reach
        // that distribution's environment — which is the one thing the caller
        // wanted. Point at the environment that does this properly instead of
        // running something that cannot work.
        if file_stem_lower(Path::new(&shell.path)) == "wsl" {
            return Err(anyhow!(
                "'{}' is not a shell: use 'local:wsl' (or 'local:wsl:<distribution>') so the \
                 command runs in a distribution's own login shell",
                shell.path
            ));
        }
        return Ok(EnvironmentPlan::plain(&shell));
    }

    let installed = list_shells();
    match environment {
        LocalEnvironment::Pwsh => {
            shell_with_stem(&installed, &["pwsh", "pwsh-preview", "powershell"])
                .map(|shell| EnvironmentPlan::plain(&shell))
                .ok_or_else(|| anyhow!("no PowerShell found on PATH"))
        }
        LocalEnvironment::Cmd => shell_with_stem(&installed, &["cmd"])
            .map(|shell| EnvironmentPlan::plain(&shell))
            .ok_or_else(|| anyhow!("cmd.exe not found on PATH")),
        LocalEnvironment::GitBash => git_bash(&installed)
            .map(|shell| EnvironmentPlan::plain(&shell))
            .ok_or_else(|| anyhow!("Git Bash not found: no bash.exe on PATH")),
        LocalEnvironment::VsDevShell => vs_dev_shell_plan(&installed),
        LocalEnvironment::Wsl => wsl_plan(distro.as_deref(), &installed),
        // Returned before the scan. Kept so the match stays total and a future
        // variant cannot silently inherit another environment's behaviour.
        LocalEnvironment::Default => Ok(EnvironmentPlan::plain(&resolve_shell())),
    }
}

// ── Container capture exec ──────────────────────────────────────────────────

/// Decode a `local:<runtime>:<container-id>` host id into the runtime it names.
///
/// `Some` only when the first segment after `local:` is a *runtime* id and a
/// container id follows; everything else — `local`, `local:pwsh`,
/// `local:wsl:Ubuntu` — belongs to the shell environments. A runtime id that
/// is not a runtime (`local:pwsx:abc`) decodes to `None` here and is refused by
/// the environment path with the full list of what *is* selectable.
fn container_target_for_host(host: &str) -> Option<(ContainerRuntime, &str)> {
    let rest = host.strip_prefix(crate::types::LOCAL_ENV_PREFIX)?;
    let (kind, container_id) = rest.split_once(':')?;
    let runtime = ContainerRuntime::from_id(kind)?;
    Some((runtime, container_id))
}

/// Longest accepted container name or id, and the characters one may contain.
///
/// Every runtime here restricts names to `[A-Za-z0-9][A-Za-z0-9_.-]*` (Docker's
/// own rule; Apple ids are UUIDs and wslc follows Docker), so the check costs
/// nothing real while making a container reference unrepresentable as anything
/// but a container reference.
const MAX_CONTAINER_ID_LEN: usize = 128;

fn validate_container_id(container_id: &str) -> Result<()> {
    if container_id.is_empty()
        || container_id.len() > MAX_CONTAINER_ID_LEN
        || !container_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        return Err(anyhow!(
            "invalid container id '{container_id}': expected at most {MAX_CONTAINER_ID_LEN} \
             characters from [A-Za-z0-9._-]"
        ));
    }
    Ok(())
}

/// Build the plan for running one command inside a container.
///
/// The container CLI is spawned on this machine — `docker exec <id> /bin/sh -c
/// <command>` in the non-TTY capture shape from
/// [`ContainerRuntime::exec_capture_args`] — so a container reuses this
/// module's spawn, timeout, process-tree and bounded-capture machinery
/// unchanged, and the only new logic is resolving which CLI and argv to spawn.
///
/// There is no prelude: a container's environment is whatever the image ships,
/// and inventing setup for it would be a second, unaudited way to establish an
/// environment.
fn container_plan(runtime: ContainerRuntime, container_id: &str) -> Result<EnvironmentPlan> {
    validate_container_id(container_id)?;
    let binary = runtime.resolve().ok_or_else(|| {
        anyhow!(
            "{} is not installed, so there is nothing to exec into: its CLI ({}) was not found \
             on PATH or in the known install locations",
            runtime.display_name(),
            runtime.binary_candidates().join(", ")
        )
    })?;
    Ok(EnvironmentPlan {
        program: binary,
        prefix: runtime.exec_capture_args(container_id, CONTAINER_SHELL),
        style: CommandStyle::PosixDashC,
        prelude: None,
        forwards_stdin: false,
        label: format!("{} container {container_id}", runtime.display_name()),
    })
}

/// Resolve whatever a local host id names — a shell environment or a container
/// — into something spawnable.
///
/// This is the one resolution entry callers should use: it keeps the refusal
/// rules in one place (an unknown name is refused and listed, never defaulted)
/// whether the name selected a shell or a container.
pub(crate) fn resolve_plan_for_host(host: &str) -> Result<EnvironmentPlan> {
    match container_target_for_host(host) {
        Some((runtime, container_id)) => container_plan(runtime, container_id),
        None => environment_for_host(host).and_then(resolve_environment),
    }
}

/// How a binary wants to be told to run one command string.
///
/// Separate from [`ShellFamily`] because they answer different questions: a
/// family says what a shell *is*, this says how to invoke the thing being
/// spawned. WSL is what forces the distinction — `wsl.exe` is not a shell, and
/// the command has to reach the distribution's own login shell instead of a `-c`
/// flag this crate picked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandStyle {
    /// `-c <command>` — a POSIX shell.
    PosixDashC,
    /// `/D /C <command>`, appended verbatim: cmd.exe does not parse its command
    /// line by the MSVCRT rules `Command` quotes for.
    CmdSlashC,
    /// `-Command <command>`, behind the flags that keep a user profile out of it.
    PowerShellCommand,
    /// `--shell-type login <command>` — WSL runs it in the distribution's login
    /// shell. Preferred over naming a shell here because only WSL knows which
    /// shell a distribution uses and how to start it as a login one, and getting
    /// that environment's own profile is the entire point of this target.
    WslLoginShell,
}

impl CommandStyle {
    /// The flags between the environment's prefix and the command, and whether
    /// the command must be appended verbatim rather than as a quoted argument.
    fn invocation(self, command: &str) -> (Vec<String>, bool) {
        match self {
            Self::PosixDashC => (vec!["-c".into(), command.into()], false),
            // `/D` skips the AutoRun script, so a machine-wide `cmd /AutoRun`
            // hook cannot inject into a command this app authorized.
            Self::CmdSlashC => (vec!["/D".into(), "/C".into()], true),
            Self::PowerShellCommand => (
                vec![
                    "-NoLogo".into(),
                    "-NoProfile".into(),
                    "-NonInteractive".into(),
                    "-Command".into(),
                    command.into(),
                ],
                false,
            ),
            Self::WslLoginShell => (
                vec!["--shell-type".into(), "login".into(), command.into()],
                false,
            ),
        }
    }

    /// How a setup step is chained in front of the command.
    fn prelude_separator(self) -> &'static str {
        match self {
            // `;` is not a separator in cmd, so a cmd setup has to be chained.
            Self::CmdSlashC => " && ",
            Self::PosixDashC | Self::PowerShellCommand | Self::WslLoginShell => "; ",
        }
    }

    fn for_family(family: ShellFamily) -> Self {
        match family {
            ShellFamily::Posix => Self::PosixDashC,
            ShellFamily::Cmd => Self::CmdSlashC,
            ShellFamily::PowerShell => Self::PowerShellCommand,
        }
    }
}

/// Join an environment's setup with the command so both run in one shell.
///
/// The separator keeps the caller's command running even if the setup only
/// warned — the setup itself is resolved and checked before we get here, so an
/// environment that cannot be built is a refusal rather than a silently
/// unapplied shell.
fn with_prelude(plan: &EnvironmentPlan, command: &str) -> String {
    match &plan.prelude {
        None => command.to_string(),
        Some(prelude) => format!("{prelude}{}{command}", plan.style.prelude_separator()),
    }
}

fn build_command(plan: &EnvironmentPlan, command: &str) -> Command {
    let command = with_prelude(plan, command);
    let (args, raw_command) = plan.style.invocation(&command);
    let mut process = Command::new(&plan.program);
    // WSL's `-d <distribution>` and anything like it lands here, ahead of the
    // style's own flags.
    process.args(&plan.prefix);
    process.args(&args);
    if raw_command {
        // cmd.exe does not parse its command line by the MSVCRT rules `Command`
        // applies when it quotes an argument, so an embedded quote — `echo "hi
        // there"` — would reach cmd as `\"` and be echoed literally. Handing cmd
        // the command verbatim is what keeps such commands byte-exact.
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            process.raw_arg(&command);
        }
        #[cfg(not(windows))]
        process.arg(&command);
    }
    configure_process_group(&mut process);
    process
}

/// Put the child in its own process group so [`kill_process_tree`] can signal
/// the whole tree at once instead of just the shell.
///
/// Shared with `resident_session`, whose long-lived shells need the same
/// reclamation story.
#[cfg(unix)]
pub(crate) fn configure_process_group(process: &mut Command) {
    use std::os::unix::process::CommandExt;
    // `0` means "new group whose id is this child's pid".
    process.process_group(0);
}

#[cfg(windows)]
pub(crate) fn configure_process_group(process: &mut Command) {
    use std::os::windows::process::CommandExt;
    /// Children of this process must not inherit its console group, so a tree
    /// kill does not reach back into the app.
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    /// Keeps a console window from flashing up when the desktop app runs a
    /// command; stdio is piped in every case, so nothing is lost by it.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    process.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
}

/// Kill the child and everything it spawned.
///
/// SIGKILL rather than SIGTERM on Unix: the command has already blown its own
/// deadline, so asking politely would only add a second deadline to bound, and
/// the SSH path never had the choice at all.
///
/// Safe to call once the child has already been reaped — the process group
/// outlives its leader, so the group kill still reaches survivors.
///
/// Shared with `resident_session`: an exec that outlives its deadline there
/// kills the whole session, which is the same process-group problem.
#[cfg(unix)]
pub(crate) fn kill_process_tree(child: &mut Child) {
    let pid = child.id() as i32;
    // Negative pid targets the process group set up by `configure_process_group`.
    unsafe {
        libc::kill(-pid, libc::SIGKILL);
    }
    let _ = child.kill();
}

#[cfg(windows)]
pub(crate) fn kill_process_tree(child: &mut Child) {
    // `/T` walks the child tree, `/F` forces it. A Job Object would be the more
    // precise tool but has to be assigned at spawn time; this is the same
    // pid-based form the container path already uses.
    let _ = Command::new("taskkill")
        .args(["/T", "/F", "/PID", &child.id().to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ = child.kill();
}

// ── Execution ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
enum ReaderStream {
    Stdout,
    Stderr,
}

/// Drain one stream to EOF on its own thread and hand the bounded result back.
///
/// The thread is deliberately detached — there is nothing useful to do with its
/// handle, because a reader that never sees EOF because a descendant is holding
/// the pipe open is released by the caller killing the process group, not by
/// anything this thread can decide on its own.
fn spawn_reader<R: Read + Send + 'static>(
    mut source: R,
    max_bytes: usize,
    stream: ReaderStream,
    sender: mpsc::Sender<(ReaderStream, BoundedCapture)>,
) {
    thread::spawn(move || {
        let mut capture = BoundedCapture::new(max_bytes);
        let mut buffer = [0u8; 8192];
        loop {
            match source.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => capture.push(&buffer[..read]),
                Err(error) if error.kind() == ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
        let _ = sender.send((stream, capture));
    });
}

/// Collect both streams, killing the process group if a reader is stuck holding
/// a pipe that a descendant inherited.
fn collect_streams(
    receiver: mpsc::Receiver<(ReaderStream, BoundedCapture)>,
    reader_count: usize,
    max_bytes: usize,
    child: &mut Child,
) -> (BoundedCapture, BoundedCapture) {
    let mut stdout = BoundedCapture::new(max_bytes);
    let mut stderr = BoundedCapture::new(max_bytes);
    let mut received = 0;
    let mut released_pipes = false;

    while received < reader_count {
        match receiver.recv_timeout(STREAM_SETTLE_GRACE) {
            Ok((ReaderStream::Stdout, capture)) => {
                stdout = capture;
                received += 1;
            }
            Ok((ReaderStream::Stderr, capture)) => {
                stderr = capture;
                received += 1;
            }
            Err(mpsc::RecvTimeoutError::Timeout) if !released_pipes => {
                // The command is over but its output is still being held open by
                // a descendant (`sh -c 'server &'`). The command's output ended
                // when the command ended, so release the pipe instead of waiting
                // on a process the caller never asked to outlive the exec.
                released_pipes = true;
                kill_process_tree(child);
            }
            // Either a second grace elapsed or both senders are gone: report
            // what was captured rather than block the caller indefinitely.
            Err(mpsc::RecvTimeoutError::Timeout) => break,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    (stdout, stderr)
}

/// Run `command` in a resolved local environment and capture both output
/// streams.
///
/// The plan is passed in rather than resolved here so that an environment which
/// cannot be built — no Git Bash installed, no Visual Studio found — is reported
/// where the caller can audit it, instead of surfacing as an I/O error from
/// inside this function.
///
/// `timeout_secs` bounds the command itself, starting once it has been spawned,
/// and `max_bytes` bounds each stream independently — the same contract
/// `embedded_ssh::run_command_capture` offers, so callers cannot tell the two
/// apart by their result shape.
pub(crate) fn run_local_command_capture(
    plan: &EnvironmentPlan,
    command: &str,
    stdin: Option<&[u8]>,
    timeout_secs: u64,
    max_bytes: usize,
) -> Result<CapturedExec> {
    let child_depth = check_exec_depth()?;

    let mut process = build_command(plan, command);
    process
        .env(DEPTH_ENV, child_depth.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = process.spawn().map_err(|error| {
        anyhow!(
            "failed to start {} for '{}': {error}",
            plan.label,
            plan.program.display()
        )
    })?;

    let (sender, receiver) = mpsc::channel();
    let mut readers = 0;
    if let Some(stream) = child.stdout.take() {
        spawn_reader(stream, max_bytes, ReaderStream::Stdout, sender.clone());
        readers += 1;
    }
    if let Some(stream) = child.stderr.take() {
        spawn_reader(stream, max_bytes, ReaderStream::Stderr, sender.clone());
        readers += 1;
    }
    // Dropping the last sender is what lets the receiver report `Disconnected`
    // once both readers are done.
    drop(sender);

    if let Some(mut sink) = child.stdin.take() {
        let payload = stdin.map(|data| data.to_vec());
        // Detached on purpose: joining would block the result on a child that
        // never reads its stdin, and the thread ends by itself when the pipe
        // breaks on child exit.
        thread::spawn(move || {
            if let Some(payload) = payload {
                let _ = sink.write_all(&payload);
            }
            let _ = sink.flush();
            // Dropping `sink` closes the pipe, which is how the child sees EOF.
        });
    }

    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            timed_out = true;
            kill_process_tree(&mut child);
            break child.wait()?;
        }
        thread::sleep(POLL_INTERVAL);
    };

    let (stdout, stderr) = collect_streams(receiver, readers, max_bytes, &mut child);

    if timed_out {
        return Err(LocalExecTimeout { timeout_secs }.into());
    }

    Ok(CapturedExec {
        // A signalling death has no exit code on Unix, which is exactly what
        // `Option<i32>` is for — the SSH path reports `None` in the same case.
        exit_code: status.code(),
        truncated: stdout.truncated() || stderr.truncated(),
        dropped_bytes: stdout.dropped_bytes() + stderr.dropped_bytes(),
        stdout: stdout.into_bytes(),
        stderr: stderr.into_bytes(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn posix_shells_take_the_command_after_dash_c() {
        let (args, raw) = CommandStyle::PosixDashC.invocation("ls -la");
        assert_eq!(args, vec!["-c".to_string(), "ls -la".to_string()]);
        assert!(!raw);
    }

    #[test]
    fn powershell_suppresses_profile_and_logo_before_the_command() {
        let (args, raw) = CommandStyle::PowerShellCommand.invocation("Get-Date");
        assert_eq!(
            args,
            vec![
                "-NoLogo".to_string(),
                "-NoProfile".to_string(),
                "-NonInteractive".to_string(),
                "-Command".to_string(),
                "Get-Date".to_string(),
            ]
        );
        assert!(!raw);
    }

    #[test]
    fn cmd_takes_the_command_raw_after_slash_c() {
        // The command must not travel as an argument: cmd does not understand
        // MSVCRT quote escaping, so `echo "hi"` would arrive mangled.
        let (args, raw) = CommandStyle::CmdSlashC.invocation("echo \"hi there\"");
        assert_eq!(args, vec!["/D".to_string(), "/C".to_string()]);
        assert!(raw);
    }

    #[test]
    fn a_shell_family_still_maps_to_the_style_that_drives_it() {
        // The family answers "what is this shell", the style answers "how do I
        // invoke it". They agree for real shells, and diverge only for WSL, which
        // has no family of its own.
        assert_eq!(
            CommandStyle::for_family(ShellFamily::Posix),
            CommandStyle::PosixDashC
        );
        assert_eq!(
            CommandStyle::for_family(ShellFamily::Cmd),
            CommandStyle::CmdSlashC
        );
        assert_eq!(
            CommandStyle::for_family(ShellFamily::PowerShell),
            CommandStyle::PowerShellCommand
        );
    }

    #[test]
    fn binary_names_classify_across_platform_spellings() {
        // Forward slashes on purpose: `Path::file_name` treats them as
        // separators on Windows too, so these assertions hold on both platforms.
        assert_eq!(family_for(Path::new("pwsh.exe")), ShellFamily::PowerShell);
        assert_eq!(family_for(Path::new("/bin/sh")), ShellFamily::Posix);
        assert_eq!(
            family_for(Path::new("C:/Windows/System32/cmd.exe")),
            ShellFamily::Cmd
        );
        assert_eq!(family_for(Path::new("bash")), ShellFamily::Posix);
    }

    #[test]
    fn unknown_shells_are_classified_by_name_not_rejected() {
        let described = describe_shell(Path::new("/opt/custom/zsh"), &[]);
        assert_eq!(described.family, ShellFamily::Posix);
        assert_eq!(described.name, "zsh");
        assert_eq!(described.path, "/opt/custom/zsh");
    }

    #[test]
    fn known_shells_keep_the_scanners_own_description() {
        let installed = vec![ShellInfo {
            name: "PowerShell 7".into(),
            path: "/usr/bin/pwsh".into(),
            family: ShellFamily::PowerShell,
        }];
        let described = describe_shell(Path::new("pwsh"), &installed);
        assert_eq!(described.name, "PowerShell 7");
        assert_eq!(described.family, ShellFamily::PowerShell);
    }

    #[test]
    fn depth_parsing_tolerates_junk_and_absence() {
        assert_eq!(depth_from(None), 0);
        assert_eq!(depth_from(Some("")), 0);
        assert_eq!(depth_from(Some("  ")), 0);
        assert_eq!(depth_from(Some("not-a-number")), 0);
        assert_eq!(depth_from(Some(" 2 ")), 2);
    }

    #[test]
    fn only_the_first_level_of_nesting_is_allowed() {
        assert!(!depth_exceeded(0));
        assert!(depth_exceeded(MAX_LOCAL_EXEC_DEPTH));
        assert!(depth_exceeded(MAX_LOCAL_EXEC_DEPTH + 5));
    }

    #[test]
    fn the_id_this_module_emits_classifies_as_local() {
        // The audit reader classifies a host string; this module is what writes
        // that string. If the two ever drifted, every local exec would be filed
        // as an SSH one.
        assert_eq!(
            crate::types::ExecTarget::from_host(LOCAL_TARGET_ID),
            crate::types::ExecTarget::Local
        );
    }

    #[test]
    fn environment_ids_round_trip_with_their_aliases() {
        for id in LocalEnvironment::SELECTABLE_IDS {
            let environment = LocalEnvironment::from_id(id).expect("a selectable id must parse");
            assert_eq!(environment.id(), id, "id and parsing must agree");
        }
        assert_eq!(
            LocalEnvironment::from_id("powershell"),
            Some(LocalEnvironment::Pwsh)
        );
        assert_eq!(
            LocalEnvironment::from_id("bash"),
            Some(LocalEnvironment::GitBash)
        );
        assert_eq!(
            LocalEnvironment::from_id("vsdev"),
            Some(LocalEnvironment::VsDevShell)
        );
        assert_eq!(
            LocalEnvironment::from_id("VS"),
            Some(LocalEnvironment::VsDevShell)
        );
    }

    #[test]
    fn an_unknown_environment_is_refused_and_lists_the_known_ones() {
        // Refused rather than defaulted to a shell: a caller that asked for an
        // environment it cannot have must not be handed a different one silently.
        assert_eq!(LocalEnvironment::from_id("nope"), None);
        assert_eq!(LocalEnvironment::from_id(""), None);

        let error = environment_for_host("local:nope").expect_err("must be refused");
        let message = error.to_string();
        assert!(message.contains("nope"), "message was {message}");
        for id in LocalEnvironment::SELECTABLE_IDS {
            assert!(message.contains(id), "message omits '{id}': {message}");
        }
    }

    #[test]
    fn a_plain_local_host_is_the_default_environment() {
        assert_eq!(
            environment_for_host(LOCAL_TARGET_ID).unwrap(),
            LocalEnvironmentRequest::plain(LocalEnvironment::Default)
        );
        assert_eq!(
            environment_for_host("local:gitbash").unwrap(),
            LocalEnvironmentRequest::plain(LocalEnvironment::GitBash)
        );
    }

    #[test]
    fn a_distribution_is_only_accepted_where_something_can_use_it() {
        assert_eq!(
            environment_for_host("local:wsl").unwrap(),
            LocalEnvironmentRequest::plain(LocalEnvironment::Wsl)
        );
        assert_eq!(
            environment_for_host("local:wsl:Ubuntu-22.04").unwrap(),
            LocalEnvironmentRequest {
                environment: LocalEnvironment::Wsl,
                distro: Some("Ubuntu-22.04".to_string()),
            }
        );

        // An empty distribution names nothing, and a trailing argument on an
        // environment with no use for one has to be refused rather than ignored —
        // otherwise `local:cmd:ls` looks as though it asked for something.
        let empty = environment_for_host("local:wsl:").expect_err("must be refused");
        assert!(
            empty.to_string().contains("names no distribution"),
            "{empty}"
        );
        let stray = environment_for_host("local:cmd:ls").expect_err("must be refused");
        assert!(stray.to_string().contains("takes no argument"), "{stray}");
        assert!(stray.to_string().contains("cmd"), "{stray}");
    }

    #[test]
    fn the_wsl_plan_enters_the_distribution_through_its_login_shell() {
        let installed = vec![ShellInfo {
            name: "WSL".into(),
            path: "C:/Windows/System32/wsl.exe".into(),
            family: ShellFamily::Posix,
        }];

        let default = wsl_plan(None, &installed).expect("a plan");
        assert_eq!(default.prefix, Vec::<String>::new());
        // `--shell-type login` is what makes the distribution's own profile apply.
        // A `-c` flag this crate chose could not reach that environment at all,
        // which is exactly the bug this environment exists to avoid.
        let (args, raw) = default.style.invocation("echo $PATH");
        assert_eq!(args, vec!["--shell-type", "login", "echo $PATH"]);
        assert!(!raw);

        let named = wsl_plan(Some("Ubuntu-22.04"), &installed).expect("a plan");
        assert_eq!(named.prefix, vec!["-d", "Ubuntu-22.04"]);
        assert!(named.label.contains("Ubuntu-22.04"));
    }

    #[test]
    fn a_prelude_is_joined_with_the_separator_its_shell_understands() {
        let plan = |style, prelude: Option<&str>| EnvironmentPlan {
            program: PathBuf::from("shell"),
            prefix: Vec::new(),
            style,
            prelude: prelude.map(str::to_string),
            forwards_stdin: true,
            label: "test".into(),
        };

        assert_eq!(
            with_prelude(&plan(CommandStyle::PowerShellCommand, None), "echo hi"),
            "echo hi"
        );
        // `;` for PowerShell, which is what the Visual Studio bootstrap needs so
        // the command still runs if the bootstrap only warned.
        assert_eq!(
            with_prelude(
                &plan(CommandStyle::PowerShellCommand, Some("& 'vs.ps1'")),
                "cl /?"
            ),
            "& 'vs.ps1'; cl /?"
        );
        // `;` is not a separator in cmd, so a cmd prelude has to be chained.
        assert_eq!(
            with_prelude(
                &plan(CommandStyle::CmdSlashC, Some("call vcvars64.bat")),
                "cl /?"
            ),
            "call vcvars64.bat && cl /?"
        );
        assert_eq!(
            with_prelude(&plan(CommandStyle::PosixDashC, Some(". ./env.sh")), "make"),
            ". ./env.sh; make"
        );
    }

    #[test]
    fn msvc_arch_maps_the_rust_names_to_the_ones_visual_studio_accepts() {
        // Getting this wrong means `-Arch` is rejected by the bootstrap script,
        // which surfaces as a developer shell that silently is not one.
        match std::env::consts::ARCH {
            "x86_64" => assert_eq!(msvc_arch(), "x64"),
            "aarch64" => assert_eq!(msvc_arch(), "arm64"),
            "x86" => assert_eq!(msvc_arch(), "x86"),
            other => assert_eq!(msvc_arch(), other),
        }
    }

    #[cfg(windows)]
    #[test]
    fn a_discovered_visual_studio_bootstrap_is_a_real_script() {
        // Tolerates a machine with no Visual Studio. When one *is* found it has
        // to be the real file: the prelude is built from this path unquoted-by-
        // accident, so a wrong guess would fail at run time, not here.
        if let Some(script) = find_vs_dev_shell_script() {
            assert!(script.is_file(), "{} is not a file", script.display());
            assert_eq!(
                script.file_name().and_then(|name| name.to_str()),
                Some("Launch-VsDevShell.ps1")
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn only_the_developer_shell_carries_a_prelude() {
        let installed = list_shells();
        if let Ok(plan) = vs_dev_shell_plan(&installed) {
            let prelude = plan
                .prelude
                .expect("the developer shell must have a prelude");
            assert!(
                prelude.contains("-SkipAutomaticLocation"),
                "the prelude must keep the caller's working directory: {prelude}"
            );
            assert!(
                prelude.contains("-Arch") && prelude.contains("-HostArch"),
                "the prelude must pin the architecture: {prelude}"
            );
        }
        // A plain environment must never carry one.
        for environment in [LocalEnvironment::Pwsh, LocalEnvironment::Cmd] {
            if let Ok(plan) = resolve_environment(LocalEnvironmentRequest::plain(environment)) {
                assert!(
                    plan.prelude.is_none(),
                    "{environment:?} should have no prelude"
                );
            }
        }
    }

    /// A container host id has to resolve to the runtime's non-TTY exec: the
    /// runtime's own binary, the exec prefix, and a POSIX `-c` style that
    /// carries the command into the container's shell. Asserted per *installed*
    /// runtime; the missing ones are covered by the refusal test below.
    #[test]
    fn a_container_host_resolves_to_a_non_tty_exec_plan() {
        for runtime in ContainerRuntime::ALL {
            let Some(binary) = runtime.resolve() else {
                continue;
            };
            let plan =
                resolve_plan_for_host(&format!("local:{}:abc123", runtime.id())).expect("a plan");
            assert_eq!(plan.program, binary, "{}", runtime.id());
            assert_eq!(
                plan.prefix,
                vec![
                    "exec".to_string(),
                    "abc123".to_string(),
                    CONTAINER_SHELL.to_string()
                ],
                "{} capture argv",
                runtime.id()
            );
            assert_eq!(plan.style, CommandStyle::PosixDashC, "{}", runtime.id());
            assert!(plan.prelude.is_none(), "{}", runtime.id());
            assert!(
                !plan.forwards_stdin,
                "{} capture exec has no -i to carry stdin",
                runtime.id()
            );
            assert!(plan.label.contains("abc123"), "{}", runtime.id());
        }
    }

    /// An installed runtime id with no container behind the colon names
    /// nothing: the id check runs before the binary lookup, so the refusal does
    /// not depend on what is installed.
    #[test]
    fn an_empty_container_id_is_refused_before_the_runtime_lookup() {
        let error = resolve_plan_for_host("local:docker:")
            .expect_err("an empty container id names nothing");
        assert!(
            error.to_string().contains("invalid container id"),
            "{error}"
        );
    }

    /// Anything that is not a plain container name or id is refused — the id
    /// travels as a single argv element, but keeping it inside the same
    /// character set the runtimes themselves enforce means a malformed caller
    /// is a refusal rather than a question of how the CLI would parse it.
    #[test]
    fn an_invalid_container_id_is_refused() {
        let overlong = "x".repeat(MAX_CONTAINER_ID_LEN + 1);
        for bad in [
            "",
            "a b",
            "a;b",
            "a/b",
            "a:b",
            "$(id)",
            "`id`",
            overlong.as_str(),
        ] {
            let host = format!("local:docker:{bad}");
            let error =
                resolve_plan_for_host(&host).expect_err("a malformed container id is refused");
            let message = error.to_string();
            assert!(
                message.contains("invalid container id"),
                "host {host:?} was not refused as an id: {message}"
            );
        }
        for good in ["abc123def456", "my-app_1.2", "AaBb09._-"] {
            validate_container_id(good).expect("a well-formed container id");
        }
    }

    /// A runtime id that parses but whose binary is missing is a refusal that
    /// names the runtime — never a silent fallback to some shell.
    #[test]
    fn a_container_host_with_a_missing_runtime_is_refused() {
        for runtime in ContainerRuntime::ALL {
            if runtime.resolve().is_some() {
                continue;
            }
            let host = format!("local:{}:abc123", runtime.id());
            let error = resolve_plan_for_host(&host).expect_err("the runtime is not installed");
            let message = error.to_string();
            assert!(
                message.contains("not installed"),
                "host {host:?} must be refused as uninstalled: {message}"
            );
            assert!(
                message.contains(runtime.display_name()),
                "the refusal should name the runtime: {message}"
            );
        }
    }

    /// A name that is neither a runtime nor an environment is refused by the
    /// environment path, and the refusal says what a container id looks like.
    #[test]
    fn an_unknown_container_kind_lists_the_container_form() {
        let error =
            resolve_plan_for_host("local:pwsx:abc123").expect_err("an unknown kind is refused");
        let message = error.to_string();
        assert!(message.contains("pwsx"), "{message}");
        assert!(
            message.contains("container-id"),
            "the refusal should mention the container form: {message}"
        );
    }

    /// `wsl.exe` is a launcher, not a shell: a command handed to it through a `-c`
    /// this crate picked would never reach the distribution's environment — which
    /// is the only thing a caller naming WSL wanted. The default environment
    /// therefore refuses it and points at the one that works.
    #[test]
    #[serial_test::serial]
    fn the_default_environment_refuses_wsl_exe_as_a_shell() {
        let previous = std::env::var(SHELL_ENV).ok();
        // Only the binary *name* matters to the refusal, so the path does not have
        // to exist for this to still be the right answer.
        std::env::set_var(SHELL_ENV, "C:/Windows/System32/wsl.exe");

        let result = resolve_environment(LocalEnvironmentRequest::plain(LocalEnvironment::Default));

        match previous {
            Some(value) => std::env::set_var(SHELL_ENV, value),
            None => std::env::remove_var(SHELL_ENV),
        }

        let error = result.expect_err("wsl.exe must not be accepted as a shell");
        let message = error.to_string();
        assert!(message.contains("not a shell"), "{message}");
        assert!(message.contains("local:wsl"), "{message}");
    }
}
