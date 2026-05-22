//! Verification result handoff for parent ideation sessions.
//!
//! We persist a structured system message for the UI and only enqueue the legacy
//! `<verification-result>` XML handoff when the outcome is actionable for the parent
//! ideation agent. Infra/runtime failures should inform the user without nudging the
//! parent agent into a bogus plan-revision loop.

use std::sync::Arc;

use serde_json::json;
use tracing::warn;

use crate::domain::entities::{
    ChatContextType, ChatConversation, ChatConversationId, ChatMessage, IdeationSessionId,
    VerificationGap, VerificationRunSnapshot, VerificationStatus,
};
use crate::domain::repositories::{ChatConversationRepository, ChatMessageRepository};
use crate::domain::services::MessageQueue;

/// Dedup guard: skip synthesis when ralphx-plan-verifier already delivered a structured
/// `<escalation type="verification">` message via the same parent session.
pub(crate) const ESCALATED_TO_PARENT: &str = "escalated_to_parent";

/// XML tag marker used to detect an already-injected verification-result message.
/// Used for legacy content-based dedup and agent-facing queued handoff payloads.
pub(crate) const VERIFICATION_RESULT_MARKER: &str = "<verification-result>";
pub(crate) const VERIFICATION_RESULT_METADATA_KEY: &str = "verification_result";

/// Result returned by `reconcile_verification_on_child_complete`.
///
/// `None` is returned for early exits (parent not found, already resolved, etc.).
/// `Some` is returned when reconciliation ran and determined a terminal status.
pub struct ReconcileChildCompleteResult {
    pub terminal_status: VerificationStatus,
    pub parsed_snapshot: Option<VerificationRunSnapshot>,
}

/// Inject a `<verification-result>` XML message into the parent ideation session when:
/// - `result.terminal_status == NeedsRevision`, AND
/// - `convergence_reason != "escalated_to_parent"` (dedup guard)
///
/// Fire-and-forget: logs errors but never blocks the caller.
pub async fn maybe_inject_verification_result_message(
    parent_id: &IdeationSessionId,
    result: &ReconcileChildCompleteResult,
    conversation_repo: &Arc<dyn ChatConversationRepository>,
    chat_message_repo: &Arc<dyn ChatMessageRepository>,
    message_queue: &Arc<MessageQueue>,
) {
    // Only synthesize for NeedsRevision terminal status
    if result.terminal_status != VerificationStatus::NeedsRevision {
        return;
    }

    // Dedup guard: skip when escalation was already delivered to the parent
    let convergence_reason = result
        .parsed_snapshot
        .as_ref()
        .and_then(|snapshot| snapshot.convergence_reason.as_deref());
    if convergence_reason == Some(ESCALATED_TO_PARENT) {
        return;
    }

    let current_round = result
        .parsed_snapshot
        .as_ref()
        .map(|snapshot| snapshot.current_round)
        .unwrap_or(0);
    let max_rounds = result
        .parsed_snapshot
        .as_ref()
        .map(|snapshot| snapshot.max_rounds)
        .unwrap_or(0);
    let gaps = result
        .parsed_snapshot
        .as_ref()
        .map(|snapshot| snapshot.current_gaps.as_slice())
        .unwrap_or(&[]);
    let summary = summarize_gaps(gaps);
    let blockers = top_3_blockers(gaps);
    let recommended_action = derive_recommended_action(convergence_reason);
    let actionable_for_parent = is_actionable_for_parent_agent(convergence_reason);

    // Build legacy XML payload for the agent-facing queue only.
    let payload = format_verification_result_xml(
        parent_id.as_str(),
        convergence_reason,
        current_round,
        max_rounds,
        gaps,
    );
    let metadata = build_verification_result_metadata(
        convergence_reason,
        current_round,
        max_rounds,
        &summary,
        &blockers,
        recommended_action,
        actionable_for_parent,
    );
    let content = format_verification_result_summary(
        &summary,
        &blockers,
        recommended_action,
        actionable_for_parent,
    );

    // Find or create the active conversation for the parent session
    let conversation_id = match conversation_repo
        .get_active_for_context(ChatContextType::Ideation, parent_id.as_str())
        .await
    {
        Ok(Some(conv)) => Some(conv.id),
        Ok(None) => {
            let new_conv = ChatConversation::new_ideation(parent_id.clone());
            match conversation_repo.create(new_conv).await {
                Ok(created) => Some(created.id),
                Err(e) => {
                    warn!(
                        parent_id = %parent_id.as_str(),
                        error = %e,
                        "Failed to create conversation for verification-result injection"
                    );
                    None
                }
            }
        }
        Err(e) => {
            warn!(
                parent_id = %parent_id.as_str(),
                error = %e,
                "Failed to look up conversation for verification-result injection"
            );
            None
        }
    };

    // Build and persist the user-facing system message
    let mut message = ChatMessage::system_in_session(parent_id.clone(), content)
        .with_metadata(metadata);
    message.conversation_id = conversation_id;

    if let Err(e) = chat_message_repo.create(message).await {
        warn!(
            parent_id = %parent_id.as_str(),
            error = %e,
            "Failed to store verification-result message — continuing"
        );
    }

    // Only actionable verification outcomes should wake the parent ideation agent.
    // Infra/runtime failures still persist a user-facing card but do not trigger
    // a bogus self-revision loop in the parent session.
    if actionable_for_parent {
        message_queue.queue(ChatContextType::Ideation, parent_id.as_str(), payload);
    }
}

