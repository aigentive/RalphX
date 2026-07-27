use chrono::Utc;
use serde_json::json;

use super::judge::AutomationJudgeAttachmentContext;
use super::plan_judge::{
    append_automation_plan_judge_retry_instruction, build_automation_plan_judge_prompt,
    parse_automation_plan_judge_verdict, AutomationPlanJudgeDecision,
    AutomationPlanJudgeValidationContext, AutomationPlanVerificationGapSummary,
    AutomationPlanVerificationJudgeContext, BuildAutomationPlanJudgePromptInput,
    AUTOMATION_PLAN_JUDGE_PROMPT_MAX_BYTES,
};
use crate::domain::entities::{
    Automation, AutomationId, AutomationJudgeState, AutomationPlanApprovalMode,
    AutomationPlanJudgeState, AutomationPrMergeMode, AutomationPromptAuthor, AutomationRun,
    AutomationRunId, AutomationRunStatus, AutomationStatus, ProjectId,
};
use crate::error::AppError;

fn automation_with_goal_items() -> Automation {
    let now = Utc::now();
    Automation {
        id: AutomationId::from_string("automation-1"),
        project_id: ProjectId::from_string("project-1".to_string()),
        name: "Plan Gate Automation".to_string(),
        status: AutomationStatus::Active,
        paused_reason_code: None,
        paused_reason_detail: None,
        goal_prompt: "Ship the automation plan gate one PR slice at a time.".to_string(),
        setup_conversation_id: None,
        provider_harness: "claude".to_string(),
        model_id: "sonnet".to_string(),
        logical_effort: Some("medium".to_string()),
        run_mode: "edit".to_string(),
        base_ref_kind: "local_branch".to_string(),
        base_ref: "main".to_string(),
        base_display_name: Some("main".to_string()),
        base_source_pull_request_json: None,
        goal_items_json: Some(
            json!([
                { "id": "item-1", "title": "Manual gate", "status": "done" },
                { "id": "item-2", "title": "Automatic judge", "status": "in_progress" },
                { "id": "item-3", "title": "Auto merge", "status": "pending" }
            ])
            .to_string(),
        ),
        chain_mode: "merged_base".to_string(),
        completion_signal: "pr_merged".to_string(),
        plan_approval_mode: AutomationPlanApprovalMode::Automatic,
        pr_merge_mode: AutomationPrMergeMode::Manual,
        plan_deep_verification: false,
        max_runs: 25,
        max_consecutive_failures: 3,
        first_run_prompt: Some("Initial prompt".to_string()),
        setup_analysis_summary: Some("Setup analysis".to_string()),
        spec_artifact_id: Some("spec-artifact-1".to_string()),
        authoring_state_json: None,
        created_at: now,
        updated_at: now,
    }
}

fn automation_run() -> AutomationRun {
    let now = Utc::now();
    AutomationRun {
        id: AutomationRunId::from_string("run-1"),
        automation_id: AutomationId::from_string("automation-1"),
        run_index: 1,
        status: AutomationRunStatus::AwaitingPlanApproval,
        judge_state: AutomationJudgeState::None,
        judge_lease_expires_at: None,
        plan_judge_state: AutomationPlanJudgeState::None,
        plan_judge_lease_expires_at: None,
        plan_judge_verdict_json: None,
        plan_revision_round: 2,
        plan_reminder_count: 0,
        plan_pending_instructions: None,
        plan_last_parked_artifact_id: Some("plan-artifact-1".to_string()),
        agent_phase_started_at: None,
        conversation_id: None,
        run_prompt: "Author the automatic plan judge and keep it scoped to PR 5.".to_string(),
        prompt_author: AutomationPromptAuthor::SetupAgent,
        base_ref_kind: "local_branch".to_string(),
        base_ref_used: "main".to_string(),
        base_from_run_id: None,
        goal_item_id: None,
        branch_name: Some("ralphx/automation-run-1".to_string()),
        pr_number: None,
        pr_url: None,
        pr_title: None,
        pr_head_ref_name: None,
        pr_base_ref_name: None,
        pr_merged_at: None,
        merge_commit_sha: None,
        diff_stats_json: None,
        agent_summary: None,
        judge_verdict_json: None,
        judge_model_id: None,
        error_code: None,
        error_detail: None,
        signal_check_failures: 0,
        started_at: Some(now),
        finished_at: None,
        created_at: now,
        updated_at: now,
    }
}

