fn main() {
    #[cfg(feature = "tauri")]
    {
        tauri_build::build();
        delay_load_comctl32();
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
