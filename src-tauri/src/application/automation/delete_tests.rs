use std::sync::Arc;

use async_trait::async_trait;
use chrono::{Duration, Utc};
use ralphx_events::RecordingEventSink;

use crate::application::automation::delete::{
    delete_automation_run_with_archive, delete_automation_with_archive,
};
use crate::application::plan_artifact_approval::{
    DbPlanArtifactApprovalWriter, PlanArtifactApprovalWriter,
};
use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, Artifact, ArtifactId, ArtifactType,
    Automation, AutomationId, AutomationJudgeState, AutomationPlanApprovalMode,
    AutomationPlanJudgeState, AutomationPrMergeMode, AutomationPromptAuthor, AutomationRun,
    AutomationRunId, AutomationRunStatus, AutomationStatus, ChatConversation, ChatConversationId,
    IdeationAnalysisBaseRefKind, IdeationSession, IdeationSessionFlow, Project, ProjectId,
};
use crate::domain::repositories::{
    AutomationConfigPatch, AutomationRepository, AutomationSettingsPatch, PlanApprovalActor,
    PlanArtifactApprovalRepository,
};
use crate::domain::services::github_service::GithubServiceTrait;
use crate::error::{AppError, AppResult};
use crate::infrastructure::sqlite::SqlitePlanArtifactApprovalRepository;
use crate::tests::mock_github_service::MockGithubService;

fn project(temp: &tempfile::TempDir) -> Project {
    Project::new(
        "Delete automation project".to_string(),
        temp.path().to_string_lossy().to_string(),
    )
}

