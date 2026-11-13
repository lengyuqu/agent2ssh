#![cfg(all(feature = "daemon", target_os = "macos"))]
//! Acceptance for the Mach discovery name (CNB #39): registration, release
//! after a clean exit and after SIGKILL, and what a stop sends when the name
//! is held by some other process of the same user.
//!
//! Every daemon here is a real process on an isolated config directory with a
//! synthetic token. Nothing proves `Bound`; these only pin the observed
//! behaviour of the self-registered name on the machine that runs them.
use std::ffi::{c_char, c_int, CString};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

const TOKEN: &str = "synthetic-mach-discovery-token";
const A2S_HELLO: i32 = 0xA231;
const A2S_STOP: i32 = 0xA233;
/// The stop message carries the token in a fixed 128-byte field, so nothing
/// shorter than this can hold one.
const TOKEN_FIELD: u32 = 128;

type Port = u32;
const KERN_SUCCESS: c_int = 0;
const MACH_PORT_RIGHT_RECEIVE: u32 = 1;
const MACH_MSG_TYPE_MAKE_SEND: u32 = 20;
const MACH_RCV_MSG: c_int = 0x2;
const MACH_RCV_TIMEOUT: c_int = 0x100;

unsafe extern "C" {
    static bootstrap_port: Port;
    static mach_task_self_: Port;
    fn bootstrap_look_up(bp: Port, name: *const c_char, port: *mut Port) -> c_int;
    fn bootstrap_register(bp: Port, name: *const c_char, port: Port) -> c_int;
    fn mach_port_allocate(task: Port, right: u32, name: *mut Port) -> c_int;
    fn mach_port_insert_right(task: Port, name: Port, poly: Port, poly_type: u32) -> c_int;
    fn mach_port_mod_refs(task: Port, name: Port, right: u32, delta: c_int) -> c_int;
    fn mach_port_deallocate(task: Port, name: Port) -> c_int;
    fn mach_msg(
        msg: *mut u8,
        option: c_int,
        send_size: u32,
        rcv_size: u32,
        rcv_name: Port,
        timeout: u32,
        notify: Port,
    ) -> c_int;
}

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn address() -> String {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .to_string()
}

fn daemon_command(dir: &Path, addr: &str) -> Command {
    std::fs::write(dir.join("daemon.token"), TOKEN).unwrap();
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("daemon-output.log"))
        .unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_agent2ssh-daemon"));
    cmd.env("AGENT2SSH_CONFIG_DIR", dir)
        .env("AGENT2SSH_DAEMON_ADDR", addr)
        .env_remove("AGENT2SSH_MASTER_PASSWORD")
        .env_remove("AGENT2SSH_ALLOW_UNBOUND_HTTP_SHUTDOWN")
        .stdin(Stdio::null())
        .stdout(Stdio::from(log.try_clone().unwrap()))
        .stderr(Stdio::from(log));
    cmd
}

fn output_log(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("daemon-output.log")).unwrap_or_default()
}

