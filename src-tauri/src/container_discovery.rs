//! Container discovery for Docker, wslc, Apple container and Kubernetes (B33).
//!
//! Discovers running containers and Kubernetes pods that can be used as exec
//! targets — converting them into `ContainerDiscoveryTarget` entries that the
//! CLI or desktop app can present as quick-connect options.
//!
//! Mirrors rssh's `commands/discovery.rs` pattern, adapted to agent2ssh's
//! architecture (no DB, uses CLI subprocess calls to the container CLIs).
//!
//! ## How it works
//!
//! 1. Ask [`crate::container_runtime`] which local runtimes are installed
//!    (`ContainerRuntime::resolve` also looks at the install locations the
//!    desktop app's inherited PATH misses — see `path_resolver`).
//! 2. For each runtime (Docker, wslc, Apple container) run its list command;
//!    for Kubernetes run `kubectl config get-contexts` then `get pods -A -o json`.
//! 3. Parse the runtime's own JSON shape into one normalized row:
//!    - Docker and wslc emit docker-compatible **JSON-lines** (`ID`, `Image`,
//!      `Names`, `Status`/`State`).
//!    - Apple emits a single JSON **array** of `ManagedContainer`
//!      (`id`, `configuration.image.reference`, `status.state`).
//!    - Kubernetes emits a pod list (`items[].metadata` / `status` / `spec`).
//! 4. Convert each row into a `ContainerDiscoveryTarget` with a unique ID and
//!    the argv needed to exec into it, built by the runtime abstraction.
//!
//! Everything runtime-specific (binary name, install dirs, list flags, exec
//! argv, id prefix) lives in `container_runtime.rs`; this module only knows how
//! to turn a runtime's JSON into targets.

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use crate::container_runtime::{self, ContainerRuntime, CONTAINER_SHELL};
use crate::path_resolver::resolve_executable_in;

/// Scope label used for runtimes that have no context concept (wslc, Apple).
/// Docker contexts and Kubernetes contexts carry their real name here instead.
pub const LOCAL_CONTEXT: &str = "local";

/// The container platform a discovered target belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContainerPlatform {
    Docker,
    /// Windows Subsystem for Linux container CLI.
    Wslc,
    /// Apple's container runtime.
    Apple,
    K8s,
}

impl ContainerPlatform {
    /// The local runtime behind this platform, when it is a local one.
    pub const fn runtime(self) -> Option<ContainerRuntime> {
        match self {
            Self::Docker => Some(ContainerRuntime::Docker),
            Self::Wslc => Some(ContainerRuntime::Wslc),
            Self::Apple => Some(ContainerRuntime::Apple),
            Self::K8s => None,
        }
    }

    pub const fn id(self) -> &'static str {
        match self {
            Self::Docker => "docker",
            Self::Wslc => "wslc",
            Self::Apple => "apple",
            Self::K8s => "k8s",
        }
    }
}

/// A discovered container/pod that can be used as an exec target.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerDiscoveryTarget {
    /// Unique ID (e.g. `docker_exec:default:abc123` or `wslc_exec:local:abc123`).
    pub id: String,
    /// The platform (docker, wslc, apple or k8s).
    pub platform: ContainerPlatform,
    /// The context name (Docker context / Kubernetes context). Local runtimes
    /// without a context concept report [`LOCAL_CONTEXT`].
    pub context: String,
    /// Container ID (Docker/wslc/Apple) or pod name (Kubernetes).
    pub container_id: String,
    /// Container/pod display name. On Apple's runtime this equals the id —
    /// `ManagedContainer.name` returns `configuration.id`.
    pub container_name: String,
    /// Image the container was created from ("Running" pods report the first
    /// container image is not available here, so Kubernetes pods report empty).
    pub image: String,
    /// Status string from the platform (e.g. "Up 2 hours", "Created", "Running").
    pub status: String,
    /// The exec command args to connect to this target.
    /// e.g. ["exec", "-it", "abc123", "/bin/sh"] for Docker/wslc/Apple.
    pub exec_args: Vec<String>,
    /// The resolved binary path to execute (docker, wslc, container or kubectl).
    pub exec_binary: String,
    /// For Kubernetes: the namespace. None for local runtimes.
    pub namespace: Option<String>,
}

impl ContainerDiscoveryTarget {
    /// The argv to exec into this target, split from the binary path so callers
    /// that build a shell command for the user can quote each part.
    pub fn exec_argv(&self) -> Vec<String> {
        let mut argv = vec![self.exec_binary.clone()];
        argv.extend(self.exec_args.iter().cloned());
        argv
    }
}

// ── Normalized rows ─────────────────────────────────────────────────────────

