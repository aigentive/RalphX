use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ralphx_events::catalog::{AGENT_RUN_COMPLETED, AGENT_TURN_COMPLETED};
use ralphx_events::{EventSink, InternalEventBus};
use serde_json::{json, Value};

use crate::application::completion_correlation::{
    CompletionCorrelationRegistry, CompletionCorrelationSource,
};
use super::agent_completion_event_runtime::{
    CorrelatedTauriBusEventSink, TauriCompletionEventEmitter,
};

#[derive(Clone, Default)]
struct RecordingEmitter {
    emissions: Arc<Mutex<Vec<(String, Value)>>>,
    failures: Arc<Mutex<VecDeque<String>>>,
}

impl RecordingEmitter {
    fn fail_next(&self, message: impl Into<String>) {
        self.failures
            .lock()
            .expect("test emitter failures poisoned")
            .push_back(message.into());
    }

    fn emissions(&self) -> Vec<(String, Value)> {
        self.emissions
            .lock()
            .expect("test emitter emissions poisoned")
            .clone()
    }
}

impl TauriCompletionEventEmitter for RecordingEmitter {
    fn emit_completion_event(&self, event: &str, payload: &Value) -> Result<(), String> {
        if let Some(error) = self
            .failures
            .lock()
            .expect("test emitter failures poisoned")
            .pop_front()
        {
            return Err(error);
        }
        self.emissions
            .lock()
            .expect("test emitter emissions poisoned")
            .push((event.to_string(), payload.clone()));
        Ok(())
    }
}

fn clock_at(start: Instant) -> (Arc<Mutex<Instant>>, Arc<dyn Fn() -> Instant + Send + Sync>) {
    let clock = Arc::new(Mutex::new(start));
    let reader = Arc::clone(&clock);
    (
        clock,
        Arc::new(move || *reader.lock().expect("test clock poisoned")),
    )
}

#[test]
fn correlation_registry_matches_identical_payloads_in_fifo_order() {
    let start = Instant::now();
    let (_clock, now) = clock_at(start);
    let registry = CompletionCorrelationRegistry::with_clock(Duration::from_secs(60), 4, now);
    let payload = json!({"run_id": "run-1", "nested": {"value": true}});

    let first = registry
        .reserve(AGENT_RUN_COMPLETED, &payload)
        .expect("first reservation");
    let second = registry
        .reserve(AGENT_RUN_COMPLETED, &payload)
        .expect("second reservation");

    assert_eq!(
        registry.resolve_tauri(AGENT_RUN_COMPLETED, &payload),
        Some(first)
    );
    assert_eq!(
        registry.resolve_tauri(AGENT_RUN_COMPLETED, &payload),
        Some(second)
    );
}

