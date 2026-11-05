//! What the local daemon is holding that a stop would destroy, and the tiny
//! HTTP client used to ask it.
//!
//! PTY sessions, resident shells, port forwards and pending approvals live in
//! the daemon's memory (see `session.rs`, `resident_session.rs`, `forward.rs`,
//! `approval.rs`), so stopping the daemon ends all of them without a trace.
//! The desktop app uses [`fetch_daemon_activity`] to tell the person that
//! before it does.
//!
//! The client is a raw `TcpStream`, like `daemon_control::daemon_health_ok`,
//! rather than `reqwest::blocking`: the CLI calls into this from inside a tokio
//! runtime, where building a blocking reqwest client panics.

use anyhow::{anyhow, Result};
use serde::Serialize;
use std::{
    io::{Read, Write},
    net::{TcpStream, ToSocketAddrs},
    time::Duration,
};

use crate::store::config_dir;

/// One response from the local daemon.
#[derive(Debug)]
pub struct LocalResponse {
    pub status: u16,
    pub body: String,
}

/// Send one bodiless request to the local daemon and read the whole response.
///
/// `Connection: close` makes end-of-stream the end of the response, so there is
/// no framing to parse beyond the header/body split. Every daemon route this is
/// used for answers with a `Content-Length` JSON body.
pub fn local_daemon_request(
    method: &str,
    path: &str,
    token: Option<&str>,
    timeout: Duration,
) -> Result<LocalResponse> {
    request_at(
        &crate::local_daemon_connect_addr(),
        method,
        path,
        token,
        timeout,
        None,
    )
}

/// Header used by guarded shutdown requests to identify the intended daemon.
pub const EXPECTED_PID_HEADER: &str = "X-Agent2SSH-Expected-Pid";

/// Send a local request that the recipient must bind to `expected_pid`.
pub fn local_daemon_request_for_pid(
    method: &str,
    path: &str,
    token: Option<&str>,
    timeout: Duration,
    expected_pid: u32,
) -> Result<LocalResponse> {
    request_at(
        &crate::local_daemon_connect_addr(),
        method,
        path,
        token,
        timeout,
        Some(expected_pid),
    )
}

/// [`local_daemon_request`] against an explicit `host:port`.
fn request_at(
    addr: &str,
    method: &str,
    path: &str,
    token: Option<&str>,
    timeout: Duration,
    expected_pid: Option<u32>,
) -> Result<LocalResponse> {
    let addr = addr
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| anyhow!("the local daemon address did not resolve"))?;
    let mut stream = TcpStream::connect_timeout(&addr, timeout)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;

    let authorization = token
        .map(|token| format!("Authorization: Bearer {token}\r\n"))
        .unwrap_or_default();
    let expected = expected_pid
        .map(|pid| format!("{EXPECTED_PID_HEADER}: {pid}\r\n"))
        .unwrap_or_default();
    // One write: `write!` straight to the socket issues a write per format
    // fragment, and a server that reads the request in one go sees only the first.
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n{authorization}{expected}Content-Length: 0\r\n\
         Connection: close\r\n\r\n"
    );
    stream.write_all(request.as_bytes())?;

    let mut raw = Vec::new();
    stream.read_to_end(&mut raw)?;
    let raw = String::from_utf8_lossy(&raw);
    let (head, body) = raw
        .split_once("\r\n\r\n")
        .ok_or_else(|| anyhow!("the daemon sent a response with no header terminator"))?;
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| anyhow!("the daemon sent an unreadable status line"))?;
    Ok(LocalResponse {
        status,
        body: body.to_string(),
    })
}

/// The admin bearer token the daemon wrote to its config directory.
pub fn read_admin_token() -> Option<String> {
    let raw = std::fs::read_to_string(config_dir().ok()?.join("daemon.token")).ok()?;
    let token = raw.trim();
    (!token.is_empty()).then(|| token.to_string())
}

/// How much live state the daemon is holding.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct DaemonActivity {
    pub pty_sessions: usize,
    pub resident_sessions: usize,
    pub forwards: usize,
    pub pending_approvals: usize,
}

impl DaemonActivity {
    pub fn is_idle(&self) -> bool {
        *self == Self::default()
    }
}

/// Count the elements of a JSON array body that satisfy `keep`.
///
/// `None` when the body is not an array, which is how a changed or error
/// response is told apart from an empty list.
fn count_listed(body: &str, keep: impl Fn(&serde_json::Value) -> bool) -> Option<usize> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    Some(value.as_array()?.iter().filter(|item| keep(item)).count())
}

