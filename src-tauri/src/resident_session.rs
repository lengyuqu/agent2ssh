//! Resident environment sessions — a long-lived shell for a `local:<id>`
//! environment, so an agent can pay an environment's establishment cost once
//! instead of once per command.
//!
//! A one-shot exec (`local_exec`) builds its environment from scratch every
//! time: the Visual Studio developer shell costs roughly seven seconds before
//! `cl.exe` runs, and an agent compiling in a loop pays it on every command. A
//! resident session spawns the environment's shell *without* a command, keeps
//! it alive in the daemon, and feeds it commands over stdin — so the bootstrap
//! runs once at open, and every later command starts warm, with the shell's
//! own state (`cd`, variables, functions) persisting across commands.
//!
//! ## Demarcation: the sentinel protocol
//!
//! A pipe-connected shell never signals "this command is done". The session
//! does it explicitly: every command is written to the shell's stdin followed
//! by a sentinel line the *session* appends, carrying a nonce the caller could
//! not have predicted and the command's exit code:
//!
//! ```text
//! <command>\n
//! __A2S_DONE_<nonce>:<exit-code>\n
//! ```
//!
//! `exec` then reads the session buffer until that exact marker appears, and
//! everything before it is the command's output. Because the nonce is
//! generated per command and written *after* the command text, a command
//! cannot print its own sentinel — the worst a colliding output line can do
//! is be skipped by the exit-code parse, not end a command early.
//!
//! Framing per shell family (verified against the real binaries on Windows —
//! `pwsh -Command -` executes statements incrementally while stdin stays open;
//! `cmd` echoes each command line before running it, which the exit-code parse
//! skips because `%ERRORLEVEL%` is not a number until cmd expands it):
//!
//! * PowerShell — `Write-Output "__A2S_DONE_<nonce>:$LASTEXITCODE"`. The
//!   command is deliberately *not* wrapped in a scriptblock: `& { $x = 1 }`
//!   scopes the assignment away, which would quietly break the state
//!   persistence this feature exists for. `$LASTEXITCODE` is reset before each
//!   command, so a cmdlet-only command reports "no exit code" rather than a
//!   stale one left by an earlier native command.
//! * cmd — `ver >nul` then `echo __A2S_DONE_<nonce>:%ERRORLEVEL%`. The reset
//!   is the same idea as PowerShell's: cmd's `%ERRORLEVEL%` is sticky and
//!   `echo`, `set` and `cd` never touch it, so without it one failed command
//!   would make every later one report that code. `ver >nul` is the way to
//!   clear it — a bare `call`, the other commonly suggested trick, sets it to
//!   1 (measured on Windows 11).
//! * POSIX / WSL — `printf '__A2S_DONE_<nonce>:%s\n' "$?"`.
//!
//! ## Semantics that differ from a one-shot exec
//!
//! * **State persists.** That is the point, but it also means a caller cannot
//!   assume each command starts from a clean slate; one-shot `local:<id>`
//!   remains the default for exactly that reason.
//! * **A timeout kills the session.** A command that outlives its deadline
//!   cannot be interrupted inside the shell (Windows offers no console-ctrl to
//!   a shell without a console), so the honest move is to kill the whole shell
//!   and report the session dead. Long-running work belongs in a one-shot
//!   exec, which reaps its own process tree and returns.
//! * **A command that exits the shell kills the session.** `exit` is a
//!   perfectly authorized command and it does what it says.
//! * **Streams are merged.** A long-lived shell's stdout and stderr interleave
//!   in one pipe-side buffer; separating them after the fact would reorder
//!   reality.
//!
//! ## Authorization
//!
//! Nothing here weakens the exec gate chain. The daemon handler runs *both*
//! the open and every subsequent command through the same
//! `preflight_guarded_request` the other exec targets use, keyed on the
//! session's `local:<id>` host — so token scopes, approval policies and audit
//! entries name a resident session's commands exactly as they name one-shot
//! ones. This module itself performs no authorization; it is the mechanism
//! underneath the policy, like `local_exec::run_local_command_capture`.
//!
//! ## Lifecycle
//!
//! Sessions are daemon-resident and die with the daemon. Caps
//! (`AGENT2SSH_MAX_RESIDENT_SESSIONS`, 0 = unlimited), an idle TTL
//! (`AGENT2SSH_RESIDENT_IDLE_TTL_SECS`), an explicit close and the lifecycle
//! registry bound how many long-lived shells can exist; every handle kills its
//! process group on drop, so removing a session from the registry is enough to
//! reclaim it.

use anyhow::{anyhow, Context, Result};
use std::{
    collections::HashMap,
    io::{ErrorKind, Read, Write},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{Arc, Mutex as StdMutex},
    time::{Duration, Instant},
};
use uuid::Uuid;

use crate::{
    app_state::app_state,
    container_runtime::ContainerRuntime,
    events::{publish_event, EventType},
    local_exec::{
        check_exec_depth, configure_process_group, kill_process_tree, resolve_plan_for_host,
        CommandStyle, EnvironmentPlan, DEPTH_ENV,
    },
    store::append_audit,
    types::{ExecResult, RiskLevel, LOCAL_ENV_PREFIX},
};

/// How long opening a session may take. The Visual Studio bootstrap — the
/// slowest environment this module warm-starts — lands well inside it.
const OPEN_TIMEOUT_SECS: u64 = 120;

