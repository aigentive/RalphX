use chrono::Utc;
use serde_json::json;

use super::decomposition_verifier::{
    AutomationAuthoringState, AutomationGoalReplanState, AutomationGoalReplanStatus,
};
use super::judge::{
    append_automation_judge_retry_instruction, apply_updated_item_statuses,
    automation_judge_loop_suspected, build_automation_judge_prompt,
    build_automation_run_context_block, current_goal_item_id, mark_current_goal_item_in_progress,
    parse_automation_judge_verdict, AutomationGoalItemStatus, AutomationJudgeAttachmentContext,
    AutomationJudgeDecision, AutomationJudgeItemStatusUpdate, AutomationJudgeNextBaseBranch,
    AutomationJudgeValidationContext, AutomationJudgeVerdict, BuildAutomationJudgePromptInput,
    AUTOMATION_JUDGE_PROMPT_MAX_BYTES,
};
use crate::domain::entities::{
    Automation, AutomationId, AutomationJudgeState, AutomationPlanApprovalMode,
    AutomationPlanJudgeState, AutomationPrMergeMode, AutomationPromptAuthor, AutomationRun,
    AutomationRunId, AutomationRunStatus, AutomationStatus, ProjectId,
};
use crate::error::AppError;

fn automation_with_goal_items(goal_items_json: Option<String>) -> Automation {
    let now = Utc::now();
    Automation {
        id: AutomationId::from_string("automation-1"),
        project_id: ProjectId::from_string("project-1".to_string()),
        name: "Automation 1".to_string(),
        status: AutomationStatus::Active,
        paused_reason_code: None,
        paused_reason_detail: None,
        goal_prompt: "Implement the migration spec one numbered item per PR.".to_string(),
        setup_conversation_id: None,
        provider_harness: "claude".to_string(),
        model_id: "sonnet".to_string(),
        logical_effort: Some("medium".to_string()),
        run_mode: "edit".to_string(),
        base_ref_kind: "local_branch".to_string(),
        base_ref: "main".to_string(),
        base_display_name: Some("main".to_string()),
        base_source_pull_request_json: None,
        goal_items_json,
        chain_mode: "merged_base".to_string(),
        completion_signal: "pr_merged".to_string(),
        plan_approval_mode: AutomationPlanApprovalMode::Manual,
        pr_merge_mode: AutomationPrMergeMode::Manual,
        plan_deep_verification: false,
        max_runs: 25,
        max_consecutive_failures: 3,
        first_run_prompt: Some("Run 1 prompt".to_string()),
        setup_analysis_summary: Some("Setup summary".to_string()),
        spec_artifact_id: None,
        authoring_state_json: None,
        created_at: now,
        updated_at: now,
    }
}

fn automation_run(index: i64, status: AutomationRunStatus) -> AutomationRun {
    let now = Utc::now();
    AutomationRun {
        id: AutomationRunId::from_string(format!("run-{index}")),
        automation_id: AutomationId::from_string("automation-1"),
        run_index: index,
        status,
        judge_state: AutomationJudgeState::None,
        judge_lease_expires_at: None,
        plan_judge_state: AutomationPlanJudgeState::None,
        plan_judge_lease_expires_at: None,
        plan_judge_verdict_json: None,
        plan_revision_round: 0,
        plan_reminder_count: 0,
        plan_pending_instructions: None,
        plan_last_parked_artifact_id: None,
        plan_last_parked_blueprint_artifact_id: None,
        agent_phase_started_at: None,
        conversation_id: None,
        run_prompt: format!("Implement item {index} from the migration spec."),
        prompt_author: AutomationPromptAuthor::SetupAgent,
        base_ref_kind: "local_branch".to_string(),
        base_ref_used: "main".to_string(),
        base_from_run_id: None,
        goal_item_id: None,
        branch_name: Some(format!("ralphx/run-{index}")),
        pr_number: Some(100 + index),
        pr_url: Some(format!(
            "https://github.test/acme/project/pull/{}",
            100 + index
        )),
        pr_title: Some(format!("Run {index} PR")),
        pr_head_ref_name: Some(format!("ralphx/run-{index}")),
        pr_base_ref_name: Some("main".to_string()),
        pr_merged_at: None,
        merge_commit_sha: None,
        diff_stats_json: None,
        agent_summary: Some(format!("Run {index} changed the target files.")),
        judge_verdict_json: None,
        judge_model_id: None,
        error_code: None,
        error_detail: None,
        signal_check_failures: 0,
        started_at: Some(now),
        finished_at: Some(now),
        created_at: now,
        updated_at: now,
    }
}

#[test]
fn build_automation_run_context_block_emits_goal_items_and_phase() {
    let automation = automation_with_goal_items(Some(goal_items_json()));
    let run = automation_run(3, AutomationRunStatus::Running);

    let block = build_automation_run_context_block(&automation, &run);

    assert!(block.starts_with("<automation_context>"));
    assert!(block.trim_end().ends_with("</automation_context>"));
    assert!(block.contains("<goal "));
    assert!(block.contains("Implement the migration spec one numbered item per PR."));
    assert!(block.contains("<goal_items "));
    assert!(block.contains("item-1"));
    assert!(block.contains("<phase "));
    assert!(block.contains("\"runIndex\": 3"));
    assert!(block.contains("\"maxRuns\": 25"));
    assert!(block.contains("\"goalItemsTotal\": 2"));
    assert!(block.contains("\"goalItemsDone\": 1"));
    assert!(block.contains("\"goalItemsPending\": 1"));
}

