// Explicitly opted-in laboratory binary: the production source is shared so
// the real daemon lifecycle is tested, but no default binary enables the lab.
#[cfg(target_os = "macos")]
include!("agent2ssh-daemon.rs");

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("The Mach lab is only available on macOS.");
    std::process::exit(1);
}