/// Default cap on concurrent resident sessions. Each is a live shell process,
/// so the budget is deliberately tighter than the PTY session cap.
const MAX_SESSIONS_DEFAULT: usize = 8;

/// Default idle TTL. A session no one has commanded for ten minutes is
/// reclaimed; an agent that wants it back opens a new one.
const IDLE_TTL_DEFAULT: u64 = 600;

/// Per-session output budget. A resident shell's output accumulates across
/// commands, so the bound is a rolling window (oldest dropped first) rather
/// than a per-command cut.
const SESSION_MAX_OUTPUT: usize = 4 * 1024 * 1024;

/// How often the sentinel wait loop re-checks the buffer and the child.
const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// After the shell has exited, how long buffered output may still be flushed
/// (and a sentinel with it) before the session is declared dead.
const EXIT_GRACE: Duration = Duration::from_secs(2);

/// Marker every command's sentinel line starts with. The nonce following it
/// is what makes one command's sentinel unforgeable by its own output.
pub const SENTINEL_PREFIX: &str = "__A2S_DONE_";

pub const MAX_SESSIONS_ENV: &str = "AGENT2SSH_MAX_RESIDENT_SESSIONS";
pub const IDLE_TTL_ENV: &str = "AGENT2SSH_RESIDENT_IDLE_TTL_SECS";

fn env_count(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(default)
}

/// Concurrent resident session cap. `0` means unlimited — testing only.
pub fn max_sessions() -> usize {
    env_count(MAX_SESSIONS_ENV, MAX_SESSIONS_DEFAULT)
}

/// Idle seconds after which a session is reclaimed by [`reap_idle_sessions`].
pub fn idle_ttl_secs() -> u64 {
    env_count(IDLE_TTL_ENV, IDLE_TTL_DEFAULT as usize) as u64
}

// ── Framing ─────────────────────────────────────────────────────────────────

/// A fresh nonce for one command's sentinel.
fn new_nonce() -> String {
    Uuid::new_v4().simple().to_string()
}

/// Arguments the shell is spawned with so it reads commands from stdin.
///
/// `-Command -` for PowerShell: verified to execute each statement as it
/// completes while stdin stays open, without echoing prompts or input. `cmd`
/// reads stdin line by line on its own; `/D` keeps a machine-wide AutoRun
/// script out of it, same as the one-shot path.
fn resident_args(style: CommandStyle) -> &'static [&'static str] {
    match style {
        CommandStyle::PowerShellCommand => {
            &["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", "-"]
        }
        CommandStyle::CmdSlashC => &["/D", "/Q"],
        CommandStyle::PosixDashC => &[],
        // Without a command argument WSL starts the distribution's own default
        // shell, which is the resident form of `WslLoginShell`.
        CommandStyle::WslLoginShell => &[],
    }
}

/// A no-op command used to warm the shell and verify the sentinel round-trip.
fn warmup_command(style: CommandStyle) -> &'static str {
    match style {
        CommandStyle::PowerShellCommand => "$null",
        CommandStyle::CmdSlashC => "rem agent2ssh warmup",
        CommandStyle::PosixDashC | CommandStyle::WslLoginShell => ":",
    }
}

/// Build what is written to the shell's stdin to run one command and mark its
/// end. `command` may span multiple lines; the frame only appends to it.
fn command_frame(plan: &EnvironmentPlan, command: &str, nonce: &str) -> String {
    let marker = format!("{SENTINEL_PREFIX}{nonce}:");
    match plan.style {
        // Double quotes are load-bearing: `$LASTEXITCODE` must interpolate.
        // The reset keeps a cmdlet-only command from inheriting the previous
        // command's exit code.
        CommandStyle::PowerShellCommand => format!(
            "$global:LASTEXITCODE = $null\n{command}\nWrite-Output \"{marker}$LASTEXITCODE\"\n"
        ),
        // `\r\n`: cmd treats a lone `\n` as line end too, but staying with the
        // family's own terminator costs nothing.
        //
        // The leading reset is cmd's counterpart to PowerShell's
        // `$LASTEXITCODE = $null`: cmd's `%ERRORLEVEL%` is sticky, and commands
        // like `echo`, `set` and `cd` never touch it, so without this a single
        // failed command would make every later one report the stale code —
        // `exit 7` followed by `echo hi` would answer 7 for a command that
        // cannot fail. `ver >nul` is the way to clear it: a bare `call`, the
        // other commonly suggested trick, sets it to 1 instead (measured), and
        // `cmd /c exit /b 0` pays a child process for what `ver` does in-place.
        CommandStyle::CmdSlashC => {
            format!("ver >nul\r\n{command}\r\necho {marker}%ERRORLEVEL%\r\n")
        }
        CommandStyle::PosixDashC | CommandStyle::WslLoginShell => {
            format!("{command}\nprintf '{marker}%s\\n' \"$?\"\n")
        }
    }
}

