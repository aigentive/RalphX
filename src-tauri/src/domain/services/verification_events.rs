// Shared payload builders for plan verification lifecycle events.
//
// Application-layer emission helpers use these builders to prevent payload drift
// across reconciliation, recovery, revert-and-skip, artifact reset, and the main
// post_verification_status handler.

use crate::domain::entities::{VerificationRunSnapshot, VerificationStatus};
use crate::domain::services::gap_score;

/// Build the canonical native snapshot for a freshly started verification run.
///
/// This keeps verification-start event payloads consistent across all entry points
/// (manual verify, external trigger, auto-verify on plan creation, re-verify).
pub fn build_verification_started_snapshot(
    generation: i32,
    max_rounds: u32,
) -> VerificationRunSnapshot {
    VerificationRunSnapshot {
        generation,
        status: VerificationStatus::Reviewing,
        in_progress: true,
        current_round: 0,
        max_rounds,
        best_round_index: None,
        convergence_reason: None,
        current_gaps: Vec::new(),
        rounds: Vec::new(),
    }
}

/// Build the canonical JSON payload.
pub fn build_verification_payload(
    session_id: &str,
    status: VerificationStatus,
    in_progress: bool,
    snapshot: Option<&VerificationRunSnapshot>,
    convergence_reason: Option<&str>,
    generation: Option<i32>,
) -> serde_json::Value {
    if let Some(snapshot) = snapshot {
        let weighted_gap_score = gap_score(&snapshot.current_gaps);

        let reason = convergence_reason.or(snapshot.convergence_reason.as_deref());

        serde_json::json!({
            "session_id": session_id,
            "status": status.to_string(),
            "in_progress": in_progress,
            "generation": generation,
            "round": if snapshot.current_round > 0 { serde_json::Value::from(snapshot.current_round) } else { serde_json::Value::Null },
            "max_rounds": if snapshot.max_rounds > 0 { serde_json::Value::from(snapshot.max_rounds) } else { serde_json::Value::Null },
            "gap_score": weighted_gap_score,
            "convergence_reason": reason,
            "current_gaps": snapshot.current_gaps,
            "rounds": snapshot.rounds,
        })
    } else {
        serde_json::json!({
            "session_id": session_id,
            "status": status.to_string(),
            "in_progress": in_progress,
            "generation": generation,
            "round": serde_json::Value::Null,
            "max_rounds": serde_json::Value::Null,
            "gap_score": serde_json::Value::Null,
            "convergence_reason": convergence_reason,
            "current_gaps": serde_json::Value::Array(vec![]),
            "rounds": serde_json::Value::Array(vec![]),
        })
    }
}

#[cfg(test)]
#[path = "verification_events_tests.rs"]
mod tests;
