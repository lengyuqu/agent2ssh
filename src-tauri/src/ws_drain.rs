//! T2-15: WebSocket request draining for graceful shutdown.
//!
//! When the daemon receives a shutdown signal (Ctrl-C / SIGTERM), active
//! WebSocket connections need to drain their pending messages before closing.
//! This module provides:
//!
//! - `ShutdownToken`: A clone-able cancellation token that propagates shutdown
//!   to active WebSocket and event-stream handlers.
//! - `DrainHandle`: Per-connection handle that tracks pending messages and
//!   signals when draining is complete.
//!
//! The flow:
//! 1. `shutdown_signal()` fires → `ShutdownToken::cancel()`
//! 2. Each WS handler's `select!` loop sees the cancellation
//! 3. Handler enters drain mode: stops accepting new input, flushes pending output
//! 4. Handler sends `Close` frame and exits cleanly
//! 5. No RST frames — the TCP connection closes with a proper WS close handshake

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::{Mutex, Notify};
use uuid::Uuid;

/// T2-15: Shutdown token for propagating graceful shutdown to stream handlers.
///
/// Cloning the token creates a new handle to the same cancellation source.
/// When `cancel()` is called, all clones see the cancellation.
#[derive(Clone)]
pub struct ShutdownToken {
    inner: Arc<ShutdownInner>,
}

struct ShutdownInner {
    cancelled: Mutex<bool>,
    cancelled_now: AtomicBool,
    notify: Notify,
}

impl ShutdownToken {
    /// Create a new, un-cancelled token.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(ShutdownInner {
                cancelled: Mutex::new(false),
                cancelled_now: AtomicBool::new(false),
                notify: Notify::new(),
            }),
        }
    }

    /// Signal cancellation to all holders of this token.
    pub async fn cancel(&self) {
        self.inner.cancelled_now.store(true, Ordering::Release);
        {
            let mut guard = self.inner.cancelled.lock().await;
            *guard = true;
        }
        self.inner.notify.notify_waiters();
    }

    /// Synchronous view for blocking workers which cooperatively stop children.
    pub fn is_cancelled_now(&self) -> bool {
        self.inner.cancelled_now.load(Ordering::Acquire)
    }

    /// Returns `true` if cancellation has been signalled.
    pub async fn is_cancelled(&self) -> bool {
        *self.inner.cancelled.lock().await
    }

    /// Wait until cancellation is signalled. Returns immediately if already cancelled.
    pub async fn cancelled(&self) {
        if self.is_cancelled().await {
            return;
        }
        loop {
            let notified = self.inner.notify.notified();
            tokio::pin!(notified);
            // notify_waiters does not retain a permit: register before checking
            // the flag so cancellation cannot be lost between check and await.
            notified.as_mut().enable();
            if self.is_cancelled().await {
                return;
            }
            notified.await;
            if self.is_cancelled().await {
                return;
            }
        }
    }
}

impl Default for ShutdownToken {
    fn default() -> Self {
        Self::new()
    }
}

/// T2-15: Per-connection drain tracker.
///
/// Tracks the number of pending messages for a single WS connection.
/// When draining is requested, the handler stops accepting new input
/// and waits for `pending_count` to reach zero before closing.
pub struct DrainHandle {
    id: Uuid,
    pending: Arc<Mutex<usize>>,
    drain_complete: Arc<Notify>,
    registry: Arc<Mutex<HashMap<Uuid, DrainEntry>>>,
    registry_changed: Arc<Notify>,
}

struct DrainEntry {
    pending: Arc<Mutex<usize>>,
    drain_complete: Arc<Notify>,
}

/// T2-15: Registry of all active WS connections for coordinated draining.
#[derive(Clone)]
pub struct DrainRegistry {
    connections: Arc<Mutex<HashMap<Uuid, DrainEntry>>>,
    registry_changed: Arc<Notify>,
}