/// One container, normalized across the docker-compatible and Apple dialects.
#[derive(Debug, Clone, PartialEq, Eq)]
struct LocalContainerRow {
    id: String,
    name: String,
    image: String,
    status: String,
}

/// Read a string (or number) at a JSON path. Returns `None` for missing keys,
/// wrong types, or null.
fn str_at(value: &Value, path: &[&str]) -> Option<String> {
    let mut cursor = value;
    for key in path {
        cursor = cursor.get(key)?;
    }
    match cursor {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// First non-empty string among several candidate paths.
fn str_first(value: &Value, paths: &[&[&str]]) -> Option<String> {
    paths.iter().find_map(|path| str_at(value, path))
}

/// Normalize one JSON object into a row, per the runtime's dialect.
///
/// Docker and wslc share the docker CLI's key names (`ID`/`Names`/`Image`/
/// `Status`), so one branch covers both; Apple's `ManagedContainer` nests the
/// image under `configuration.image.reference` and the state under
/// `status.state`.
fn row_from_json(value: &Value, runtime: ContainerRuntime) -> Option<LocalContainerRow> {
    match runtime {
        ContainerRuntime::Docker | ContainerRuntime::Wslc => {
            let id = str_first(value, &[&["ID"], &["Id"], &["id"]])?;
            let name =
                str_first(value, &[&["Names"], &["Name"], &["name"]]).unwrap_or_else(|| id.clone());
            let image = str_first(value, &[&["Image"], &["image"]]).unwrap_or_default();
            let status = str_first(value, &[&["Status"], &["status"], &["State"], &["state"]])
                .unwrap_or_default();
            Some(LocalContainerRow {
                id,
                name,
                image,
                status,
            })
        }
        ContainerRuntime::Apple => {
            let id = str_at(value, &["id"])?;
            let image = str_first(
                value,
                &[
                    &["configuration", "image", "reference"],
                    &["configuration", "image", "name"],
                    &["image"],
                ],
            )
            .unwrap_or_default();
            let status = str_first(value, &[&["status", "state"], &["state"], &["status"]])
                .unwrap_or_default();
            Some(LocalContainerRow {
                id: id.clone(),
                // Apple containers have no separate name field: the id is the name.
                name: id,
                image,
                status,
            })
        }
    }
}

/// Parse a runtime's list output into rows.
///
/// Accepts both shapes a runtime may emit: a JSON array (`container list
/// --format json`) and JSON-lines (`docker ps --format {{json .}}`, `wslc ps
/// --format json`). Individual unparseable lines are skipped and counted, so a
/// single odd row does not hide the rest.
fn parse_local_rows(raw: &str, runtime: ContainerRuntime) -> (Vec<LocalContainerRow>, usize) {
    let mut rows = Vec::new();
    let mut skipped = 0usize;
    let trimmed = raw.trim();

    if trimmed.is_empty() {
        return (rows, skipped);
    }

    // Array shape first: it is a single JSON document, so a successful parse is
    // unambiguous. JSON-lines never parses as one document, so it falls through.
    if trimmed.starts_with('[') {
        if let Ok(Value::Array(items)) = serde_json::from_str::<Value>(trimmed) {
            for item in &items {
                match row_from_json(item, runtime) {
                    Some(row) => rows.push(row),
                    None => skipped += 1,
                }
            }
            return (rows, skipped);
        }
    }

    for line in trimmed.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<Value>(line) {
            Ok(value) => match row_from_json(&value, runtime) {
                Some(row) => rows.push(row),
                None => skipped += 1,
            },
            Err(_) => skipped += 1,
        }
    }

    (rows, skipped)
}

/// Build targets from parsed rows. Pure, so the argv shape is unit-testable
/// without spawning a container CLI.
fn targets_from_rows(
    runtime: ContainerRuntime,
    binary: &Path,
    rows: &[LocalContainerRow],
) -> Vec<ContainerDiscoveryTarget> {
    let binary_display = binary.display().to_string();
    rows.iter()
        .map(|row| ContainerDiscoveryTarget {
            id: format!("{}:{LOCAL_CONTEXT}:{}", runtime.exec_id_prefix(), row.id),
            platform: match runtime {
                ContainerRuntime::Docker => ContainerPlatform::Docker,
                ContainerRuntime::Wslc => ContainerPlatform::Wslc,
                ContainerRuntime::Apple => ContainerPlatform::Apple,
            },
            context: LOCAL_CONTEXT.to_string(),
            container_id: row.id.clone(),
            container_name: row.name.clone(),
            image: row.image.clone(),
            status: row.status.clone(),
            exec_args: runtime.exec_args(&row.id, CONTAINER_SHELL),
            exec_binary: binary_display.clone(),
            namespace: None,
        })
        .collect()
}

// ── Kubernetes JSON output structs ──────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct K8sPodList {
    items: Vec<K8sPod>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct K8sPod {
    metadata: K8sPodMeta,
    status: K8sPodStatus,
    spec: K8sPodSpec,
}

#[derive(Debug, Deserialize)]
struct K8sPodMeta {
    name: String,
    namespace: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct K8sPodStatus {
    phase: String,
    #[serde(default)]
    container_statuses: Option<Vec<K8sContainerStatus>>,
}

#[derive(Debug, Deserialize)]
struct K8sPodSpec {
    #[serde(default)]
    containers: Vec<K8sContainerSpec>,
}

#[derive(Debug, Deserialize)]
struct K8sContainerSpec {
    name: String,
    #[serde(default)]
    #[cfg_attr(not(test), allow(dead_code))]
    image: Option<String>,
}

#[derive(Debug, Deserialize)]
struct K8sContainerStatus {
    name: String,
    #[serde(default)]
    #[cfg_attr(not(test), allow(dead_code))]
    ready: bool,
    #[serde(default)]
    #[cfg_attr(not(test), allow(dead_code))]
    image: Option<String>,
}

// ── Discovery functions ─────────────────────────────────────────────────────

/// Normalize a namespace string.
/// Returns `None` for `"*"`, `"all"`, or empty — meaning "all namespaces".
fn normalize_namespace(ns: &str) -> Option<String> {
    let trimmed = ns.trim();
    if trimmed.is_empty() || trimmed == "*" || trimmed.eq_ignore_ascii_case("all") {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Resolve the kubectl binary path.
fn resolve_kubectl() -> Result<std::path::PathBuf> {
    resolve_executable_in("kubectl")
        .ok_or_else(|| anyhow!("kubectl binary not found in PATH or common locations"))
}

/// Run a command and return its stdout as a string, with a 15-second timeout.
///
/// Q1: Without a timeout, a hung Docker daemon or unreachable K8s API server
/// would block the entire discovery call indefinitely, potentially freezing
/// the desktop UI. The timeout kills the orphaned process if it doesn't
/// complete within the allotted time. The same applies to wslc, which starts a
/// WSL session on first use, and to Apple's `container`, which talks to a
/// system service.
fn run_capture(binary: &std::path::Path, args: &[&str]) -> Result<String> {
    let child = Command::new(binary)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to execute {}", binary.display()))?;

    const TIMEOUT: Duration = Duration::from_secs(15);

    // Store the PID before moving the Child into the wait thread, so we
    // can kill it if the timeout fires.
    let pid = child.id();

    let (tx, rx) = std::sync::mpsc::channel::<std::io::Result<std::process::Output>>();
    thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });

    match rx.recv_timeout(TIMEOUT) {
        Ok(Ok(output)) => {
            if !output.status.success() {
                return Err(anyhow!(
                    "{} {} failed with exit code {:?}: {}",
                    binary.display(),
                    args.join(" "),
                    output.status.code(),
                    String::from_utf8_lossy(&output.stderr).trim()
                ));
            }
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        }
        Ok(Err(e)) => Err(anyhow!(
            "failed to wait for {} {}: {e}",
            binary.display(),
            args.join(" ")
        )),
        Err(_) => {
            // Timeout — kill the process to avoid orphaned children.
            kill_process_by_pid(pid);
            Err(anyhow!(
                "{} {} timed out after {}s",
                binary.display(),
                args.join(" "),
                TIMEOUT.as_secs()
            ))
        }
    }
}

/// Kill a process by its PID (cross-platform best-effort).
fn kill_process_by_pid(pid: u32) {
    #[cfg(unix)]
    {
        let _ = Command::new("kill").arg("-9").arg(pid.to_string()).output();
    }
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .output();
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
    }
}

/// Discover containers managed by one local runtime.
///
/// `all` includes containers that are not running; the default mirrors the
/// historical Docker behaviour of listing running ones only.
fn discover_local_targets(
    runtime: ContainerRuntime,
    binary: &std::path::Path,
    all: bool,
) -> Result<Vec<ContainerDiscoveryTarget>> {
    let args = runtime.list_args(all);
    let raw = run_capture(binary, args).with_context(|| {
        format!(
            "{} list failed ({})",
            runtime.display_name(),
            args.join(" ")
        )
    })?;

    let (rows, skipped) = parse_local_rows(&raw, runtime);
    if skipped > 0 {
        let _ = crate::diagnostics::append_diagnostic_log(
            "warn",
            "container_discovery",
            "container list rows skipped",
            Some(serde_json::json!({
                "runtime": runtime.id(),
                "skipped": skipped,
                "parsed": rows.len(),
            })),
        );
    }

    Ok(targets_from_rows(runtime, binary, &rows))
}

/// List Kubernetes contexts from kubeconfig.
fn k8s_contexts(binary: &std::path::Path) -> Result<Vec<String>> {
    let raw = run_capture(binary, &["config", "get-contexts", "-o", "name"])?;
    let contexts: Vec<String> = raw
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    Ok(contexts)
}

/// Discover Kubernetes pods across all contexts (or a single context).
fn discover_k8s_targets(binary: &std::path::Path) -> Result<Vec<ContainerDiscoveryTarget>> {
    let contexts = k8s_contexts(binary)?;
    let mut targets = Vec::new();

    for ctx in &contexts {
        // Get all pods in all namespaces.
        let raw = match run_capture(
            binary,
            &["--context", ctx, "get", "pods", "-A", "-o", "json"],
        ) {
            Ok(s) => s,
            Err(e) => {
                // Q6: Log the failure so users can diagnose why pods from a
                // particular K8s context don't show up.
                let _ = crate::diagnostics::append_diagnostic_log(
                    "warn",
                    "container_discovery",
                    "kubectl get pods failed for context",
                    Some(serde_json::json!({
                        "context": ctx,
                        "error": e.to_string(),
                    })),
                );
                continue;
            }
        };

        let pod_list: K8sPodList = match serde_json::from_str(&raw) {
            Ok(p) => p,
            Err(e) => {
                let _ = crate::diagnostics::append_diagnostic_log(
                    "warn",
                    "container_discovery",
                    "kubectl get pods JSON parse error",
                    Some(serde_json::json!({
                        "context": ctx,
                        "error": e.to_string(),
                    })),
                );
                continue;
            }
        };

        for pod in &pod_list.items {
            if pod.status.phase != "Running" {
                continue;
            }
            // Finding 12: Normalize namespace (* → None for "all namespaces").
            let namespace = normalize_namespace(&pod.metadata.namespace);
            let pod_name = &pod.metadata.name;

            // Use container_statuses if available, otherwise fall back to spec.containers.
            let containers: Vec<(&str, Option<&str>)> = pod
                .status
                .container_statuses
                .as_ref()
                .map(|cs| {
                    cs.iter()
                        .map(|c| (c.name.as_str(), c.image.as_deref()))
                        .collect()
                })
                .unwrap_or_else(|| {
                    pod.spec
                        .containers
                        .iter()
                        .map(|c| (c.name.as_str(), c.image.as_deref()))
                        .collect()
                });

            // Finding 16: For multi-container pods, format name as "pod/container".
            let multi_container = containers.len() > 1;

            for (container_name, image) in &containers {
                let display_name = if multi_container {
                    format!("{pod_name}/{container_name}")
                } else {
                    container_name.to_string()
                };
                let target = ContainerDiscoveryTarget {
                    id: format!(
                        "kubectl_exec:{ctx}:{}:{pod_name}:{container_name}",
                        namespace.as_deref().unwrap_or("default")
                    ),
                    platform: ContainerPlatform::K8s,
                    context: ctx.clone(),
                    container_id: pod_name.clone(),
                    container_name: display_name,
                    image: image.unwrap_or_default().to_string(),
                    status: pod.status.phase.clone(),
                    exec_args: {
                        let mut args = vec!["--context".into(), ctx.clone(), "exec".into()];
                        if let Some(ns) = &namespace {
                            args.push("-n".into());
                            args.push(ns.clone());
                        }
                        args.push("-it".into());
                        args.push(pod_name.clone());
                        args.push("-c".into());
                        args.push(container_name.to_string());
                        args.push("--".into());
                        args.push(CONTAINER_SHELL.into());
                        args
                    },
                    exec_binary: binary.display().to_string(),
                    namespace: namespace.clone(),
                };
                targets.push(target);
            }
        }
    }

    Ok(targets)
}

/// What to discover, parsed from the CLI's `--runtime` flag or the Tauri
/// command's arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeFilter {
    /// Every local runtime that is installed, plus Kubernetes.
    Auto,
    /// One local runtime only.
    Local(ContainerRuntime),
    /// Kubernetes only.
    Kubernetes,
}