fn automation(id: &str, project_id: &ProjectId, status: AutomationStatus) -> Automation {
    let now = Utc::now();
    Automation {
        id: AutomationId::from_string(id),
        project_id: project_id.clone(),
        name: format!("Automation {id}"),
        status,
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

fn run_with_judge(
    id: &str,
    automation_id: &AutomationId,
    judge_state: AutomationJudgeState,
    judge_lease_expires_at: Option<chrono::DateTime<Utc>>,
) -> AutomationRun {
    let now = Utc::now();
    AutomationRun {
        id: AutomationRunId::from_string(id),
        automation_id: automation_id.clone(),
        run_index: 1,
        status: AutomationRunStatus::Merged,
        judge_state,
        judge_lease_expires_at,
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
        base_ref_kind: "local_branch".to_string(),
        base_ref_used: "main".to_string(),
        base_from_run_id: None,
        goal_item_id: None,
        branch_name: None,
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
        finished_at: Some(now),
        created_at: now,
        updated_at: now,
    }
}

fn deletable_run(
    id: &str,
    automation_id: &AutomationId,
    run_index: i64,
    status: AutomationRunStatus,
    judge_state: AutomationJudgeState,
) -> AutomationRun {
    let mut run = run_with_judge(id, automation_id, judge_state, None);
    run.run_index = run_index;
    run.status = status;
    run
}

/// AppState wired with an in-memory project and a mock GitHub service so
/// conversation archiving (stop-agent + optional PR close) runs cleanly.
async fn setup_state() -> (
    tempfile::TempDir,
    AppState,
    ProjectId,
    Arc<MockGithubService>,
) {
    let temp = tempfile::tempdir().expect("tempdir");
    let project = project(&temp);
    let project_id = project.id.clone();
    let github = Arc::new(MockGithubService::new());
    let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
    let mut state = AppState::new_test();
    state.github_service = Some(github_trait);
    state.project_repo.create(project).await.expect("project");
    (temp, state, project_id, github)
}

/// SQLite-backed artifact/session state for tests that need durable plan-gate cleanup.
async fn setup_state_sqlite() -> (
    tempfile::TempDir,
    AppState,
    ProjectId,
    Arc<MockGithubService>,
) {
    let temp = tempfile::tempdir().expect("tempdir");
    let project = project(&temp);
    let project_id = project.id.clone();
    let github = Arc::new(MockGithubService::new());
    let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
    let mut state = AppState::new_sqlite_test();
    state.github_service = Some(github_trait);
    state.project_repo.create(project).await.expect("project");
    (temp, state, project_id, github)
}

/// Persist a project conversation bound to an automation (and optionally a run).
async fn seed_conversation(
    state: &AppState,
    project_id: &ProjectId,
    automation_id: &AutomationId,
    run_id: Option<&AutomationRunId>,
    pre_archived: bool,
) -> ChatConversationId {
    let mut conversation = ChatConversation::new_project(project_id.clone());
    conversation.id = ChatConversationId::new();
    conversation.automation_id = Some(automation_id.clone());
    conversation.automation_run_id = run_id.cloned();
    if pre_archived {
        conversation.archive();
    }
    let created = state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("conversation persisted");
    created.id
}

async fn seed_run_with_workspace(
    state: &AppState,
    project_id: &ProjectId,
    automation_id: &AutomationId,
    mut run: AutomationRun,
    branch_name: &str,
    worktree_path: &std::path::Path,
) -> (AutomationRun, ChatConversationId) {
    let conversation_id =
        seed_conversation(state, project_id, automation_id, Some(&run.id), false).await;
    run.conversation_id = Some(conversation_id.clone());
    run.branch_name = Some(branch_name.to_string());
    state
        .automation_run_repo
        .create_run(run.clone())
        .await
        .expect("run persisted");

    let workspace = AgentConversationWorkspace::new(
        conversation_id.clone(),
        project_id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        Some("base-sha".to_string()),
        branch_name.to_string(),
        worktree_path.to_string_lossy().to_string(),
    );
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace persisted");

    (run, conversation_id)
}

#[tokio::test]
async fn delete_automation_run_latest_failed_cascades_only_target_and_resyncs_goals() {
    let (temp, mut state, project_id, github) = setup_state().await;
    let events = Arc::new(RecordingEventSink::new());
    state.events = events.clone();
    let mut stopped = automation(
        "automation-delete-latest-failed",
        &project_id,
        AutomationStatus::Stopped,
    );
    stopped.goal_items_json =
        Some(r#"[{"id":"phase-1","title":"Run 1","status":"in_progress"}]"#.to_string());
    state.automation_repo.create(stopped.clone()).await.unwrap();

    let previous = deletable_run(
        "run-delete-previous",
        &stopped.id,
        1,
        AutomationRunStatus::Completed,
        AutomationJudgeState::Done,
    );
    let (previous, previous_conversation) = seed_run_with_workspace(
        &state,
        &project_id,
        &stopped.id,
        previous,
        "ralphx/previous-run",
        &temp.path().join("missing-previous-worktree"),
    )
    .await;
    let previous_workspace_before = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&previous_conversation)
        .await
        .unwrap()
        .unwrap();

    let latest = deletable_run(
        "run-delete-latest",
        &stopped.id,
        2,
        AutomationRunStatus::AgentFailed,
        AutomationJudgeState::Failed,
    );
    let (latest, latest_conversation) = seed_run_with_workspace(
        &state,
        &project_id,
        &stopped.id,
        latest,
        "ralphx/latest-failed-run",
        &temp.path().join("missing-latest-worktree"),
    )
    .await;

    delete_automation_run_with_archive(&state, &stopped.id, &latest.id)
        .await
        .expect("latest failed run delete succeeds even when worktree is already gone");

    assert!(state
        .automation_run_repo
        .get_by_id(&latest.id)
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        state
            .automation_run_repo
            .get_by_id(&previous.id)
            .await
            .unwrap(),
        Some(previous.clone())
    );
    assert_eq!(
        state
            .automation_run_repo
            .latest_for_automation(&stopped.id)
            .await
            .unwrap()
            .map(|run| run.id),
        Some(previous.id.clone())
    );
    assert!(state
        .automation_run_repo
        .list_for_automation(&stopped.id)
        .await
        .unwrap()
        .iter()
        .all(|run| !crate::domain::entities::is_open_automation_run(run.status, run.judge_state)));

    let latest_conversation_after = state
        .chat_conversation_repo
        .get_by_id(&latest_conversation)
        .await
        .unwrap()
        .unwrap();
    assert!(latest_conversation_after.archived_at.is_some());
    let latest_workspace_after = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&latest_conversation)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        latest_workspace_after.status,
        crate::domain::entities::AgentConversationWorkspaceStatus::Archived
    );

    let previous_conversation_after = state
        .chat_conversation_repo
        .get_by_id(&previous_conversation)
        .await
        .unwrap()
        .unwrap();
    assert!(previous_conversation_after.archived_at.is_none());
    let previous_workspace_after = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&previous_conversation)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        previous_workspace_after.status,
        previous_workspace_before.status
    );
    assert_eq!(
        previous_workspace_after.branch_name,
        previous_workspace_before.branch_name
    );

    let automation_after = state
        .automation_repo
        .get_by_id(&stopped.id)
        .await
        .unwrap()
        .unwrap();
    assert!(automation_after
        .goal_items_json
        .as_deref()
        .is_some_and(|json| json.contains(r#""status":"pending""#)));
    assert_eq!(github.state().delete_remote_branch_calls, 1);
    assert_eq!(
        github.state().last_delete_remote_branch_name.as_deref(),
        Some("ralphx/latest-failed-run")
    );
    assert_eq!(
        events
            .events()
            .into_iter()
            .map(|event| event.event)
            .collect::<Vec<_>>(),
        vec!["automation:run:updated", "automation:updated"]
    );
}

#[tokio::test]
async fn delete_automation_run_rejects_non_latest_without_teardown() {
    let (temp, state, project_id, github) = setup_state().await;
    let stopped = automation(
        "automation-delete-non-latest",
        &project_id,
        AutomationStatus::Stopped,
    );
    state.automation_repo.create(stopped.clone()).await.unwrap();
    let target = deletable_run(
        "run-delete-non-latest-target",
        &stopped.id,
        1,
        AutomationRunStatus::AgentFailed,
        AutomationJudgeState::Done,
    );
    let (target, target_conversation) = seed_run_with_workspace(
        &state,
        &project_id,
        &stopped.id,
        target,
        "ralphx/non-latest",
        &temp.path().join("missing-non-latest-worktree"),
    )
    .await;
    let latest = deletable_run(
        "run-delete-newer",
        &stopped.id,
        2,
        AutomationRunStatus::Completed,
        AutomationJudgeState::Done,
    );
    state
        .automation_run_repo
        .create_run(latest.clone())
        .await
        .unwrap();

    let error = delete_automation_run_with_archive(&state, &stopped.id, &target.id)
        .await
        .unwrap_err();

    assert!(
        matches!(error, AppError::Conflict(message) if message == "only the latest run can be deleted")
    );
    assert!(state
        .automation_run_repo
        .get_by_id(&target.id)
        .await
        .unwrap()
        .is_some());
    assert!(state
        .automation_run_repo
        .get_by_id(&latest.id)
        .await
        .unwrap()
        .is_some());
    assert!(state
        .chat_conversation_repo
        .get_by_id(&target_conversation)
        .await
        .unwrap()
        .unwrap()
        .archived_at
        .is_none());
    assert_eq!(github.state().delete_remote_branch_calls, 0);
}

#[tokio::test]
async fn delete_automation_run_rejects_success_statuses_without_teardown() {
    for (suffix, status) in [
        ("completed", AutomationRunStatus::Completed),
        ("published", AutomationRunStatus::Published),
        ("merged", AutomationRunStatus::Merged),
    ] {
        let (temp, state, project_id, github) = setup_state().await;
        let stopped = automation(
            &format!("automation-delete-reject-{suffix}"),
            &project_id,
            AutomationStatus::Stopped,
        );
        state.automation_repo.create(stopped.clone()).await.unwrap();
        let run = deletable_run(
            &format!("run-delete-reject-{suffix}"),
            &stopped.id,
            1,
            status,
            AutomationJudgeState::Done,
        );
        let (run, conversation_id) = seed_run_with_workspace(
            &state,
            &project_id,
            &stopped.id,
            run,
            &format!("ralphx/reject-{suffix}"),
            &temp.path().join(format!("missing-{suffix}-worktree")),
        )
        .await;

        let error = delete_automation_run_with_archive(&state, &stopped.id, &run.id)
            .await
            .unwrap_err();

        assert!(
            matches!(error, AppError::Conflict(message) if message == format!("run status {} cannot be deleted", status.as_str()))
        );
        assert!(state
            .automation_run_repo
            .get_by_id(&run.id)
            .await
            .unwrap()
            .is_some());
        assert!(state
            .chat_conversation_repo
            .get_by_id(&conversation_id)
            .await
            .unwrap()
            .unwrap()
            .archived_at
            .is_none());
        assert_eq!(github.state().delete_remote_branch_calls, 0);
    }
}

#[tokio::test]
async fn delete_automation_run_running_is_cancelled_before_delete() {
    let (temp, state, project_id, _github) = setup_state().await;
    let mut stopped = automation(
        "automation-delete-running",
        &project_id,
        AutomationStatus::Stopped,
    );
    stopped.pr_merge_mode = AutomationPrMergeMode::Automatic;
    state.automation_repo.create(stopped.clone()).await.unwrap();
    let running = deletable_run(
        "run-delete-running",
        &stopped.id,
        1,
        AutomationRunStatus::Running,
        AutomationJudgeState::None,
    );
    let (running, conversation_id) = seed_run_with_workspace(
        &state,
        &project_id,
        &stopped.id,
        running,
        "ralphx/running-run",
        &temp.path().join("missing-running-worktree"),
    )
    .await;
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    state
        .agent_conversation_workspace_repo
        .update_pr_supervision_preferences(
            &conversation_id,
            workspace.pr_autofix_enabled,
            true,
            &workspace.pr_auto_merge_method,
        )
        .await
        .unwrap();

    delete_automation_run_with_archive(&state, &stopped.id, &running.id)
        .await
        .expect("running run is cancelled then deleted");

    assert!(state
        .automation_run_repo
        .get_by_id(&running.id)
        .await
        .unwrap()
        .is_none());
    let conversation_after = state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert!(conversation_after.archived_at.is_some());
    let workspace_after = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        !workspace_after.pr_auto_merge_desired,
        "cancel_run must disarm automatic merge before the row disappears"
    );
}

#[tokio::test]
async fn delete_automation_run_rejects_live_judge_lease_without_teardown() {
    let (temp, state, project_id, github) = setup_state().await;
    let stopped = automation(
        "automation-delete-live-judge",
        &project_id,
        AutomationStatus::Stopped,
    );
    state.automation_repo.create(stopped.clone()).await.unwrap();
    let mut run = deletable_run(
        "run-delete-live-judge",
        &stopped.id,
        1,
        AutomationRunStatus::AgentFailed,
        AutomationJudgeState::InProgress,
    );
    run.judge_lease_expires_at = Some(Utc::now() + Duration::minutes(5));
    let (run, conversation_id) = seed_run_with_workspace(
        &state,
        &project_id,
        &stopped.id,
        run,
        "ralphx/live-judge",
        &temp.path().join("missing-live-judge-worktree"),
    )
    .await;

    let error = delete_automation_run_with_archive(&state, &stopped.id, &run.id)
        .await
        .unwrap_err();

    assert!(
        matches!(error, AppError::Validation(message) if message.contains("judge is finalizing"))
    );
    assert!(state
        .automation_run_repo
        .get_by_id(&run.id)
        .await
        .unwrap()
        .is_some());
    assert!(state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .unwrap()
        .unwrap()
        .archived_at
        .is_none());
    assert_eq!(github.state().delete_remote_branch_calls, 0);
}

#[tokio::test]
async fn delete_automation_run_rejects_pre_execution_statuses_without_teardown() {
    for (suffix, status) in [
        ("pending", AutomationRunStatus::Pending),
        ("provisioning", AutomationRunStatus::Provisioning),
        (
            "awaiting-plan-approval",
            AutomationRunStatus::AwaitingPlanApproval,
        ),
    ] {
        let (temp, state, project_id, github) = setup_state().await;
        let stopped = automation(
            &format!("automation-delete-reject-{suffix}"),
            &project_id,
            AutomationStatus::Stopped,
        );
        state.automation_repo.create(stopped.clone()).await.unwrap();
        let run = deletable_run(
            &format!("run-delete-reject-{suffix}"),
            &stopped.id,
            1,
            status,
            AutomationJudgeState::None,
        );
        let (run, conversation_id) = seed_run_with_workspace(
            &state,
            &project_id,
            &stopped.id,
            run,
            &format!("ralphx/reject-{suffix}"),
            &temp.path().join(format!("missing-{suffix}-worktree")),
        )
        .await;

        let error = delete_automation_run_with_archive(&state, &stopped.id, &run.id)
            .await
            .unwrap_err();

        assert!(
            matches!(error, AppError::Conflict(message) if message == format!("run status {} cannot be deleted", status.as_str()))
        );
        assert!(state
            .automation_run_repo
            .get_by_id(&run.id)
            .await
            .unwrap()
            .is_some());
        assert!(state
            .chat_conversation_repo
            .get_by_id(&conversation_id)
            .await
            .unwrap()
            .unwrap()
            .archived_at
            .is_none());
        assert_eq!(github.state().close_pr_calls, 0);
        assert_eq!(github.state().delete_remote_branch_calls, 0);
    }
}

#[tokio::test]
async fn delete_automation_run_rejects_successor_inserted_before_authority_cas() {
    let (temp, state, project_id, github) = setup_state().await;
    let stopped = automation(
        "automation-delete-cas-race",
        &project_id,
        AutomationStatus::Stopped,
    );
    state.automation_repo.create(stopped.clone()).await.unwrap();
    let target = deletable_run(
        "run-delete-cas-race-target",
        &stopped.id,
        1,
        AutomationRunStatus::AgentFailed,
        AutomationJudgeState::Done,
    );
    let (target, conversation_id) = seed_run_with_workspace(
        &state,
        &project_id,
        &stopped.id,
        target,
        "ralphx/cas-race-target",
        &temp.path().join("missing-cas-race-worktree"),
    )
    .await;
    let successor = deletable_run(
        "run-delete-cas-race-successor",
        &stopped.id,
        2,
        AutomationRunStatus::Provisioning,
        AutomationJudgeState::None,
    );
    state
        .automation_run_repo
        .create_run(successor.clone())
        .await
        .unwrap();

    let error = delete_automation_run_with_archive(&state, &stopped.id, &target.id)
        .await
        .unwrap_err();

    assert!(
        matches!(error, AppError::Conflict(message) if message == "only the latest run can be deleted")
    );
    assert_eq!(
        state
            .automation_run_repo
            .delete_run_if_deletable(&stopped.id, &target.id)
            .await
            .unwrap(),
        0,
        "the authority CAS must independently reject a stale run"
    );
    assert!(state
        .automation_run_repo
        .get_by_id(&target.id)
        .await
        .unwrap()
        .is_some());
    assert_eq!(
        state
            .automation_run_repo
            .get_by_id(&successor.id)
            .await
            .unwrap(),
        Some(successor)
    );
    assert!(state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .unwrap()
        .unwrap()
        .archived_at
        .is_none());
    assert_eq!(github.state().close_pr_calls, 0);
    assert_eq!(github.state().delete_remote_branch_calls, 0);
}

#[tokio::test]
async fn delete_automation_run_second_delete_has_no_repeated_teardown() {
    let (temp, state, project_id, github) = setup_state().await;
    let stopped = automation(
        "automation-delete-twice",
        &project_id,
        AutomationStatus::Stopped,
    );
    state.automation_repo.create(stopped.clone()).await.unwrap();
    let run = deletable_run(
        "run-delete-twice",
        &stopped.id,
        1,
        AutomationRunStatus::AgentFailed,
        AutomationJudgeState::Done,
    );
    let (run, conversation_id) = seed_run_with_workspace(
        &state,
        &project_id,
        &stopped.id,
        run,
        "ralphx/delete-twice",
        &temp.path().join("missing-delete-twice-worktree"),
    )
    .await;
    let mut workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    workspace.publication_pr_number = Some(42);
    workspace.publication_pr_url = Some("https://github.com/mock/repo/pull/42".to_string());
    workspace.publication_pr_status = Some("open".to_string());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();

    delete_automation_run_with_archive(&state, &stopped.id, &run.id)
        .await
        .expect("first delete succeeds");
    let archive_timestamp = state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .unwrap()
        .unwrap()
        .archived_at;
    let close_calls = github.state().close_pr_calls;
    let branch_delete_calls = github.state().delete_remote_branch_calls;

    let error = delete_automation_run_with_archive(&state, &stopped.id, &run.id)
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        AppError::NotFound(_) | AppError::Conflict(_)
    ));
    assert_eq!(
        state
            .chat_conversation_repo
            .get_by_id(&conversation_id)
            .await
            .unwrap()
            .unwrap()
            .archived_at,
        archive_timestamp
    );
    assert_eq!(github.state().close_pr_calls, close_calls);
    assert_eq!(
        github.state().delete_remote_branch_calls,
        branch_delete_calls
    );
}

