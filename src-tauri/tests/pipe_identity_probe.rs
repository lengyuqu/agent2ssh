//! CNB #24 — Windows named-pipe identity primitives probe (v1 design §4.2 / §7.4).
//!
//! Real-machine measurements only; every test is `#[ignore]`d and Windows-only,
//! in the shape of `container_terminal_manual`. Run:
//!
//! ```text
//! cargo test --manifest-path src-tauri/Cargo.toml --no-default-features \
//!     --test pipe_identity_probe -- --ignored --nocapture --test-threads=1
//! ```
//!
//! The probe answers one question: can a named-pipe control connection C be
//! *provably* tied to the daemon process object O, or does Windows stay
//! `Unbound`? It measures first-instance, DACL, remote rejection, multi-instance,
//! `GetNamedPipeServerProcessId` sampling, handle inheritance / delegation, and
//! what the phase-2 tick record does and does not close. It changes no
//! production code and declares no success in advance (v1 §4.2).
//!
//! Subprocess roles re-exec this same test binary (`current_exe`) with
//! `A2S_PIPE_PROBE_ROLE` set, so no external stand-in image is needed. Pipe
//! names are unique per test; nothing touches a real daemon or `~/.agent2ssh`.
#![cfg(windows)]

use std::ffi::c_void;
use std::sync::atomic::{AtomicU64, Ordering};

use windows_sys::Win32::Foundation::{
    CloseHandle, DuplicateHandle, GetLastError, DUPLICATE_SAME_ACCESS, ERROR_ACCESS_DENIED,
    ERROR_PIPE_BUSY, FALSE, HANDLE, INVALID_HANDLE_VALUE, TRUE,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, TokenUser, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, ReadFile, WriteFile, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_GENERIC_READ,
    FILE_GENERIC_WRITE, OPEN_EXISTING, PIPE_ACCESS_DUPLEX,
};
use windows_sys::Win32::System::Memory::LocalFree;
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeClientProcessId, GetNamedPipeServerProcessId,
    PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows_sys::Win32::System::Threading::{
    CreateProcessW, GetCurrentProcess, GetProcessTimes, OpenProcess, OpenProcessToken,
    TerminateProcess, WaitForSingleObject, CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT,
    PROCESS_DUP_HANDLE, PROCESS_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION, STARTUPINFOW,
};

/// The right a client must NOT hold, or it can plant a second server instance.
const FILE_CREATE_PIPE_INSTANCE: u32 = 0x0004;
const SDDL_REVISION_1: u32 = 1;
const ERROR_PIPE_CONNECTED: u32 = 535;
const INFINITE: u32 = 0xFFFFFFFF;
/// FILE_GENERIC_READ | FILE_GENERIC_WRITE for a pipe. Note this INCLUDES
/// FILE_CREATE_PIPE_INSTANCE (0x4) — a correct client DACL must mask it off.
const PIPE_CLIENT_RW: u32 = FILE_GENERIC_READ | FILE_GENERIC_WRITE;

static SEQ: AtomicU64 = AtomicU64::new(0);

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn pipe_name(label: &str) -> String {
    format!(
        "\\\\.\\pipe\\a2s-probe-{}-{}-{label}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::SeqCst)
    )
}

fn last_error() -> u32 {
    unsafe { GetLastError() }
}

fn create_pipe(name: &str, open_mode: u32, instances: u32, sa: *mut SECURITY_ATTRIBUTES) -> HANDLE {
    create_pipe_mode(
        name,
        open_mode,
        PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
        instances,
        sa,
    )
}

fn create_pipe_mode(
    name: &str,
    open_mode: u32,
    pipe_mode: u32,
    instances: u32,
    sa: *mut SECURITY_ATTRIBUTES,
) -> HANDLE {
    unsafe {
        CreateNamedPipeW(
            wide(name).as_ptr(),
            open_mode,
            pipe_mode,
            instances,
            4096,
            4096,
            0,
            sa,
        )
    }
}

