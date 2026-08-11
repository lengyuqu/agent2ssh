//! Resource lifecycle management (P2 #9).
//!
//! ## Why this module exists
//!
//! Before this module, each resource type (SSH session, port forward,
//! SFTP transfer, retained connection) managed its own lifecycle
//! independently. There was no unified way to:
//!
//! - Enumerate all active resources across types.
//! - Clean up orphaned resources when a connection fails mid-setup.
//! - Prevent ABA: if resource X is closed and a new resource reuses X's
//!   ID, stale references to the old X should be rejected.
//! - Cascade-close: closing an SSH session should close its SFTP children.
//!
//! ## ResourceReservation RAII guard
//!
//! The `ResourceReservation` guard is the core of the lifecycle pattern:
//!
//! ```text
//!   let reservation = LifecycleRegistry::reserve(&registry, id, kind, owner)?;
//!   // ... attempt to connect ...
//!   reservation.activate()?;    // marks Ready
//!   // ... use the resource ...
//!   // When dropped while still `armed` (never activated), the guard
//!   // marks the record as Closed automatically.
//! ```
//!
//! This prevents orphaned `Pending` entries: if the connection fails and
//! the function returns early, the reservation's `Drop` impl cleans up
//! the registry entry.
//!
//! ## Design note: Arc<Mutex> instead of &'a mut
//!
//! The reservation holds an `Arc<Mutex<LifecycleRegistry>>` rather than
//! a `&'a mut LifecycleRegistry` reference. This allows multiple
//! simultaneous reservations (e.g., an SSH session and its port forward)
//! without borrow-checker conflicts, while still providing RAII cleanup.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use uuid::Uuid;

use crate::app_state::{ResourceKind, ResourceOwner, ResourcePhase, ResourceRecord};

/// Central lifecycle registry for all process-wide resources.
#[derive(Debug, Default)]
pub struct LifecycleRegistry {
    /// All known resources, keyed by their string ID.
    pub records: HashMap<String, ResourceRecord>,
}

