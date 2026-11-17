//! A TCP proxy in front of an sshd that counts the connections it accepts.
//!
//! "No new connection was opened" is a claim about the network, so tests that
//! make it point a host profile at this proxy and read the count, instead of
//! inferring reuse from timing. The same proxy can reset or silence the
//! connections it already carries, which is how a dead connection is produced
//! on demand: a reset is a peer that crashed, silence is a network that went
//! away without telling anyone.

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// The proxy. Dropping it stops accepting and resets what it carries.
pub struct Proxy {
    pub port: u16,
    accepted: Arc<AtomicUsize>,
    /// Connections numbered at or below this stop forwarding, silently.
    silenced_through: Arc<AtomicUsize>,
    streams: Arc<Mutex<Vec<TcpStream>>>,
    stop: Arc<AtomicBool>,
}

impl Proxy {
    pub fn start(upstream_host: &str, upstream_port: u16) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind the proxy");
        let port = listener.local_addr().unwrap().port();
        let accepted = Arc::new(AtomicUsize::new(0));
        let silenced_through = Arc::new(AtomicUsize::new(0));
        let streams = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let upstream = format!("{upstream_host}:{upstream_port}");
        {
            let accepted = Arc::clone(&accepted);
            let silenced = Arc::clone(&silenced_through);
            let streams = Arc::clone(&streams);
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                for client in listener.incoming() {
                    if stop.load(Ordering::SeqCst) {
                        return;
                    }
                    let Ok(client) = client else { continue };
                    let id = accepted.fetch_add(1, Ordering::SeqCst) + 1;
                    let Ok(server) = TcpStream::connect(&upstream) else {
                        continue;
                    };
                    {
                        let mut kept = streams.lock().unwrap();
                        kept.push(client.try_clone().unwrap());
                        kept.push(server.try_clone().unwrap());
                    }
                    for (from, to) in [
                        (client.try_clone().unwrap(), server.try_clone().unwrap()),
                        (server, client),
                    ] {
                        let silenced = Arc::clone(&silenced);
                        std::thread::spawn(move || pump(from, to, id, &silenced));
                    }
                }
            });
        }
        Self {
            port,
            accepted,
            silenced_through,
            streams,
            stop,
        }
    }

    pub fn accepted(&self) -> usize {
        self.accepted.load(Ordering::SeqCst)
    }

    /// Reset every live connection, as a peer that crashed would.
    pub fn sever_all(&self) {
        for stream in self.streams.lock().unwrap().iter() {
            let _ = stream.shutdown(Shutdown::Both);
        }
    }

    /// Keep every connection open but stop carrying anything on it — a NAT
    /// mapping that expired, a network that went away without a reset. New
    /// connections are unaffected.
    pub fn silence_existing(&self) {
        self.silenced_through
            .store(self.accepted(), Ordering::SeqCst);
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wake the accept loop so its thread ends.
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        self.sever_all();
    }
}

fn pump(mut from: TcpStream, mut to: TcpStream, id: usize, silenced_through: &AtomicUsize) {
    let mut buf = [0u8; 16 * 1024];
    loop {
        match from.read(&mut buf) {
            Ok(0) | Err(_) => {
                let _ = to.shutdown(Shutdown::Both);
                return;
            }
            Ok(n) => {
                if silenced_through.load(Ordering::SeqCst) >= id {
                    continue;
                }
                if to.write_all(&buf[..n]).is_err() {
                    return;
                }
            }
        }
    }
}