fn spec_attachment() -> Vec<AutomationJudgeAttachmentContext> {
    vec![AutomationJudgeAttachmentContext {
        file_name: "spec.md".to_string(),
        mime_type: Some("text/markdown".to_string()),
        file_size: Some(23),
        text_content: Some("Spec context is advisory.".to_string()),
    }]
}

#[test]
fn plan_judge_prompt_includes_budgeted_sections_current_phase_and_artifact_pin() {
    let automation = automation_with_goal_items();
    let run = automation_run();
    let previous = json!({
        "decision": "revise",
        "reason": "The plan missed artifact pinning.",
        "confidence": "medium",
        "revisionInstructions": "Add artifact id pinning and crash recovery details before implementation proceeds.",
        "evaluatedArtifactId": "plan-artifact-0"
    })
    .to_string();
    let oversized_plan = format!(
        "Plan intro.\n{}\nTail that should be truncated away.",
        "inspect scheduler and plan gate ".repeat(10_000)
    );

    let spec_attachments = spec_attachment();
    let prompt = build_automation_plan_judge_prompt(BuildAutomationPlanJudgePromptInput {
        automation: &automation,
        run: &run,
        evaluated_artifact_id: "plan-artifact-1",
        plan_content: &oversized_plan,
        verification_context: None,
        spec_attachments: &spec_attachments,
        previous_verdict_json: Some(&previous),
    })
    .unwrap();

    assert!(prompt.len() <= AUTOMATION_PLAN_JUDGE_PROMPT_MAX_BYTES);
    assert!(prompt.contains("<goal "));
    assert!(prompt.contains("<goal_items "));
    assert!(prompt.contains("\"currentPhase\""));
    assert!(prompt.contains("\"id\": \"item-2\""));
    assert!(prompt.contains("<spec "));
    assert!(prompt.contains("Spec context is advisory."));
    assert!(prompt.contains("<run_prompt "));
    assert!(prompt.contains("Author the automatic plan judge"));
    assert!(prompt.contains("<plan artifact_id=\"plan-artifact-1\" truncated=\"true\">"));
    assert!(!prompt.contains("<verification "));
    assert!(prompt.contains("<previous_verdict "));
    assert!(prompt.contains("\"planRevisionRound\": 2"));
    assert!(prompt.contains("artifact pinning"));
    assert!(prompt.contains("<output_contract "));
}

#[test]
fn plan_judge_prompt_uses_current_goal_item_without_id() {
    let mut automation = automation_with_goal_items();
    automation.goal_items_json = Some(
        json!([
            { "title": "No id but current", "status": "pending" },
            { "id": "item-2", "title": "Later", "status": "pending" }
        ])
        .to_string(),
    );
    let run = automation_run();

    let prompt = build_automation_plan_judge_prompt(BuildAutomationPlanJudgePromptInput {
        automation: &automation,
        run: &run,
        evaluated_artifact_id: "plan-artifact-1",
        plan_content: "Plan body.",
        verification_context: None,
        spec_attachments: &[],
        previous_verdict_json: None,
    })
    .unwrap();

    assert!(prompt.contains("\"currentPhase\""));
    assert!(prompt.contains("\"title\": \"No id but current\""));
}