impl RuntimeFilter {
    /// Parse a filter. `None`, `""` and `"auto"` mean auto-detect.
    pub fn parse(value: Option<&str>) -> Result<Self> {
        let Some(raw) = value.map(str::trim).filter(|v| !v.is_empty()) else {
            return Ok(Self::Auto);
        };
        if raw.eq_ignore_ascii_case("auto") || raw.eq_ignore_ascii_case("all") {
            return Ok(Self::Auto);
        }
        if raw.eq_ignore_ascii_case("k8s") || raw.eq_ignore_ascii_case("kubernetes") {
            return Ok(Self::Kubernetes);
        }
        ContainerRuntime::from_id(raw)
            .map(Self::Local)
            .ok_or_else(|| {
                anyhow!(
                    "unknown container runtime {raw:?}; expected one of: auto, {}, k8s",
                    ContainerRuntime::ALL
                        .iter()
                        .map(|rt| rt.id())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })
    }
}

/// Discover container/pod exec targets.
///
/// * `all` — include containers that are not running (Kubernetes pods are
///   always filtered to `Running`).
/// * `runtime` — see [`RuntimeFilter::parse`]. Auto-detection skips runtimes
///   whose binary is missing; an explicitly requested runtime that is missing
///   or unsupported on this host is an error, because a caller that named one
///   wants to know.
///
/// Returns an empty list, not an error, when nothing is installed and no
/// runtime was requested — the caller can distinguish "nothing running" from
/// "no runtime here".
pub fn discover_containers(
    all: bool,
    runtime: Option<&str>,
) -> Result<Vec<ContainerDiscoveryTarget>> {
    let filter = RuntimeFilter::parse(runtime)?;
    let mut targets = Vec::new();

    match &filter {
        RuntimeFilter::Auto => {
            for resolved in container_runtime::available() {
                match discover_local_targets(resolved.runtime, &resolved.binary, all) {
                    Ok(mut found) => targets.append(&mut found),
                    Err(e) => log_discovery_failure(resolved.runtime.id(), &e),
                }
            }
            if let Ok(binary) = resolve_kubectl() {
                match discover_k8s_targets(&binary) {
                    Ok(mut found) => targets.append(&mut found),
                    Err(e) => log_discovery_failure("k8s", &e),
                }
            }
        }
        RuntimeFilter::Local(requested) => {
            let resolved = container_runtime::select(Some(requested.id()))?;
            targets = discover_local_targets(resolved.runtime, &resolved.binary, all)?;
        }
        RuntimeFilter::Kubernetes => {
            let binary = resolve_kubectl()?;
            targets = discover_k8s_targets(&binary)?;
        }
    }

    Ok(targets)
}

/// Log a per-runtime discovery failure. Auto-detection keeps going: one broken
/// runtime must not hide the containers of another.
fn log_discovery_failure(runtime: &str, error: &anyhow::Error) {
    let _ = crate::diagnostics::append_diagnostic_log(
        "warn",
        "container_discovery",
        "container discovery failed for runtime",
        Some(serde_json::json!({
            "runtime": runtime,
            "error": error.to_string(),
        })),
    );
}

/// Check whether one platform is usable on this host.
pub fn is_platform_available(platform: ContainerPlatform) -> bool {
    match platform.runtime() {
        Some(runtime) => runtime.resolve().is_some(),
        None => resolve_kubectl().is_ok(),
    }
}

/// Build the exec command for a discovered target.
///
/// Returns (binary_path, args) ready for `Command::new(binary).args(&args)`.
pub fn build_exec_command(target: &ContainerDiscoveryTarget) -> (&str, &[String]) {
    (&target.exec_binary, &target.exec_args)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(id: &str, platform: ContainerPlatform) -> ContainerDiscoveryTarget {
        ContainerDiscoveryTarget {
            id: id.into(),
            platform,
            context: LOCAL_CONTEXT.into(),
            container_id: "abc123".into(),
            container_name: "web".into(),
            image: "nginx:latest".into(),
            status: "Up".into(),
            exec_args: vec!["exec".into()],
            exec_binary: "docker".into(),
            namespace: None,
        }
    }

    #[test]
    fn target_id_is_unique() {
        let t1 = target("docker_exec:default:abc123", ContainerPlatform::Docker);
        let t2 = target(
            "kubectl_exec:default:default:my-pod:my-container",
            ContainerPlatform::K8s,
        );
        assert_ne!(t1.id, t2.id);
        assert_ne!(t1.platform, t2.platform);
    }

    #[test]
    fn platform_serializes_as_lowercase() {
        assert_eq!(
            serde_json::to_string(&ContainerPlatform::Docker).unwrap(),
            "\"docker\""
        );
        assert_eq!(
            serde_json::to_string(&ContainerPlatform::Wslc).unwrap(),
            "\"wslc\""
        );
        assert_eq!(
            serde_json::to_string(&ContainerPlatform::Apple).unwrap(),
            "\"apple\""
        );
        assert_eq!(
            serde_json::to_string(&ContainerPlatform::K8s).unwrap(),
            "\"k8s\""
        );
    }

    #[test]
    fn platform_deserializes_from_lowercase() {
        for (json, expected) in [
            ("\"docker\"", ContainerPlatform::Docker),
            ("\"wslc\"", ContainerPlatform::Wslc),
            ("\"apple\"", ContainerPlatform::Apple),
            ("\"k8s\"", ContainerPlatform::K8s),
        ] {
            let parsed: ContainerPlatform = serde_json::from_str(json).unwrap();
            assert_eq!(parsed, expected);
            assert_eq!(parsed.id(), expected.id());
        }
    }

    #[test]
    fn platform_maps_to_runtime_only_for_local_ones() {
        assert_eq!(
            ContainerPlatform::Docker.runtime(),
            Some(ContainerRuntime::Docker)
        );
        assert_eq!(
            ContainerPlatform::Wslc.runtime(),
            Some(ContainerRuntime::Wslc)
        );
        assert_eq!(
            ContainerPlatform::Apple.runtime(),
            Some(ContainerRuntime::Apple)
        );
        assert_eq!(ContainerPlatform::K8s.runtime(), None);
    }

    #[test]
    fn target_serializes_with_all_fields() {
        let mut t = target("docker_exec:default:abc123", ContainerPlatform::Docker);
        t.image = "nginx:latest".into();
        t.status = "Up 2 hours".into();
        t.exec_args = vec![
            "exec".into(),
            "-it".into(),
            "abc123".into(),
            "/bin/sh".into(),
        ];
        t.exec_binary = "/usr/local/bin/docker".into();
        let json = serde_json::to_string(&t).unwrap();
        let back: ContainerDiscoveryTarget = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, "docker_exec:default:abc123");
        assert_eq!(back.platform, ContainerPlatform::Docker);
        assert_eq!(back.container_name, "web");
        assert_eq!(back.image, "nginx:latest");
        assert!(back.namespace.is_none());
        assert_eq!(
            back.exec_argv(),
            vec!["/usr/local/bin/docker", "exec", "-it", "abc123", "/bin/sh"]
        );
    }