/// Inject a `<verification-result>` handoff into the parent conversation if it hasn't
/// been injected already.
///
/// Called on the timeout path (Gate C) when a verification child's idle process hits the
/// 600s no-output timeout after the parent has already reached a terminal verification state.
///
/// Dedup logic:
/// 1. `exists_verification_result_in_conversation` — skips if already injected (conversation-level check)
/// 2. `ESCALATED_TO_PARENT` convergence reason — skips if verifier escalated directly
///
/// Only injects for `NeedsRevision` terminal status (same filter as `maybe_inject_verification_result_message`).
/// For `Verified` / `Skipped`, calls the inner fn which will return immediately after the status check.
///
/// Fire-and-forget: logs errors but never blocks the caller.
pub(crate) async fn inject_verification_handoff_if_missing(
    parent_id: &IdeationSessionId,
    parent_conversation_id: &ChatConversationId,
    terminal_status: VerificationStatus,
    current_gaps: &[VerificationGap],
    convergence_reason: Option<&str>,
    conversation_repo: &Arc<dyn ChatConversationRepository>,
    chat_message_repo: &Arc<dyn ChatMessageRepository>,
    message_queue: &Arc<MessageQueue>,
) {
    // Conversation-level dedup guard: skip if already injected
    match chat_message_repo
        .exists_verification_result_in_conversation(parent_conversation_id)
        .await
    {
        Ok(true) => {
            tracing::debug!(
                parent_id = %parent_id.as_str(),
                "Gate C: verification-result already present in parent conversation — skipping"
            );
            return;
        }
        Ok(false) => {}
        Err(e) => {
            warn!(
                parent_id = %parent_id.as_str(),
                error = %e,
                "Gate C: failed to check for existing verification-result — skipping injection"
            );
            return;
        }
    }

    // Build a ReconcileChildCompleteResult from the pre-fetched state and delegate
    // to maybe_inject_verification_result_message. It handles the ESCALATED_TO_PARENT
    // dedup guard and the NeedsRevision filter internally.
    let parsed_snapshot = Some(VerificationRunSnapshot {
        generation: 0,
        status: terminal_status,
        in_progress: false,
        current_round: 0,
        max_rounds: 0,
        current_gaps: current_gaps.to_vec(),
        rounds: vec![],
        convergence_reason: convergence_reason.map(str::to_string),
        best_round_index: None,
    });

    let result = ReconcileChildCompleteResult {
        terminal_status,
        parsed_snapshot,
    };

    maybe_inject_verification_result_message(
        parent_id,
        &result,
        conversation_repo,
        chat_message_repo,
        message_queue,
    )
    .await;
}

/// Build the `<verification-result>` XML payload.
pub fn format_verification_result_xml(
    child_session_id: &str,
    convergence_reason: Option<&str>,
    current_round: u32,
    max_rounds: u32,
    gaps: &[VerificationGap],
) -> String {
    let reason = convergence_reason.unwrap_or("unknown");
    let summary = summarize_gaps(gaps);
    let blockers = top_3_blockers(gaps);
    let action = derive_recommended_action(convergence_reason);

    let blockers_section = if blockers.is_empty() {
        String::new()
    } else {
        let blocker_lines: String = blockers
            .iter()
            .map(|(severity, desc)| {
                format!("    <blocker severity=\"{severity}\">{desc}</blocker>")
            })
            .collect::<Vec<_>>()
            .join("\n");
        format!("  <top_blockers>\n{blocker_lines}\n  </top_blockers>\n")
    };

    format!(
        "<verification-result>\n\
         <child_session_id>{child_session_id}</child_session_id>\n\
         <status>needs_revision</status>\n\
         <convergence_reason>{reason}</convergence_reason>\n\
         <round>{current_round}</round>\n\
         <max_rounds>{max_rounds}</max_rounds>\n\
         <summary>{summary}</summary>\n\
         {blockers_section}\
         <recommended_next_action>{action}</recommended_next_action>\n\
         </verification-result>"
    )
}

