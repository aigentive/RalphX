use std::sync::{Arc, Mutex};

use serde::Serialize;
use serde_json::Value;

use crate::internal::{EventEnvelope, InternalEventBus};

pub trait EventSink: Send + Sync {
    fn emit(&self, event: &str, payload: Value);
}

pub fn emit_serialized<T: Serialize + ?Sized>(
    sink: &dyn EventSink,
    event: &str,
    payload: &T,
) -> Result<(), serde_json::Error> {
    let value = serde_json::to_value(payload)?;
    sink.emit(event, value);
    Ok(())
}

#[derive(Debug, Clone, Default)]
pub struct NullEventSink;

impl EventSink for NullEventSink {
    fn emit(&self, _event: &str, _payload: Value) {}
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecordedEvent {
    pub event: String,
    pub payload: Value,
}

#[derive(Debug, Clone, Default)]
pub struct RecordingEventSink {
    events: Arc<Mutex<Vec<RecordedEvent>>>,
}

impl RecordingEventSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn events(&self) -> Vec<RecordedEvent> {
        self.events.lock().expect("recording sink poisoned").clone()
    }
}

impl EventSink for RecordingEventSink {
    fn emit(&self, event: &str, payload: Value) {
        self.events
            .lock()
            .expect("recording sink poisoned")
            .push(RecordedEvent {
                event: event.to_string(),
                payload,
            });
    }
}

#[derive(Clone)]
pub struct TeeEventSink {
    sinks: Vec<Arc<dyn EventSink>>,
}

impl TeeEventSink {
    pub fn new(sinks: Vec<Arc<dyn EventSink>>) -> Self {
        Self { sinks }
    }

    pub fn len(&self) -> usize {
        self.sinks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sinks.is_empty()
    }
}

impl EventSink for TeeEventSink {
    fn emit(&self, event: &str, payload: Value) {
        for sink in &self.sinks {
            sink.emit(event, payload.clone());
        }
    }
}

#[derive(Debug, Clone)]
pub struct BusEventSink {
    bus: InternalEventBus,
}

impl BusEventSink {
    pub fn new(bus: InternalEventBus) -> Self {
        Self { bus }
    }

    pub fn bus(&self) -> &InternalEventBus {
        &self.bus
    }
}

impl EventSink for BusEventSink {
    fn emit(&self, event: &str, payload: Value) {
        let _ = self.bus.publish(EventEnvelope::new(event, payload));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recording_sink_records_json_payloads() {
        let sink = RecordingEventSink::new();
        sink.emit(
            "task:status_changed",
            serde_json::json!({"task_id": "task-1"}),
        );

        assert_eq!(
            sink.events(),
            vec![RecordedEvent {
                event: "task:status_changed".to_string(),
                payload: serde_json::json!({"task_id": "task-1"}),
            }]
        );
    }

    #[test]
    fn tee_sink_forwards_to_all_sinks() {
        let first = RecordingEventSink::new();
        let second = RecordingEventSink::new();
        let tee = TeeEventSink::new(vec![Arc::new(first.clone()), Arc::new(second.clone())]);

        assert_eq!(tee.len(), 2);
        assert!(!tee.is_empty());

        tee.emit("event", serde_json::json!({"ok": true}));

        assert_eq!(
            first.events(),
            vec![RecordedEvent {
                event: "event".to_string(),
                payload: serde_json::json!({"ok": true}),
            }]
        );
        assert_eq!(
            second.events(),
            vec![RecordedEvent {
                event: "event".to_string(),
                payload: serde_json::json!({"ok": true}),
            }]
        );
    }

    #[test]
    fn empty_tee_sink_reports_no_sinks() {
        let tee = TeeEventSink::new(Vec::new());

        assert_eq!(tee.len(), 0);
        assert!(tee.is_empty());

        tee.emit("event", serde_json::json!({"ok": true}));
    }

    #[test]
    fn serialized_emit_skips_on_serialization_error() {
        use serde::ser::{Error, SerializeStruct};

        struct FailingPayload;

        impl Serialize for FailingPayload {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                let mut state = serializer.serialize_struct("FailingPayload", 1)?;
                state
                    .serialize_field("field", &serde_json::Value::Null)
                    .and_then(|_| Err(S::Error::custom("intentional failure")))
            }
        }

        let sink = RecordingEventSink::new();
        let result = emit_serialized(&sink, "event", &FailingPayload);

        assert!(result.is_err());
        assert!(sink.events().is_empty());
    }

    #[test]
    fn bus_sink_publishes_envelopes() {
        let bus = InternalEventBus::new();
        let mut sub = bus.subscribe();
        let sink = BusEventSink::new(bus.clone());

        assert_eq!(sink.bus().subscriber_count(), 1);

        sink.emit("event", serde_json::json!({"value": 1}));

        let envelope = sub.try_recv().expect("envelope");
        assert_eq!(envelope.name, "event");
        assert_eq!(envelope.payload, serde_json::json!({"value": 1}));
        assert_eq!(bus.events_published(), 1);
    }
}
