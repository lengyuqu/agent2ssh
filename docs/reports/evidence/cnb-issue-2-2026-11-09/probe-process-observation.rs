// Reproduces the Windows observation behind the 5 failing daemon_stop_e2e tests:
// a parent that keeps an unreaped `std::process::Child` handle. Question: after
// the child exits, does sysinfo still list it, and does OpenProcess still succeed?
use std::process::Command;
use std::time::Duration;

use sysinfo::{Pid, ProcessesToUpdate, System};

#[cfg(windows)]
fn open_process_succeeds(pid: u32) -> String {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            format!("no (GetLastError={})", GetLastError())
        } else {
            let mut code: u32 = 0;
            let ok = GetExitCodeProcess(handle, &mut code) != 0;
            CloseHandle(handle);
            if ok && code == 259 {
                format!("yes (still active => pid reused by a live process)")
            } else {
                format!("yes (exit code {code} => terminated process object still openable)")
            }
        }
    }
}

fn observe(pid: u32) -> (bool, Option<(u64, String)>) {
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::Some(&[Pid::from_u32(pid)]), true);
    let listed = system
        .process(Pid::from_u32(pid))
        .map(|process| (process.start_time(), format!("{:?}", process.status())));
    (listed.is_some(), listed)
}

fn main() {
    // A child that exits immediately, with its handle kept open by this process.
    let mut child = Command::new("cmd").args(["/c", "exit 3"]).spawn().unwrap();
    let pid = child.id();
    println!("child pid: {pid}");
    std::thread::sleep(Duration::from_secs(2));

    let (listed, detail) = observe(pid);
    println!("while the handle is held:");
    println!("  sysinfo lists the process : {listed} {detail:?}");
    #[cfg(windows)]
    println!("  OpenProcess succeeds       : {}", open_process_succeeds(pid));

    // Reaping closes the handle and frees the process object.
    let status = child.wait().unwrap();
    println!("exit status after wait(): {status}");
    std::thread::sleep(Duration::from_secs(3));

    let (listed_after, detail_after) = observe(pid);
    println!("after wait() reaped it (+3 s):");
    println!("  sysinfo lists the process : {listed_after} {detail_after:?}");
    #[cfg(windows)]
    println!("  OpenProcess succeeds       : {}", open_process_succeeds(pid));
}
