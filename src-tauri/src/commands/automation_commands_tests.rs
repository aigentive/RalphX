use chrono::Utc;
use serde_json::json;
use std::process::Command;

use super::automation_commands::{
    automation_service, create_automation_draft_for_state, parse_automation_id,
    parse_automation_run_id, parse_project_id, trigger_automation_run_now_for_state, trim_optional,
    AutomationRunScopedInput, CreateAutomationDraftInput, UpdateAutomationSettingsInput,
};
use crate::application::automation::api::{
    automation_detail_response_for_state, AutomationResponse, AutomationRunResponse,
    AutomationScheduleResponse,
};
use crate::application::automation::service::{AutomationDetail, AutomationScheduleOutcome};
use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspaceMode, AgentRun, Automation, AutomationId, AutomationJudgeState,
    AutomationPlanApprovalMode, AutomationPlanJudgeState, AutomationPrMergeMode,
    AutomationPromptAuthor, AutomationRun, AutomationRunId, AutomationRunStatus, AutomationStatus,
    ChatContextType, ChatConversationId, Project, ProjectId,
};
use crate::error::AppError;

fn automation() -> Automation {
    let now = Utc::now();
    Automation {
        id: AutomationId::from_string("automation-1"),
        project_id: ProjectId::from_string("project-1".to_string()),
        name: "Automation 1".to_string(),
        status: AutomationStatus::Draft,
        paused_reason_code: None,
        paused_reason_detail: None,
        goal_prompt: "Goal".to_string(),
        setup_conversation_id: None,
        provider_harness: "claude".to_string(),
        model_id: "sonnet".to_string(),
        logical_effort: None,
        run_mode: "edit".to_string(),
        base_ref_kind: "project_default".to_string(),
        base_ref: String::new(),
        base_display_name: None,
        base_source_pull_request_json: None,
        goal_items_json: Some(
            r#"[{"id":"phase-1","title":"Run 1","status":"pending"}]"#.to_string(),
        ),
        chain_mode: "merged_base".to_string(),
        completion_signal: "pr_merged".to_string(),
        plan_approval_mode: AutomationPlanApprovalMode::Manual,
        pr_merge_mode: AutomationPrMergeMode::Manual,
        plan_deep_verification: false,
        max_runs: 25,
        max_consecutive_failures: 3,
        first_run_prompt: Some("Run 1".to_string()),
        setup_analysis_summary: None,
        spec_artifact_id: None,
        created_at: now,
        updated_at: now,
    }
}

fn automation_run(automation_id: &AutomationId) -> AutomationRun {
    let now = Utc::now();
    AutomationRun {
        id: AutomationRunId::from_string("run-1"),
        automation_id: automation_id.clone(),
        run_index: 1,
        status: AutomationRunStatus::Merged,
        judge_state: AutomationJudgeState::Done,
        judge_lease_expires_at: None,
        plan_judge_state: AutomationPlanJudgeState::None,
        plan_judge_lease_expires_at: None,
        plan_judge_verdict_json: None,
        plan_revision_round: 0,
        plan_reminder_count: 0,
        plan_pending_instructions: None,
        plan_last_parked_artifact_id: None,
        agent_phase_started_at: None,
        conversation_id: None,
        run_prompt: "Run 1 prompt".to_string(),
        prompt_author: AutomationPromptAuthor::SetupAgent,
        base_ref_kind: "project_default".to_string(),
        base_ref_used: String::new(),
        base_from_run_id: None,
        branch_name: None,
        pr_number: Some(593),
        pr_url: None,
        pr_title: None,
        pr_head_ref_name: None,
        pr_base_ref_name: Some("main".to_string()),
        pr_merged_at: None,
        merge_commit_sha: None,
        diff_stats_json: None,
        agent_summary: None,
        judge_verdict_json: Some(continue_verdict(
            "Implement the next automation item with focused tests and publish the follow-up PR.",
        )),
        judge_model_id: Some("haiku".to_string()),
        error_code: None,
        error_detail: None,
        signal_check_failures: 0,
        started_at: Some(now),
        finished_at: Some(now),
        created_at: now,
        updated_at: now,
    }
}

