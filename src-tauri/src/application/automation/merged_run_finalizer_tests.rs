use std::sync::Arc;

use super::merged_run_finalizer::{
    AppStateAutomationMergedRunFinalizer, AutomationMergedRunFinalizer,
    NoopAutomationMergedRunFinalizer,
};
use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentConversationWorkspaceStatus,
    ChatConversation, ChatConversationId, IdeationAnalysisBaseRefKind, Project,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, TaskOutcomeListOptions, WORKSPACE_PR_MERGED_CLASS,
    WORKSPACE_PR_MERGED_CLEAN_CLASS, WORKSPACE_SESSION_ABANDONED_CLASS,
};
use crate::domain::services::{
    GithubServiceTrait, PrStatus, PrSyncState, AGENT_WORKSPACE_OUTCOME_SOURCE,
    AGENT_WORKSPACE_PR_OUTCOME_SOURCE,
};
use crate::infrastructure::memory::MemoryAgentConversationWorkspaceRepository;
use crate::tests::mock_github_service::MockGithubService;

async fn setup_finalizer_state(
    persist_project: bool,
) -> (
    tempfile::TempDir,
    AppState,
    Arc<MemoryAgentConversationWorkspaceRepository>,
    ChatConversationId,
) {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let mut project = Project::new(
        "Automation merged cleanup".to_string(),
        temp.path().to_string_lossy().to_string(),
    );
    project.worktree_parent_directory =
        Some(temp.path().join("worktrees").to_string_lossy().to_string());
    let conversation_id = ChatConversationId::new();
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id.clone(),
        project.id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        Some("base-sha".to_string()),
        "ralphx/automation-merged-cleanup".to_string(),
        temp.path()
            .join("unexpected-worktree")
            .to_string_lossy()
            .to_string(),
    );
    workspace.publication_pr_number = Some(42);
    workspace.publication_pr_url = Some("https://github.com/acme/project/pull/42".to_string());
    workspace.publication_pr_status = Some("merged".to_string());

    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let mut state = AppState::new_test();
    state.agent_conversation_workspace_repo = workspace_repo.clone();
    if persist_project {
        state
            .project_repo
            .create(project.clone())
            .await
            .expect("project should be persisted");
    }
    let mut conversation = ChatConversation::new_project(project.id);
    conversation.id = conversation_id.clone();
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("conversation should be persisted");
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should be persisted");

    (temp, state, workspace_repo, conversation_id)
}

#[tokio::test]
async fn merged_run_finalizer_errors_when_conversation_is_missing() {
    let state = AppState::new_test();
    let conversation_id = ChatConversationId::new();
    let finalizer = AppStateAutomationMergedRunFinalizer::new(state);

    let error = finalizer
        .finalize_merged_conversation(&conversation_id)
        .await
        .expect_err("missing conversation must keep finalization retryable");

    assert!(matches!(error, crate::error::AppError::NotFound(_)));
    assert!(error.to_string().contains(&conversation_id.as_str()));
}

#[tokio::test]
async fn merged_run_finalizer_archives_conversation_without_workspace() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let project = Project::new(
        "Automation merged cleanup".to_string(),
        temp.path().to_string_lossy().to_string(),
    );
    let conversation_id = ChatConversationId::new();
    let mut conversation = ChatConversation::new_project(project.id);
    conversation.id = conversation_id.clone();
    let state = AppState::new_test();
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("conversation should be persisted");
    let finalizer = AppStateAutomationMergedRunFinalizer::new(state.clone());

    finalizer
        .finalize_merged_conversation(&conversation_id)
        .await
        .expect("conversation without workspace should still archive");

    let conversation = state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert!(conversation.archived_at.is_some());
}

#[tokio::test]
async fn merged_run_finalizer_marks_unsafe_cleanup_and_archives_without_closing_pr() {
    let (_temp, state, workspace_repo, conversation_id) = setup_finalizer_state(true).await;
    let finalizer = AppStateAutomationMergedRunFinalizer::new(state.clone());

    finalizer
        .finalize_merged_conversation(&conversation_id)
        .await
        .expect("merged finalization should succeed");
    finalizer
        .finalize_merged_conversation(&conversation_id)
        .await
        .expect("merged finalization should be idempotent");

    let conversation = state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert!(conversation.archived_at.is_some());
    let workspace = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(workspace.status, AgentConversationWorkspaceStatus::Archived);
    assert_eq!(workspace.publication_pr_status.as_deref(), Some("merged"));
    assert_eq!(
        workspace_repo
            .local_cleanup_status_for_test(&conversation_id)
            .await
            .as_deref(),
        Some("failed_unsafe")
    );
    let pr_outcomes = state
        .task_outcome_repo
        .list_by_project(
            &workspace.project_id,
            TaskOutcomeListOptions {
                source: Some(AGENT_WORKSPACE_PR_OUTCOME_SOURCE),
                ..TaskOutcomeListOptions::default()
            },
        )
        .await
        .expect("merged finalizer outcomes should be readable");
    assert_eq!(pr_outcomes.len(), 1);
    assert_eq!(
        pr_outcomes[0]
            .outcome_class
            .as_ref()
            .map(|class| class.as_str()),
        Some(WORKSPACE_PR_MERGED_CLASS)
    );
    assert_eq!(pr_outcomes[0].pull_request_id.as_deref(), Some("42"));
    let no_pr_outcomes = state
        .task_outcome_repo
        .list_by_project(
            &workspace.project_id,
            TaskOutcomeListOptions {
                source: Some(AGENT_WORKSPACE_OUTCOME_SOURCE),
                ..TaskOutcomeListOptions::default()
            },
        )
        .await
        .expect("merged finalizer no-PR outcomes should be readable");
    assert!(no_pr_outcomes.iter().all(|outcome| {
        outcome.outcome_class.as_ref().map(|class| class.as_str())
            != Some(WORKSPACE_SESSION_ABANDONED_CLASS)
    }));
}

