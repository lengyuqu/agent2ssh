//! An exec on a host somebody asked to keep connected runs on that connection.
//!
//! `ssh_connect` has always kept a connection open and watched it, but nothing
//! ever used it: every exec, SFTP transfer and session dialled its own. These
//! tests run against a real sshd and pin what changed — an exec on a retained
//! connection opens a *channel*, not a connection — and, as importantly, what
//! must not: a command that times out must not take the connection (or anyone
//! else's command on it) down, and a connection that has died must fall back to
//! a fresh one instead of failing the exec.
//!
//! How reuse is proved rather than assumed: the host profile points at a small
//! TCP proxy that counts the connections it accepts, so "no new connection" is a
//! number, not an inference from timing. The same proxy can sever or silence the
//! connections it already carries, which is how a dead retained connection is
//! produced on demand.
//!
//! Skipped unless `AGENT2SSH_TEST_SSH_PORT` names a reachable sshd — the same
//! variables as `exec_fixture.rs` (`AGENT2SSH_TEST_SSH_HOST`, `_USER`, `_KEY`).
//! Key authentication only: the suite has to run without the credential store.

mod common;
use common::ssh_proxy::Proxy;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use agent2ssh::core::exec_ssh_core;
use agent2ssh::store::CONFIG_DIR_ENV;
use agent2ssh::types::{ExecRequest, ExecResult};
use agent2ssh::{connect_host, disconnect_host, ExecTimedOut};

/// A real sshd and the key that opens it, from the same variables
/// `exec_fixture.rs` reads.
struct Target {
    host: String,
    port: u16,
    user: String,
    key: String,
}

fn target() -> Option<Target> {
    let port = std::env::var("AGENT2SSH_TEST_SSH_PORT")
        .ok()
        .and_then(|raw| raw.trim().parse::<u16>().ok())
        .filter(|port| *port != 0)?;
    // Key auth only: a password would route through the credential store, whose
    // state is process-global and not what is under test here.
    let key = std::env::var("AGENT2SSH_TEST_SSH_KEY")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())?;
    Some(Target {
        host: std::env::var("AGENT2SSH_TEST_SSH_HOST").unwrap_or_else(|_| "127.0.0.1".to_string()),
        port,
        user: std::env::var("AGENT2SSH_TEST_SSH_USER").unwrap_or_else(|_| "root".to_string()),
        key,
    })
}

fn skip() {
    eprintln!(
        "skipping: set AGENT2SSH_TEST_SSH_PORT and AGENT2SSH_TEST_SSH_KEY \
         (see src-tauri/tests/exec_fixture.rs) to run against a real sshd"
    );
}

/// A throwaway config dir holding exactly one host, `fixture`, that goes
/// through `proxy`. `~/.agent2ssh` is never touched.
fn isolated_config(target: &Target, proxy: &Proxy, label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "a2s-exec-retained-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("create the config dir");
    let hosts = serde_json::json!({
        "schema_version": 1,
        "hosts": [{
            "name": "fixture",
            "host": "127.0.0.1",
            "port": proxy.port,
            "user": target.user,
            "key_path": target.key,
        }],
    });
    std::fs::write(
        dir.join("hosts.json"),
        serde_json::to_vec_pretty(&hosts).unwrap(),
    )
    .expect("write hosts.json");
    std::env::set_var(CONFIG_DIR_ENV, &dir);
    dir
}

/// Everything a test holds, torn down in order: the retained connection first
/// (it is process-global state), then the config dir.
struct Rig {
    proxy: Proxy,
    dir: PathBuf,
}