#[test]
fn plan_judge_retry_instruction_respects_prompt_budget() {
    let automation = automation_with_goal_items();
    let run = automation_run();
    let spec_attachments = spec_attachment();
    let mut prompt = build_automation_plan_judge_prompt(BuildAutomationPlanJudgePromptInput {
        automation: &automation,
        run: &run,
        evaluated_artifact_id: "plan-artifact-1",
        plan_content: "Plan body.",
        verification_context: None,
        spec_attachments: &spec_attachments,
        previous_verdict_json: None,
    })
    .unwrap();

    assert!(append_automation_plan_judge_retry_instruction(&mut prompt));
    assert!(prompt.contains("Previous plan judge output was invalid"));

    let mut full = "x".repeat(AUTOMATION_PLAN_JUDGE_PROMPT_MAX_BYTES);
    assert!(!append_automation_plan_judge_retry_instruction(&mut full));
}

#[test]
fn plan_judge_prompt_includes_budgeted_verification_context_when_present() {
    let automation = automation_with_goal_items();
    let run = automation_run();
    let verification = AutomationPlanVerificationJudgeContext {
        status: "needs_revision".to_string(),
        in_progress: false,
        generation: Some(7),
        current_round: Some(3),
        max_rounds: Some(5),
        convergence_reason: Some("max_rounds".to_string()),
        gap_count: Some(1),
        gap_score: Some(10),
        gaps: vec![AutomationPlanVerificationGapSummary {
            severity: "critical".to_string(),
            category: "state_machine".to_string(),
            description: "Plan misses the stale-cache falsification path.".to_string(),
            why_it_matters: Some("The judge could approve a false success.".to_string()),
            source: Some("implementation_feasibility".to_string()),
        }],
        unavailable_reason: None,
    };

    let prompt = build_automation_plan_judge_prompt(BuildAutomationPlanJudgePromptInput {
        automation: &automation,
        run: &run,
        evaluated_artifact_id: "plan-artifact-1",
        plan_content: "Plan body.",
        verification_context: Some(&verification),
        spec_attachments: &[],
        previous_verdict_json: None,
    })
    .unwrap();

    assert!(prompt.len() <= AUTOMATION_PLAN_JUDGE_PROMPT_MAX_BYTES);
    assert!(prompt.contains("<verification "));
    assert!(prompt.contains("\"status\": \"needs_revision\""));
    assert!(prompt.contains("\"convergenceReason\": \"max_rounds\""));
    assert!(prompt.contains("stale-cache falsification"));
    assert!(prompt.contains("advisory verification outcome"));
    assert!(prompt.contains("Verification gap findings inform the verdict"));
}

#[test]
fn parses_valid_approve_verdict() {
    let verdict = parse_automation_plan_judge_verdict(
        &json!({
            "decision": "approve",
            "reason": "The plan is scoped to the current automatic judge slice.",
            "confidence": "high",
            "evaluatedArtifactId": "plan-artifact-1"
        })
        .to_string(),
        AutomationPlanJudgeValidationContext {
            expected_artifact_id: Some("plan-artifact-1"),
            expected_artifact_version: Some(3),
        },
    )
    .unwrap();

    assert_eq!(verdict.decision, AutomationPlanJudgeDecision::Approve);
    assert!(verdict.revision_instructions.is_none());
    assert_eq!(verdict.evaluated_artifact_id, "plan-artifact-1");
    assert_eq!(verdict.evaluated_artifact_version, Some(3));
}

#[test]
fn stamps_backend_artifact_version_over_model_supplied_version() {
    let verdict = parse_automation_plan_judge_verdict(
        &json!({
            "decision": "approve",
            "reason": "The plan is scoped to the current automatic judge slice.",
            "confidence": "high",
            "evaluatedArtifactId": "plan-artifact-1",
            "evaluatedArtifactVersion": 99
        })
        .to_string(),
        AutomationPlanJudgeValidationContext {
            expected_artifact_id: Some("plan-artifact-1"),
            expected_artifact_version: Some(3),
        },
    )
    .unwrap();

    assert_eq!(verdict.evaluated_artifact_version, Some(3));
}

#[test]
fn preserves_stored_artifact_version_when_replaying_without_a_pin() {
    let verdict = parse_automation_plan_judge_verdict(
        &json!({
            "decision": "approve",
            "reason": "The plan is scoped to the current automatic judge slice.",
            "confidence": "high",
            "evaluatedArtifactId": "plan-artifact-1",
            "evaluatedArtifactVersion": 2
        })
        .to_string(),
        AutomationPlanJudgeValidationContext {
            expected_artifact_id: None,
            expected_artifact_version: None,
        },
    )
    .unwrap();

    assert_eq!(verdict.evaluated_artifact_version, Some(2));
}

