use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::app_state::app_state;

/// Represents an event emitted by the agent2ssh system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent2SSHEvent {
    /// Unique identifier for this event instance.
    pub id: String,
    /// The type of event.
    pub event_type: EventType,
    /// When the event was created.
    pub timestamp: DateTime<Utc>,
    /// Arbitrary structured data associated with the event.
    pub data: serde_json::Value,
}

/// The category of an event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    StreamConnected,
    ExecStarted,
    ExecOutput,
    ExecCompleted,
    ApprovalRequested,
    ApprovalResponded,
    HostConnected,
    HostDisconnected,
    SessionOpened,
    SessionInput,
    SessionOutput,
    SessionClosed,
    AuditRotated,
    ConfigChanged,
    GateChanged,
    GateRejected,
    LimitRejected,
    AnomalyDetected,
}

/// Get the global event bus sender.
pub fn event_bus() -> &'static broadcast::Sender<Agent2SSHEvent> {
    &app_state().event_tx
}

/// Publish an event to the event bus.
pub fn publish_event(event_type: EventType, data: serde_json::Value) {
    let event = Agent2SSHEvent {
        id: uuid::Uuid::new_v4().to_string(),
        event_type,
        timestamp: Utc::now(),
        data,
    };
    // Ignore error if no subscribers
    let _ = event_bus().send(event);
}

/// Subscribe to events. Returns a receiver that gets all future events.
pub fn subscribe_events() -> broadcast::Receiver<Agent2SSHEvent> {
    event_bus().subscribe()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_serialization() {
        let event = Agent2SSHEvent {
            id: "test-id-123".to_string(),
            event_type: EventType::ExecCompleted,
            timestamp: Utc::now(),
            data: serde_json::json!({"host": "prod-1", "exit_code": 0}),
        };
        let json = serde_json::to_string(&event).unwrap();
        let deserialized: Agent2SSHEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, "test-id-123");
        assert_eq!(deserialized.event_type, EventType::ExecCompleted);
        assert_eq!(deserialized.data["host"], "prod-1");
        assert_eq!(deserialized.data["exit_code"], 0);

        // Test all event type variants serialize correctly
        let types = vec![
            EventType::ExecStarted,
            EventType::ExecOutput,
            EventType::ExecCompleted,
            EventType::ApprovalRequested,
            EventType::ApprovalResponded,
            EventType::HostConnected,
            EventType::HostDisconnected,
            EventType::SessionOpened,
            EventType::SessionInput,
            EventType::SessionOutput,
            EventType::SessionClosed,
            EventType::AuditRotated,
            EventType::ConfigChanged,
            EventType::GateChanged,
            EventType::GateRejected,
            EventType::LimitRejected,
            EventType::AnomalyDetected,
        ];
        for et in types {
            let json = serde_json::to_string(&et).unwrap();
            let de: EventType = serde_json::from_str(&json).unwrap();
            assert_eq!(et, de);
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_event_bus_publish_subscribe() {
        let mut rx = subscribe_events();
        let run_id = uuid::Uuid::new_v4().to_string();
        publish_event(
            EventType::ExecCompleted,
            serde_json::json!({"run_id": run_id, "host": "test-host", "exit_code": 0}),
        );

        let event = loop {
            let event = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
                .await
                .unwrap()
                .unwrap();
            if event.data["run_id"] == run_id {
                break event;
            }
        };
        assert_eq!(event.event_type, EventType::ExecCompleted);
        assert_eq!(event.data["host"], "test-host");
        assert_eq!(event.data["exit_code"], 0);
        assert!(!event.id.is_empty());
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_event_bus_handles_1000_event_burst() {
        let mut rx = subscribe_events();
        let run_id = uuid::Uuid::new_v4().to_string();
        for i in 0..1000 {
            publish_event(
                EventType::ExecOutput,
                serde_json::json!({"run_id": run_id, "seq": i, "host": format!("scale-{i:03}")}),
            );
        }

        let mut expected = 0;
        while expected < 1000 {
            let event = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
                .await
                .unwrap()
                .unwrap();
            if event.data["run_id"] != run_id {
                continue;
            }
            assert_eq!(event.event_type, EventType::ExecOutput);
            assert_eq!(event.data["seq"], expected);
            expected += 1;
        }
    }

    #[test]
    fn test_event_bus_no_subscribers() {
        // Publishing with no subscribers should not panic
        publish_event(
            EventType::AuditRotated,
            serde_json::json!({"file": "audit.jsonl.1"}),
        );
        // If we get here without panicking, the test passes
    }
}
