use std::sync::Arc;

use async_trait::async_trait;
use chrono::{Duration, Utc};

use crate::application::automation::delete::delete_automation_with_archive;
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
        agent_phase_started_at: None,
        conversation_id: None,
        run_prompt: "Run 1 prompt".to_string(),
        prompt_author: AutomationPromptAuthor::SetupAgent,
        base_ref_kind: "local_branch".to_string(),
        base_ref_used: "main".to_string(),
        base_from_run_id: None,
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
        artifact_ids.push(first_artifact_id);
        artifact_ids.push(latest_artifact_id.clone());
        let session = IdeationSession::builder()
            .project_id(project_id.clone())
            .session_flow(IdeationSessionFlow::Planning)
            .plan_artifact_id(latest_artifact_id.clone())
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