impl DrainRegistry {
    pub fn new() -> Self {
        Self {
            connections: Arc::new(Mutex::new(HashMap::new())),
            registry_changed: Arc::new(Notify::new()),
        }
    }

    /// Register a new connection. Returns a `DrainHandle` for tracking pending messages.
    pub async fn register(&self) -> DrainHandle {
        let id = Uuid::new_v4();
        let pending = Arc::new(Mutex::new(0usize));
        let drain_complete = Arc::new(Notify::new());
        let entry = DrainEntry {
            pending: pending.clone(),
            drain_complete: drain_complete.clone(),
        };
        self.connections.lock().await.insert(id, entry);
        DrainHandle {
            id,
            pending,
            drain_complete,
            registry: self.connections.clone(),
            registry_changed: self.registry_changed.clone(),
        }
    }

    /// Wait for the registered pending sends with one shared deadline.
    /// Returns the number of connections observed, including timed-out ones.
    pub async fn drain_all(&self, timeout: std::time::Duration) -> usize {
        let entries: Vec<(Uuid, Arc<Notify>, Arc<Mutex<usize>>)> = {
            let conns = self.connections.lock().await;
            conns
                .iter()
                .map(|(id, e)| (*id, e.drain_complete.clone(), e.pending.clone()))
                .collect()
        };
        let count = entries.len();
        if count == 0 {
            return 0;
        }

        let deadline = tokio::time::Instant::now() + timeout;
        for (_, notify, pending) in &entries {
            loop {
                let notified = notify.notified();
                tokio::pin!(notified);
                // Register before testing pending: notify_waiters does not
                // retain a permit when completion races the check.
                notified.as_mut().enable();
                if *pending.lock().await == 0 {
                    break;
                }
                if tokio::time::timeout_at(deadline, notified).await.is_err() {
                    return count;
                }
            }
        }
        count
    }

    /// Get the number of active connections.
    pub async fn active_count(&self) -> usize {
        self.connections.lock().await.len()
    }

    /// Wait for WebSocket handlers to finish cleanup, with a single deadline.
    pub async fn wait_until_empty(&self, timeout: std::time::Duration) -> bool {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let notified = self.registry_changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.connections.lock().await.is_empty() {
                return true;
            }
            if tokio::time::timeout_at(deadline, notified).await.is_err() {
                return false;
            }
        }
    }
}

impl Default for DrainRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl DrainHandle {
    /// Increment the pending message counter.
    pub async fn begin_send(&self) {
        let mut guard = self.pending.lock().await;
        *guard += 1;
    }

    /// Decrement the pending message counter. Notifies the drain waiter
    /// when pending reaches zero.
    pub async fn end_send(&self) {
        let mut guard = self.pending.lock().await;
        if *guard > 0 {
            *guard -= 1;
        }
        if *guard == 0 {
            self.drain_complete.notify_waiters();
        }
    }

    /// Remove this connection from the registry (on clean disconnect).
    pub async fn unregister(self) {
        self.registry.lock().await.remove(&self.id);
        self.registry_changed.notify_waiters();
    }