#[tokio::test]
async fn delete_automation_run_skips_project_default_branch_cleanup() {
    let (temp, state, project_id, github) = setup_state().await;
    let stopped = automation(
        "automation-delete-default-branch",
        &project_id,
        AutomationStatus::Stopped,
    );
    state.automation_repo.create(stopped.clone()).await.unwrap();
    let run = deletable_run(
        "run-delete-default-branch",
        &stopped.id,
        1,
        AutomationRunStatus::AgentFailed,
        AutomationJudgeState::Done,
    );
    let (run, conversation_id) = seed_run_with_workspace(
        &state,
        &project_id,
        &stopped.id,
        run,
        "main",
        &temp.path().join("missing-default-branch-worktree"),
    )
    .await;

    delete_automation_run_with_archive(&state, &stopped.id, &run.id)
        .await
        .expect("run row deletion still succeeds");

    assert!(state
        .automation_run_repo
        .get_by_id(&run.id)
        .await
        .unwrap()
        .is_none());
    assert!(state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .unwrap()
        .unwrap()
        .archived_at
        .is_some());
    assert_eq!(github.state().delete_remote_branch_calls, 0);
}

#[tokio::test]
async fn delete_automation_run_is_fail_open_when_archive_worktree_and_remote_cleanup_fail() {
    let (_temp, state, project_id, github) = setup_state().await;
    github.state().delete_remote_branch_result = Some(Err(AppError::Infrastructure(
        "remote branch cleanup failed".to_string(),
    )));
    let stopped = automation(
        "automation-delete-cleanup-failures",
        &project_id,
        AutomationStatus::Stopped,
    );
    state.automation_repo.create(stopped.clone()).await.unwrap();
    let run = deletable_run(
        "run-delete-cleanup-failures",
        &stopped.id,
        1,
        AutomationRunStatus::AgentFailed,
        AutomationJudgeState::Done,
    );
    let (run, conversation_id) = seed_run_with_workspace(
        &state,
        &project_id,
        &stopped.id,
        run,
        "ralphx/cleanup-failures",
        std::path::Path::new("relative-unsafe-worktree"),
    )
    .await;
    let mut workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    workspace.mode = AgentConversationWorkspaceMode::Ideation;
    workspace.linked_plan_branch_id = Some(crate::domain::entities::PlanBranchId::from_string(
        "missing-plan-branch".to_string(),
    ));
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();

    delete_automation_run_with_archive(&state, &stopped.id, &run.id)
        .await
        .expect("cleanup failures must not resurrect an authority-deleted run");

    assert!(state
        .automation_run_repo
        .get_by_id(&run.id)
        .await
        .unwrap()
        .is_none());
    assert!(state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .unwrap()
        .unwrap()
        .archived_at
        .is_none());
    assert_eq!(github.state().delete_remote_branch_calls, 1);
    assert_eq!(
        github.state().last_delete_remote_branch_name.as_deref(),
        Some("ralphx/cleanup-failures")
    );
}