impl LifecycleRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reserve a resource ID, creating a `Pending` record.
    /// Returns an error if the ID is already in use and still `Pending`
    /// or `Ready`.
    ///
    /// This is a free-function-style entry point: it takes
    /// `&Arc<Mutex<LifecycleRegistry>>` so the returned
    /// `ResourceReservation` can clean up on drop without holding a
    /// mutable borrow that would prevent concurrent reservations.
    pub fn reserve(
        registry: &Arc<Mutex<LifecycleRegistry>>,
        id: &str,
        kind: ResourceKind,
        owner: ResourceOwner,
    ) -> Result<ResourceReservation, LifecycleError> {
        let mut reg = registry.lock().unwrap();
        if let Some(existing) = reg.records.get(id) {
            if existing.phase != ResourcePhase::Closed {
                return Err(LifecycleError::AlreadyInUse(id.to_string()));
            }
        }
        let nonce = Uuid::new_v4();
        let record = ResourceRecord {
            id: id.to_string(),
            kind,
            owner: owner.clone(),
            phase: ResourcePhase::Pending,
            nonce,
            reserved_at: Instant::now(),
        };
        reg.records.insert(id.to_string(), record);
        Ok(ResourceReservation {
            registry: Arc::clone(registry),
            id: id.to_string(),
            nonce,
            armed: true,
        })
    }

    /// Activate a reserved resource, transitioning it from `Pending` to
    /// `Ready`. Verifies the nonce matches to prevent ABA.
    pub fn activate(&mut self, id: &str, nonce: Uuid) -> Result<(), LifecycleError> {
        let record = self
            .records
            .get_mut(id)
            .ok_or_else(|| LifecycleError::NotFound(id.to_string()))?;
        if record.nonce != nonce {
            return Err(LifecycleError::NonceMismatch(id.to_string()));
        }
        if record.phase != ResourcePhase::Pending {
            return Err(LifecycleError::NotPending {
                id: id.to_string(),
                phase: record.phase,
            });
        }
        record.phase = ResourcePhase::Ready;
        Ok(())
    }

    /// Close a resource, transitioning it to `Closed`.
    /// Returns `Ok(())` even if the resource was already closed (idempotent).
    pub fn close(&mut self, id: &str, nonce: Option<Uuid>) -> Result<(), LifecycleError> {
        let record = self
            .records
            .get_mut(id)
            .ok_or_else(|| LifecycleError::NotFound(id.to_string()))?;
        if let Some(nonce) = nonce {
            if record.nonce != nonce {
                return Err(LifecycleError::NonceMismatch(id.to_string()));
            }
        }
        record.phase = ResourcePhase::Closed;
        Ok(())
    }

    /// Get a reference to a resource record.
    pub fn get(&self, id: &str) -> Option<&ResourceRecord> {
        self.records.get(id)
    }

    /// List all resources in a given phase.
    pub fn list_by_phase(&self, phase: ResourcePhase) -> Vec<&ResourceRecord> {
        self.records.values().filter(|r| r.phase == phase).collect()
    }

    /// List all resources owned by a given owner.
    pub fn list_by_owner(&self, owner: &ResourceOwner) -> Vec<&ResourceRecord> {
        self.records
            .values()
            .filter(|r| &r.owner == owner)
            .collect()
    }

    /// Close all resources owned by `owner`, except those in `keep_ids`.
    /// Returns the IDs that were closed.
    pub fn close_owner(&mut self, owner: &ResourceOwner, keep_ids: &[&str]) -> Vec<String> {
        let to_close: Vec<String> = self
            .records
            .iter()
            .filter(|(_, r)| &r.owner == owner && r.phase != ResourcePhase::Closed)
            .filter(|(id, _)| !keep_ids.contains(&id.as_str()))
            .map(|(id, _)| id.clone())
            .collect();
        for id in &to_close {
            if let Some(r) = self.records.get_mut(id) {
                r.phase = ResourcePhase::Closed;
            }
        }
        to_close
    }

    /// Remove all `Closed` records from the registry (garbage collection).
    /// Returns the number of records removed.
    pub fn gc(&mut self) -> usize {
        let before = self.records.len();
        self.records.retain(|_, r| r.phase != ResourcePhase::Closed);
        before - self.records.len()
    }
}

/// RAII guard for a reserved resource.
///
/// When dropped while still `armed` (i.e., `activate()` was never called),
/// the `Drop` impl marks the registry record as `Closed`, preventing
/// orphaned `Pending` entries.
///
/// Unlike a `&'a mut`-based guard, this holds an `Arc<Mutex<...>>` clone,
/// allowing multiple simultaneous reservations without borrow conflicts.
pub struct ResourceReservation {
    registry: Arc<Mutex<LifecycleRegistry>>,
    id: String,
    nonce: Uuid,
    armed: bool,
}

impl ResourceReservation {
    /// Activate the reservation, transitioning it from `Pending` to `Ready`.
    /// After this call, the reservation is no longer armed (Drop is a no-op).
    pub fn activate(mut self) -> Result<(), LifecycleError> {
        self.armed = false;
        let result = {
            let mut reg = self.registry.lock().unwrap();
            reg.activate(&self.id, self.nonce)
        };
        // self is dropped here; Drop sees armed=false and does nothing.
        result
    }

    /// Get the resource ID.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Get the nonce (for passing to `close` later).
    pub fn nonce(&self) -> Uuid {
        self.nonce
    }
}

impl Drop for ResourceReservation {
    fn drop(&mut self) {
        if self.armed {
            // Never activated — mark as Closed to prevent orphaned Pending.
            let mut reg = self.registry.lock().unwrap();
            let _ = reg.close(&self.id, Some(self.nonce));
        }
    }
}

