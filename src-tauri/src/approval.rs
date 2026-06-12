use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::OnceLock,
};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::types::RiskLevel;

/// Default TTL for approval requests in seconds (5 minutes).
/// Can be overridden per-request by setting `ttl_secs` on the `ApprovalRequest`
/// after creation, or by using `approval_request_with_ttl`.
pub const DEFAULT_APPROVAL_TTL_SECS: u64 = 300;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub id: Uuid,
    pub host: String,
    pub command: String,
    pub risk_level: RiskLevel,
    pub requested_at: DateTime<Utc>,
    pub ttl_secs: u64,
    pub status: ApprovalStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalStatus {
    Pending,
    Approved,
    Rejected,
    TimedOut,
}

struct ApprovalStore {
    requests: HashMap<Uuid, ApprovalRequest>,
}

static APPROVALS: OnceLock<Mutex<ApprovalStore>> = OnceLock::new();

fn store() -> &'static Mutex<ApprovalStore> {
    APPROVALS.get_or_init(|| {
        Mutex::new(ApprovalStore {
            requests: HashMap::new(),
        })
    })
}

pub async fn approval_request(host: &str, command: &str, risk_level: RiskLevel) -> Uuid {
    approval_request_with_ttl(host, command, risk_level, DEFAULT_APPROVAL_TTL_SECS).await
}

/// Create an approval request with a custom TTL.
pub async fn approval_request_with_ttl(
    host: &str,
    command: &str,
    risk_level: RiskLevel,
    ttl_secs: u64,
) -> Uuid {
    let id = Uuid::new_v4();
    let req = ApprovalRequest {
        id,
        host: host.to_string(),
        command: command.to_string(),
        risk_level,
        requested_at: Utc::now(),
        ttl_secs,
        status: ApprovalStatus::Pending,
    };
    store().lock().await.requests.insert(id, req);
    id
}

pub async fn approval_poll(id: Uuid) -> Option<ApprovalStatus> {
    let s = store().lock().await;
    s.requests.get(&id).map(|r| {
        if r.status == ApprovalStatus::Pending {
            let elapsed = Utc::now().signed_duration_since(r.requested_at);
            if elapsed.num_seconds() as u64 > r.ttl_secs {
                ApprovalStatus::TimedOut
            } else {
                ApprovalStatus::Pending
            }
        } else {
            r.status
        }
    })
}

pub async fn approval_respond(id: Uuid, approved: bool) -> Result<()> {
    let mut s = store().lock().await;
    let req = s
        .requests
        .get_mut(&id)
        .ok_or_else(|| anyhow!("unknown approval: {id}"))?;

    // Check TTL expiration first, regardless of current status
    let elapsed = Utc::now().signed_duration_since(req.requested_at);
    if elapsed.num_seconds() as u64 > req.ttl_secs {
        if req.status == ApprovalStatus::Pending {
            req.status = ApprovalStatus::TimedOut;
        }
        return Err(anyhow!("approval {id} has timed out"));
    }

    if req.status != ApprovalStatus::Pending {
        return Err(anyhow!("approval {id} already {:?}", req.status));
    }

    req.status = if approved {
        ApprovalStatus::Approved
    } else {
        ApprovalStatus::Rejected
    };
    Ok(())
}

pub async fn approval_list() -> Vec<ApprovalRequest> {
    let mut s = store().lock().await;
    let now = Utc::now();
    for req in s.requests.values_mut() {
        if req.status == ApprovalStatus::Pending {
            let elapsed = now.signed_duration_since(req.requested_at);
            if elapsed.num_seconds() as u64 > req.ttl_secs {
                req.status = ApprovalStatus::TimedOut;
            }
        }
    }
    s.requests.values().cloned().collect()
}