fn continue_verdict(next_prompt: &str) -> String {
    json!({
        "decision": "continue",
        "goalMet": false,
        "reason": "The next item remains and should be implemented in a scoped PR.",
        "confidence": 0.87,
        "goalProgress": { "completedItems": 1, "totalItems": 2, "summary": "One item complete." },
        "updatedItemStatuses": null,
        "nextRunPrompt": next_prompt,
        "nextBaseBranch": "automation_base"
    })
    .to_string()
}

fn git(repo: &std::path::Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git command should spawn");
    assert!(
        output.status.success(),
        "git {:?} failed: {}{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn setup_git_project() -> (tempfile::TempDir, Project) {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    std::fs::create_dir_all(&repo_path).expect("repo root should be created");
    git(&repo_path, &["init", "-b", "main"]);
    git(&repo_path, &["config", "user.email", "test@example.com"]);
    git(&repo_path, &["config", "user.name", "Test User"]);
    std::fs::write(repo_path.join("README.md"), "hello\n").expect("fixture file should be written");
    git(&repo_path, &["add", "README.md"]);
    git(&repo_path, &["commit", "-m", "initial"]);

    let mut project = Project::new(
        "Automation Workspace".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.id = ProjectId::from_string("project-1".to_string());
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
    (temp, project)
}

#[test]
fn command_inputs_accept_camel_case_wrapped_payloads() {
    let input: UpdateAutomationSettingsInput = serde_json::from_value(json!({
        "id": "automation-1",
        "maxRuns": 12,
        "maxConsecutiveFailures": 4,
        "planApprovalMode": "automatic",
        "prMergeMode": "automatic",
        "planDeepVerification": true
    }))
    .unwrap();

    assert_eq!(input.max_runs, Some(12));
    assert_eq!(input.max_consecutive_failures, Some(4));
    assert_eq!(input.plan_approval_mode.as_deref(), Some("automatic"));
    assert_eq!(input.pr_merge_mode.as_deref(), Some("automatic"));
    assert_eq!(input.plan_deep_verification, Some(true));

    let run_input: AutomationRunScopedInput = serde_json::from_value(json!({
        "id": "automation-1",
        "runId": "automation-run-1"
    }))
    .unwrap();
    assert_eq!(run_input.run_id, "automation-run-1");
}

#[test]
fn automation_response_serializes_with_api_layer_snake_case() {
    let value = serde_json::to_value(AutomationResponse::from(automation())).unwrap();

    assert_eq!(value["project_id"], "project-1");
    assert_eq!(value["max_runs"], 25);
    assert!(value.get("projectId").is_none());
    assert!(value.get("maxRuns").is_none());
}

#[test]
fn automation_run_response_derives_plan_revision_pending() {
    let mut run = automation_run(&AutomationId::from_string("automation-1"));
    run.plan_pending_instructions = Some("Revise the plan before approval.".to_string());

    let value = serde_json::to_value(AutomationRunResponse::from(run)).unwrap();

    assert_eq!(value["plan_revision_pending"], true);
    assert!(value.get("planRevisionPending").is_none());
}

#[test]
fn automation_schedule_response_serializes_with_api_layer_snake_case() {
    let value = serde_json::to_value(AutomationScheduleResponse::from(
        AutomationScheduleOutcome {
            scheduled: false,
            reason: Some("deferred".to_string()),
        },
    ))
    .unwrap();

    assert_eq!(value["scheduled"], false);
    assert_eq!(value["reason"], "deferred");
}

#[test]
fn command_helpers_trim_inputs_and_reject_empty_ids() {
    assert_eq!(
        trim_optional(Some("  project-1  ".to_string())).as_deref(),
        Some("project-1")
    );
    assert!(trim_optional(Some("   ".to_string())).is_none());
    assert_eq!(
        parse_automation_id(" automation-1 ").unwrap().as_str(),
        "automation-1"
    );
    assert_eq!(
        parse_automation_run_id(" run-1 ").unwrap().as_str(),
        "run-1"
    );
    assert_eq!(
        parse_project_id(" project-1 ").unwrap().as_str(),
        "project-1"
    );
    assert_eq!(
        parse_automation_id(" ").unwrap_err(),
        "automation id is required"
    );
    assert_eq!(
        parse_automation_run_id("").unwrap_err(),
        "automation run id is required"
    );
    assert_eq!(parse_project_id("").unwrap_err(), "project id is required");
}

#[tokio::test]
async fn automation_detail_response_aggregates_usage_from_run_conversations() {
    let state = AppState::new_test();
    let automation = automation();
    let conversation_id = ChatConversationId::from_string("conversation-1");
    let mut run = automation_run(&automation.id);
    run.conversation_id = Some(conversation_id);

    let mut first_agent_run = AgentRun::new(conversation_id.clone());
    first_agent_run.input_tokens = Some(120);
    first_agent_run.output_tokens = Some(30);
    first_agent_run.cache_creation_tokens = Some(7);
    first_agent_run.cache_read_tokens = Some(9);
    first_agent_run.estimated_usd = Some(0.04);
    state.agent_run_repo.create(first_agent_run).await.unwrap();

    let mut second_agent_run = AgentRun::new(conversation_id);
    second_agent_run.input_tokens = Some(80);
    second_agent_run.output_tokens = Some(20);
    second_agent_run.estimated_usd = Some(0.02);
    state.agent_run_repo.create(second_agent_run).await.unwrap();

    let response = automation_detail_response_for_state(
        AutomationDetail {
            automation,
            runs: vec![run],
        },
        &state,
    )
    .await
    .unwrap();

    assert_eq!(response.usage.input_tokens, 200);
    assert_eq!(response.usage.output_tokens, 50);
    assert_eq!(response.usage.cache_creation_tokens, 7);
    assert_eq!(response.usage.cache_read_tokens, 9);
    assert_eq!(response.usage.estimated_usd, Some(0.06));
}

#[tokio::test]
async fn create_draft_creates_bound_setup_conversation_with_automation_workspace_base() {
    let state = AppState::new_test();
    let (_temp, project) = setup_git_project();
    state.project_repo.create(project).await.unwrap();

    let response = create_automation_draft_for_state(
        CreateAutomationDraftInput {
            project_id: "project-1".to_string(),
            name: Some("Nightly cleanup".to_string()),
        },
        &state,
    )
    .await
    .unwrap();

    let setup_conversation_id = response
        .setup_conversation_id
        .as_deref()
        .expect("draft response should expose setup conversation id");
    assert_eq!(
        response.automation.setup_conversation_id.as_deref(),
        Some(setup_conversation_id)
    );
    assert_eq!(response.automation.name, "Nightly cleanup");

    let automation_id = AutomationId::from_string(response.automation.id.clone());
    let persisted = state
        .automation_repo
        .get_by_id(&automation_id)
        .await
        .unwrap()
        .expect("automation should be persisted");
    let setup_conversation_id = ChatConversationId::from_string(setup_conversation_id.to_string());
    assert_eq!(persisted.setup_conversation_id, Some(setup_conversation_id));
    assert_eq!(persisted.base_ref_kind, "local_branch");
    assert!(
        persisted
            .base_ref
            .starts_with("ralphx/automation-workspace/automation-"),
        "automation setup branch should be automation-scoped, got {}",
        persisted.base_ref
    );
    let expected_display_name = format!("Automation branch ({})", persisted.base_ref);
    assert_eq!(
        persisted.base_display_name.as_deref(),
        Some(expected_display_name.as_str())
    );

    let setup_conversation = state
        .chat_conversation_repo
        .get_by_id(&setup_conversation_id)
        .await
        .unwrap()
        .expect("setup conversation should be persisted");
    assert_eq!(setup_conversation.context_type, ChatContextType::Project);
    assert_eq!(setup_conversation.context_id, "project-1");
    assert_eq!(setup_conversation.title.as_deref(), Some("Nightly cleanup"));
    assert_eq!(
        setup_conversation.agent_mode,
        Some(AgentConversationWorkspaceMode::Automation)
    );
    assert_eq!(setup_conversation.automation_id, Some(automation_id));
    assert!(setup_conversation.automation_run_id.is_none());

    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&setup_conversation_id)
        .await
        .unwrap()
        .expect("setup conversations should create a workspace");
    assert_eq!(workspace.mode, AgentConversationWorkspaceMode::Automation);
    assert_eq!(workspace.branch_name, persisted.base_ref);
    assert_eq!(workspace.base_ref, "main");
    assert!(
        std::path::Path::new(&workspace.worktree_path).is_dir(),
        "automation setup workspace path should exist"
    );
}

#[tokio::test]
async fn create_draft_cleans_setup_conversation_when_draft_validation_fails() {
    let state = AppState::new_test();

    let error = create_automation_draft_for_state(
        CreateAutomationDraftInput {
            project_id: "project-1".to_string(),
            name: Some("   ".to_string()),
        },
        &state,
    )
    .await
    .unwrap_err();

    assert!(error.contains("automation name cannot be empty"));
    let conversations = state
        .chat_conversation_repo
        .get_by_context(ChatContextType::Project, "project-1")
        .await
        .unwrap();
    assert!(conversations.is_empty());
    let automations = state
        .automation_repo
        .list(Some(ProjectId::from_string("project-1".to_string())))
        .await
        .unwrap();
    assert!(automations.is_empty());
}

#[tokio::test]
async fn finalize_command_activates_configured_draft() {
    let state = AppState::new_test();
    let automation = automation();
    assert_eq!(automation.status, AutomationStatus::Draft);
    state
        .automation_repo
        .create(automation.clone())
        .await
        .unwrap();

    let finalized = automation_service(&state)
        .finalize(&automation.id)
        .await
        .unwrap();
    assert_eq!(finalized.status, AutomationStatus::Active);

    let persisted = state
        .automation_repo
        .get_by_id(&automation.id)
        .await
        .unwrap()
        .expect("automation should be persisted");
    assert_eq!(persisted.status, AutomationStatus::Active);
}

#[tokio::test]
async fn finalize_command_rejects_unconfigured_draft() {
    let state = AppState::new_test();
    let mut automation = automation();
    automation.goal_prompt = String::new();
    state
        .automation_repo
        .create(automation.clone())
        .await
        .unwrap();

    let error = automation_service(&state)
        .finalize(&automation.id)
        .await
        .unwrap_err();
    assert!(matches!(error, AppError::Validation(_)));
    assert!(error
        .to_string()
        .contains("automation goal_prompt is required before approval"));

    let persisted = state
        .automation_repo
        .get_by_id(&automation.id)
        .await
        .unwrap()
        .expect("automation should be persisted");
    assert_eq!(persisted.status, AutomationStatus::Draft);
}

#[tokio::test]
async fn run_now_command_applies_stored_verdict_without_deferred_placeholder() {
    let state = AppState::new_test();
    let mut automation = automation();
    automation.status = AutomationStatus::Active;
    state
        .automation_repo
        .create(automation.clone())
        .await
        .unwrap();
    state
        .automation_run_repo
        .create_run(automation_run(&automation.id))
        .await
        .unwrap();

    let outcome = trigger_automation_run_now_for_state(&automation.id, &state)
        .await
        .unwrap();

    assert!(outcome.scheduled);
    assert!(outcome.reason.is_none());
    let runs = state
        .automation_run_repo
        .list_for_automation(&automation.id)
        .await
        .unwrap();
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[1].prompt_author, AutomationPromptAuthor::Judge);
    assert_eq!(
        runs[1].run_prompt,
        "Implement the next automation item with focused tests and publish the follow-up PR."
    );
}