fn connect_with(name: &str, access: u32) -> HANDLE {
    unsafe {
        CreateFileW(
            wide(name).as_ptr(),
            access,
            0,
            std::ptr::null_mut(),
            OPEN_EXISTING,
            0,
            0,
        )
    }
}

fn connect(name: &str) -> HANDLE {
    connect_with(name, PIPE_CLIENT_RW)
}

fn server_pid(client: HANDLE) -> Result<u32, u32> {
    let mut pid = 0u32;
    if unsafe { GetNamedPipeServerProcessId(client, &mut pid) } == 0 {
        Err(last_error())
    } else {
        Ok(pid)
    }
}

fn creation_ticks(pid: u32) -> Option<u64> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle == 0 {
            return None;
        }
        let mut creation = std::mem::zeroed();
        let mut exit = std::mem::zeroed();
        let mut kernel = std::mem::zeroed();
        let mut user = std::mem::zeroed();
        let ok = GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user);
        CloseHandle(handle);
        if ok == 0 {
            return None;
        }
        Some(((creation.dwHighDateTime as u64) << 32) | creation.dwLowDateTime as u64)
    }
}

fn current_user_sid() -> String {
    unsafe {
        let mut token: HANDLE = 0;
        let ok = OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token);
        assert_ne!(ok, 0, "OpenProcessToken failed: {}", last_error());
        let mut len = 0u32;
        GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut len);
        let mut buf = vec![0u8; len as usize];
        let ok = GetTokenInformation(
            token,
            TokenUser,
            buf.as_mut_ptr() as *mut c_void,
            len,
            &mut len,
        );
        CloseHandle(token);
        assert_ne!(ok, 0, "GetTokenInformation failed: {}", last_error());
        let user = &*(buf.as_ptr() as *const TOKEN_USER);
        let mut sid_str = std::ptr::null_mut::<u16>();
        let ok = ConvertSidToStringSidW(user.User.Sid, &mut sid_str);
        assert_ne!(ok, 0, "ConvertSidToStringSidW failed: {}", last_error());
        let mut n = 0;
        while *sid_str.add(n) != 0 {
            n += 1;
        }
        let s = String::from_utf16_lossy(std::slice::from_raw_parts(sid_str, n));
        LocalFree(sid_str as _);
        s
    }
}

fn sddl_dacl(sddl: &str) -> (*mut c_void, SECURITY_ATTRIBUTES) {
    let mut sd: *mut c_void = std::ptr::null_mut();
    let ok = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            wide(sddl).as_ptr(),
            SDDL_REVISION_1,
            &mut sd,
            std::ptr::null_mut(),
        )
    };
    assert_ne!(ok, 0, "SDDL convert failed: {}", last_error());
    let sa = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd,
        bInheritHandle: FALSE,
    };
    (sd, sa)
}

struct Child {
    process: HANDLE,
    pid: u32,
}

impl Child {
    fn wait_exit(&self, ms: u32) -> bool {
        unsafe { WaitForSingleObject(self.process, ms) == 0 }
    }
    fn kill(&self) {
        unsafe {
            TerminateProcess(self.process, 1);
            WaitForSingleObject(self.process, INFINITE);
        }
    }
}

impl Drop for Child {
    fn drop(&mut self) {
        // Never leave a 600 s sleeper orphaned when a probe assertion fails.
        unsafe {
            TerminateProcess(self.process, 1);
            WaitForSingleObject(self.process, INFINITE);
            CloseHandle(self.process);
        }
    }
}

fn spawn_exe(exe: &str, args: &str) -> Child {
    spawn_raw(exe, args, false, None)
}

fn spawn_role(role: &str, name: &str, inherit: bool, extra: &[(&str, String)]) -> Child {
    let exe = std::env::current_exe().unwrap();
    let args = "--ignored --exact probe_role_entry".to_string();
    let mut env: Vec<(String, String)> = vec![
        ("A2S_PIPE_PROBE_ROLE".into(), role.into()),
        ("A2S_PIPE_PROBE_NAME".into(), name.into()),
    ];
    for (k, v) in extra {
        env.push((k.to_string(), v.clone()));
    }
    spawn_raw(&exe.display().to_string(), &args, inherit, Some(&env))
}

