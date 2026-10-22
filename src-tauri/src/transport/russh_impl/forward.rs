//! Remote forwarding for the russh backend.
//!
//! The two backends differ in shape here, not just in names. libssh2 hands out a
//! listener you *pull* from: `accept()` returns the next inbound connection.
//! russh *pushes*: the server opens a channel and it arrives at
//! `Handler::server_channel_open_forwarded_tcpip`, so the only way to turn that
//! into `accept()` is a registry between the two.
//!
//! That registry is this module. The handler deposits channels keyed by the port
//! the server says they arrived on; `accept()` pops one, honouring the session's
//! blocking flag and timeout the same way a channel read does. `forward.rs`
//! depends on that: it runs with blocking off and treats `WouldBlock` as "sleep
//! 50 ms and look again", which is also how it notices its stop flag.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Condvar, Mutex};

use russh::client::Msg;

use super::channel::{self, Settings};
use super::Channel;
use crate::transport::types::{Error, ErrorKind};

/// Channels that arrived for a forward, waiting to be accepted.
pub(super) struct Registry {
    /// Keyed by **port only**. The server reports the address it bound, which is
    /// not necessarily the string we asked for (`0.0.0.0` vs `localhost`), so
    /// matching on the address would be brittle; ports are what callers pick.
    queues: Mutex<HashMap<u32, VecDeque<russh::Channel<Msg>>>>,
    changed: Condvar,
    settings: Arc<Settings>,
}

impl Registry {
    pub(super) fn new(settings: Arc<Settings>) -> Self {
        Self {
            queues: Mutex::new(HashMap::new()),
            changed: Condvar::new(),
            settings,
        }
    }

    fn settings(&self) -> &Arc<Settings> {
        &self.settings
    }

    /// Called from the handler. Notifying under the lock is what keeps a waiter
    /// from missing an arrival between its emptiness check and its wait.
    pub(super) fn push(&self, port: u32, channel: russh::Channel<Msg>) {
        let mut queues = self.queues.lock().unwrap();
        queues.entry(port).or_default().push_back(channel);
        self.changed.notify_all();
    }

    fn take(&self, port: u32) -> Result<russh::Channel<Msg>, Error> {
        let mut queues = self.queues.lock().unwrap();
        loop {
            if let Some(channel) = queues
                .get_mut(&port)
                .and_then(std::collections::VecDeque::pop_front)
            {
                return Ok(channel);
            }

            if !self.settings.blocking() {
                return Err(Error::new(
                    ErrorKind::WouldBlock,
                    "no forwarded connection is pending",
                ));
            }

            let timeout = self.settings.timeout();
            let (next, expired) = match timeout {
                Some(limit) => {
                    let (guard, result) = self.changed.wait_timeout(queues, limit).unwrap();
                    (guard, result.timed_out())
                }
                None => (self.changed.wait(queues).unwrap(), false),
            };
            queues = next;

            if expired {
                // A connection may have arrived at the deadline; the loop's
                // next pass checks before reporting the timeout.
                let empty = queues.get(&port).is_none_or(VecDeque::is_empty);
                if empty {
                    return Err(Error::new(
                        ErrorKind::Other,
                        "timed out waiting for a forwarded connection",
                    ));
                }
            }
        }
    }
}

/// An accepted remote forward: the server-side listener plus the queue that
/// inbound connections land in.
pub struct Listener {
    port: u32,
    registry: Arc<Registry>,
}

impl Listener {
    pub(super) fn new(port: u32, registry: Arc<Registry>) -> Self {
        Self { port, registry }
    }

    pub fn accept(&mut self) -> Result<Channel, Error> {
        let channel = self.registry.take(self.port)?;
        Ok(channel::open(channel, Arc::clone(self.registry.settings())))
    }
}