#[test]
fn build_automation_run_context_block_handles_missing_goal_items() {
    let automation = automation_with_goal_items(None);
    let run = automation_run(1, AutomationRunStatus::Running);

    let block = build_automation_run_context_block(&automation, &run);

    assert!(block.contains("<goal_items "));
    assert!(block.contains("[]"));
    assert!(block.contains("\"goalItemsTotal\": 0"));
    assert!(block.contains("\"goalItemsPending\": 0"));
}

#[test]
fn build_automation_run_context_block_exposes_matching_pending_goal_replan() {
    let mut automation = automation_with_goal_items(Some(goal_items_json()));
    let proposal = json!([
        { "id": "item-1", "title": "First", "status": "done" },
        { "id": "item-2a", "title": "Split backend", "status": "pending" },
        { "id": "item-2b", "title": "Split UI", "status": "pending" }
    ])
    .to_string();
    let state = AutomationAuthoringState {
        pending_goal_replan: Some(AutomationGoalReplanState {
            source_run_id: "run-2".to_string(),
            base_goal_items_json: automation.goal_items_json.clone().unwrap(),
            proposed_goal_items_json: proposal,
            reason: "Split the remaining work.".to_string(),
            status: AutomationGoalReplanStatus::Pending,
            created_at: Utc::now().to_rfc3339(),
            applied_at: None,
        }),
        ..Default::default()
    };
    automation.authoring_state_json = Some(serde_json::to_string(&state).unwrap());
    let mut matching = automation_run(3, AutomationRunStatus::Pending);
    matching.base_from_run_id = Some(AutomationRunId::from_string("run-2"));
    let unrelated = automation_run(4, AutomationRunStatus::Pending);

    let matching_block = build_automation_run_context_block(&automation, &matching);
    let unrelated_block = build_automation_run_context_block(&automation, &unrelated);

    assert!(matching_block.contains("<goal_items_proposal "));
    assert!(matching_block.contains("item-2a"));
    assert!(!unrelated_block.contains("<goal_items_proposal "));
}

fn goal_items_json() -> String {
    json!([
        { "id": "item-1", "title": "First", "status": "done" },
        { "id": "item-2", "title": "Second", "status": "pending" }
    ])
    .to_string()
}

fn valid_continue_output() -> String {
    json!({
        "decision": "continue",
        "goalMet": false,
        "reason": "Item 2 remains and should be implemented next.",
        "confidence": 0.86,
        "goalProgress": { "completedItems": 1, "totalItems": 2, "summary": "One of two items is complete." },
        "updatedItemStatuses": [{ "id": "item-1", "status": "done" }],
        "nextRunPrompt": "Implement item 2 from the migration spec. Keep the PR scoped, include tests, and publish the PR.",
        "nextBaseBranch": "automation_base"
    })
    .to_string()
}

#[test]
fn judge_accepts_add_split_and_reorder_proposal_for_plan_gated_continuation() {
    let automation = automation_with_goal_items(Some(goal_items_json()));
    let previous = automation_run(1, AutomationRunStatus::Merged);
    let output = json!({
        "decision": "continue",
        "goalMet": false,
        "reason": "The second phase needs to split after the implementation discovery.",
        "confidence": 0.9,
        "goalProgress": { "completedItems": 1, "totalItems": 3, "summary": "One phase done; two refined phases remain." },
        "updatedItemStatuses": null,
        "goalItemsProposal": [
            { "id": "item-1", "title": "First", "status": "done" },
            { "id": "item-2b", "title": "Second integration", "status": "pending" },
            { "id": "item-2a", "title": "Second backend", "status": "pending" }
        ],
        "nextRunPrompt": "Plan and implement the refined second backend phase, with focused tests and a scoped pull request.",
        "nextBaseBranch": "automation_base"
    })
    .to_string();

    let verdict = parse_automation_judge_verdict(
        &output,
        AutomationJudgeValidationContext {
            automation: &automation,
            previous_run: &previous,
        },
    )
    .unwrap();

    let proposal = verdict.goal_items_proposal.unwrap();
    assert_eq!(proposal.len(), 3);
    assert_eq!(proposal[1].id, "item-2b");
    assert_eq!(proposal[2].id, "item-2a");
}

#[test]
fn judge_replan_proposal_cannot_drop_completed_history() {
    let automation = automation_with_goal_items(Some(goal_items_json()));
    let previous = automation_run(1, AutomationRunStatus::Merged);
    let output = json!({
        "decision": "continue",
        "goalMet": false,
        "reason": "Replace the remaining phase.",
        "confidence": 0.8,
        "goalProgress": null,
        "updatedItemStatuses": null,
        "goalItemsProposal": [
            { "id": "replacement", "title": "Replacement", "status": "pending" }
        ],
        "nextRunPrompt": "Plan and implement the replacement phase with focused tests and publish the scoped pull request.",
        "nextBaseBranch": "automation_base"
    })
    .to_string();

    let error = parse_automation_judge_verdict(
        &output,
        AutomationJudgeValidationContext {
            automation: &automation,
            previous_run: &previous,
        },
    )
    .unwrap_err();

    assert!(error
        .to_string()
        .contains("preserve completed goal item item-1"));
}

#[test]
fn judge_stop_verdict_rejects_a_structural_replan_proposal() {
    let automation = automation_with_goal_items(Some(goal_items_json()));
    let previous = automation_run(1, AutomationRunStatus::Merged);
    let output = json!({
        "decision": "stop",
        "goalMet": false,
        "reason": "Human input is required.",
        "confidence": 0.8,
        "goalProgress": null,
        "updatedItemStatuses": null,
        "goalItemsProposal": [
            { "id": "item-1", "title": "First", "status": "done" },
            { "id": "item-2", "title": "Second", "status": "pending" }
        ],
        "nextRunPrompt": null,
        "nextBaseBranch": null
    })
    .to_string();

    let error = parse_automation_judge_verdict(
        &output,
        AutomationJudgeValidationContext {
            automation: &automation,
            previous_run: &previous,
        },
    )
    .unwrap_err();

    assert!(error.to_string().contains("continue verdicts"));
}

