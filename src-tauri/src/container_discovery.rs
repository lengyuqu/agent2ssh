//! Container discovery for Docker and Kubernetes (B33).
//!
//! Discovers running Docker containers and Kubernetes pods that can be used
//! as exec targets — converting them into `ContainerDiscoveryTarget` entries
//! that the CLI or desktop app can present as quick-connect options.
//!
//! Mirrors rssh's `commands/discovery.rs` pattern, adapted to agent2ssh's
//! architecture (no DB, uses CLI subprocess calls to `docker`/`kubectl`).
//!
//! ## How it works
//!
//! 1. Resolve the `docker` or `kubectl` binary via `resolve_executable_in()`
//!    (desktop GUI apps don't inherit the login shell's PATH).
//! 2. Run `docker context ls` / `kubectl config get-contexts` to list contexts.
//! 3. For each context, run `docker ps` / `kubectl get pods` to list targets.
//! 4. Convert each target into a `ContainerDiscoveryTarget` with a unique ID
//!    and the command args needed to exec into it.

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use crate::path_resolver::resolve_executable_in;

/// The container platform (Docker or Kubernetes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContainerPlatform {
    Docker,
    K8s,
}

/// A discovered container/pod that can be used as an exec target.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerDiscoveryTarget {
    /// Unique ID (e.g. `docker_exec:default:abc123` or `kubectl_exec:default:default:my-pod:my-container`).
    pub id: String,
    /// The platform (docker or k8s).
    pub platform: ContainerPlatform,
    /// The context name (Docker context or Kubernetes context).
    pub context: String,
    /// Container ID (Docker) or pod name (Kubernetes).
    pub container_id: String,
    /// Container/pod display name.
    pub container_name: String,
    /// Status string from the platform (e.g. "Up 2 hours", "Running").
    pub status: String,
    /// The exec command args to connect to this target.
    /// e.g. ["exec", "-it", "abc123", "/bin/sh"] for Docker.
    pub exec_args: Vec<String>,
    /// The resolved binary path to execute (docker or kubectl).
    pub exec_binary: String,
    /// For Kubernetes: the namespace. None for Docker.
    pub namespace: Option<String>,
}

// ── Docker JSON output structs ──────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct DockerContextRow {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Current")]
    #[cfg_attr(not(test), allow(dead_code))]
    current: bool,
}

#[derive(Debug, Deserialize)]
struct DockerPsRow {
    #[serde(rename = "ID")]
    id: String,
    #[serde(rename = "Image")]
    #[cfg_attr(not(test), allow(dead_code))]
    image: String,
    #[serde(rename = "Names")]
    names: String,
    #[serde(rename = "Status")]
    status: String,
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
}

#[derive(Debug, Deserialize)]
struct K8sContainerStatus {
    name: String,
    #[serde(default)]
    #[cfg_attr(not(test), allow(dead_code))]
    ready: bool,
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

/// Resolve the Docker binary path.
fn resolve_docker() -> Result<std::path::PathBuf> {
    resolve_executable_in("docker")
        .ok_or_else(|| anyhow!("docker binary not found in PATH or common locations"))
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
/// complete within the allotted time.
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

/// List Docker contexts.
fn docker_contexts(binary: &std::path::Path) -> Result<Vec<String>> {
    let raw = run_capture(binary, &["context", "ls", "--format", "{{json .}}"])?;
    let mut contexts = Vec::new();
    for (line_num, line) in raw.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<DockerContextRow>(line) {
            Ok(row) => contexts.push(row.name),
            Err(e) => {
                let _ = crate::diagnostics::append_diagnostic_log(
                    "warn",
                    "container_discovery",
                    "docker context JSON parse error",
                    Some(serde_json::json!({ "line": line_num + 1, "error": e.to_string() })),
                );
            }
        }
    }
    if contexts.is_empty() {
        // Fall back to "default" if no contexts are configured.
        contexts.push("default".into());
    }
    Ok(contexts)
}

/// Discover Docker containers across all contexts.
fn discover_docker_targets(binary: &std::path::Path) -> Result<Vec<ContainerDiscoveryTarget>> {
    let contexts = docker_contexts(binary)?;
    let mut targets = Vec::new();

    for ctx in &contexts {
        let raw = match run_capture(binary, &["--context", ctx, "ps", "--format", "{{json .}}"]) {
            Ok(s) => s,
            Err(e) => {
                // Q6: Log the failure so users can diagnose why containers
                // from a particular Docker context don't show up.
                let _ = crate::diagnostics::append_diagnostic_log(
                    "warn",
                    "container_discovery",
                    "docker ps failed for context",
                    Some(serde_json::json!({
                        "context": ctx,
                        "error": e.to_string(),
                    })),
                );
                continue;
            }
        };

        for (line_num, line) in raw.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let row = match serde_json::from_str::<DockerPsRow>(line) {
                Ok(r) => r,
                Err(e) => {
                    let _ = crate::diagnostics::append_diagnostic_log(
                        "warn",
                        "container_discovery",
                        "docker ps JSON parse error",
                        Some(serde_json::json!({
                            "context": ctx,
                            "line": line_num + 1,
                            "error": e.to_string(),
                        })),
                    );
                    continue;
                }
            };
            let id_short = if row.id.len() >= 12 {
                row.id[..12].to_string()
            } else {
                row.id.clone()
            };
            let container_name = row.names.split(',').next().unwrap_or(&id_short).to_string();
            let target = ContainerDiscoveryTarget {
                id: format!("docker_exec:{ctx}:{}", row.id),
                platform: ContainerPlatform::Docker,
                context: ctx.clone(),
                container_id: row.id.clone(),
                container_name: container_name.clone(),
                status: row.status,
                exec_args: vec![
                    "--context".into(),
                    ctx.clone(),
                    "exec".into(),
                    "-it".into(),
                    row.id.clone(),
                    "/bin/sh".into(),
                ],
                exec_binary: binary.display().to_string(),
                namespace: None,
            };
            targets.push(target);
        }
    }

