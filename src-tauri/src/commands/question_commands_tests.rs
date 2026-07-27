use super::question_commands::{
    accepted_plan_mode_proposal, build_plan_mode_proposal_continuation,
    declined_plan_mode_proposal, plan_mode_proposal_continuation_metadata,
    plan_mode_proposal_continuation_metadata_with_outcome, PLAN_MODE_PROPOSAL_ACCEPT_VALUE,
    PLAN_MODE_PROPOSAL_CONTINUATION_BASE, PLAN_MODE_PROPOSAL_KIND,
};
use crate::application::{PendingQuestionInfo, QuestionAnswer, QuestionOption};
use crate::domain::services::learned_skill_adapters::PlanModeVerdictOutcome;
use std::collections::BTreeMap;

fn answer(selected_options: Vec<&str>, skipped: bool) -> QuestionAnswer {
    QuestionAnswer {
        selected_options: selected_options.into_iter().map(str::to_string).collect(),
        text: None,
        skipped,
    }
}

fn pending_question(metadata: serde_json::Value) -> PendingQuestionInfo {
    PendingQuestionInfo {
        request_id: "question-1".to_string(),
        session_id: "22222222-2222-2222-2222-222222222222".to_string(),
        question: "Switch to plan mode?".to_string(),
        header: Some("Plan mode".to_string()),
        options: vec![QuestionOption {
            value: PLAN_MODE_PROPOSAL_ACCEPT_VALUE.to_string(),
            label: "Switch".to_string(),
            description: None,
        }],
        multi_select: false,
        allow_skip: true,
        batch_index: None,
        batch_total: None,
        metadata: Some(metadata),
        created_at: "2026-07-10T00:00:00+00:00".to_string(),
    }
}

#[test]
fn accepted_plan_mode_proposal_extracts_conversation_and_reason() {
    let question = pending_question(serde_json::json!({
        "kind": PLAN_MODE_PROPOSAL_KIND,
        "conversation_id": "11111111-1111-1111-1111-111111111111",
        "reason": "  tighten scope first  "
    }));

    let proposal = accepted_plan_mode_proposal(
        Some(&question),
        &answer(vec![PLAN_MODE_PROPOSAL_ACCEPT_VALUE], false),
    )
    .expect("accepted proposal");

    assert_eq!(
        proposal.conversation_id.as_str(),
        "11111111-1111-1111-1111-111111111111"
    );
    assert_eq!(proposal.reason.as_deref(), Some("tighten scope first"));
}

#[test]
fn accepted_plan_mode_proposal_falls_back_to_question_session_id() {
    let question = pending_question(serde_json::json!({
        "kind": PLAN_MODE_PROPOSAL_KIND,
        "reason": " "
    }));

    let proposal = accepted_plan_mode_proposal(
        Some(&question),
        &answer(vec![PLAN_MODE_PROPOSAL_ACCEPT_VALUE], false),
    )
    .expect("accepted proposal");

    assert_eq!(
        proposal.conversation_id.as_str(),
        "22222222-2222-2222-2222-222222222222"
    );
    assert_eq!(proposal.reason, None);
}

#[test]
fn accepted_plan_mode_proposal_rejects_non_acceptance_cases() {
    let question = pending_question(serde_json::json!({
        "kind": PLAN_MODE_PROPOSAL_KIND,
        "conversation_id": "11111111-1111-1111-1111-111111111111"
    }));
    assert!(accepted_plan_mode_proposal(
        Some(&question),
        &answer(vec![PLAN_MODE_PROPOSAL_ACCEPT_VALUE], true),
    )
    .is_none());
    assert!(
        accepted_plan_mode_proposal(Some(&question), &answer(vec!["keep_edit"], false)).is_none()
    );
    assert!(accepted_plan_mode_proposal(
        None,
        &answer(vec![PLAN_MODE_PROPOSAL_ACCEPT_VALUE], false)
    )
    .is_none());

    let wrong_kind = pending_question(serde_json::json!({
        "kind": "other",
        "conversation_id": "11111111-1111-1111-1111-111111111111"
    }));
    assert!(accepted_plan_mode_proposal(
        Some(&wrong_kind),
        &answer(vec![PLAN_MODE_PROPOSAL_ACCEPT_VALUE], false),
    )
    .is_none());

    let empty_conversation = pending_question(serde_json::json!({
        "kind": PLAN_MODE_PROPOSAL_KIND,
        "conversation_id": " "
    }));
    assert!(accepted_plan_mode_proposal(
        Some(&empty_conversation),
        &answer(vec![PLAN_MODE_PROPOSAL_ACCEPT_VALUE], false),
    )
    .is_none());
}