fn validation_context<'a>(
    automation: &'a Automation,
    run: &'a AutomationRun,
) -> AutomationJudgeValidationContext<'a> {
    AutomationJudgeValidationContext {
        automation,
        previous_run: run,
    }
}

#[test]
fn parses_valid_continue_verdict() {
    let automation = automation_with_goal_items(Some(goal_items_json()));
    let run = automation_run(1, AutomationRunStatus::Merged);

    let verdict = parse_automation_judge_verdict(
        &valid_continue_output(),
        validation_context(&automation, &run),
    )
    .unwrap();

    assert_eq!(verdict.decision, AutomationJudgeDecision::Continue);
    assert_eq!(
        verdict.next_base_branch,
        Some(AutomationJudgeNextBaseBranch::AutomationBase)
    );
    assert_eq!(verdict.updated_item_statuses.unwrap()[0].id, "item-1");
}

#[test]
fn continue_verdict_keeps_repeated_run_on_authoritative_goal_item_phase() {
    let goal_items = json!([
        { "id": "item-1", "status": "done", "title": "Contract" },
        { "id": "item-2", "status": "done", "title": "Backend" },
        { "id": "item-3", "status": "done", "title": "Cache" },
        { "id": "item-4", "status": "done", "title": "MCP tools" },
        { "id": "item-5", "status": "done", "title": "Prompt alignment" },
        { "id": "item-6", "status": "in_progress", "title": "Verification" }
    ])
    .to_string();
    let automation = automation_with_goal_items(Some(goal_items));
    let run = automation_run(12, AutomationRunStatus::Merged);
    let output = json!({
        "decision": "continue",
        "goalMet": false,
        "reason": "Verification still needs another focused run.",
        "confidence": 0.95,
        "goalProgress": { "completedItems": 5, "totalItems": 6, "summary": "Five of six items are complete." },
        "updatedItemStatuses": [{ "id": "item-6", "status": "in_progress" }],
        "nextRunPrompt": "Phase 7: finish verification with focused behavioral tests and a security review before publishing the scoped PR.",
        "nextBaseBranch": "automation_base"
    })
    .to_string();

    let verdict =
        parse_automation_judge_verdict(&output, validation_context(&automation, &run)).unwrap();

    assert_eq!(
        verdict.next_run_prompt.as_deref(),
        Some(
            "Phase 6: finish verification with focused behavioral tests and a security review before publishing the scoped PR."
        )
    );
}

#[test]
fn continue_verdict_leaves_non_phase_prompt_unchanged() {
    let automation = automation_with_goal_items(Some(goal_items_json()));
    let run = automation_run(2, AutomationRunStatus::Merged);
    let output = json!({
        "decision": "continue",
        "goalMet": false,
        "reason": "Item 2 still needs the next scoped run.",
        "confidence": 0.82,
        "goalProgress": { "completedItems": 1, "totalItems": 2, "summary": "One item remains." },
        "updatedItemStatuses": null,
        "nextRunPrompt": "Continue item 2 with focused behavioral tests and publish the scoped pull request.",
        "nextBaseBranch": "automation_base"
    })
    .to_string();

    let verdict =
        parse_automation_judge_verdict(&output, validation_context(&automation, &run)).unwrap();

    assert_eq!(
        verdict.next_run_prompt.as_deref(),
        Some("Continue item 2 with focused behavioral tests and publish the scoped pull request.")
    );
}

#[test]
fn continue_verdict_leaves_malformed_phase_prompt_unchanged() {
    let automation = automation_with_goal_items(Some(goal_items_json()));
    let run = automation_run(2, AutomationRunStatus::Merged);
    let output = json!({
        "decision": "continue",
        "goalMet": false,
        "reason": "Item 2 still needs the next scoped run.",
        "confidence": 0.82,
        "goalProgress": { "completedItems": 1, "totalItems": 2, "summary": "One item remains." },
        "updatedItemStatuses": null,
        "nextRunPrompt": "Phase next: continue item 2 with focused behavioral tests and publish the scoped pull request.",
        "nextBaseBranch": "automation_base"
    })
    .to_string();

    let verdict =
        parse_automation_judge_verdict(&output, validation_context(&automation, &run)).unwrap();

    assert_eq!(
        verdict.next_run_prompt.as_deref(),
        Some(
            "Phase next: continue item 2 with focused behavioral tests and publish the scoped pull request."
        )
    );
}

#[test]
fn continue_verdict_leaves_phase_prompt_without_goal_items_unchanged() {
    let automation = automation_with_goal_items(None);
    let run = automation_run(2, AutomationRunStatus::Merged);
    let output = json!({
        "decision": "continue",
        "goalMet": false,
        "reason": "The automation has no structured goal items to map to a phase.",
        "confidence": 0.82,
        "goalProgress": null,
        "updatedItemStatuses": null,
        "nextRunPrompt": "Phase 4: continue with focused behavioral tests and publish the scoped pull request.",
        "nextBaseBranch": "automation_base"
    })
    .to_string();

    let verdict =
        parse_automation_judge_verdict(&output, validation_context(&automation, &run)).unwrap();

    assert_eq!(
        verdict.next_run_prompt.as_deref(),
        Some(
            "Phase 4: continue with focused behavioral tests and publish the scoped pull request."
        )
    );
}

