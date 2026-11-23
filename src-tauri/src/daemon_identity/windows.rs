//! Windows versioned instance record (#24 phase 2, #22 D6).
//!
//! The record names which process object a `daemon.pid` entry belongs to, at
//! the resolution `GetProcessTimes` actually provides: raw 100 ns FILETIME
//! ticks. The stop path compares those ticks against the handle it opened, so
//! a PID reused by another daemon-named image inside the same whole second is
//! refused instead of bound. A daemon that published no record (an older
//! version) keeps the seconds-resolution behaviour; the record is what upgrades
//! the comparison, never a second stop protocol.
//!
//! Shape relative to `docs/plans/daemon-identity-v1.md` §5: `version`,
//! `instance_id`, `pid` and `config_id` are as designed. There is no `endpoint`
//! field because Windows has no provable local control endpoint yet (§4.2
//! keeps it `Unbound`); inventing one would name a channel that does not exist.
//! `creation_ticks` replaces the diagnostic `start_hint`: on Windows the
//! creation time is a binding input, which is the hook #24's closing review
//! named (`bound_identity_matches`). It is still not a credential — the
//! comparison only decides whether an opened handle is the object the record
//! names.
//!
//! Locking: [`read`] takes `.daemon-pid.lock` itself. [`publish`],
//! [`Publication::cleanup_locked`], [`remove_if_named`], [`recover`] and
//! [`read_locked`] require the caller to already hold it
//! (`daemon_control::daemon_pid_lock`); taking it twice in one process would
//! deadlock.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};

pub const INSTANCE: &str = "daemon.instance.json";
const CONFIG_ID: &str = "daemon.config-id";
const MAX_RECORD: u64 = 4096;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Instance {
    pub version: u32,
    pub instance_id: String,
    pub pid: u32,
    pub config_id: String,
    /// Raw `GetProcessTimes().ftCreationTime`: 100 ns ticks since 1601-01-01.
    /// Not Unix seconds — converting is the reader's job, and truncating is
    /// exactly what this record exists to avoid.
    pub creation_ticks: u64,
}

impl Instance {
    fn validate(&self, expected_pid: u32) -> Result<()> {
        if self.version != super::VERSION
            || !super::valid_id(&self.instance_id)
            || !super::valid_id(&self.config_id)
            || self.pid == 0
            || self.pid != expected_pid
            || self.creation_ticks == 0
        {
            bail!("invalid daemon instance record; retained");
        }
        Ok(())
    }
}

fn new_id() -> Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes)
        .map_err(|e| anyhow::anyhow!("identity entropy unavailable: {e}"))?;
    Ok(hex::encode(bytes))
}

/// Raw creation ticks of the current process, from the kernel rather than a
/// process-table snapshot. `GetCurrentProcess` is a pseudo-handle that needs no
/// rights and cannot name another object.
pub fn own_creation_ticks() -> Result<u64> {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
    unsafe {
        let mut creation: FILETIME = std::mem::zeroed();
        let mut exit: FILETIME = std::mem::zeroed();
        let mut kernel: FILETIME = std::mem::zeroed();
        let mut user: FILETIME = std::mem::zeroed();
        if GetProcessTimes(
            GetCurrentProcess(),
            &mut creation,
            &mut exit,
            &mut kernel,
            &mut user,
        ) == 0
        {
            bail!(
                "cannot read own process creation time: {}",
                std::io::Error::last_os_error()
            );
        }
        Ok(((creation.dwHighDateTime as u64) << 32) | creation.dwLowDateTime as u64)
    }
}

fn regular_file(path: &Path) -> Result<fs::Metadata> {
    let md = fs::symlink_metadata(path)
        .with_context(|| format!("cannot inspect {}; retained", path.display()))?;
    if !md.file_type().is_file() {
        bail!("{} is not a regular file; retained", path.display());
    }
    Ok(md)
}

fn read_config_id(dir: &Path) -> Result<String> {
    let path = dir.join(CONFIG_ID);
    let md = regular_file(&path)?;
    if md.len() != 32 {
        bail!("invalid daemon config identifier; retained");
    }
    let id = fs::read_to_string(&path)?;
    if !super::valid_id(&id) {
        bail!("invalid daemon config identifier; retained");
    }
    Ok(id)
}

fn config_id(dir: &Path) -> Result<String> {
    let path = dir.join(CONFIG_ID);
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?;
            file.write_all(new_id()?.as_bytes())?;
            file.sync_all()?;
        }
        Err(e) => return Err(e.into()),
        Ok(_) => {}
    }
    read_config_id(dir)
}