#[test]
fn correlation_registry_uses_json_value_equality_not_serialized_key_order() {
    let start = Instant::now();
    let (_clock, now) = clock_at(start);
    let registry = CompletionCorrelationRegistry::with_clock(Duration::from_secs(60), 4, now);
    let reserved = json!({"outer": {"a": 1, "b": 2}});
    let callback = serde_json::from_str(r#"{"outer":{"b":2,"a":1}}"#).expect("json payload");

    let event_id = registry
        .reserve(AGENT_TURN_COMPLETED, &reserved)
        .expect("reservation");

    assert_eq!(
        registry.resolve_tauri(AGENT_TURN_COMPLETED, &callback),
        Some(event_id)
    );
}

#[test]
fn correlation_registry_marks_each_source_before_removing_completed_entry() {
    let start = Instant::now();
    let (_clock, now) = clock_at(start);
    let registry = CompletionCorrelationRegistry::with_clock(Duration::from_secs(60), 4, now);
    let payload = json!({"run_id": "run-1"});
    let event_id = registry
        .reserve(AGENT_RUN_COMPLETED, &payload)
        .expect("reservation");

    assert!(registry.mark_source(event_id, CompletionCorrelationSource::Bus));
    assert_eq!(registry.len(), 1, "Tauri is still outstanding");
    assert_eq!(
        registry.resolve_tauri(AGENT_RUN_COMPLETED, &payload),
        Some(event_id)
    );
    assert_eq!(registry.len(), 0, "both delivery sources settled");
}

#[test]
fn correlation_registry_purges_expired_entries_before_accepting_capacity_pressure() {
    let start = Instant::now();
    let (clock, now) = clock_at(start);
    let registry = CompletionCorrelationRegistry::with_clock(Duration::from_secs(60), 1, now);
    let first = json!({"run_id": "first"});
    let second = json!({"run_id": "second"});

    registry
        .reserve(AGENT_RUN_COMPLETED, &first)
        .expect("initial reservation");
    assert!(registry.reserve(AGENT_RUN_COMPLETED, &second).is_none());

    *clock.lock().expect("test clock poisoned") = start + Duration::from_secs(60);

    assert!(registry.reserve(AGENT_RUN_COMPLETED, &second).is_some());
    assert_eq!(registry.len(), 1);
}

#[test]
fn correlation_registry_never_evicts_an_unexpired_reservation() {
    let start = Instant::now();
    let (_clock, now) = clock_at(start);
    let registry = CompletionCorrelationRegistry::with_clock(Duration::from_secs(60), 1, now);
    let first = json!({"run_id": "first"});
    let second = json!({"run_id": "second"});

    let first_id = registry
        .reserve(AGENT_RUN_COMPLETED, &first)
        .expect("initial reservation");

    assert!(registry.reserve(AGENT_RUN_COMPLETED, &second).is_none());
    assert_eq!(
        registry.resolve_tauri(AGENT_RUN_COMPLETED, &first),
        Some(first_id)
    );
}

#[test]
fn correlated_sink_keeps_bus_delivery_eligible_when_tauri_emit_fails() {
    let start = Instant::now();
    let (_clock, now) = clock_at(start);
    let registry = Arc::new(CompletionCorrelationRegistry::with_clock(
        Duration::from_secs(60),
        4,
        now,
    ));
    let bus = InternalEventBus::new();
    let mut subscriber = bus.subscribe();
    let emitter = RecordingEmitter::default();
    emitter.fail_next("tauri unavailable");
    let sink = CorrelatedTauriBusEventSink::new(emitter.clone(), bus, Arc::clone(&registry));
    let payload = json!({"run_id": "run-1"});

    sink.emit(AGENT_RUN_COMPLETED, payload.clone());

    assert!(emitter.emissions().is_empty());
    assert_eq!(registry.len(), 0, "failed Tauri reservation is cleaned up");
    let envelope = subscriber
        .try_recv()
        .expect("bus envelope remains available");
    assert_eq!(envelope.name, AGENT_RUN_COMPLETED);
    assert_eq!(envelope.payload, payload);
}

#[test]
fn correlated_sink_preserves_frontend_payload_and_shares_one_bus_envelope_id() {
    let start = Instant::now();
    let (_clock, now) = clock_at(start);
    let registry = Arc::new(CompletionCorrelationRegistry::with_clock(
        Duration::from_secs(60),
        4,
        now,
    ));
    let bus = InternalEventBus::new();
    let mut subscriber = bus.subscribe();
    let emitter = RecordingEmitter::default();
    let sink = CorrelatedTauriBusEventSink::new(emitter.clone(), bus, Arc::clone(&registry));
    let payload = json!({"run_id": "run-1", "snake_case": true});

    sink.emit(AGENT_RUN_COMPLETED, payload.clone());

    assert_eq!(
        emitter.emissions(),
        vec![(AGENT_RUN_COMPLETED.to_string(), payload.clone())]
    );
    let envelope = subscriber.try_recv().expect("bus envelope");
    assert_eq!(envelope.payload, payload);
    assert_eq!(
        registry.resolve_tauri(AGENT_RUN_COMPLETED, &envelope.payload),
        Some(envelope.event_id)
    );
}
