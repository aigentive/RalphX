use crate::application::agent_workspace_review_context::{
    load_agent_workspace_review_presentation_context, AgentWorkspaceReviewContextReadMode,
};
use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentWorkspaceReviewGateStatus,
    AgentWorkspaceReviewMonitor, AgentWorkspaceReviewMonitorStatus,
    AgentWorkspaceReviewTargetScope, ChatConversationId, IdeationAnalysisBaseRefKind, Project,
    ProjectId,
};
use crate::AppError;
use futures::future::join_all;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git command should spawn");
    assert!(
        output.status.success(),
        "git {args:?} failed\\nstdout:\\n{}\\nstderr:\\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn init_repo() -> (tempfile::TempDir, std::path::PathBuf, String) {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(&repo).expect("repo directory should be created");
    git(&repo, &["init", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test User"]);
    std::fs::write(repo.join("README.md"), "base\\n").expect("base file should be written");
    git(&repo, &["add", "README.md"]);
    git(&repo, &["commit", "-m", "base"]);
    let base_sha = git(&repo, &["rev-parse", "HEAD"]);
    (temp, repo, base_sha)
}

async fn setup_full_context() -> (
    tempfile::TempDir,
    std::path::PathBuf,
    AppState,
    AgentConversationWorkspace,
) {
    let (temp, repo, base_sha) = init_repo();
    let state = AppState::new_test();
    let mut project = Project::new(
        "Workspace Review context".to_string(),
        repo.to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    state
        .project_repo
        .create(project.clone())
        .await
        .expect("project should persist");
    let workspace = AgentConversationWorkspace::new(
        ChatConversationId::new(),
        project.id,
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        Some(base_sha),
        "ralphx/test/context-full".to_string(),
        repo.to_string_lossy().to_string(),
    );
    (temp, repo, state, workspace)
}

fn workspace(
    conversation_id: ChatConversationId,
    project_id: ProjectId,
) -> AgentConversationWorkspace {
    AgentConversationWorkspace::new(
        conversation_id,
        project_id,
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some("base-sha".to_string()),
        "ralphx/test/context-snapshot".to_string(),
        "/path/that/must/not/be-read-for-status".to_string(),
    )
}

#[tokio::test]
async fn complete_reviewing_monitor_uses_status_snapshot_without_project_or_git_reads() {
    let state = AppState::new_test();
    let conversation_id = ChatConversationId::new();
    let project_id = ProjectId::new();
    let workspace = workspace(conversation_id.clone(), project_id.clone());
    let mut monitor = AgentWorkspaceReviewMonitor::new(conversation_id, project_id);
    monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Reviewing;
    monitor.current_target_scope = Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta);
    monitor.current_diff_fingerprint = Some("current-fingerprint".to_string());
    monitor.workspace_base_ref = Some("main".to_string());
    monitor.workspace_base_sha = Some("base-sha".to_string());
    monitor.workspace_head_ref = Some("HEAD".to_string());
    monitor.workspace_head_sha = Some("head-sha".to_string());
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("seed monitor");

    let context = load_agent_workspace_review_presentation_context(
        &state,
        &workspace,
        AgentWorkspaceReviewContextReadMode::StatusSnapshot,
    )
    .await
    .expect("load status snapshot");

    assert_eq!(
        context.monitor.status,
        AgentWorkspaceReviewMonitorStatus::Reviewing
    );
    assert_eq!(
        context
            .target
            .as_ref()
            .map(|target| target.diff_fingerprint.as_str()),
        Some("current-fingerprint")
    );
    assert!(context
        .target
        .as_ref()
        .expect("snapshot target")
        .review_packet
        .changed_files
        .is_empty());
}

#[tokio::test]
async fn incomplete_reviewing_monitor_fails_closed_instead_of_becoming_idle() {
    let state = AppState::new_test();
    let conversation_id = ChatConversationId::new();
    let project_id = ProjectId::new();
    let workspace = workspace(conversation_id.clone(), project_id.clone());
    let mut monitor = AgentWorkspaceReviewMonitor::new(conversation_id, project_id);
    monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Reviewing;
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("seed monitor");

    let error = load_agent_workspace_review_presentation_context(
        &state,
        &workspace,
        AgentWorkspaceReviewContextReadMode::StatusSnapshot,
    )
    .await
    .expect_err("incomplete reviewing state must fail closed");

    assert!(matches!(error, AppError::Conflict(_)));
}

#[tokio::test]
async fn selected_source_reviewing_monitor_uses_status_snapshot_without_project_reads() {
    let state = AppState::new_test();
    let conversation_id = ChatConversationId::new();
    let project_id = ProjectId::new();
    let workspace = workspace(conversation_id.clone(), project_id.clone());
    let mut monitor = AgentWorkspaceReviewMonitor::new(conversation_id, project_id);
    monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Reviewing;
    monitor.current_target_scope = Some(AgentWorkspaceReviewTargetScope::SelectedSource);
    monitor.current_diff_fingerprint = Some("selected-fingerprint".to_string());
    monitor.selected_source_base_ref = Some("main".to_string());
    monitor.selected_source_head_ref = Some("feature/selected".to_string());
    monitor.selected_source_pull_request_number = Some(42);
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("seed monitor");

    let context = load_agent_workspace_review_presentation_context(
        &state,
        &workspace,
        AgentWorkspaceReviewContextReadMode::StatusSnapshot,
    )
    .await
    .expect("selected source snapshot should load");

    let target = context.target.expect("snapshot target");
    assert_eq!(
        target.scope,
        AgentWorkspaceReviewTargetScope::SelectedSource
    );
    assert_eq!(target.source_pull_request_number, Some(42));
}

#[tokio::test]
async fn full_context_propagates_missing_project_instead_of_treating_it_as_no_review() {
    let state = AppState::new_test();
    let workspace = workspace(ChatConversationId::new(), ProjectId::new());

    let error = load_agent_workspace_review_presentation_context(
        &state,
        &workspace,
        AgentWorkspaceReviewContextReadMode::FullPacket,
    )
    .await
    .expect_err("missing project must fail closed");

    assert!(matches!(error, AppError::NotFound(_)));
}

#[tokio::test]
async fn full_target_reuses_a_current_calculation_without_reloading_git_context() {
    let (_temp, repo, state, workspace) = setup_full_context().await;

    let initial = load_agent_workspace_review_presentation_context(
        &state,
        &workspace,
        AgentWorkspaceReviewContextReadMode::FullTarget,
    )
    .await
    .expect("initial full context should load");
    std::fs::write(repo.join("uncommitted.rs"), "pub fn changed() {}\n")
        .expect("workspace change should be written");
    let cached = load_agent_workspace_review_presentation_context(
        &state,
        &workspace,
        AgentWorkspaceReviewContextReadMode::FullTarget,
    )
    .await
    .expect("cached full target context should load");

    assert_eq!(
        initial.monitor.status,
        AgentWorkspaceReviewMonitorStatus::Idle
    );
    assert_eq!(cached.monitor, initial.monitor);
    assert!(cached.target.is_none());
}

#[tokio::test]
async fn cached_edit_context_is_rejected_after_persisted_mode_changes_to_plan() {
    let (_temp, _repo, state, workspace) = setup_full_context().await;
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("Edit workspace should persist");
    load_agent_workspace_review_presentation_context(
        &state,
        &workspace,
        AgentWorkspaceReviewContextReadMode::FullTarget,
    )
    .await
    .expect("Edit context should warm the presentation cache");

    let mut plan_workspace = workspace.clone();
    plan_workspace.mode = AgentConversationWorkspaceMode::Plan;
    state
        .agent_conversation_workspace_repo
        .create_or_update(plan_workspace)
        .await
        .expect("PLAN workspace should persist");

    let error = load_agent_workspace_review_presentation_context(
        &state,
        &workspace,
        AgentWorkspaceReviewContextReadMode::FullTarget,
    )
    .await
    .expect_err("PLAN must reject before reusing the Edit presentation cache");

    assert!(matches!(error, AppError::Validation(_)));
}

#[tokio::test]
async fn full_packet_refreshes_instead_of_reusing_the_presentation_cache() {
    let (_temp, repo, state, workspace) = setup_full_context().await;

    let initial = load_agent_workspace_review_presentation_context(
        &state,
        &workspace,
        AgentWorkspaceReviewContextReadMode::FullPacket,
    )
    .await
    .expect("initial full packet context should load");
    std::fs::write(repo.join("uncommitted.rs"), "pub fn changed() {}\n")
        .expect("workspace change should be written");
    let refreshed = load_agent_workspace_review_presentation_context(
        &state,
        &workspace,
        AgentWorkspaceReviewContextReadMode::FullPacket,
    )
    .await
    .expect("refreshed full packet context should load");

    assert_eq!(
        initial.monitor.status,
        AgentWorkspaceReviewMonitorStatus::Idle
    );
    assert!(initial.target.is_none());
    assert!(refreshed.target.is_some());
}

#[tokio::test]
async fn simultaneous_full_packet_requests_join_one_calculation() {
    let (_temp, repo, state, workspace) = setup_full_context().await;
    std::fs::write(repo.join("uncommitted.rs"), "pub fn changed() {}\n")
        .expect("workspace change should be written");
    let state = Arc::new(state);

    let requests = (0..8).map(|_| {
        let state = state.clone();
        let workspace = workspace.clone();
        tokio::spawn(async move {
            load_agent_workspace_review_presentation_context(
                state.as_ref(),
                &workspace,
                AgentWorkspaceReviewContextReadMode::FullPacket,
            )
            .await
        })
    });
    let results = join_all(requests).await;

    for result in results {
        let context = result
            .expect("request task should complete")
            .expect("full packet context should load");
        assert_eq!(
            context.monitor.status,
            AgentWorkspaceReviewMonitorStatus::Idle
        );
        assert!(context.target.is_some());
    }
}