#[tokio::test]
async fn delete_automation_run_uses_run_branch_without_workspace_or_conversation() {
    let (temp, state, project_id, github) = setup_state().await;
    let stopped = automation(
        "automation-delete-run-branch",
        &project_id,
        AutomationStatus::Stopped,
    );
    state.automation_repo.create(stopped.clone()).await.unwrap();
    let mut run = deletable_run(
        "run-delete-run-branch",
        &stopped.id,
        1,
        AutomationRunStatus::Cancelled,
        AutomationJudgeState::Failed,
    );
    run.branch_name = Some("ralphx/run-only-branch".to_string());
    state
        .automation_run_repo
        .create_run(run.clone())
        .await
        .unwrap();

    delete_automation_run_with_archive(&state, &stopped.id, &run.id)
        .await
        .expect("run-owned branch cleanup should be best effort");

    assert!(state
        .automation_run_repo
        .get_by_id(&run.id)
        .await
        .unwrap()
        .is_none());
    assert_eq!(github.state().delete_remote_branch_calls, 1);
    assert_eq!(
        github.state().last_delete_remote_branch_name.as_deref(),
        Some("ralphx/run-only-branch")
    );
    drop(temp);
}

#[tokio::test]
async fn delete_automation_run_skips_branch_cleanup_when_project_disappears() {
    let (temp, state, project_id, github) = setup_state().await;
    let stopped = automation(
        "automation-delete-missing-project",
        &project_id,
        AutomationStatus::Stopped,
    );
    state.automation_repo.create(stopped.clone()).await.unwrap();
    let mut run = deletable_run(
        "run-delete-missing-project",
        &stopped.id,
        1,
        AutomationRunStatus::AgentFailed,
        AutomationJudgeState::Done,
    );
    run.branch_name = Some("ralphx/project-disappeared".to_string());
    state
        .automation_run_repo
        .create_run(run.clone())
        .await
        .unwrap();
    state.project_repo.delete(&project_id).await.unwrap();

    delete_automation_run_with_archive(&state, &stopped.id, &run.id)
        .await
        .expect("missing cleanup project must not block run deletion");

    assert!(state
        .automation_run_repo
        .get_by_id(&run.id)
        .await
        .unwrap()
        .is_none());
    assert_eq!(github.state().delete_remote_branch_calls, 0);
    drop(temp);
}