#[test]
fn declined_plan_mode_proposal_accepts_only_an_explicit_non_skipped_decline() {
    let question = pending_question(serde_json::json!({
        "kind": PLAN_MODE_PROPOSAL_KIND,
        "conversation_id": "11111111-1111-1111-1111-111111111111"
    }));
    let mut decline = answer(vec!["keep_edit"], false);
    decline.text = Some("Keep this change narrowly scoped.".to_string());

    let proposal = declined_plan_mode_proposal(Some(&question), &decline)
        .expect("explicit decline should be captured");
    assert_eq!(
        proposal.conversation_id.as_str(),
        "11111111-1111-1111-1111-111111111111"
    );
    assert_eq!(
        proposal.reason.as_deref(),
        Some("Keep this change narrowly scoped.")
    );
    assert!(
        declined_plan_mode_proposal(Some(&question), &answer(vec!["keep_edit"], true)).is_none()
    );
    assert!(declined_plan_mode_proposal(
        Some(&question),
        &answer(vec![PLAN_MODE_PROPOSAL_ACCEPT_VALUE], false)
    )
    .is_none());
}

#[test]
fn continuation_message_and_metadata_are_hidden_resume_payloads() {
    assert_eq!(
        build_plan_mode_proposal_continuation(None),
        PLAN_MODE_PROPOSAL_CONTINUATION_BASE
    );
    assert_eq!(
        build_plan_mode_proposal_continuation(Some("  check scope  ")),
        format!("{PLAN_MODE_PROPOSAL_CONTINUATION_BASE}\n\nPlanning focus: check scope")
    );

    let metadata: serde_json::Value =
        serde_json::from_str(&plan_mode_proposal_continuation_metadata("request-123"))
            .expect("metadata json");
    assert_eq!(
        metadata.get("source").and_then(|value| value.as_str()),
        Some("accepted_plan_mode_proposal")
    );
    assert_eq!(
        metadata
            .get("source_request_id")
            .and_then(|value| value.as_str()),
        Some("request-123")
    );
    assert_eq!(
        metadata
            .get("required_workspace_mode")
            .and_then(|value| value.as_str()),
        Some("plan")
    );
    assert_eq!(
        metadata
            .get("resume_in_place")
            .and_then(|value| value.as_bool()),
        Some(true)
    );
    assert_eq!(
        metadata
            .get("persist_hidden_marker")
            .and_then(|value| value.as_bool()),
        Some(true)
    );
}

#[test]
fn continuation_metadata_can_carry_compact_plan_mode_verdict_outcome() {
    let outcome = PlanModeVerdictOutcome {
        project_id: "project-1".to_string(),
        source: "plan_mode".to_string(),
        outcome_class: "plan_mode_accepted".to_string(),
        status: "eligible".to_string(),
        refs: BTreeMap::from([
            (
                "conversation_id".to_string(),
                "conversation-plan-1".to_string(),
            ),
            (
                "planning_session_id".to_string(),
                "planning-session-1".to_string(),
            ),
        ]),
        evidence_summary: "Plan first.".to_string(),
        mutates_accepted_session: false,
    };

    let metadata: serde_json::Value = serde_json::from_str(
        &plan_mode_proposal_continuation_metadata_with_outcome("req-plan", Some(&outcome)),
    )
    .expect("metadata json");
    let captured = metadata
        .get("plan_mode_verdict_outcome")
        .expect("metadata should include captured outcome");

    assert_eq!(
        captured
            .get("outcome_class")
            .and_then(|value| value.as_str()),
        Some("plan_mode_accepted")
    );
    assert_eq!(
        captured
            .get("refs")
            .and_then(|value| value.get("planning_session_id"))
            .and_then(|value| value.as_str()),
        Some("planning-session-1")
    );
    assert_eq!(
        captured
            .get("mutates_accepted_session")
            .and_then(|value| value.as_bool()),
        Some(false)
    );
}
