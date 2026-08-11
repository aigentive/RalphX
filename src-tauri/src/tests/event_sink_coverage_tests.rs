use std::sync::Arc;

use ralphx_events::{
    BusEventSink, EventBus, EventSink, EventSubscriber, InternalEventBus, RecordedEvent,
    RecordingEventSink, TeeEventSink,
};

#[test]
fn root_lib_coverage_exercises_event_sink_helpers() {
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
fn root_lib_coverage_exercises_empty_tee_sink() {
    let tee = TeeEventSink::new(Vec::new());

    assert_eq!(tee.len(), 0);
    assert!(tee.is_empty());

    tee.emit("event", serde_json::json!({"ok": true}));
}

#[test]
fn root_lib_coverage_exercises_bus_sink_envelopes() {
    let bus = InternalEventBus::new();
    let mut subscriber = bus.subscribe();
    let sink = BusEventSink::new(bus.clone());

    assert_eq!(sink.bus().subscriber_count(), 1);

    sink.emit("event", serde_json::json!({"value": 1}));

    let envelope = subscriber.try_recv().expect("envelope");
    assert_ne!(envelope.event_id, uuid::Uuid::nil());
    assert_eq!(envelope.name, "event");
    assert_eq!(envelope.payload, serde_json::json!({"value": 1}));
    assert_eq!(bus.events_published(), 1);
}

#[test]
fn root_lib_coverage_exercises_subscriber_closed_state() {
    let bus = EventBus::<String>::new();
    let subscriber: EventSubscriber<String> = bus.subscribe();

    assert!(!subscriber.is_closed());

    drop(bus);

    assert!(subscriber.is_closed());
}
