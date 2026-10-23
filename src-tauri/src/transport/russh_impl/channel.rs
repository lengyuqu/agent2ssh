//! A synchronous view of a russh channel.
//!
//! This is where the six contracts from the migration plan actually live. russh
//! delivers stdout, stderr, EOF, the exit status and the close as messages on
//! *one* stream, while the rest of this crate reads stdout and stderr as two
//! independent `Read` cursors and polls both until they report EOF. The split
//! happens here: a reader task owns the read half and demultiplexes into two
//! queues, and every public method is a synchronous view of that state.
//!
//! Three consequences are deliberate and should not be "simplified" later:
//!
//! - `stderr()` is a cursor into a second queue, not a second socket. There is
//!   only one stream underneath.
//! - `eof()` means "the incoming stream ended **and** both queues are drained".
//!   EOF, Close, and a dropped reader all end that stream; returning true before
//!   both queues drain truncates stderr.
//! - the blocking flag and the timeout are separate knobs. Non-blocking means
//!   "return `WouldBlock` now"; a timeout bounds how long a *blocking* call may
//!   wait. Collapsing them breaks the exec teardown path, which switches to
//!   blocking and then calls `wait_close`.

use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use russh::client::Msg;
use russh::{ChannelMsg, ChannelWriteHalf};

use super::block_on;
use crate::transport::types::Error;

/// Session-wide blocking/timeout state.
///
/// Shared by every channel of one session because libssh2's equivalents were
/// session-level, and callers switch modes mid-flight
/// (`embedded_ssh.rs` sets non-blocking, drains, then restores blocking for the
/// teardown handshake).
pub(super) struct Settings {
    blocking: AtomicBool,
    timeout_ms: AtomicU64,
    aborted: AtomicBool,
    socket: Mutex<Option<TcpStream>>,
}

impl Settings {
    pub(super) fn new() -> Self {
        Self {
            blocking: AtomicBool::new(true),
            timeout_ms: AtomicU64::new(0),
            aborted: AtomicBool::new(false),
            socket: Mutex::new(None),
        }
    }

    pub(super) fn set_blocking(&self, blocking: bool) {
        self.blocking.store(blocking, Ordering::SeqCst);
    }

    pub(super) fn blocking(&self) -> bool {
        self.blocking.load(Ordering::SeqCst)
    }

    /// `0` keeps libssh2's meaning of "no timeout", although the caller already
    /// clamps a zero-second budget to 1 ms before it gets here.
    pub(super) fn set_timeout(&self, timeout_ms: u32) {
        self.timeout_ms
            .store(u64::from(timeout_ms), Ordering::SeqCst);
    }

    pub(super) fn timeout(&self) -> Option<Duration> {
        match self.timeout_ms.load(Ordering::SeqCst) {
            0 => None,
            ms => Some(Duration::from_millis(ms)),
        }
    }

    pub(super) fn set_socket(&self, socket: &TcpStream) -> io::Result<()> {
        *self.socket.lock().unwrap() = Some(socket.try_clone()?);
        self.aborted.store(false, Ordering::SeqCst);
        Ok(())
    }

    pub(super) fn aborted(&self) -> bool {
        self.aborted.load(Ordering::SeqCst)
    }

    /// Cancelling a request future does not cancel russh's connection task.
    /// Shut down its socket too, so a silent peer cannot retain a worker or
    /// deliver a late authentication/request reply to a subsequent operation.
    pub(super) fn abort(&self) {
        self.aborted.store(true, Ordering::SeqCst);
        if let Some(socket) = self.socket.lock().unwrap().take() {
            let _ = socket.shutdown(Shutdown::Both);
        }
    }

    pub(super) fn run<T, F>(&self, operation: &str, future: F) -> Result<T, Error>
    where
        T: Send,
        F: std::future::Future<Output = Result<T, Error>> + Send,
    {
        self.run_with_abort(operation, future, true)
    }

    /// Agent discovery uses a separate socket and has not sent an SSH request.
    /// Timing it out must leave SSH available for fallback key authentication.
    pub(super) fn run_independent<T, F>(&self, operation: &str, future: F) -> Result<T, Error>
    where
        T: Send,
        F: std::future::Future<Output = Result<T, Error>> + Send,
    {
        self.run_with_abort(operation, future, false)
    }

