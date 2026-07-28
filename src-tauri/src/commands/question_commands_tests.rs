use super::question_commands::{
    accepted_plan_mode_proposal, build_plan_mode_proposal_continuation,
    declined_plan_mode_proposal, plan_mode_proposal_continuation_metadata,
    plan_mode_proposal_continuation_metadata_with_outcome, plan_mode_proposal_source_ref_id,
    task_outcome_from_plan_mode_verdict, PLAN_MODE_PROPOSAL_ACCEPT_VALUE,
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

fn plan_mode_verdict_outcome(outcome_class: &str) -> PlanModeVerdictOutcome {
    PlanModeVerdictOutcome {
        project_id: "project-1".to_string(),
        source: "plan_mode".to_string(),
        outcome_class: outcome_class.to_string(),
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
fn declined_plan_mode_proposal_extracts_conversation_and_user_reason() {
    let question = pending_question(serde_json::json!({
        "kind": PLAN_MODE_PROPOSAL_KIND,
        "conversation_id": "11111111-1111-1111-1111-111111111111"
    }));
    let mut declined_answer = answer(vec!["keep_edit"], false);
    declined_answer.text = Some("Stay in edit mode".to_string());

    let declined = declined_plan_mode_proposal(Some(&question), &declined_answer)
        .expect("non-accepting answer should be a decline");
    assert_eq!(
        declined.conversation_id.as_str(),
        "11111111-1111-1111-1111-111111111111"
    );
    assert_eq!(declined.reason.as_deref(), Some("Stay in edit mode"));
}

#[test]
fn declined_plan_mode_proposal_rejects_non_decline_cases() {
    let question = pending_question(serde_json::json!({
        "kind": PLAN_MODE_PROPOSAL_KIND,
        "conversation_id": "11111111-1111-1111-1111-111111111111"
    }));

    assert!(
        declined_plan_mode_proposal(Some(&question), &answer(vec!["keep_edit"], true)).is_none(),
        "a skipped answer records no verdict"
    );
    assert!(
        declined_plan_mode_proposal(
            Some(&question),
            &answer(vec![PLAN_MODE_PROPOSAL_ACCEPT_VALUE], false)
        )
        .is_none(),
        "acceptance is not a decline"
    );
    assert!(declined_plan_mode_proposal(None, &answer(vec!["keep_edit"], false)).is_none());

    let wrong_kind = pending_question(serde_json::json!({
        "kind": "other",
        "conversation_id": "11111111-1111-1111-1111-111111111111"
    }));
    assert!(
        declined_plan_mode_proposal(Some(&wrong_kind), &answer(vec!["keep_edit"], false)).is_none()
    );

    let mut without_metadata = pending_question(serde_json::json!({}));
    without_metadata.metadata = None;
    assert!(declined_plan_mode_proposal(
        Some(&without_metadata),
        &answer(vec!["keep_edit"], false)
    )
    .is_none());

    let empty_conversation = pending_question(serde_json::json!({
        "kind": PLAN_MODE_PROPOSAL_KIND,
        "conversation_id": " "
    }));
    assert!(declined_plan_mode_proposal(
        Some(&empty_conversation),
        &answer(vec!["keep_edit"], false)
    )
    .is_none());
}

#[test]
fn accept_and_decline_detectors_are_mutually_exclusive() {
    let question = pending_question(serde_json::json!({
        "kind": PLAN_MODE_PROPOSAL_KIND,
        "conversation_id": "11111111-1111-1111-1111-111111111111"
    }));
    let answers = [
        answer(vec![PLAN_MODE_PROPOSAL_ACCEPT_VALUE], false),
        answer(vec!["keep_edit"], false),
        answer(vec![], false),
        answer(vec![PLAN_MODE_PROPOSAL_ACCEPT_VALUE], true),
        answer(vec!["keep_edit"], true),
    ];

    for candidate in answers {
        let accepted = accepted_plan_mode_proposal(Some(&question), &candidate).is_some();
        let declined = declined_plan_mode_proposal(Some(&question), &candidate).is_some();
        assert!(
            !(accepted && declined),
            "an answer must not be both accepted and declined: {candidate:?}"
        );
    }
}

#[test]
fn plan_mode_verdict_rows_key_accept_and_decline_separately() {
    let accepted = plan_mode_verdict_outcome("plan_mode_accepted");
    let declined = plan_mode_verdict_outcome("plan_mode_declined");

    let accepted_row =
        task_outcome_from_plan_mode_verdict(&accepted).expect("accepted conversion should succeed");
    let declined_row =
        task_outcome_from_plan_mode_verdict(&declined).expect("declined conversion should succeed");

    assert_ne!(
        accepted_row.source_ref_id, declined_row.source_ref_id,
        "accept and decline in one planning session must be distinct ledger rows"
    );
    assert_eq!(accepted_row.source_ref_kind, declined_row.source_ref_kind);

    let repeated =
        task_outcome_from_plan_mode_verdict(&plan_mode_verdict_outcome("plan_mode_declined"))
            .expect("repeat conversion should succeed");
    assert_eq!(
        declined_row.source_ref_id, repeated.source_ref_id,
        "repeat delivery of one verdict must stay idempotent"
    );
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

#[test]
fn plan_mode_verdict_outcome_converts_to_task_outcome_ledger_row() {
    let outcome = plan_mode_verdict_outcome("plan_mode_accepted");

    let task_outcome =
        task_outcome_from_plan_mode_verdict(&outcome).expect("task outcome conversion");

    assert_eq!(task_outcome.project_id.as_str(), "project-1");
    assert_eq!(task_outcome.source.as_str(), "plan_mode");
    assert_eq!(task_outcome.source_ref_kind, "planning_session");
    assert_eq!(
        task_outcome.source_ref_id,
        plan_mode_proposal_source_ref_id("planning-session-1", "plan_mode_accepted")
    );
    assert_eq!(
        task_outcome.source_ref_id, "s18:planning-session-1c18:plan_mode_accepted",
        "the key must stay length-prefixed so session ids cannot collide with the verdict"
    );
    assert_eq!(
        task_outcome
            .outcome_class
            .as_ref()
            .map(|class| class.as_str()),
        Some("plan_mode_accepted")
    );
    assert_eq!(task_outcome.status.to_string(), "eligible");
    assert_eq!(
        task_outcome.conversation_id.as_deref(),
        Some("conversation-plan-1")
    );
    assert_eq!(
        task_outcome.evidence_json["evidence_summary"].as_str(),
        Some("Plan first.")
    );
}