async fn seed_plan_artifact_chain(state: &AppState, prefix: &str) -> (ArtifactId, ArtifactId) {
    let mut first = Artifact::new_inline(
        "Run Plan",
        ArtifactType::Specification,
        format!("{prefix} version 1"),
        "assistant",
    );
    first.id = ArtifactId::from_string(format!("{prefix}-v1"));
    first.metadata.version = 1;
    let first = state.artifact_repo.create(first).await.unwrap();

    let mut second = Artifact::new_inline(
        "Run Plan",
        ArtifactType::Specification,
        format!("{prefix} version 2"),
        "assistant",
    );
    second.id = ArtifactId::from_string(format!("{prefix}-v2"));
    second.metadata.version = 2;
    let second = state
        .artifact_repo
        .create_with_previous_version(second, first.id.clone())
        .await
        .unwrap();

    (first.id, second.id)
}

async fn seed_plan_workspace(
    state: &AppState,
    project_id: &ProjectId,
    conversation_id: &ChatConversationId,
    session_id: crate::domain::entities::IdeationSessionId,
) {
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id.clone(),
        project_id.clone(),
        AgentConversationWorkspaceMode::Plan,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        None,
        "ralphx/automation-plan".to_string(),
        "/tmp/ralphx-automation-plan".to_string(),
    );
    workspace.linked_ideation_session_id = Some(session_id);
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();
}

#[tokio::test]
async fn delete_draft_archives_conversations_and_hard_deletes_rows() {
    let (temp, state, project_id, _github) = setup_state().await;
    let mut draft = automation("automation-draft", &project_id, AutomationStatus::Draft);
    let spec = state
        .artifact_repo
        .create(Artifact::new_inline(
            "Draft spec",
            ArtifactType::Specification,
            "# spec",
            "automation-setup",
        ))
        .await
        .unwrap();
    draft.spec_artifact_id = Some(spec.id.as_str().to_string());
    state.automation_repo.create(draft.clone()).await.unwrap();
    let setup_conv = seed_conversation(&state, &project_id, &draft.id, None, false).await;

    delete_automation_with_archive(&state, &draft.id)
        .await
        .expect("draft delete succeeds");

    // Automation row hard-deleted.
    assert!(state
        .automation_repo
        .get_by_id(&draft.id)
        .await
        .unwrap()
        .is_none());
    // Conversation archived (not deleted).
    let archived = state
        .chat_conversation_repo
        .get_by_id(&setup_conv)
        .await
        .unwrap()
        .unwrap();
    assert!(archived.archived_at.is_some());
    // Spec artifact archived, never hard-deleted.
    let spec_after = state
        .artifact_repo
        .get_by_id(&ArtifactId::from_string(spec.id.as_str().to_string()))
        .await
        .unwrap()
        .unwrap();
    assert!(spec_after.archived_at.is_some());
    drop(temp);
}

#[tokio::test]
async fn delete_archives_setup_and_run_conversations() {
    let (temp, state, project_id, _github) = setup_state().await;
    let stopped = automation("automation-stopped", &project_id, AutomationStatus::Stopped);
    state.automation_repo.create(stopped.clone()).await.unwrap();
    let run = run_with_judge("run-1", &stopped.id, AutomationJudgeState::Done, None);
    state
        .automation_run_repo
        .create_run(run.clone())
        .await
        .unwrap();
    let setup_conv = seed_conversation(&state, &project_id, &stopped.id, None, false).await;
    let run_conv = seed_conversation(&state, &project_id, &stopped.id, Some(&run.id), false).await;

    delete_automation_with_archive(&state, &stopped.id)
        .await
        .expect("delete succeeds");

    for conv in [setup_conv, run_conv] {
        let archived = state
            .chat_conversation_repo
            .get_by_id(&conv)
            .await
            .unwrap()
            .unwrap();
        assert!(
            archived.archived_at.is_some(),
            "conversation should be archived"
        );
    }
    assert!(state
        .automation_repo
        .get_by_id(&stopped.id)
        .await
        .unwrap()
        .is_none());
    assert!(state
        .automation_run_repo
        .list_for_automation(&stopped.id)
        .await
        .unwrap()
        .is_empty());
    drop(temp);
}