    #[test]
    fn build_exec_command_returns_binary_and_args() {
        let mut t = target("test", ContainerPlatform::Docker);
        t.exec_args = vec!["exec".into(), "-it".into(), "abc".into(), "/bin/sh".into()];
        t.exec_binary = "/usr/local/bin/docker".into();
        let (binary, args) = build_exec_command(&t);
        assert_eq!(binary, "/usr/local/bin/docker");
        assert_eq!(args, &["exec", "-it", "abc", "/bin/sh"]);
    }

    // ── Dialect parsing ────────────────────────────────────────────────────

    #[test]
    fn docker_jsonl_row_parses() {
        let raw = r#"{"Command":"\"/bin/sh\"","ID":"abc123def456","Image":"nginx:latest","Names":"web-server","State":"running","Status":"Up 2 hours"}"#;
        let (rows, skipped) = parse_local_rows(raw, ContainerRuntime::Docker);
        assert_eq!(skipped, 0);
        assert_eq!(
            rows,
            vec![LocalContainerRow {
                id: "abc123def456".into(),
                name: "web-server".into(),
                image: "nginx:latest".into(),
                status: "Up 2 hours".into(),
            }]
        );
    }

    /// Captured verbatim from `wslc ps -a --format json` (wslc 2.9.13) after
    /// `wslc create --name a2s-probe alpine:latest true`. The one long `Labels`
    /// value is elided; nothing in the parser reads it.
    const WSLC_JSONL: &str = r#"{"Command":"\"true\"","CreatedAt":"2026-09-25 15:33:58 +0800 GMT+8","HealthStatus":"","ID":"488611800bc0","Image":"alpine:latest","LocalVolumes":"0","Mounts":"","Names":"a2s-probe","Networks":"bridge","Ports":"","RunningFor":"Less than a second ago","Size":"0B","State":"created","Status":"Created"}"#;