/// Locate the *last* parseable sentinel for `nonce` in accumulated output.
///
/// Returns `(marker_start, end_past_line, exit_code)`. An empty exit code
/// parses as `None` (a cmdlet-only PowerShell command has none). An
/// unparseable value — cmd echoing `echo __A2S_DONE_..:%ERRORLEVEL%` back
/// before running it, or user output that happens to contain the marker — is
/// skipped in favour of a later occurrence rather than trusted.
fn find_sentinel(data: &[u8], nonce: &str) -> Option<(usize, usize, Option<i32>)> {
    let marker = format!("{SENTINEL_PREFIX}{nonce}:");
    let marker = marker.as_bytes();
    let mut best: Option<(usize, usize, Option<i32>)> = None;
    let mut from = 0usize;
    while let Some(rel) = data[from..]
        .windows(marker.len())
        .position(|window| window == marker)
    {
        let start = from + rel;
        let after = start + marker.len();
        let line_end = data[after..]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(data.len(), |p| after + p);
        let value = std::str::from_utf8(&data[after..line_end])
            .unwrap_or("")
            .trim_end_matches('\r');
        let parsed = if value.is_empty() {
            Some(None)
        } else {
            value.parse::<i32>().ok().map(Some)
        };
        if let Some(code) = parsed {
            best = Some((start, (line_end + 1).min(data.len()), code));
        }
        from = after.max(start + 1);
    }
    best
}

// ── Session buffer ──────────────────────────────────────────────────────────

/// Rolling bounded accumulation of one session's merged output.
///
/// Oldest bytes are dropped first, so the newest output — where the sentinel
/// for the command being awaited lives — is always retained, and
/// `dropped_bytes` counts what an `exec` can no longer see, the same contract
/// `exec_capture::BoundedCapture` offers per command on the other targets.
pub(crate) struct SessionBuffer {
    data: Vec<u8>,
    max_bytes: usize,
    total: usize,
}

impl SessionBuffer {
    fn new(max_bytes: usize) -> Self {
        Self {
            data: Vec::new(),
            max_bytes,
            total: 0,
        }
    }

    fn push(&mut self, chunk: &[u8]) {
        self.total = self.total.saturating_add(chunk.len());
        self.data.extend_from_slice(chunk);
        if self.data.len() > self.max_bytes {
            let overflow = self.data.len() - self.max_bytes;
            self.data.drain(..overflow);
        }
    }

    fn truncated(&self) -> bool {
        self.total > self.max_bytes
    }

    fn dropped_bytes(&self) -> usize {
        self.total.saturating_sub(self.max_bytes)
    }

    fn consume(&mut self, through: usize) {
        let end = through.min(self.data.len());
        self.data.drain(..end);
    }
}

// ── Handle ──────────────────────────────────────────────────────────────────

pub struct ResidentHandle {
    pub(crate) id: Uuid,
    /// The `local:<id>` the session was opened for. Authorization, approval
    /// policy and audit entries all key on this, exactly as a one-shot exec
    /// keyed on the same host id would.
    pub(crate) host: String,
    pub(crate) label: String,
    pub(crate) plan: EnvironmentPlan,
    pub(crate) child: Child,
    /// Command serialization and pipe writes must not hold the handle lock:
    /// close needs the child while a command is running or blocked on stdin.
    stdin: Arc<StdMutex<Option<ChildStdin>>>,
    command_lock: Arc<tokio::sync::Mutex<()>>,
    pub(crate) buffer: Arc<StdMutex<SessionBuffer>>,
    pub(crate) last_active: Instant,
    pub(crate) dead: bool,
    running: bool,
    closed: bool,
    tree_killed: bool,
}

impl ResidentHandle {
    fn kill_tree(&mut self) {
        if !self.tree_killed {
            kill_process_tree(&mut self.child);
            self.tree_killed = true;
        }
        self.dead = true;
    }
}

impl Drop for ResidentHandle {
    fn drop(&mut self) {
        self.kill_tree();
    }
}

fn registry() -> &'static tokio::sync::Mutex<HashMap<Uuid, Arc<StdMutex<ResidentHandle>>>> {
    &app_state().resident_sessions
}

// ── Open ────────────────────────────────────────────────────────────────────

/// What opening a session reports.
#[derive(Debug, serde::Serialize)]
pub struct ResidentSessionInfo {
    pub id: Uuid,
    pub host: String,
    pub label: String,
    pub warmup_ms: u128,
}

/// One session as listed.
#[derive(Debug, serde::Serialize)]
pub struct ResidentSessionSummary {
    pub id: Uuid,
    pub host: String,
    pub label: String,
    pub idle_secs: u64,
    pub dead: bool,
}

/// One session in detail: the summary fields plus what the session's rolling
/// buffer currently holds. The buffer is the *same* accumulation `exec` reads —
/// there is no second capture path — so this is the "start then collect" view:
/// a command left running in the background inside the shell keeps writing
/// here, and a later read picks it up.
#[derive(Debug, serde::Serialize)]
pub struct ResidentSessionDetail {
    pub id: Uuid,
    pub host: String,
    pub label: String,
    pub idle_secs: u64,
    pub dead: bool,
    /// Recent merged output (oldest bytes already rolled off when the window
    /// filled). Lossy UTF-8: a torn multibyte character decimates rather than
    /// failing the read.
    pub output: String,
    pub truncated: bool,
    pub dropped_bytes: usize,
}

/// A container cannot host a resident shell: its capture exec is one-shot by
/// design and its environment is whatever the image ships. Refuse with the
/// reason rather than letting the generic environment resolver produce a
/// confusing "unknown environment" for a name that *is* known.
fn reject_container_host(host: &str) -> Result<()> {
    if let Some(rest) = host.strip_prefix(LOCAL_ENV_PREFIX) {
        if let Some((kind, _)) = rest.split_once(':') {
            if ContainerRuntime::from_id(kind).is_some() {
                return Err(anyhow!(
                    "resident sessions need a shell environment, and '{host}' names a \
                     container: containers are one-shot exec targets, so open a session \
                     in a shell environment instead"
                ));
            }
        }
    }
    Ok(())
}

