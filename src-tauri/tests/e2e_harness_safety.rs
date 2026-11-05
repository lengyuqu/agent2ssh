#![cfg(unix)]

//! The SSH E2E scripts must never remove someone else's container, including
//! when a runtime rejects a name or a fixture fails before becoming ready.
//! All runtime and CLI calls here are mocked; no container runtime is invoked.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

struct Harness {
    dir: tempfile::TempDir,
    cli: PathBuf,
    log: PathBuf,
}

impl Harness {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        fs::create_dir(&bin).unwrap();
        let cli = bin.join("agent2ssh");
        let runtime = bin.join("runtime");
        let log = dir.path().join("runtime.log");
        script(
            &runtime,
            r#"#!/bin/bash
printf '%s\n' "$*" >> "$FAKE_LOG"
if [[ "$1" == run ]]; then
  if [[ "${FAKE_RUN_FAIL:-0}" == 1 ]]; then exit 71; fi
  name=""; cidfile=""
  while [[ $# -gt 0 ]]; do
    case "$1" in --name) name="$2"; shift 2 ;; --cidfile) cidfile="$2"; shift 2 ;; *) shift ;; esac
  done
  if [[ -f "$FAKE_DIR/fail-second" && "$name" == *-2 ]]; then exit 71; fi
  if [[ -f "$FAKE_DIR/no-cid" || ( -f "$FAKE_DIR/legacy-no-cid" && "$name" == *-legacy ) ]]; then echo no-id; exit 0; fi
  if [[ -f "$FAKE_DIR/crlf-cid" ]]; then printf 'fake-id-%s\r\n' "$name" > "$cidfile"; else printf 'fake-id-%s\n' "$name" > "$cidfile"; fi
  if [[ -f "$FAKE_DIR/fail-after-create" ]]; then exit 72; fi
  echo "fake-id-$name"
fi
exit 0
"#,
        );
        script(
            &cli,
            r#"#!/bin/bash
case "$1 $2" in
  'containers runtime') if [[ "$3" == --binary ]]; then printf '%s\n' "$FAKE_RUNTIME"; else printf '{"runtime": "docker"}\n'; fi ;;
  'keys algorithm') echo ed25519 ;;
  'keys generate') mkdir -p "$AGENT2SSH_CONFIG_DIR/keys"; : > "$AGENT2SSH_CONFIG_DIR/keys/$3"; printf 'ssh-ed25519 fake\n' > "$AGENT2SSH_CONFIG_DIR/keys/$3.pub" ;;
  'host add') [[ "${FAKE_FIXTURE:-0}" != 1 ]] ;;
  'exec '*) if [[ -f "$FAKE_DIR/agent-success" ]]; then printf '%s\n' "${SSH_AUTH_SOCK:-none}" >> "$FAKE_CLI_AGENT_LOG"; fi; if [[ -f "$FAKE_DIR/matrix-ready" ]]; then echo matrix-ok; else exit 1; fi ;;
  *) exit 0 ;;
