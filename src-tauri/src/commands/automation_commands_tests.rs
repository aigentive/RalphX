use async_trait::async_trait;
use chrono::Utc;
use serde_json::json;
use std::process::Command;

use super::automation_commands::{
    automation_service, create_automation_draft_for_state, delete_automation_run,
    parse_automation_id, parse_automation_run_id, parse_project_id, resume_automation_run,
    trigger_automation_run_now_for_state, trim_optional, AutomationRunScopedInput,
    CreateAutomationDraftInput, UpdateAutomationSettingsInput,
};
use crate::application::agent_conversation_start_service::AgentWorkspaceSourcePullRequestInput;
use crate::application::automation::api::{
    automation_detail_response_for_state, automation_run_response_for_state, AutomationResponse,
    AutomationRunResponse, AutomationScheduleResponse,
};
use crate::application::automation::service::{AutomationDetail, AutomationScheduleOutcome};
use crate::application::git_service::GitService;
use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceBranchMode,
    AgentConversationWorkspaceMode, AgentRun, ArtifactId, Automation, AutomationId,
    AutomationJudgeState, AutomationPlanApprovalMode, AutomationPlanJudgeState,
    AutomationPrMergeMode, AutomationPromptAuthor, AutomationRun, AutomationRunId,
    AutomationRunStatus, AutomationStatus, ChatContextType, ChatConversationId,
    IdeationAnalysisBaseRefKind, IdeationSession, IdeationSessionId, IdeationSessionStatus,
    InternalStatus, Project, ProjectId, Task,
};
use crate::domain::repositories::{PlanArtifactApproval, PlanArtifactApprovalRepository};
use crate::error::AppError;
use tauri::Manager;

struct FailingPlanApprovalRepository;

#[async_trait]
impl PlanArtifactApprovalRepository for FailingPlanApprovalRepository {
    async fn get_by_session(
        &self,
        _session_id: &IdeationSessionId,
    ) -> crate::error::AppResult<Option<PlanArtifactApproval>> {
        Err(AppError::Database(
            "approval repository unavailable".to_string(),
        ))
    }

    async fn delete_by_session(
        &self,
        _session_id: &IdeationSessionId,
    ) -> crate::error::AppResult<usize> {
        Ok(0)
    }
}

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
        authoring_state_json: None,
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
        plan_last_parked_blueprint_artifact_id: None,
        agent_phase_started_at: None,
        conversation_id: None,
        run_prompt: "Run 1 prompt".to_string(),
        prompt_author: AutomationPromptAuthor::SetupAgent,
        base_ref_kind: "project_default".to_string(),
        base_ref_used: String::new(),
        base_from_run_id: None,
        goal_item_id: None,
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

async fn link_run_to_plan_session(
    state: &AppState,
    automation: &Automation,
    conversation_id: &ChatConversationId,
    workspace_mode: AgentConversationWorkspaceMode,
    artifact_id: Option<&str>,
) -> IdeationSessionId {
    let session_id = IdeationSessionId::from_string("plan-session-1");
    let mut session = IdeationSession::new(automation.project_id.clone());
    session.id = session_id.clone();
    session.plan_artifact_id = artifact_id.map(ArtifactId::from_string);
    session.plan_blueprint_artifact_id =
        artifact_id.map(|artifact_id| ArtifactId::from_string(format!("{artifact_id}-blueprint")));
    state.ideation_session_repo.create(session).await.unwrap();

    let mut workspace = AgentConversationWorkspace::new(
        conversation_id.clone(),
        automation.project_id.clone(),
        workspace_mode,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        String::new(),
        None,
        None,
        "ralphx/automation-run-1".to_string(),
        "/tmp/ralphx-automation-run-1".to_string(),
    );
    workspace.linked_ideation_session_id = Some(session_id.clone());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();
    session_id
}

