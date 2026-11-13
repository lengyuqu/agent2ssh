//! Runtime code-signing evidence for a kernel-provided macOS audit token.
//!
//! A matching signature is not endpoint binding, process liveness or stop
//! authorization. The production client is deliberately not wired to use this
//! primitive until a trusted release policy and the full binding are ready.
use std::{
    ffi::{c_char, c_int, CString},
    io,
    os::{fd::AsRawFd, unix::net::UnixStream},
};

/// The Darwin audit token representation consumed by Security.framework.
/// Only create it from a kernel-supplied message or peer credential.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct KernelAuditToken([u32; 8]);

/// Read a kernel peer observation for diagnostics and rejection only. A Unix
/// socket and this snapshot do not prove which process will receive later data.
pub(super) fn peer_audit_token(stream: &UnixStream) -> io::Result<KernelAuditToken> {
    let mut token = KernelAuditToken([0; 8]);
    let mut len = std::mem::size_of_val(&token) as libc::socklen_t;
    // SAFETY: token and len point to writable storage of the declared size;
    // stream owns an open Unix socket throughout the call.
    let rc = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_LOCAL,
            libc::LOCAL_PEERTOKEN,
            (&mut token as *mut KernelAuditToken).cast(),
            &mut len,
        )
    };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    if len as usize != std::mem::size_of_val(&token) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid peer audit token size",
        ));
    }
    Ok(token)
}

unsafe extern "C" {
    fn a2s_verify_running_code(audit: *const KernelAuditToken, requirement: *const c_char)
        -> c_int;
    fn a2s_task_audit_matches(
        audit: *const KernelAuditToken,
        name: libc::mach_port_t,
        expected_pid: libc::pid_t,
    ) -> c_int;
    fn a2s_exit_observer_create(
        audit: *const KernelAuditToken,
        task_identity: libc::mach_port_t,
        expected_pid: libc::pid_t,
        out: *mut *mut std::ffi::c_void,
    ) -> c_int;
    fn a2s_exit_observer_wait(observer: *mut std::ffi::c_void, timeout_ms: u32) -> c_int;
    fn a2s_exit_observer_destroy(observer: *mut std::ffi::c_void);
}

/// Compare a kernel-provided audit observation with a held task port. The
/// caller retains the port; a match alone does not prove endpoint ownership.
pub(crate) fn task_audit_matches(
    audit: &KernelAuditToken,
    name: libc::mach_port_t,
    expected_pid: u32,
) -> bool {
    let Ok(pid) = libc::pid_t::try_from(expected_pid) else {
        return false;
    };
    // SAFETY: audit has the asserted Darwin layout; name is a borrowed port
    // name and the C helper does not consume or transfer the right.
    unsafe { a2s_task_audit_matches(audit, name, pid) == 1 }
}