#[test]
fn continue_verdict_leaves_phase_prompt_when_all_goal_items_are_finished() {
    let goal_items = json!([
        { "id": "item-1", "status": "done", "title": "Contract" },
        { "id": "item-2", "status": "skipped", "title": "Optional cleanup" }
    ])
    .to_string();
    let automation = automation_with_goal_items(Some(goal_items));
    let run = automation_run(2, AutomationRunStatus::Merged);
    let output = json!({
        "decision": "continue",
        "goalMet": false,
        "reason": "The judge asked for one more audit run even though structured items are terminal.",
        "confidence": 0.82,
        "goalProgress": { "completedItems": 2, "totalItems": 2, "summary": "All items are terminal." },
        "updatedItemStatuses": null,
        "nextRunPrompt": "Phase 5: run a final audit with focused behavioral tests and publish the scoped pull request.",
        "nextBaseBranch": "automation_base"
    })
    .to_string();

    let verdict =
        parse_automation_judge_verdict(&output, validation_context(&automation, &run)).unwrap();

    assert_eq!(
        verdict.next_run_prompt.as_deref(),
        Some("Phase 5: run a final audit with focused behavioral tests and publish the scoped pull request.")
    );
}

#[test]
fn continue_verdict_normalizes_nested_status_only_goal_item_phase() {
    let goal_items = json!({
        "group": {
            "items": [
                { "status": "done", "title": "Implicit first item" },
                { "status": "pending", "title": "Implicit second item" }
            ]
        }
    })
    .to_string();
    let automation = automation_with_goal_items(Some(goal_items));
    let run = automation_run(2, AutomationRunStatus::Merged);
    let output = json!({
        "decision": "continue",
        "goalMet": false,
        "reason": "The nested second item still needs a focused run.",
        "confidence": 0.82,
        "goalProgress": { "completedItems": 1, "totalItems": 2, "summary": "One nested item remains." },
        "updatedItemStatuses": null,
        "nextRunPrompt": "Phase 5: continue the nested second item with focused behavioral tests and publish the scoped pull request.",
        "nextBaseBranch": "automation_base"
    })
    .to_string();

    let verdict =
        parse_automation_judge_verdict(&output, validation_context(&automation, &run)).unwrap();

    assert_eq!(
        verdict.next_run_prompt.as_deref(),
        Some(
            "Phase 2: continue the nested second item with focused behavioral tests and publish the scoped pull request."
        )
    );
}

#[test]
fn parses_valid_stop_verdict() {
    let automation = automation_with_goal_items(Some(goal_items_json()));
    let run = automation_run(1, AutomationRunStatus::Merged);
    let output = json!({
        "decision": "stop",
        "goalMet": true,
        "reason": "All requested goal items are complete.",
        "confidence": 0.91,
        "goalProgress": { "completedItems": 2, "totalItems": 2, "summary": "Both items are done." },
        "updatedItemStatuses": [{ "id": "item-2", "status": "done" }],
        "nextRunPrompt": null,
        "nextBaseBranch": null
    })
    .to_string();

    let verdict =
        parse_automation_judge_verdict(&output, validation_context(&automation, &run)).unwrap();

    assert_eq!(verdict.decision, AutomationJudgeDecision::Stop);
    assert!(verdict.goal_met);
    assert!(verdict.next_run_prompt.is_none());
    assert!(verdict.next_base_branch.is_none());
}

#[test]
fn rejects_goal_met_stop_when_updated_items_leave_non_terminal_work() {
    let automation = automation_with_goal_items(Some(goal_items_json()));
    let run = automation_run(1, AutomationRunStatus::Merged);
    let output = json!({
        "decision": "stop",
        "goalMet": true,
        "reason": "The goal is complete.",
        "confidence": 0.91,
        "goalProgress": { "completedItems": 2, "totalItems": 2, "summary": "Both items are done." },
        "updatedItemStatuses": [{ "id": "item-2", "status": "in_progress" }],
        "nextRunPrompt": null,
        "nextBaseBranch": null
    })
    .to_string();

    let error =
        parse_automation_judge_verdict(&output, validation_context(&automation, &run)).unwrap_err();

    assert!(matches!(error, AppError::Validation(message) if message.contains("goalMet")));
}

#[test]
fn parses_fenced_json_verdict() {
    let automation = automation_with_goal_items(Some(goal_items_json()));
    let run = automation_run(1, AutomationRunStatus::Merged);
    let output = format!("```json\n{}\n```", valid_continue_output());

    let verdict =
        parse_automation_judge_verdict(&output, validation_context(&automation, &run)).unwrap();

    assert_eq!(verdict.decision, AutomationJudgeDecision::Continue);
}

#[test]
fn parses_last_json_object_in_text() {
    let automation = automation_with_goal_items(None);
    let run = automation_run(1, AutomationRunStatus::Merged);
    let stop = json!({
        "decision": "stop",
        "goalMet": true,
        "reason": "All items complete.",
        "confidence": 0.7,
        "goalProgress": null,
        "updatedItemStatuses": null,
        "nextRunPrompt": null,
        "nextBaseBranch": null
    });
    let output = format!("draft {{\"ignored\": true}}\nfinal: {stop}");

    let verdict =
        parse_automation_judge_verdict(&output, validation_context(&automation, &run)).unwrap();

    assert_eq!(verdict.decision, AutomationJudgeDecision::Stop);
    assert!(verdict.goal_met);
}