fn read_record(dir: &Path, expected_pid: u32) -> Result<Instance> {
    let path = dir.join(INSTANCE);
    let md = regular_file(&path)?;
    if md.len() > MAX_RECORD {
        bail!("daemon instance record is too large; retained");
    }
    let record: Instance = serde_json::from_slice(&fs::read(&path)?)
        .context("invalid daemon instance record; retained")?;
    record.validate(expected_pid)?;
    if record.config_id != read_config_id(dir)? {
        bail!("daemon instance record names a different config identifier; retained");
    }
    Ok(record)
}

/// Read the record under the shared lock. `None` means no record exists — an
/// older daemon, whose stop keeps the seconds-resolution comparison. A present
/// but invalid, conflicting or unreadable record is an error: the stop refuses
/// rather than guessing which object the PID names.
pub fn read(dir: &Path, expected_pid: u32) -> Result<Option<Instance>> {
    let _guard = crate::store::lock_config_file(crate::daemon_control::DAEMON_PID_LOCK)?;
    read_locked(dir, expected_pid)
}

/// [`read`] for callers that already hold `.daemon-pid.lock`.
pub fn read_locked(dir: &Path, expected_pid: u32) -> Result<Option<Instance>> {
    match fs::symlink_metadata(dir.join(INSTANCE)) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
        Ok(_) => read_record(dir, expected_pid).map(Some),
    }
}

/// Refuse to write a PID record that would orphan or overwrite an instance
/// record. A crashed publication is recovered explicitly, never replaced.
pub fn refuse_existing_record(dir: &Path) -> Result<()> {
    match fs::symlink_metadata(dir.join(INSTANCE)) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
        Ok(_) => bail!(
            "daemon instance record exists; retained for explicit recovery. \
             Inspect it, then use 'agent2ssh daemon clear-stale --expect-pid <pid> \
             --expect-instance <instance_id>' only if the process has exited"
        ),
    }
}

pub struct Publication {
    pub instance: Instance,
    record: Vec<u8>,
}

/// Publish `daemon.pid` and the instance record. The caller holds
/// `.daemon-pid.lock` and has already refused a live or unverifiable previous
/// PID. Order follows v1 §5: PID first, then the record; a crash between them
/// leaves a legacy PID file (recoverable), never a record without a PID.
pub fn publish(dir: &Path, pid: u32) -> Result<Publication> {
    refuse_existing_record(dir)?;
    let instance = Instance {
        version: super::VERSION,
        instance_id: new_id()?,
        pid,
        config_id: config_id(dir)?,
        creation_ticks: own_creation_ticks()?,
    };
    let record = serde_json::to_vec(&instance)?;
    fs::write(dir.join("daemon.pid"), pid.to_string())
        .context("failed to write daemon PID file")?;
    crate::store::write_private_bytes(&dir.join(INSTANCE), &record)
        .context("failed to publish daemon instance record")?;
    Ok(Publication { instance, record })
}

impl Publication {
    /// Conditional cleanup on a normal shutdown, under the caller's lock.
    /// Both files must still byte-match what this daemon published; anything
    /// else is preserved for manual inspection. The record goes first so a
    /// crash mid-cleanup leaves a legacy PID file rather than an orphan record.
    pub fn cleanup_locked(self, dir: &Path) -> Result<bool> {
        let pid_path = dir.join("daemon.pid");
        let record_path = dir.join(INSTANCE);
        let pid_ok =
            fs::read(&pid_path).is_ok_and(|raw| raw == self.instance.pid.to_string().as_bytes());
        let record_ok = fs::read(&record_path).is_ok_and(|raw| raw == self.record);
        if !pid_ok || !record_ok {
            return Ok(false);
        }
        fs::remove_file(&record_path).context("failed to remove daemon instance record")?;
        fs::remove_file(&pid_path).context("failed to remove daemon PID file")?;
        Ok(true)
    }
}

/// Conditional removal for a client that did not publish the record itself
/// (token rotation). The caller holds `.daemon-pid.lock`. Both files must name
/// `pid`; a missing record means a legacy daemon and only the PID file is
/// removed. A corrupt, conflicting or unreadable record preserves both files
/// for explicit recovery — cleanup never guesses which object the PID names.
pub fn remove_if_named(dir: &Path, pid: u32) -> Result<bool> {
    let pid_path = dir.join("daemon.pid");
    let raw = match fs::read_to_string(&pid_path) {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e).context("failed to read daemon PID file for cleanup"),
    };
    if raw.trim().parse::<u32>().ok() != Some(pid) {
        return Ok(false);
    }
    match read_locked(dir, pid) {
        Err(_) => return Ok(false),
        Ok(Some(_)) => {
            fs::remove_file(dir.join(INSTANCE))
                .context("failed to remove daemon instance record")?;
        }
        Ok(None) => {}
    }
    fs::remove_file(&pid_path).context("failed to remove daemon PID file")?;
    Ok(true)
}