    fn run_with_abort<T, F>(
        &self,
        operation: &str,
        future: F,
        abort_connection: bool,
    ) -> Result<T, Error>
    where
        T: Send,
        F: std::future::Future<Output = Result<T, Error>> + Send,
    {
        if self.aborted() {
            return Err(super::other(
                "SSH connection was closed after an operation timed out",
            ));
        }
        block_on(async {
            match self.timeout() {
                Some(limit) => match tokio::time::timeout(limit, future).await {
                    Ok(result) => result,
                    Err(_) => {
                        if abort_connection {
                            self.abort();
                        }
                        Err(super::other(format!(
                            "SSH {operation} timed out after {} ms",
                            limit.as_millis()
                        )))
                    }
                },
                None => future.await,
            }
        })
    }
}

#[derive(Default)]
struct State {
    eof: bool,
    closed: bool,
    exit_status: Option<i32>,
    request_replies: VecDeque<bool>,
}

/// Which of the two demultiplexed streams a read is asking for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Stdout,
    Stderr,
}

/// The state both halves of a channel share.
pub(super) struct Shared {
    stdout: Mutex<VecDeque<u8>>,
    stderr: Mutex<VecDeque<u8>>,
    state: Mutex<State>,
    changed: Condvar,
    request_changed: tokio::sync::Notify,
    settings: Arc<Settings>,
}

impl Shared {
    fn new(settings: Arc<Settings>) -> Self {
        Self {
            stdout: Mutex::new(VecDeque::new()),
            stderr: Mutex::new(VecDeque::new()),
            state: Mutex::new(State::default()),
            changed: Condvar::new(),
            request_changed: tokio::sync::Notify::new(),
            settings,
        }
    }

    fn queue(&self, side: Side) -> &Mutex<VecDeque<u8>> {
        match side {
            Side::Stdout => &self.stdout,
            Side::Stderr => &self.stderr,
        }
    }

    /// Push bytes and wake any blocking reader.
    ///
    /// The state lock is taken before notifying so a reader cannot slip between
    /// "queue was empty" and "went to sleep": the reader re-checks under that
    /// same lock.
    fn push(&self, side: Side, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        self.queue(side).lock().unwrap().extend(bytes);
        let _guard = self.state.lock().unwrap();
        self.changed.notify_all();
    }

    fn mark_eof(&self) {
        let mut state = self.state.lock().unwrap();
        state.eof = true;
        self.changed.notify_all();
        drop(state);
    }

    fn mark_exit_status(&self, status: u32) {
        let mut state = self.state.lock().unwrap();
        state.exit_status = Some(status as i32);
        self.changed.notify_all();
        drop(state);
    }

    fn mark_closed(&self) {
        let mut state = self.state.lock().unwrap();
        state.closed = true;
        // SSH permits Close without a preceding EOF. A dropped channel reader
        // also gets here, and in either case no further bytes can arrive.
        state.eof = true;
        self.changed.notify_all();
        self.request_changed.notify_one();
        drop(state);
    }

    fn request_reply(&self, accepted: bool) {
        self.state
            .lock()
            .unwrap()
            .request_replies
            .push_back(accepted);
        // Only one request can be awaiting a reply: request methods take &mut
        // Channel. notify_one retains a permit if the response beats the wait.
        self.request_changed.notify_one();
    }

    async fn wait_request_reply(&self, request: &str) -> Result<(), Error> {
        loop {
            {
                let mut state = self.state.lock().unwrap();
                if let Some(accepted) = state.request_replies.pop_front() {
                    return if accepted {
                        Ok(())
                    } else {
                        Err(super::other(format!(
                            "the SSH server rejected the {request} request"
                        )))
                    };
                }
                if state.closed {
                    return Err(super::other(format!(
                        "SSH channel closed before the server replied to the {request} request"
                    )));
                }
            }
            self.request_changed.notified().await;
        }
    }

    fn take(&self, side: Side, buf: &mut [u8]) -> usize {
        let mut queue = self.queue(side).lock().unwrap();
        let count = queue.len().min(buf.len());
        for slot in buf.iter_mut().take(count) {
            *slot = queue.pop_front().expect("length was just checked");
        }
        count
    }

    /// The incoming stream ended and nothing is left to read on either side.
    fn drained(&self) -> bool {
        let state = self.state.lock().unwrap();
        state.eof
            && self.stdout.lock().unwrap().is_empty()
            && self.stderr.lock().unwrap().is_empty()
    }
}

fn would_block() -> io::Error {
    io::Error::from(io::ErrorKind::WouldBlock)
}

fn timed_out() -> io::Error {
    io::Error::from(io::ErrorKind::TimedOut)
}