#[test]
fn rejects_malformed_output() {
    let automation = automation_with_goal_items(Some(goal_items_json()));
    let run = automation_run(1, AutomationRunStatus::Merged);

    let error = parse_automation_judge_verdict(
        "{ \"decision\": \"continue\" ",
        validation_context(&automation, &run),
    )
    .unwrap_err();

    assert!(matches!(error, AppError::Validation(_)));
}

#[test]
fn rejects_missing_required_key() {
    let automation = automation_with_goal_items(Some(goal_items_json()));
    let run = automation_run(1, AutomationRunStatus::Merged);
    let output = json!({
        "decision": "stop",
        "goalMet": true,
        "reason": "done",
        "confidence": 0.8,
        "goalProgress": null,
        "updatedItemStatuses": null,
        "nextRunPrompt": null
    })
    .to_string();

    let error =
        parse_automation_judge_verdict(&output, validation_context(&automation, &run)).unwrap_err();

    assert!(matches!(error, AppError::Validation(_)));
}

#[test]
fn rejects_continue_without_substantive_prompt() {
    let automation = automation_with_goal_items(Some(goal_items_json()));
    let run = automation_run(1, AutomationRunStatus::Merged);
    let output = json!({
        "decision": "continue",
        "goalMet": false,
        "reason": "keep going",
        "confidence": 0.8,
        "goalProgress": null,
        "updatedItemStatuses": null,
        "nextRunPrompt": "Do it",
        "nextBaseBranch": "automation_base"
    })
    .to_string();

    let error =
        parse_automation_judge_verdict(&output, validation_context(&automation, &run)).unwrap_err();

    assert!(matches!(error, AppError::Validation(_)));
}

#[test]
fn rejects_continue_without_next_base_branch() {
    let automation = automation_with_goal_items(Some(goal_items_json()));
    let run = automation_run(1, AutomationRunStatus::Merged);
    let output = json!({
        "decision": "continue",
        "goalMet": false,
        "reason": "There is still work to continue.",
        "confidence": 0.8,
        "goalProgress": null,
        "updatedItemStatuses": null,
        "nextRunPrompt": "Implement the next scoped migration item with tests and publish the follow-up pull request.",
        "nextBaseBranch": null
    })
    .to_string();

    let error =
        parse_automation_judge_verdict(&output, validation_context(&automation, &run)).unwrap_err();

    assert!(matches!(error, AppError::Validation(_)));
}

#[test]
fn rejects_empty_reason_and_stop_with_next_run_payload() {
    let automation = automation_with_goal_items(Some(goal_items_json()));
    let run = automation_run(1, AutomationRunStatus::Merged);
    let empty_reason = json!({
        "decision": "stop",
        "goalMet": true,
        "reason": "   ",
        "confidence": 0.8,
        "goalProgress": null,
        "updatedItemStatuses": null,
        "nextRunPrompt": null,
        "nextBaseBranch": null
    })
    .to_string();

    let error =
        parse_automation_judge_verdict(&empty_reason, validation_context(&automation, &run))
            .unwrap_err();
    assert!(matches!(error, AppError::Validation(_)));

    let stop_with_next_run = json!({
        "decision": "stop",
        "goalMet": true,
        "reason": "The judge is stopping.",
        "confidence": 0.8,
        "goalProgress": null,
        "updatedItemStatuses": null,
        "nextRunPrompt": "Do more work even though this is a stop verdict.",
        "nextBaseBranch": "automation_base"
    })
    .to_string();

    let error =
        parse_automation_judge_verdict(&stop_with_next_run, validation_context(&automation, &run))
            .unwrap_err();
    assert!(matches!(error, AppError::Validation(_)));
}

#[test]
fn rejects_previous_pr_head_without_stacked_mode() {
    let automation = automation_with_goal_items(Some(goal_items_json()));
    let run = automation_run(1, AutomationRunStatus::Merged);
    let output = valid_continue_output().replace("automation_base", "previous_pr_head");

    let error =
        parse_automation_judge_verdict(&output, validation_context(&automation, &run)).unwrap_err();

    assert!(matches!(error, AppError::Validation(_)));
}

#[test]
fn parses_previous_pr_head_for_stacked_mode() {
    let mut automation = automation_with_goal_items(Some(goal_items_json()));
    automation.chain_mode = "pr_head_stacked".to_string();
    let run = automation_run(1, AutomationRunStatus::Merged);
    let output = valid_continue_output().replace("automation_base", "previous_pr_head");

    let verdict =
        parse_automation_judge_verdict(&output, validation_context(&automation, &run)).unwrap();

    assert_eq!(
        verdict.next_base_branch,
        Some(AutomationJudgeNextBaseBranch::PreviousPrHead)
    );
}

#[test]
fn rejects_automation_base_for_stacked_mode() {
    let mut automation = automation_with_goal_items(Some(goal_items_json()));
    automation.chain_mode = "pr_head_stacked".to_string();
    let run = automation_run(1, AutomationRunStatus::Merged);

    let error = parse_automation_judge_verdict(
        &valid_continue_output(),
        validation_context(&automation, &run),
    )
    .unwrap_err();

    assert!(matches!(error, AppError::Validation(_)));
}

#[test]
fn rejects_unknown_goal_item_update_id() {
    let automation = automation_with_goal_items(Some(goal_items_json()));
    let run = automation_run(1, AutomationRunStatus::Merged);
    let output = valid_continue_output().replace("item-1", "missing-item");

    let error =
        parse_automation_judge_verdict(&output, validation_context(&automation, &run)).unwrap_err();

    assert!(matches!(error, AppError::Validation(_)));
}