fn spawn_raw(
    exe: &str,
    args: &str,
    inherit: bool,
    extra_env: Option<&[(String, String)]>,
) -> Child {
    let mut cmd = wide(&format!("\"{exe}\" {args}"));
    let mut si: STARTUPINFOW = unsafe { std::mem::zeroed() };
    si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
    let mut pi: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
    let env_block: Vec<u16>;
    let env_ptr = match extra_env {
        None => std::ptr::null(),
        Some(extra) => {
            let mut block: Vec<u16> = Vec::new();
            for (k, v) in std::env::vars() {
                if k.starts_with("A2S_PIPE_PROBE_") {
                    continue;
                }
                block.extend(wide(&format!("{k}={v}")));
            }
            for (k, v) in extra {
                block.extend(wide(&format!("{k}={v}")));
            }
            block.push(0);
            env_block = block;
            env_block.as_ptr() as *const c_void
        }
    };
    let ok = unsafe {
        CreateProcessW(
            std::ptr::null(),
            cmd.as_mut_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            if inherit { TRUE } else { FALSE },
            if extra_env.is_some() {
                CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT
            } else {
                CREATE_NO_WINDOW
            },
            env_ptr,
            std::ptr::null(),
            &si,
            &mut pi,
        )
    };
    assert_ne!(ok, 0, "CreateProcessW({exe}) failed: {}", last_error());
    unsafe { CloseHandle(pi.hThread) };
    Child {
        process: pi.hProcess,
        pid: pi.dwProcessId,
    }
}

/// The re-exec entry point. Not a real measurement: it only acts when a parent
/// probe sets `A2S_PIPE_PROBE_ROLE`, and otherwise returns immediately.
#[test]
#[ignore]
fn probe_role_entry() {
    let Ok(role) = std::env::var("A2S_PIPE_PROBE_ROLE") else {
        return;
    };
    let name = std::env::var("A2S_PIPE_PROBE_NAME").unwrap();
    match role.as_str() {
        // Create the pipe, wait for one client, then hold the instance open
        // without ever answering. This is the honest daemon shape.
        "server" => {
            let handle = create_pipe(
                &name,
                PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
                1,
                std::ptr::null_mut(),
            );
            assert_ne!(
                handle,
                INVALID_HANDLE_VALUE,
                "server create: {}",
                last_error()
            );
            wait_connected(handle);
            std::thread::sleep(std::time::Duration::from_secs(600));
        }
        // Hold an inherited / duplicated server handle and answer one byte
        // with its own PID. This is the delegate / proxy shape.
        "proxy" => {
            let raw = probe_handle_value();
            let handle = raw as HANDLE;
            let mut buf = [0u8; 1];
            let mut read = 0u32;
            let ok = unsafe {
                ReadFile(
                    handle,
                    buf.as_mut_ptr() as *mut c_void,
                    1,
                    &mut read,
                    std::ptr::null_mut(),
                )
            };
            if ok == 0 || read == 0 {
                std::process::exit(3);
            }
            let answer = std::process::id().to_le_bytes();
            let mut written = 0u32;
            let ok = unsafe {
                WriteFile(
                    handle,
                    answer.as_ptr(),
                    answer.len() as u32,
                    &mut written,
                    std::ptr::null_mut(),
                )
            };
            if ok == 0 {
                std::process::exit(4);
            }
            std::thread::sleep(std::time::Duration::from_secs(600));
        }
        _ => std::process::exit(2),
    }
    std::process::exit(0);
}