esac
"#,
        );
        // The matrix generates keys before it starts a server. Stub keygen and
        // agent tools so the test never touches a real agent or private key.
        script(
            &bin.join("ssh-keygen"),
            r#"#!/bin/bash
file=""
while [[ $# -gt 0 ]]; do
  if [[ "$1" == -f ]]; then file="$2"; shift 2; else shift; fi
done
if [[ -n "$file" ]]; then : > "$file"; printf 'ssh-ed25519 fake\n' > "$file.pub"; fi
exit 0
"#,
        );
        script(
            &bin.join("ssh-agent"),
            r#"#!/bin/bash
if [[ -f "$FAKE_DIR/agent-success" ]]; then
  if [[ "$1" == -s ]]; then
    printf 'SSH_AUTH_SOCK=%q; export SSH_AUTH_SOCK; SSH_AGENT_PID=98765; export SSH_AGENT_PID;\n' "$FAKE_DIR/owned-agent.sock"
  fi
  exit 0
fi
exit 1
"#,
        );
        script(
            &bin.join("ssh-add"),
            r#"#!/bin/bash
printf '%s\n' "${SSH_AUTH_SOCK:-none}" >> "$FAKE_AGENT_LOG"
[[ -f "$FAKE_DIR/agent-success" ]]
"#,
        );
        script(&bin.join("sleep"), "#!/bin/bash\nexit 0\n");
        for command in ["PerSourcePenalties", "MaxStartups"] {
            script(
                &bin.join(command),
                "#!/bin/bash\nprintf called >> \"$FAKE_SUBSTITUTION_LOG\"\nexit 0\n",
            );
        }
        Self { dir, cli, log }
    }

    fn run(
        &self,
        script_name: &str,
        fail_run: bool,
        fixture: bool,
        servers: &str,
    ) -> (bool, Vec<String>) {
        self.run_with_strict(script_name, fail_run, fixture, servers, false)
    }

    fn run_with_strict(
        &self,
        script_name: &str,
        fail_run: bool,
        fixture: bool,
        servers: &str,
        strict: bool,
    ) -> (bool, Vec<String>) {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let path = if self.dir.path().join("no-keygen").exists() {
            let isolated = self.dir.path().join("isolated-path");
            fs::create_dir_all(&isolated).unwrap();
            for name in ["dirname", "mktemp", "mkdir", "sed", "cat", "rm"] {
                let executable =
                    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                        .map(|dir| dir.join(name))
                        .find(|path| path.is_file())
                        .expect("required utility");
                std::os::unix::fs::symlink(executable, isolated.join(name)).unwrap();
            }
            isolated.to_string_lossy().into_owned()
        } else {
            format!(
                "{}:{}",
                self.dir.path().join("bin").display(),
                std::env::var("PATH").unwrap_or_default()
            )
        };
        let result = Command::new("/bin/bash")
            .arg(root.join("scripts").join(script_name))
            .env("AGENT2SSH_CLI", &self.cli)
            .env("FAKE_RUNTIME", self.dir.path().join("bin/runtime"))
            .env("FAKE_LOG", &self.log)
            .env("FAKE_AGENT_LOG", self.dir.path().join("agent.log"))
            .env("FAKE_CLI_AGENT_LOG", self.dir.path().join("cli-agent.log"))
            .env(
                "FAKE_SUBSTITUTION_LOG",
                self.dir.path().join("substitution.log"),
            )
            .env("FAKE_DIR", self.dir.path())
            .env("FAKE_RUN_FAIL", if fail_run { "1" } else { "0" })
            .env("FAKE_FIXTURE", if fixture { "1" } else { "0" })
            .env("AGENT2SSH_MATRIX_SERVERS", servers)
            .env(
                "AGENT2SSH_MATRIX_LEGACY",
                if self.dir.path().join("legacy-on").exists() {
                    "1"
                } else {
                    "0"
                },
            )
            .env("AGENT2SSH_E2E_PORT", "32123")
            .env(
                "AGENT2SSH_MATRIX_REQUIRE_FULL",
                if strict { "1" } else { "0" },
            )
            .env("PATH", path)
            .env("HOME", self.dir.path())
            // A caller's agent must not be used when the disposable one fails.
            .env("SSH_AUTH_SOCK", self.dir.path().join("caller-agent.sock"))
            .env("SSH_AGENT_PID", "12345")
            .output()
            .expect("run harness script");
        fs::write(self.dir.path().join("stderr.log"), &result.stderr).unwrap();
        fs::write(self.dir.path().join("stdout.log"), &result.stdout).unwrap();
        let log = fs::read_to_string(&self.log).unwrap_or_default();
        fs::write(&self.log, "").unwrap();
        (
            result.status.success(),
            log.lines().map(str::to_owned).collect(),
        )
    }
}

