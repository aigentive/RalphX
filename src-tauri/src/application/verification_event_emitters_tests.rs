use ralphx_events::RecordingEventSink;

use crate::application::verification_event_emitters::{
    emit_verification_pending_confirmation, emit_verification_started,
    emit_verification_status_changed,
};
use crate::domain::entities::VerificationStatus;

#[test]
fn status_changed_emits_canonical_payload() {
    let sink = RecordingEventSink::new();

    emit_verification_status_changed(
        &sink,
        "session-1",
        VerificationStatus::Unverified,
        false,
        None,
        Some("spawn_failed"),
        Some(7),
    );

    let events = sink.events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event, "plan_verification:status_changed");
    assert_eq!(events[0].payload["session_id"], "session-1");
    assert_eq!(events[0].payload["status"], "unverified");
    assert_eq!(events[0].payload["in_progress"], false);
    assert_eq!(events[0].payload["generation"], 7);
    assert_eq!(events[0].payload["convergence_reason"], "spawn_failed");
    assert_eq!(events[0].payload["current_gaps"], serde_json::json!([]));
    assert_eq!(events[0].payload["rounds"], serde_json::json!([]));
}

#[test]
fn status_changed_skips_imported_verified() {
    let sink = RecordingEventSink::new();

    emit_verification_status_changed(
        &sink,
        "session-imported",
        VerificationStatus::ImportedVerified,
        false,
        None,
        None,
        None,
    );

    assert!(sink.events().is_empty());
}

#[test]
fn verification_started_emits_reviewing_snapshot() {
    let sink = RecordingEventSink::new();

    emit_verification_started(&sink, "session-started", 3, 5);

    let events = sink.events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event, "plan_verification:status_changed");
    assert_eq!(events[0].payload["session_id"], "session-started");
    assert_eq!(events[0].payload["status"], "reviewing");
    assert_eq!(events[0].payload["in_progress"], true);
    assert_eq!(events[0].payload["generation"], 3);
    assert_eq!(events[0].payload["round"], serde_json::Value::Null);
    assert_eq!(events[0].payload["max_rounds"], 5);
    assert_eq!(events[0].payload["current_gaps"], serde_json::json!([]));
    assert_eq!(events[0].payload["rounds"], serde_json::json!([]));
}

#[test]
fn pending_confirmation_emits_dialog_payload() {
    let sink = RecordingEventSink::new();

    emit_verification_pending_confirmation(&sink, "session-pending", "Plan title", "artifact-1");

    let events = sink.events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event, "verification:pending_confirmation");
    assert_eq!(
        events[0].payload,
        serde_json::json!({
            "session_id": "session-pending",
            "session_title": "Plan title",
            "plan_artifact_id": "artifact-1",
        })
    );
}