fn build_verification_result_metadata(
    convergence_reason: Option<&str>,
    current_round: u32,
    max_rounds: u32,
    summary: &str,
    blockers: &[(String, String)],
    recommended_action: &str,
    actionable_for_parent: bool,
) -> String {
    let blockers_json: Vec<_> = blockers
        .iter()
        .map(|(severity, description)| {
            json!({
                "severity": severity,
                "description": description,
            })
        })
        .collect();

    json!({
        VERIFICATION_RESULT_METADATA_KEY: true,
        "status": "needs_revision",
        "convergence_reason": convergence_reason,
        "current_round": current_round,
        "max_rounds": max_rounds,
        "summary": summary,
        "top_blockers": blockers_json,
        "recommended_next_action": recommended_action,
        "actionable_for_parent": actionable_for_parent,
    })
    .to_string()
}

fn format_verification_result_summary(
    summary: &str,
    blockers: &[(String, String)],
    recommended_action: &str,
    actionable_for_parent: bool,
) -> String {
    let lead = if actionable_for_parent {
        "Verification found plan blockers."
    } else {
        "Verification hit an infrastructure/runtime blocker."
    };

    let blocker_line = blockers.first().map_or_else(
        || "No blocker details were published.".to_string(),
        |(severity, description)| format!("Top blocker ({severity}): {description}"),
    );

    format!(
        "{lead}\n\nSummary: {summary}\n{blocker_line}\nRecommended next action: {recommended_action}"
    )
}

/// Summarize gap severity distribution as a human-readable sentence.
pub(crate) fn summarize_gaps(gaps: &[VerificationGap]) -> String {
    if gaps.is_empty() {
        return "Agent completed without producing gap analysis (possible mid-round crash)."
            .to_string();
    }

    let mut critical = 0u32;
    let mut high = 0u32;
    let mut medium = 0u32;
    let mut low = 0u32;

    for gap in gaps {
        match gap.severity.as_str() {
            "critical" => critical += 1,
            "high" => high += 1,
            "medium" => medium += 1,
            "low" => low += 1,
            _ => {}
        }
    }

    let total = gaps.len();
    let mut parts = Vec::new();
    if critical > 0 {
        parts.push(format!("{critical} critical"));
    }
    if high > 0 {
        parts.push(format!("{high} high"));
    }
    if medium > 0 {
        parts.push(format!("{medium} medium"));
    }
    if low > 0 {
        parts.push(format!("{low} low"));
    }

    if parts.is_empty() {
        format!("{total} unclassified gap(s) remain unresolved.")
    } else {
        format!("{total} gap(s) remain: {}.", parts.join(", "))
    }
}

/// Return up to 3 top blockers (highest severity first), descriptions capped at 200 chars.
pub(crate) fn top_3_blockers(gaps: &[VerificationGap]) -> Vec<(String, String)> {
    fn severity_rank(s: &str) -> u8 {
        match s {
            "critical" => 0,
            "high" => 1,
            "medium" => 2,
            "low" => 3,
            _ => 4,
        }
    }

    let mut sorted: Vec<&VerificationGap> = gaps.iter().collect();
    sorted.sort_by_key(|g| severity_rank(g.severity.as_str()));

    sorted
        .iter()
        .take(3)
        .map(|g| {
            let desc = if g.description.len() > 200 {
                // Truncate to 199 chars + ellipsis (byte-safe via char boundary)
                let truncated = g
                    .description
                    .char_indices()
                    .take_while(|(i, _)| *i < 199)
                    .last()
                    .map(|(i, c)| i + c.len_utf8())
                    .map(|end| &g.description[..end])
                    .unwrap_or("");
                format!("{truncated}…")
            } else {
                g.description.clone()
            };
            (g.severity.clone(), desc)
        })
        .collect()
}

/// Deterministically map convergence_reason to a recommended_next_action enum value.
///
/// | Reason | Action |
/// |--------|--------|
/// | `max_rounds` | `revise_plan` — rounds exhausted; plan needs a revision pass |
/// | `jaccard_converged` | `revise_plan` — gaps stable; plan needs revision |
/// | `gap_score_plateau` | `explore_code_paths` — plateau suggests missing context |
/// | `agent_crashed_mid_round` | `rerun_verification` — transient failure; retry first |
/// | `agent_completed_without_update` | `rerun_verification` — no output; retry first |
/// | anything else / unknown | `rerun_verification` — safe default |
pub(crate) fn derive_recommended_action(convergence_reason: Option<&str>) -> &'static str {
    match convergence_reason {
        Some("max_rounds") => "revise_plan",
        Some("jaccard_converged") => "revise_plan",
        Some("gap_score_plateau") => "explore_code_paths",
        Some("agent_crashed_mid_round")
        | Some("agent_completed_without_update")
        | Some("agent_error")
        | Some("critic_parse_failure")
        | Some("user_stopped")
        | Some("user_skipped")
        | Some("user_reverted") => {
            "rerun_verification"
        }
        _ => "rerun_verification",
    }
}

pub(crate) fn is_actionable_for_parent_agent(convergence_reason: Option<&str>) -> bool {
    matches!(
        convergence_reason,
        Some("max_rounds") | Some("jaccard_converged") | Some("gap_score_plateau")
    )
}

#[cfg(test)]
#[path = "verification_handoff_tests.rs"]
mod tests;