#[test]
fn applies_updated_item_statuses_to_stored_goal_items() {
    let automation = automation_with_goal_items(Some(goal_items_json()));
    let run = automation_run(1, AutomationRunStatus::Merged);
    let verdict = parse_automation_judge_verdict(
        &valid_continue_output(),
        validation_context(&automation, &run),
    )
    .unwrap();

    let updated = apply_updated_item_statuses(
        automation.goal_items_json.as_deref(),
        verdict.updated_item_statuses.as_deref(),
    )
    .unwrap()
    .unwrap();

    assert!(updated.contains("\"id\":\"item-1\""));
    assert!(updated.contains("\"status\":\"done\""));
}

#[test]
fn applies_all_goal_item_status_variants_and_rejects_missing_storage() {
    let updates = vec![
        AutomationJudgeItemStatusUpdate {
            id: "item-1".to_string(),
            status: AutomationGoalItemStatus::Pending,
        },
        AutomationJudgeItemStatusUpdate {
            id: "item-2".to_string(),
            status: AutomationGoalItemStatus::InProgress,
        },
        AutomationJudgeItemStatusUpdate {
            id: "item-3".to_string(),
            status: AutomationGoalItemStatus::Skipped,
        },
    ];

    let missing_storage = apply_updated_item_statuses(None, Some(&updates)).unwrap_err();
    assert!(matches!(missing_storage, AppError::Validation(_)));

    let goal_items = json!([
        { "id": "item-1", "status": "done" },
        { "id": "item-2", "status": "pending" },
        { "id": "item-3", "status": "pending" },
        { "id": "item-4", "status": "unknown" }
    ])
    .to_string();
    let updated = apply_updated_item_statuses(Some(&goal_items), Some(&updates))
        .unwrap()
        .unwrap();

    assert!(updated.contains("\"status\":\"pending\""));
    assert!(updated.contains("\"status\":\"in_progress\""));
    assert!(updated.contains("\"status\":\"skipped\""));

    let unmatched = [AutomationJudgeItemStatusUpdate {
        id: "missing".to_string(),
        status: AutomationGoalItemStatus::Done,
    }];
    let error = apply_updated_item_statuses(Some(&goal_items), Some(&unmatched)).unwrap_err();
    assert!(matches!(error, AppError::Validation(_)));
}

#[test]
fn start_mark_treats_missing_status_as_pending_current_item() {
    let goal_items = json!([
        { "id": "item-1", "title": "Implicit pending" },
        { "id": "item-2", "title": "Later", "status": "pending" }
    ])
    .to_string();

    let updated = mark_current_goal_item_in_progress(Some(&goal_items))
        .unwrap()
        .expect("implicit pending item should be marked");
    let value: serde_json::Value = serde_json::from_str(&updated).unwrap();

    assert_eq!(value[0]["status"], "in_progress");
    assert_eq!(value[1]["status"], "pending");
}

#[test]
fn start_mark_does_not_create_second_in_progress_item() {
    let goal_items = json!([
        { "id": "item-1", "title": "First", "status": "pending" },
        { "id": "item-2", "title": "Already active", "status": "in_progress" }
    ])
    .to_string();

    let updated = mark_current_goal_item_in_progress(Some(&goal_items)).unwrap();

    assert_eq!(updated, None);
}

#[test]
fn detects_continue_loop_when_prompt_repeats_after_produced_but_unmerged_run() {
    // A run that produced a PR which was closed unmerged, then the judge proposes the exact
    // same prompt again -> a genuine judge loop.
    let automation = automation_with_goal_items(Some(goal_items_json()));
    let mut run = automation_run(1, AutomationRunStatus::PrClosed);
    run.run_prompt =
        "Implement item 2 from spec with targeted tests and publish the scoped PR".to_string();
    let output = valid_continue_output().replace(
        "Implement item 2 from the migration spec. Keep the PR scoped, include tests, and publish the PR.",
        " Implement item 2 from spec with targeted tests and publish the scoped PR! ",
    );
    let verdict =
        parse_automation_judge_verdict(&output, validation_context(&automation, &run)).unwrap();

    assert!(automation_judge_loop_suspected(&run, &verdict));
}

#[test]
fn retry_after_agent_failed_run_is_not_a_loop() {
    // Runs that crashed / timed out / were killed never got a fair attempt, so re-issuing the
    // same prompt is a legitimate retry, not a judge loop (repeated agent failures are bounded
    // by max_consecutive_failures instead). Regression for an automation that could not be
    // resumed after its agent was killed by a full disk.
    let automation = automation_with_goal_items(Some(goal_items_json()));
    let mut failed = automation_run(1, AutomationRunStatus::AgentFailed);
    failed.run_prompt =
        "Implement item 2 from spec with targeted tests and publish the scoped PR".to_string();
    let output = valid_continue_output().replace(
        "Implement item 2 from the migration spec. Keep the PR scoped, include tests, and publish the PR.",
        " Implement item 2 from spec with targeted tests and publish the scoped PR! ",
    );
    let verdict =
        parse_automation_judge_verdict(&output, validation_context(&automation, &failed)).unwrap();
    assert!(!automation_judge_loop_suspected(&failed, &verdict));

    // Cancelled runs are likewise a retry, not a loop.
    let mut cancelled = failed.clone();
    cancelled.status = AutomationRunStatus::Cancelled;
    assert!(!automation_judge_loop_suspected(&cancelled, &verdict));
}