impl Rig {
    fn new(label: &str) -> Option<Self> {
        let Some(target) = target() else {
            // CI sets this so a mis-wired fixture fails instead of passing by
            // running nothing.
            assert!(
                std::env::var("AGENT2SSH_TEST_REQUIRE_SSH").as_deref() != Ok("1"),
                "AGENT2SSH_TEST_REQUIRE_SSH=1 but AGENT2SSH_TEST_SSH_PORT / _KEY are not both set"
            );
            skip();
            return None;
        };
        let proxy = Proxy::start(&target.host, target.port);
        let dir = isolated_config(&target, &proxy, label);
        Some(Self { proxy, dir })
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        // Drop runs inside the test's own runtime, where blocking on another one
        // panics. A thread of its own has no runtime around it. Skipping this
        // would leave the retained connection in the process-global table, and
        // the next test's `connect_host("fixture")` would find it and return
        // early instead of connecting.
        let _ = std::thread::spawn(|| {
            if let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                let _ = runtime.block_on(disconnect_host("fixture"));
            }
        })
        .join();
        common::restore_default_config_env();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn request(command: &str, timeout_secs: u64) -> ExecRequest {
    ExecRequest {
        host: "fixture".to_string(),
        command: command.to_string(),
        // Benign commands; `force` keeps the suite off the risk classifier.
        force: true,
        timeout_secs: Some(timeout_secs),
        stdin: None,
        max_output_bytes: None,
        reason: Some("exec retained test".to_string()),
        change_id: None,
        side_effect: None,
        source: Some("test".to_string()),
    }
}

async fn run(command: &str) -> ExecResult {
    exec_ssh_core(request(command, 30))
        .await
        .unwrap_or_else(|error| panic!("`{command}` failed: {error:#}"))
}

#[tokio::test]
#[serial_test::serial]
async fn an_exec_without_a_retained_connection_dials_its_own() {
    let Some(rig) = Rig::new("baseline") else {
        return;
    };

    for round in 1..=3 {
        let result = run("echo baseline").await;
        assert_eq!(result.stdout.trim(), "baseline");
        // The behavior reuse is built on top of, unchanged: no retention, no
        // lending — one connection per exec.
        assert_eq!(rig.proxy.accepted(), round, "after exec {round}");
    }
}

#[tokio::test]
#[serial_test::serial]
async fn execs_on_a_retained_connection_open_channels_not_connections() {
    let Some(rig) = Rig::new("reuse") else {
        return;
    };

    connect_host("fixture").await.expect("retain the host");
    assert_eq!(rig.proxy.accepted(), 1, "the retained connection");

    for round in 1..=6 {
        let result = run(&format!("echo round-{round}")).await;
        assert_eq!(result.exit_code, Some(0));
        assert_eq!(result.stdout.trim(), format!("round-{round}"));
    }
    assert_eq!(
        rig.proxy.accepted(),
        1,
        "six execs must ride the retained connection, not dial six more"
    );

    // Output and exit status still come back per command, not per connection.
    let failing = run("echo out; echo err >&2; exit 3").await;
    assert_eq!(failing.exit_code, Some(3));
    assert_eq!(failing.stdout.trim(), "out");
    assert_eq!(failing.stderr.trim(), "err");
    assert_eq!(rig.proxy.accepted(), 1);

    // Stdin goes into this command's channel, not into a neighbour's.
    let mut piped = request("cat", 30);
    piped.stdin = Some("through the retained connection".to_string());
    let result = exec_ssh_core(piped).await.expect("exec with stdin");
    assert_eq!(result.stdout, "through the retained connection");
    assert_eq!(rig.proxy.accepted(), 1);

    // Letting go of the connection returns to one connection per exec.
    disconnect_host("fixture").await.expect("disconnect");
    run("echo after-disconnect").await;
    assert_eq!(rig.proxy.accepted(), 2);
}

#[tokio::test]
#[serial_test::serial]
async fn concurrent_execs_share_one_connection_without_disturbing_each_other() {
    let Some(rig) = Rig::new("concurrent") else {
        return;
    };
    connect_host("fixture").await.expect("retain the host");

    // Each command switches its own view of the connection to non-blocking and
    // back while the others are mid-flight. Sharing the old session-wide
    // settings, one exec's teardown timeout or mode switch reaches the rest.
    let started = Instant::now();
    let runs = (0..6).map(|n| {
        tokio::spawn(async move {
            let result = run(&format!("sleep 1; echo concurrent-{n}")).await;
            (n, result)
        })
    });
    for handle in runs.collect::<Vec<_>>() {
        let (n, result) = handle.await.expect("exec task");
        assert_eq!(result.exit_code, Some(0), "exec {n}");
        assert_eq!(result.stdout.trim(), format!("concurrent-{n}"));
    }
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "six one-second commands took {:?}: they were serialized",
        started.elapsed()
    );
    assert_eq!(rig.proxy.accepted(), 1, "all six shared the connection");
}

