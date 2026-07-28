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
fn full_durable_channel_still_drops_when_control_receiver_is_closed() {
    let registrar = RecordingRegistrar::default();
    let (feed, mut receivers) = CaptureFeed::channels(1);
    let (_replacement, closed_control) = tokio::sync::mpsc::unbounded_channel();
    let original_control = std::mem::replace(&mut receivers.control, closed_control);
    drop(original_control);
    drop(receivers.control_sender);
    RemoteEventCapture::install_with_registrar(registrar.clone(), feed);

    registrar.emit("notification:created", r#"{"sequence":1}"#);
    registrar.emit("notification:created", r#"{"sequence":2}"#);

    assert_eq!(
        receivers
            .durable
            .try_recv()
            .expect("first event remains queued")
            .payload,
        r#"{"sequence":1}"#
    );
    assert!(
        receivers.durable.try_recv().is_err(),
        "overflow event is dropped"
    );
}

#[test]
fn disconnected_capture_channels_drop_without_panicking() {
    let registrar = RecordingRegistrar::default();
    let (feed, receivers) = CaptureFeed::channels(1);
    drop(receivers);
    RemoteEventCapture::install_with_registrar(registrar.clone(), feed);

    registrar.emit("notification:created", r#"{"id":"durable"}"#);
    registrar.emit("agent:chunk", r#"{"id":"transient"}"#);

    assert_eq!(registrar.count("notification:created"), 1);
    assert_eq!(registrar.count("agent:chunk"), 1);
}

/// `TauriRegistrar` and `RemoteEventCapture::install` were the two uncovered seams in this file:
/// the coverage round could not reach them because the only handle available was a Wry one, and
/// building it off the main thread panics on macOS. `MockRuntime` has no `EventLoop`, so the real
/// production install path (`install` → `TauriRegistrar::listen` → `listen_any`) runs here.
#[cfg(feature = "test-utils")]
mod tauri_registrar {
    use super::*;
    use tauri::Emitter;

    fn mock_app() -> tauri::App<tauri::test::MockRuntime> {
        tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app should build")
    }

    #[test]
    fn install_registers_real_tauri_listeners_and_routes_emitted_payloads() {
        let app = mock_app();
        let (feed, mut receivers) = CaptureFeed::channels(16);
        RemoteEventCapture::install(app.handle().clone(), feed);

        app.emit("notification:created", serde_json::json!({"id": "n-1"}))
            .expect("durable event should emit");
        app.emit("agent:chunk", serde_json::json!({"text": "hi"}))
            .expect("transient event should emit");

        let durable = receivers
            .durable
            .try_recv()
            .expect("durable event reaches the durable channel");
        assert_eq!(durable.name, "notification:created");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&durable.payload).unwrap(),
            serde_json::json!({"id": "n-1"})
        );

        let transient = receivers
            .transient
            .try_recv()
            .expect("transient event reaches the transient channel");
        assert_eq!(transient.name, "agent:chunk");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&transient.payload).unwrap(),
            serde_json::json!({"text": "hi"})
        );
    }

    /// The delivery filter is part of `install`, not of the registrar: a Local-only backend row
    /// must not reach a capture channel even when Tauri really emits it.
    #[test]
    fn install_leaves_local_only_events_uncaptured() {
        let app = mock_app();
        let (feed, mut receivers) = CaptureFeed::channels(16);
        RemoteEventCapture::install(app.handle().clone(), feed);

        app.emit("ralphx://check-for-updates", serde_json::json!({}))
            .expect("local-only event should emit");

        assert!(receivers.durable.try_recv().is_err());
        assert!(receivers.transient.try_recv().is_err());
    }

    /// `TauriRegistrar` is `Clone` so the install loop can hand a handle to every classified row;
    /// a clone must register against the same app, not a detached one.
    #[test]
    fn cloned_registrar_registers_against_the_same_app() {
        let app = mock_app();
        let (feed, mut receivers) = CaptureFeed::channels(16);
        let registrar = super::super::TauriRegistrar(app.handle().clone());
        RemoteEventCapture::install_with_registrar(registrar.clone(), feed);

        app.emit("notification:created", serde_json::json!({"id": "clone"}))
            .expect("event should emit");

        assert_eq!(
            receivers
                .durable
                .try_recv()
                .expect("cloned registrar registered a live listener")
                .name,
            "notification:created"
        );
    }
}

#[test]
fn table_has_no_duplicate_names() {
    let mut names = std::collections::HashSet::new();
    for entry in EVENT_CLASSIFICATIONS {
        assert!(names.insert(entry.name), "duplicate {}", entry.name);
    }
    assert!(EventClassification::find("notification:created").is_some());
}