#[tokio::test]
async fn delete_cleans_plan_gate_sessions_approvals_and_artifact_chains_for_each_run() {
    let (temp, state, project_id, _github) = setup_state_sqlite().await;
    let stopped = automation("automation-stopped", &project_id, AutomationStatus::Stopped);
    state.automation_repo.create(stopped.clone()).await.unwrap();
    let approval_writer = DbPlanArtifactApprovalWriter::new(state.db.clone());
    let approval_repo = SqlitePlanArtifactApprovalRepository::new(state.db.clone());
    let mut session_ids = Vec::new();
    let mut artifact_ids = Vec::new();

    for index in 1..=2 {
        let mut run = run_with_judge(
            &format!("run-{index}"),
            &stopped.id,
            AutomationJudgeState::Done,
            None,
        );
        run.run_index = index;
        state
            .automation_run_repo
            .create_run(run.clone())
            .await
            .unwrap();
        let conversation_id =
            seed_conversation(&state, &project_id, &stopped.id, Some(&run.id), false).await;
        let (first_artifact_id, latest_artifact_id) =
            seed_plan_artifact_chain(&state, &format!("run-{index}-plan")).await;
        let (first_blueprint_id, latest_blueprint_id) =
            seed_plan_artifact_chain(&state, &format!("run-{index}-blueprint")).await;
        artifact_ids.push(first_artifact_id);
        artifact_ids.push(latest_artifact_id.clone());
        artifact_ids.push(first_blueprint_id);
        artifact_ids.push(latest_blueprint_id.clone());
        let session = IdeationSession::builder()
            .project_id(project_id.clone())
            .session_flow(IdeationSessionFlow::Planning)
            .plan_artifact_id(latest_artifact_id.clone())
            .plan_blueprint_artifact_id(latest_blueprint_id)
            .build();
        let session_id = session.id.clone();
        state.ideation_session_repo.create(session).await.unwrap();
        seed_plan_workspace(&state, &project_id, &conversation_id, session_id.clone()).await;
        approval_writer
            .approve_current_plan_artifact(
                session_id.clone(),
                Some(latest_artifact_id.as_str().to_string()),
                PlanApprovalActor::Judge,
            )
            .await
            .unwrap();
        session_ids.push(session_id);
    }

    delete_automation_with_archive(&state, &stopped.id)
        .await
        .expect("delete succeeds");

    for session_id in session_ids {
        assert!(state
            .ideation_session_repo
            .get_by_id(&session_id)
            .await
            .unwrap()
            .is_none());
        assert!(approval_repo
            .get_by_session(&session_id)
            .await
            .unwrap()
            .is_none());
    }
    for artifact_id in artifact_ids {
        let archived = state
            .artifact_repo
            .get_by_id(&artifact_id)
            .await
            .unwrap()
            .unwrap();
        assert!(
            archived.archived_at.is_some(),
            "plan artifact {} should be archived",
            artifact_id.as_str()
        );
    }
    assert!(state
        .automation_repo
        .get_by_id(&stopped.id)
        .await
        .unwrap()
        .is_none());
    drop(temp);
}

#[tokio::test]
async fn delete_removes_remote_automation_base_branch_for_local_branch_base() {
    // B4: an automation whose base is a local (integration) branch we pushed to
    // origin has that remote branch cleaned up on delete.
    let (temp, state, project_id, github) = setup_state().await;
    let mut completed = automation(
        "automation-base-cleanup",
        &project_id,
        AutomationStatus::Completed,
    );
    completed.base_ref_kind = "local_branch".to_string();
    completed.base_ref = "ralphx/ralphx/automation-abc123".to_string();
    state
        .automation_repo
        .create(completed.clone())
        .await
        .unwrap();

    delete_automation_with_archive(&state, &completed.id)
        .await
        .expect("delete succeeds");

    assert!(state
        .automation_repo
        .get_by_id(&completed.id)
        .await
        .unwrap()
        .is_none());
    let mock = github.state();
    assert_eq!(
        mock.delete_remote_branch_calls, 1,
        "remote base branch should be deleted once"
    );
    assert_eq!(
        mock.last_delete_remote_branch_name.as_deref(),
        Some("ralphx/ralphx/automation-abc123"),
    );
    drop(temp);
}

#[tokio::test]
async fn delete_skips_remote_cleanup_for_project_default_base() {
    // B4: never-published automations (project-default base) have no remote branch
    // to clean up — the delete path must not attempt a remote delete.
    let (temp, state, project_id, github) = setup_state().await;
    let completed = automation(
        "automation-default-base",
        &project_id,
        AutomationStatus::Completed,
    );
    state
        .automation_repo
        .create(completed.clone())
        .await
        .unwrap();

    delete_automation_with_archive(&state, &completed.id)
        .await
        .expect("delete succeeds");

    assert_eq!(github.state().delete_remote_branch_calls, 0);
    drop(temp);
}

#[tokio::test]
async fn delete_is_fail_open_when_remote_branch_delete_errors() {
    // B4: a failed remote-branch delete must NOT block the automation delete.
    let (temp, state, project_id, github) = setup_state().await;
    github.state().delete_remote_branch_result =
        Some(Err(AppError::Infrastructure("boom".to_string())));
    let mut completed = automation(
        "automation-base-fail-open",
        &project_id,
        AutomationStatus::Completed,
    );
    completed.base_ref_kind = "local_branch".to_string();
    completed.base_ref = "ralphx/ralphx/automation-def456".to_string();
    state
        .automation_repo
        .create(completed.clone())
        .await
        .unwrap();

    delete_automation_with_archive(&state, &completed.id)
        .await
        .expect("delete succeeds even when remote branch delete errors");

    assert!(state
        .automation_repo
        .get_by_id(&completed.id)
        .await
        .unwrap()
        .is_none());
    assert_eq!(github.state().delete_remote_branch_calls, 1);
    drop(temp);
}

#[tokio::test]
async fn delete_rejects_active_and_paused() {
    let (temp, state, project_id, _github) = setup_state().await;
    let active = automation("automation-active", &project_id, AutomationStatus::Active);
    let paused = automation("automation-paused", &project_id, AutomationStatus::Paused);
    state.automation_repo.create(active.clone()).await.unwrap();
    state.automation_repo.create(paused.clone()).await.unwrap();

    let active_err = delete_automation_with_archive(&state, &active.id)
        .await
        .unwrap_err();
    assert!(matches!(active_err, AppError::Validation(_)));
    let paused_err = delete_automation_with_archive(&state, &paused.id)
        .await
        .unwrap_err();
    assert!(matches!(paused_err, AppError::Validation(_)));

    // Both automations survive.
    assert!(state
        .automation_repo
        .get_by_id(&active.id)
        .await
        .unwrap()
        .is_some());
    assert!(state
        .automation_repo
        .get_by_id(&paused.id)
        .await
        .unwrap()
        .is_some());
    drop(temp);
}

