use chrono::Utc;
use serde_json::json;

use super::judge::{
    apply_updated_item_statuses, automation_judge_loop_suspected, build_automation_judge_prompt,
    parse_automation_judge_verdict, AutomationJudgeDecision, AutomationJudgeNextBaseBranch,
    AutomationJudgeValidationContext, BuildAutomationJudgePromptInput,
    AUTOMATION_JUDGE_PROMPT_MAX_BYTES,
};
use crate::domain::entities::{
    Automation, AutomationId, AutomationJudgeState, AutomationPromptAuthor, AutomationRun,
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
        max_runs: 25,
        max_consecutive_failures: 3,
        first_run_prompt: Some("Run 1 prompt".to_string()),
        setup_analysis_summary: Some("Setup summary".to_string()),
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
        conversation_id: None,
        run_prompt: format!("Implement item {index} from the migration spec."),
        prompt_author: AutomationPromptAuthor::SetupAgent,
        base_ref_kind: "local_branch".to_string(),
        base_ref_used: "main".to_string(),
        base_from_run_id: None,
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
    let automation = automation_with_goal_items(Some(goal_items_json()));
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
fn rejects_previous_pr_head_without_stacked_mode() {
    let automation = automation_with_goal_items(Some(goal_items_json()));
    let run = automation_run(1, AutomationRunStatus::Merged);
    let output = valid_continue_output().replace("automation_base", "previous_pr_head");

    let error =
        parse_automation_judge_verdict(&output, validation_context(&automation, &run)).unwrap_err();

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
fn detects_continue_loop_when_prompt_repeats_after_non_merged_run() {
    let automation = automation_with_goal_items(Some(goal_items_json()));
    let mut run = automation_run(1, AutomationRunStatus::AgentFailed);
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
