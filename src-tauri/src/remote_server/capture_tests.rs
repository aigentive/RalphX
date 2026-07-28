use super::{CaptureFeed, CapturedEvent, EventRegistrar, RemoteEventCapture};
use ralphx_remote_protocol::{EventClassification, EventDelivery, EVENT_CLASSIFICATIONS};
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
fn installs_once_for_each_backend_non_excluded_non_local_event_only() {
    let registrar = RecordingRegistrar::default();
    let (feed, _receivers) = CaptureFeed::channels(16);
    RemoteEventCapture::install_with_registrar(registrar.clone(), feed);

    for entry in EVENT_CLASSIFICATIONS {
        let expected = usize::from(
            entry.origin == ralphx_remote_protocol::EventOrigin::Backend
                && entry.delivery != EventDelivery::LocalOnly
                && !entry.excluded_from_v1,
        );
        assert_eq!(registrar.count(entry.name), expected, "{}", entry.name);
    }
    assert_eq!(registrar.count("agent_terminal:event"), 0);
    assert_eq!(registrar.count("task:updated"), 0);
}

/// Backend-origin Local-only rows exist today (native-menu/gh chrome) and PR 1.4 adds more
/// (`remote:session_connected`/`remote:session_closed`). None may reach a capture seam.
#[test]
fn backend_origin_local_only_entries_register_no_handler() {
    let registrar = RecordingRegistrar::default();
    let (feed, mut receivers) = CaptureFeed::channels(16);
    RemoteEventCapture::install_with_registrar(registrar.clone(), feed);

    let backend_local_names = EVENT_CLASSIFICATIONS
        .iter()
        .filter(|entry| {
            entry.delivery == EventDelivery::LocalOnly
                && entry.origin == ralphx_remote_protocol::EventOrigin::Backend
        })
        .map(|entry| entry.name)
        .collect::<Vec<_>>();
    assert!(
        backend_local_names.contains(&"ralphx://check-for-updates"),
        "expected the native-menu chrome events to be classified backend + local-only"
    );

    for name in backend_local_names {
        assert_eq!(registrar.count(name), 0, "{name}");
        // Emitting is a no-op precisely because nothing registered a handler.
        registrar.emit(name, "{}");
    }
    assert!(receivers.durable.try_recv().is_err());
    assert!(receivers.transient.try_recv().is_err());
}

#[test]
fn routes_raw_payloads_to_the_classified_sync_channel() {
    let registrar = RecordingRegistrar::default();
    let (feed, mut receivers) = CaptureFeed::channels(16);
    RemoteEventCapture::install_with_registrar(registrar.clone(), feed);

    registrar.emit("notification:created", r#"{"id":"n-1"}"#);
    registrar.emit("agent:chunk", r#"{"text":"hi"}"#);

    assert_eq!(
        receivers.durable.try_recv().unwrap(),
        CapturedEvent {
            name: "notification:created",
            payload: r#"{"id":"n-1"}"#.to_string()
        }
    );
    assert_eq!(
        receivers.transient.try_recv().unwrap(),
        CapturedEvent {
            name: "agent:chunk",
            payload: r#"{"text":"hi"}"#.to_string()
        }
    );
}

/// The handler is channel-send-only (§3.4): it never parses on the emitting thread, so payload
/// validation is the drain side's job (PR 1.4's sequencer). What it must still guarantee is that
/// a full durable channel drops instead of blocking the emit thread.
#[test]
fn full_durable_channel_drops_without_blocking_the_emit_thread() {
    let registrar = RecordingRegistrar::default();
    let (feed, mut receivers) = CaptureFeed::channels(1);
    RemoteEventCapture::install_with_registrar(registrar.clone(), feed);
    registrar.emit("notification:created", "{}");
    registrar.emit("notification:created", "{}");
    assert!(receivers.durable.try_recv().is_ok());
    assert!(receivers.durable.try_recv().is_err());
}

#[test]
fn table_has_no_duplicate_names() {
    let mut names = std::collections::HashSet::new();
    for entry in EVENT_CLASSIFICATIONS {
        assert!(names.insert(entry.name), "duplicate {}", entry.name);
    }
    assert!(EventClassification::find("notification:created").is_some());
}
