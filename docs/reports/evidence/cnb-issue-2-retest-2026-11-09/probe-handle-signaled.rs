// Independent check of the two properties c011b52 relies on, on this machine:
//  1. an exited child whose handle is retained is *signaled* (WaitForSingleObject
//     returns WAIT_OBJECT_0) even though its exit code is 259 (STILL_ACTIVE);
//  2. queries that fail or are denied stay "not signaled" (conservative).
use std::process::Command;
use std::time::{Duration, Instant};

use sysinfo::{Pid, ProcessesToUpdate, System};

#[cfg(windows)]
fn handle_state(pid: u32) -> (bool, u32, bool) {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
    };
    unsafe {
        let handle = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if handle.is_null() {
            return (false, GetLastError(), false);
        }
        let wait = WaitForSingleObject(handle, 0);
        CloseHandle(handle);
        (true, 0, wait == WAIT_OBJECT_0)
    }
}

fn main() {
    #[cfg(windows)]
    {
        // Same access mask as `process_handle_is_signaled`: this is not the
        // `PROCESS_QUERY_LIMITED_INFORMATION` probe that `pid_definitely_missing`
        // performs, and the two functions must not be conflated when reading the
        // access-denied boundary.
        println!("access=PROCESS_SYNCHRONIZE (process_handle_is_signaled)");
        let extra: Vec<u32> = std::env::args().skip(1).filter_map(|a| a.parse().ok()).collect();
        // 0 is rejected with ERROR_INVALID_PARAMETER; 0xFFFE does not exist.
        for pid in [0u32, 0xFFFEu32, 4].into_iter().chain(extra) {
            let (opened, err, signaled) = handle_state(pid);
            println!("pid {pid}: OpenProcess={opened} GetLastError={err} signaled={signaled}");
        }

        // Live process: this one.
        let (opened, err, signaled) = handle_state(std::process::id());
        println!("self: OpenProcess={opened} GetLastError={err} signaled={signaled}");

        // Exited child with exit code 259, handle retained by this process.
        let mut child = Command::new("cmd.exe")
            .args(["/C", "exit", "/B", "259"])
            .spawn()
            .unwrap();
        let pid = child.id();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !handle_state(pid).2 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        let (opened, err, signaled) = handle_state(pid);
        let mut system = System::new();
        system.refresh_processes(ProcessesToUpdate::Some(&[Pid::from_u32(pid)]), true);
        let listed = system.process(Pid::from_u32(pid)).is_some();
        println!(
            "exited child {pid}: OpenProcess={opened} GetLastError={err} signaled={signaled} sysinfo_listed={listed}"
        );
        let status = child.wait().unwrap();
        println!("child exit status: {:?}", status.code());
    }
}
