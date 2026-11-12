//! Probe: is `GetProcessTimes().ftCreationTime` the same quantity as
//! `sysinfo::Process::start_time()` on Windows?
//!
//! Issue #24 step 0. The whole handle-binding design rests on this: if the
//! two are not the same clock (unit, epoch, truncation), every "handle +
//! creation time" comparison silently degrades to `Unverified`.
//!
//! Run: cargo run --release

use std::process::{Child, Command, Stdio};
use std::thread::sleep;
use std::time::Duration;

use windows_sys::Win32::Foundation::{CloseHandle, FILETIME};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
};

const TICKS_PER_SEC: u64 = 10_000_000;
/// Seconds between 1601-01-01 (FILETIME epoch) and 1970-01-01 (Unix epoch).
const EPOCH_DIFF_SECS: u64 = 11_644_473_600;

fn creation_ticks(handle: isize) -> Option<u64> {
    unsafe {
        let mut creation: FILETIME = std::mem::zeroed();
        let mut exit: FILETIME = std::mem::zeroed();
        let mut kernel: FILETIME = std::mem::zeroed();
        let mut user: FILETIME = std::mem::zeroed();
        if GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) == 0 {
            return None;
        }
        Some(((creation.dwHighDateTime as u64) << 32) | creation.dwLowDateTime as u64)
    }
}

fn open_and_read(pid: u32) -> Option<u64> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle == 0 {
            return None;
        }
        let ticks = creation_ticks(handle);
        CloseHandle(handle);
        ticks
    }
}

fn sysinfo_start(pid: u32) -> Option<u64> {
    let sys_pid = sysinfo::Pid::from_u32(pid);
    let mut system = sysinfo::System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[sys_pid]), true);
    system.process(sys_pid).map(|p| p.start_time())
}

