//! Centralized application state (P2 #5, #6, #9).
//!
//! ## Why this module exists
//!
//! Before this module, process-wide state was scattered across ~13
//! `OnceLock` statics in 11 different files (`store.rs`, `forward.rs`,
//! `secrets.rs`, `events.rs`, `connection.rs`, `session.rs`, `approval.rs`,
//! `risk_config.rs`, `diagnostics.rs`, `anomaly.rs`, `sftp_transfer.rs`,
//! `tauri_commands.rs`).
//!
//! Problems with the scattered approach:
//! - No single place to audit "what global state does the process hold?"
//! - No lifecycle ordering: any module could initialize any static at any
//!   time, making startup ordering implicit and fragile.
//! - No cleanup hook: when the process exits (especially the daemon),
//!   there's no central place to flush buffers, disconnect sessions, or
//!   close file handles.
//! - Testing was harder: tests had to know which statics to reset.
//!
//! ## Design
//!
//! `AppState` bundles every piece of process-wide mutable state into a single
//! `Arc`-able struct. The existing `OnceLock`-based accessor functions
//! (`sessions()`, `forwards()`, `connections()`, etc.) remain as thin
//! wrappers that delegate to `app_state()`, so existing call sites don't
//! need to change. New code should prefer `app_state()` directly.
//!
//! ## Transport-agnostic Host (P2 #6)
//!
//! The `Host` enum abstracts over the transport layer: Tauri IPC (desktop),
//! WebSocket (daemon), or CLI (headless). Engine code uses `Host::emit()`
//! and `Host::state()` uniformly without knowing which transport it's
//! running under.
//!
//! ## ResourceReservation (P2 #9)
//!
//! The `ResourceReservation` RAII guard ensures that every resource (SSH
//! session, port forward, SFTP transfer) goes through a unified lifecycle:
//! `reserve → activate → close`. If a reservation is dropped before
//! activation (e.g., the connection failed), the guard marks the registry
//! entry as `Closed` automatically, preventing orphaned `Pending` entries.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use serde::Serialize;
use tokio::sync::{broadcast, Mutex as TokioMutex};
use uuid::Uuid;

// ── P2 #5: Centralized state ─────────────────────────────────────────────

/// Process-wide application state.
///
/// Every field was previously a separate `OnceLock` static in its own module.
/// They are collected here so there's a single source of truth for what state
/// the process holds. The accessor functions in each module (`sessions()`,
/// `forwards()`, etc.) delegate to `app_state()` so existing call sites
/// remain unchanged.
///
/// Fields are `pub` because the per-module accessors need to reach them,
/// but new code should prefer `app_state()` over reaching into individual
/// fields from unrelated modules.
pub struct AppState {
    // session.rs — interactive SSH terminal sessions
    pub sessions: TokioMutex<HashMap<Uuid, Arc<Mutex<crate::session::SessionHandle>>>>,

    // forward.rs — port forwarding handles
    pub forwards: TokioMutex<HashMap<Uuid, crate::forward::ForwardHandle>>,

    // connection.rs — retained SSH connections + per-host locks
    pub connections: TokioMutex<HashMap<String, crate::connection::RetainedConnection>>,
    pub host_locks: TokioMutex<HashMap<String, Arc<TokioMutex<()>>>>,

    // sftp_transfer.rs — transfer cancellation flags
    pub transfer_cancels: Mutex<HashMap<String, Arc<std::sync::atomic::AtomicBool>>>,

    // tauri_commands.rs — desktop session input buffers
    pub desktop_session_buffers: TokioMutex<HashMap<Uuid, String>>,

    // anomaly.rs — error burst detection sliding window
    pub error_times: Mutex<std::collections::VecDeque<chrono::DateTime<chrono::Utc>>>,
    pub last_error_alert: Mutex<Option<chrono::DateTime<chrono::Utc>>>,

    // diagnostics.rs — file write lock
    pub diagnostic_lock: Mutex<()>,
    // diagnostics.rs — error sink for diagnostic error notifications.
    // RwLock (not Mutex) so the read path (checking + cloning the sink) doesn't
    // block other readers. The sink is behind Arc so the call site can invoke
    // it without holding the lock (re-entrancy safety).
    pub error_sink: RwLock<
        Option<std::sync::Arc<dyn Fn(&crate::diagnostics::DiagnosticLogEntry) + Send + Sync>>,
    >,

    // store.rs — config file write lock
    pub store_lock: Mutex<()>,

    // events.rs — event bus sender (created with a channel at init time)
    pub event_tx: broadcast::Sender<crate::events::Agent2SSHEvent>,

    // approval.rs — approval requests store (loaded from disk on init)
    pub approvals: TokioMutex<crate::approval::ApprovalStore>,

    // risk_config.rs — risk rules cache (loaded lazily)
    pub risk_rules_cache: TokioMutex<crate::risk_config::CachedRules>,