    /// Get the current pending count (for testing).
    pub async fn pending_count(&self) -> usize {
        *self.pending.lock().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn shutdown_token_propagates_cancellation() {
        let token = ShutdownToken::new();
        let clone1 = token.clone();
        let clone2 = token.clone();

        assert!(!clone1.is_cancelled().await);

        token.cancel().await;

        assert!(clone1.is_cancelled().await);
        assert!(clone2.is_cancelled().await);
    }

    #[tokio::test]
    async fn shutdown_token_cancelled_returns_immediately() {
        let token = ShutdownToken::new();
        token.cancel().await;

        // Should return immediately, not hang
        let result = tokio::time::timeout(Duration::from_millis(100), token.cancelled()).await;
        assert!(result.is_ok());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_waiters_observe_cancellation() {
        for _ in 0..100 {
            let token = ShutdownToken::new();
            let mut waiters = Vec::new();
            for _ in 0..8 {
                let clone = token.clone();
                waiters.push(tokio::spawn(async move { clone.cancelled().await }));
            }
            tokio::task::yield_now().await;
            token.cancel().await;
            for waiter in waiters {
                tokio::time::timeout(Duration::from_secs(1), waiter)
                    .await
                    .expect("waiter missed shutdown")
                    .expect("waiter task failed");
            }
        }
    }

    #[tokio::test]
    async fn drain_registry_tracks_connections() {
        let registry = DrainRegistry::new();
        assert_eq!(registry.active_count().await, 0);

        let handle1 = registry.register().await;
        let handle2 = registry.register().await;
        assert_eq!(registry.active_count().await, 2);

        handle1.unregister().await;
        assert_eq!(registry.active_count().await, 1);

        handle2.unregister().await;
        assert_eq!(registry.active_count().await, 0);
    }

    #[tokio::test]
    async fn drain_handle_pending_counter() {
        let registry = DrainRegistry::new();
        let handle = registry.register().await;

        assert_eq!(handle.pending_count().await, 0);
        handle.begin_send().await;
        handle.begin_send().await;
        assert_eq!(handle.pending_count().await, 2);
        handle.end_send().await;
        assert_eq!(handle.pending_count().await, 1);
        handle.end_send().await;
        assert_eq!(handle.pending_count().await, 0);
    }

    #[tokio::test]
    async fn drain_all_returns_immediately_when_no_pending() {
        let registry = DrainRegistry::new();
        let _handle = registry.register().await;

        // No pending messages — drain should return quickly
        let count = registry.drain_all(Duration::from_secs(1)).await;
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn drain_all_waits_for_pending_to_complete() {
        let registry = DrainRegistry::new();
        let handle = registry.register().await;

        // Simulate a pending message
        handle.begin_send().await;

        // Start drain in background — it should wait
        let registry_clone = DrainRegistry {
            connections: registry.connections.clone(),
            registry_changed: registry.registry_changed.clone(),
        };
        let drain_task =
            tokio::spawn(async move { registry_clone.drain_all(Duration::from_secs(2)).await });

        // Give drain task time to start waiting
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Complete the pending message
        handle.end_send().await;

        // Drain should complete
        let result = tokio::time::timeout(Duration::from_secs(1), drain_task).await;
        assert!(result.is_ok(), "drain_all should complete after end_send");
    }

    #[tokio::test]
    async fn drain_all_times_out() {
        let registry = DrainRegistry::new();
        let handle = registry.register().await;

        // Simulate a stuck pending message
        handle.begin_send().await;

        // Drain with a very short timeout — should time out
        let start = std::time::Instant::now();
        let _count = registry.drain_all(Duration::from_millis(100)).await;
        let elapsed = start.elapsed();

        // Should have waited at least ~100ms (the timeout)
        assert!(
            elapsed >= Duration::from_millis(80),
            "drain_all should wait for timeout, elapsed: {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn drain_timeout_is_shared_across_connections() {
        let registry = DrainRegistry::new();
        let first = registry.register().await;
        let second = registry.register().await;
        let third = registry.register().await;
        first.begin_send().await;
        second.begin_send().await;
        third.begin_send().await;
        let result = tokio::time::timeout(
            Duration::from_millis(500),
            registry.drain_all(Duration::from_millis(200)),
        )
        .await;
        assert_eq!(result.expect("drain exceeded one shared deadline"), 3);
    }

    #[tokio::test]
    async fn waits_for_websocket_handler_cleanup_or_deadline() {
        let registry = DrainRegistry::new();
        let handle = registry.register().await;
        assert!(!registry.wait_until_empty(Duration::from_millis(30)).await);
        let other = registry.clone();
        let waiter =
            tokio::spawn(async move { other.wait_until_empty(Duration::from_secs(1)).await });
        tokio::task::yield_now().await;
        handle.unregister().await;
        assert!(waiter.await.unwrap());
    }
}
