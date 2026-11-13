//! macOS production stop channel.
//!
//! Discovery is a bootstrap name. The admin token moves only on the private
//! send right returned by hello. Exit is `NOTE_EXIT` on the task identity
//! checked before and after that registration. A code signature is not read,
//! and a failure here does not fall back to HTTP or a bare PID signal.
use anyhow::{bail, Result};
use std::ffi::{c_char, c_int, CString};
use std::path::Path;

pub fn service_name(config_id: &str) -> Result<String> {
    if !super::valid_id(config_id) {
        bail!("invalid daemon config identifier");
    }
    Ok(format!("org.agent2ssh.{config_id}"))
}

pub fn start(config_id: &str, instance_id: &str, token: &str) -> Result<()> {
    if !super::valid_id(instance_id) {
        bail!("invalid daemon instance identifier");
    }
    if token.is_empty() || token.len() >= 128 || token.as_bytes().contains(&0) {
        bail!("daemon token cannot be used on the Mach control channel");
    }
    let service = CString::new(service_name(config_id)?)?;
    let instance = CString::new(instance_id)?;
    let token = CString::new(token)?;
    // SAFETY: all three strings are NUL-terminated and live for the call.
    // The C side copies them before returning.
    let rc = unsafe { a2s_mach_control_start(service.as_ptr(), instance.as_ptr(), token.as_ptr()) };
    if rc != 0 {
        bail!("Mach control channel failed to start ({rc})");
    }
    Ok(())
}

pub enum StopOutcome {
    Exited,
    /// Nothing was sent. `detail` must not contain the admin token.
    Refused(String),
    /// The stop may have reached the daemon and its exit was not observed.
    Unknown,
}

pub fn stop(config: &Path, pid: u32) -> StopOutcome {
    let (config_id, instance_id) = match super::macos::mach_target(config, pid) {
        Ok(value) => value,
        Err(error) => return StopOutcome::Refused(error.to_string()),
    };
    let token = match super::macos::read_existing_token(config) {
        Ok(value) => value,
        Err(error) => return StopOutcome::Refused(error.to_string()),
    };
    let service = match service_name(&config_id)
        .and_then(|name| CString::new(name).map_err(|error| anyhow::anyhow!(error)))
    {
        Ok(value) => value,
        Err(error) => return StopOutcome::Refused(error.to_string()),
    };
    let instance = match CString::new(instance_id) {
        Ok(value) => value,
        Err(error) => return StopOutcome::Refused(error.to_string()),
    };
    let mut raw = match CString::new(token) {
        Ok(value) => value.into_bytes_with_nul(),
        Err(error) => return StopOutcome::Refused(error.to_string()),
    };
    let mut sent: c_int = 0;
    // SAFETY: service and instance are live C strings. `raw` is NUL-terminated
    // for the call, then wiped. The C side does not retain the pointer.
    let rc = unsafe {
        a2s_mach_control_stop(
            service.as_ptr(),
            instance.as_ptr(),
            pid,
            raw.as_ptr() as *const c_char,
            &mut sent,
        )
    };
    raw.fill(0);
    if rc == 0 {
        StopOutcome::Exited
    } else if rc == 1 && sent == 0 {
        StopOutcome::Refused("Mach control refused before the admin token was sent".to_string())
    } else {
        StopOutcome::Unknown
    }
}

unsafe extern "C" {
    fn a2s_mach_control_start(
        service: *const c_char,
        instance: *const c_char,
        token: *const c_char,
    ) -> c_int;
    fn a2s_mach_control_stop(
        service: *const c_char,
        instance: *const c_char,
        pid: u32,
        token: *const c_char,
        token_sent: *mut c_int,
    ) -> c_int;
}

#[cfg(test)]
mod tests {
    use super::service_name;

    #[test]
    fn service_name_is_the_config_id() {
        let id = "0123456789abcdef0123456789abcdef";
        assert_eq!(service_name(id).unwrap(), format!("org.agent2ssh.{id}"));
        assert!(service_name("0123456789ABCDEF0123456789ABCDEF").is_err());
        assert!(service_name("not-a-config-id").is_err());
        assert!(service_name(&id[..31]).is_err());
    }
}