/// The proxy learns its server-handle value either directly (inheritance) or
/// through a rendezvous file the parent writes after `DuplicateHandle` into the
/// live proxy — duplicating into a process that has already exited would close
/// the handle, so the value cannot be known at spawn time.
fn probe_handle_value() -> usize {
    if let Ok(path) = std::env::var("A2S_PIPE_PROBE_HANDLE_FILE") {
        for _ in 0..400 {
            if let Ok(s) = std::fs::read_to_string(&path) {
                if let Ok(v) = s.trim().parse::<usize>() {
                    return v;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        panic!("handle rendezvous file never appeared: {path}");
    }
    std::env::var("A2S_PIPE_PROBE_HANDLE")
        .unwrap()
        .parse()
        .unwrap()
}

fn wait_connected(server: HANDLE) {
    let ok = unsafe { ConnectNamedPipe(server, std::ptr::null_mut()) };
    if ok == 0 {
        assert_eq!(
            last_error(),
            ERROR_PIPE_CONNECTED,
            "ConnectNamedPipe failed: {}",
            last_error()
        );
    }
}

fn roundtrip(client: HANDLE) -> Option<u32> {
    let byte = [0x41u8];
    let mut written = 0u32;
    let ok = unsafe { WriteFile(client, byte.as_ptr(), 1, &mut written, std::ptr::null_mut()) };
    if ok == 0 || written != 1 {
        return None;
    }
    let mut answer = [0u8; 4];
    let mut read = 0u32;
    let ok = unsafe {
        ReadFile(
            client,
            answer.as_mut_ptr() as *mut c_void,
            4,
            &mut read,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 || read != 4 {
        return None;
    }
    Some(u32::from_le_bytes(answer))
}

fn wait_for_pipe(name: &str) -> HANDLE {
    for _ in 0..200 {
        let client = connect(name);
        if client != INVALID_HANDLE_VALUE {
            return client;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    panic!("pipe never came up: {name}");
}

/// Diagnostic: isolate `CreateProcessW` behaviour (flags, exe choice) before the
/// delegation probes rely on it. Prints, asserts only that cmd.exe launches.
#[test]
#[ignore]
fn m0_createprocess_sanity() {
    let exe = std::env::current_exe().unwrap();
    for (label, target, args, flags) in [
        ("cmd/0", "cmd.exe", "/c exit", 0u32),
        ("cmd/no_window", "cmd.exe", "/c exit", CREATE_NO_WINDOW),
        ("self/0", exe.display().to_string().as_str(), "--list", 0),
        (
            "self/no_window",
            exe.display().to_string().as_str(),
            "--list",
            CREATE_NO_WINDOW,
        ),
    ] {
        let mut cmd = wide(&format!("\"{target}\" {args}"));
        let mut si: STARTUPINFOW = unsafe { std::mem::zeroed() };
        si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
        let mut pi: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
        let ok = unsafe {
            CreateProcessW(
                std::ptr::null(),
                cmd.as_mut_ptr(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                FALSE,
                flags,
                std::ptr::null(),
                std::ptr::null(),
                &si,
                &mut pi,
            )
        };
        let err = last_error();
        println!("[m0] {label}: ok={ok} err={err} pid={}", pi.dwProcessId);
        if ok != 0 {
            unsafe {
                WaitForSingleObject(pi.hProcess, INFINITE);
                CloseHandle(pi.hThread);
                CloseHandle(pi.hProcess);
            }
        }
        if label == "cmd/0" {
            assert_ne!(ok, 0, "cmd.exe must launch: {err}");
        }
    }
}

/// §7.4 first instance: with `FILE_FLAG_FIRST_PIPE_INSTANCE` a second server
/// for the same name must fail; without it, multiple instances are allowed.
#[test]
#[ignore]
fn m1_first_instance_and_multi_instance() {
    let name = pipe_name("m1");
    let first = create_pipe(
        &name,
        PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
        1,
        std::ptr::null_mut(),
    );
    assert_ne!(
        first,
        INVALID_HANDLE_VALUE,
        "first create: {}",
        last_error()
    );
    let second = create_pipe(
        &name,
        PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
        1,
        std::ptr::null_mut(),
    );
    let err = last_error();
    println!("[m1] second FIRST_PIPE_INSTANCE create: invalid={} err={err} (PIPE_BUSY={ERROR_PIPE_BUSY}, ACCESS_DENIED={ERROR_ACCESS_DENIED})", second == INVALID_HANDLE_VALUE);
    assert_eq!(
        second, INVALID_HANDLE_VALUE,
        "a second first-instance server was allowed"
    );
    unsafe { CloseHandle(first) };

    let plain = pipe_name("m1plain");
    let a = create_pipe(&plain, PIPE_ACCESS_DUPLEX, 4, std::ptr::null_mut());
    assert_ne!(a, INVALID_HANDLE_VALUE);
    let b = create_pipe(&plain, PIPE_ACCESS_DUPLEX, 4, std::ptr::null_mut());
    println!(
        "[m1] without FIRST flag, second instance allowed: {}",
        b != INVALID_HANDLE_VALUE
    );
    assert_ne!(
        b, INVALID_HANDLE_VALUE,
        "multi-instance should be allowed without the flag"
    );
    unsafe {
        CloseHandle(a);
        CloseHandle(b);
    }
}

/// §7.4 DACL: a client may read/write but must not hold
/// `FILE_CREATE_PIPE_INSTANCE`, so it cannot plant a second server instance.
/// Measures the generic-rights subtlety: FILE_GENERIC_WRITE itself contains
/// 0x4, so both the DACL and a well-behaved client must mask it off.
#[test]
#[ignore]
fn m2_dacl_denies_creating_an_instance_to_the_client() {
    let sid = current_user_sid();
    let name = pipe_name("m2");
    let client_mask = PIPE_CLIENT_RW & !FILE_CREATE_PIPE_INSTANCE;
    let sddl = format!("D:(A;;0x{client_mask:x};;;{sid})");
    let (sd, mut sa) = sddl_dacl(&sddl);
    let server = create_pipe(
        &name,
        PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
        1,
        &mut sa,
    );
    assert_ne!(
        server,
        INVALID_HANDLE_VALUE,
        "owner create with restrictive DACL: {}",
        last_error()
    );

    let attacker = create_pipe(&name, PIPE_ACCESS_DUPLEX, 1, std::ptr::null_mut());
    let err = last_error();
    println!("[m2] second instance under restrictive DACL: denied={} err={err} (ACCESS_DENIED={ERROR_ACCESS_DENIED})", attacker == INVALID_HANDLE_VALUE);
    assert_eq!(attacker, INVALID_HANDLE_VALUE);
    assert_eq!(
        err, ERROR_ACCESS_DENIED,
        "expected access denied, got {err}"
    );

    // The connect access check ignores FILE_CREATE_PIPE_INSTANCE: a client
    // asking for full GENERIC_WRITE (which contains 0x4) still connects. The
    // right is enforced only at CreateNamedPipe, which is what blocks planting
    // a second server instance above. (One server instance serves one client;
    // nMaxInstances is a ceiling on CreateNamedPipe calls, not auto-instances.)
    let client = connect_with(&name, PIPE_CLIENT_RW);
    println!(
        "[m2] client requesting 0x4 (full GENERIC_RW): connected={} err={} mask=0x{client_mask:x}",
        client != INVALID_HANDLE_VALUE,
        last_error()
    );
    assert_ne!(
        client, INVALID_HANDLE_VALUE,
        "connect does not check FILE_CREATE_PIPE_INSTANCE"
    );

    // Control: full-access DACL allows a second instance.
    let full_name = pipe_name("m2full");
    let full_sddl = format!("D:(A;;0x1f01ff;;;{sid})");
    let (full_sd, mut full_sa) = sddl_dacl(&full_sddl);
    let f1 = create_pipe(&full_name, PIPE_ACCESS_DUPLEX, 2, &mut full_sa);
    assert_ne!(f1, INVALID_HANDLE_VALUE);
    let f2 = create_pipe(&full_name, PIPE_ACCESS_DUPLEX, 2, std::ptr::null_mut());
    println!(
        "[m2] control: second instance under full DACL allowed: {}",
        f2 != INVALID_HANDLE_VALUE
    );
    assert_ne!(
        f2, INVALID_HANDLE_VALUE,
        "full-access DACL should allow a second instance"
    );
    unsafe {
        CloseHandle(client);
        CloseHandle(server);
        CloseHandle(f1);
        CloseHandle(f2);
        LocalFree(sd as _);
        LocalFree(full_sd as _);
    }
}

/// §7.4 remote rejection: `PIPE_REJECT_REMOTE_CLIENTS` accepts local
/// connections; a connection attempt through the SMB redirector path
/// (`\\<host>\pipe\...`) is measured, not assumed.
#[test]
#[ignore]
fn m3_remote_rejection_flag() {
    let name = pipe_name("m3");
    let server = create_pipe_mode(
        &name,
        PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
        PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
        1,
        std::ptr::null_mut(),
    );
    assert_ne!(
        server,
        INVALID_HANDLE_VALUE,
        "create with reject-remote: {}",
        last_error()
    );
    let local = connect(&name);
    println!(
        "[m3] local connect with REJECT_REMOTE: connected={}",
        local != INVALID_HANDLE_VALUE
    );
    assert_ne!(local, INVALID_HANDLE_VALUE);

    let host = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "localhost".into());
    let tail = name.trim_start_matches("\\\\.\\");
    for prefix in [format!("\\\\{host}\\"), "\\\\127.0.0.1\\".to_string()] {
        let remote_name = format!("{prefix}{tail}");
        let h = connect(&remote_name);
        println!(
            "[m3] connect via {prefix}: connected={} err={}",
            h != INVALID_HANDLE_VALUE,
            last_error()
        );
        if h != INVALID_HANDLE_VALUE {
            unsafe { CloseHandle(h) };
        }
    }
    unsafe {
        CloseHandle(local);
        CloseHandle(server);
    }
}

/// §7.4 server-PID sampling: `GetNamedPipeServerProcessId` returns the creator
/// PID; `GetNamedPipeClientProcessId` is symmetric. After the server exits the
/// client handle is broken — measure what the API still reports.
#[test]
#[ignore]
fn m4_server_pid_sampling_and_broken_handle() {
    // Symmetry, in-process: the server handle reports the connecting client.
    let sym = pipe_name("m4sym");
    let sym_server = create_pipe(&sym, PIPE_ACCESS_DUPLEX, 1, std::ptr::null_mut());
    assert_ne!(sym_server, INVALID_HANDLE_VALUE);
    let sym_client = connect(&sym);
    assert_ne!(sym_client, INVALID_HANDLE_VALUE);
    wait_connected(sym_server);
    let mut cpid = 0u32;
    let ok = unsafe { GetNamedPipeClientProcessId(sym_server, &mut cpid) };
    println!(
        "[m4] GetNamedPipeClientProcessId: ok={ok} reported={cpid} self={} equal={}",
        std::process::id(),
        cpid == std::process::id()
    );
    assert_ne!(ok, 0);
    assert_eq!(cpid, std::process::id());
    unsafe {
        CloseHandle(sym_client);
        CloseHandle(sym_server);
    }

    // Creator PID across processes, then the broken-handle state.
    let name = pipe_name("m4");
    let server = spawn_role("server", &name, false, &[]);
    let client = wait_for_pipe(&name);
    let reported = server_pid(client).expect("GetNamedPipeServerProcessId");
    println!(
        "[m4] server PID reported={reported} actual={} equal={}",
        server.pid,
        reported == server.pid
    );
    assert_eq!(reported, server.pid);

    server.kill();
    let after = server_pid(client);
    println!("[m4] after server exit, GetNamedPipeServerProcessId on broken client: {after:?}");
    let mut buf = [0u8; 1];
    let mut read = 0u32;
    let ok = unsafe {
        ReadFile(
            client,
            buf.as_mut_ptr() as *mut c_void,
            1,
            &mut read,
            std::ptr::null_mut(),
        )
    };
    println!(
        "[m4] ReadFile on broken client: ok={ok} read={read} err={}",
        last_error()
    );
    unsafe { CloseHandle(client) };
}

/// §7.4 delegation: the creator duplicates its (non-inheritable) server handle
/// into a live proxy which answers; the client's `GetNamedPipeServerProcessId`
/// still names the creator, and the phase-2 tick check on that PID still passes.
/// The connection is therefore NOT bound to the responder.
#[test]
#[ignore]
fn m5_delegation_is_invisible_to_server_pid() {
    let name = pipe_name("m5");
    let server = create_pipe(
        &name,
        PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
        1,
        std::ptr::null_mut(),
    );
    assert_ne!(server, INVALID_HANDLE_VALUE);
    let client = connect(&name);
    assert_ne!(client, INVALID_HANDLE_VALUE);
    wait_connected(server);

    let tmp = tempfile::tempdir().unwrap();
    let hfile = tmp.path().join("handle.txt");
    let proxy = spawn_role(
        "proxy",
        &name,
        false,
        &[("A2S_PIPE_PROBE_HANDLE_FILE", hfile.display().to_string())],
    );
    let mut duplicated: HANDLE = 0;
    let ok = unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            server,
            proxy.process,
            &mut duplicated,
            0,
            FALSE,
            DUPLICATE_SAME_ACCESS,
        )
    };
    assert_ne!(ok, 0, "DuplicateHandle into live proxy: {}", last_error());
    std::fs::write(&hfile, (duplicated as usize).to_string()).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(400));

    let reported = server_pid(client).expect("server pid after delegation");
    println!(
        "[m5] creator PID={} proxy PID={} reported={reported}",
        std::process::id(),
        proxy.pid
    );
    assert_eq!(
        reported,
        std::process::id(),
        "API must still name the creator"
    );
    assert_ne!(reported, proxy.pid);

    let responder = roundtrip(client);
    println!(
        "[m5] roundtrip responder PID={responder:?} (proxy={}, creator={})",
        proxy.pid,
        std::process::id()
    );
    assert_eq!(
        responder,
        Some(proxy.pid),
        "the proxy answered, not the creator"
    );

    let ticks = creation_ticks(reported);
    println!(
        "[m5] creation_ticks(reported creator)={ticks:?} — tick check PASSES while a proxy answers"
    );
    assert!(ticks.is_some());

    unsafe {
        CloseHandle(client);
        CloseHandle(server);
    }
}

/// §7.4 inheritance: an inheritable server handle leaks into a child spawned
/// with `bInheritHandles=TRUE`; `FALSE` blocks that. A same-user process can
/// still open the creator with `PROCESS_DUP_HANDLE` and duplicate the
/// non-inheritable handle out, so non-inheritable is not non-delegable.
#[test]
#[ignore]
fn m6_inheritance_and_same_user_dup_handle() {
    // (a) Inheritable handle + bInheritHandles=TRUE: the child answers.
    let name = pipe_name("m6a");
    let mut sa = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: std::ptr::null_mut(),
        bInheritHandle: TRUE,
    };
    let server = create_pipe(
        &name,
        PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
        1,
        &mut sa,
    );
    assert_ne!(server, INVALID_HANDLE_VALUE);
    let client = connect(&name);
    assert_ne!(client, INVALID_HANDLE_VALUE);
    wait_connected(server);
    let child = spawn_role(
        "proxy",
        &name,
        true,
        &[("A2S_PIPE_PROBE_HANDLE", (server as usize).to_string())],
    );
    std::thread::sleep(std::time::Duration::from_millis(400));
    let responder = roundtrip(client);
    println!(
        "[m6a] inheritable + TRUE: responder={responder:?} child={} self={}",
        child.pid,
        std::process::id()
    );
    assert_eq!(
        responder,
        Some(child.pid),
        "the child inherited and answered"
    );
    unsafe {
        CloseHandle(client);
        CloseHandle(server);
    }

    // (b) Non-inheritable + bInheritHandles=FALSE: the child cannot use it.
    let name2 = pipe_name("m6b");
    let server2 = create_pipe(
        &name2,
        PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
        4,
        std::ptr::null_mut(),
    );
    assert_ne!(server2, INVALID_HANDLE_VALUE);
    let client2 = connect(&name2);
    assert_ne!(client2, INVALID_HANDLE_VALUE);
    wait_connected(server2);
    let child2 = spawn_role(
        "proxy",
        &name2,
        false,
        &[("A2S_PIPE_PROBE_HANDLE", (server2 as usize).to_string())],
    );
    let exited = child2.wait_exit(3000);
    println!("[m6b] FALSE: child exited on invalid handle={exited} (expected true)");
    assert!(exited, "child should fail on a non-inherited handle");

    // (c) Same-user PROCESS_DUP_HANDLE on the creator duplicates server2 out
    // into a fresh proxy, which answers the SAME client2 connection.
    let creator_pid = std::process::id();
    let opened = unsafe { OpenProcess(PROCESS_DUP_HANDLE, 0, creator_pid) };
    println!(
        "[m6c] OpenProcess(self, PROCESS_DUP_HANDLE) same user: ok={} err={}",
        opened != 0,
        last_error()
    );
    assert_ne!(
        opened, 0,
        "same user should hold PROCESS_DUP_HANDLE on its own process"
    );
    let tmp = tempfile::tempdir().unwrap();
    let hfile = tmp.path().join("handle.txt");
    let target = spawn_role(
        "proxy",
        &name2,
        false,
        &[("A2S_PIPE_PROBE_HANDLE_FILE", hfile.display().to_string())],
    );
    let mut dup: HANDLE = 0;
    let ok = unsafe {
        DuplicateHandle(
            opened,
            server2,
            target.process,
            &mut dup,
            0,
            FALSE,
            DUPLICATE_SAME_ACCESS,
        )
    };
    println!(
        "[m6c] DuplicateHandle via PROCESS_DUP_HANDLE: ok={ok} err={}",
        last_error()
    );
    assert_ne!(ok, 0);
    std::fs::write(&hfile, (dup as usize).to_string()).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(400));
    let responder = roundtrip(client2);
    println!(
        "[m6c] duplicated-handle responder={responder:?} target={} creator={creator_pid}",
        target.pid
    );
    assert_eq!(responder, Some(target.pid), "the dup-handle proxy answered");
    unsafe {
        CloseHandle(opened);
        CloseHandle(client2);
        CloseHandle(server2);
    }
}

/// §7.4 reuse window: after the server exits, try to observe a reused PID and
/// measure whether the tick record detects the impostor. Best-effort — Windows
/// allocates PIDs sequentially and may not reuse within the budget; the
/// broken-handle measurement (m4) and the delegation proof (m5) stand regardless.
#[test]
#[ignore]
fn m7_reuse_window_and_tick_detection() {
    let name = pipe_name("m7");
    let server = spawn_role("server", &name, false, &[]);
    let client = wait_for_pipe(&name);
    let recorded_pid = server_pid(client).unwrap();
    let recorded_ticks = creation_ticks(recorded_pid).unwrap();
    println!("[m7] recorded pid={recorded_pid} ticks={recorded_ticks}");
    server.kill();

    let exe = std::env::current_exe().unwrap();
    let mut reused = None;
    for _ in 0..500 {
        let c = spawn_exe(&exe.display().to_string(), "--list");
        if c.pid == recorded_pid {
            reused = Some(c);
            break;
        }
        c.wait_exit(INFINITE);
    }
    match reused {
        Some(c) => {
            let impostor_ticks = creation_ticks(c.pid);
            println!("[m7] PID reused; impostor ticks={impostor_ticks:?} recorded={recorded_ticks} match={}", impostor_ticks == Some(recorded_ticks));
            assert_ne!(
                impostor_ticks,
                Some(recorded_ticks),
                "ticks must distinguish the impostor"
            );
        }
        None => {
            println!("[m7] no PID reuse within 500 spawns; reuse window documented by API shape (numeric PID -> OpenProcess), not forced here");
        }
    }
    unsafe { CloseHandle(client) };
}
