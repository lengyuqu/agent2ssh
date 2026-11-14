//! Windows subprocess hygiene: every `Command` spawned from the desktop app or
//! the daemon targets a console executable (icacls, reg.exe, wslc, taskkill,
//! vswhere, …). Without `CREATE_NO_WINDOW` a GUI process allocating a console
//! for each child flashes a cmd window per spawn — during first launch the
//! store hardening alone (icacls per protected file, reg.exe PATH reads) pops
//! a stack of windows, and each console allocation costs 100-300 ms on the
//! spawning thread.

/// Apply `CREATE_NO_WINDOW` to a child process on Windows; a no-op elsewhere.
///
/// Takes the builder by value and returns it, so call sites chain naturally:
///
/// ```ignore
/// let output = crate::win_proc::no_window(Command::new("icacls")).arg(path).output()?;
/// ```
#[cfg(windows)]
pub fn no_window(mut command: std::process::Command) -> std::process::Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

/// Non-Windows counterpart: returns the builder unchanged.
#[cfg(not(windows))]
pub fn no_window(command: std::process::Command) -> std::process::Command {
    command
}