fn spawn_sleeper() -> Child {
    Command::new("cmd")
        .args(["/C", "ping -n 30 127.0.0.1 > NUL"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn sleeper")
}

fn main() {
    println!("== environment ==");
    println!("probe pid            {}", std::process::id());
    println!();

    // --- 1. current process: FILETIME vs sysinfo, same object, two readers.
    println!("== 1. self: FILETIME vs sysinfo ==");
    let self_pid = std::process::id();
    let self_handle = unsafe { GetCurrentProcess() };
    let self_ticks = creation_ticks(self_handle).expect("GetProcessTimes(self)");
    let self_unix_secs = self_ticks / TICKS_PER_SEC - EPOCH_DIFF_SECS;
    let self_frac = self_ticks % TICKS_PER_SEC;
    let self_sys = sysinfo_start(self_pid).expect("sysinfo self");
    println!("ft_raw_100ns         {self_ticks}");
    println!("ft_unix_secs         {self_unix_secs}   (raw/1e7 - epoch_diff)");
    println!("ft_subsec_ticks      {self_frac}   (0 => whole second, else precision lost)");
    println!("sysinfo_start_time   {self_sys}");
    println!(
        "delta (sys - ft)     {}",
        self_sys as i64 - self_unix_secs as i64
    );
    println!();

    // --- 2. children: same comparison through OpenProcess, plus truncation.
    println!("== 2. children: OpenProcess(QUERY_LIMITED_INFORMATION) vs sysinfo ==");
    println!(
        "{:<8} {:>20} {:>14} {:>10} {:>14} {:>7}",
        "pid", "ft_raw_100ns", "ft_unix_secs", "ft_frac", "sysinfo_start", "delta"
    );
    let mut children = Vec::new();
    for _ in 0..5 {
        let child = spawn_sleeper();
        children.push(child);
    }
    sleep(Duration::from_millis(300));
    let mut deltas = Vec::new();
    let mut fracs = Vec::new();
    for child in &children {
        let pid = child.id();
        if let (Some(ticks), Some(sys)) = (open_and_read(pid), sysinfo_start(pid)) {
            let unix_secs = ticks / TICKS_PER_SEC - EPOCH_DIFF_SECS;
            let frac = ticks % TICKS_PER_SEC;
            deltas.push(sys as i64 - unix_secs as i64);
            fracs.push(frac);
            println!(
                "{:<8} {:>20} {:>14} {:>10} {:>14} {:>7}",
                pid,
                ticks,
                unix_secs,
                frac,
                sys,
                sys as i64 - unix_secs as i64
            );
        } else {
            println!("{:<8} <unreadable>", pid);
        }
    }
    println!();
    println!("distinct deltas      {:?}", {
        let mut d = deltas.clone();
        d.sort_unstable();
        d.dedup();
        d
    });
    println!(
        "any subsec ticks     {}",
        fracs.iter().any(|f| *f != 0)
    );
    println!();

    // --- 3. second-level collision: children spawned in the same second.
    println!("== 3. second-level collision (does sysinfo collapse them?) ==");
    let mut starts = Vec::new();
    for child in &children {
        let pid = child.id();
        if let (Some(ticks), Some(sys)) = (open_and_read(pid), sysinfo_start(pid)) {
            starts.push((pid, ticks, sys));
        }
    }
    let mut by_sec: std::collections::BTreeMap<u64, Vec<u32>> = Default::default();
    for (pid, _, sys) in &starts {
        by_sec.entry(*sys).or_default().push(*pid);
    }
    for (sec, pids) in &by_sec {
        println!(
            "sysinfo_start {sec} -> {} process(es): {:?}{}",
            pids.len(),
            pids,
            if pids.len() > 1 {
                "   <-- same second, indistinguishable by start_time alone"
            } else {
                ""
            }
        );
    }
    // How many distinct ft_raw values vs distinct sysinfo values?
    let mut raw_set: Vec<u64> = starts.iter().map(|(_, t, _)| *t).collect();
    raw_set.sort_unstable();
    raw_set.dedup();
    println!(
        "distinct ft_raw      {} of {}",
        raw_set.len(),
        starts.len()
    );
    println!("distinct sysinfo     {} of {}", by_sec.len(), starts.len());
    println!();

    // --- 4. exited-but-handed process: does the handle keep the object?
    println!("== 4. handle survives exit: read creation time after the process is gone ==");
    let exited_pid;
    let held_handle;
    {
        let mut quick = Command::new("cmd")
            .args(["/C", "exit 7"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn quick");
        exited_pid = quick.id();
        // Open while alive, keep the handle across the exit.
        held_handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, exited_pid) };
        let _ = quick.wait();
        // Give the kernel a moment to tear the process down.
        sleep(Duration::from_millis(200));
    }
    if held_handle == 0 {
        println!("OpenProcess failed for {exited_pid}");
    } else {
        let before = creation_ticks(held_handle);
        let reopened = open_and_read(exited_pid);
        let sys_after = sysinfo_start(exited_pid);
        println!("pid                  {exited_pid} (exited)");
        println!("held handle ticks    {before:?}   (Some => handle outlived exit)");
        println!("reopen by pid        {reopened:?}   (None expected: pid free or reused)");
        println!("sysinfo after exit   {sys_after:?}");
        unsafe { CloseHandle(held_handle) };
    }
    println!();

    // --- 5. what a stale start_time would compare against after reuse.
    println!("== 5. identity comparison the current code performs ==");
    let mut mismatches = 0;
    let mut matches = 0;
    for child in &children {
        let pid = child.id();
        if let (Some(ticks), Some(sys)) = (open_and_read(pid), sysinfo_start(pid)) {
            let unix_secs = ticks / TICKS_PER_SEC - EPOCH_DIFF_SECS;
            if sys == unix_secs {
                matches += 1;
            } else {
                mismatches += 1;
            }
            let _ = child;
        }
    }
    println!("sysinfo == ft_unix_secs (truncated): {matches} match / {mismatches} mismatch");
    println!();
    println!("== verdict ==");
    if deltas.iter().all(|d| *d == 0) && matches == children.len() {
        println!("OK: sysinfo start_time == ftCreationTime/1e7 - 11644473600 (truncated to seconds)");
        println!("    => a held handle's GetProcessTimes is directly comparable to OwnerMarker.start_time");
        println!(
            "    => second-level resolution confirmed; subsec ticks present: {}",
            fracs.iter().any(|f| *f != 0)
        );
    } else {
        println!("MISMATCH: the two clocks are not interchangeable; do not compare them directly");
    }

    for mut child in children {
        let _ = child.kill();
        let _ = child.wait();
    }
}
