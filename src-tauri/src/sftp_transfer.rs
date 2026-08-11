//! SFTP transfer robustness: cancellation + resume (K6).
//!
//! Before K6 transfers were a single `std::io::copy` with no way to stop a
//! large transfer mid-flight and no way to recover from an interruption without
//! re-sending from byte 0. This module adds:
//!
//! - a process-global **cancellation registry** keyed by an opaque transfer id,
//!   so a caller can flip a flag that the copy loop observes between chunks; and
//! - a **resume offset** helper that decides where a transfer should pick up
//!   given how many bytes already landed on the destination.
//!
//! Both `session.rs`/`forward.rs` state and this registry are process-local; a
//! daemon restart drops in-flight transfers (callers must restart them — see the
//! restart notice logged at daemon startup).

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use uuid::Uuid;

use crate::app_state::app_state;

/// Chunk size for cancellable copies. Large enough to keep throughput high,
/// small enough that a cancel is observed within a few hundred KB.
const COPY_CHUNK: usize = 64 * 1024;

fn registry() -> &'static Mutex<HashMap<String, Arc<AtomicBool>>> {
    &app_state().transfer_cancels
}

/// Register a cancellable transfer, returning its shared cancel flag. The copy
/// loop is handed this flag; [`cancel_transfer`] flips it.
pub fn register(transfer_id: &str) -> Arc<AtomicBool> {
    let flag = Arc::new(AtomicBool::new(false));
    registry()
        .lock()
        .unwrap()
        .insert(transfer_id.to_string(), flag.clone());

    // Reserve + activate a lifecycle entry so this transfer is tracked.
    let lifecycle = crate::app_state::lifecycle();
    if let Ok(reservation) = crate::lifecycle::LifecycleRegistry::reserve(
        &lifecycle,
        transfer_id,
        crate::app_state::ResourceKind::Transfer,
        crate::app_state::ResourceOwner::Headless(Uuid::new_v4()),
    ) {
        let _ = reservation.activate();
    }

    flag
}

/// Remove a transfer from the registry once it finishes (success or failure).
pub fn unregister(transfer_id: &str) {
    registry().lock().unwrap().remove(transfer_id);

    // Mark the lifecycle entry as Closed.
    let _ = crate::app_state::lifecycle()
        .lock()
        .unwrap()
        .close(transfer_id, None);
}

/// Request cancellation of an in-flight transfer. Returns true if a transfer
/// with that id was registered.
pub fn cancel_transfer(transfer_id: &str) -> bool {
    match registry().lock().unwrap().get(transfer_id) {
        Some(flag) => {
            flag.store(true, Ordering::SeqCst);
            true
        }
        None => false,
    }
}

/// Number of currently-registered (in-flight) transfers.
pub fn active_count() -> usize {
    registry().lock().unwrap().len()
}

/// Copy `reader` into `writer` in chunks, aborting with an `Interrupted` error
/// as soon as `cancel` is set. Returns the number of bytes copied this call (the
/// caller adds any pre-existing resume offset to get the file's total length).
pub fn copy_cancellable<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    cancel: &AtomicBool,
) -> io::Result<u64> {
    let mut buf = vec![0u8; COPY_CHUNK];
    let mut total: u64 = 0;
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "transfer cancelled",
            ));
        }
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        writer.write_all(&buf[..n])?;
        total += n as u64;
    }
    writer.flush()?;
    Ok(total)
}

/// Decide where a resumed transfer should start.
///
/// `existing` is how many bytes already exist on the destination; `total` is the
/// full size of the source (when known). Returns the byte offset to resume from:
/// - `Some(0)` when not resuming, or when the destination is empty, or when the
///   destination is already as large as (or larger than) the source — in which
///   case the caller should restart from scratch to avoid a corrupt/over-long
///   result;
/// - `Some(existing)` when a partial transfer can be continued.
pub fn resume_offset(resume: bool, existing: u64, total: Option<u64>) -> u64 {
    if !resume || existing == 0 {
        return 0;
    }
    match total {
        // Destination already complete or longer than source: don't append, redo.
        Some(t) if existing >= t => 0,
        _ => existing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn copy_cancellable_copies_all_when_not_cancelled() {
        let data = vec![7u8; 200 * 1024];
        let mut reader = Cursor::new(data.clone());
        let mut writer: Vec<u8> = Vec::new();
        let flag = AtomicBool::new(false);
        let n = copy_cancellable(&mut reader, &mut writer, &flag).unwrap();
        assert_eq!(n, data.len() as u64);
        assert_eq!(writer, data);
    }

    #[test]
    fn copy_cancellable_aborts_when_flag_set() {
        let data = vec![0u8; 1024];
        let mut reader = Cursor::new(data);
        let mut writer: Vec<u8> = Vec::new();
        let flag = AtomicBool::new(true); // already cancelled
        let err = copy_cancellable(&mut reader, &mut writer, &flag).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::Interrupted);
    }

    #[test]
    fn registry_cancel_roundtrip() {
        let id = "k6-test-transfer";
        let flag = register(id);
        assert!(!flag.load(Ordering::SeqCst));
        assert!(cancel_transfer(id));
        assert!(flag.load(Ordering::SeqCst));
        unregister(id);
        assert!(!cancel_transfer(id)); // gone after unregister
    }

    #[test]
    fn resume_offset_decisions() {
        // Not resuming -> always from 0.
        assert_eq!(resume_offset(false, 500, Some(1000)), 0);
        // Resuming, partial -> continue from existing.
        assert_eq!(resume_offset(true, 500, Some(1000)), 500);
        // Resuming but empty destination -> from 0.
        assert_eq!(resume_offset(true, 0, Some(1000)), 0);
        // Resuming but destination already complete/over -> restart from 0.
        assert_eq!(resume_offset(true, 1000, Some(1000)), 0);
        assert_eq!(resume_offset(true, 1200, Some(1000)), 0);
        // Resuming with unknown total -> trust existing.
        assert_eq!(resume_offset(true, 500, None), 500);
    }
}