fn daemon(dir: &Path, addr: &str) -> ChildGuard {
    let child = ChildGuard(daemon_command(dir, addr).spawn().unwrap());
    let until = Instant::now() + Duration::from_secs(20);
    while Instant::now() < until {
        if dir.join("daemon.pid").exists()
            && agent2ssh::daemon_activity::direct_daemon_request(addr, "GET", "/health", None)
                .is_ok_and(|response| {
                    response.status == 200
                        && serde_json::from_str::<serde_json::Value>(&response.body)
                            .ok()
                            .and_then(|body| body["pid"].as_u64())
                            == Some(u64::from(child.0.id()))
                })
        {
            return child;
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    panic!("daemon did not become ready: {}", output_log(dir));
}

fn cli(dir: &Path, addr: &str, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_agent2ssh"))
        .args(args)
        .env("AGENT2SSH_CONFIG_DIR", dir)
        .env("AGENT2SSH_DAEMON_ADDR", addr)
        .env_remove("AGENT2SSH_MASTER_PASSWORD")
        .env_remove("AGENT2SSH_ALLOW_UNBOUND_HTTP_SHUTDOWN")
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn service(dir: &Path) -> CString {
    let id = std::fs::read_to_string(dir.join("daemon.config-id")).unwrap();
    CString::new(format!("org.agent2ssh.{id}")).unwrap()
}

fn registered(name: &CString) -> bool {
    let mut port: Port = 0;
    // SAFETY: `name` is a live C string and `port` is a valid out pointer.
    let found =
        unsafe { bootstrap_look_up(bootstrap_port, name.as_ptr(), &mut port) } == KERN_SUCCESS;
    if found {
        // SAFETY: a successful look-up returned a send right this task owns.
        unsafe { mach_port_deallocate(mach_task_self_, port) };
    }
    found
}

fn wait_released(name: &CString) {
    let until = Instant::now() + Duration::from_secs(5);
    while registered(name) {
        assert!(
            Instant::now() < until,
            "discovery name is still registered after its process is gone"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn wait_exit(child: &mut ChildGuard) {
    let until = Instant::now() + Duration::from_secs(10);
    while child.0.try_wait().unwrap().is_none() {
        assert!(Instant::now() < until, "daemon remained alive");
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn records(dir: &Path) -> (Vec<u8>, Vec<u8>) {
    (
        std::fs::read(dir.join("daemon.pid")).unwrap(),
        std::fs::read(dir.join("daemon.instance.json")).unwrap(),
    )
}

fn no_records(dir: &Path) -> bool {
    !dir.join("daemon.pid").exists()
        && !dir.join("daemon.instance.json").exists()
        && !dir.join(".daemon-ipc/s").exists()
}

fn clear_stale(dir: &Path, addr: &str, pid: u32, instance: &[u8]) {
    let record: serde_json::Value = serde_json::from_slice(instance).unwrap();
    let out = cli(
        dir,
        addr,
        &[
            "daemon",
            "clear-stale",
            "--expect-pid",
            &pid.to_string(),
            "--expect-instance",
            record["instance_id"].as_str().unwrap(),
        ],
    );
    assert!(out.status.success(), "{}", text(&out));
    assert!(no_records(dir));
}

/// A receive right registered under the daemon's discovery name by this test
/// process: the same-user squatter a stop may find instead of the daemon.
struct Squatter(Port);
impl Squatter {
    fn register(name: &CString) -> Self {
        let mut port: Port = 0;
        // SAFETY: plain Mach calls on this task's own port space.
        unsafe {
            assert_eq!(
                mach_port_allocate(mach_task_self_, MACH_PORT_RIGHT_RECEIVE, &mut port),
                KERN_SUCCESS
            );
            assert_eq!(
                mach_port_insert_right(mach_task_self_, port, port, MACH_MSG_TYPE_MAKE_SEND),
                KERN_SUCCESS
            );
            assert_eq!(
                bootstrap_register(bootstrap_port, name.as_ptr(), port),
                KERN_SUCCESS,
                "the test process could not register the discovery name"
            );
        }
        Self(port)
    }
    /// Every message queued on the port, as (id, size, raw bytes).
    fn drain(&self) -> Vec<(i32, u32, Vec<u8>)> {
        #[repr(C, align(8))]
        struct Buffer([u8; 4096]);
        let mut seen = Vec::new();
        loop {
            let mut buffer = Buffer([0; 4096]);
            // SAFETY: the buffer is writable for its whole stated size.
            let kr = unsafe {
                mach_msg(
                    buffer.0.as_mut_ptr(),
                    MACH_RCV_MSG | MACH_RCV_TIMEOUT,
                    0,
                    buffer.0.len() as u32,
                    self.0,
                    300,
                    0,
                )
            };
            if kr != KERN_SUCCESS {
                return seen;
            }
            // mach_msg_header_t: bits, size, remote, local, voucher, id.
            let size = u32::from_ne_bytes(buffer.0[4..8].try_into().unwrap());
            let id = i32::from_ne_bytes(buffer.0[20..24].try_into().unwrap());
            seen.push((id, size, buffer.0[..size as usize].to_vec()));
        }
    }
}
impl Drop for Squatter {
    fn drop(&mut self) {
        // SAFETY: drops the receive right and the send right made from it.
        unsafe {
            mach_port_mod_refs(mach_task_self_, self.0, MACH_PORT_RIGHT_RECEIVE, -1);
            mach_port_deallocate(mach_task_self_, self.0);
        }
    }
}

#[test]
#[serial_test::serial]
fn name_is_released_after_a_clean_stop_and_the_same_config_starts_again() {
    let dir = tempfile::tempdir().unwrap();
    let addr = address();
    let mut first = daemon(dir.path(), &addr);
    let name = service(dir.path());
    assert!(registered(&name), "running daemon has no discovery name");

    let out = cli(dir.path(), &addr, &["daemon", "stop"]);
    assert!(out.status.success(), "{}", text(&out));
    wait_exit(&mut first);
    wait_released(&name);
    assert!(no_records(dir.path()));

    // The config id, and so the name, is the same for the next instance. If
    // the name had outlived the first process this start would fail.
    let addr = address();
    let mut second = daemon(dir.path(), &addr);
    assert_eq!(service(dir.path()), name);
    assert!(registered(&name));
    let out = cli(dir.path(), &addr, &["daemon", "stop"]);
    assert!(out.status.success(), "{}", text(&out));
    wait_exit(&mut second);
    wait_released(&name);
}

#[test]
#[serial_test::serial]
fn name_is_released_after_sigkill_and_stop_sends_nothing_until_explicit_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let addr = address();
    let mut crashed = daemon(dir.path(), &addr);
    let pid = crashed.0.id();
    let name = service(dir.path());
    let before = records(dir.path());
    crashed.0.kill().unwrap();
    crashed.0.wait().unwrap();
    wait_released(&name);

    // The records survive the crash and nothing answers under the name.
    let out = cli(dir.path(), &addr, &["daemon", "stop"]);
    assert!(!out.status.success());
    assert!(
        text(&out).contains("no shutdown or signal was sent"),
        "{}",
        text(&out)
    );
    assert!(!text(&out).contains(TOKEN));
    assert_eq!(records(dir.path()), before);
    assert!(dir.path().join(".daemon-ipc/s").exists());

    clear_stale(dir.path(), &addr, pid, &before.1);
    let addr = address();
    let mut next = daemon(dir.path(), &addr);
    assert!(registered(&name));
    let out = cli(dir.path(), &addr, &["daemon", "stop"]);
    assert!(out.status.success(), "{}", text(&out));
    wait_exit(&mut next);
}

#[test]
#[serial_test::serial]
fn a_squatted_name_receives_a_hello_without_the_token_and_blocks_a_new_start() {
    let dir = tempfile::tempdir().unwrap();
    let addr = address();
    let mut crashed = daemon(dir.path(), &addr);
    let pid = crashed.0.id();
    let name = service(dir.path());
    let before = records(dir.path());
    crashed.0.kill().unwrap();
    crashed.0.wait().unwrap();
    wait_released(&name);

    let squatter = Squatter::register(&name);
    let out = cli(dir.path(), &addr, &["daemon", "stop"]);
    assert!(!out.status.success());
    assert!(
        text(&out).contains("no shutdown or signal was sent"),
        "{}",
        text(&out)
    );
    assert_eq!(records(dir.path()), before);

    // The squatter holds the discovery name and never answers. What reached
    // it is one hello, too short to hold a token, and no stop.
    let seen = squatter.drain();
    assert_eq!(seen.len(), 1, "unexpected messages: {:?}", seen.len());
    let (id, size, bytes) = &seen[0];
    assert_eq!(*id, A2S_HELLO);
    assert!(
        *size < TOKEN_FIELD,
        "hello is large enough to carry a token"
    );
    assert!(seen.iter().all(|(id, _, _)| *id != A2S_STOP));
    assert!(!bytes
        .windows(TOKEN.len())
        .any(|window| window == TOKEN.as_bytes()));

    // A new daemon cannot take a name that is already held. It exits and
    // leaves no records behind for the next start to trip over.
    clear_stale(dir.path(), &addr, pid, &before.1);
    let addr = address();
    let mut blocked = ChildGuard(daemon_command(dir.path(), &addr).spawn().unwrap());
    let until = Instant::now() + Duration::from_secs(15);
    let status = loop {
        if let Some(status) = blocked.0.try_wait().unwrap() {
            break status;
        }
        assert!(
            Instant::now() < until,
            "daemon kept running although its discovery name was taken"
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(!status.success());
    assert!(
        output_log(dir.path()).contains("Mach control channel failed to start"),
        "{}",
        output_log(dir.path())
    );
    assert!(no_records(dir.path()));

    // Once the squatter lets go the same config starts and stops normally.
    drop(squatter);
    wait_released(&name);
    let addr = address();
    let mut next = daemon(dir.path(), &addr);
    let out = cli(dir.path(), &addr, &["daemon", "stop"]);
    assert!(out.status.success(), "{}", text(&out));
    wait_exit(&mut next);
}