/// The reason a borrowed connection gets *views* of the settings rather than
/// the settings themselves. An exec that is waiting on its deadline polls its
/// channel without blocking; another exec finishing on the same connection used
/// to switch the connection to blocking reads and a 30 s teardown timeout, which
/// turned the first one's 3 s deadline into a 30 s wait. Commands that simply
/// print and exit cannot show this — data arriving ends the blocking read — so
/// the victim here is a command that prints nothing and has to be timed out.
#[tokio::test]
#[serial_test::serial]
async fn another_execs_teardown_does_not_stretch_a_deadline() {
    let Some(_rig) = Rig::new("deadline") else {
        return;
    };
    connect_host("fixture").await.expect("retain the host");

    let started = Instant::now();
    let victim = tokio::spawn(async { exec_ssh_core(request("sleep 30", 3)).await });
    // Let the victim settle into its non-blocking drain, then finish another
    // exec on the same connection: its teardown is what used to reach the victim.
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(run("echo bystander").await.stdout.trim(), "bystander");

    let error = victim
        .await
        .expect("victim task")
        .expect_err("`sleep 30` outlives a 3 s timeout");
    assert!(
        error.downcast_ref::<ExecTimedOut>().is_some(),
        "the victim must end in its own timeout, got: {error:#}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "a 3 s deadline took {:?}: another exec's teardown settings reached it",
        started.elapsed()
    );
}

#[tokio::test]
#[serial_test::serial]
async fn more_execs_than_the_server_allows_channels_still_all_run() {
    let Some(rig) = Rig::new("overflow") else {
        return;
    };
    connect_host("fixture").await.expect("retain the host");

    // sshd's MaxSessions defaults to 10. Fourteen at once cannot all get a
    // channel on one connection; the ones that cannot must dial their own
    // rather than fail, because nothing had been sent to the server for them.
    let runs = (0..14).map(|n| {
        tokio::spawn(async move {
            let result = run(&format!("sleep 2; echo overflow-{n}")).await;
            (n, result)
        })
    });
    for handle in runs.collect::<Vec<_>>() {
        let (n, result) = handle.await.expect("exec task");
        assert_eq!(result.exit_code, Some(0), "exec {n}");
        assert_eq!(result.stdout.trim(), format!("overflow-{n}"));
    }
    assert!(
        rig.proxy.accepted() > 1,
        "fourteen channels cannot all have fit on one connection"
    );
}

#[tokio::test]
#[serial_test::serial]
async fn a_timeout_closes_its_channel_and_leaves_the_connection_to_the_others() {
    let Some(rig) = Rig::new("timeout") else {
        return;
    };
    connect_host("fixture").await.expect("retain the host");

    // A neighbour mid-flight on the same connection while the other one times
    // out. A timeout used to abort the whole connection; on a shared one that
    // would take the neighbour down with it.
    let neighbour =
        tokio::spawn(async { run("echo neighbour-start; sleep 4; echo neighbour-done").await });

    let started = Instant::now();
    let error = exec_ssh_core(request("echo before-the-deadline; sleep 30", 2))
        .await
        .expect_err("the command outlives its timeout");
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "the timeout did not bound the call: {:?}",
        started.elapsed()
    );
    let timed_out = error
        .downcast_ref::<ExecTimedOut>()
        .unwrap_or_else(|| panic!("a timeout must be the typed error, got: {error:#}"));
    assert!(
        timed_out.stdout.contains("before-the-deadline"),
        "the output printed before the deadline must survive: {:?}",
        timed_out.stdout
    );
    assert!(
        !timed_out.process_stopped,
        "closing a channel does not stop the remote process"
    );

    let neighbour = neighbour.await.expect("neighbour task");
    assert_eq!(neighbour.exit_code, Some(0));
    assert!(
        neighbour.stdout.contains("neighbour-done"),
        "the neighbour was cut off by the other command's timeout: {:?}",
        neighbour.stdout
    );

    // And the connection is still the retained one.
    assert_eq!(run("echo still-here").await.stdout.trim(), "still-here");
    assert_eq!(rig.proxy.accepted(), 1, "nothing had to reconnect");
}