#[tokio::test]
async fn delete_rejected_when_judge_lease_is_live() {
    let (temp, state, project_id, _github) = setup_state().await;
    let completed = automation("automation-judge", &project_id, AutomationStatus::Completed);
    state
        .automation_repo
        .create(completed.clone())
        .await
        .unwrap();
    let run = run_with_judge(
        "run-1",
        &completed.id,
        AutomationJudgeState::InProgress,
        Some(Utc::now() + Duration::minutes(5)),
    );
    state.automation_run_repo.create_run(run).await.unwrap();

    let error = delete_automation_with_archive(&state, &completed.id)
        .await
        .unwrap_err();
    assert!(
        matches!(error, AppError::Validation(message) if message.contains("judge is finalizing"))
    );
    assert!(state
        .automation_repo
        .get_by_id(&completed.id)
        .await
        .unwrap()
        .is_some());
    drop(temp);
}

#[tokio::test]
async fn delete_allowed_when_judge_lease_is_null_or_expired() {
    // Crashed judge left InProgress with a NULL lease → must NOT block.
    let (temp, state, project_id, _github) = setup_state().await;
    let null_lease = automation(
        "automation-null-lease",
        &project_id,
        AutomationStatus::Completed,
    );
    state
        .automation_repo
        .create(null_lease.clone())
        .await
        .unwrap();
    state
        .automation_run_repo
        .create_run(run_with_judge(
            "run-null",
            &null_lease.id,
            AutomationJudgeState::InProgress,
            None,
        ))
        .await
        .unwrap();
    delete_automation_with_archive(&state, &null_lease.id)
        .await
        .expect("null lease should not block delete");
    assert!(state
        .automation_repo
        .get_by_id(&null_lease.id)
        .await
        .unwrap()
        .is_none());

    // Crashed judge left InProgress with an expired lease → must NOT block.
    let expired_lease = automation(
        "automation-expired-lease",
        &project_id,
        AutomationStatus::Completed,
    );
    state
        .automation_repo
        .create(expired_lease.clone())
        .await
        .unwrap();
    state
        .automation_run_repo
        .create_run(run_with_judge(
            "run-expired",
            &expired_lease.id,
            AutomationJudgeState::InProgress,
            Some(Utc::now() - Duration::minutes(5)),
        ))
        .await
        .unwrap();
    delete_automation_with_archive(&state, &expired_lease.id)
        .await
        .expect("expired lease should not block delete");
    assert!(state
        .automation_repo
        .get_by_id(&expired_lease.id)
        .await
        .unwrap()
        .is_none());
    drop(temp);
}

#[tokio::test]
async fn delete_aborts_when_conversation_archive_fails_and_leaves_rows_intact() {
    let (temp, state, project_id, _github) = setup_state().await;
    let stopped = automation("automation-abort", &project_id, AutomationStatus::Stopped);
    state.automation_repo.create(stopped.clone()).await.unwrap();
    let run = run_with_judge("run-1", &stopped.id, AutomationJudgeState::Done, None);
    state.automation_run_repo.create_run(run).await.unwrap();
    let conv = seed_conversation(&state, &project_id, &stopped.id, None, false).await;

    // Force a fail-closed archive: an Ideation workspace whose linked plan branch
    // references a missing execution plan (mirrors the archive orchestrator's
    // fail-closed path).
    let mut workspace = AgentConversationWorkspace::new(
        conv.clone(),
        project_id.clone(),
        AgentConversationWorkspaceMode::Ideation,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        Some("base-sha".to_string()),
        "agent/abort".to_string(),
        temp.path().join("worktree").to_string_lossy().to_string(),
    );
    workspace.linked_plan_branch_id = Some(crate::domain::entities::PlanBranchId::from_string(
        "missing-branch".to_string(),
    ));
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();

    let error = delete_automation_with_archive(&state, &stopped.id)
        .await
        .unwrap_err();
    let conv_id = conv.as_str();
    assert!(
        matches!(error, AppError::Infrastructure(message) if message.contains(conv_id.as_str()))
    );

    // Fail closed: automation + run rows still present, conversation not archived.
    assert!(state
        .automation_repo
        .get_by_id(&stopped.id)
        .await
        .unwrap()
        .is_some());
    assert_eq!(
        state
            .automation_run_repo
            .list_for_automation(&stopped.id)
            .await
            .unwrap()
            .len(),
        1
    );
    let conv_after = state
        .chat_conversation_repo
        .get_by_id(&conv)
        .await
        .unwrap()
        .unwrap();
    assert!(conv_after.archived_at.is_none());
    drop(temp);
}