#[test]
fn legacy_stored_verdict_without_version_replays_as_unversioned() {
    let verdict = parse_automation_plan_judge_verdict(
        &json!({
            "decision": "approve",
            "reason": "The plan is scoped to the current automatic judge slice.",
            "confidence": "high",
            "evaluatedArtifactId": "plan-artifact-1"
        })
        .to_string(),
        AutomationPlanJudgeValidationContext {
            expected_artifact_id: None,
            expected_artifact_version: None,
        },
    )
    .unwrap();

    assert_eq!(verdict.evaluated_artifact_version, None);
}

#[test]
fn parses_valid_revise_verdict() {
    let verdict = parse_automation_plan_judge_verdict(
        &json!({
            "decision": "revise",
            "reason": "The plan does not cover crash recovery.",
            "confidence": "medium",
            "revisionInstructions": "Add explicit crash recovery for a stored revise verdict after pending instructions were cleared.",
            "evaluatedArtifactId": "plan-artifact-1"
        })
        .to_string(),
        AutomationPlanJudgeValidationContext {
            expected_artifact_id: Some("plan-artifact-1"),
            expected_artifact_version: Some(3),
        },
    )
    .unwrap();

    assert_eq!(verdict.decision, AutomationPlanJudgeDecision::Revise);
    assert!(verdict
        .revision_instructions
        .as_deref()
        .unwrap()
        .contains("crash recovery"));
}

#[test]
fn rejects_missing_artifact_pin_short_revision_and_approve_instructions() {
    let missing_pin = parse_automation_plan_judge_verdict(
        &json!({
            "decision": "approve",
            "reason": "Looks good.",
            "confidence": "high"
        })
        .to_string(),
        AutomationPlanJudgeValidationContext {
            expected_artifact_id: Some("plan-artifact-1"),
            expected_artifact_version: Some(3),
        },
    )
    .unwrap_err();
    assert!(matches!(missing_pin, AppError::Validation(_)));

    let short_revision = parse_automation_plan_judge_verdict(
        &json!({
            "decision": "revise",
            "reason": "Too vague.",
            "confidence": "low",
            "revisionInstructions": "Be better.",
            "evaluatedArtifactId": "plan-artifact-1"
        })
        .to_string(),
        AutomationPlanJudgeValidationContext {
            expected_artifact_id: Some("plan-artifact-1"),
            expected_artifact_version: Some(3),
        },
    )
    .unwrap_err();
    assert!(matches!(short_revision, AppError::Validation(_)));

    let approve_with_instructions = parse_automation_plan_judge_verdict(
        &json!({
            "decision": "approve",
            "reason": "Approve but also change things.",
            "confidence": "medium",
            "revisionInstructions": "This should be absent for approve verdicts, not merely ignored by the parser.",
            "evaluatedArtifactId": "plan-artifact-1"
        })
        .to_string(),
        AutomationPlanJudgeValidationContext {
            expected_artifact_id: Some("plan-artifact-1"),
            expected_artifact_version: Some(3),
        },
    )
    .unwrap_err();
    assert!(matches!(approve_with_instructions, AppError::Validation(_)));
}

#[test]
fn rejects_wrong_evaluated_artifact_when_context_expects_a_pin() {
    let error = parse_automation_plan_judge_verdict(
        &json!({
            "decision": "approve",
            "reason": "The plan is otherwise plausible.",
            "confidence": "high",
            "evaluatedArtifactId": "plan-artifact-old"
        })
        .to_string(),
        AutomationPlanJudgeValidationContext {
            expected_artifact_id: Some("plan-artifact-1"),
            expected_artifact_version: Some(3),
        },
    )
    .unwrap_err();

    assert!(matches!(error, AppError::Validation(_)));
}