async fn insert_plan_approval(
    state: &AppState,
    session_id: &IdeationSessionId,
    artifact_id: &str,
    version: i64,
    approved_at: &str,
    approved_by: &str,
) {
    let session_id = session_id.as_str().to_string();
    let artifact_id = artifact_id.to_string();
    let blueprint_artifact_id = format!("{artifact_id}-blueprint");
    let approved_at = approved_at.to_string();
    let approved_by = approved_by.to_string();
    state
        .db
        .run(move |conn| {
            conn.execute(
                "INSERT INTO plan_artifact_approvals (
                    session_id, artifact_id, artifact_version,
                    blueprint_artifact_id, blueprint_artifact_version,
                    status, approved_at, approved_by
                 ) VALUES (?1, ?2, ?3, ?4, ?3, 'approved', ?5, ?6)",
                rusqlite::params![
                    session_id,
                    artifact_id,
                    version,
                    blueprint_artifact_id,
                    approved_at,
                    approved_by
                ],
            )
            .map(|_| ())
            .map_err(AppError::from)
        })
        .await
        .unwrap();
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

    let draft_input: CreateAutomationDraftInput = serde_json::from_value(json!({
        "projectId": "project-1",
        "baseRefKind": "local_branch",
        "baseBranchMode": "linked",
        "baseRef": "feature/automation-base",
        "baseDisplayName": "feature/automation-base",
        "baseSourcePullRequest": {
            "number": 42,
            "url": "https://github.com/example/repo/pull/42",
            "title": "Automation base",
            "headRefName": "feature/automation-base",
            "baseRefName": "release",
            "headRefOid": "abc123"
        }
    }))
    .unwrap();
    assert_eq!(draft_input.base_ref_kind.as_deref(), Some("local_branch"));
    assert_eq!(draft_input.base_branch_mode.as_deref(), Some("linked"));
    assert_eq!(
        draft_input.base_ref.as_deref(),
        Some("feature/automation-base")
    );
    assert_eq!(
        draft_input
            .base_source_pull_request
            .as_ref()
            .map(|pull_request| pull_request.number),
        Some(42)
    );

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
    assert!(value["base_target_ref"].is_null());
    assert!(value["base_target_display_name"].is_null());
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
async fn automation_detail_response_exposes_integration_fork_point_only_for_local_branch() {
    let state = AppState::new_test();
    let setup_conversation_id = ChatConversationId::from_string("automation-setup");
    let setup_workspace = AgentConversationWorkspace::new(
        setup_conversation_id.clone(),
        ProjectId::from_string("project-1".to_string()),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        None,
        "ralphx/ralphx/automation-abc".to_string(),
        "/tmp/ralphx-automation-abc".to_string(),
    );
    state
        .agent_conversation_workspace_repo
        .create_or_update(setup_workspace)
        .await
        .unwrap();

    let mut integration_automation = automation();
    integration_automation.setup_conversation_id = Some(setup_conversation_id.clone());
    integration_automation.base_ref_kind = "local_branch".to_string();
    integration_automation.base_ref = "ralphx/ralphx/automation-abc".to_string();
    let integration_response = automation_detail_response_for_state(
        AutomationDetail {
            automation: integration_automation,
            runs: Vec::new(),
        },
        &state,
    )
    .await
    .unwrap();

    assert_eq!(
        integration_response.automation.base_target_ref.as_deref(),
        Some("main")
    );
    assert_eq!(
        integration_response
            .automation
            .base_target_display_name
            .as_deref(),
        Some("Project default (main)")
    );
    assert_ne!(
        integration_response.automation.base_target_ref.as_deref(),
        Some("ralphx/ralphx/automation-abc")
    );

    let mut project_default_automation = automation();
    project_default_automation.setup_conversation_id = Some(setup_conversation_id);
    project_default_automation.base_ref = "main".to_string();
    let project_default_response = automation_detail_response_for_state(
        AutomationDetail {
            automation: project_default_automation,
            runs: Vec::new(),
        },
        &state,
    )
    .await
    .unwrap();

    assert!(project_default_response
        .automation
        .base_target_ref
        .is_none());
    assert!(project_default_response
        .automation
        .base_target_display_name
        .is_none());
}

