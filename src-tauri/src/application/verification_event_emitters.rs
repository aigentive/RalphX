use std::sync::Arc;

use ralphx_events::EventSink;
use tauri::Manager;

use crate::application::AppState;
use crate::domain::entities::{VerificationRunSnapshot, VerificationStatus};
use crate::domain::services::verification_events::{
    build_verification_payload, build_verification_started_snapshot,
};

pub fn event_sink_from_app_handle<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
) -> Option<Arc<dyn EventSink>> {
    app_handle
        .try_state::<AppState>()
        .map(|state| Arc::clone(&state.events))
}

/// Emits `plan_verification:status_changed` with the canonical payload shape.
///
/// - `snapshot`: `Some` -> includes round/max_rounds/gap_score/current_gaps/rounds.
///   `None` -> all those fields are null / empty arrays.
/// - `convergence_reason`: explicit override. When `snapshot` is `Some` and this is
///   `None`, the convergence_reason stored inside the snapshot is used instead.
///
/// All emission points must call this function to maintain a consistent frontend
/// contract and prevent partial payload bugs (B2, B3, B4).
pub fn emit_verification_status_changed(
    event_sink: &dyn EventSink,
    session_id: &str,
    status: VerificationStatus,
    in_progress: bool,
    snapshot: Option<&VerificationRunSnapshot>,
    convergence_reason: Option<&str>,
    generation: Option<i32>,
) {
    // ImportedVerified is a terminal import state. The frontend learns this status
    // via polling/initial load, not via real-time events.
    if status == VerificationStatus::ImportedVerified {
        return;
    }
    let payload = build_verification_payload(
        session_id,
        status,
        in_progress,
        snapshot,
        convergence_reason,
        generation,
    );
    event_sink.emit("plan_verification:status_changed", payload);
}

/// Emit the canonical "verification started" event.
pub fn emit_verification_started(
    event_sink: &dyn EventSink,
    session_id: &str,
    generation: i32,
    max_rounds: u32,
) {
    let snapshot = build_verification_started_snapshot(generation, max_rounds);
    emit_verification_status_changed(
        event_sink,
        session_id,
        VerificationStatus::Reviewing,
        true,
        Some(&snapshot),
        None,
        Some(generation),
    );
}

/// Emits `verification:pending_confirmation` when a plan needs user confirmation before verification.
pub fn emit_verification_pending_confirmation(
    event_sink: &dyn EventSink,
    session_id: &str,
    session_title: &str,
    plan_artifact_id: &str,
) {
    event_sink.emit(
        "verification:pending_confirmation",
        serde_json::json!({
            "session_id": session_id,
            "session_title": session_title,
            "plan_artifact_id": plan_artifact_id,
        }),
    );
}