    #[test]
    fn wslc_jsonl_row_parses_with_the_shared_docker_keys() {
        let (rows, skipped) = parse_local_rows(WSLC_JSONL, ContainerRuntime::Wslc);
        assert_eq!(skipped, 0);
        assert_eq!(
            rows,
            vec![LocalContainerRow {
                id: "488611800bc0".into(),
                name: "a2s-probe".into(),
                image: "alpine:latest".into(),
                // `Status` ("Created") wins over `State` ("created"): it is the
                // field the UI shows for Docker too.
                status: "Created".into(),
            }]
        );
    }

    #[test]
    fn wslc_multiple_lines_all_parse() {
        let raw = format!("{WSLC_JSONL}\n{WSLC_JSONL}");
        let (rows, skipped) = parse_local_rows(&raw, ContainerRuntime::Wslc);
        assert_eq!(rows.len(), 2);
        assert_eq!(skipped, 0);
    }

    /// Shaped after `ManagedContainer`:
    /// `id`, `configuration.image.reference`, `status.state`.
    const APPLE_ARRAY: &str = r#"[
      {
        "id": "a1b2c3d4-0000-4000-8000-000000000001",
        "configuration": {
          "id": "a1b2c3d4-0000-4000-8000-000000000001",
          "creationDate": "2026-09-25T07:00:00Z",
          "image": {
            "reference": "docker.io/library/alpine:latest",
            "descriptor": {
              "digest": "sha256:294b683cb724975bec92580e1e685676bd4b50bda910ddb8c51d4cabeaec77e6",
              "mediaType": "application/vnd.oci.image.index.v1+json"
            }
          },
          "initProcess": { "arguments": ["/bin/sh"] }
        },
        "status": {
          "state": "running",
          "networks": [],
          "startedDate": "2026-09-25T07:00:01Z"
        }
      }
    ]"#;

    #[test]
    fn apple_array_row_parses_from_nested_fields() {
        let (rows, skipped) = parse_local_rows(APPLE_ARRAY, ContainerRuntime::Apple);
        assert_eq!(skipped, 0);
        assert_eq!(
            rows,
            vec![LocalContainerRow {
                id: "a1b2c3d4-0000-4000-8000-000000000001".into(),
                // Apple containers have no separate name: the id doubles as one.
                name: "a1b2c3d4-0000-4000-8000-000000000001".into(),
                image: "docker.io/library/alpine:latest".into(),
                status: "running".into(),
            }]
        );
    }

    #[test]
    fn apple_raw_image_string_still_parses() {
        let raw = r#"[{"id":"c1","image":"alpine:latest","status":{"state":"stopped"}}]"#;
        let (rows, skipped) = parse_local_rows(raw, ContainerRuntime::Apple);
        assert_eq!(skipped, 0);
        assert_eq!(rows[0].image, "alpine:latest");
        assert_eq!(rows[0].status, "stopped");
    }

    #[test]
    fn empty_output_yields_no_rows_for_every_runtime() {
        for runtime in ContainerRuntime::ALL {
            let (rows, skipped) = parse_local_rows("   \n", runtime);
            assert!(rows.is_empty(), "{}", runtime.id());
            assert_eq!(skipped, 0, "{}", runtime.id());
        }
    }

    #[test]
    fn malformed_lines_are_skipped_without_losing_good_ones() {
        let raw = format!("not json\n{WSLC_JSONL}\n{{\"unrelated\":true}}");
        let (rows, skipped) = parse_local_rows(&raw, ContainerRuntime::Wslc);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "a2s-probe");
        // The junk line and the JSON object without an id.
        assert_eq!(skipped, 2);
    }

    #[test]
    fn empty_array_is_not_an_error() {
        for runtime in ContainerRuntime::ALL {
            let (rows, skipped) = parse_local_rows("[]", runtime);
            assert!(rows.is_empty());
            assert_eq!(skipped, 0);
        }
    }

    // ── Target construction ────────────────────────────────────────────────

    #[test]
    fn targets_carry_runtime_specific_ids_and_argv() {
        let rows = vec![LocalContainerRow {
            id: "488611800bc0".into(),
            name: "a2s-probe".into(),
            image: "alpine:latest".into(),
            status: "Created".into(),
        }];
        let binary = Path::new("/usr/bin/wslc");

        let wslc = targets_from_rows(ContainerRuntime::Wslc, binary, &rows);
        assert_eq!(wslc.len(), 1);
        assert_eq!(wslc[0].id, "wslc_exec:local:488611800bc0");
        assert_eq!(wslc[0].platform, ContainerPlatform::Wslc);
        assert_eq!(wslc[0].context, LOCAL_CONTEXT);
        assert_eq!(wslc[0].image, "alpine:latest");
        assert_eq!(
            wslc[0].exec_args,
            vec!["exec", "-it", "488611800bc0", "/bin/sh"]
        );

        let apple = targets_from_rows(
            ContainerRuntime::Apple,
            Path::new("/usr/local/bin/container"),
            &rows,
        );
        assert_eq!(apple[0].id, "apple_exec:local:488611800bc0");
        assert_eq!(apple[0].platform, ContainerPlatform::Apple);

        let docker = targets_from_rows(ContainerRuntime::Docker, Path::new("docker"), &rows);
        assert_eq!(docker[0].id, "docker_exec:local:488611800bc0");
        assert_eq!(docker[0].platform, ContainerPlatform::Docker);
    }

    #[test]
    fn local_target_ids_never_collide_across_runtimes() {
        let rows = vec![LocalContainerRow {
            id: "same-id".into(),
            name: "same-id".into(),
            image: String::new(),
            status: String::new(),
        }];
        let ids: Vec<String> = ContainerRuntime::ALL
            .iter()
            .flat_map(|rt| targets_from_rows(*rt, Path::new("bin"), &rows))
            .map(|t| t.id)
            .collect();
        let mut unique = ids.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(ids.len(), unique.len(), "{ids:?}");
    }

    // ── Filters ────────────────────────────────────────────────────────────

    #[test]
    fn runtime_filter_accepts_auto_local_and_k8s_spellings() {
        assert_eq!(RuntimeFilter::parse(None).unwrap(), RuntimeFilter::Auto);
        assert_eq!(RuntimeFilter::parse(Some("")).unwrap(), RuntimeFilter::Auto);
        assert_eq!(
            RuntimeFilter::parse(Some(" AUTO ")).unwrap(),
            RuntimeFilter::Auto
        );
        assert_eq!(
            RuntimeFilter::parse(Some("wslc")).unwrap(),
            RuntimeFilter::Local(ContainerRuntime::Wslc)
        );
        assert_eq!(
            RuntimeFilter::parse(Some("Apple")).unwrap(),
            RuntimeFilter::Local(ContainerRuntime::Apple)
        );
        assert_eq!(
            RuntimeFilter::parse(Some("kubernetes")).unwrap(),
            RuntimeFilter::Kubernetes
        );
    }

    #[test]
    fn runtime_filter_rejects_unknown_values_with_the_valid_list() {
        let error = RuntimeFilter::parse(Some("podman"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("podman"), "{error}");
        for runtime in ContainerRuntime::ALL {
            assert!(error.contains(runtime.id()), "{error}");
        }
        assert!(error.contains("k8s"), "{error}");
    }

    // ── Kubernetes fixtures ────────────────────────────────────────────────

    #[test]
    fn k8s_pod_list_parses_minimal_json() {
        let json = r#"{"items":[{"metadata":{"name":"my-pod","namespace":"default"},"status":{"phase":"Running"},"spec":{"containers":[{"name":"app","image":"app:1"}]}}]}"#;
        let list: K8sPodList = serde_json::from_str(json).unwrap();
        assert_eq!(list.items.len(), 1);
        assert_eq!(list.items[0].metadata.name, "my-pod");
        assert_eq!(list.items[0].metadata.namespace, "default");
        assert_eq!(list.items[0].status.phase, "Running");
        assert_eq!(list.items[0].spec.containers.len(), 1);
        assert_eq!(list.items[0].spec.containers[0].name, "app");
        assert_eq!(
            list.items[0].spec.containers[0].image.as_deref(),
            Some("app:1")
        );
    }

    #[test]
    fn k8s_pod_with_container_statuses_parses() {
        let json = r#"{"items":[{"metadata":{"name":"pod1","namespace":"ns1"},"status":{"phase":"Running","containerStatuses":[{"name":"c1","ready":true,"image":"c1:1"},{"name":"c2","ready":false}]},"spec":{"containers":[]}}]}"#;
        let list: K8sPodList = serde_json::from_str(json).unwrap();
        let pod = &list.items[0];
        assert!(pod.status.container_statuses.is_some());
        let cs = pod.status.container_statuses.as_ref().unwrap();
        assert_eq!(cs.len(), 2);
        assert_eq!(cs[0].name, "c1");
        assert!(cs[0].ready);
        assert!(!cs[1].ready);
        assert_eq!(cs[0].image.as_deref(), Some("c1:1"));
    }

    // ── End-to-end (host-dependent) ────────────────────────────────────────

    #[test]
    fn discover_containers_never_errors_without_any_runtime() {
        // On a machine with no container CLI this returns an empty vec; where a
        // runtime exists it returns real targets. Either way it must not error.
        let targets = discover_containers(false, None).unwrap();
        assert!(targets.iter().all(|t| !t.id.is_empty()));
        assert!(targets.iter().all(|t| !t.exec_binary.is_empty()));
        assert!(targets.iter().all(|t| t.exec_args.len() >= 4));
    }

    #[test]
    fn discover_containers_errors_only_when_a_named_runtime_is_missing() {
        // "podman" is never a known runtime: the filter must reject it.
        assert!(discover_containers(false, Some("podman")).is_err());
        // A supported-but-absent runtime is an error only for a named request;
        // on this host wslc/Apple may legitimately be absent.
        for runtime in ContainerRuntime::ALL {
            let result = discover_containers(false, Some(runtime.id()));
            if runtime.supported_on_host() && runtime.resolve().is_some() {
                assert!(result.is_ok(), "{} should be discoverable", runtime.id());
            } else {
                assert!(result.is_err(), "{} should report why", runtime.id());
            }
        }
    }

    #[test]
    fn is_platform_available_does_not_panic() {
        let _ = is_platform_available(ContainerPlatform::Docker);
        let _ = is_platform_available(ContainerPlatform::Wslc);
        let _ = is_platform_available(ContainerPlatform::Apple);
        let _ = is_platform_available(ContainerPlatform::K8s);
    }
}