fn script(path: &Path, source: &str) {
    fs::write(path, source).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

fn run_name(line: &str) -> &str {
    let args: Vec<_> = line.split_whitespace().collect();
    let pos = args
        .iter()
        .position(|arg| *arg == "--name")
        .expect("container name");
    args[pos + 1]
}

#[test]
fn fixture_removes_only_its_successfully_created_container() {
    let harness = Harness::new();
    let (success, failed_log) = harness.run("e2e-container.sh", true, true, "");
    assert!(!success);
    assert_eq!(
        failed_log.len(),
        1,
        "a failed run must not remove any container: {failed_log:?}"
    );
    let failed_name = run_name(&failed_log[0]).to_string();

    let (success, log) = harness.run("e2e-container.sh", false, true, "");
    assert!(!success, "the mocked host registration fails intentionally");
    assert_eq!(log.len(), 2, "{log:?}");
    assert!(
        log[0].contains("-p 32123:2222"),
        "port override must reach the runtime: {log:?}"
    );
    let created = run_name(&log[0]);
    assert_ne!(created, failed_name, "each run must use a different name");
    assert_eq!(log[1], format!("rm -f fake-id-{created}"));
    assert_ne!(created, "agent2ssh-e2e-sshd");
}

#[test]
fn crlf_container_id_is_cleaned_without_a_carriage_return() {
    let harness = Harness::new();
    fs::write(harness.dir.path().join("crlf-cid"), "").unwrap();
    for (script_name, fixture) in [("e2e-container.sh", true), ("e2e-ssh-matrix.sh", false)] {
        let (success, log) = harness.run(script_name, false, fixture, "openssh:10.3");
        assert!(!success);
        let run = log.iter().find(|line| line.starts_with("run ")).unwrap();
        let name = run_name(run);
        assert!(
            log.iter()
                .any(|line| line == &format!("rm -f fake-id-{name}")),
            "{log:?}"
        );
    }
}

#[test]
fn missing_container_id_fails_without_removing_by_name() {
    let harness = Harness::new();
    fs::write(harness.dir.path().join("no-cid"), "").unwrap();
    for (script_name, fixture) in [("e2e-container.sh", true), ("e2e-ssh-matrix.sh", false)] {
        let (success, log) = harness.run(script_name, false, fixture, "openssh:10.3");
        assert!(!success, "missing runtime ID must not be a green run");
        assert_eq!(log.len(), 1, "no name-based cleanup is safe: {log:?}");
        assert!(log[0].starts_with("run "), "{log:?}");
    }
}

#[test]
fn a_container_created_before_runtime_failure_is_removed_by_its_recorded_id() {
    let harness = Harness::new();
    fs::write(harness.dir.path().join("fail-after-create"), "").unwrap();
    for (script_name, fixture) in [("e2e-container.sh", true), ("e2e-ssh-matrix.sh", false)] {
        let (success, log) = harness.run(script_name, false, fixture, "openssh:10.3");
        assert!(!success);
        assert_eq!(log.len(), 2, "{log:?}");
        let name = run_name(&log[0]);
        assert_eq!(log[1], format!("rm -f fake-id-{name}"));
    }
}

#[test]
fn matrix_cleans_each_owned_container_and_never_removes_failed_runs() {
    let harness = Harness::new();
    let (success, failed) = harness.run("e2e-ssh-matrix.sh", true, false, "openssh:10.3");
    assert!(!success);
    assert_eq!(
        failed.len(),
        1,
        "a failed run must not remove any container: {failed:?}"
    );
    let failed_name = run_name(&failed[0]).to_string();

    let (success, log) = harness.run(
        "e2e-ssh-matrix.sh",
        false,
        false,
        "openssh:10.3 openssh:9.9",
    );
    assert!(!success, "mocked SSH readiness must fail");
    let runs: Vec<_> = log.iter().filter(|line| line.starts_with("run ")).collect();
    let removals: Vec<_> = log
        .iter()
        .filter(|line| line.starts_with("rm -f "))
        .collect();
    assert_eq!(runs.len(), 2, "{log:?}");
    assert_eq!(removals.len(), 2, "{log:?}");
    let first = run_name(runs[0]);
    let second = run_name(runs[1]);
    assert_ne!(first, second);
    assert_ne!(first, failed_name);
    assert_eq!(removals[0], &format!("rm -f fake-id-{first}"));
    assert_eq!(removals[1], &format!("rm -f fake-id-{second}"));
    assert!(
        fs::read_to_string(harness.dir.path().join("agent.log"))
            .unwrap_or_default()
            .is_empty(),
        "ssh-add must not run when the disposable agent fails to start"
    );
    assert!(
        log.iter().position(|line| line == removals[0]).unwrap()
            < log.iter().position(|line| line == runs[1]).unwrap(),
        "first fixture must be removed before the next starts: {log:?}"
    );
}

#[test]
fn matrix_cleans_first_fixture_when_the_second_run_fails() {
    let harness = Harness::new();
    fs::write(harness.dir.path().join("fail-second"), "").unwrap();
    let (success, log) = harness.run(
        "e2e-ssh-matrix.sh",
        false,
        false,
        "openssh:10.3 openssh:9.9",
    );
    assert!(!success, "the second runtime run must fail");
    let runs: Vec<_> = log.iter().filter(|line| line.starts_with("run ")).collect();
    let removals: Vec<_> = log
        .iter()
        .filter(|line| line.starts_with("rm -f "))
        .collect();
    assert_eq!(runs.len(), 2, "{log:?}");
    assert_eq!(
        removals,
        vec![&format!("rm -f fake-id-{}", run_name(runs[0]))],
        "{log:?}"
    );
    assert!(
        log.iter().position(|line| line == removals[0]).unwrap()
            < log.iter().position(|line| line == runs[1]).unwrap(),
        "{log:?}"
    );
}

#[test]
fn legacy_missing_id_cleans_previous_fixture_without_removing_by_name() {
    let harness = Harness::new();
    fs::write(harness.dir.path().join("legacy-on"), "").unwrap();
    fs::write(harness.dir.path().join("legacy-no-cid"), "").unwrap();
    let (success, log) = harness.run("e2e-ssh-matrix.sh", false, false, "openssh:10.3");
    let output = fs::read_to_string(harness.dir.path().join("stdout.log")).unwrap();
    assert!(
        !success && output.contains("runtime did not record a usable fixture ID"),
        "{output}"
    );
    assert!(
        output.contains("Summary"),
        "failed legacy fixture must still report the summary: {output}"
    );
    let runs: Vec<_> = log.iter().filter(|line| line.starts_with("run ")).collect();
    let removals: Vec<_> = log
        .iter()
        .filter(|line| line.starts_with("rm -f "))
        .collect();
    assert_eq!(runs.len(), 2, "{log:?}");
    assert_eq!(
        removals,
        vec![&format!("rm -f fake-id-{}", run_name(runs[0]))],
        "{log:?}"
    );
    assert!(
        log.iter().position(|line| line == removals[0]).unwrap()
            < log.iter().position(|line| line == runs[1]).unwrap(),
        "{log:?}"
    );
}

#[test]
fn matrix_exports_only_the_disposable_agent_to_children() {
    let harness = Harness::new();
    fs::write(harness.dir.path().join("agent-success"), "").unwrap();
    let (success, log) =
        harness.run_with_strict("e2e-ssh-matrix.sh", false, false, "openssh:10.3", true);
    assert!(!success, "mocked SSH readiness fails deliberately");
    assert!(
        log.iter().any(|line| line.starts_with("run ")),
        "preflight should accept the test agent: {log:?}"
    );
    let agent_log = fs::read_to_string(harness.dir.path().join("agent.log")).unwrap();
    assert_eq!(
        agent_log.trim(),
        harness
            .dir
            .path()
            .join("owned-agent.sock")
            .to_str()
            .unwrap()
    );
    let cli_log = fs::read_to_string(harness.dir.path().join("cli-agent.log")).unwrap();
    assert!(!cli_log.is_empty(), "readiness must invoke the CLI");
    assert!(
        cli_log.lines().all(|socket| socket
            == harness
                .dir
                .path()
                .join("owned-agent.sock")
                .to_str()
                .unwrap()),
        "CLI children must receive the disposable agent, never the caller's socket: {cli_log}"
    );
}

#[test]
fn matrix_does_not_execute_remote_comment_words_on_the_host() {
    let harness = Harness::new();
    fs::write(harness.dir.path().join("matrix-ready"), "").unwrap();
    let (success, log) = harness.run("e2e-ssh-matrix.sh", false, false, "openssh:10.3");
    assert!(
        !success,
        "the fake SSH client deliberately fails negative checks"
    );
    assert!(
        log.iter().any(|line| line.starts_with("exec ")),
        "server setup must be reached: {log:?}"
    );
    assert!(
        fs::read_to_string(harness.dir.path().join("substitution.log"))
            .unwrap_or_default()
            .is_empty(),
        "remote comments must not invoke host commands"
    );
}

#[test]
fn strict_matrix_rejects_missing_keygen_before_starting_a_container() {
    let harness = Harness::new();
    fs::write(harness.dir.path().join("no-keygen"), "").unwrap();
    let (success, log) =
        harness.run_with_strict("e2e-ssh-matrix.sh", false, false, "openssh:10.3", true);
    let stderr = fs::read_to_string(harness.dir.path().join("stderr.log")).unwrap();
    assert!(
        !success && stderr.contains("requires ssh-keygen"),
        "{stderr}"
    );
    assert!(
        log.is_empty(),
        "strict preflight must not start a container: {log:?}"
    );
}

#[test]
fn strict_matrix_rejects_an_unavailable_test_agent_before_starting_a_container() {
    let harness = Harness::new();
    let (success, log) =
        harness.run_with_strict("e2e-ssh-matrix.sh", false, false, "openssh:10.3", true);
    assert!(!success);
    assert!(
        log.is_empty(),
        "strict preflight must not start a container: {log:?}"
    );
    assert!(
        fs::read_to_string(harness.dir.path().join("agent.log"))
            .unwrap_or_default()
            .is_empty(),
        "the caller's agent must not receive a key when the test agent fails"
    );
}

#[test]
fn matrix_rejects_unknown_or_empty_server_lists() {
    let harness = Harness::new();
    for servers in ["not-a-runtime", " "] {
        let (success, log) = harness.run("e2e-ssh-matrix.sh", false, false, servers);
        assert!(
            !success,
            "no executed checks must not be reported as success"
        );
        assert!(
            log.is_empty(),
            "unknown servers must not start or remove containers: {log:?}"
        );
    }
}
