use axum::extract::{Json, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use ralphx_lib::application::AppState;
use ralphx_lib::commands::unified_chat_commands::AgentConversationWorkspaceResponse;
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentRun,
    AgentWorkspaceReviewArtifactOutcome, AgentWorkspaceReviewGateStatus, AgentWorkspaceReviewMonitor, AgentWorkspaceReviewMonitorStatus,
    AgentWorkspaceReviewOutcome, AgentWorkspaceReviewTargetScope, ArtifactContent, ArtifactId,
    ChatConversation, ChatConversationId, IdeationAnalysisBaseRefKind, Project,
};
use ralphx_lib::http_server::handlers::agent_workspaces::{
    get_agent_workspace_review_context, get_agent_workspace_review_start_preview,
    write_agent_workspace_review_artifact, AgentWorkspaceReviewContextQuery,
    CommitAgentWorkspaceLocallyResponse, WriteAgentWorkspaceReviewArtifactRequest,
};
use ralphx_lib::http_server::types::HttpServerState;
use std::path::Path as StdPath;
use std::process::Command;
use std::sync::Arc;

fn git(repo: impl AsRef<StdPath>, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git command should spawn");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn test_state() -> HttpServerState {
    let app_state = Arc::new(AppState::new_test());
    HttpServerState {
        app_state,
        execution_state: Arc::new(ExecutionState::new()),
        delegation_service: Default::default(),
        external_mcp_supervisor: None,
    }
}

#[test]
fn local_commit_http_response_serializes_snake_case_contract_fields() {
    let workspace = AgentConversationWorkspace::new(
        ChatConversationId::from_string("commit-http-conversation".to_string()),
        ralphx_lib::domain::entities::ProjectId::from_string("commit-http-project".to_string()),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some("base".to_string()),
        "ralphx/commit-http".to_string(),
        "/tmp/commit-http".to_string(),
    );
    let response = CommitAgentWorkspaceLocallyResponse {
        success: true,
        workspace: AgentConversationWorkspaceResponse::from(workspace),
        outcome: "committed_local".to_string(),
        branch_name: "ralphx/commit-http".to_string(),
        previous_head_sha: "before".to_string(),
        commit_sha: "after".to_string(),
        had_changes: true,
        attempt_token: "attempt-1".to_string(),
    };

    let value = serde_json::to_value(response).expect("HTTP response should serialize");

    assert_eq!(value["branch_name"], "ralphx/commit-http");
    assert_eq!(value["previous_head_sha"], "before");
    assert_eq!(value["commit_sha"], "after");
    assert_eq!(value["had_changes"], true);
    assert_eq!(value["attempt_token"], "attempt-1");
    assert!(value.get("branch").is_none());
    assert!(value.get("current_head_sha").is_none());
}

#[tokio::test]
async fn workspace_review_start_preview_blocks_unfinished_git_operation() {
    let root = tempfile::TempDir::new().expect("fixture root");
    let repo = root.path().join("repo");
    let workspace_path = root.path().join("workspace");
    std::fs::create_dir_all(&repo).expect("repo directory");
    git(&repo, &["init", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "RalphX Test"]);
    std::fs::write(repo.join("shared.txt"), "base\n").expect("base file");
    git(&repo, &["add", "shared.txt"]);
    git(&repo, &["commit", "-m", "base"]);
    let base_sha = git(&repo, &["rev-parse", "HEAD"]);
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-b",
            "ralphx/test/http-unfinished-review",
            workspace_path.to_str().expect("workspace path"),
            "main",
        ],
    );
    std::fs::write(workspace_path.join("shared.txt"), "feature\n").expect("feature file");
    git(&workspace_path, &["add", "shared.txt"]);
    git(&workspace_path, &["commit", "-m", "feature"]);
    std::fs::write(repo.join("shared.txt"), "main\n").expect("main file");
    git(&repo, &["add", "shared.txt"]);
    git(&repo, &["commit", "-m", "main"]);
    let merge = Command::new("git")
        .args(["merge", "main"])
        .current_dir(&workspace_path)
        .output()
        .expect("merge should spawn");
    assert!(!merge.status.success(), "merge should create a conflict");

    let state = test_state();
    let conversation_id = ChatConversationId::new();
    let mut project = Project::new(
        "Blocked Review Preview".to_string(),
        repo.to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    state
        .app_state
        .project_repo
        .create(project.clone())
        .await
        .expect("seed project");
    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.id = conversation_id;
    state
        .app_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("seed conversation");
    let workspace = AgentConversationWorkspace::new(
        conversation_id,
        project.id,
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some(base_sha),
        "ralphx/test/http-unfinished-review".to_string(),
        workspace_path.to_string_lossy().to_string(),
    );
    state
        .app_state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("seed workspace");

    let (status, axum::Json(body)) = get_agent_workspace_review_start_preview(
        State(state.clone()),
        Path(conversation_id.to_string()),
    )
    .await
    .expect_err("unfinished operation must block preview");

    assert_eq!(status, StatusCode::CONFLICT);
    let detail = body["error"].as_str().expect("error detail");
    assert_eq!(
        detail,
        "Resolve conflicts and complete or abort the merge or rebase before retrying Workspace Review."
    );
    assert!(!detail.contains("write-tree"));
    assert!(state
        .app_state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&conversation_id)
        .await
        .expect("read monitor")
        .is_none());
}