#[tokio::test]
async fn automation_detail_response_exposes_ideation_task_graph_progress() {
    let state = AppState::new_test();
    let mut automation = automation();
    automation.run_mode = "ideation".to_string();
    automation.completion_signal = "ideation_finalized".to_string();
    let conversation_id = ChatConversationId::new();
    let mut run = automation_run(&automation.id);
    run.conversation_id = Some(conversation_id.clone());
    let session_id = link_run_to_plan_session(
        &state,
        &automation,
        &conversation_id,
        AgentConversationWorkspaceMode::Ideation,
        Some("plan-artifact-1"),
    )
    .await;
    state
        .ideation_session_repo
        .update_status(&session_id, IdeationSessionStatus::Accepted)
        .await
        .unwrap();

    let mut merged = Task::new(automation.project_id.clone(), "Backend".to_string());
    merged.ideation_session_id = Some(session_id.clone());
    merged.internal_status = InternalStatus::Merged;
    let merged = state.task_repo.create(merged).await.unwrap();
    let mut ready = Task::new(automation.project_id.clone(), "Frontend".to_string());
    ready.ideation_session_id = Some(session_id);
    ready.internal_status = InternalStatus::Ready;
    let ready = state.task_repo.create(ready).await.unwrap();
    state
        .task_dependency_repo
        .add_dependency(&ready.id, &merged.id)
        .await
        .unwrap();

    let response = automation_detail_response_for_state(
        AutomationDetail {
            automation,
            runs: vec![run],
        },
        &state,
    )
    .await
    .unwrap();
    let pipeline = response.pipeline.expect("ideation pipeline progress");

    assert_eq!(pipeline.deliverable, "task_graph");
    assert_eq!(pipeline.status, "executing");
    assert_eq!(pipeline.task_total, 2);
    assert_eq!(pipeline.task_merged, 1);
    assert_eq!(pipeline.task_terminal, 1);
    let frontend = pipeline
        .tasks
        .iter()
        .find(|task| task.id == ready.id.to_string())
        .expect("frontend task");
    assert_eq!(frontend.blocked_by, vec![merged.id.to_string()]);
}

#[tokio::test]
async fn automation_detail_response_exposes_open_run_plan_gate_fields() {
    let state = AppState::new_sqlite_test();
    let automation = automation();
    let conversation_id = ChatConversationId::from_string("conversation-plan-open");
    let mut run = automation_run(&automation.id);
    run.status = AutomationRunStatus::AwaitingPlanApproval;
    run.judge_state = AutomationJudgeState::None;
    run.conversation_id = Some(conversation_id.clone());

    let session_id = link_run_to_plan_session(
        &state,
        &automation,
        &conversation_id,
        AgentConversationWorkspaceMode::Plan,
        Some("plan-artifact-1"),
    )
    .await;
    insert_plan_approval(
        &state,
        &session_id,
        "plan-artifact-1",
        3,
        "2026-07-09T13:45:00Z",
        "judge",
    )
    .await;

    let response = automation_detail_response_for_state(
        AutomationDetail {
            automation,
            runs: vec![run],
        },
        &state,
    )
    .await
    .unwrap();
    let run = &response.runs[0];

    assert!(run.plan_phase);
    assert_eq!(run.plan_artifact_id.as_deref(), Some("plan-artifact-1"));
    assert_eq!(run.plan_approved_by.as_deref(), Some("judge"));
    assert_eq!(run.plan_approved_artifact_version, Some(3));
    assert_eq!(
        run.plan_approved_at.as_deref(),
        Some("2026-07-09T13:45:00Z")
    );
}