    // secrets.rs — unlocked master key (None = locked)
    pub secrets_key: RwLock<Option<[u8; 32]>>,
    // secrets.rs — in-memory store for tests / memory backend
    pub secrets_memory: Mutex<HashMap<String, String>>,

    // ── P2 #9: Lifecycle registry ────────────────────────────────────────
    /// Central lifecycle registry. Every resource (session, forward,
    /// connection, transfer) must be reserved before creation and activated
    /// once the handle is ready. This prevents orphaned resources and
    /// provides a single place to enumerate or clean up all active
    /// resources.
    ///
    /// Uses `Arc<Mutex<...>>` so `ResourceReservation` guards can hold a
    /// clone of the Arc and clean up on drop without borrow conflicts.
    pub lifecycle: Arc<Mutex<crate::lifecycle::LifecycleRegistry>>,

    // ── P2 #6: Transport-agnostic Host ─────────────────────────────────
    /// The active transport mode (Tauri / Headless / Cli).
    /// Set once at startup by the binary entry point (`run_tauri`,
    /// daemon `main`, or CLI `main`). Engine code reads it via `host()`
    /// to make transport-aware decisions without `#[cfg]` gates.
    pub host: RwLock<Host>,
}

impl AppState {
    fn new() -> Self {
        let (event_tx, _) = broadcast::channel(1024);
        let approvals = crate::approval::load_persisted_approval_store().unwrap_or_else(|_| {
            crate::approval::ApprovalStore {
                requests: HashMap::new(),
            }
        });
        Self {
            sessions: TokioMutex::new(HashMap::new()),
            forwards: TokioMutex::new(HashMap::new()),
            connections: TokioMutex::new(HashMap::new()),
            host_locks: TokioMutex::new(HashMap::new()),
            transfer_cancels: Mutex::new(HashMap::new()),
            desktop_session_buffers: TokioMutex::new(HashMap::new()),
            error_times: Mutex::new(std::collections::VecDeque::new()),
            last_error_alert: Mutex::new(None),
            diagnostic_lock: Mutex::new(()),
            error_sink: RwLock::new(None),
            store_lock: Mutex::new(()),
            event_tx,
            approvals: TokioMutex::new(approvals),
            risk_rules_cache: TokioMutex::new(crate::risk_config::CachedRules {
                rules: crate::risk_config::RiskRules::default(),
                modified: None,
            }),
            secrets_key: RwLock::new(None),
            secrets_memory: Mutex::new(HashMap::new()),
            lifecycle: Arc::new(Mutex::new(crate::lifecycle::LifecycleRegistry::new())),
            host: RwLock::new(Host::Cli),
        }
    }
}

static APP_STATE: OnceLock<AppState> = OnceLock::new();

/// Get the process-wide `AppState`. Initializes on first call.
pub fn app_state() -> &'static AppState {
    APP_STATE.get_or_init(AppState::new)
}

/// Get the active transport `Host`. Defaults to `Host::Cli` until a binary
/// entry point (`run_tauri`, daemon `main`, etc.) calls `set_host()`.
///
/// Engine code uses this to make transport-aware decisions without
/// `#[cfg]` gates — e.g., `host().is_desktop()` to decide whether to
/// push events to the Tauri frontend.
pub fn host() -> std::sync::RwLockReadGuard<'static, Host> {
    app_state().host.read().unwrap_or_else(|p| p.into_inner())
}

/// Set the active transport. Called once at startup by the binary entry
/// point. Returns the previous host (usually `Host::Cli`).
pub fn set_host(new_host: Host) -> Host {
    let mut guard = app_state().host.write().unwrap_or_else(|p| p.into_inner());
    std::mem::replace(&mut *guard, new_host)
}

/// Get a clone of the lifecycle registry handle for resource reservation.
///
/// Engine code uses this to integrate resource creation with the lifecycle
/// system: call `LifecycleRegistry::reserve()` before creating a resource,
/// `reservation.activate()` once the handle is ready, and `registry.close()`
/// when the resource is released.
pub fn lifecycle() -> Arc<Mutex<crate::lifecycle::LifecycleRegistry>> {
    Arc::clone(&app_state().lifecycle)
}

// ── P2 #6: Transport-agnostic Host ───────────────────────────────────────

/// Discriminates the kind of transport a resource belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    SshSession,
    Forward,
    Connection,
    Transfer,
}

/// Who owns a resource — a desktop window or a headless connection.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceOwner {
    /// Desktop window label (Tauri).
    Window(String),
    /// Headless connection (daemon CLI / MCP).
    Headless(Uuid),
    /// No specific owner (CLI one-shot).
    Anonymous,
}

/// Lifecycle phase of a resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourcePhase {
    /// Reserved but not yet connected.
    Pending,
    /// Active and ready.
    Ready,
    /// Closed (manually or automatically).
    Closed,
}

