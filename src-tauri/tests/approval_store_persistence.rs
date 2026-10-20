//! On-disk contract of the approval store (`approvals.jsonl`).
//!
//! * Loading is read-only: expired entries are ignored in memory but never
//!   rewritten, so a one-shot CLI command cannot truncate the shared store.
//! * Loading only needs a shared lock, so readers don't queue behind each other.
//! * `cleanup_expired_approvals` is the one place that compacts the file.
//!
//! Single `#[test]` because it points the whole process at a temp config dir
//! through an environment variable.

use agent2ssh::approval::{cleanup_expired_approvals, load_persisted_approval_store};
use agent2ssh::store::{lock_config_file, lock_config_file_shared, CONFIG_DIR_ENV};
use std::time::Duration;

fn entry(age_days: i64, status: &str) -> String {
    serde_json::json!({
        "id": uuid::Uuid::new_v4(),
        "host": "web-1",
        "command": "systemctl restart nginx",
        "risk_level": "high",
        "requested_at": chrono::Utc::now() - chrono::Duration::days(age_days),
        "ttl_secs": 300,
        "status": status,
    })
    .to_string()
}

/// Run `f` on another thread and fail the test if it doesn't finish in time.
fn within<T: Send + 'static>(what: &str, f: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx.recv_timeout(Duration::from_secs(20))
        .unwrap_or_else(|_| panic!("{what} did not finish: lock contention or deadlock"))
}

#[test]
fn load_is_read_only_and_cleanup_compacts() {
    let dir = std::env::temp_dir().join(format!("agent2ssh-approvals-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var(CONFIG_DIR_ENV, &dir);

    let path = dir.join("approvals.jsonl");
    let original = [
        entry(400, "approved"),
        "{ not json".to_string(),
        entry(1, "pending"),
    ]
    .join("\n")
        + "\n";
    std::fs::write(&path, &original).unwrap();

    // Loading ignores the expired and malformed lines but leaves the file alone.
    let store = within("load", || load_persisted_approval_store().unwrap());
    assert_eq!(store.requests.len(), 1);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);

    // Another reader holding the shared lock does not block a load.
    let reader = lock_config_file_shared(".approvals.lock").unwrap();
    let store = within("load beside a shared reader", || {
        load_persisted_approval_store().unwrap()
    });
    assert_eq!(store.requests.len(), 1);
    drop(reader);

    // A writer holding the exclusive lock does: the load waits, then completes.
    let writer = lock_config_file(".approvals.lock").unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(load_persisted_approval_store().unwrap().requests.len());
    });
    assert!(rx.recv_timeout(Duration::from_millis(300)).is_err());
    drop(writer);
    assert_eq!(rx.recv_timeout(Duration::from_secs(20)).unwrap(), 1);

    // Only the periodic cleanup rewrites the file.
    within("cleanup", || cleanup_expired_approvals().unwrap());
    let after = std::fs::read_to_string(&path).unwrap();
    assert_eq!(after.lines().count(), 1, "unexpected store: {after}");

    let _ = std::fs::remove_dir_all(&dir);
}