/// Open a resident session in the environment `host` names (`local:vs`,
/// `local:pwsh`, ...).
///
/// Blocks until the shell is warm — the prelude, if the environment has one,
/// has run and answered its own sentinel — so the first `exec` pays no
/// environment-establishment cost. Performed on a blocking task: the VS
/// bootstrap takes seconds.
pub async fn resident_open_core(host: &str) -> Result<ResidentSessionInfo> {
    // Resolved on the async side like the one-shot path does: an environment
    // that cannot be built is a refusal the caller can audit, not an I/O
    // error from mid-spawn.
    reject_container_host(host)?;
    let plan = resolve_plan_for_host(host)?;
    let depth = check_exec_depth()?;

    let max = max_sessions();
    if max > 0 {
        let current = registry().lock().await.len();
        if current >= max {
            return Err(anyhow!(
                "resident session limit reached: {current} active (max {max}); close one or \
                 raise {MAX_SESSIONS_ENV}"
            ));
        }
    }

    let id = Uuid::new_v4();
    let lifecycle = crate::app_state::lifecycle();
    let reservation = crate::lifecycle::LifecycleRegistry::reserve(
        &lifecycle,
        &id.to_string(),
        crate::app_state::ResourceKind::ResidentSession,
        crate::app_state::ResourceOwner::Headless(Uuid::new_v4()),
    )
    .map_err(|e| anyhow!("lifecycle reserve failed: {e}"))?;

    let host_owned = host.to_string();
    let spawned = tokio::task::spawn_blocking(move || spawn_and_warm(host_owned, plan, depth))
        .await
        .map_err(|e| anyhow!("resident session warmup task failed: {e}"))?;
    let (mut handle, warmup_ms) = spawned.inspect_err(|_| {
        // A failed warmup leaves nothing to activate; the reservation's Drop
        // marks the lifecycle entry Closed, and the handle's Drop kills the
        // half-warm shell.
    })?;
    handle.id = id;

    let label;
    {
        let mut store = registry().lock().await;
        if max > 0 && store.len() >= max {
            return Err(anyhow!(
                "resident session limit reached: {} active (max {max}); close one or raise \
                 {MAX_SESSIONS_ENV}",
                store.len()
            ));
        }
        label = handle.label.clone();
        store.insert(id, Arc::new(StdMutex::new(handle)));
    }
    reservation
        .activate()
        .map_err(|e| anyhow!("lifecycle activate failed: {e}"))?;

    Ok(ResidentSessionInfo {
        id,
        host: host.to_string(),
        label,
        warmup_ms,
    })
}