/// A candidate exit event observer. It owns its queue and task name port but
/// does not own a control endpoint or authorize a stop/record cleanup.
pub(crate) struct ExitObserver {
    raw: std::ptr::NonNull<std::ffi::c_void>,
    observed: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ExitObservation {
    TimedOut,
    EventObserved,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ExitObserverUnavailable;

impl ExitObserver {
    pub(crate) fn create(
        audit: &KernelAuditToken,
        task_identity: libc::mach_port_t,
        expected_pid: u32,
    ) -> Result<Self, ExitObserverUnavailable> {
        let pid = libc::pid_t::try_from(expected_pid).map_err(|_| ExitObserverUnavailable)?;
        let mut raw = std::ptr::null_mut();
        // SAFETY: the audit token lives throughout the call, task_identity is
        // borrowed, and `raw` points to writable output storage.
        if unsafe { a2s_exit_observer_create(audit, task_identity, pid, &mut raw) } != 0 {
            return Err(ExitObserverUnavailable);
        }
        let raw = std::ptr::NonNull::new(raw).ok_or(ExitObserverUnavailable)?;
        Ok(Self {
            raw,
            observed: false,
        })
    }

    pub(crate) fn wait(
        &mut self,
        timeout: std::time::Duration,
    ) -> Result<ExitObservation, ExitObserverUnavailable> {
        if self.observed {
            return Ok(ExitObservation::EventObserved);
        }
        // A positive fractional millisecond must not become an immediate poll.
        let millis = u32::try_from(timeout.as_nanos().div_ceil(1_000_000))
            .map_err(|_| ExitObserverUnavailable)?;
        // SAFETY: raw remains uniquely owned and valid until Drop.
        match unsafe { a2s_exit_observer_wait(self.raw.as_ptr(), millis) } {
            0 => Ok(ExitObservation::TimedOut),
            1 => {
                self.observed = true;
                Ok(ExitObservation::EventObserved)
            }
            _ => Err(ExitObserverUnavailable),
        }
    }
}

impl Drop for ExitObserver {
    fn drop(&mut self) {
        // SAFETY: this object uniquely owns the pointer returned by create.
        unsafe { a2s_exit_observer_destroy(self.raw.as_ptr()) };
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SigningEvidenceError {
    /// No project release policy has been configured and reviewed yet.
    TrustRootUnavailable,
    Rejected,
}

/// Only a reviewed, immutable release policy may construct a code requirement.
/// Matching this policy is still not a bound endpoint or stop authorization.
struct ReleaseIdentity<'a> {
    team: &'a str,
    identifier: &'a str,
    cdhash: &'a str,
}

impl ReleaseIdentity<'_> {
    fn requirement(&self) -> Result<String, SigningEvidenceError> {
        if self.team.len() != 10
            || !self
                .team
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
            || self.identifier.is_empty()
            || self.identifier.len() > 255
            || !self.identifier.as_bytes()[0].is_ascii_alphanumeric()
            || !self
                .identifier
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            || self.cdhash.len() != 40
            || !self.cdhash.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(SigningEvidenceError::Rejected);
        }
        Ok(format!(
            "anchor apple generic and certificate 1[field.1.2.840.113635.100.6.2.6] exists and certificate leaf[field.1.2.840.113635.100.6.1.13] exists and certificate leaf[subject.OU] = \"{}\" and identifier \"{}\" and cdhash H\"{}\"",
            self.team, self.identifier, self.cdhash
        ))
    }
}

// No project team, release identifier, or release CDHash has been selected.
// Never populate this from a daemon response, config file or environment.
// Multi-architecture releases and safe hash rotation remain undecided.
const COMPILED_RELEASE_IDENTITY: Option<ReleaseIdentity<'static>> = None;

/// Check code identity without granting a bound-process or endpoint capability.
pub(crate) fn verify_release_code(audit: &KernelAuditToken) -> Result<(), SigningEvidenceError> {
    let identity = COMPILED_RELEASE_IDENTITY
        .as_ref()
        .ok_or(SigningEvidenceError::TrustRootUnavailable)?;
    verify_identity(audit, identity)
}

fn verify_identity(
    audit: &KernelAuditToken,
    identity: &ReleaseIdentity<'_>,
) -> Result<(), SigningEvidenceError> {
    verify_requirement(audit, &identity.requirement()?)
}

fn verify_requirement(
    audit: &KernelAuditToken,
    requirement: &str,
) -> Result<(), SigningEvidenceError> {
    let requirement = CString::new(requirement).map_err(|_| SigningEvidenceError::Rejected)?;
    // SAFETY: `audit` is a Darwin audit-token-shaped value, and the CString is
    // NUL-terminated and remains alive for the entire synchronous call.
    if unsafe { a2s_verify_running_code(audit, requirement.as_ptr()) } == 1 {
        Ok(())
    } else {
        Err(SigningEvidenceError::Rejected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        os::{fd::AsRawFd, unix::net::UnixStream},
        path::Path,
        process::{Child, Command, Stdio},
    };

    struct TestChild(Child);
    impl Drop for TestChild {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    fn clean_env(command: &mut Command) {
        for (key, _) in std::env::vars_os() {
            let name = key.to_string_lossy();
            if name.starts_with("DYLD_")
                || name.starts_with("AGENT2SSH_")
                || name.starts_with("A2S_MACH_LAB_")
                || name.starts_with("A2S_TEST_DEVELOPER_")
            {
                command.env_remove(key);
            }
        }
    }

    fn run(program: &str, args: &[&std::ffi::OsStr]) -> std::process::Output {
        let mut command = Command::new(program);
        clean_env(&mut command);
        let result = command.args(args).output().unwrap();
        assert!(
            result.status.success(),
            "{program}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        result
    }

    fn signed_child(
        dir: &Path,
        entitlement: Option<&str>,
        hardened: bool,
    ) -> (TestChild, KernelAuditToken, String) {
        let source = dir.join("synthetic.c");
        let binary = dir.join("synthetic");
        std::fs::write(&source, b"#include <unistd.h>\nint main(void) { if (write(1, \"x\", 1) != 1) return 2; sleep(30); return 0; }\n").unwrap();
        run(
            "/usr/bin/clang",
            &[source.as_os_str(), "-o".as_ref(), binary.as_os_str()],
        );
        let mut args: Vec<&std::ffi::OsStr> = vec![
            "--force".as_ref(),
            "--sign".as_ref(),
            "-".as_ref(),
            "--identifier".as_ref(),
            "org.agent2ssh.signing-test".as_ref(),
        ];
        if hardened {
            args.extend([
                std::ffi::OsStr::new("--options"),
                std::ffi::OsStr::new("runtime"),
            ]);
        }
        let plist = dir.join("entitlements.plist");
        if let Some(key) = entitlement {
            std::fs::write(&plist, format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><plist version=\"1.0\"><dict><key>{key}</key><true/></dict></plist>")).unwrap();
            args.extend(["--entitlements".as_ref(), plist.as_os_str()]);
        }
        args.push(binary.as_os_str());
        run("/usr/bin/codesign", &args);
        let details = run(
            "/usr/bin/codesign",
            &["-dv".as_ref(), "--verbose=4".as_ref(), binary.as_os_str()],
        );
        let details = String::from_utf8(details.stderr).unwrap();
        let hash = details
            .lines()
            .find_map(|line| line.strip_prefix("CDHash="))
            .unwrap();
        assert_eq!(hash.len(), 40);
        let requirement =
            format!("identifier \"org.agent2ssh.signing-test\" and cdhash H\"{hash}\"");
        let (mut reader, writer) = UnixStream::pair().unwrap();
        reader
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let fd: std::os::fd::OwnedFd = writer.into();
        let mut command = Command::new(binary);
        clean_env(&mut command);
        let child = command
            .stdin(Stdio::null())
            .stdout(Stdio::from(fd))
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let child = TestChild(child);
        let mut byte = [0u8; 1];
        reader.read_exact(&mut byte).unwrap();
        assert_eq!(byte, *b"x");
        let audit = peer_audit_token(&reader).unwrap();
        let mut peer: libc::pid_t = 0;
        let mut peer_len = std::mem::size_of_val(&peer) as libc::socklen_t;
        let rc = unsafe {
            libc::getsockopt(
                reader.as_raw_fd(),
                libc::SOL_LOCAL,
                libc::LOCAL_PEERPID,
                (&mut peer as *mut libc::pid_t).cast(),
                &mut peer_len,
            )
        };
        assert_eq!(rc, 0, "{}", std::io::Error::last_os_error());
        assert_eq!(peer_len as usize, std::mem::size_of_val(&peer));
        assert_eq!(peer as u32, child.0.id());
        (child, audit, requirement)
    }

    #[test]
    fn dynamic_code_evidence_refuses_missing_root_wrong_code_and_unsafe_runtime() {
        let clean = tempfile::tempdir_in("/tmp").unwrap();
        let (_child, audit, requirement) = signed_child(clean.path(), None, true);
        assert_eq!(
            verify_release_code(&audit),
            Err(SigningEvidenceError::TrustRootUnavailable)
        );
        assert_eq!(verify_requirement(&audit, &requirement), Ok(()));
        assert_eq!(
            verify_requirement(&audit, "anchor apple generic"),
            Err(SigningEvidenceError::Rejected)
        );
        assert_eq!(
            verify_requirement(&audit, "identifier \"org.agent2ssh.wrong\""),
            Err(SigningEvidenceError::Rejected)
        );
        assert_eq!(
            verify_requirement(&audit, "bad\0requirement"),
            Err(SigningEvidenceError::Rejected)
        );
        assert_eq!(
            verify_requirement(&audit, ""),
            Err(SigningEvidenceError::Rejected)
        );

        let jit = tempfile::tempdir_in("/tmp").unwrap();
        let (_child, audit, requirement) =
            signed_child(jit.path(), Some("com.apple.security.cs.allow-jit"), true);
        assert_eq!(
            verify_requirement(&audit, &requirement),
            Err(SigningEvidenceError::Rejected)
        );
        let plain = tempfile::tempdir_in("/tmp").unwrap();
        let (_child, audit, requirement) = signed_child(plain.path(), None, false);
        assert_eq!(
            verify_requirement(&audit, &requirement),
            Err(SigningEvidenceError::Rejected)
        );
    }

    #[test]
    #[allow(deprecated)]
    fn task_audit_evidence_rejects_mismatched_task_or_pid() {
        // TASK_AUDIT_TOKEN is 15 in the public Darwin SDK. The self task
        // port is borrowed and remains owned by the process.
        let task = unsafe { libc::mach_task_self() };
        let mut audit = KernelAuditToken([0; 8]);
        let mut count = 8;
        let rc = unsafe {
            libc::task_info(
                task,
                15,
                (&mut audit as *mut KernelAuditToken).cast(),
                &mut count,
            )
        };
        assert_eq!(rc, 0);
        assert_eq!(count, 8);
        assert!(task_audit_matches(&audit, task, std::process::id()));
        let mut altered = audit;
        altered.0[7] ^= 1;
        assert!(!task_audit_matches(&altered, task, std::process::id()));
        assert!(!task_audit_matches(
            &audit,
            task,
            std::process::id().wrapping_add(1)
        ));
        assert!(!task_audit_matches(&audit, 0, std::process::id()));
        assert!(!task_audit_matches(&audit, task, u32::MAX));
    }

    #[test]
    #[allow(deprecated)]
    fn exit_observer_requires_matching_task_and_keeps_timeouts_distinct() {
        unsafe extern "C" {
            fn task_create_identity_token(
                task: libc::mach_port_t,
                token: *mut libc::mach_port_t,
            ) -> libc::c_int;
            fn mach_port_deallocate(
                task: libc::mach_port_t,
                name: libc::mach_port_t,
            ) -> libc::c_int;
        }
        let task = unsafe { libc::mach_task_self() };
        let mut audit = KernelAuditToken([0; 8]);
        let mut count = 8;
        assert_eq!(
            unsafe {
                libc::task_info(
                    task,
                    15,
                    (&mut audit as *mut KernelAuditToken).cast(),
                    &mut count,
                )
            },
            0
        );
        assert_eq!(count, 8);
        let mut identity = 0;
        assert_eq!(
            unsafe { task_create_identity_token(task, &mut identity) },
            0
        );
        assert_ne!(identity, 0);
        let mut observer = ExitObserver::create(&audit, identity, std::process::id()).unwrap();
        assert_eq!(
            observer.wait(std::time::Duration::ZERO),
            Ok(ExitObservation::TimedOut)
        );
        assert_eq!(
            observer.wait(std::time::Duration::from_nanos(1)),
            Ok(ExitObservation::TimedOut)
        );
        assert_eq!(
            observer.wait(std::time::Duration::from_millis(u64::from(u32::MAX) + 1)),
            Err(ExitObserverUnavailable)
        );
        let mut altered = audit;
        altered.0[7] ^= 1;
        assert!(matches!(
            ExitObserver::create(&altered, identity, std::process::id()),
            Err(ExitObserverUnavailable)
        ));
        assert!(matches!(
            ExitObserver::create(&audit, 0, std::process::id()),
            Err(ExitObserverUnavailable)
        ));
        assert!(matches!(
            ExitObserver::create(&audit, identity, u32::MAX),
            Err(ExitObserverUnavailable)
        ));
        drop(observer);
        assert_eq!(unsafe { mach_port_deallocate(task, identity) }, 0);
    }

    #[test]
    fn release_identity_rejects_invalid_or_injectable_values() {
        let valid = ReleaseIdentity {
            team: "ABCDEFGHIJ",
            identifier: "org.agent2ssh.daemon",
            cdhash: "0123456789abcdef0123456789abcdef01234567",
        };
        assert!(valid.requirement().is_ok());
        for bad in [
            ReleaseIdentity {
                team: "short",
                identifier: valid.identifier,
                cdhash: valid.cdhash,
            },
            ReleaseIdentity {
                team: valid.team,
                identifier: "daemon\" or true",
                cdhash: valid.cdhash,
            },
            ReleaseIdentity {
                team: valid.team,
                identifier: valid.identifier,
                cdhash: "abc",
            },
            ReleaseIdentity {
                team: valid.team,
                identifier: valid.identifier,
                cdhash: "0123456789abcdef0123456789abcdef0123456!",
            },
        ] {
            assert_eq!(bad.requirement(), Err(SigningEvidenceError::Rejected));
        }
    }

    #[test]
    fn optional_running_developer_id_fixture_is_checked_dynamically() {
        let (Some(binary), Ok(team), Ok(identifier)) = (
            std::env::var_os("A2S_TEST_DEVELOPER_BINARY"),
            std::env::var("A2S_TEST_DEVELOPER_TEAM"),
            std::env::var("A2S_TEST_DEVELOPER_IDENTIFIER"),
        ) else {
            eprintln!(
                "no signed ripgrep fixture configured; skipping optional Developer ID observation"
            );
            return;
        };
        assert_eq!(team.len(), 10);
        assert!(team
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit()));
        assert!(
            !identifier.is_empty()
                && identifier
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
        );
        let binary = std::path::PathBuf::from(binary);
        let details = run(
            "/usr/bin/codesign",
            &["-dv".as_ref(), "--verbose=4".as_ref(), binary.as_os_str()],
        );
        let details = String::from_utf8(details.stderr).unwrap();
        let cdhash = details
            .lines()
            .find_map(|line| line.strip_prefix("CDHash="))
            .unwrap();
        let (mut reader, writer) = UnixStream::pair().unwrap();
        reader
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let fd: std::os::fd::OwnedFd = writer.into();
        let mut command = Command::new(&binary);
        clean_env(&mut command);
        // ripgrep is explicitly fed one synthetic line through stdin; it does
        // not scan files or access the user's application configuration.
        let child = command
            .args(["--no-config", "--line-buffered", "^A2S-SYNTHETIC$", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::from(fd))
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut child = TestChild(child);
        child
            .0
            .stdin
            .as_mut()
            .unwrap()
            .write_all(b"A2S-SYNTHETIC\n")
            .unwrap();
        let mut line = [0u8; 14];
        reader.read_exact(&mut line).unwrap();
        assert_eq!(&line, b"A2S-SYNTHETIC\n");
        let audit = peer_audit_token(&reader).unwrap();
        let mut peer: libc::pid_t = 0;
        let mut len = std::mem::size_of_val(&peer) as libc::socklen_t;
        let rc = unsafe {
            libc::getsockopt(
                reader.as_raw_fd(),
                libc::SOL_LOCAL,
                libc::LOCAL_PEERPID,
                (&mut peer as *mut libc::pid_t).cast(),
                &mut len,
            )
        };
        assert_eq!(rc, 0, "{}", std::io::Error::last_os_error());
        assert_eq!(len as usize, std::mem::size_of_val(&peer));
        assert_eq!(peer as u32, child.0.id());
        let identity = ReleaseIdentity {
            team: &team,
            identifier: &identifier,
            cdhash,
        };
        assert_eq!(verify_identity(&audit, &identity), Ok(()));
        let wrong_team_id = if team == "AAAAAAAAAA" {
            "BBBBBBBBBB"
        } else {
            "AAAAAAAAAA"
        };
        let wrong_team = ReleaseIdentity {
            team: wrong_team_id,
            identifier: &identifier,
            cdhash,
        };
        assert_eq!(
            verify_identity(&audit, &wrong_team),
            Err(SigningEvidenceError::Rejected)
        );
        let wrong_id = if identifier == "org.agent2ssh.incorrect" {
            "org.agent2ssh.other"
        } else {
            "org.agent2ssh.incorrect"
        };
        let wrong_identifier = ReleaseIdentity {
            team: &team,
            identifier: wrong_id,
            cdhash,
        };
        assert_eq!(
            verify_identity(&audit, &wrong_identifier),
            Err(SigningEvidenceError::Rejected)
        );
        let wrong_hash = ReleaseIdentity {
            team: &team,
            identifier: &identifier,
            cdhash: "0000000000000000000000000000000000000000",
        };
        assert_eq!(
            verify_identity(&audit, &wrong_hash),
            Err(SigningEvidenceError::Rejected)
        );
        assert_eq!(
            verify_release_code(&audit),
            Err(SigningEvidenceError::TrustRootUnavailable)
        );
    }
}