#[test]
fn loop_detection_stays_false_for_stop_merged_or_missing_prompt() {
    let mut run = automation_run(1, AutomationRunStatus::AgentFailed);
    run.run_prompt =
        "Implement item 2 from spec with targeted tests and publish the scoped PR".to_string();
    let stop = AutomationJudgeVerdict {
        decision: AutomationJudgeDecision::Stop,
        goal_met: false,
        reason: "Stop instead of continuing.".to_string(),
        confidence: 0.5,
        goal_progress: None,
        updated_item_statuses: None,
        goal_items_proposal: None,
        next_run_prompt: None,
        next_base_branch: None,
    };
    assert!(!automation_judge_loop_suspected(&run, &stop));

    let missing_prompt = AutomationJudgeVerdict {
        decision: AutomationJudgeDecision::Continue,
        next_base_branch: Some(AutomationJudgeNextBaseBranch::AutomationBase),
        ..stop.clone()
    };
    assert!(!automation_judge_loop_suspected(&run, &missing_prompt));

    let mut merged = run.clone();
    merged.status = AutomationRunStatus::Merged;
    let repeating_prompt = AutomationJudgeVerdict {
        decision: AutomationJudgeDecision::Continue,
        next_run_prompt: Some(run.run_prompt.clone()),
        next_base_branch: Some(AutomationJudgeNextBaseBranch::AutomationBase),
        ..stop
    };
    assert!(!automation_judge_loop_suspected(&merged, &repeating_prompt));
}

#[test]
fn parses_uppercase_fence_after_invalid_fence_and_escaped_text_json() {
    let automation = automation_with_goal_items(Some(goal_items_json()));
    let run = automation_run(1, AutomationRunStatus::Merged);
    let output = format!(
        "```json\nnot-json\n```\nnoise\n```JSON\n{}\n```",
        valid_continue_output()
    );

    let verdict =
        parse_automation_judge_verdict(&output, validation_context(&automation, &run)).unwrap();
    assert_eq!(verdict.decision, AutomationJudgeDecision::Continue);

    let escaped_stop = r#"draft {"ignored":true}
final: {"decision":"stop","goalMet":true,"reason":"done with \"quoted\" path C:\\tmp","confidence":1.8,"goalProgress":null,"updatedItemStatuses":null,"nextRunPrompt":null,"nextBaseBranch":null}"#;
    let stop_automation = automation_with_goal_items(None);
    let verdict =
        parse_automation_judge_verdict(escaped_stop, validation_context(&stop_automation, &run))
            .unwrap();
    assert_eq!(verdict.confidence, 1.0);
    assert!(verdict.reason.contains("\"quoted\""));
}

#[test]
fn prompt_builder_keeps_goal_full_and_truncates_history_within_budget() {
    let goal = format!("Goal: {}", "preserve-me ".repeat(800));
    let mut automation = automation_with_goal_items(Some(goal_items_json()));
    automation.goal_prompt = goal.clone();
    automation.setup_analysis_summary = Some("setup ".repeat(4_000));
    let mut runs = (1..=20)
        .map(|index| {
            let mut run = automation_run(index, AutomationRunStatus::Merged);
            run.run_prompt = format!("history-{index} {}", "large ".repeat(2_000));
            run.agent_summary = Some("summary ".repeat(2_000));
            run
        })
        .collect::<Vec<_>>();
    let previous = runs.pop().unwrap();

    let prompt = build_automation_judge_prompt(BuildAutomationJudgePromptInput {
        automation: &automation,
        runs: &runs,
        previous_run: &previous,
        attachments: &[],
        context_refs: &[],
    })
    .unwrap();

    assert!(prompt.len() <= AUTOMATION_JUDGE_PROMPT_MAX_BYTES);
    assert_eq!(xml_section_body(&prompt, "goal"), goal.trim());
    assert!(prompt.contains("<run_history truncated=\"true\">"));
}

#[test]
fn prompt_builder_counts_goal_statuses_and_consecutive_failures() {
    let goal_items = json!([
        { "id": "item-1", "status": "done" },
        { "id": "item-2", "status": "skipped" },
        { "id": "item-3", "status": "pending" },
        { "id": "item-4", "status": "in_progress" },
        { "id": "item-5", "status": "unknown" }
    ]);
    let automation = automation_with_goal_items(Some(goal_items.to_string()));
    let runs = vec![
        automation_run(1, AutomationRunStatus::Merged),
        automation_run(2, AutomationRunStatus::PrClosed),
        automation_run(3, AutomationRunStatus::AgentFailed),
    ];
    let previous = runs.last().unwrap();

    let prompt = build_automation_judge_prompt(BuildAutomationJudgePromptInput {
        automation: &automation,
        runs: &runs,
        previous_run: previous,
        attachments: &[],
        context_refs: &[],
    })
    .unwrap();

    assert!(prompt.contains("\"goalItemsTotal\": 5"));
    assert!(prompt.contains("\"goalItemsDone\": 2"));
    assert!(prompt.contains("\"goalItemsPending\": 2"));
    assert!(prompt.contains("\"consecutiveFailureCount\": 2"));

    let mut invalid_items = automation.clone();
    invalid_items.goal_items_json = Some("not-json".to_string());
    let prompt = build_automation_judge_prompt(BuildAutomationJudgePromptInput {
        automation: &invalid_items,
        runs: std::slice::from_ref(previous),
        previous_run: previous,
        attachments: &[],
        context_refs: &[],
    })
    .unwrap();
    assert!(prompt.contains("\"goalItemsTotal\": 0"));
}