/// Spawn the environment's shell without a command, then run its prelude (if
/// any) plus a no-op and wait for the warmup sentinel. Returns the handle and
/// how long the warmup took — the number that shows an agent what opening
/// saved it on every later command.
fn spawn_and_warm(
    host: String,
    plan: EnvironmentPlan,
    depth: u32,
) -> Result<(ResidentHandle, u128)> {
    let started = Instant::now();
    let mut process = Command::new(&plan.program);
    process.args(&plan.prefix);
    process.args(resident_args(plan.style));
    configure_process_group(&mut process);
    process
        .env(DEPTH_ENV, depth.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = process.spawn().map_err(|error| {
        anyhow!(
            "failed to start resident shell for '{host}' ({}): {error}",
            plan.program.display()
        )
    })?;

    let buffer = Arc::new(StdMutex::new(SessionBuffer::new(SESSION_MAX_OUTPUT)));
    spawn_readers(&mut child, Arc::clone(&buffer));
    let stdin = child.stdin.take();

    let mut handle = ResidentHandle {
        id: Uuid::new_v4(), // replaced by the caller with the lifecycle id
        host,
        label: plan.label.clone(),
        plan,
        child,
        stdin: Arc::new(StdMutex::new(stdin)),
        command_lock: Arc::new(tokio::sync::Mutex::new(())),
        buffer,
        last_active: Instant::now(),
        dead: false,
        running: false,
        closed: false,
        tree_killed: false,
    };

    // Warm the shell: the prelude runs inside the very shell that will serve
    // later commands, so `Launch-VsDevShell.ps1` pays its cost exactly once.
    let warm_command = match &handle.plan.prelude {
        Some(prelude) => prelude.clone(),
        None => warmup_command(handle.plan.style).to_string(),
    };
    let nonce = new_nonce();
    write_frame(
        &handle.stdin,
        &handle.plan,
        &handle.host,
        &warm_command,
        &nonce,
    )?;
    wait_for_sentinel(&mut handle, &nonce, Duration::from_secs(OPEN_TIMEOUT_SECS))
        .map_err(|error| anyhow!("resident session failed to warm up: {error}"))?;

    handle.last_active = started;
    let warmup_ms = started.elapsed().as_millis();
    Ok((handle, warmup_ms))
}

fn spawn_readers(child: &mut Child, buffer: Arc<StdMutex<SessionBuffer>>) {
    // One thread per stream, merged into the buffer on arrival. Detached on
    // purpose: they end when the pipes close, which the process-group kill on
    // drop guarantees.
    if let Some(stream) = child.stdout.take() {
        spawn_one_reader(stream, Arc::clone(&buffer));
    }
    if let Some(stream) = child.stderr.take() {
        spawn_one_reader(stream, buffer);
    }
}

fn spawn_one_reader<R: Read + Send + 'static>(source: R, buffer: Arc<StdMutex<SessionBuffer>>) {
    std::thread::spawn(move || {
        let mut source = source;
        let mut chunk = [0u8; 8192];
        loop {
            match source.read(&mut chunk) {
                Ok(0) => break,
                Ok(read) => {
                    buffer
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .push(&chunk[..read]);
                }
                Err(error) if error.kind() == ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
    });
}

fn write_frame(
    stdin: &StdMutex<Option<ChildStdin>>,
    plan: &EnvironmentPlan,
    host: &str,
    command: &str,
    nonce: &str,
) -> Result<()> {
    let frame = command_frame(plan, command, nonce);
    let mut sink = stdin.lock().unwrap_or_else(|p| p.into_inner());
    let stdin = sink
        .as_mut()
        .ok_or_else(|| anyhow!("resident session shell has no stdin (already dead?)"))?;
    stdin
        .write_all(frame.as_bytes())
        .and_then(|_| stdin.flush())
        .with_context(|| format!("failed to write to resident shell for '{host}'"))
}

/// What waiting for a sentinel can end in.
pub(crate) enum WaitOutcome {
    /// Command output (bytes before the sentinel), its exit code, whether the
    /// session's window truncated anything, and how many bytes were dropped.
    Completed {
        output: Vec<u8>,
        exit_code: Option<i32>,
        truncated: bool,
        dropped_bytes: usize,
    },
    /// The deadline passed first. The session has been killed.
    TimedOut { timeout_secs: u64 },
    /// The shell itself exited before the sentinel appeared.
    ShellExited { code: Option<i32> },
    /// An explicit close cancelled the command.
    Closed,
}

/// Read the session buffer until `nonce`'s sentinel appears, the deadline
/// passes, or the shell exits.
///
/// On timeout the whole session is killed — a command cannot be interrupted
/// inside its shell, and a shell stuck mid-command is not a session worth
/// keeping. The caller owns what that means for the API surface.
fn wait_for_sentinel(
    handle: &mut ResidentHandle,
    nonce: &str,
    timeout: Duration,
) -> Result<WaitOutcome> {
    let deadline = Instant::now() + timeout;
    let mut exited_at: Option<Instant> = None;

    loop {
        if let Some(outcome) = poll_sentinel(handle, nonce, &mut exited_at, deadline, timeout)? {
            return Ok(outcome);
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

/// One non-waiting observation. Registered sessions release the handle lock
/// between polls so metadata, buffer reads and cancellation remain available.
fn poll_sentinel(
    handle: &mut ResidentHandle,
    nonce: &str,
    exited_at: &mut Option<Instant>,
    deadline: Instant,
    timeout: Duration,
) -> Result<Option<WaitOutcome>> {
    if handle.closed {
        return Ok(Some(WaitOutcome::Closed));
    }
    {
        let mut buffer = handle.buffer.lock().unwrap_or_else(|p| p.into_inner());
        if let Some((start, end, exit_code)) = find_sentinel(&buffer.data, nonce) {
            let output = buffer.data[..start].to_vec();
            let truncated = buffer.truncated();
            let dropped_bytes = buffer.dropped_bytes();
            buffer.consume(end);
            handle.last_active = Instant::now();
            return Ok(Some(WaitOutcome::Completed {
                output,
                exit_code,
                truncated,
                dropped_bytes,
            }));
        }
    }

    if let Some(status) = handle.child.try_wait()? {
        // Buffered output may still be in flight; give it a moment before
        // declaring the session dead.
        let since = *exited_at.get_or_insert(Instant::now());
        if since.elapsed() > EXIT_GRACE {
            handle.dead = true;
            return Ok(Some(WaitOutcome::ShellExited {
                code: status.code(),
            }));
        }
    }

    if Instant::now() >= deadline {
        handle.kill_tree();
        return Ok(Some(WaitOutcome::TimedOut {
            timeout_secs: timeout.as_secs(),
        }));
    }
    Ok(None)
}

// ── Exec / close / list / reap ──────────────────────────────────────────────

/// How a command inside a session can fail.
pub(crate) enum ResidentExecError {
    Closed,
    /// Deadline passed; the session has been killed.
    Timeout {
        timeout_secs: u64,
    },
    /// The shell exited before the sentinel appeared; the session is dead.
    ShellExited {
        code: Option<i32>,
    },
    /// The session was already dead when the command arrived.
    Dead,
    Failed(anyhow::Error),
}

/// Run one command in a resident session and capture its output.
///
/// Commands on one session are serialized by a dedicated command lock. The
/// handle stays available to close and observation while a command is running.
///
/// Authorization and audit are the *caller's* job — the daemon handler runs
/// the shared preflight before reaching here and appends the audit entry from
/// the result, keyed on the session's `local:<id>` host.
#[allow(clippy::too_many_arguments)]
pub async fn resident_exec_core(
    session_id: Uuid,
    command: String,
    timeout_secs: Option<u64>,
    risk: RiskLevel,
    source: &str,
    reason: Option<String>,
    change_id: Option<String>,
    side_effect: Option<String>,
) -> Result<ExecResult> {
    let handle = {
        let store = registry().lock().await;
        store.get(&session_id).cloned()
    }
    .ok_or_else(|| anyhow!("unknown resident session: {session_id}"))?;

    let timeout = Duration::from_secs(timeout_secs.unwrap_or(60));
    let command_for_run = command.clone();
    let started = Instant::now();
    let command_lock = {
        let handle = handle.lock().unwrap_or_else(|p| p.into_inner());
        Arc::clone(&handle.command_lock)
    };
    // Queue on the async side. Waiting commands must not fill the blocking
    // pool needed to terminate the child when close arrives.
    let command_guard = command_lock.lock_owned().await;
    let outcome = tokio::task::spawn_blocking(move || {
        let _command = command_guard;
        exec_in_session(&handle, &command_for_run, timeout)
    })
    .await
    .map_err(|e| anyhow!("resident exec task failed: {e}"))?;

    let (captured, host) = match outcome {
        Ok((captured, host)) => (captured, host),
        Err(ResidentExecError::Closed) => {
            return Err(anyhow!("resident session was closed; command cancelled"));
        }
        Err(ResidentExecError::Timeout { timeout_secs }) => {
            return Err(anyhow!(
                "resident command timed out after {timeout_secs}s: '{command}'. The session \
                 was killed — long-running work belongs in a one-shot exec, which reaps its \
                 own process tree and returns"
            ));
        }
        Err(ResidentExecError::ShellExited { code }) => {
            return Err(match code {
                Some(code) => anyhow!(
                    "resident session shell exited with code {code} (the command terminated \
                     the shell, e.g. 'exit'); the session is dead"
                ),
                None => anyhow!("resident session shell exited; the session is dead"),
            });
        }
        Err(ResidentExecError::Dead) => {
            return Err(anyhow!("resident session is dead: open a new one"));
        }
        Err(ResidentExecError::Failed(error)) => return Err(error),
    };

    let result = ExecResult {
        host,
        command,
        exit_code: captured.exit_code,
        stdout: String::from_utf8_lossy(&captured.output).into_owned(),
        stderr: String::new(),
        duration_ms: started.elapsed().as_millis(),
        risk_level: risk,
        truncated: captured.truncated,
        dropped_bytes: captured.dropped_bytes,
        side_effect,
    };
    append_audit(
        &result,
        risk,
        reason.as_deref(),
        change_id.as_deref(),
        Some(source),
    )?;

    publish_event(
        EventType::ExecCompleted,
        serde_json::json!({
            "host": result.host,
            "command": result.command,
            "exit_code": result.exit_code,
            "risk_level": format!("{}", result.risk_level),
            "duration_ms": result.duration_ms,
            "reason": reason,
        }),
    );
    Ok(result)
}

/// What one completed command produced, ready for result construction.
pub(crate) struct CommandCapture {
    output: Vec<u8>,
    exit_code: Option<i32>,
    truncated: bool,
    dropped_bytes: usize,
}

/// Blocking body of [`resident_exec_core`]. Returns the captured output and
/// the session's host id for result construction.
fn exec_in_session(
    handle: &Arc<StdMutex<ResidentHandle>>,
    command: &str,
    timeout: Duration,
) -> Result<(CommandCapture, String), ResidentExecError> {
    let (stdin, plan, host) = {
        let mut handle = handle.lock().unwrap_or_else(|p| p.into_inner());
        if handle.closed {
            return Err(ResidentExecError::Closed);
        }
        if handle.dead {
            return Err(ResidentExecError::Dead);
        }
        if let Ok(Some(_)) = handle.child.try_wait() {
            handle.dead = true;
            return Err(ResidentExecError::Dead);
        }
        handle.running = true;
        handle.last_active = Instant::now();
        (
            Arc::clone(&handle.stdin),
            handle.plan.clone(),
            handle.host.clone(),
        )
    };

    let nonce = new_nonce();
    let outcome = (|| {
        // Pipe writes may block too; keep them outside the handle lock so a
        // close can kill the shell and release a writer waiting on stdin.
        write_frame(&stdin, &plan, &host, command, &nonce)?;
        let deadline = Instant::now() + timeout;
        let mut exited_at = None;
        loop {
            let outcome = {
                let mut handle = handle.lock().unwrap_or_else(|p| p.into_inner());
                poll_sentinel(&mut handle, &nonce, &mut exited_at, deadline, timeout)?
            };
            if let Some(outcome) = outcome {
                return Ok(outcome);
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    })();
    {
        let mut handle = handle.lock().unwrap_or_else(|p| p.into_inner());
        handle.running = false;
        handle.last_active = Instant::now();
        // Killing the shell may interrupt write_all before the sentinel loop
        // notices close. Report cancellation instead of an incidental pipe error.
        if handle.closed {
            return Err(ResidentExecError::Closed);
        }
    }
    let outcome = outcome.map_err(ResidentExecError::Failed)?;
    let capture = match outcome {
        WaitOutcome::Completed {
            output,
            exit_code,
            truncated,
            dropped_bytes,
        } => CommandCapture {
            output,
            exit_code,
            truncated,
            dropped_bytes,
        },
        WaitOutcome::Closed => return Err(ResidentExecError::Closed),
        WaitOutcome::TimedOut { timeout_secs } => {
            return Err(ResidentExecError::Timeout { timeout_secs })
        }
        WaitOutcome::ShellExited { code } => return Err(ResidentExecError::ShellExited { code }),
    };
    Ok((capture, host))
}

/// The `local:<id>` host a session was opened for, or `None` if the id is
/// unknown. Read-only lookup so the daemon can authorize a command against
/// the session's own host id rather than anything the client claims.
pub async fn session_host(session_id: Uuid) -> Option<String> {
    let handle = registry().lock().await.get(&session_id).cloned()?;
    let handle = handle.lock().ok()?;
    Some(handle.host.clone())
}

/// Close a session: kill its shell, release it from the registry and close its
/// lifecycle entry. Returns the session's host id (for the audit entry).
/// Closing a dead session still reclaims the entry.
pub async fn resident_close_core(session_id: Uuid) -> Result<String> {
    let handle = registry()
        .lock()
        .await
        .remove(&session_id)
        .ok_or_else(|| anyhow!("unknown resident session: {session_id}"))?;
    {
        let mut handle = handle.lock().unwrap_or_else(|p| p.into_inner());
        handle.closed = true;
        handle.dead = true;
    }
    // taskkill can take time on Windows. Perform process-tree teardown on a
    // blocking worker, without a registry lock or the command-serialization lock.
    let host = tokio::task::spawn_blocking(move || {
        let host = {
            let mut handle = handle.lock().unwrap_or_else(|p| p.into_inner());
            handle.kill_tree();
            let _ = handle.child.wait();
            handle.host.clone()
        };
        // This worker continues if its caller cancels while awaiting teardown.
        // Keep lifecycle cleanup here too, after releasing the handle lock.
        let _ = crate::app_state::lifecycle()
            .lock()
            .unwrap()
            .close(&session_id.to_string(), None);
        host
    })
    .await
    .map_err(|e| anyhow!("resident close task failed: {e}"))?;
    Ok(host)
}

/// List sessions, oldest activity first not guaranteed — registry order.
pub async fn resident_list_core() -> Vec<ResidentSessionSummary> {
    let handles: Vec<_> = registry().lock().await.values().cloned().collect();
    handles
        .iter()
        .filter_map(|handle| {
            let handle = handle.lock().ok()?;
            Some(ResidentSessionSummary {
                id: handle.id,
                host: handle.host.clone(),
                label: handle.label.clone(),
                idle_secs: handle.last_active.elapsed().as_secs(),
                dead: handle.dead,
            })
        })
        .collect()
}

/// Read one session: identity fields plus the current buffer contents.
///
/// A read is observation only — it does not consume the buffer (exec's
/// sentinel-driven `consume` stays the only consumer), does not count as
/// activity for the idle TTL, and does not need the execution gate, exactly
/// like listing. A session whose shell has exited is reported `dead` here even
/// if nothing marked it yet, so a collector can stop polling without waiting
/// for the idle reaper.
pub async fn resident_get_core(session_id: Uuid) -> Result<ResidentSessionDetail> {
    let handle = registry()
        .lock()
        .await
        .get(&session_id)
        .cloned()
        .ok_or_else(|| anyhow!("unknown resident session: {session_id}"))?;
    let mut handle = handle.lock().unwrap_or_else(|p| p.into_inner());
    let mut dead = handle.dead;
    if !dead {
        if let Ok(Some(_)) = handle.child.try_wait() {
            dead = true;
        }
    }
    let buffer = handle.buffer.lock().unwrap_or_else(|p| p.into_inner());
    Ok(ResidentSessionDetail {
        id: handle.id,
        host: handle.host.clone(),
        label: handle.label.clone(),
        idle_secs: handle.last_active.elapsed().as_secs(),
        dead,
        output: String::from_utf8_lossy(&buffer.data).into_owned(),
        truncated: buffer.truncated(),
        dropped_bytes: buffer.dropped_bytes(),
    })
}

/// Reclaim every session idle past the TTL. Called periodically by the daemon;
/// returns how many sessions were closed.
pub async fn reap_idle_sessions() -> usize {
    let ttl = Duration::from_secs(idle_ttl_secs());
    let expired: Vec<Uuid> = {
        let handles: Vec<_> = registry().lock().await.values().cloned().collect();
        handles
            .iter()
            .filter_map(|handle| {
                let handle = handle.lock().ok()?;
                (!handle.dead && !handle.running && handle.last_active.elapsed() > ttl)
                    .then_some(handle.id)
            })
            .collect()
    };
    let mut closed = 0;
    for id in expired {
        if resident_close_core(id).await.is_ok() {
            closed += 1;
        }
    }
    closed
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(style: CommandStyle) -> EnvironmentPlan {
        EnvironmentPlan {
            program: "shell".into(),
            prefix: vec![],
            style,
            prelude: None,
            forwards_stdin: true,
            label: "test".into(),
        }
    }

    #[test]
    fn power_shell_frame_resets_exit_code_and_interpolates_it() {
        let frame = command_frame(&plan(CommandStyle::PowerShellCommand), "Get-Date", "abc123");
        assert!(frame.starts_with("$global:LASTEXITCODE = $null\nGet-Date\n"));
        // Double quotes so $LASTEXITCODE interpolates; single quotes would
        // echo the literal variable name and the parse would never complete.
        assert!(frame.ends_with("Write-Output \"__A2S_DONE_abc123:$LASTEXITCODE\"\n"));
    }

    #[test]
    fn cmd_frame_clears_stale_errorlevel_and_uses_crlf() {
        let frame = command_frame(&plan(CommandStyle::CmdSlashC), "dir", "abc123");
        // The reset is what keeps a command that cannot fail (`echo`, `set`)
        // from reporting the previous command's exit code. `call` is not it:
        // measured, it sets ERRORLEVEL to 1.
        assert_eq!(
            frame,
            "ver >nul\r\ndir\r\necho __A2S_DONE_abc123:%ERRORLEVEL%\r\n"
        );
        assert!(!frame.contains("\n\n"), "cmd frames stay CRLF-terminated");
    }

    #[test]
    fn cmd_frame_keeps_a_multi_line_command_together() {
        let command = "if exist . (\r\n  echo here\r\n)";
        let frame = command_frame(&plan(CommandStyle::CmdSlashC), command, "n1");
        assert!(frame.starts_with("ver >nul\r\n"));
        assert!(frame.contains("if exist . (\r\n  echo here\r\n)"));
        assert!(frame.ends_with("echo __A2S_DONE_n1:%ERRORLEVEL%\r\n"));
    }

    #[test]
    fn posix_frame_passes_status_through_printf() {
        let frame = command_frame(&plan(CommandStyle::PosixDashC), "ls", "abc123");
        assert_eq!(frame, "ls\nprintf '__A2S_DONE_abc123:%s\\n' \"$?\"\n");
    }

    #[test]
    fn multi_line_command_is_sent_verbatim_before_the_sentinel() {
        let command = "if ($true) {\n  Write-Output hi\n}";
        let frame = command_frame(&plan(CommandStyle::PowerShellCommand), command, "n1");
        assert!(frame.contains("if ($true) {\n  Write-Output hi\n}\n"));
    }

    #[test]
    fn sentinel_parse_reads_exit_code() {
        let data = b"hello\n__A2S_DONE_n1:3\nafter\n";
        let (start, end, code) = find_sentinel(data, "n1").expect("sentinel present");
        assert_eq!(&data[..start], b"hello\n");
        assert_eq!(end, data.len() - b"after\n".len());
        assert_eq!(code, Some(3));
    }

    #[test]
    fn sentinel_parse_accepts_empty_exit_code() {
        // A cmdlet-only PowerShell command has no $LASTEXITCODE.
        let data = b"out\n__A2S_DONE_n1:\n";
        let (_, _, code) = find_sentinel(data, "n1").expect("sentinel present");
        assert_eq!(code, None);
    }

    #[test]
    fn sentinel_parse_skips_cmd_echo_and_takes_the_last_parseable() {
        // cmd echoes the frame back before executing it: the echoed line
        // carries the literal `%ERRORLEVEL%`, which must not satisfy the
        // wait.
        let data = b"D:\\>echo __A2S_DONE_n1:%ERRORLEVEL%\r\nout\r\n__A2S_DONE_n1:0\r\n";
        let (start, _, code) = find_sentinel(data, "n1").expect("sentinel present");
        assert_eq!(code, Some(0));
        assert!(&data[..start].ends_with(b"out\r\n"));
    }

    #[test]
    fn sentinel_parse_ignores_unrelated_and_wrong_nonce_markers() {
        assert!(find_sentinel(b"__A2S_DONE_other:0\n", "n1").is_none());
        assert!(find_sentinel(b"__A2S_DONE_n1:notanumber\n", "n1").is_none());
    }

    #[test]
    fn sentinel_found_across_chunk_boundaries() {
        let mut buffer = SessionBuffer::new(1024);
        buffer.push(b"out __A2S_DO");
        assert!(find_sentinel(&buffer.data, "n1").is_none());
        buffer.push(b"NE_n1:7\n");
        let (start, _, code) = find_sentinel(&buffer.data, "n1").expect("sentinel present");
        assert_eq!(code, Some(7));
        assert_eq!(start, 4);
    }

    #[test]
    fn buffer_rolls_oldest_bytes_out_and_counts_them() {
        let mut buffer = SessionBuffer::new(16);
        buffer.push(&[b'a'; 20]);
        buffer.push(b"__A2S_DONE_n1:0\n");
        assert!(buffer.truncated());
        assert_eq!(buffer.dropped_bytes(), 20 + 16 - 16);
        assert_eq!(buffer.data.len(), 16);
        // The sentinel (newest) survives the roll.
        assert!(find_sentinel(&buffer.data, "n1").is_some());
        buffer.consume(16);
        assert!(buffer.data.is_empty());
    }

    #[test]
    fn container_hosts_are_refused_for_resident_sessions() {
        assert!(reject_container_host("local:docker:abc123").is_err());
        assert!(reject_container_host("local:wslc:xyz").is_err());
        assert!(reject_container_host("local:vs").is_ok());
        assert!(reject_container_host("local").is_ok());
    }

    #[test]
    fn warmup_commands_are_noops_in_their_own_family() {
        assert_eq!(warmup_command(CommandStyle::PowerShellCommand), "$null");
        assert_eq!(
            warmup_command(CommandStyle::CmdSlashC),
            "rem agent2ssh warmup"
        );
        assert_eq!(warmup_command(CommandStyle::PosixDashC), ":");
    }
}
