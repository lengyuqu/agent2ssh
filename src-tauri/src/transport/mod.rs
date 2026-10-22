//! SSH transport facade: a synchronous API over whichever backend is linked.
//!
//! # Why a facade
//!
//! The session code in this crate is thread-per-connection, blocking, and
//! written against libssh2. Replacing the backend wholesale would mean rewriting
//! 9.5k lines as async; instead the async-ness is confined to this module and the
//! rest of the crate keeps calling the same synchronous methods.
//!
//! # The runtime rule (read before adding a caller)
//!
//! The backend owns a **dedicated** tokio runtime on its own threads. It must not
//! borrow the daemon's runtime.
//!
//! - Synchronous callers (CLI, Tauri commands, the worker threads in
//!   `forward.rs`) may call these methods directly.
//! - **Code already running inside an async runtime must not call them.** The
//!   facade blocks on its own runtime, and blocking a runtime thread is how you
//!   deadlock or panic. Wrap the call in `tokio::task::spawn_blocking`.
//!
//! Today the daemon's handlers reach SSH through exactly the same blocking calls
//! they always did, so nothing new is at risk — but the rule is written down
//! here because the backend swap is what makes it non-obvious.
//!
//! # Backend
//!
//! There is one: `russh_impl`. The libssh2 backend this facade was built around
//! is gone, and with it every libssh2/OpenSSL dependency. The facade stayed
//! because it is what kept that swap from reaching the 900-odd lines of session
//! code above it.

mod russh_impl;
pub use russh_impl::{Agent, Channel, File, Listener, PublicKey, Session, Sftp, Stderr};

mod types;
pub use types::{
    DisconnectCode, Error, ErrorKind, FileStat, HashType, HostKeyKind, KeyboardInteractivePrompt,
    MethodType, OpenFlags, OpenType, Prompt,
};