/// Explicit, operator-directed recovery of a crashed publication. The caller
/// holds the lock and has confirmed the PID is definitely absent. Both files
/// must match the expected instance; a changed or partial state is retained.
pub fn recover(dir: &Path, expected_pid: u32, expected_instance: &str) -> Result<()> {
    if !super::valid_id(expected_instance) {
        bail!("invalid expected instance id");
    }
    let pid_path = dir.join("daemon.pid");
    regular_file(&pid_path)?;
    let raw_pid = fs::read(&pid_path).context("cannot read daemon PID file; retained")?;
    if raw_pid != expected_pid.to_string().as_bytes() {
        bail!("daemon PID file differs from the expected instance; retained");
    }
    let record_path = dir.join(INSTANCE);
    regular_file(&record_path)?;
    let raw_record = fs::read(&record_path)?;
    let record: Instance =
        serde_json::from_slice(&raw_record).context("invalid daemon instance record; retained")?;
    record.validate(expected_pid)?;
    if record.instance_id != expected_instance {
        bail!("daemon instance record names a different instance; retained");
    }
    if record.config_id != read_config_id(dir)? {
        bail!("daemon instance record names a different config identifier; retained");
    }
    if fs::read(&pid_path)? != raw_pid || fs::read(&record_path)? != raw_record {
        bail!("identity records changed during recovery; retained");
    }
    fs::remove_file(&record_path).context("failed to remove daemon instance record")?;
    fs::remove_file(&pid_path).context("failed to remove daemon PID file")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(pid: u32) -> Instance {
        Instance {
            version: crate::daemon_identity::VERSION,
            instance_id: "0123456789abcdef0123456789abcdef".into(),
            pid,
            config_id: "fedcba9876543210fedcba9876543210".into(),
            creation_ticks: 13_400_000_000_000_000,
        }
    }

    #[test]
    fn validation_refuses_every_malformed_shape() {
        let base = record(42);
        assert!(base.validate(42).is_ok());
        assert!(
            base.validate(43).is_err(),
            "a record for another PID is a conflict"
        );
        for bad in [
            Instance {
                version: 2,
                ..base.clone()
            },
            Instance {
                version: 0,
                ..base.clone()
            },
            Instance {
                instance_id: "not-hex".into(),
                ..base.clone()
            },
            Instance {
                instance_id: "0123456789ABCDEF0123456789ABCDEF".into(),
                ..base.clone()
            },
            Instance {
                config_id: String::new(),
                ..base.clone()
            },
            Instance {
                pid: 0,
                ..base.clone()
            },
            Instance {
                creation_ticks: 0,
                ..base.clone()
            },
        ] {
            assert!(bad.validate(42).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn unknown_fields_and_truncation_are_refused() {
        let mut value = serde_json::to_value(record(42)).unwrap();
        value["extra"] = serde_json::json!(1);
        assert!(serde_json::from_value::<Instance>(value).is_err());
        assert!(serde_json::from_slice::<Instance>(br#"{"version":1"#).is_err());
    }

    #[test]
    fn own_creation_ticks_are_nonzero_and_stable() {
        let a = own_creation_ticks().unwrap();
        let b = own_creation_ticks().unwrap();
        assert_eq!(a, b, "one process has one creation time");
        assert!(a > 0);
    }

    #[test]
    fn publish_read_cleanup_round_trip() {
        let _config = crate::store::TestConfigDir::new("win-instance-roundtrip");
        let dir = crate::store::config_dir().unwrap();
        let _guard =
            crate::store::lock_config_file(crate::daemon_control::DAEMON_PID_LOCK).unwrap();
        let pid = std::process::id();
        let publication = publish(&dir, pid).unwrap();
        assert_eq!(
            publication.instance.creation_ticks,
            own_creation_ticks().unwrap()
        );
        let read_back = read_locked(&dir, pid).unwrap().unwrap();
        assert_eq!(read_back, publication.instance);
        assert!(dir.join("daemon.pid").exists());
        // A second publication over a live record is refused, not replaced.
        assert!(publish(&dir, pid).is_err());
        assert!(publication.cleanup_locked(&dir).unwrap());
        assert!(!dir.join(INSTANCE).exists());
        assert!(!dir.join("daemon.pid").exists());
    }

    #[test]
    fn cleanup_preserves_a_replaced_record() {
        let _config = crate::store::TestConfigDir::new("win-instance-replaced");
        let dir = crate::store::config_dir().unwrap();
        let _guard =
            crate::store::lock_config_file(crate::daemon_control::DAEMON_PID_LOCK).unwrap();
        let pid = std::process::id();
        let publication = publish(&dir, pid).unwrap();
        let replacement = br#"{"version":1,"instance_id":"ffffffffffffffffffffffffffffffff","pid":9,"config_id":"ffffffffffffffffffffffffffffffff","creation_ticks":1}"#;
        fs::write(dir.join(INSTANCE), replacement).unwrap();
        assert!(!publication.cleanup_locked(&dir).unwrap());
        assert_eq!(fs::read(dir.join(INSTANCE)).unwrap(), replacement);
        assert!(dir.join("daemon.pid").exists());
    }

    #[test]
    fn absent_record_reads_as_none_and_corrupt_record_is_an_error() {
        let _config = crate::store::TestConfigDir::new("win-instance-corrupt");
        let dir = crate::store::config_dir().unwrap();
        let _guard =
            crate::store::lock_config_file(crate::daemon_control::DAEMON_PID_LOCK).unwrap();
        assert!(read_locked(&dir, 42).unwrap().is_none());
        fs::write(dir.join(INSTANCE), b"partial").unwrap();
        assert!(read_locked(&dir, 42).is_err());
        assert!(refuse_existing_record(&dir).is_err());
    }

    #[test]
    fn remove_if_named_covers_legacy_valid_and_conflicting_records() {
        let _config = crate::store::TestConfigDir::new("win-instance-remove-if");
        let dir = crate::store::config_dir().unwrap();
        let _guard =
            crate::store::lock_config_file(crate::daemon_control::DAEMON_PID_LOCK).unwrap();
        let pid = std::process::id();
        // No PID file at all: nothing to remove.
        assert!(!remove_if_named(&dir, pid).unwrap());
        // Legacy shape: PID file without a record.
        fs::write(dir.join("daemon.pid"), pid.to_string()).unwrap();
        assert!(remove_if_named(&dir, pid).unwrap());
        assert!(!dir.join("daemon.pid").exists());
        // Valid record naming this PID: both files go.
        let _publication = publish(&dir, pid).unwrap();
        assert!(remove_if_named(&dir, pid).unwrap());
        assert!(!dir.join(INSTANCE).exists());
        assert!(!dir.join("daemon.pid").exists());
        // Corrupt record: both files are preserved.
        fs::write(dir.join("daemon.pid"), pid.to_string()).unwrap();
        fs::write(dir.join(INSTANCE), b"partial").unwrap();
        assert!(!remove_if_named(&dir, pid).unwrap());
        assert!(dir.join(INSTANCE).exists());
        assert!(dir.join("daemon.pid").exists());
        // A record naming another PID is a conflict, not a cleanup target.
        let other = serde_json::to_vec(&record(pid + 1)).unwrap();
        fs::write(dir.join(INSTANCE), &other).unwrap();
        assert!(!remove_if_named(&dir, pid).unwrap());
        assert_eq!(fs::read(dir.join(INSTANCE)).unwrap(), other);
        assert!(dir.join("daemon.pid").exists());
        // A PID file naming another PID is left alone too.
        fs::write(dir.join("daemon.pid"), (pid + 2).to_string()).unwrap();
        assert!(!remove_if_named(&dir, pid).unwrap());
    }

    #[test]
    fn recover_requires_the_named_instance_and_removes_both_files() {
        let _config = crate::store::TestConfigDir::new("win-instance-recover");
        let dir = crate::store::config_dir().unwrap();
        let _guard =
            crate::store::lock_config_file(crate::daemon_control::DAEMON_PID_LOCK).unwrap();
        let pid = std::process::id();
        let publication = publish(&dir, pid).unwrap();
        let instance_id = publication.instance.instance_id.clone();
        assert!(recover(&dir, pid, "ffffffffffffffffffffffffffffffff").is_err());
        assert!(recover(&dir, pid + 1, &instance_id).is_err());
        assert!(dir.join(INSTANCE).exists());
        recover(&dir, pid, &instance_id).unwrap();
        assert!(!dir.join(INSTANCE).exists());
        assert!(!dir.join("daemon.pid").exists());
    }
}
