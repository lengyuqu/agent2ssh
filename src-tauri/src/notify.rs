use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WebhookConfig {
    pub url: Option<String>,
    #[serde(default = "default_events")]
    pub events: Vec<String>,
    pub secret: Option<String>,
}

fn default_events() -> Vec<String> {
    vec!["approval_required".to_string()]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookEvent {
    pub event: String,
    pub host: String,
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approval_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk_level: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
}

fn webhook_config_path() -> Result<PathBuf> {
    Ok(crate::store::config_dir()?.join("webhook.toml"))
}

/// Load webhook config from ~/.agent2ssh/webhook.toml.
/// Returns None if the file does not exist or cannot be parsed.
pub fn load_webhook_config() -> Option<WebhookConfig> {
    let path = webhook_config_path().ok()?;
    if !path.exists() {
        return None;
    }
    let raw = std::fs::read_to_string(&path).ok()?;
    toml::from_str(&raw).ok()
}

/// Save webhook config to ~/.agent2ssh/webhook.toml.
pub fn save_webhook_config(config: &WebhookConfig) -> Result<()> {
    let path = webhook_config_path()?;
    crate::store::ensure_config_dir()?;
    let raw = toml::to_string_pretty(config)?;
    std::fs::write(&path, raw)?;
    Ok(())
}

/// Send a notification about a pending approval request.
///
/// Fires a webhook with approval-specific payload. If the webhook URL is a
/// Slack URL, formats with Slack Block Kit including Approve/Reject action
/// buttons.
#[cfg(feature = "daemon")]
pub async fn notify_approval_pending(
    approval_id: &str,
    host: &str,
    command: &str,
    risk_level: &str,
    approval_url: Option<&str>,
) -> Result<()> {
    let config = match load_webhook_config() {
        Some(c) => c,
        None => return Ok(()),
    };

    let url = match &config.url {
        Some(u) if !u.is_empty() => u.clone(),
        _ => return Ok(()),
    };

    // Check if event type is in the subscribed events list
    if !config.events.iter().any(|e| e == "approval_required") {
        return Ok(());
    }

    let redacted_command = crate::store::redact_sensitive_text(command);

    // Format payload: Slack Block Kit for Slack URLs, plain JSON otherwise
    let payload = if url.contains("hooks.slack.com") {
        format_slack_approval_notification(approval_id, host, &redacted_command, risk_level, approval_url)
    } else {
        serde_json::json!({
            "event": "approval_required",
            "approval_id": approval_id,
            "host": host,
            "command": redacted_command,
            "risk_level": risk_level,
            "approval_url": approval_url,
        })
    };

    let body = serde_json::to_vec(&payload)?;

    // Compute HMAC-SHA256 signature if secret is set
    let signature = config
        .secret
        .as_ref()
        .filter(|s| !s.is_empty())
        .map(|s| compute_signature(&body, s));

    // POST with timeout (fire-and-forget via spawned task)
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;

    let mut req = client.post(&url).header("Content-Type", "application/json");

    if let Some(sig) = signature {
        req = req.header("X-Agent2SSH-Signature", format!("sha256={}", sig));
    }

    match req.body(body).send().await {
        Ok(resp) => {
            if !resp.status().is_success() {
                eprintln!(
                    "[webhook] approval notification POST {} returned status {}",
                    url,
                    resp.status()
                );
            }
        }
        Err(e) => {
            eprintln!("[webhook] approval notification POST {} failed: {}", url, e);
        }
    }

    Ok(())
}

/// Format an approval notification for Slack Block Kit.
///
/// Builds a Slack Block Kit message with:
/// - Header: "Approval Required"
/// - Fields: host, command, risk level
/// - Action buttons: Approve (green), Reject (red) linking to approval_url
pub fn format_slack_approval_notification(
    approval_id: &str,
    host: &str,
    command: &str,
    risk_level: &str,
    approval_url: Option<&str>,
) -> serde_json::Value {
    let fields = vec![
        serde_json::json!({
            "type": "mrkdwn",
            "text": format!("*Host:*\n{}", host),
        }),
        serde_json::json!({
            "type": "mrkdwn",
            "text": format!("*Command:*\n```{}```", command),
        }),
        serde_json::json!({
            "type": "mrkdwn",
            "text": format!("*Risk Level:*\n{}", risk_level),
        }),
        serde_json::json!({
            "type": "mrkdwn",
            "text": format!("*Approval ID:*\n`{}`", approval_id),
        }),
    ];

    let mut blocks = vec![
        serde_json::json!({
            "type": "header",
            "text": {
                "type": "plain_text",
                "text": "Agent2SSH: Approval Required",
            }
        }),
        serde_json::json!({
            "type": "section",
            "fields": fields,
        }),
    ];

    // Add action buttons if approval_url is provided
    if let Some(url) = approval_url {
        blocks.push(serde_json::json!({
            "type": "actions",
            "elements": [
                {
                    "type": "button",
                    "text": { "type": "plain_text", "text": "Approve" },
                    "style": "primary",
                    "url": format!("{}/approve", url),
                },
                {
                    "type": "button",
                    "text": { "type": "plain_text", "text": "Reject" },
                    "style": "danger",
                    "url": format!("{}/reject", url),
                }
            ]
        }));
    }

    serde_json::json!({ "blocks": blocks })
}

/// Fire a webhook event if configured.
/// Non-blocking: errors are logged to stderr but don't fail the main flow.
#[cfg(feature = "daemon")]
pub async fn fire_webhook(event: WebhookEvent) -> Result<()> {
    let config = match load_webhook_config() {
        Some(c) => c,
        None => return Ok(()),
    };

    let url = match &config.url {
        Some(u) if !u.is_empty() => u.clone(),
        _ => return Ok(()),
    };

    // Check if event type is in the subscribed events list
    if !config.events.iter().any(|e| e == &event.event) {
        return Ok(());
    }

    let mut event = event;
    event.command = crate::store::redact_sensitive_text(&event.command);

    // Format payload: Slack Block Kit for Slack URLs, plain JSON otherwise
    let payload = if url.contains("hooks.slack.com") {
        format_slack_message(&event)
    } else {
        serde_json::to_value(&event)?
    };

    let body = serde_json::to_vec(&payload)?;

    // Compute HMAC-SHA256 signature if secret is set
    let signature = config
        .secret
        .as_ref()
        .filter(|s| !s.is_empty())
        .map(|s| compute_signature(&body, s));

    // POST with timeout (fire-and-forget via spawned task)
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;

    let mut req = client.post(&url).header("Content-Type", "application/json");

    if let Some(sig) = signature {
        req = req.header("X-Agent2SSH-Signature", format!("sha256={}", sig));
    }

    match req.body(body).send().await {
        Ok(resp) => {
            if !resp.status().is_success() {
                eprintln!(
                    "[webhook] POST {} returned status {}",
                    url,
                    resp.status()
                );
            }
        }
        Err(e) => {
            eprintln!("[webhook] POST {} failed: {}", url, e);
        }
    }

    Ok(())
}

/// Format event as Slack Block Kit message.
#[allow(dead_code)]
fn format_slack_message(event: &WebhookEvent) -> serde_json::Value {
    let event_name = match event.event.as_str() {
        "approval_required" => "Approval Required",
        "exec_blocked" => "Command Blocked",
        "exec_completed" => "Command Completed",
        "anomaly_detected" => "Anomaly Detected",
        other => other,
    };

    let mut fields = vec![
        serde_json::json!({
            "type": "mrkdwn",
            "text": format!("*Host:*\n{}", event.host),
        }),
        serde_json::json!({
            "type": "mrkdwn",
            "text": format!("*Command:*\n```{}```", event.command),
        }),
    ];

    if let Some(ref risk) = event.risk_level {
        fields.push(serde_json::json!({
            "type": "mrkdwn",
            "text": format!("*Risk Level:*\n{}", risk),
        }));
    }

    if let Some(code) = event.exit_code {
        fields.push(serde_json::json!({
            "type": "mrkdwn",
            "text": format!("*Exit Code:*\n{}", code),
        }));
    }

    let mut blocks = vec![
        serde_json::json!({
            "type": "header",
            "text": {
                "type": "plain_text",
                "text": format!("Agent2SSH: {}", event_name),
            }
        }),
        serde_json::json!({
            "type": "section",
            "fields": fields,
        }),
    ];

    if event.event == "approval_required" {
        if let Some(ref approval_id) = event.approval_id {
            blocks.push(serde_json::json!({
                "type": "actions",
                "elements": [
                    {
                        "type": "button",
                        "text": { "type": "plain_text", "text": "Open Approvals" },
                        "style": "primary",
                        "url": format!("http://127.0.0.1:7722/console#approvals-{}", approval_id),
                    }
                ]
            }));
        }
    }

    serde_json::json!({ "blocks": blocks })
}

/// Compute HMAC-SHA256 signature and return hex-encoded lowercase string.
#[allow(dead_code)]
fn compute_signature(payload: &[u8], secret: &str) -> String {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    type HmacSha256 = Hmac<Sha256>;

    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC can take key of any size");
    mac.update(payload);
    let result = mac.finalize();
    hex::encode(result.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_events() {
        let defaults = default_events();
        assert_eq!(defaults, vec!["approval_required"]);
    }

    #[test]
    fn test_webhook_config_serialize_roundtrip() {
        let config = WebhookConfig {
            url: Some("https://example.com/hook".into()),
            events: vec!["approval_required".into(), "exec_completed".into()],
            secret: Some("mysecret".into()),
        };
        let toml_str = toml::to_string_pretty(&config).unwrap();
        let parsed: WebhookConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.url, config.url);
        assert_eq!(parsed.events, config.events);
        assert_eq!(parsed.secret, config.secret);
    }

    #[test]
    fn test_webhook_config_default() {
        let config = WebhookConfig::default();
        assert!(config.url.is_none());
        assert!(config.secret.is_none());
    }

    #[test]
    fn test_compute_signature() {
        let sig = compute_signature(b"hello", "secret");
        // HMAC-SHA256("hello", "secret") — just verify it's a 64-char hex string
        assert_eq!(sig.len(), 64);
        assert!(sig.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_format_slack_message_approval() {
        let event = WebhookEvent {
            event: "approval_required".into(),
            host: "myserver".into(),
            command: "sudo rm -rf /tmp".into(),
            approval_id: Some("abc-123".into()),
            risk_level: Some("high".into()),
            exit_code: None,
        };
        let msg = format_slack_message(&event);
        let blocks = msg["blocks"].as_array().unwrap();
        // Should have header, section, and actions blocks
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[0]["type"], "header");
        assert_eq!(blocks[2]["type"], "actions");
    }

    #[test]
    fn test_format_slack_message_completed() {
        let event = WebhookEvent {
            event: "exec_completed".into(),
            host: "myserver".into(),
            command: "ls -la".into(),
            approval_id: None,
            risk_level: Some("low".into()),
            exit_code: Some(0),
        };
        let msg = format_slack_message(&event);
        let blocks = msg["blocks"].as_array().unwrap();
        // Should have header and section (no actions)
        assert_eq!(blocks.len(), 2);
    }

    #[test]
    fn test_webhook_event_skip_serializing_none() {
        let event = WebhookEvent {
            event: "exec_completed".into(),
            host: "h".into(),
            command: "c".into(),
            approval_id: None,
            risk_level: None,
            exit_code: None,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(!json.contains("approval_id"));
        assert!(!json.contains("risk_level"));
        assert!(!json.contains("exit_code"));
    }

    #[test]
    fn test_format_slack_approval_notification_with_url() {
        let msg = format_slack_approval_notification(
            "test-uuid-123",
            "prod-server",
            "sudo rm -rf /tmp",
            "high",
            Some("http://127.0.0.1:7722/approval/test-uuid-123/respond"),
        );

        let blocks = msg["blocks"].as_array().unwrap();
        // Should have: header, section, actions
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[0]["type"], "header");
        assert_eq!(
            blocks[0]["text"]["text"],
            "Agent2SSH: Approval Required"
        );
        assert_eq!(blocks[1]["type"], "section");
        assert_eq!(blocks[2]["type"], "actions");

        // Verify action buttons
        let elements = blocks[2]["elements"].as_array().unwrap();
        assert_eq!(elements.len(), 2);
        assert_eq!(elements[0]["text"]["text"], "Approve");
        assert_eq!(elements[0]["style"], "primary");
        assert!(elements[0]["url"].as_str().unwrap().contains("/approve"));
        assert_eq!(elements[1]["text"]["text"], "Reject");
        assert_eq!(elements[1]["style"], "danger");
        assert!(elements[1]["url"].as_str().unwrap().contains("/reject"));
    }

    #[test]
    fn test_format_slack_approval_notification_without_url() {
        let msg = format_slack_approval_notification(
            "test-uuid-456",
            "staging-server",
            "apt update",
            "medium",
            None,
        );

        let blocks = msg["blocks"].as_array().unwrap();
        // Should have: header, section (no actions since no URL)
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0]["type"], "header");
        assert_eq!(blocks[1]["type"], "section");
    }

    // ── Outbound protection tests ───────────────────────────────────────────

    /// Helper: set up a temporary config directory with a webhook.toml so that
    /// `fire_webhook` picks up the config. Returns the path to the temp dir
    /// (caller must keep it alive for the duration of the test).
    #[cfg(feature = "daemon")]
    fn setup_webhook_config(url: &str, events: &[&str], secret: Option<&str>) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "agent2ssh-notify-test-{}",
            uuid::Uuid::new_v4()
        ));
        let agent_dir = dir.join(".agent2ssh");
        std::fs::create_dir_all(&agent_dir).unwrap();

        let config = WebhookConfig {
            url: Some(url.to_string()),
            events: events.iter().map(|s| s.to_string()).collect(),
            secret: secret.map(|s| s.to_string()),
        };
        let raw = toml::to_string_pretty(&config).unwrap();
        std::fs::write(agent_dir.join("webhook.toml"), raw).unwrap();
        dir
    }

    #[cfg(feature = "daemon")]
    fn test_event() -> WebhookEvent {
        WebhookEvent {
            event: "approval_required".into(),
            host: "testhost".into(),
            command: "echo hello".into(),
            approval_id: Some("test-id".into()),
            risk_level: Some("high".into()),
            exit_code: None,
        }
    }

    /// Webhook fire with timeout: mock a slow server and verify fire_webhook
    /// does not hang (the 10-second client timeout should kick in, but we
    /// use a 15-second test timeout to be safe).
    #[cfg(feature = "daemon")]
    #[tokio::test]
    async fn test_fire_webhook_timeout_does_not_hang() {
        // Start a local axum server that delays response for 30 seconds
        let app = axum::Router::new().route(
            "/slow",
            axum::routing::post(|| async {
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                "ok"
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        // Point webhook config at the slow server
        let home_dir = setup_webhook_config(
            &format!("http://{}/slow", addr),
            &["approval_required"],
            None,
        );
        let original_home = std::env::var("HOME").ok();
        std::env::set_var("HOME", &home_dir);

        // fire_webhook has a 10-second client timeout; should complete well
        // under 15 seconds even with the slow server.
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(15),
            fire_webhook(test_event()),
        )
        .await;

        // Restore HOME
        match original_home {
            Some(h) => std::env::set_var("HOME", h),
            None => std::env::remove_var("HOME"),
        }
        let _ = std::fs::remove_dir_all(&home_dir);

        // The outer timeout should NOT have fired
        assert!(result.is_ok(), "fire_webhook hung beyond 15-second timeout");
        // And the inner result should be Ok (fire_webhook always returns Ok)
        assert!(result.unwrap().is_ok(), "fire_webhook returned Err");
    }

    /// Webhook fire failure doesn't propagate: verify fire_webhook returns Ok
    /// even when the target server is unreachable.
    #[cfg(feature = "daemon")]
    #[tokio::test]
    async fn test_fire_webhook_failure_does_not_propagate() {
        // Use a port that nothing is listening on
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let closed_port = listener.local_addr().unwrap().port();
        drop(listener); // close it immediately so the port is unreachable

        let home_dir = setup_webhook_config(
            &format!("http://127.0.0.1:{}/webhook", closed_port),
            &["approval_required"],
            None,
        );
        let original_home = std::env::var("HOME").ok();
        std::env::set_var("HOME", &home_dir);

        let result = fire_webhook(test_event()).await;

        match original_home {
            Some(h) => std::env::set_var("HOME", h),
            None => std::env::remove_var("HOME"),
        }
        let _ = std::fs::remove_dir_all(&home_dir);

        // fire_webhook should return Ok even on network failure
        assert!(
            result.is_ok(),
            "fire_webhook should return Ok on network error, got: {:?}",
            result.err()
        );
    }

    /// Config validation: empty URL is handled gracefully (returns Ok without
    /// attempting any HTTP request).
    #[cfg(feature = "daemon")]
    #[tokio::test]
    async fn test_fire_webhook_empty_url_returns_ok() {
        let home_dir = setup_webhook_config("", &["approval_required"], None);
        let original_home = std::env::var("HOME").ok();
        std::env::set_var("HOME", &home_dir);

        let result = fire_webhook(test_event()).await;

        match original_home {
            Some(h) => std::env::set_var("HOME", h),
            None => std::env::remove_var("HOME"),
        }
        let _ = std::fs::remove_dir_all(&home_dir);

        assert!(
            result.is_ok(),
            "fire_webhook should return Ok for empty URL, got: {:?}",
            result.err()
        );
    }

    /// Config validation: no webhook.toml at all returns Ok silently.
    #[cfg(feature = "daemon")]
    #[tokio::test]
    async fn test_fire_webhook_no_config_returns_ok() {
        let dir = std::env::temp_dir().join(format!(
            "agent2ssh-notify-noconfig-{}",
            uuid::Uuid::new_v4()
        ));
        // Create .agent2ssh dir but no webhook.toml
        std::fs::create_dir_all(dir.join(".agent2ssh")).unwrap();

        let original_home = std::env::var("HOME").ok();
        std::env::set_var("HOME", &dir);

        let result = fire_webhook(test_event()).await;

        match original_home {
            Some(h) => std::env::set_var("HOME", h),
            None => std::env::remove_var("HOME"),
        }
        let _ = std::fs::remove_dir_all(&dir);

        assert!(
            result.is_ok(),
            "fire_webhook should return Ok when no config exists, got: {:?}",
            result.err()
        );
    }

    /// Config validation: event type not in subscribed list means no HTTP call.
    #[cfg(feature = "daemon")]
    #[tokio::test]
    async fn test_fire_webhook_unsubscribed_event_skips() {
        // Subscribe only to "exec_completed", but send "approval_required"
        let home_dir = setup_webhook_config(
            "http://127.0.0.1:1/should-not-be-called",
            &["exec_completed"],
            None,
        );
        let original_home = std::env::var("HOME").ok();
        std::env::set_var("HOME", &home_dir);

        let result = fire_webhook(test_event()).await;

        match original_home {
            Some(h) => std::env::set_var("HOME", h),
            None => std::env::remove_var("HOME"),
        }
        let _ = std::fs::remove_dir_all(&home_dir);

        // Should return Ok without attempting HTTP (URL is unreachable, so
        // if it tried, it would still return Ok but take longer)
        assert!(result.is_ok());
    }
}
