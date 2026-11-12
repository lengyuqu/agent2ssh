//! Evidence helper for issue #27 — prints the decrypted password for a host as
//! hex so trailing CR/LF and spaces are unambiguous. Not part of the shipped
//! product; created for the probe and removed afterwards.

fn main() {
    let name = match std::env::args().nth(1) {
        Some(name) => name,
        None => {
            eprintln!("usage: show_secret <host-name>");
            std::process::exit(64);
        }
    };
    if !agent2ssh::secrets::try_unlock_from_env() {
        eprintln!("could not unlock credential store from AGENT2SSH_MASTER_PASSWORD");
        std::process::exit(3);
    }
    match agent2ssh::secrets::get_secret(&agent2ssh::secrets::host_account(&name)) {
        Some(value) => {
            let mut hex = String::new();
            for byte in value.as_bytes() {
                hex.push_str(&format!("{byte:02x}"));
            }
            println!("hex:{hex}");
            println!("len:{}", value.len());
            println!("lossy:{}", value);
        }
        None => {
            println!("missing");
            std::process::exit(2);
        }
    }
}
