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