#[tokio::test]
async fn automation_detail_response_falls_back_to_parked_plan_artifact() {
    let state = AppState::new_test();
    let automation = automation();
    let mut run = automation_run(&automation.id);
    run.id = AutomationRunId::from_string("run-3");
    run.run_index = 3;
    run.status = AutomationRunStatus::AwaitingPlanApproval;
    run.judge_state = AutomationJudgeState::None;
    run.prompt_author = AutomationPromptAuthor::Judge;
    run.conversation_id = Some(ChatConversationId::from_string(
        "conversation-plan-without-workspace",
    ));
    run.plan_last_parked_artifact_id = Some("plan-artifact-parked".to_string());

    let response = automation_detail_response_for_state(
        AutomationDetail {
            automation,
            runs: vec![run],
        },
        &state,
    )
    .await
    .unwrap();
    let run = &response.runs[0];

    assert!(!run.plan_phase);
    assert_eq!(
        run.plan_artifact_id.as_deref(),
        Some("plan-artifact-parked")
    );
    assert!(run.plan_approved_by.is_none());
    assert!(run.plan_approved_artifact_version.is_none());
    assert!(run.plan_approved_at.is_none());
}

#[tokio::test]
async fn automation_run_response_falls_back_to_parked_plan_artifact_without_conversation() {
    let state = AppState::new_test();
    let automation = automation();
    let mut run = automation_run(&automation.id);
    run.id = AutomationRunId::from_string("run-4");
    run.run_index = 4;
    run.status = AutomationRunStatus::AwaitingPlanApproval;
    run.judge_state = AutomationJudgeState::None;
    run.conversation_id = None;
    run.plan_last_parked_artifact_id = Some("plan-artifact-parked".to_string());

    let response = automation_run_response_for_state(run, &state)
        .await
        .unwrap();

    assert!(!response.plan_phase);
    assert_eq!(
        response.plan_artifact_id.as_deref(),
        Some("plan-artifact-parked")
    );
    assert!(response.plan_approved_by.is_none());
    assert!(response.plan_approved_artifact_version.is_none());
    assert!(response.plan_approved_at.is_none());
}

#[tokio::test]
async fn automation_detail_response_keeps_terminal_run_plan_artifact_auditable_only() {
    let state = AppState::new_sqlite_test();
    let automation = automation();
    let conversation_id = ChatConversationId::from_string("conversation-plan-terminal");
    let mut run = automation_run(&automation.id);
    run.status = AutomationRunStatus::Merged;
    run.judge_state = AutomationJudgeState::Done;
    run.conversation_id = Some(conversation_id.clone());

    let session_id = link_run_to_plan_session(
        &state,
        &automation,
        &conversation_id,
        AgentConversationWorkspaceMode::Plan,
        Some("plan-artifact-1"),
    )
    .await;
    insert_plan_approval(
        &state,
        &session_id,
        "plan-artifact-1",
        3,
        "2026-07-09T13:45:00Z",
        "user",
    )
    .await;

    let response = automation_detail_response_for_state(
        AutomationDetail {
            automation,
            runs: vec![run],
        },
        &state,
    )
    .await
    .unwrap();
    let run = &response.runs[0];

    assert!(!run.plan_phase);
    assert_eq!(run.plan_artifact_id.as_deref(), Some("plan-artifact-1"));
    assert!(run.plan_approved_by.is_none());
    assert!(run.plan_approved_artifact_version.is_none());
    assert!(run.plan_approved_at.is_none());
}

