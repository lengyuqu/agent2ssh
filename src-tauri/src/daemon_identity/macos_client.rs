//! Read-only, fail-closed macOS IPC discovery. A consistent hello is never a
//! process-object binding and this module deliberately has no token/stop API.
use super::{check_hello_reply, macos_peer_pid, write_hello, HelloFrame, MAX_FRAME, VERSION};
use anyhow::{bail, Context, Result};
use std::{
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{ffi::OsStrExt, net::UnixStream},
    },
    path::Path,
    time::{Duration, Instant},
};

#[derive(Debug)]
pub struct UnboundObservation {
    pub peer_pid: u32,
    snapshot: super::macos::DiscoverySnapshot,
}

impl UnboundObservation {
    /// Detect a replacement since hello. This is still not a bound process
    /// object and does not eliminate races after the check.
    pub fn revalidate(&self) -> Result<()> {
        super::macos::verify_discovery_snapshot(&self.snapshot)
    }
}

fn wait_fd(fd: libc::c_int, events: libc::c_short, deadline: Instant) -> Result<()> {
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .context("IPC deadline expired")?;
        let ms = remaining.as_millis().min(i32::MAX as u128).max(1) as i32;
        let mut pollfd = libc::pollfd {
            fd,
            events,
            revents: 0,
        };
        let n = unsafe { libc::poll(&mut pollfd, 1, ms) };
        if n > 0 {
            return Ok(());
        }
        if n == 0 {
            bail!("IPC deadline expired");
        }
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::Interrupted {
            return Err(error.into());
        }
    }
}

fn connect(path: &Path, deadline: Instant) -> Result<UnixStream> {
    let bytes = path.as_os_str().as_bytes();
    let mut addr: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    if bytes.len() >= addr.sun_path.len() {
        bail!("IPC socket path too long");
    }
    addr.sun_len = std::mem::size_of_val(&addr) as u8;
    addr.sun_family = libc::AF_UNIX as libc::sa_family_t;
    for (slot, byte) in addr.sun_path.iter_mut().zip(bytes) {
        *slot = *byte as libc::c_char;
    }
    let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: fd is a new, uniquely owned socket; UnixStream closes it on all
    // return paths. CLOEXEC prevents accidental exec inheritance.
    let stream = unsafe { UnixStream::from_raw_fd(fd) };
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) } < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    stream.set_nonblocking(true)?;
    let rc = unsafe {
        libc::connect(
            fd,
            (&addr as *const libc::sockaddr_un).cast(),
            std::mem::size_of_val(&addr) as libc::socklen_t,
        )
    };
    if rc != 0 {
        let e = std::io::Error::last_os_error();
        if !matches!(
            e.raw_os_error(),
            Some(libc::EINPROGRESS) | Some(libc::EAGAIN)
        ) {
            return Err(e).context("IPC connect failed");
        }
        wait_fd(fd, libc::POLLOUT, deadline)?;
        if let Some(e) = stream.take_error()? {
            return Err(e).context("IPC connect failed");
        }
    }
    Ok(stream)
}

fn write_until(stream: &mut UnixStream, mut data: &[u8], deadline: Instant) -> Result<()> {
    while !data.is_empty() {
        if Instant::now() >= deadline {
            bail!("IPC deadline expired");
        }
        match stream.write(data) {
            Ok(0) => bail!("IPC closed during hello"),
            Ok(n) => data = &data[n..],
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                wait_fd(stream.as_raw_fd(), libc::POLLOUT, deadline)?
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}
fn read_until(stream: &mut UnixStream, mut data: &mut [u8], deadline: Instant) -> Result<()> {
    while !data.is_empty() {
        if Instant::now() >= deadline {
            bail!("IPC deadline expired");
        }
        match stream.read(data) {
            Ok(0) => bail!("IPC closed during hello"),
            Ok(n) => {
                let (_, rest) = data.split_at_mut(n);
                data = rest;
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                wait_fd(stream.as_raw_fd(), libc::POLLIN, deadline)?
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

/// Discover and compare evidence on one connection. Returning `Ok` still means
/// Unbound; no secret is read, sent, or authorized and no stop is attempted.
pub fn inspect_unbound(config: &Path, expected_pid: u32) -> Result<UnboundObservation> {
    let snapshot = super::macos::discovery_snapshot(config, expected_pid)?;
    let deadline = Instant::now() + Duration::from_millis(1500);
    let mut stream = connect(&snapshot.socket_path, deadline)?;
    let before = macos_peer_pid(&stream).context("cannot read IPC peer PID")?;
    if before != expected_pid {
        bail!("IPC peer PID conflicts with record");
    }
    let mut nonce_bytes = [0u8; 16];
    getrandom::fill(&mut nonce_bytes)
        .map_err(|e| anyhow::anyhow!("cannot create hello nonce: {e}"))?;
    let nonce = hex::encode(nonce_bytes);
    let mut hello = Vec::new();
    write_hello(
        &mut hello,
        &HelloFrame::Hello {
            version: VERSION,
            client_nonce: nonce.clone(),
        },
    )?;
    write_until(&mut stream, &hello, deadline)?;
    let mut header = [0u8; 4];
    read_until(&mut stream, &mut header, deadline)?;
    let size = u32::from_be_bytes(header) as usize;
    if size == 0 || size > MAX_FRAME {
        bail!("invalid IPC hello length");
    }
    let mut body = vec![0; size];
    read_until(&mut stream, &mut body, deadline)?;
    let reply: HelloFrame = serde_json::from_slice(&body).context("invalid IPC hello")?;
    let after = macos_peer_pid(&stream).context("cannot recheck IPC peer PID")?;
    if before != after {
        bail!("IPC peer changed during hello");
    }
    check_hello_reply(&reply, &nonce, &snapshot.instance_id, expected_pid, after)?;
    super::macos::verify_discovery_snapshot(&snapshot)?;
    Ok(UnboundObservation {
        peer_pid: after,
        snapshot,
    })
}