fn read_side(shared: &Shared, side: Side, buf: &mut [u8]) -> io::Result<usize> {
    if buf.is_empty() {
        return Ok(0);
    }

    loop {
        let taken = shared.take(side, buf);
        if taken > 0 {
            return Ok(taken);
        }

        let state = shared.state.lock().unwrap();
        // A producer may have queued data after `take` but before this lock.
        // Re-check while holding the notification lock to avoid sleeping after
        // its wakeup has already happened.
        if !shared.queue(side).lock().unwrap().is_empty() {
            drop(state);
            continue;
        }
        if state.eof {
            // This stream is finished. `read` on a finished stream returns 0 even
            // if the *other* stream still has bytes: that is what libssh2 did,
            // and the drain loops rely on it to keep polling the other side.
            return Ok(0);
        }
        if !shared.settings.blocking() {
            return Err(would_block());
        }

        let timeout = shared.settings.timeout();
        let (state, expired) = match timeout {
            Some(limit) => {
                let (guard, result) = shared.changed.wait_timeout(state, limit).unwrap();
                (guard, result.timed_out())
            }
            None => (shared.changed.wait(state).unwrap(), false),
        };
        drop(state);
        if expired {
            return Err(timed_out());
        }
    }
}

/// One SSH channel: a command, a shell, or a forwarded TCP stream.
pub struct Channel {
    pub(super) writer: Arc<ChannelWriteHalf<Msg>>,
    pub(super) shared: Arc<Shared>,
}

impl Read for Channel {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        read_side(&self.shared, Side::Stdout, buf)
    }
}

impl Write for Channel {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let writer = Arc::clone(&self.writer);
        let timeout = self.shared.settings.timeout();
        let sent = block_on(async move {
            match timeout {
                Some(limit) => tokio::time::timeout(limit, writer.data(buf)).await,
                None => Ok(writer.data(buf).await),
            }
        });
        match sent {
            Ok(Ok(())) => Ok(buf.len()),
            Ok(Err(error)) => Err(io::Error::other(error.to_string())),
            Err(_) => Err(timed_out()),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        // russh has no buffered writer here; every `data` call is already a
        // completed send.
        Ok(())
    }
}

impl Channel {
    fn request<F>(&mut self, request: &str, send: F) -> Result<(), Error>
    where
        F: std::future::Future<Output = Result<(), russh::Error>> + Send,
    {
        self.shared
            .settings
            .run(&format!("{request} request"), async {
                send.await.map_err(Error::from)?;
                self.shared.wait_request_reply(request).await
            })
    }

    /// A cursor over the channel's stderr.
    ///
    /// Takes `&mut self` even though this backend only needs `&self`, because a
    /// cursor that mutates a shared queue is what both backends can implement.
    pub fn stderr(&mut self) -> Stderr<'_> {
        Stderr {
            shared: &self.shared,
        }
    }

    pub fn eof(&self) -> bool {
        self.shared.drained()
    }

    pub fn exit_status(&self) -> Result<i32, Error> {
        self.shared
            .state
            .lock()
            .unwrap()
            .exit_status
            .ok_or_else(|| {
                Error::new(
                    super::super::types::ErrorKind::Other,
                    "no exit status received",
                )
            })
    }

    /// Wait for the server to close the channel.
    pub fn wait_close(&mut self) -> Result<(), Error> {
        let mut state = self.shared.state.lock().unwrap();
        while !state.closed {
            let waited = match self.shared.settings.timeout() {
                Some(limit) => {
                    let (guard, result) = self.shared.changed.wait_timeout(state, limit).unwrap();
                    state = guard;
                    result.timed_out()
                }
                None => {
                    state = self.shared.changed.wait(state).unwrap();
                    false
                }
            };
            if waited && !state.closed {
                return Err(Error::new(
                    super::super::types::ErrorKind::Other,
                    "timed out waiting for the channel to close",
                ));
            }
        }
        Ok(())
    }

    pub fn close(&mut self) -> Result<(), Error> {
        let writer = Arc::clone(&self.writer);
        block_on(async move { writer.close().await }).map_err(Error::from)
    }

    pub fn send_eof(&mut self) -> Result<(), Error> {
        let writer = Arc::clone(&self.writer);
        block_on(async move { writer.eof().await }).map_err(Error::from)
    }

    pub fn exec(&mut self, command: &str) -> Result<(), Error> {
        let writer = Arc::clone(&self.writer);
        let command = command.to_string();
        self.request("exec", async move { writer.exec(true, command).await })
    }

    pub fn shell(&mut self) -> Result<(), Error> {
        let writer = Arc::clone(&self.writer);
        self.request("shell", async move { writer.request_shell(true).await })
    }

    /// Request a PTY.
    ///
    /// `mode` is refused rather than ignored: no caller in this crate sets
    /// terminal mode strings, and silently dropping one would look like success.
    pub fn request_pty(
        &mut self,
        term: &str,
        mode: Option<&str>,
        dim: Option<(u32, u32, u32, u32)>,
    ) -> Result<(), Error> {
        if mode.is_some() {
            return Err(Error::new(
                super::super::types::ErrorKind::Other,
                "PTY mode strings are not supported by this transport",
            ));
        }
        let (cols, rows, width_px, height_px) = dim.unwrap_or((80, 24, 0, 0));
        let writer = Arc::clone(&self.writer);
        let term = term.to_string();
        self.request("PTY", async move {
            writer
                .request_pty(true, &term, cols, rows, width_px, height_px, &[])
                .await
        })
    }

    /// Resize the PTY. This is the only "write" that is not `data`.
    pub fn request_pty_size(
        &mut self,
        width: u32,
        height: u32,
        width_px: Option<u32>,
        height_px: Option<u32>,
    ) -> Result<(), Error> {
        let writer = Arc::clone(&self.writer);
        block_on(async move {
            writer
                .window_change(width, height, width_px.unwrap_or(0), height_px.unwrap_or(0))
                .await
        })
        .map_err(Error::from)
    }
}