/// A record in the lifecycle registry.
#[derive(Debug, Clone, Serialize)]
pub struct ResourceRecord {
    pub id: String,
    pub kind: ResourceKind,
    pub owner: ResourceOwner,
    pub phase: ResourcePhase,
    /// Monotonic nonce to prevent ABA: if a resource is closed and a new
    /// one reuses the same ID, stale reservations carrying the old nonce
    /// are rejected.
    pub nonce: Uuid,
    /// When the resource was reserved (not serialized — Instant is not
    /// serde-serializable, and this is only for internal diagnostics).
    #[serde(skip)]
    pub reserved_at: std::time::Instant,
}

// ── P2 #9: Transport-agnostic Host abstraction ──────────────────────────

/// Transport-agnostic host context.
///
/// Abstracts over the transport layer so engine code can emit events and
/// access state without knowing whether it's running under Tauri (desktop),
/// a WebSocket server (daemon), or a CLI one-shot.
///
/// ## Usage
///
/// Engine code changes only `app.state::<AppState>()` to `host.state()` and
/// `app.emit(...)` to `host.emit(...)`. The `Host` enum handles the dispatch.
#[derive(Clone)]
pub enum Host {
    /// Desktop: delegate to Tauri's AppHandle for IPC.
    #[cfg(feature = "tauri")]
    Tauri(tauri::AppHandle),

    /// Headless: emit via a sink closure, reach state via the global
    /// `app_state()`.
    Headless {
        /// A sink closure that receives (event_name, payload_json) and
        /// returns `true` if delivered, `false` if the connection is gone.
        sink: Arc<dyn Fn(&str, serde_json::Value) -> bool + Send + Sync>,
    },

    /// CLI: no event emission, just log to stderr.
    Cli,
}

impl Host {
    /// Emit an event to whatever transport this Host represents.
    pub fn emit<S: Serialize + Clone>(&self, event: &str, payload: S) {
        match self {
            #[cfg(feature = "tauri")]
            Host::Tauri(app) => {
                use tauri::Emitter as _;
                let _ = app.emit(event, payload);
            }
            Host::Headless { sink } => {
                let value = serde_json::to_value(payload).unwrap_or(serde_json::Value::Null);
                let _ = sink(event, value);
            }
            Host::Cli => {
                // CLI mode: events are silently dropped. The caller can
                // still use the event bus directly if needed.
                let _ = (event, payload);
            }
        }
    }

    /// Emit an event through the internal event bus (all transports).
    /// This is the common path for events that should be observable
    /// regardless of transport — the bus was already used by `events.rs`.
    pub fn emit_bus(&self, event_type: crate::events::EventType, data: serde_json::Value) {
        crate::events::publish_event(event_type, data);
    }

    /// Get the process-wide AppState.
    pub fn state(&self) -> &'static AppState {
        app_state()
    }

    /// Check if this host is the desktop (Tauri) variant.
    #[cfg(feature = "tauri")]
    pub fn is_desktop(&self) -> bool {
        matches!(self, Host::Tauri(..))
    }

    /// Check if this host is the desktop (Tauri) variant.
    #[cfg(not(feature = "tauri"))]
    pub fn is_desktop(&self) -> bool {
        false
    }

    /// Check if this host is the headless (daemon) variant.
    pub fn is_headless(&self) -> bool {
        matches!(self, Host::Headless { .. })
    }

    /// Check if this host is the CLI variant.
    pub fn is_cli(&self) -> bool {
        matches!(self, Host::Cli)
    }

    /// Return a string identifier for the transport, suitable for use
    /// as the `source` field in audit logs and events.
    ///
    /// - `Host::Tauri` → `"desktop"`
    /// - `Host::Headless` → `"daemon"`
    /// - `Host::Cli` → `"cli"`
    pub fn transport_name(&self) -> &'static str {
        match self {
            #[cfg(feature = "tauri")]
            Host::Tauri(..) => "desktop",
            Host::Headless { .. } => "daemon",
            Host::Cli => "cli",
        }
    }
}

/// Default host for CLI / library usage — no event emission.
impl Default for Host {
    fn default() -> Self {
        Host::Cli
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_state_initializes_once() {
        let state = app_state();
        // Should be empty on first access in a fresh process.
        assert!(state.lifecycle.lock().unwrap().records.is_empty());
        // Verify it's an Arc (cloneable).
        let _clone = Arc::clone(&state.lifecycle);
    }

    #[test]
    fn host_cli_emits_nothing() {
        let host = Host::Cli;
        // Should not panic.
        host.emit("test_event", serde_json::json!({"ok": true}));
    }

    #[test]
    fn host_headless_sink_receives_events() {
        let received = Arc::new(Mutex::new(Vec::new()));
        let received_clone = received.clone();
        let host = Host::Headless {
            sink: Arc::new(move |event, payload| {
                received_clone
                    .lock()
                    .unwrap()
                    .push((event.to_string(), payload));
                true
            }),
        };
        host.emit("test_event", serde_json::json!({"ok": true}));
        let events = received.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "test_event");
        assert_eq!(events[0].1["ok"], true);
    }
}