/// Build a [`DaemonActivity`] from the four list bodies the daemon serves.
///
/// A resident session that has `dead` set is a husk the daemon is about to
/// reap, not something a stop would cost anyone, and an approval only counts
/// while it is still `pending`.
pub fn summarize_activity(
    sessions: &str,
    resident: &str,
    forwards: &str,
    approvals: &str,
) -> Option<DaemonActivity> {
    Some(DaemonActivity {
        pty_sessions: count_listed(sessions, |_| true)?,
        resident_sessions: count_listed(resident, |item| {
            item.get("dead").and_then(|dead| dead.as_bool()) != Some(true)
        })?,
        forwards: count_listed(forwards, |_| true)?,
        pending_approvals: count_listed(approvals, |item| {
            item.get("status").and_then(|status| status.as_str()) == Some("pending")
        })?,
    })
}

const ACTIVITY_TIMEOUT: Duration = Duration::from_secs(2);

/// Ask the running daemon what a stop would end.
///
/// `None` means *unknown* — the daemon did not answer, refused the token, or
/// answered something unexpected — not *idle*. Callers decide what unknown
/// should do; this never guesses in either direction.
pub fn fetch_daemon_activity() -> Option<DaemonActivity> {
    let token = read_admin_token()?;
    let get = |path: &str| -> Option<String> {
        let response = local_daemon_request("GET", path, Some(&token), ACTIVITY_TIMEOUT).ok()?;
        (response.status == 200).then_some(response.body)
    };
    summarize_activity(
        &get("/sessions")?,
        &get("/resident-sessions")?,
        &get("/forwards")?,
        &get("/approvals")?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_lists_are_idle() {
        let activity = summarize_activity("[]", "[]", "[]", "[]").unwrap();
        assert!(activity.is_idle());
    }

    #[test]
    fn every_kind_of_live_state_is_counted() {
        let activity = summarize_activity(
            r#"[{"id":"a","host":"h"},{"id":"b","host":"h"}]"#,
            r#"[{"id":"r","host":"local","dead":false}]"#,
            r#"[{"id":"f"}]"#,
            r#"[{"id":"p","status":"pending"},{"id":"q","status":"pending"},{"id":"x","status":"approved"}]"#,
        )
        .unwrap();
        assert_eq!(
            activity,
            DaemonActivity {
                pty_sessions: 2,
                resident_sessions: 1,
                forwards: 1,
                pending_approvals: 2,
            }
        );
        assert!(!activity.is_idle());
    }

    #[test]
    fn a_dead_resident_session_and_a_settled_approval_do_not_count() {
        let activity = summarize_activity(
            "[]",
            r#"[{"id":"r","dead":true}]"#,
            "[]",
            r#"[{"status":"timed_out"},{"status":"rejected"},{"status":"revoked"}]"#,
        )
        .unwrap();
        assert!(activity.is_idle(), "{activity:?}");
    }

    #[test]
    fn an_unexpected_body_is_unknown_not_idle() {
        assert!(summarize_activity("[]", "[]", "[]", r#"{"error":"nope"}"#).is_none());
        assert!(summarize_activity("not json", "[]", "[]", "[]").is_none());
    }

    #[test]
    fn a_request_reads_status_and_body() {
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            // Read to the end of the headers, as a real server does.
            let mut request = String::new();
            let mut chunk = [0u8; 256];
            while !request.contains("\r\n\r\n") {
                let read = stream.read(&mut chunk).unwrap();
                assert!(read > 0, "the client closed before finishing the request");
                request.push_str(&String::from_utf8_lossy(&chunk[..read]));
            }
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 11\r\nConnection: close\r\n\r\n{\"ok\":true}\n",
                )
                .unwrap();
            request
        });

        let response = request_at(
            &format!("127.0.0.1:{port}"),
            "POST",
            "/shutdown",
            Some("tok"),
            Duration::from_secs(2),
            Some(42),
        );

        let response = response.unwrap();
        assert_eq!(response.status, 200);
        assert!(response.body.contains("\"ok\":true"));
        let request = server.join().unwrap();
        assert!(request.starts_with("POST /shutdown HTTP/1.1"), "{request}");
        assert!(request.contains("Authorization: Bearer tok"), "{request}");
        assert!(
            request.contains("X-Agent2SSH-Expected-Pid: 42"),
            "{request}"
        );
    }
}
