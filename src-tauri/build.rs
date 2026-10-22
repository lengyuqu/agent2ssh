fn main() {
    #[cfg(feature = "tauri")]
    {
        blank_missing_sidecars();
        tauri_build::build();
        delay_load_comctl32();
    }
}

/// Compile the tauri-feature library even when the sidecars have not been built.
///
/// `bundle.externalBin` in `tauri.conf.json` names three binaries that
/// `scripts/prepare-sidecars.sh` assembles from a release build, and
/// `.gitignore` excludes them. On a fresh clone every `cargo check` or
/// `cargo test` that enables the tauri feature therefore died in this build
/// script with
///
/// ```text
/// resource path binaries/agent2ssh-cli-x86_64-pc-windows-msvc doesn't exist
/// ```
///
/// — a message that names neither the cause nor the fix, on the one command
/// `AGENTS.md` tells a new contributor to run.
///
/// Blanking the list is safe because it cannot reach a shipped bundle: the
/// packaging job builds the sidecars *before* it invokes tauri, so by the time
/// `tauri_build` runs there they exist and this branch is not taken. What the
/// branch buys is a working `cargo test --lib` on a clean checkout, which is
/// the thing that made "clone and build" untrue.
///
/// An explicit `TAURI_CONFIG` always wins — CI sets one precisely because it
/// runs on a runner with no sidecars at all, and a caller who has set it is
/// expressing intent this heuristic should not second-guess.
#[cfg(feature = "tauri")]
fn blank_missing_sidecars() {
    if std::env::var_os("TAURI_CONFIG").is_some() {
        return;
    }
    let Ok(target) = std::env::var("TARGET") else {
        return;
    };
    let Ok(entries) = std::fs::read_dir("binaries") else {
        // No `binaries/` at all: same conclusion as "none for this target".
        std::env::set_var("TAURI_CONFIG", r#"{"bundle":{"externalBin":[]}}"#);
        return;
    };
    let suffix = format!("-{target}");
    let present = entries.flatten().any(|entry| {
        entry
            .file_name()
            .to_string_lossy()
            .contains(suffix.as_str())
    });
    if !present {
        println!(
            "cargo:warning=no sidecar binaries for {target} in binaries/; \
             building the library without bundle.externalBin. Run \
             ./scripts/prepare-sidecars.sh {target} to produce them."
        );
        std::env::set_var("TAURI_CONFIG", r#"{"bundle":{"externalBin":[]}}"#);
    }
}

/// `cargo test --lib` on Windows fails to load with STATUS_ENTRYPOINT_NOT_FOUND
/// (0xc0000139): the tauri feature pulls a direct import of
/// `comctl32!TaskDialogIndirect`, which exists only in comctl32 v6 -- the
/// WinSxS side-by-side assembly. `tauri_build` embeds the Common-Controls v6
/// manifest dependency into the *binaries*, but cargo test targets get no
/// manifest, so the loader binds the comctl32 5.82 copy in System32 and dies
/// before `main`.
///
/// The fix delay-loads comctl32: the import is resolved on first call instead
/// of at process start, and tests never open a native dialog, so the 5.82
/// copy is never touched. `rustc-link-arg-tests` cannot be used here -- it
/// reaches only `tests/*.rs` targets, not the lib's unittest harness -- so
/// this is the unscoped variant, which also covers the binaries. That is
/// safe: delay-loading changes nothing for them (their v6 manifest still
/// governs which comctl32 the delayed load binds, and tauri's dialogs work
/// as before), and it does not touch their embedded manifest resource.
#[cfg(feature = "tauri")]
fn delay_load_comctl32() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    println!("cargo:rustc-link-arg=/DELAYLOAD:comctl32.dll");
    println!("cargo:rustc-link-lib=delayimp");
}