/// The stderr half of a [`Channel`].
pub struct Stderr<'a> {
    shared: &'a Arc<Shared>,
}

impl Read for Stderr<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        read_side(self.shared, Side::Stderr, buf)
    }
}

/// Open a channel and start the task that fills its two queues.
pub(super) fn open(channel: russh::Channel<Msg>, settings: Arc<Settings>) -> Channel {
    let shared = Arc::new(Shared::new(settings));
    let (mut reader, writer) = channel.split();

    let task_shared = Arc::clone(&shared);
    super::runtime().spawn(async move {
        while let Some(message) = reader.wait().await {
            match message {
                ChannelMsg::Data { data } => task_shared.push(Side::Stdout, &data),
                // Extended data stream 1 is stderr; anything else is a stream
                // this crate never asked for.
                ChannelMsg::ExtendedData { data, ext: 1 } => task_shared.push(Side::Stderr, &data),
                ChannelMsg::Eof => task_shared.mark_eof(),
                ChannelMsg::ExitStatus { exit_status } => task_shared.mark_exit_status(exit_status),
                ChannelMsg::Success => task_shared.request_reply(true),
                ChannelMsg::Failure => task_shared.request_reply(false),
                ChannelMsg::Close => {
                    task_shared.mark_closed();
                    break;
                }
                _ => {}
            }
        }
        // `wait` returning `None` means the channel is gone, which is a close
        // even if the message never arrived.
        task_shared.mark_closed();
    });

    Channel {
        writer: Arc::new(writer),
        shared,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_without_eof_drains_both_streams_before_reporting_eof() {
        let settings = Arc::new(Settings::new());
        settings.set_blocking(false);
        let shared = Shared::new(settings);
        shared.push(Side::Stdout, b"output");
        shared.push(Side::Stderr, b"warning");
        shared.mark_exit_status(17);
        shared.mark_closed();

        let mut buf = [0; 16];
        assert!(!shared.drained());
        assert_eq!(read_side(&shared, Side::Stdout, &mut buf).unwrap(), 6);
        assert_eq!(&buf[..6], b"output");
        assert_eq!(read_side(&shared, Side::Stdout, &mut buf).unwrap(), 0);
        assert!(!shared.drained(), "stderr must survive channel close");
        assert_eq!(read_side(&shared, Side::Stderr, &mut buf).unwrap(), 7);
        assert_eq!(&buf[..7], b"warning");
        assert_eq!(read_side(&shared, Side::Stderr, &mut buf).unwrap(), 0);
        assert!(shared.drained());
        assert_eq!(shared.state.lock().unwrap().exit_status, Some(17));
    }

    #[test]
    fn close_without_eof_finishes_empty_nonblocking_reads() {
        let settings = Arc::new(Settings::new());
        settings.set_blocking(false);
        let shared = Shared::new(settings);
        let mut buf = [0; 1];
        for side in [Side::Stdout, Side::Stderr] {
            assert_eq!(
                read_side(&shared, side, &mut buf).unwrap_err().kind(),
                io::ErrorKind::WouldBlock
            );
        }
        shared.mark_closed();
        for side in [Side::Stdout, Side::Stderr] {
            assert_eq!(read_side(&shared, side, &mut buf).unwrap(), 0);
        }
        assert!(shared.drained());
    }

    #[test]
    fn close_without_eof_wakes_blocking_readers_on_both_streams() {
        let settings = Arc::new(Settings::new());
        // A regression must fail instead of leaving a test thread hung forever.
        settings.set_timeout(2_000);
        let shared = Arc::new(Shared::new(settings));
        let readers: Vec<_> = [Side::Stdout, Side::Stderr]
            .into_iter()
            .map(|side| {
                let shared = Arc::clone(&shared);
                std::thread::spawn(move || read_side(&shared, side, &mut [0; 1]))
            })
            .collect();
        shared.mark_closed();
        for reader in readers {
            assert_eq!(reader.join().unwrap().unwrap(), 0);
        }
        assert!(shared.drained());
    }

    #[test]
    fn eof_alone_still_requires_both_queues_to_drain() {
        let shared = Shared::new(Arc::new(Settings::new()));
        shared.push(Side::Stderr, b"last bytes");
        shared.mark_eof();
        assert!(!shared.drained());
        assert!(!shared.state.lock().unwrap().closed);
        assert_eq!(read_side(&shared, Side::Stdout, &mut [0; 16]).unwrap(), 0);
        assert_eq!(read_side(&shared, Side::Stderr, &mut [0; 16]).unwrap(), 10);
        assert!(shared.drained());
    }

    #[tokio::test]
    async fn channel_reader_finishes_on_close_and_connection_disappearance() {
        struct Client;
        impl russh::client::Handler for Client {
            type Error = russh::Error;

            async fn check_server_key(
                &mut self,
                _key: &russh::keys::PublicKeyOrCertificate,
            ) -> Result<bool, Self::Error> {
                Ok(true)
            }
        }

        struct Server;
        impl russh::server::Handler for Server {
            type Error = russh::Error;

            async fn auth_none(&mut self, _user: &str) -> Result<russh::server::Auth, Self::Error> {
                Ok(russh::server::Auth::Accept)
            }

            async fn channel_open_session(
                &mut self,
                _channel: russh::Channel<russh::server::Msg>,
                reply: russh::server::ChannelOpenHandle,
                _session: &mut russh::server::Session,
            ) -> Result<(), Self::Error> {
                reply.accept().await;
                Ok(())
            }

            async fn exec_request(
                &mut self,
                channel: russh::ChannelId,
                command: &[u8],
                session: &mut russh::server::Session,
            ) -> Result<(), Self::Error> {
                if command == b"close" {
                    session.data(channel, b"stdout before close".as_slice())?;
                    session.extended_data(channel, 1, b"stderr before close".as_slice())?;
                    // Deliberately omit session.eof(channel).
                    session.close(channel)?;
                } else {
                    // Drop the entire connection without a channel EOF or
                    // Close message, so the reader's wait() returns None.
                    session.disconnect(russh::Disconnect::ByApplication, "test shutdown", "")?;
                }
                Ok(())
            }
        }

        tokio::time::timeout(Duration::from_secs(10), async {
            for command in ["close", "disconnect"] {
                let (client_stream, server_stream) = tokio::io::duplex(64 * 1024);
                let config = russh::server::Config {
                    keys: vec![
                        russh::keys::ssh_key::private::Ed25519Keypair::from_seed(&[5; 32]).into(),
                    ],
                    ..Default::default()
                };
                let server_task = tokio::spawn(async move {
                    russh::server::run_stream(Arc::new(config), server_stream, Server)
                        .await
                        .unwrap()
                        .await
                });
                let mut handle = russh::client::connect_stream(
                    Arc::new(russh::client::Config::default()),
                    client_stream,
                    Client,
                )
                .await
                .unwrap();
                assert!(handle
                    .authenticate_none("testuser")
                    .await
                    .unwrap()
                    .success());
                let raw_channel = handle.channel_open_session().await.unwrap();
                let settings = Arc::new(Settings::new());
                settings.set_blocking(false);
                let mut channel = open(raw_channel, settings);
                channel.writer.exec(false, command).await.unwrap();
                while !channel.shared.state.lock().unwrap().closed {
                    tokio::task::yield_now().await;
                }

                let mut stdout = Vec::new();
                let mut stderr = Vec::new();
                channel.read_to_end(&mut stdout).unwrap();
                channel.stderr().read_to_end(&mut stderr).unwrap();
                if command == "close" {
                    assert_eq!(stdout, b"stdout before close");
                    assert_eq!(stderr, b"stderr before close");
                    handle
                        .disconnect(russh::Disconnect::ByApplication, "test complete", "")
                        .await
                        .unwrap();
                } else {
                    assert!(stdout.is_empty());
                    assert!(stderr.is_empty());
                }
                assert!(channel.eof());
                channel.wait_close().unwrap();
                let _ = server_task.await.unwrap();
            }
        })
        .await
        .expect("close and reader disappearance must not leave drain loops running");
    }
}
