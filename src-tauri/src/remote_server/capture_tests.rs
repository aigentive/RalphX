use super::{CaptureFeed, CapturedEvent, EventRegistrar, RemoteEventCapture};
use ralphx_remote_protocol::{EventClassification, EventDelivery, EVENT_CLASSIFICATIONS};
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

type Handler = Box<dyn Fn(&str) + Send + Sync>;

#[derive(Clone, Default)]
struct RecordingRegistrar(Arc<Mutex<HashMap<&'static str, Vec<Handler>>>>);

impl EventRegistrar for RecordingRegistrar {
    fn listen(&self, name: &'static str, handler: Handler) {
        self.0
            .lock()
            .unwrap()
            .entry(name)
            .or_default()
            .push(handler);
    }
}

impl RecordingRegistrar {
    fn emit(&self, name: &'static str, payload: &str) {
        if let Some(handlers) = self.0.lock().unwrap().get(name) {
            for handler in handlers {
                handler(payload);
            }
        }
    }
    fn count(&self, name: &str) -> usize {
        self.0.lock().unwrap().get(name).map_or(0, Vec::len)
    }
}

#[test]
fn installs_once_for_each_backend_non_excluded_event_only() {
    let registrar = RecordingRegistrar::default();
    let (feed, _receivers) = CaptureFeed::channels(16);
    RemoteEventCapture::install_with_registrar(registrar.clone(), feed);

    for entry in EVENT_CLASSIFICATIONS {
        let expected = usize::from(
            entry.origin == ralphx_remote_protocol::EventOrigin::Backend && !entry.excluded_from_v1,
        );
        assert_eq!(registrar.count(entry.name), expected, "{}", entry.name);
    }
    assert_eq!(registrar.count("agent_terminal:event"), 0);
    assert_eq!(registrar.count("task:updated"), 0);
}

#[test]
fn routes_parsed_payloads_to_the_classified_sync_channel() {
    let registrar = RecordingRegistrar::default();
    let (feed, receivers) = CaptureFeed::channels(16);
    RemoteEventCapture::install_with_registrar(registrar.clone(), feed);

    registrar.emit("notification:created", r#"{"id":"n-1"}"#);
    registrar.emit("agent:chunk", r#"{"text":"hi"}"#);

    assert_eq!(
        receivers.durable.try_recv().unwrap(),
        CapturedEvent {
            name: "notification:created",
            payload: json!({"id":"n-1"})
        }
    );
    assert_eq!(
        receivers.transient.try_recv().unwrap(),
        CapturedEvent {
            name: "agent:chunk",
            payload: json!({"text":"hi"})
        }
    );
}

#[test]
fn malformed_payload_and_full_durable_channel_fail_closed_without_blocking() {
    let registrar = RecordingRegistrar::default();
    let (feed, receivers) = CaptureFeed::channels(1);
    RemoteEventCapture::install_with_registrar(registrar.clone(), feed);
    registrar.emit("notification:created", "not-json");
    assert!(receivers.durable.try_recv().is_err());
    registrar.emit("notification:created", "{}");
    registrar.emit("notification:created", "{}");
    assert!(receivers.durable.try_recv().is_ok());
    assert!(receivers.durable.try_recv().is_err());
}

#[test]
fn table_has_no_duplicate_names_and_local_entries_are_not_backend_origin() {
    let mut names = std::collections::HashSet::new();
    for entry in EVENT_CLASSIFICATIONS {
        assert!(names.insert(entry.name), "duplicate {}", entry.name);
        if entry.delivery == EventDelivery::LocalOnly {
            assert_ne!(entry.origin, ralphx_remote_protocol::EventOrigin::Backend);
        }
    }
    assert!(EventClassification::find("notification:created").is_some());
}