/// Errors from lifecycle operations.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum LifecycleError {
    #[error("resource already in use: {0}")]
    AlreadyInUse(String),
    #[error("resource not found: {0}")]
    NotFound(String),
    #[error("nonce mismatch for resource {0} (possible ABA)")]
    NonceMismatch(String),
    #[error("resource {id} is not Pending (current phase: {phase:?})")]
    NotPending { id: String, phase: ResourcePhase },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owner() -> ResourceOwner {
        ResourceOwner::Headless(Uuid::new_v4())
    }

    fn make_registry() -> Arc<Mutex<LifecycleRegistry>> {
        Arc::new(Mutex::new(LifecycleRegistry::new()))
    }

    #[test]
    fn reserve_activate_close_roundtrip() {
        let reg = make_registry();

        let reservation =
            LifecycleRegistry::reserve(&reg, "session-1", ResourceKind::SshSession, owner())
                .unwrap();
        assert_eq!(
            reg.lock().unwrap().get("session-1").unwrap().phase,
            ResourcePhase::Pending
        );

        reservation.activate().unwrap();
        assert_eq!(
            reg.lock().unwrap().get("session-1").unwrap().phase,
            ResourcePhase::Ready
        );
    }

    #[test]
    fn drop_without_activate_marks_closed() {
        let reg = make_registry();

        {
            let reservation =
                LifecycleRegistry::reserve(&reg, "session-2", ResourceKind::SshSession, owner())
                    .unwrap();
            assert_eq!(
                reg.lock().unwrap().get("session-2").unwrap().phase,
                ResourcePhase::Pending
            );
            // Drop without activating.
            drop(reservation);
        }
        assert_eq!(
            reg.lock().unwrap().get("session-2").unwrap().phase,
            ResourcePhase::Closed
        );
    }

    #[test]
    fn double_reserve_fails() {
        let reg = make_registry();
        let owner = owner();

        let _r1 =
            LifecycleRegistry::reserve(&reg, "session-3", ResourceKind::SshSession, owner.clone())
                .unwrap();
        let result = LifecycleRegistry::reserve(&reg, "session-3", ResourceKind::SshSession, owner);
        assert!(matches!(result, Err(LifecycleError::AlreadyInUse(_))));
    }

    #[test]
    fn reserve_after_close_succeeds() {
        let reg = make_registry();
        let id = "session-4";
        let owner = owner();

        let nonce1;
        {
            let r1 = LifecycleRegistry::reserve(&reg, id, ResourceKind::SshSession, owner.clone())
                .unwrap();
            nonce1 = r1.nonce();
            r1.activate().unwrap();
        }
        reg.lock().unwrap().close(id, None).unwrap();

        // After close, a new reservation with the same ID should work
        // and get a different nonce (ABA prevention).
        let r2 = LifecycleRegistry::reserve(&reg, id, ResourceKind::SshSession, owner).unwrap();
        let nonce2 = r2.nonce();
        assert_ne!(nonce1, nonce2);
        assert_eq!(reg.lock().unwrap().get(id).unwrap().nonce, nonce2);
    }

    #[test]
    fn nonce_mismatch_prevents_aba() {
        let reg = make_registry();
        let id = "session-5";

        let r1 = LifecycleRegistry::reserve(&reg, id, ResourceKind::SshSession, owner()).unwrap();
        let nonce1 = r1.nonce();
        r1.activate().unwrap();
        reg.lock().unwrap().close(id, None).unwrap();

        // New reservation gets a new nonce.
        let r2 = LifecycleRegistry::reserve(&reg, id, ResourceKind::SshSession, owner()).unwrap();
        let nonce2 = r2.nonce();
        assert_ne!(nonce1, nonce2);
        r2.activate().unwrap();

        // Trying to close with the old nonce should fail.
        let result = reg.lock().unwrap().close(id, Some(nonce1));
        assert!(matches!(result, Err(LifecycleError::NonceMismatch(_))));
    }

    #[test]
    fn activate_wrong_phase_fails() {
        let reg = make_registry();
        let id = "session-6";

        let r1 = LifecycleRegistry::reserve(&reg, id, ResourceKind::SshSession, owner()).unwrap();
        r1.activate().unwrap();

        // Try to activate again — should fail because it's already Ready.
        let r2 =
            LifecycleRegistry::reserve(&reg, "other", ResourceKind::SshSession, owner()).unwrap();
        // Manually call activate on the already-Ready resource.
        let result = reg.lock().unwrap().activate(id, r2.nonce());
        // nonce mismatch (r2 has a different nonce)
        assert!(matches!(result, Err(LifecycleError::NonceMismatch(_))));
    }

    #[test]
    fn close_owner_cascades() {
        let reg = make_registry();
        let owner = owner();

        // Reserve several resources for the same owner.
        let r1 =
            LifecycleRegistry::reserve(&reg, "a", ResourceKind::SshSession, owner.clone()).unwrap();
        let r2 =
            LifecycleRegistry::reserve(&reg, "b", ResourceKind::Forward, owner.clone()).unwrap();
        let r3 =
            LifecycleRegistry::reserve(&reg, "c", ResourceKind::Transfer, owner.clone()).unwrap();
        r1.activate().unwrap();
        r2.activate().unwrap();
        r3.activate().unwrap();

        // Close all except "b".
        let closed = reg.lock().unwrap().close_owner(&owner, &["b"]);
        assert_eq!(closed.len(), 2);
        assert!(closed.contains(&"a".to_string()));
        assert!(closed.contains(&"c".to_string()));
        assert!(!closed.contains(&"b".to_string()));

        assert_eq!(
            reg.lock().unwrap().get("a").unwrap().phase,
            ResourcePhase::Closed
        );
        assert_eq!(
            reg.lock().unwrap().get("b").unwrap().phase,
            ResourcePhase::Ready
        );
        assert_eq!(
            reg.lock().unwrap().get("c").unwrap().phase,
            ResourcePhase::Closed
        );
    }

    #[test]
    fn gc_removes_closed_records() {
        let reg = make_registry();

        let r1 = LifecycleRegistry::reserve(&reg, "a", ResourceKind::SshSession, owner()).unwrap();
        r1.activate().unwrap();
        reg.lock().unwrap().close("a", None).unwrap();

        let r2 = LifecycleRegistry::reserve(&reg, "b", ResourceKind::Forward, owner()).unwrap();
        r2.activate().unwrap();

        let mut reg_guard = reg.lock().unwrap();
        assert_eq!(reg_guard.records.len(), 2);
        let removed = reg_guard.gc();
        assert_eq!(removed, 1);
        assert_eq!(reg_guard.records.len(), 1);
        assert!(reg_guard.records.contains_key("b"));
    }

    #[test]
    fn list_by_phase_and_owner() {
        let reg = make_registry();
        let owner1 = owner();
        let owner2 = ResourceOwner::Headless(Uuid::new_v4());

        let r1 = LifecycleRegistry::reserve(&reg, "a", ResourceKind::SshSession, owner1.clone())
            .unwrap();
        r1.activate().unwrap();
        let r2 =
            LifecycleRegistry::reserve(&reg, "b", ResourceKind::Forward, owner2.clone()).unwrap();
        // r2 stays Pending — drop it so it gets marked Closed
        drop(r2);

        let reg_guard = reg.lock().unwrap();
        let ready = reg_guard.list_by_phase(ResourcePhase::Ready);
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, "a");

        // r2 was dropped without activating, so it's Closed, not Pending
        let closed = reg_guard.list_by_phase(ResourcePhase::Closed);
        assert_eq!(closed.len(), 1);
        assert_eq!(closed[0].id, "b");

        let owner1_resources = reg_guard.list_by_owner(&owner1);
        assert_eq!(owner1_resources.len(), 1);
        let owner2_resources = reg_guard.list_by_owner(&owner2);
        assert_eq!(owner2_resources.len(), 1);
    }
}