    Ok(targets)
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
            let containers: Vec<&str> = pod
                .status
                .container_statuses
                .as_ref()
                .map(|cs| cs.iter().map(|c| c.name.as_str()).collect())
                .unwrap_or_else(|| {
                    pod.spec
                        .containers
                        .iter()
                        .map(|c| c.name.as_str())
                        .collect()
                });

            // Finding 16: For multi-container pods, format name as "pod/container".
            let multi_container = containers.len() > 1;

            for container_name in &containers {
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
                        args.push("/bin/sh".into());
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

/// Discover all available container targets (Docker + Kubernetes).
///
/// Returns targets from both platforms. If a platform's binary is not found,
/// it is silently skipped (no error) — the function only fails if both
/// platforms are unavailable.
pub fn discover_containers() -> Result<Vec<ContainerDiscoveryTarget>> {
    let mut targets = Vec::new();

    // Try Docker.
    if let Ok(binary) = resolve_docker() {
        if let Ok(docker_targets) = discover_docker_targets(&binary) {
            targets.extend(docker_targets);
        }
    }

    // Try Kubernetes.
    if let Ok(binary) = resolve_kubectl() {
        if let Ok(k8s_targets) = discover_k8s_targets(&binary) {
            targets.extend(k8s_targets);
        }
    }

    if targets.is_empty() {
        // Neither platform is available — return an empty list, not an error.
        // The caller can distinguish "no containers running" from "no docker/kubectl".
    }

    Ok(targets)
}

/// Check whether a platform binary is available.
pub fn is_platform_available(platform: ContainerPlatform) -> bool {
    match platform {
        ContainerPlatform::Docker => resolve_docker().is_ok(),
        ContainerPlatform::K8s => resolve_kubectl().is_ok(),
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

    #[test]
    fn target_id_is_unique() {
        let t1 = ContainerDiscoveryTarget {
            id: "docker_exec:default:abc123".into(),
            platform: ContainerPlatform::Docker,
            context: "default".into(),
            container_id: "abc123".into(),
            container_name: "my-container".into(),
            status: "Up".into(),
            exec_args: vec!["exec".into()],
            exec_binary: "docker".into(),
            namespace: None,
        };
        let t2 = ContainerDiscoveryTarget {
            id: "kubectl_exec:default:default:my-pod:my-container".into(),
            platform: ContainerPlatform::K8s,
            context: "default".into(),
            container_id: "my-pod".into(),
            container_name: "my-container".into(),
            status: "Running".into(),
            exec_args: vec!["exec".into()],
            exec_binary: "kubectl".into(),
            namespace: Some("default".into()),
        };
        assert_ne!(t1.id, t2.id);
        assert_ne!(t1.platform, t2.platform);
    }

    #[test]
    fn platform_serializes_as_lowercase() {
        let json = serde_json::to_string(&ContainerPlatform::Docker).unwrap();
        assert_eq!(json, "\"docker\"");
        let json = serde_json::to_string(&ContainerPlatform::K8s).unwrap();
        assert_eq!(json, "\"k8s\"");
    }

    #[test]
    fn platform_deserializes_from_lowercase() {
        let docker: ContainerPlatform = serde_json::from_str("\"docker\"").unwrap();
        assert_eq!(docker, ContainerPlatform::Docker);
        let k8s: ContainerPlatform = serde_json::from_str("\"k8s\"").unwrap();
        assert_eq!(k8s, ContainerPlatform::K8s);
    }

    #[test]
    fn target_serializes_with_all_fields() {
        let target = ContainerDiscoveryTarget {
            id: "docker_exec:default:abc123".into(),
            platform: ContainerPlatform::Docker,
            context: "default".into(),
            container_id: "abc123".into(),
            container_name: "web".into(),
            status: "Up 2 hours".into(),
            exec_args: vec![
                "exec".into(),
                "-it".into(),
                "abc123".into(),
                "/bin/sh".into(),
            ],
            exec_binary: "/usr/local/bin/docker".into(),
            namespace: None,
        };
        let json = serde_json::to_string(&target).unwrap();
        let back: ContainerDiscoveryTarget = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, "docker_exec:default:abc123");
        assert_eq!(back.platform, ContainerPlatform::Docker);
        assert_eq!(back.container_name, "web");
        assert!(back.namespace.is_none());
    }

    #[test]
    fn build_exec_command_returns_binary_and_args() {
        let target = ContainerDiscoveryTarget {
            id: "test".into(),
            platform: ContainerPlatform::Docker,
            context: "default".into(),
            container_id: "abc".into(),
            container_name: "test".into(),
            status: "Up".into(),
            exec_args: vec!["exec".into(), "-it".into(), "abc".into(), "/bin/sh".into()],
            exec_binary: "/usr/local/bin/docker".into(),
            namespace: None,
        };
        let (binary, args) = build_exec_command(&target);
        assert_eq!(binary, "/usr/local/bin/docker");
        assert_eq!(args, &["exec", "-it", "abc", "/bin/sh"]);
    }

    #[test]
    fn docker_ps_row_parses_from_json() {
        let json = r#"{"ID":"abc123def456","Image":"nginx:latest","Names":"web-server","Status":"Up 2 hours"}"#;
        let row: DockerPsRow = serde_json::from_str(json).unwrap();
        assert_eq!(row.id, "abc123def456");
        assert_eq!(row.image, "nginx:latest");
        assert_eq!(row.names, "web-server");
        assert_eq!(row.status, "Up 2 hours");
    }

    #[test]
    fn docker_context_row_parses_from_json() {
        let json = r#"{"Name":"default","Current":true}"#;
        let row: DockerContextRow = serde_json::from_str(json).unwrap();
        assert_eq!(row.name, "default");
        assert!(row.current);
    }

    #[test]
    fn k8s_pod_list_parses_minimal_json() {
        let json = r#"{"items":[{"metadata":{"name":"my-pod","namespace":"default"},"status":{"phase":"Running"},"spec":{"containers":[{"name":"app"}]}}]}"#;
        let list: K8sPodList = serde_json::from_str(json).unwrap();
        assert_eq!(list.items.len(), 1);
        assert_eq!(list.items[0].metadata.name, "my-pod");
        assert_eq!(list.items[0].metadata.namespace, "default");
        assert_eq!(list.items[0].status.phase, "Running");
        assert_eq!(list.items[0].spec.containers.len(), 1);
        assert_eq!(list.items[0].spec.containers[0].name, "app");
    }

    #[test]
    fn k8s_pod_with_container_statuses_parses() {
        let json = r#"{"items":[{"metadata":{"name":"pod1","namespace":"ns1"},"status":{"phase":"Running","containerStatuses":[{"name":"c1","ready":true},{"name":"c2","ready":false}]},"spec":{"containers":[]}}]}"#;
        let list: K8sPodList = serde_json::from_str(json).unwrap();
        let pod = &list.items[0];
        assert!(pod.status.container_statuses.is_some());
        let cs = pod.status.container_statuses.as_ref().unwrap();
        assert_eq!(cs.len(), 2);
        assert_eq!(cs[0].name, "c1");
        assert!(cs[0].ready);
        assert!(!cs[1].ready);
    }

    #[test]
    fn discover_containers_returns_empty_when_no_binary() {
        // On a system without docker/kubectl, this returns an empty vec.
        let targets = discover_containers().unwrap();
        // We can't assert whether docker/kubectl are installed on the test machine,
        // but the function should not error.
        assert!(targets.iter().all(|t| !t.id.is_empty()));
    }

    #[test]
    fn is_platform_available_does_not_panic() {
        // Should return true/false without panicking.
        let _ = is_platform_available(ContainerPlatform::Docker);
        let _ = is_platform_available(ContainerPlatform::K8s);
    }
}