#[test]
fn prompt_builder_errors_when_fixed_sections_exceed_budget() {
    let mut automation = automation_with_goal_items(None);
    automation.goal_prompt = "goal ".repeat(20_000);
    let previous = automation_run(1, AutomationRunStatus::Merged);

    let error = build_automation_judge_prompt(BuildAutomationJudgePromptInput {
        automation: &automation,
        runs: std::slice::from_ref(&previous),
        previous_run: &previous,
        attachments: &[],
        context_refs: &[],
    })
    .unwrap_err();

    assert!(matches!(error, AppError::Validation(_)));
}

#[test]
fn prompt_builder_mid_loop_payload_keeps_goal_items_and_run_counts() {
    let items = (1..=20)
        .map(|index| {
            json!({
                "id": format!("item-{index}"),
                "title": format!("Item {index}"),
                "status": if index < 12 { "done" } else { "pending" }
            })
        })
        .collect::<Vec<_>>();
    let automation = automation_with_goal_items(Some(json!(items).to_string()));
    let runs = (1..=12)
        .map(|index| automation_run(index, AutomationRunStatus::Merged))
        .collect::<Vec<_>>();
    let previous = runs.last().unwrap();

    let prompt = build_automation_judge_prompt(BuildAutomationJudgePromptInput {
        automation: &automation,
        runs: &runs,
        previous_run: previous,
        attachments: &[],
        context_refs: &[],
    })
    .unwrap();

    assert!(prompt.contains("\"id\":\"item-12\""));
    assert!(prompt.contains("\"runsUsed\": 12"));
    assert!(prompt.contains("\"goalItemsPending\": 9"));
}

#[test]
fn prompt_builder_inlines_spec_attachment_into_original_inputs() {
    let automation = automation_with_goal_items(None);
    let previous = automation_run(1, AutomationRunStatus::Merged);
    let attachment = AutomationJudgeAttachmentContext {
        file_name: "Automation spec".to_string(),
        mime_type: Some("text/markdown".to_string()),
        file_size: Some(64),
        text_content: Some("SPEC_MARKER: implement phase one before phase two.".to_string()),
    };

    let prompt = build_automation_judge_prompt(BuildAutomationJudgePromptInput {
        automation: &automation,
        runs: std::slice::from_ref(&previous),
        previous_run: &previous,
        attachments: std::slice::from_ref(&attachment),
        context_refs: &[],
    })
    .unwrap();

    let body = xml_section_body(&prompt, "original_inputs");
    assert!(body.contains("SPEC_MARKER: implement phase one before phase two."));
    assert!(body.contains("Automation spec"));
}

#[test]
fn prompt_builder_truncates_oversized_spec_attachment() {
    let automation = automation_with_goal_items(None);
    let previous = automation_run(1, AutomationRunStatus::Merged);
    // ~30KB of text — larger than ORIGINAL_INPUTS_MAX_BYTES (12KB) so the whole
    // original_inputs section is byte-truncated by the prompt builder.
    let attachment = AutomationJudgeAttachmentContext {
        file_name: "Automation spec".to_string(),
        mime_type: Some("text/markdown".to_string()),
        file_size: Some(30 * 1024),
        text_content: Some("spec ".repeat(6_000)),
    };

    let prompt = build_automation_judge_prompt(BuildAutomationJudgePromptInput {
        automation: &automation,
        runs: std::slice::from_ref(&previous),
        previous_run: &previous,
        attachments: std::slice::from_ref(&attachment),
        context_refs: &[],
    })
    .unwrap();

    assert!(prompt.contains("<original_inputs truncated=\"true\">"));
}

#[test]
fn retry_instruction_never_exceeds_prompt_budget() {
    let mut prompt = "x".repeat(AUTOMATION_JUDGE_PROMPT_MAX_BYTES);

    assert!(!append_automation_judge_retry_instruction(&mut prompt));
    assert_eq!(prompt.len(), AUTOMATION_JUDGE_PROMPT_MAX_BYTES);

    prompt.truncate(AUTOMATION_JUDGE_PROMPT_MAX_BYTES - 200);
    assert!(append_automation_judge_retry_instruction(&mut prompt));
    assert!(prompt.len() <= AUTOMATION_JUDGE_PROMPT_MAX_BYTES);
    assert!(prompt.contains("<retry_instruction"));
}

fn xml_section_body(prompt: &str, tag: &str) -> String {
    let start_marker = format!("<{tag} truncated=\"false\">\n");
    let end_marker = format!("\n</{tag}>");
    let start = prompt.find(&start_marker).expect("expected prompt section") + start_marker.len();
    let end = prompt[start..]
        .find(&end_marker)
        .expect("expected section end")
        + start;
    prompt[start..end].to_string()
}

#[test]
fn current_goal_item_id_returns_first_non_done_item() {
    let goal_items = json!([
        {"id": "item-1", "title": "First", "status": "done"},
        {"id": "item-2", "title": "Second", "status": "skipped"},
        {"id": "item-3", "title": "Third", "status": "in_progress"},
        {"id": "item-4", "title": "Fourth", "status": "pending"}
    ])
    .to_string();

    assert_eq!(
        current_goal_item_id(Some(&goal_items)).as_deref(),
        Some("item-3")
    );
}

#[test]
fn current_goal_item_id_is_none_when_all_items_finished() {
    let goal_items = json!([
        {"id": "item-1", "status": "done"},
        {"id": "item-2", "status": "skipped"}
    ])
    .to_string();

    assert_eq!(current_goal_item_id(Some(&goal_items)), None);
}

#[test]
fn current_goal_item_id_fails_soft_on_missing_or_invalid_json() {
    assert_eq!(current_goal_item_id(None), None);
    assert_eq!(current_goal_item_id(Some("   ")), None);
    assert_eq!(current_goal_item_id(Some("not-json")), None);
}