#[tokio::test]
async fn outdated_artifact_does_not_revoke_exact_active_reviewer_authority() {
    let repo = tempfile::TempDir::new().expect("repo tempdir");
    let worktrees = tempfile::TempDir::new().expect("worktree tempdir");
    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@example.com"]);
    git(repo.path(), &["config", "user.name", "RalphX Test"]);
    std::fs::write(repo.path().join("README.md"), "base\n").expect("write base file");
    git(repo.path(), &["add", "README.md"]);
    git(repo.path(), &["commit", "-m", "base"]);
    let base_sha = git(repo.path(), &["rev-parse", "HEAD"]);

    let state = test_state();
    let conversation_id = ChatConversationId::new();
    let mut project = Project::new(
        "Review Runtime Authority".to_string(),
        repo.path().to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    state
        .app_state
        .project_repo
        .create(project.clone())
        .await
        .expect("seed project");
    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.id = conversation_id;
    state
        .app_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("seed conversation");

    let workspace_path = worktrees.path().join("workspace");
    let branch_name = "ralphx/test/review-runtime-authority";
    git(
        repo.path(),
        &[
            "worktree",
            "add",
            "-b",
            branch_name,
            workspace_path.to_str().expect("workspace path"),
            "main",
        ],
    );
    std::fs::write(
        workspace_path.join("implementation.txt"),
        "current change\n",
    )
    .expect("write workspace change");
    let workspace = AgentConversationWorkspace::new(
        conversation_id,
        project.id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some(base_sha),
        branch_name.to_string(),
        workspace_path.to_string_lossy().to_string(),
    );
    state
        .app_state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("seed workspace");

    let axum::Json(initial) = get_agent_workspace_review_context(
        State(state.clone()),
        Path(conversation_id.to_string()),
        HeaderMap::new(),
        Query(AgentWorkspaceReviewContextQuery::default()),
    )
    .await
    .expect("load initial context");
    let target = initial.target.expect("review target");
    let review_conversation_id = ChatConversationId::new();
    let run = AgentRun::new(review_conversation_id);
    let run_id = run.id;
    let mut monitor = AgentWorkspaceReviewMonitor::new(conversation_id, project.id.clone());
    let target_scope: AgentWorkspaceReviewTargetScope =
        target.scope.parse().expect("valid target scope");
    monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    monitor.review_outcome = AgentWorkspaceReviewOutcome::None;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Reviewing;
    monitor.current_target_scope = Some(target_scope);
    monitor.current_diff_fingerprint = Some(target.diff_fingerprint.clone());
    match target_scope {
        AgentWorkspaceReviewTargetScope::WorkspaceDelta => {
            monitor.workspace_base_ref = Some(target.base_ref.clone());
            monitor.workspace_base_sha = target.base_sha.clone();
            monitor.workspace_head_ref = Some(target.head_ref.clone());
            monitor.workspace_head_sha = target.head_sha.clone();
        }
        AgentWorkspaceReviewTargetScope::SelectedSource => {
            monitor.selected_source_base_ref = Some(target.base_ref.clone());
            monitor.selected_source_base_sha = target.base_sha.clone();
            monitor.selected_source_head_ref = Some(target.head_ref.clone());
            monitor.selected_source_head_sha = target.head_sha.clone();
            monitor.selected_source_pull_request_number = target.source_pull_request_number;
        }
    }
    monitor.reviewed_target_scope = Some(target_scope);
    monitor.reviewed_diff_fingerprint = Some("historical-fingerprint".to_string());
    monitor.review_artifact_id = Some(ArtifactId::from_string("historical-review"));
    monitor.review_artifact_version = Some(1);
    monitor.review_conversation_id = Some(review_conversation_id);
    monitor.last_run_id = Some(run_id.to_string());
    state
        .app_state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("bind active review");
    state
        .app_state
        .agent_run_repo
        .create(run)
        .await
        .expect("seed active run");

    let mut headers = HeaderMap::new();
    headers.insert(
        "x-ralphx-agent-run-id",
        run_id.to_string().parse().expect("run header"),
    );
    headers.insert(
        "x-ralphx-conversation-id",
        review_conversation_id
            .as_str()
            .parse()
            .expect("conversation header"),
    );
    let axum::Json(context) = get_agent_workspace_review_context(
        State(state),
        Path(conversation_id.to_string()),
        headers,
        Query(AgentWorkspaceReviewContextQuery::default()),
    )
    .await
    .expect("load authorized context");

    assert!(context.review_artifact_is_outdated);
    assert!(!context.review_artifact_is_current);
    assert!(context.can_mutate_review_state);
    assert_eq!(context.review_runtime_state, "active_owned");
}

#[tokio::test]
async fn workspace_review_artifact_write_versions_pair_and_keeps_second_content() {
    let repo = tempfile::TempDir::new().expect("repo tempdir");
    let worktrees = tempfile::TempDir::new().expect("worktree tempdir");
    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@example.com"]);
    git(repo.path(), &["config", "user.name", "RalphX Test"]);
    std::fs::write(repo.path().join("README.md"), "base\n").expect("write base file");
    git(repo.path(), &["add", "README.md"]);
    git(repo.path(), &["commit", "-m", "base"]);
    let base_sha = git(repo.path(), &["rev-parse", "HEAD"]);

    let state = test_state();
    let conversation_id = ChatConversationId::new();
    let mut project = Project::new(
        "Repeated Review Artifact Write".to_string(),
        repo.path().to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    state
        .app_state
        .project_repo
        .create(project.clone())
        .await
        .expect("seed project");
    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.id = conversation_id;
    state
        .app_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("seed conversation");

    let workspace_path = worktrees.path().join("workspace");
    let branch_name = "ralphx/test/repeated-review-artifact-write";
    git(
        repo.path(),
        &[
            "worktree",
            "add",
            "-b",
            branch_name,
            workspace_path.to_str().expect("workspace path"),
            "main",
        ],
    );
    std::fs::write(
        workspace_path.join("implementation.txt"),
        "current change\n",
    )
    .expect("write workspace change");
    let workspace = AgentConversationWorkspace::new(
        conversation_id,
        project.id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some(base_sha),
        branch_name.to_string(),
        workspace_path.to_string_lossy().to_string(),
    );
    state
        .app_state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("seed workspace");

    let axum::Json(initial) = get_agent_workspace_review_context(
        State(state.clone()),
        Path(conversation_id.to_string()),
        HeaderMap::new(),
        Query(AgentWorkspaceReviewContextQuery::default()),
    )
    .await
    .expect("load initial context");
    let target = initial.target.expect("review target");
    let review_conversation_id = ChatConversationId::new();
    let run = AgentRun::new(review_conversation_id);
    let run_id = run.id;
    let target_scope: AgentWorkspaceReviewTargetScope =
        target.scope.parse().expect("valid target scope");
    let mut monitor = AgentWorkspaceReviewMonitor::new(conversation_id, project.id.clone());
    monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    monitor.review_outcome = AgentWorkspaceReviewOutcome::None;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Reviewing;
    monitor.current_target_scope = Some(target_scope);
    monitor.current_diff_fingerprint = Some(target.diff_fingerprint.clone());
    match target_scope {
        AgentWorkspaceReviewTargetScope::WorkspaceDelta => {
            monitor.workspace_base_ref = Some(target.base_ref.clone());
            monitor.workspace_base_sha = target.base_sha.clone();
            monitor.workspace_head_ref = Some(target.head_ref.clone());
            monitor.workspace_head_sha = target.head_sha.clone();
        }
        AgentWorkspaceReviewTargetScope::SelectedSource => {
            monitor.selected_source_base_ref = Some(target.base_ref.clone());
            monitor.selected_source_base_sha = target.base_sha.clone();
            monitor.selected_source_head_ref = Some(target.head_ref.clone());
            monitor.selected_source_head_sha = target.head_sha.clone();
            monitor.selected_source_pull_request_number = target.source_pull_request_number;
        }
    }
    monitor.review_conversation_id = Some(review_conversation_id);
    monitor.last_run_id = Some(run_id.to_string());
    state
        .app_state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("bind active review");
    state
        .app_state
        .agent_run_repo
        .create(run)
        .await
        .expect("seed active run");

    let axum::Json(first) = write_agent_workspace_review_artifact(
        State(state.clone()),
        Path(conversation_id.to_string()),
        Json(WriteAgentWorkspaceReviewArtifactRequest {
            title: Some("Workspace Review".to_string()),
            content: "Provisional overview".to_string(),
            requested_changes_title: Some("Workspace Review — Requested Changes".to_string()),
            requested_changes_content: "Provisional requested changes".to_string(),
            target_scope: Some(target.scope.clone()),
            head_sha: target.head_sha.clone(),
            diff_fingerprint: Some(target.diff_fingerprint.clone()),
            created_by_run_id: Some(run_id.to_string()),
            outcome: None,
            blocking_summary: None,
        }),
    )
    .await
    .expect("write provisional artifact pair");
    assert!(first.success);
    assert_eq!(first.artifact.version, 1);
    assert_eq!(first.requested_changes_artifact.version, 1);

    let axum::Json(second) = write_agent_workspace_review_artifact(
        State(state.clone()),
        Path(conversation_id.to_string()),
        Json(WriteAgentWorkspaceReviewArtifactRequest {
            title: Some("Workspace Review".to_string()),
            content: "Final overview".to_string(),
            requested_changes_title: Some("Workspace Review — Requested Changes".to_string()),
            requested_changes_content: "Final requested changes".to_string(),
            target_scope: Some(target.scope),
            head_sha: target.head_sha,
            diff_fingerprint: Some(target.diff_fingerprint),
            created_by_run_id: Some(run_id.to_string()),
            outcome: None,
            blocking_summary: None,
        }),
    )
    .await
    .expect("write final artifact pair");
    assert!(second.success);
    assert_eq!(second.artifact.version, 2);
    assert_eq!(second.requested_changes_artifact.version, 2);
    assert_eq!(
        second.previous_artifact_id.as_deref(),
        Some(first.artifact.id.as_str())
    );
    assert_eq!(
        second.previous_requested_changes_artifact_id.as_deref(),
        Some(first.requested_changes_artifact.id.as_str())
    );

    let monitor = state
        .app_state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&conversation_id)
        .await
        .expect("read monitor")
        .expect("persisted monitor");
    let monitor_artifact_id = monitor
        .review_artifact_id
        .clone()
        .expect("monitor overview artifact id");
    let monitor_requested_changes_artifact_id = monitor
        .review_requested_changes_artifact_id
        .clone()
        .expect("monitor requested changes artifact id");
    assert_eq!(monitor_artifact_id.as_str(), second.artifact.id);
    assert_eq!(
        monitor_requested_changes_artifact_id.as_str(),
        second.requested_changes_artifact.id
    );
    assert_eq!(monitor.review_artifact_version, Some(2));
    assert_eq!(monitor.review_requested_changes_artifact_version, Some(2));
    assert_eq!(
        monitor.previous_version_id.as_ref().map(|id| id.as_str()),
        Some(first.artifact.id.as_str())
    );
    assert_eq!(
        monitor
            .review_requested_changes_previous_version_id
            .as_ref()
            .map(|id| id.as_str()),
        Some(first.requested_changes_artifact.id.as_str())
    );

    let latest_artifact = state
        .app_state
        .artifact_repo
        .get_by_id(&monitor_artifact_id)
        .await
        .expect("load latest overview artifact")
        .expect("latest overview artifact");
    let latest_requested_changes_artifact = state
        .app_state
        .artifact_repo
        .get_by_id(&monitor_requested_changes_artifact_id)
        .await
        .expect("load latest requested changes artifact")
        .expect("latest requested changes artifact");
    assert!(matches!(
        latest_artifact.content,
        ArtifactContent::Inline { text } if text == "Final overview"
    ));
    assert!(matches!(
        latest_requested_changes_artifact.content,
        ArtifactContent::Inline { text } if text == "Final requested changes"
    ));

    let provisional_artifact = state
        .app_state
        .artifact_repo
        .get_by_id(&ArtifactId::from_string(first.artifact.id))
        .await
        .expect("load provisional overview artifact")
        .expect("provisional overview artifact");
    assert!(matches!(
        provisional_artifact.content,
        ArtifactContent::Inline { text } if text == "Provisional overview"
    ));
}

#[tokio::test]
async fn presentation_context_get_does_not_create_a_review_monitor() {
    let repo = tempfile::TempDir::new().expect("repo tempdir");
    let worktree = tempfile::TempDir::new().expect("worktree tempdir");
    let state = test_state();
    let conversation_id = ChatConversationId::new();
    let project = Project::new(
        "Read-only Review context".to_string(),
        repo.path().to_string_lossy().to_string(),
    );
    state
        .app_state
        .project_repo
        .create(project.clone())
        .await
        .expect("seed project");
    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.id = conversation_id;
    state
        .app_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("seed conversation");
    let workspace = AgentConversationWorkspace::new(
        conversation_id,
        project.id,
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        None,
        "ralphx/test/read-only-review-context".to_string(),
        worktree.path().to_string_lossy().to_string(),
    );
    state
        .app_state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("seed workspace");

    let axum::Json(context) = get_agent_workspace_review_context(
        State(state.clone()),
        Path(conversation_id.to_string()),
        HeaderMap::new(),
        Query(AgentWorkspaceReviewContextQuery::default()),
    )
    .await
    .expect("load presentation context");

    assert_eq!(context.monitor.status, "idle");
    assert!(state
        .app_state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&conversation_id)
        .await
        .expect("read monitor")
        .is_none());
}

