use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::event_bus::EventBus;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub event_id: Uuid,
    pub name: String,
    pub payload: Value,
}

impl EventEnvelope {
    pub fn new(name: impl Into<String>, payload: Value) -> Self {
        Self {
            event_id: Uuid::new_v4(),
            name: name.into(),
            payload,
        }
    }
}

pub type InternalEventBus = EventBus<EventEnvelope>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_envelope_new_assigns_id_name_and_payload() {
        let payload = serde_json::json!({"task_id": "task-1"});

        let envelope = EventEnvelope::new("task:status_changed", payload.clone());

        assert_ne!(envelope.event_id, Uuid::nil());
        assert_eq!(envelope.name, "task:status_changed");
        assert_eq!(envelope.payload, payload);
    }
}