#[tokio::test]
async fn merged_run_finalizer_records_clean_merge_from_authoritative_remote_head() {
    let (_temp, mut state, workspace_repo, conversation_id) = setup_finalizer_state(true).await;
    let mut workspace = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    workspace.publication_pushed_sha = Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .unwrap();
    let github = Arc::new(MockGithubService::new());
    github.state().check_pr_sync_state_result = Some(Ok(PrSyncState {
        status: PrStatus::Merged {
            merge_commit_sha: Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string()),
            merged_at: Some("2026-07-27T00:00:00Z".to_string()),
        },
        merge_state_status: None,
        mergeable: None,
        is_draft: false,
        head_ref_name: workspace.branch_name.clone(),
        base_ref_name: workspace.base_ref.clone(),
        head_ref_oid: workspace.publication_pushed_sha.clone(),
        base_ref_oid: None,
    }));
    state.github_service = Some(github as Arc<dyn GithubServiceTrait>);

    AppStateAutomationMergedRunFinalizer::new(state.clone())
        .finalize_merged_conversation(&conversation_id)
        .await
        .expect("merged finalization should succeed");

    let pr_outcomes = state
        .task_outcome_repo
        .list_by_project(
            &workspace.project_id,
            TaskOutcomeListOptions {
                source: Some(AGENT_WORKSPACE_PR_OUTCOME_SOURCE),
                ..TaskOutcomeListOptions::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(pr_outcomes.len(), 1);
    assert_eq!(
        pr_outcomes[0]
            .outcome_class
            .as_ref()
            .map(|class| class.as_str()),
        Some(WORKSPACE_PR_MERGED_CLEAN_CLASS)
    );
    assert_eq!(
        workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .unwrap()
            .unwrap()
            .publication_pr_status
            .as_deref(),
        Some("merged"),
        "cleanliness is transient ledger evidence, not workspace lifecycle state"
    );
}

#[tokio::test]
async fn noop_merged_run_finalizer_succeeds_without_side_effect_requirements() {
    NoopAutomationMergedRunFinalizer
        .finalize_merged_conversation(&ChatConversationId::new())
        .await
        .expect("noop finalizer should never block scheduler construction");
}

#[tokio::test]
async fn merged_run_finalizer_cleans_pre_archived_conversation_and_archives_workspace() {
    let (_temp, state, workspace_repo, conversation_id) = setup_finalizer_state(true).await;
    state
        .chat_conversation_repo
        .archive(&conversation_id)
        .await
        .expect("conversation should be pre-archived");
    assert!(workspace_repo
        .local_cleanup_status_for_test(&conversation_id)
        .await
        .is_none());
    let finalizer = AppStateAutomationMergedRunFinalizer::new(state);

    finalizer
        .finalize_merged_conversation(&conversation_id)
        .await
        .expect("pre-archived merged finalization should still clean artifacts");

    assert_eq!(
        workspace_repo
            .local_cleanup_status_for_test(&conversation_id)
            .await
            .as_deref(),
        Some("failed_unsafe")
    );
    assert_eq!(
        workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        AgentConversationWorkspaceStatus::Archived
    );
}

#[tokio::test]
async fn merged_run_finalizer_does_not_archive_when_runtime_cleanup_context_is_missing() {
    let (_temp, state, _workspace_repo, conversation_id) = setup_finalizer_state(false).await;
    let finalizer = AppStateAutomationMergedRunFinalizer::new(state.clone());

    let error = finalizer
        .finalize_merged_conversation(&conversation_id)
        .await
        .expect_err("missing project must keep finalization retryable");

    assert!(error.to_string().contains("project"));
    assert!(state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .unwrap()
        .unwrap()
        .archived_at
        .is_none());
}