// ── Recorded artifact outcome (degraded-settlement evidence) ─────────────

/// Owns the temp dirs for the lifetime of a fixture so the worktree stays valid.
struct ActiveReviewFixture {
    _repo: tempfile::TempDir,
    _worktrees: tempfile::TempDir,
    state: HttpServerState,
    conversation_id: ChatConversationId,
    target_scope: String,
    head_sha: Option<String>,
    diff_fingerprint: String,
    run_id: String,
}

/// Seeds a project + workspace with an uncommitted change and binds an active reviewer run,
/// which is the precondition for every artifact-write authority path.
async fn setup_active_review(slug: &str) -> ActiveReviewFixture {
    let repo = tempfile::TempDir::new().expect("repo tempdir");
    let worktrees = tempfile::TempDir::new().expect("worktree tempdir");
    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@example.com"]);
    git(repo.path(), &["config", "user.name", "RalphX Test"]);
    std::fs::write(repo.path().join("README.md"), "base\n").expect("write base file");
    git(repo.path(), &["add", "README.md"]);
    git(repo.path(), &["commit", "-m", "base"]);
    let base_sha = git(repo.path(), &["rev-parse", "HEAD"]);

    let state = test_state();
    let conversation_id = ChatConversationId::new();
    let mut project = Project::new(
        format!("Recorded Outcome {slug}"),
        repo.path().to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    state
        .app_state
        .project_repo
        .create(project.clone())
        .await
        .expect("seed project");
    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.id = conversation_id;
    state
        .app_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("seed conversation");

    let workspace_path = worktrees.path().join("workspace");
    let branch_name = format!("ralphx/test/{slug}");
    git(
        repo.path(),
        &[
            "worktree",
            "add",
            "-b",
            &branch_name,
            workspace_path.to_str().expect("workspace path"),
            "main",
        ],
    );
    std::fs::write(workspace_path.join("implementation.txt"), "change\n")
        .expect("write workspace change");
    let workspace = AgentConversationWorkspace::new(
        conversation_id,
        project.id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some(base_sha),
        branch_name,
        workspace_path.to_string_lossy().to_string(),
    );
    state
        .app_state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("seed workspace");

    let axum::Json(initial) = get_agent_workspace_review_context(
        State(state.clone()),
        Path(conversation_id.to_string()),
        HeaderMap::new(),
        Query(AgentWorkspaceReviewContextQuery::default()),
    )
    .await
    .expect("load initial context");
    let target = initial.target.expect("review target");
    let target_scope: AgentWorkspaceReviewTargetScope =
        target.scope.parse().expect("valid target scope");

    let review_conversation_id = ChatConversationId::new();
    let run = AgentRun::new(review_conversation_id);
    let run_id = run.id.to_string();
    let mut monitor = AgentWorkspaceReviewMonitor::new(conversation_id, project.id.clone());
    monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    monitor.review_outcome = AgentWorkspaceReviewOutcome::None;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Reviewing;
    monitor.current_target_scope = Some(target_scope);
    monitor.current_diff_fingerprint = Some(target.diff_fingerprint.clone());
    monitor.workspace_base_ref = Some(target.base_ref.clone());
    monitor.workspace_base_sha = target.base_sha.clone();
    monitor.workspace_head_ref = Some(target.head_ref.clone());
    monitor.workspace_head_sha = target.head_sha.clone();
    monitor.review_conversation_id = Some(review_conversation_id);
    monitor.last_run_id = Some(run_id.clone());
    state
        .app_state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("bind active review");
    state
        .app_state
        .agent_run_repo
        .create(run)
        .await
        .expect("seed active run");

    ActiveReviewFixture {
        _repo: repo,
        _worktrees: worktrees,
        state,
        conversation_id,
        target_scope: target.scope,
        head_sha: target.head_sha,
        diff_fingerprint: target.diff_fingerprint,
        run_id,
    }
}

impl ActiveReviewFixture {
    fn write_request(
        &self,
        outcome: Option<&str>,
        blocking_summary: Option<&str>,
    ) -> WriteAgentWorkspaceReviewArtifactRequest {
        WriteAgentWorkspaceReviewArtifactRequest {
            title: Some("Workspace Review".to_string()),
            content: "Overview".to_string(),
            requested_changes_title: None,
            requested_changes_content: "Requested changes".to_string(),
            target_scope: Some(self.target_scope.clone()),
            head_sha: self.head_sha.clone(),
            diff_fingerprint: Some(self.diff_fingerprint.clone()),
            created_by_run_id: Some(self.run_id.clone()),
            outcome: outcome.map(str::to_string),
            blocking_summary: blocking_summary.map(str::to_string),
        }
    }

    async fn monitor(&self) -> AgentWorkspaceReviewMonitor {
        self.state
            .app_state
            .agent_conversation_workspace_repo
            .get_workspace_review_monitor(&self.conversation_id)
            .await
            .expect("read monitor")
            .expect("persisted monitor")
    }
}

#[tokio::test]
async fn artifact_write_records_typed_outcome_scoped_to_the_writing_run() {
    let fixture = setup_active_review("records-typed-outcome").await;

    let _written = write_agent_workspace_review_artifact(
        State(fixture.state.clone()),
        Path(fixture.conversation_id.to_string()),
        Json(fixture.write_request(Some("blocking"), Some("Missing rollback path"))),
    )
    .await
    .expect("write artifact pair with recorded outcome");

    let monitor = fixture.monitor().await;
    assert_eq!(
        monitor.review_artifact_recorded_outcome,
        Some(AgentWorkspaceReviewArtifactOutcome::Blocking)
    );
    assert_eq!(
        monitor.review_artifact_recorded_outcome_run_id.as_deref(),
        Some(fixture.run_id.as_str())
    );
    assert_eq!(
        monitor.review_artifact_recorded_blocking_summary.as_deref(),
        Some("Missing rollback path")
    );
    assert!(monitor.has_recorded_outcome_for_run(&fixture.run_id));
    assert!(!monitor.has_recorded_outcome_for_run("some-other-run"));
}

/// A write that carries no outcome must not leave the previous write's evidence behind, or a
/// later run could degrade-settle from an artifact it did not produce.
#[tokio::test]
async fn artifact_write_without_outcome_clears_previously_recorded_evidence() {
    let fixture = setup_active_review("clears-recorded-evidence").await;

    let _written = write_agent_workspace_review_artifact(
        State(fixture.state.clone()),
        Path(fixture.conversation_id.to_string()),
        Json(fixture.write_request(Some("passed"), None)),
    )
    .await
    .expect("write artifact pair with recorded outcome");
    assert!(fixture
        .monitor()
        .await
        .review_artifact_recorded_outcome
        .is_some());

    let _rewritten = write_agent_workspace_review_artifact(
        State(fixture.state.clone()),
        Path(fixture.conversation_id.to_string()),
        Json(fixture.write_request(None, None)),
    )
    .await
    .expect("write artifact pair without recorded outcome");

    let monitor = fixture.monitor().await;
    assert!(monitor.review_artifact_recorded_outcome.is_none());
    assert!(monitor.review_artifact_recorded_outcome_run_id.is_none());
    assert!(monitor.review_artifact_recorded_blocking_summary.is_none());
}

#[tokio::test]
async fn artifact_write_rejects_unknown_outcome_with_bad_request() {
    let fixture = setup_active_review("rejects-unknown-outcome").await;

    let error = write_agent_workspace_review_artifact(
        State(fixture.state.clone()),
        Path(fixture.conversation_id.to_string()),
        Json(fixture.write_request(Some("run_failed"), None)),
    )
    .await
    .expect_err("unknown outcome should be rejected");

    assert_eq!(error.0, StatusCode::BAD_REQUEST);
    assert!(fixture
        .monitor()
        .await
        .review_artifact_recorded_outcome
        .is_none());
}

/// Mirrors the typed-completion rule: a blocking gate the user cannot act on is worse than none,
/// and the fixer-start path fails closed without a summary.
#[tokio::test]
async fn artifact_write_rejects_blocking_outcome_without_summary() {
    let fixture = setup_active_review("rejects-blocking-without-summary").await;

    let error = write_agent_workspace_review_artifact(
        State(fixture.state.clone()),
        Path(fixture.conversation_id.to_string()),
        Json(fixture.write_request(Some("blocking"), Some("   "))),
    )
    .await
    .expect_err("blocking outcome without summary should be rejected");

    assert_eq!(error.0, StatusCode::BAD_REQUEST);
    assert!(fixture
        .monitor()
        .await
        .review_artifact_recorded_outcome
        .is_none());
}

/// Target metadata mismatch is already a 409; it must run before anything is recorded.
#[tokio::test]
async fn artifact_write_with_mismatched_target_records_nothing() {
    let fixture = setup_active_review("mismatched-target").await;

    let mut request = fixture.write_request(Some("passed"), None);
    request.diff_fingerprint = Some("fingerprint-from-another-target".to_string());
    let error = write_agent_workspace_review_artifact(
        State(fixture.state.clone()),
        Path(fixture.conversation_id.to_string()),
        Json(request),
    )
    .await
    .expect_err("stale fingerprint should be rejected");

    assert_eq!(error.0, StatusCode::CONFLICT);
    assert!(fixture
        .monitor()
        .await
        .review_artifact_recorded_outcome
        .is_none());
}

// ── Context payload compaction ───────────────────────────────────────────

/// The model path drops publication events; the UI path (default) keeps them.
#[tokio::test]
async fn context_include_events_false_omits_publication_events_and_default_keeps_them() {
    let fixture = setup_active_review("include-events").await;
    fixture
        .state
        .app_state
        .agent_conversation_workspace_repo
        .append_publication_event(
            ralphx_lib::domain::entities::AgentConversationWorkspacePublicationEvent::new(
                fixture.conversation_id,
                "workspace_review",
                "reviewing",
                "Review started".to_string(),
                None,
            ),
        )
        .await
        .expect("publication event should persist");

    let axum::Json(default_context) = get_agent_workspace_review_context(
        State(fixture.state.clone()),
        Path(fixture.conversation_id.to_string()),
        HeaderMap::new(),
        Query(AgentWorkspaceReviewContextQuery::default()),
    )
    .await
    .expect("default context should load");
    assert!(
        !default_context.events.is_empty(),
        "the UI path must keep publication events"
    );

    let axum::Json(model_context) = get_agent_workspace_review_context(
        State(fixture.state.clone()),
        Path(fixture.conversation_id.to_string()),
        HeaderMap::new(),
        Query(AgentWorkspaceReviewContextQuery {
            include_events: Some(false),
            ..Default::default()
        }),
    )
    .await
    .expect("model context should load");
    assert!(
        model_context.events.is_empty(),
        "the model path must not carry publication events"
    );
    // Everything else the reviewer acts on is unchanged.
    assert_eq!(model_context.is_current, default_context.is_current);
    assert_eq!(
        model_context.can_mutate_review_state,
        default_context.can_mutate_review_state
    );
}