/// Wait for an approval to be resolved (approved/rejected/timed out).
/// Returns the final status.
pub async fn approval_wait(id: Uuid) -> ApprovalStatus {
    loop {
        let status = approval_poll(id).await;
        match status {
            Some(ApprovalStatus::Pending) => {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
            Some(s) => return s,
            None => return ApprovalStatus::Rejected,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_approval_approve_flow() {
        let id = approval_request("testhost", "ls -la", RiskLevel::High).await;
        assert_eq!(approval_poll(id).await, Some(ApprovalStatus::Pending));
        approval_respond(id, true).await.unwrap();
        assert_eq!(approval_poll(id).await, Some(ApprovalStatus::Approved));
    }

    #[tokio::test]
    async fn test_approval_reject_flow() {
        let id = approval_request("testhost", "rm -rf /tmp", RiskLevel::High).await;
        assert_eq!(approval_poll(id).await, Some(ApprovalStatus::Pending));
        approval_respond(id, false).await.unwrap();
        assert_eq!(approval_poll(id).await, Some(ApprovalStatus::Rejected));
    }

    #[tokio::test]
    async fn test_approval_unknown_id() {
        let fake = Uuid::new_v4();
        assert_eq!(approval_poll(fake).await, None);
        assert!(approval_respond(fake, true).await.is_err());
    }

    #[tokio::test]
    async fn test_approval_double_respond() {
        let id = approval_request("testhost", "sudo whoami", RiskLevel::High).await;
        approval_respond(id, true).await.unwrap();
        assert!(approval_respond(id, false).await.is_err());
    }

    #[tokio::test]
    async fn test_approval_list_returns_all() {
        let id1 = approval_request("h1", "cmd1", RiskLevel::Medium).await;
        let id2 = approval_request("h2", "cmd2", RiskLevel::High).await;
        let list = approval_list().await;
        assert!(list.iter().any(|r| r.id == id1));
        assert!(list.iter().any(|r| r.id == id2));
    }

    // ── TTL behavior tests ──────────────────────────────────────────────────

    /// Approval created within TTL reports as Pending.
    #[tokio::test]
    async fn test_approval_within_ttl_is_pending() {
        let id = approval_request_with_ttl("host", "cmd", RiskLevel::High, 60).await;
        let status = approval_poll(id).await;
        assert_eq!(status, Some(ApprovalStatus::Pending));

        // Also verify via approval_list: should still be pending
        let list = approval_list().await;
        let entry = list.iter().find(|r| r.id == id).unwrap();
        assert_eq!(entry.status, ApprovalStatus::Pending);
    }

    /// Approval past TTL is reported as TimedOut.
    #[tokio::test]
    async fn test_approval_past_ttl_is_timed_out() {
        // Use a 1-second TTL and wait for it to expire
        let id = approval_request_with_ttl("host", "cmd", RiskLevel::High, 1).await;

        // Confirm it's pending immediately
        assert_eq!(approval_poll(id).await, Some(ApprovalStatus::Pending));

        // Wait for TTL to expire
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        // Now it should be TimedOut
        assert_eq!(approval_poll(id).await, Some(ApprovalStatus::TimedOut));
    }

    /// Approval list marks expired entries as TimedOut.
    #[tokio::test]
    async fn test_approval_list_marks_expired_as_timed_out() {
        let id = approval_request_with_ttl("host", "cmd", RiskLevel::High, 1).await;
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        let list = approval_list().await;
        let entry = list.iter().find(|r| r.id == id).unwrap();
        assert_eq!(entry.status, ApprovalStatus::TimedOut);
    }

    /// Timed-out approval cannot be approved.
    #[tokio::test]
    async fn test_timed_out_approval_cannot_be_approved() {
        let id = approval_request_with_ttl("host", "cmd", RiskLevel::High, 1).await;
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        let result = approval_respond(id, true).await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("timed out"),
            "expected 'timed out' in error, got: {}",
            err_msg
        );
    }

    /// Timed-out approval cannot be rejected.
    #[tokio::test]
    async fn test_timed_out_approval_cannot_be_rejected() {
        let id = approval_request_with_ttl("host", "cmd", RiskLevel::High, 1).await;
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        let result = approval_respond(id, false).await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("timed out"),
            "expected 'timed out' in error, got: {}",
            err_msg
        );
    }

    /// Default TTL constant is 300 seconds.
    #[test]
    fn test_default_ttl_is_300() {
        assert_eq!(DEFAULT_APPROVAL_TTL_SECS, 300);
    }

    /// Default approval_request uses DEFAULT_APPROVAL_TTL_SECS.
    #[tokio::test]
    async fn test_default_approval_uses_default_ttl() {
        let id = approval_request("host", "cmd", RiskLevel::High).await;
        let s = store().lock().await;
        let req = s.requests.get(&id).unwrap();
        assert_eq!(req.ttl_secs, DEFAULT_APPROVAL_TTL_SECS);
    }
}