#[tokio::test]
async fn automation_cancel_run_response_keeps_plan_artifact_auditable() {
    let state = AppState::new_sqlite_test();
    let mut automation = automation();
    automation.status = AutomationStatus::Active;
    let conversation_id = ChatConversationId::from_string("conversation-plan-cancelled");
    let mut run = automation_run(&automation.id);
    run.status = AutomationRunStatus::Running;
    run.judge_state = AutomationJudgeState::None;
    run.conversation_id = Some(conversation_id.clone());

    state
        .automation_repo
        .create(automation.clone())
        .await
        .unwrap();
    state
        .automation_run_repo
        .create_run(run.clone())
        .await
        .unwrap();
    link_run_to_plan_session(
        &state,
        &automation,
        &conversation_id,
        AgentConversationWorkspaceMode::Plan,
        Some("plan-artifact-1"),
    )
    .await;

    let cancelled = automation_service(&state)
        .cancel_run(&automation.id, &run.id)
        .await
        .unwrap();
    let response = automation_run_response_for_state(cancelled, &state)
        .await
        .unwrap();

    assert_eq!(response.status, "cancelled");
    assert_eq!(
        response.plan_artifact_id.as_deref(),
        Some("plan-artifact-1")
    );
    assert!(!response.plan_phase);
    assert!(response.plan_approved_by.is_none());
}

#[tokio::test]
async fn automation_detail_response_fails_closed_when_open_run_approval_join_fails() {
    let mut state = AppState::new_test();
    state.plan_approval_repo = std::sync::Arc::new(FailingPlanApprovalRepository);
    let automation = automation();
    let conversation_id = ChatConversationId::from_string("conversation-plan-open");
    let mut run = automation_run(&automation.id);
    run.status = AutomationRunStatus::AwaitingPlanApproval;
    run.conversation_id = Some(conversation_id.clone());

    link_run_to_plan_session(
        &state,
        &automation,
        &conversation_id,
        AgentConversationWorkspaceMode::Plan,
        Some("plan-artifact-1"),
    )
    .await;

    let error = automation_detail_response_for_state(
        AutomationDetail {
            automation,
            runs: vec![run],
        },
        &state,
    )
    .await
    .unwrap_err();

    assert!(error
        .to_string()
        .contains("approval repository unavailable"));
}

