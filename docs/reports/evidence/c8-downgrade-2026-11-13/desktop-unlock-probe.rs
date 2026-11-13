//! Source of the measurement harness for CNB #26 / C8, kept with the evidence.
//! It is not part of the product and is never compiled by this workspace.
//!
//! The desktop settings UI unlocks the credential store by calling the Tauri
//! command `secrets_unlock`. Launching the GUI itself needs a window server, so
//! this harness is copied into a checkout of the measured commit and calls that
//! exact command body — `unlock_or_init` followed by `migrate_plaintext_secrets`
//! — from a build with the `tauri` feature enabled. Build and copy steps are in
//! `run_c8_gaps.py`.

fn main() {
    let password = match std::env::args().nth(1) {
        Some(value) => value,
        None => {
            eprintln!("usage: c8_desktop_unlock_probe <master-password>");
            std::process::exit(64);
        }
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let result = runtime.block_on(agent2ssh::tauri_commands::secrets_unlock(password));
    match result {
        Ok(()) => println!("secrets_unlock: ok"),
        Err(error) => {
            println!("secrets_unlock: error: {error}");
            std::process::exit(2);
        }
    }
}
