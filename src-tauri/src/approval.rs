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
    let id = Uuid::new_v4();
    let req = ApprovalRequest {
        id,
        host: host.to_string(),
        command: command.to_string(),
        risk_level,
        requested_at: Utc::now(),
        ttl_secs: 120,
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

    if req.status != ApprovalStatus::Pending {
        // Check if it's timed out
        let elapsed = Utc::now().signed_duration_since(req.requested_at);
        if elapsed.num_seconds() as u64 > req.ttl_secs {
            req.status = ApprovalStatus::TimedOut;
            return Err(anyhow!("approval {id} has timed out"));
        }
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
}