#[tokio::test]
async fn create_draft_creates_bound_setup_conversation_from_selected_branch() {
    let state = AppState::new_test();
    let (_temp, project) = setup_git_project();
    let repo_path = std::path::Path::new(&project.working_directory);
    git(repo_path, &["checkout", "-b", "feature/automation-base"]);
    std::fs::write(repo_path.join("CUSTOM_BASE.md"), "custom automation base\n").unwrap();
    git(repo_path, &["add", "CUSTOM_BASE.md"]);
    git(repo_path, &["commit", "-m", "custom automation base"]);
    let selected_base_sha = GitService::get_branch_sha(repo_path, "feature/automation-base")
        .await
        .unwrap();
    let main_sha = GitService::get_branch_sha(repo_path, "main").await.unwrap();
    state.project_repo.create(project).await.unwrap();

    let response = create_automation_draft_for_state(
        CreateAutomationDraftInput {
            project_id: "project-1".to_string(),
            name: Some("Nightly cleanup".to_string()),
            authoring_mode: None,
            base_ref_kind: Some("current_branch".to_string()),
            base_branch_mode: Some("isolated".to_string()),
            base_ref: Some("feature/automation-base".to_string()),
            base_display_name: Some("Current branch (feature/automation-base)".to_string()),
            base_source_pull_request: None,
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
    assert_eq!(workspace.base_ref, "feature/automation-base");
    assert_eq!(
        workspace.base_commit.as_deref(),
        Some(selected_base_sha.as_str())
    );
    assert_ne!(workspace.base_commit.as_deref(), Some(main_sha.as_str()));
    assert!(
        std::path::Path::new(&workspace.worktree_path).is_dir(),
        "automation setup workspace path should exist"
    );
}

#[tokio::test]
async fn create_draft_preserves_linked_branch_selection() {
    let state = AppState::new_test();
    let (_temp, project) = setup_git_project();
    let repo_path = std::path::Path::new(&project.working_directory);
    git(repo_path, &["branch", "feature/linked-automation"]);
    state.project_repo.create(project).await.unwrap();

    let response = create_automation_draft_for_state(
        CreateAutomationDraftInput {
            project_id: "project-1".to_string(),
            name: Some("Linked automation".to_string()),
            authoring_mode: None,
            base_ref_kind: Some("local_branch".to_string()),
            base_branch_mode: Some("linked".to_string()),
            base_ref: Some("feature/linked-automation".to_string()),
            base_display_name: Some("feature/linked-automation".to_string()),
            base_source_pull_request: None,
        },
        &state,
    )
    .await
    .unwrap();

    let setup_conversation_id = ChatConversationId::from_string(
        response
            .setup_conversation_id
            .expect("draft should expose setup conversation id"),
    );
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&setup_conversation_id)
        .await
        .unwrap()
        .expect("linked setup workspace should be persisted");

    assert_eq!(
        workspace.branch_mode,
        AgentConversationWorkspaceBranchMode::Linked
    );
    assert_eq!(workspace.branch_name, "feature/linked-automation");
    assert_eq!(response.automation.base_ref, "feature/linked-automation");
}

#[tokio::test]
async fn create_draft_preserves_linked_pull_request_selection() {
    let state = AppState::new_test();
    let (_temp, project) = setup_git_project();
    let repo_path = std::path::Path::new(&project.working_directory);
    git(repo_path, &["checkout", "-b", "release"]);
    std::fs::write(repo_path.join("RELEASE.md"), "release base\n").unwrap();
    git(repo_path, &["add", "RELEASE.md"]);
    git(repo_path, &["commit", "-m", "release base"]);
    let release_sha = GitService::get_branch_sha(repo_path, "release")
        .await
        .unwrap();
    git(repo_path, &["checkout", "-b", "feature/pr-automation"]);
    git(repo_path, &["checkout", "main"]);
    state.project_repo.create(project).await.unwrap();

    let response = create_automation_draft_for_state(
        CreateAutomationDraftInput {
            project_id: "project-1".to_string(),
            name: Some("PR automation".to_string()),
            authoring_mode: None,
            base_ref_kind: Some("local_branch".to_string()),
            base_branch_mode: Some("linked".to_string()),
            base_ref: Some("feature/pr-automation".to_string()),
            base_display_name: Some("PR #42: Automation base".to_string()),
            base_source_pull_request: Some(AgentWorkspaceSourcePullRequestInput {
                number: 42,
                url: Some("https://github.com/example/repo/pull/42".to_string()),
                title: Some("Automation base".to_string()),
                head_ref_name: "feature/pr-automation".to_string(),
                base_ref_name: Some("release".to_string()),
                head_ref_oid: Some(release_sha.clone()),
            }),
        },
        &state,
    )
    .await
    .unwrap();

    let setup_conversation_id = ChatConversationId::from_string(
        response
            .setup_conversation_id
            .expect("draft should expose setup conversation id"),
    );
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&setup_conversation_id)
        .await
        .unwrap()
        .expect("linked PR setup workspace should be persisted");

    assert_eq!(workspace.base_ref, "release");
    assert_eq!(workspace.base_commit.as_deref(), Some(release_sha.as_str()));
    assert_eq!(workspace.publication_pr_number, Some(42));
    assert_eq!(
        workspace
            .source_pull_request
            .as_ref()
            .map(|pull_request| pull_request.head_ref_name.as_str()),
        Some("feature/pr-automation")
    );
}

#[tokio::test]
async fn create_draft_defaults_to_project_base_when_selection_is_omitted() {
    let state = AppState::new_test();
    let (_temp, project) = setup_git_project();
    state.project_repo.create(project).await.unwrap();

    let response = create_automation_draft_for_state(
        CreateAutomationDraftInput {
            project_id: "project-1".to_string(),
            name: Some("Default-base automation".to_string()),
            authoring_mode: None,
            base_ref_kind: None,
            base_branch_mode: None,
            base_ref: None,
            base_display_name: None,
            base_source_pull_request: None,
        },
        &state,
    )
    .await
    .unwrap();

    let setup_conversation_id = ChatConversationId::from_string(
        response
            .setup_conversation_id
            .expect("draft should expose setup conversation id"),
    );
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&setup_conversation_id)
        .await
        .unwrap()
        .expect("default setup workspace should be persisted");

    assert_eq!(workspace.base_ref, "main");
    assert_eq!(
        workspace.branch_mode,
        AgentConversationWorkspaceBranchMode::Isolated
    );
}

