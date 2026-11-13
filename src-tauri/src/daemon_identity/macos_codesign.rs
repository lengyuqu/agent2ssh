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
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SigningEvidenceError {
    /// No project release policy has been configured and reviewed yet.
    TrustRootUnavailable,
    Rejected,
}

// No team or release identifier has been selected. Never populate this from a
// daemon response, config file, environment variable, or an ad-hoc hash.
const COMPILED_RELEASE_REQUIREMENT: Option<&str> = None;

/// Check code identity without granting a bound-process or endpoint capability.
pub(crate) fn verify_release_code(audit: &KernelAuditToken) -> Result<(), SigningEvidenceError> {
    let requirement =
        COMPILED_RELEASE_REQUIREMENT.ok_or(SigningEvidenceError::TrustRootUnavailable)?;
    verify_requirement(audit, requirement)
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
        let (mut reader, writer) = UnixStream::pair().unwrap();
        reader
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let fd: std::os::fd::OwnedFd = writer.into();
        let mut command = Command::new(binary);
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
        let make_requirement = |team: &str, identifier: &str| {
            format!("anchor apple generic and certificate 1[field.1.2.840.113635.100.6.2.6] exists and certificate leaf[field.1.2.840.113635.100.6.1.13] exists and certificate leaf[subject.OU] = \"{team}\" and identifier \"{identifier}\"")
        };
        let requirement = make_requirement(&team, &identifier);
        assert_eq!(verify_requirement(&audit, &requirement), Ok(()));
        let wrong_team_id = if team == "AAAAAAAAAA" {
            "BBBBBBBBBB"
        } else {
            "AAAAAAAAAA"
        };
        let wrong_team = make_requirement(wrong_team_id, &identifier);
        assert_eq!(
            verify_requirement(&audit, &wrong_team),
            Err(SigningEvidenceError::Rejected)
        );
        let wrong_id = if identifier == "org.agent2ssh.incorrect" {
            "org.agent2ssh.other"
        } else {
            "org.agent2ssh.incorrect"
        };
        let wrong_identifier = make_requirement(&team, wrong_id);
        assert_eq!(
            verify_requirement(&audit, &wrong_identifier),
            Err(SigningEvidenceError::Rejected)
        );
        assert_eq!(
            verify_release_code(&audit),
            Err(SigningEvidenceError::TrustRootUnavailable)
        );
    }
}
