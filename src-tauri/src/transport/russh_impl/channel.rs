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
//! - `eof()` means "EOF arrived **and** both queues are drained". Returning true
//!   on the EOF message alone truncates stderr.
//! - the blocking flag and the timeout are separate knobs. Non-blocking means
//!   "return `WouldBlock` now"; a timeout bounds how long a *blocking* call may
//!   wait. Collapsing them breaks the exec teardown path, which switches to
//!   blocking and then calls `wait_close`.

use std::collections::VecDeque;
use std::io::{self, Read, Write};
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
}

impl Settings {
    pub(super) fn new() -> Self {
        Self {
            blocking: AtomicBool::new(true),
            timeout_ms: AtomicU64::new(0),
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
}

#[derive(Default)]
struct State {
    eof: bool,
    closed: bool,
    exit_status: Option<i32>,
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
    settings: Arc<Settings>,
}

impl Shared {
    fn new(settings: Arc<Settings>) -> Self {
        Self {
            stdout: Mutex::new(VecDeque::new()),
            stderr: Mutex::new(VecDeque::new()),
            state: Mutex::new(State::default()),
            changed: Condvar::new(),
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
        self.changed.notify_all();
        drop(state);
    }

    fn take(&self, side: Side, buf: &mut [u8]) -> usize {
        let mut queue = self.queue(side).lock().unwrap();
        let count = queue.len().min(buf.len());
        for slot in buf.iter_mut().take(count) {
            *slot = queue.pop_front().expect("length was just checked");
        }
        count
    }

    /// EOF arrived and nothing is left to read on either stream.
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
        if state.eof {
            // This stream is finished. `read` on a finished stream returns 0 even
            // if the *other* stream still has bytes: that is what libssh2 did,
            // and the drain loops rely on it to keep polling the other side.
            let empty = shared.queue(side).lock().unwrap().is_empty();
            if empty {
                return Ok(0);
            }
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
        block_on(async move { writer.exec(true, command).await }).map_err(Error::from)
    }

    pub fn shell(&mut self) -> Result<(), Error> {
        let writer = Arc::clone(&self.writer);
        block_on(async move { writer.request_shell(true).await }).map_err(Error::from)
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
        block_on(async move {
            writer
                .request_pty(true, &term, cols, rows, width_px, height_px, &[])
                .await
        })
        .map_err(Error::from)
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
