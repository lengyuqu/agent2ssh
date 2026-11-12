//! Fail-closed building blocks for the local daemon identity protocol.
//!
//! A hello and the operating system's peer PID are discovery evidence only.
//! In particular, macOS has no verified connection-to-process-object binding
//! here. This module deliberately cannot authorize a stop or transmit a token.

use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};

pub const VERSION: u32 = 1;
pub const MAX_FRAME: usize = 16 * 1024;

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum HelloFrame {
    #[serde(rename = "hello")]
    Hello { version: u32, client_nonce: String },
    #[serde(rename = "hello_ok")]
    HelloOk {
        version: u32,
        client_nonce: String,
        instance_id: String,
        pid: u32,
    },
}

fn valid_id(id: &str) -> bool {
    id.len() == 32
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl HelloFrame {
    pub fn validate(&self) -> io::Result<()> {
        let valid = match self {
            Self::Hello {
                version,
                client_nonce,
            } => *version == VERSION && valid_id(client_nonce),
            Self::HelloOk {
                version,
                client_nonce,
                instance_id,
                pid,
            } => {
                *version == VERSION && valid_id(client_nonce) && valid_id(instance_id) && *pid != 0
            }
        };
        if valid {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid identity hello",
            ))
        }
    }
}

/// Read exactly one bounded, strict hello frame. Callers must set socket and
/// overall deadlines; a `Read` implementation itself has no timeout contract.
pub fn read_hello(mut input: impl Read) -> io::Result<HelloFrame> {
    let mut header = [0u8; 4];
    input.read_exact(&mut header)?;
    let len = u32::from_be_bytes(header) as usize;
    if len == 0 || len > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid identity frame length",
        ));
    }
    let mut bytes = vec![0; len];
    input.read_exact(&mut bytes)?;
    let frame: HelloFrame = serde_json::from_slice(&bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid identity frame"))?;
    frame.validate()?;
    Ok(frame)
}

pub fn write_hello(mut output: impl Write, frame: &HelloFrame) -> io::Result<()> {
    frame.validate()?;
    let body = serde_json::to_vec(frame).map_err(io::Error::other)?;
    if body.len() > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "identity frame too large",
        ));
    }
    output.write_all(&(body.len() as u32).to_be_bytes())?;
    output.write_all(&body)
}

/// A successful comparison is still unbound. It is only a way to reject
/// inconsistent replies before any credential is sent.
pub fn check_hello_reply(
    reply: &HelloFrame,
    nonce: &str,
    instance: &str,
    expected_pid: u32,
    peer_pid: u32,
) -> io::Result<()> {
    reply.validate()?;
    match reply {
        HelloFrame::HelloOk {
            client_nonce,
            instance_id,
            pid,
            ..
        } if client_nonce == nonce
            && instance_id == instance
            && *pid == expected_pid
            && *pid == peer_pid =>
        {
            Ok(())
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "identity evidence conflicts",
        )),
    }
}

/// On macOS LOCAL_PEERPID is diagnostic evidence, never a process handle.
#[cfg(target_os = "macos")]
pub fn macos_peer_pid(stream: &std::os::unix::net::UnixStream) -> io::Result<u32> {
    use std::os::fd::AsRawFd;
    let mut pid: libc::pid_t = 0;
    let mut len = std::mem::size_of_val(&pid) as libc::socklen_t;
    // SAFETY: pid and len point to writable values of the advertised size;
    // stream keeps the socket open for the duration of getsockopt.
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_LOCAL,
            libc::LOCAL_PEERPID,
            (&mut pid as *mut libc::pid_t).cast(),
            &mut len,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    if len as usize != std::mem::size_of_val(&pid) || pid <= 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid peer PID",
        ));
    }
    Ok(pid as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn wire(body: &[u8]) -> Vec<u8> {
        let mut frame = (body.len() as u32).to_be_bytes().to_vec();
        frame.extend_from_slice(body);
        frame
    }
    #[test]
    fn strict_frames_reject_ambiguity_and_truncation() {
        let nonce = "0123456789abcdef0123456789abcdef";
        let frame = HelloFrame::Hello {
            version: 1,
            client_nonce: nonce.into(),
        };
        let mut bytes = Vec::new();
        write_hello(&mut bytes, &frame).unwrap();
        assert_eq!(read_hello(bytes.as_slice()).unwrap(), frame);
        for bad in [
            br#"{"type":"hello","version":1,"version":1,"client_nonce":"0123456789abcdef0123456789abcdef"}"#.as_slice(),
            br#"{"type":"hello","version":1,"client_nonce":"0123456789abcdef0123456789abcdef","extra":0}"#,
            br#"{"type":"hello","version":2,"client_nonce":"0123456789abcdef0123456789abcdef"}"#,
            br#"{"type":"authorize","version":1,"admin_token":"secret"}"#,
            b"\xff",
        ] { assert!(read_hello(wire(bad).as_slice()).is_err()); }
        assert!(read_hello([0, 0, 64, 1].as_slice()).is_err());
        assert!(read_hello([0, 0, 0, 4, b'{'].as_slice()).is_err());
    }
    #[test]
    fn conflicting_evidence_never_passes() {
        let id = "0123456789abcdef0123456789abcdef";
        let frame = HelloFrame::HelloOk {
            version: 1,
            client_nonce: id.into(),
            instance_id: id.into(),
            pid: 10,
        };
        assert!(check_hello_reply(&frame, id, id, 10, 10).is_ok());
        assert!(check_hello_reply(&frame, id, id, 10, 11).is_err());
        assert!(check_hello_reply(&frame, id, "ffffffffffffffffffffffffffffffff", 10, 10).is_err());
    }
}

/// macOS publication and recovery. This is an instance discovery record, not
/// a process identity or a credential.
#[cfg(target_os = "macos")]
pub mod macos;