#[tokio::test]
async fn delete_archives_active_open_pr_without_closing_and_skips_archived_pr() {
    let (temp, state, project_id, github) = setup_state().await;
    let stopped = automation("automation-retry", &project_id, AutomationStatus::Stopped);
    state.automation_repo.create(stopped.clone()).await.unwrap();

    // Already-archived conversation with an OPEN publication PR: if it were NOT
    // skipped, archiving would call close_pr on the mock GitHub service.
    let archived_conv = seed_conversation(&state, &project_id, &stopped.id, None, true).await;
    let mut archived_ws = AgentConversationWorkspace::new(
        archived_conv.clone(),
        project_id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        Some("base-sha".to_string()),
        "agent/retry".to_string(),
        temp.path().join("worktree").to_string_lossy().to_string(),
    );
    archived_ws.publication_pr_number = Some(77);
    archived_ws.publication_pr_url = Some("https://github.com/mock/repo/pull/77".to_string());
    archived_ws.publication_pr_status = Some("open".to_string());
    state
        .agent_conversation_workspace_repo
        .create_or_update(archived_ws)
        .await
        .unwrap();

    // A fresh (unarchived) conversation with an open PR must be archived without
    // closing the PR, because automation deletion passes explicit false intent.
    let fresh_conv = seed_conversation(&state, &project_id, &stopped.id, None, false).await;
    let mut fresh_ws = AgentConversationWorkspace::new(
        fresh_conv.clone(),
        project_id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        Some("base-sha".to_string()),
        "agent/fresh".to_string(),
        temp.path()
            .join("fresh-worktree")
            .to_string_lossy()
            .to_string(),
    );
    fresh_ws.publication_pr_number = Some(78);
    fresh_ws.publication_pr_url = Some("https://github.com/mock/repo/pull/78".to_string());
    fresh_ws.publication_pr_status = Some("open".to_string());
    state
        .agent_conversation_workspace_repo
        .create_or_update(fresh_ws)
        .await
        .unwrap();

    delete_automation_with_archive(&state, &stopped.id)
        .await
        .expect("delete succeeds");

    // Neither the skipped nor newly archived conversation closes its open PR.
    assert_eq!(github.state().close_pr_calls, 0);
    // Fresh conversation and workspace archived, but its PR remains open.
    let fresh_after = state
        .chat_conversation_repo
        .get_by_id(&fresh_conv)
        .await
        .unwrap()
        .unwrap();
    assert!(fresh_after.archived_at.is_some());
    let fresh_workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&fresh_conv)
        .await
        .unwrap()
        .expect("fresh workspace should exist");
    assert_eq!(
        fresh_workspace.status,
        crate::domain::entities::AgentConversationWorkspaceStatus::Archived
    );
    assert_eq!(
        fresh_workspace.publication_pr_status.as_deref(),
        Some("open")
    );
    assert!(state
        .automation_repo
        .get_by_id(&stopped.id)
        .await
        .unwrap()
        .is_none());
    drop(temp);
}

#[tokio::test]
async fn delete_missing_or_already_deleted_automation_returns_not_found() {
    let (temp, state, project_id, _github) = setup_state().await;
    let stopped = automation("automation-once", &project_id, AutomationStatus::Stopped);
    state.automation_repo.create(stopped.clone()).await.unwrap();

    delete_automation_with_archive(&state, &stopped.id)
        .await
        .expect("first delete succeeds");
    let second = delete_automation_with_archive(&state, &stopped.id)
        .await
        .unwrap_err();
    assert!(matches!(second, AppError::NotFound(_)));

    let unknown =
        delete_automation_with_archive(&state, &AutomationId::from_string("does-not-exist"))
            .await
            .unwrap_err();
    assert!(matches!(unknown, AppError::NotFound(_)));
    drop(temp);
}

/// Automation repo whose `get_by_id` reports a live Draft but whose CAS always
/// loses — models a concurrent `finalize` winning the `Draft -> Stopped` race.
struct DraftLostCasAutomationRepository {
    automation: Automation,
}

#[async_trait]
impl AutomationRepository for DraftLostCasAutomationRepository {
    async fn create(&self, automation: Automation) -> AppResult<Automation> {
        Ok(automation)
    }
    async fn get_by_id(&self, id: &AutomationId) -> AppResult<Option<Automation>> {
        Ok((self.automation.id == *id).then(|| self.automation.clone()))
    }
    async fn list(&self, _project_id: Option<ProjectId>) -> AppResult<Vec<Automation>> {
        Ok(vec![self.automation.clone()])
    }
    async fn list_by_project(&self, _project_id: &ProjectId) -> AppResult<Vec<Automation>> {
        Ok(vec![self.automation.clone()])
    }
    async fn update_settings(
        &self,
        _id: &AutomationId,
        _patch: AutomationSettingsPatch,
    ) -> AppResult<Option<Automation>> {
        Ok(None)
    }
    async fn update_config(
        &self,
        _id: &AutomationId,
        _patch: AutomationConfigPatch,
    ) -> AppResult<Option<Automation>> {
        Ok(None)
    }
    async fn update_goal_items_json(
        &self,
        _id: &AutomationId,
        _goal_items_json: Option<String>,
    ) -> AppResult<Option<Automation>> {
        Ok(None)
    }
    async fn update_goal_items_json_if_unchanged(
        &self,
        _id: &AutomationId,
        _expected_goal_items_json: Option<String>,
        _goal_items_json: Option<String>,
    ) -> AppResult<Option<Automation>> {
        Ok(None)
    }
    async fn update_authoring_state_if_unchanged(
        &self,
        _id: &AutomationId,
        _expected_updated_at: chrono::DateTime<Utc>,
        _authoring_state_json: Option<String>,
    ) -> AppResult<bool> {
        Ok(false)
    }
    async fn compare_and_swap_status(
        &self,
        _id: &AutomationId,
        _from: AutomationStatus,
        _to: AutomationStatus,
        _paused_reason_code: Option<String>,
        _paused_reason_detail: Option<String>,
    ) -> AppResult<bool> {
        // The concurrent finalize already moved the row off Draft: CAS loses.
        Ok(false)
    }
    async fn delete_terminal(&self, _id: &AutomationId) -> AppResult<bool> {
        Ok(false)
    }
    async fn delete_attachments_for_automation(
        &self,
        _automation_id: &AutomationId,
    ) -> AppResult<usize> {
        Ok(0)
    }
    async fn delete_context_refs_for_automation(
        &self,
        _automation_id: &AutomationId,
    ) -> AppResult<usize> {
        Ok(0)
    }
}

#[tokio::test]
async fn delete_draft_returns_conflict_and_archives_nothing_when_cas_lost() {
    let (temp, mut state, project_id, github) = setup_state().await;
    let draft = automation("automation-race", &project_id, AutomationStatus::Draft);
    // Seed a conversation in the real (memory) chat repo BEFORE swapping the
    // automation repo, so we can prove it is never archived.
    let conv = seed_conversation(&state, &project_id, &draft.id, None, false).await;
    state.automation_repo = Arc::new(DraftLostCasAutomationRepository {
        automation: draft.clone(),
    });

    let error = delete_automation_with_archive(&state, &draft.id)
        .await
        .unwrap_err();
    assert!(matches!(error, AppError::Conflict(_)));

    // Zero side effects: conversation untouched, no PR close attempted.
    let conv_after = state
        .chat_conversation_repo
        .get_by_id(&conv)
        .await
        .unwrap()
        .unwrap();
    assert!(conv_after.archived_at.is_none());
    assert_eq!(github.state().close_pr_calls, 0);
    drop(temp);
}