#[tokio::test]
async fn create_draft_cleans_setup_conversation_when_draft_validation_fails() {
    let state = AppState::new_test();

    let error = create_automation_draft_for_state(
        CreateAutomationDraftInput {
            project_id: "project-1".to_string(),
            name: Some("   ".to_string()),
            authoring_mode: None,
            base_ref_kind: None,
            base_branch_mode: None,
            base_ref: None,
            base_display_name: None,
            base_source_pull_request: None,
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

#[tokio::test]
async fn delete_automation_run_command_deletes_valid_target_and_rejects_empty_run_id() {
    let state = AppState::new_test();
    let mut stopped = automation();
    stopped.status = AutomationStatus::Stopped;
    let mut failed = automation_run(&stopped.id);
    failed.status = AutomationRunStatus::AgentFailed;
    failed.judge_state = AutomationJudgeState::Done;
    failed.conversation_id = None;
    failed.branch_name = None;
    state.automation_repo.create(stopped.clone()).await.unwrap();
    state
        .automation_run_repo
        .create_run(failed.clone())
        .await
        .unwrap();
    let app = tauri::test::mock_builder()
        .manage(state)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app should build");

    let empty_error = delete_automation_run(
        AutomationRunScopedInput {
            id: stopped.id.to_string(),
            run_id: "   ".to_string(),
        },
        app.state::<AppState>(),
    )
    .await
    .unwrap_err();
    assert_eq!(empty_error, "automation run id is required");
    assert!(app
        .state::<AppState>()
        .automation_run_repo
        .get_by_id(&failed.id)
        .await
        .unwrap()
        .is_some());

    delete_automation_run(
        AutomationRunScopedInput {
            id: format!("  {}  ", stopped.id),
            run_id: format!("  {}  ", failed.id),
        },
        app.state::<AppState>(),
    )
    .await
    .expect("command should delegate valid run deletion");

    assert!(app
        .state::<AppState>()
        .automation_run_repo
        .get_by_id(&failed.id)
        .await
        .unwrap()
        .is_none());
    assert!(app
        .state::<AppState>()
        .automation_repo
        .get_by_id(&stopped.id)
        .await
        .unwrap()
        .is_some());
}

#[tokio::test]
async fn resume_automation_run_command_maps_reopen_rejection_without_mutating_state() {
    let state = AppState::new_test();
    let mut paused = automation();
    paused.status = AutomationStatus::Paused;
    paused.paused_reason_code = Some("user_paused".to_string());
    let completed = automation_run(&paused.id);
    state.automation_repo.create(paused.clone()).await.unwrap();
    state
        .automation_run_repo
        .create_run(completed.clone())
        .await
        .unwrap();
    let app = tauri::test::mock_builder()
        .manage(state)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app should build");

    let error = resume_automation_run(
        AutomationRunScopedInput {
            id: paused.id.to_string(),
            run_id: completed.id.to_string(),
        },
        app.state::<AppState>(),
    )
    .await
    .unwrap_err();

    assert!(error.contains("only a failed run can be resumed"));
    assert_eq!(
        app.state::<AppState>()
            .automation_run_repo
            .get_by_id(&completed.id)
            .await
            .unwrap(),
        Some(completed)
    );
    assert_eq!(
        app.state::<AppState>()
            .automation_repo
            .get_by_id(&paused.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        AutomationStatus::Paused
    );
}