#[tokio::test]
#[serial_test::serial]
async fn a_connection_the_peer_reset_falls_back_to_a_fresh_one() {
    let Some(rig) = Rig::new("reset") else {
        return;
    };
    connect_host("fixture").await.expect("retain the host");
    assert_eq!(run("echo before-reset").await.stdout.trim(), "before-reset");
    assert_eq!(rig.proxy.accepted(), 1);

    rig.proxy.sever_all();
    // Let the reset reach the client's reader task.
    tokio::time::sleep(Duration::from_millis(300)).await;

    // The command must run, not fail because the connection it would have used
    // is gone: nothing had been sent on it, so a new connection is safe.
    let result = run("echo after-reset").await;
    assert_eq!(result.exit_code, Some(0));
    assert_eq!(result.stdout.trim(), "after-reset");
    assert_eq!(rig.proxy.accepted(), 2, "the exec dialled its own");
}

#[tokio::test]
#[serial_test::serial]
async fn a_connection_gone_silent_is_given_up_on_quickly() {
    let Some(rig) = Rig::new("silent") else {
        return;
    };
    connect_host("fixture").await.expect("retain the host");
    assert_eq!(
        run("echo before-silence").await.stdout.trim(),
        "before-silence"
    );

    // No reset, no FIN: the connection just stops answering. Waiting out the
    // command's own timeout for it would make a half-dead retained connection
    // cost a minute per exec.
    rig.proxy.silence_existing();

    let started = Instant::now();
    let result = run("echo after-silence").await;
    assert_eq!(result.stdout.trim(), "after-silence");
    assert!(
        started.elapsed() < Duration::from_secs(15),
        "a silent retained connection cost {:?}; it must be abandoned after the \
         short channel-open bound, not the 30 s command timeout",
        started.elapsed()
    );
    assert_eq!(rig.proxy.accepted(), 2, "the exec dialled its own");

    // The abandoned connection is not offered again.
    let again = run("echo and-again").await;
    assert_eq!(again.stdout.trim(), "and-again");
    assert_eq!(rig.proxy.accepted(), 3);
}

/// What reuse is for, in numbers. Printed for a person, asserted only in the
/// direction that cannot be noise: a channel on a live connection is cheaper
/// than a connection.
#[tokio::test]
#[serial_test::serial]
async fn reuse_is_faster_than_dialling_each_time() {
    let Some(rig) = Rig::new("latency") else {
        return;
    };
    const ROUNDS: u32 = 15;

    let fresh_started = Instant::now();
    for _ in 0..ROUNDS {
        run("true").await;
    }
    let fresh = fresh_started.elapsed();
    assert_eq!(rig.proxy.accepted(), ROUNDS as usize);

    connect_host("fixture").await.expect("retain the host");
    let retained_started = Instant::now();
    for _ in 0..ROUNDS {
        run("true").await;
    }
    let retained = retained_started.elapsed();
    assert_eq!(rig.proxy.accepted(), ROUNDS as usize + 1);

    eprintln!(
        "per exec: {:?} dialling each time, {:?} on a retained connection ({:.1}x)",
        fresh / ROUNDS,
        retained / ROUNDS,
        fresh.as_secs_f64() / retained.as_secs_f64().max(f64::EPSILON)
    );
    assert!(
        retained < fresh,
        "{ROUNDS} execs took {retained:?} on a retained connection and {fresh:?} dialling each time"
    );
}
