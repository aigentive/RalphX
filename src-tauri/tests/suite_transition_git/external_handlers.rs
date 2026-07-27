// Integration tests for external API handlers (Phase 4 + Phase 5)
//
// Tests list_projects_http, get_project_status_http, get_pipeline_overview_http,
// start_ideation_http, poll_events_http, get_task_detail_http,
// get_task_review_summary_http, get_merge_pipeline_http, and related handlers
// using the in-memory AppState.

use axum::{
    extract::{Path, Query, State},
    Json,
};
use ralphx_lib::application::{
    agent_conversation_workspace::{
        prepare_agent_conversation_workspace, AgentConversationWorkspaceBaseSelection,
    },
    AppState, InteractiveProcessKey,
};
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::agents::{
    AgentHarnessKind, AgentLane, AgentLaneSettings, AgentRole, AgenticClient, LogicalEffort,
};
use ralphx_lib::domain::entities::{
    ideation::{
        ChatMessage, IdeationSession, IdeationSessionBuilder, IdeationSessionStatus, SessionOrigin,
        SessionPurpose, VerificationStatus,
    },
    project::{GitMode, Project},
    task::Task,
    types::ProjectId,
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentRun, AgentRunActionKind,
    Artifact, ArtifactType, ChatConversation, IdeationAnalysisBaseRefKind, IdeationSessionId,
    InternalStatus, Priority, ProposalCategory, TaskProposal, VerificationRunSnapshot,
};
use ralphx_lib::domain::ideation::TasksFeatureState;
use ralphx_lib::domain::services::running_agent_registry::RunningAgentKey;
use ralphx_lib::error::AppError;
use ralphx_lib::http_server::handlers::*;
use ralphx_lib::http_server::project_scope::ProjectScope;
use ralphx_lib::http_server::types::HttpServerState;
use ralphx_lib::infrastructure::agents::mock::{MockAgenticClient, MockCallType};
use std::path::{Path as FsPath, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use std::{fs, os::unix::fs::PermissionsExt};
use tempfile::TempDir;

// ============================================================================
// Setup helpers
// ============================================================================

async fn setup_test_state() -> HttpServerState {
    let app_state = Arc::new(AppState::new_test());
    let execution_state = Arc::new(ExecutionState::new());
    HttpServerState {
        app_state,
        execution_state,
        delegation_service: Default::default(),
    }
}

fn setup_test_state_with_app_state(app_state: Arc<AppState>) -> HttpServerState {
    let execution_state = Arc::new(ExecutionState::new());
    HttpServerState {
        app_state,
        execution_state,
        delegation_service: Default::default(),
    }
}

fn prepend_fake_codex_to_path(fake_codex_path: &FsPath) -> crate::support::env::EnvVarGuard {
    crate::support::env::prepend_to_path(
        fake_codex_path
            .parent()
            .expect("fake codex script should have parent dir"),
    )
}

fn install_fake_codex_cli() -> (TempDir, PathBuf) {
    let tempdir = TempDir::new().expect("tempdir");
    let script_path = tempdir.path().join("codex");
    let script = r#"#!/bin/sh
if [ "$1" = "--help" ]; then
cat <<'EOF'
Codex CLI

Commands:
  exec        Run Codex non-interactively [aliases: e]
  mcp         Manage external MCP servers for Codex
  resume      Resume a previous interactive session

Options:
  -c, --config <key=value>
  -m, --model <MODEL>
  -s, --sandbox <SANDBOX_MODE>
      --search
      --add-dir <DIR>
EOF
exit 0
fi

if [ "$1" = "--version" ]; then
echo "codex-cli 0.116.0"
exit 0
fi

if [ "$1" = "exec" ] && [ "$2" = "--help" ]; then
cat <<'EOF'
Run Codex non-interactively

Usage: codex exec [OPTIONS] [PROMPT] [COMMAND]

Options:
  -c, --config <key=value>
  -m, --model <MODEL>
  -s, --sandbox <SANDBOX_MODE>
      --add-dir <DIR>
      --json
  -C, --cd <DIR>
      --skip-git-repo-check
EOF
exit 0
fi

if [ "$1" = "exec" ]; then
printf '%s\n' '{"type":"thread.started","thread_id":"external-thread-1"}'
printf '%s\n' '{"type":"item.completed","item":{"type":"agent_message","text":"MOCK_EXTERNAL_COMPLETION"}}'
printf '%s\n' '{"type":"turn.completed","usage":{"input_tokens":11,"cached_input_tokens":2,"output_tokens":7}}'
exit 0
fi

echo "unsupported invocation" >&2
exit 2
"#;
    fs::write(&script_path, script).expect("write fake codex cli");
    let mut permissions = fs::metadata(&script_path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&script_path, permissions).expect("chmod fake codex cli");
    (tempdir, script_path)
}

fn make_project(id: &str, name: &str) -> Project {
    Project {
        id: ProjectId::from_string(id.to_string()),
        name: name.to_string(),
        working_directory: "/tmp".to_string(),
        git_mode: GitMode::Worktree,
        base_branch: None,
        worktree_parent_directory: None,
        use_feature_branches: true,
        merge_validation_mode: Default::default(),
        merge_strategy: Default::default(),
        detected_analysis: None,
        custom_analysis: None,
        analyzed_at: None,
        github_pr_enabled: false,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        archived_at: None,
    }
}

fn unrestricted_scope() -> ProjectScope {
    ProjectScope(None)
}

fn scoped(ids: &[&str]) -> ProjectScope {
    let vec: Vec<ProjectId> = ids
        .iter()
        .map(|s| ProjectId::from_string(s.to_string()))
        .collect();
    ProjectScope(Some(vec))
}

fn run_task_diff_git(repo: &FsPath, args: &[&str]) {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git command should run");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn run_task_diff_git_output(repo: &FsPath, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git command should run");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("git output is utf-8")
        .trim()
        .to_string()
}

fn setup_task_diff_repo_with_captured_base_and_advanced_main() -> (TempDir, String) {
    let dir = TempDir::new().expect("create temp dir");
    let repo = dir.path();

    run_task_diff_git(repo, &["init", "-b", "main"]);
    run_task_diff_git(repo, &["config", "user.email", "test@test.com"]);
    run_task_diff_git(repo, &["config", "user.name", "Test"]);

    fs::write(repo.join("README.md"), "# test repo\n").expect("write readme");
    run_task_diff_git(repo, &["add", "."]);
    run_task_diff_git(repo, &["commit", "-m", "initial commit"]);
    let captured_base_sha = run_task_diff_git_output(repo, &["rev-parse", "HEAD"]);

    run_task_diff_git(repo, &["checkout", "-b", "task/test"]);
    fs::write(repo.join("task.txt"), "task work\n").expect("write task");
    run_task_diff_git(repo, &["add", "."]);
    run_task_diff_git(repo, &["commit", "-m", "feat: selected task work"]);

    run_task_diff_git(repo, &["checkout", "main"]);
    fs::write(repo.join("base.txt"), "base moved ahead\n").expect("write base");
    run_task_diff_git(repo, &["add", "."]);
    run_task_diff_git(repo, &["commit", "-m", "fix: unrelated base work"]);
    run_task_diff_git(repo, &["checkout", "task/test"]);

    (dir, captured_base_sha)
}

async fn setup_ideation_parent_workspace(
    state: &HttpServerState,
    project_id: &str,
) -> (
    TempDir,
    TempDir,
    Project,
    ChatConversation,
    AgentConversationWorkspace,
) {
    let repo_dir = tempfile::TempDir::new().unwrap();
    let repo_path = repo_dir.path();
    std::process::Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(repo_path)
        .output()
        .expect("git init");
    std::process::Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(repo_path)
        .output()
        .expect("git config email");
    std::process::Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(repo_path)
        .output()
        .expect("git config name");
    std::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(repo_path)
        .output()
        .expect("git commit");
    let worktree_parent = tempfile::TempDir::new().unwrap();

    let mut project = Project::new(
        "Workspace Project".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.id = ProjectId::from_string(project_id.to_string());
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktree_parent.path().to_string_lossy().to_string());
    let project = state.app_state.project_repo.create(project).await.unwrap();

    let conversation = state
        .app_state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project.id.clone()))
        .await
        .expect("create project conversation");
    let workspace = prepare_agent_conversation_workspace(
        &project,
        &conversation.id,
        AgentConversationWorkspaceMode::Ideation,
        AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
            branch_mode: None,
            base_ref: Some("main".to_string()),
            display_name: Some("Project default (main)".to_string()),
            source_pull_request: None,
        },
    )
    .await
    .expect("prepare agent workspace");
    state
        .app_state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("save agent workspace");

    (repo_dir, worktree_parent, project, conversation, workspace)
}

fn tauri_parent_headers(conversation: &ChatConversation) -> axum::http::HeaderMap {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert("x-ralphx-tauri-mcp", "1".parse().unwrap());
    headers.insert(
        "x-ralphx-parent-conversation-id",
        conversation.id.as_str().parse().unwrap(),
    );
    headers
}

// ============================================================================
// list_projects_http
// ============================================================================

#[tokio::test]
async fn test_list_projects_no_scope() {
    let state = setup_test_state().await;

    // Create two projects
    let p1 = make_project("proj-alpha", "Alpha");
    let p2 = make_project("proj-beta", "Beta");
    state.app_state.project_repo.create(p1).await.unwrap();
    state.app_state.project_repo.create(p2).await.unwrap();

    let result = list_projects_http(State(state), unrestricted_scope()).await;
    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert_eq!(response.projects.len(), 2);

    let names: Vec<&str> = response.projects.iter().map(|p| p.name.as_str()).collect();
    assert!(names.contains(&"Alpha"));
    assert!(names.contains(&"Beta"));
}

#[tokio::test]
async fn test_list_projects_with_scope() {
    let state = setup_test_state().await;

    let p1 = make_project("proj-alpha", "Alpha");
    let p2 = make_project("proj-beta", "Beta");
    state.app_state.project_repo.create(p1).await.unwrap();
    state.app_state.project_repo.create(p2).await.unwrap();

    // Scope to proj-alpha only
    let result = list_projects_http(State(state), scoped(&["proj-alpha"])).await;
    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert_eq!(response.projects.len(), 1);
    assert_eq!(response.projects[0].name, "Alpha");
    assert_eq!(response.projects[0].id, "proj-alpha");
}

#[tokio::test]
async fn test_list_projects_empty_scope_returns_nothing() {
    let state = setup_test_state().await;

    let p1 = make_project("proj-alpha", "Alpha");
    state.app_state.project_repo.create(p1).await.unwrap();

    // Scope to an unrelated project
    let result = list_projects_http(State(state), scoped(&["proj-other"])).await;
    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert_eq!(response.projects.len(), 0);
}

#[tokio::test]
async fn test_list_projects_external_does_not_expose_working_directory() {
    // Security boundary regression: external ProjectSummary must NOT include
    // working_directory. Filesystem paths must never be visible to API key holders.
    let state = setup_test_state().await;

    let p1 = make_project("sec-proj-1", "SecureProject");
    state.app_state.project_repo.create(p1).await.unwrap();

    let result = list_projects_http(State(state), unrestricted_scope()).await;
    let response = result.unwrap().0;
    assert_eq!(response.projects.len(), 1);

    // Serialize and verify no working_directory field is present
    let json_str = serde_json::to_string(&response.projects[0]).unwrap();
    assert!(
        !json_str.contains("working_directory"),
        "external ProjectSummary must not contain working_directory: {}",
        json_str
    );

    // Verify expected fields are present via deserialized map
    let obj: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&json_str).unwrap();
    assert!(obj.contains_key("id"), "missing id");
    assert!(obj.contains_key("name"), "missing name");
    assert!(obj.contains_key("task_count"), "missing task_count");
}

// ============================================================================
// get_project_status_http
// ============================================================================

#[tokio::test]
async fn test_get_project_status_returns_task_counts() {
    let state = setup_test_state().await;

    let project_id = "proj-status-test";
    let p = make_project(project_id, "Status Test");
    state.app_state.project_repo.create(p).await.unwrap();

    // Create tasks with various statuses
    let task_backlog = Task::new(
        ProjectId::from_string(project_id.to_string()),
        "Backlog task".to_string(),
    );
    let mut task_executing = Task::new(
        ProjectId::from_string(project_id.to_string()),
        "Executing task".to_string(),
    );
    task_executing.internal_status = InternalStatus::Executing;
    let mut task_merged = Task::new(
        ProjectId::from_string(project_id.to_string()),
        "Merged task".to_string(),
    );
    task_merged.internal_status = InternalStatus::Merged;

    state
        .app_state
        .task_repo
        .create(task_backlog)
        .await
        .unwrap();
    state
        .app_state
        .task_repo
        .create(task_executing)
        .await
        .unwrap();
    state.app_state.task_repo.create(task_merged).await.unwrap();

    let result = get_project_status_http(
        State(state),
        unrestricted_scope(),
        Path(project_id.to_string()),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert_eq!(response.project.id, project_id);
    assert_eq!(response.project.name, "Status Test");
    assert_eq!(response.task_counts.total, 3);
    assert_eq!(response.task_counts.backlog, 1);
    assert_eq!(response.task_counts.executing, 1);
    assert_eq!(response.task_counts.merged, 1);
}

#[tokio::test]
async fn test_get_project_status_scope_violation() {
    let state = setup_test_state().await;

    let project_id = "proj-secret";
    let p = make_project(project_id, "Secret Project");
    state.app_state.project_repo.create(p).await.unwrap();

    // Request scoped to a different project
    let result = get_project_status_http(
        State(state),
        scoped(&["proj-other"]),
        Path(project_id.to_string()),
    )
    .await;

    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), axum::http::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_get_project_status_not_found() {
    let state = setup_test_state().await;

    let result = get_project_status_http(
        State(state),
        unrestricted_scope(),
        Path("nonexistent-proj".to_string()),
    )
    .await;

    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), axum::http::StatusCode::NOT_FOUND);
}

// ============================================================================
// get_pipeline_overview_http
// ============================================================================

#[tokio::test]
async fn test_get_pipeline_overview_counts_stages() {
    let state = setup_test_state().await;

    let project_id = "proj-pipeline";
    let p = make_project(project_id, "Pipeline Project");
    state.app_state.project_repo.create(p).await.unwrap();

    let mut task_pending = Task::new(
        ProjectId::from_string(project_id.to_string()),
        "Pending".to_string(),
    );
    task_pending.internal_status = InternalStatus::Ready;

    let mut task_exec = Task::new(
        ProjectId::from_string(project_id.to_string()),
        "Executing".to_string(),
    );
    task_exec.internal_status = InternalStatus::Executing;

    let mut task_merged = Task::new(
        ProjectId::from_string(project_id.to_string()),
        "Merged".to_string(),
    );
    task_merged.internal_status = InternalStatus::Merged;

    state
        .app_state
        .task_repo
        .create(task_pending)
        .await
        .unwrap();
    state.app_state.task_repo.create(task_exec).await.unwrap();
    state.app_state.task_repo.create(task_merged).await.unwrap();

    let result = get_pipeline_overview_http(
        State(state),
        unrestricted_scope(),
        Path(project_id.to_string()),
        Query(GetPipelineParams { since: None }),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert_eq!(response.project_id, project_id);
    assert_eq!(response.stages.pending, 1);
    assert_eq!(response.stages.executing, 1);
    assert_eq!(response.stages.merged, 1);
    assert!(response.changed_tasks.is_none());
}

#[tokio::test]
async fn test_get_pipeline_overview_scope_violation() {
    let state = setup_test_state().await;

    let project_id = "proj-pipeline-secret";
    let p = make_project(project_id, "Secret Pipeline");
    state.app_state.project_repo.create(p).await.unwrap();

    let result = get_pipeline_overview_http(
        State(state),
        scoped(&["proj-different"]),
        Path(project_id.to_string()),
        Query(GetPipelineParams { since: None }),
    )
    .await;

    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().status,
        axum::http::StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn test_get_pipeline_overview_since_filters_changed_tasks() {
    let state = setup_test_state().await;

    let project_id = "proj-pipeline-since";
    let p = make_project(project_id, "Pipeline Since Project");
    state.app_state.project_repo.create(p).await.unwrap();

    let past = chrono::Utc::now() - chrono::Duration::hours(2);
    let since = chrono::Utc::now() - chrono::Duration::hours(1);

    // task_old: updated before `since` (should NOT appear in changed_tasks)
    let mut task_old = Task::new(
        ProjectId::from_string(project_id.to_string()),
        "Old Task".to_string(),
    );
    task_old.updated_at = past;
    task_old.internal_status = InternalStatus::Ready;

    // task_new: updated after `since` (should appear in changed_tasks)
    let mut task_new = Task::new(
        ProjectId::from_string(project_id.to_string()),
        "New Task".to_string(),
    );
    task_new.updated_at = chrono::Utc::now();
    task_new.internal_status = InternalStatus::Executing;

    state.app_state.task_repo.create(task_old).await.unwrap();
    state.app_state.task_repo.create(task_new).await.unwrap();

    let result = get_pipeline_overview_http(
        State(state),
        unrestricted_scope(),
        Path(project_id.to_string()),
        Query(GetPipelineParams {
            since: Some(since.to_rfc3339()),
        }),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    // Stage counts are over ALL tasks
    assert_eq!(response.stages.pending, 1);
    assert_eq!(response.stages.executing, 1);
    // changed_tasks contains only the new task
    let changed = response
        .changed_tasks
        .expect("changed_tasks should be present");
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0].title, "New Task");
}

#[tokio::test]
async fn test_get_pipeline_overview_since_invalid_timestamp_returns_error() {
    let state = setup_test_state().await;

    let project_id = "proj-pipeline-invalid";
    let p = make_project(project_id, "Pipeline Invalid");
    state.app_state.project_repo.create(p).await.unwrap();

    let result = get_pipeline_overview_http(
        State(state),
        unrestricted_scope(),
        Path(project_id.to_string()),
        Query(GetPipelineParams {
            since: Some("not-a-timestamp".to_string()),
        }),
    )
    .await;

    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().status,
        axum::http::StatusCode::UNPROCESSABLE_ENTITY
    );
}

// ============================================================================
// start_ideation_http
// ============================================================================

#[tokio::test]
async fn test_start_ideation_creates_session() {
    let state = setup_test_state().await;

    let project_id = "proj-ideation";
    let p = make_project(project_id, "Ideation Project");
    state.app_state.project_repo.create(p).await.unwrap();

    let result = start_ideation_http(
        State(state.clone()),
        unrestricted_scope(),
        axum::http::HeaderMap::new(),
        Json(StartIdeationRequest {
            project_id: project_id.to_string(),
            title: Some("New feature brainstorm".to_string()),
            prompt: None,
            initial_prompt: Some("Let's ideate on authentication".to_string()),
            idempotency_key: None,
        }),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert!(!response.session_id.is_empty());
    assert_eq!(response.status, "ideating");
}

#[tokio::test]
async fn test_start_ideation_tauri_parent_workspace_binds_analysis_and_links_workspace() {
    use ralphx_lib::application::agent_conversation_workspace::{
        prepare_agent_conversation_workspace, AgentConversationWorkspaceBaseSelection,
    };
    use ralphx_lib::domain::entities::{
        AgentConversationWorkspaceMode, ChatConversation, IdeationAnalysisBaseRefKind,
        IdeationAnalysisWorkspaceKind,
    };

    let state = setup_test_state().await;
    let repo_dir = tempfile::TempDir::new().unwrap();
    let repo_path = repo_dir.path();
    std::process::Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(repo_path)
        .output()
        .expect("git init");
    std::process::Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(repo_path)
        .output()
        .expect("git config email");
    std::process::Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(repo_path)
        .output()
        .expect("git config name");
    std::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(repo_path)
        .output()
        .expect("git commit");
    let worktree_parent = tempfile::TempDir::new().unwrap();

    let project_id = "proj-parent-workspace";
    let mut project = Project::new(
        "Workspace Project".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.id = ProjectId::from_string(project_id.to_string());
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktree_parent.path().to_string_lossy().to_string());
    let project = state.app_state.project_repo.create(project).await.unwrap();

    let conversation = state
        .app_state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project.id.clone()))
        .await
        .expect("create project conversation");
    let workspace = prepare_agent_conversation_workspace(
        &project,
        &conversation.id,
        AgentConversationWorkspaceMode::Ideation,
        AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
            branch_mode: None,
            base_ref: Some("main".to_string()),
            display_name: Some("Project default (main)".to_string()),
            source_pull_request: None,
        },
    )
    .await
    .expect("prepare agent workspace");
    state
        .app_state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("save agent workspace");

    let mut headers = axum::http::HeaderMap::new();
    headers.insert("x-ralphx-tauri-mcp", "1".parse().unwrap());
    headers.insert(
        "x-ralphx-parent-conversation-id",
        conversation.id.as_str().parse().unwrap(),
    );

    let result = start_ideation_http(
        State(state.clone()),
        unrestricted_scope(),
        headers,
        Json(StartIdeationRequest {
            project_id: project_id.to_string(),
            title: Some("Workspace ideation".to_string()),
            prompt: None,
            initial_prompt: None,
            idempotency_key: None,
        }),
    )
    .await
    .expect("start ideation should bind parent workspace")
    .0;

    assert_eq!(
        result.parent_conversation_id,
        Some(conversation.id.as_str())
    );
    assert_eq!(
        result.workspace_branch.as_deref(),
        Some(workspace.branch_name.as_str())
    );

    let session = state
        .app_state
        .ideation_session_repo
        .get_by_id(&IdeationSessionId::from_string(result.session_id.clone()))
        .await
        .unwrap()
        .expect("session should exist");
    assert_eq!(
        session.analysis.workspace_kind,
        IdeationAnalysisWorkspaceKind::IdeationWorktree
    );
    assert_eq!(
        session.analysis.workspace_path.as_deref(),
        Some(workspace.worktree_path.as_str())
    );

    let linked_workspace = state
        .app_state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation.id)
        .await
        .unwrap()
        .expect("workspace should still exist");
    assert_eq!(
        linked_workspace.linked_ideation_session_id.as_ref(),
        Some(&session.id)
    );
}

#[tokio::test]
async fn test_start_ideation_tauri_parent_workspace_reuses_linked_session() {
    let state = setup_test_state().await;
    let project_id = "proj-parent-workspace-reuse";
    let (_repo_dir, _worktree_parent, _project, conversation, _workspace) =
        setup_ideation_parent_workspace(&state, project_id).await;

    let first = start_ideation_http(
        State(state.clone()),
        unrestricted_scope(),
        tauri_parent_headers(&conversation),
        Json(StartIdeationRequest {
            project_id: project_id.to_string(),
            title: Some("Initial workspace ideation".to_string()),
            prompt: None,
            initial_prompt: None,
            idempotency_key: None,
        }),
    )
    .await
    .expect("initial start should create linked session")
    .0;

    let second = start_ideation_http(
        State(state.clone()),
        unrestricted_scope(),
        tauri_parent_headers(&conversation),
        Json(StartIdeationRequest {
            project_id: project_id.to_string(),
            title: Some("Follow-up workspace ideation".to_string()),
            prompt: Some("Use the existing ideation session for this follow-up".to_string()),
            initial_prompt: None,
            idempotency_key: None,
        }),
    )
    .await
    .expect("second start should reuse linked session")
    .0;

    assert_eq!(second.session_id, first.session_id);
    assert!(!second.agent_spawned);
    assert_eq!(second.next_action, "use_existing_session");
    let conversation_id = conversation.id.as_str();
    assert_eq!(
        second.parent_conversation_id.as_deref(),
        Some(conversation_id.as_str())
    );

    let active_sessions = state
        .app_state
        .ideation_session_repo
        .list_active_external_by_project(&ProjectId::from_string(project_id.to_string()))
        .await
        .unwrap();
    assert_eq!(active_sessions.len(), 1);
}

#[tokio::test]
async fn test_start_ideation_scope_violation() {
    let state = setup_test_state().await;

    let project_id = "proj-ideation-secret";
    let p = make_project(project_id, "Secret Ideation");
    state.app_state.project_repo.create(p).await.unwrap();

    let result = start_ideation_http(
        State(state),
        scoped(&["proj-other"]),
        axum::http::HeaderMap::new(),
        Json(StartIdeationRequest {
            project_id: project_id.to_string(),
            title: Some("Forbidden".to_string()),
            prompt: None,
            initial_prompt: None,
            idempotency_key: None,
        }),
    )
    .await;

    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().status,
        axum::http::StatusCode::FORBIDDEN
    );
}

// ============================================================================
// start_ideation_http — extended behavior tests
// ============================================================================

/// No prompt provided → session created, agent_spawned: false (no attempt to spawn)
#[tokio::test]
async fn test_start_ideation_no_prompt_agent_not_spawned() {
    let state = setup_test_state().await;

    let project_id = "proj-no-prompt";
    let p = make_project(project_id, "No Prompt Project");
    state.app_state.project_repo.create(p).await.unwrap();

    let result = start_ideation_http(
        State(state.clone()),
        unrestricted_scope(),
        axum::http::HeaderMap::new(),
        Json(StartIdeationRequest {
            project_id: project_id.to_string(),
            title: None,
            prompt: None,
            initial_prompt: None,
            idempotency_key: None,
        }),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert!(!response.session_id.is_empty());
    assert_eq!(response.status, "ideating");
    // No prompt → no agent spawned
    assert!(
        !response.agent_spawned,
        "agent_spawned must be false when no prompt is given"
    );
}

#[tokio::test]
async fn test_start_ideation_persists_prompt_when_execution_paused() {
    let state = setup_test_state().await;

    let project_id = "proj-paused-prompt";
    let prompt = "Verify and fix the app-wide font scaling issue";
    let p = make_project(project_id, "Paused Prompt Project");
    state.app_state.project_repo.create(p).await.unwrap();
    state.execution_state.pause();

    let result = start_ideation_http(
        State(state.clone()),
        unrestricted_scope(),
        axum::http::HeaderMap::new(),
        Json(StartIdeationRequest {
            project_id: project_id.to_string(),
            title: None,
            prompt: Some(prompt.to_string()),
            initial_prompt: None,
            idempotency_key: None,
        }),
    )
    .await;

    assert!(
        result.is_ok(),
        "paused start must return a durable deferred session"
    );
    let response = result.unwrap().0;
    assert!(
        !response.agent_spawned,
        "paused ideation must not report a spawned agent"
    );
    assert_eq!(response.next_action, "wait_for_resume");
    assert_eq!(response.pending_initial_prompt.as_deref(), Some(prompt));
    assert!(
        response
            .agent_spawn_blocked_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("execution paused")),
        "paused start must surface a concrete blocked reason"
    );

    let session_id = IdeationSessionId::from_string(response.session_id);
    let stored = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .unwrap()
        .expect("session should exist");
    assert_eq!(stored.pending_initial_prompt.as_deref(), Some(prompt));
}

/// With title → session created with that title preserved in the response session_id and
/// verifiable by fetching the session from the repo.
#[tokio::test]
async fn test_start_ideation_with_title_preserved() {
    let state = setup_test_state().await;

    let project_id = "proj-with-title";
    let p = make_project(project_id, "Title Project");
    state.app_state.project_repo.create(p).await.unwrap();

    let result = start_ideation_http(
        State(state.clone()),
        unrestricted_scope(),
        axum::http::HeaderMap::new(),
        Json(StartIdeationRequest {
            project_id: project_id.to_string(),
            title: Some("My Custom Session Title".to_string()),
            prompt: None,
            initial_prompt: None,
            idempotency_key: None,
        }),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert!(!response.session_id.is_empty());
    assert_eq!(response.status, "ideating");

    // Verify the session was stored with the correct title
    let session_id = IdeationSessionId::from_string(response.session_id.clone());
    let fetched = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .unwrap()
        .expect("Session should exist in repo");
    assert_eq!(
        fetched.title.as_deref(),
        Some("My Custom Session Title"),
        "Session title should be persisted as provided"
    );
}

#[tokio::test]
async fn test_start_ideation_without_title_assigns_default_title() {
    let state = setup_test_state().await;

    let project_id = "proj-default-title";
    let p = make_project(project_id, "Default Title Project");
    state.app_state.project_repo.create(p).await.unwrap();

    let result = start_ideation_http(
        State(state.clone()),
        unrestricted_scope(),
        axum::http::HeaderMap::new(),
        Json(StartIdeationRequest {
            project_id: project_id.to_string(),
            title: None,
            prompt: Some("Hello from external ideation".to_string()),
            initial_prompt: None,
            idempotency_key: None,
        }),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap().0;

    let session_id = IdeationSessionId::from_string(response.session_id.clone());
    let fetched = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .unwrap()
        .expect("Session should exist in repo");

    let title = fetched
        .title
        .expect("External ideation should not leave title empty");
    assert!(
        !title.trim().is_empty(),
        "External ideation default title must be non-empty"
    );
    assert_ne!(
        title, "Untitled Plan",
        "External ideation should assign a real default title instead of relying on UI fallback"
    );
}

#[tokio::test]
async fn test_start_ideation_codex_lane_routes_session_namer_to_project_harness_client() {
    let (_fake_codex_dir, fake_codex_path) = install_fake_codex_cli();
    let _codex_cli_guard = prepend_fake_codex_to_path(&fake_codex_path);
    let default_mock_impl = Arc::new(MockAgenticClient::new());
    let default_mock: Arc<dyn AgenticClient> = default_mock_impl.clone();
    let codex_mock_impl = Arc::new(MockAgenticClient::new());
    let codex_mock: Arc<dyn AgenticClient> = codex_mock_impl.clone();
    let app_state = Arc::new(
        AppState::new_test()
            .with_agent_client(default_mock.clone())
            .with_harness_agent_client(AgentHarnessKind::Codex, codex_mock),
    );
    let state = setup_test_state_with_app_state(app_state.clone());

    let project_id = "proj-fresh-codex-bootstrap";
    let p = make_project(project_id, "Fresh Codex Bootstrap Project");
    state.app_state.project_repo.create(p).await.unwrap();

    let mut codex_lane = AgentLaneSettings::new(AgentHarnessKind::Codex);
    codex_lane.model = Some("gpt-5.4".to_string());
    codex_lane.effort = Some(LogicalEffort::XHigh);
    state
        .app_state
        .agent_lane_settings_repo
        .upsert_for_project(project_id, AgentLane::IdeationPrimary, &codex_lane)
        .await
        .unwrap();

    let result = start_ideation_http(
        State(state.clone()),
        unrestricted_scope(),
        axum::http::HeaderMap::new(),
        Json(StartIdeationRequest {
            project_id: project_id.to_string(),
            title: None,
            prompt: Some("hello from fresh ideation".to_string()),
            initial_prompt: None,
            idempotency_key: None,
        }),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert!(
        response.agent_spawned,
        "mock clients should allow the ideation send path to spawn"
    );

    let codex_calls = {
        let mut calls = Vec::new();
        for _ in 0..10 {
            calls = codex_mock_impl.get_spawn_calls().await;
            if !calls.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        calls
    };
    let session_namer_prompt = codex_calls.iter().find_map(|call| match &call.call_type {
        MockCallType::Spawn { role, prompt }
            if *role == AgentRole::Custom("ralphx-utility-session-namer".to_string()) =>
        {
            Some(prompt.clone())
        }
        _ => None,
    });
    let default_calls = default_mock_impl.get_spawn_calls().await;
    assert!(
        session_namer_prompt.is_some(),
        "session namer should follow the project ideation harness; default roles: {:?}; codex roles: {:?}",
        default_calls
            .iter()
            .filter_map(|call| match &call.call_type {
                MockCallType::Spawn { role, .. } => Some(role.to_string()),
                _ => None,
            })
            .collect::<Vec<_>>(),
        codex_mock_impl
            .get_spawn_calls()
            .await
            .iter()
            .filter_map(|call| match &call.call_type {
                MockCallType::Spawn { role, .. } => Some(role.to_string()),
                _ => None,
            })
            .collect::<Vec<_>>()
    );
    assert!(
        default_mock_impl.get_spawn_calls().await.is_empty(),
        "external ideation helper spawns should not be recorded on the default helper client"
    );
}

/// Non-existent project_id → 404
#[tokio::test]
async fn test_start_ideation_invalid_project_returns_404() {
    let state = setup_test_state().await;

    let result = start_ideation_http(
        State(state),
        unrestricted_scope(),
        axum::http::HeaderMap::new(),
        Json(StartIdeationRequest {
            project_id: "nonexistent-project-xyz".to_string(),
            title: None,
            prompt: None,
            initial_prompt: None,
            idempotency_key: None,
        }),
    )
    .await;

    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().status,
        axum::http::StatusCode::NOT_FOUND
    );
}

/// backward_compat: `initial_prompt` field (no `prompt`) → treated same as `prompt`.
/// Both fields absent means agent_spawned = false; presence of either triggers spawn attempt.
/// In test env the spawn attempt fails gracefully, so agent_spawned stays false in both cases,
/// but the session is still created and the response is 200.
#[tokio::test]
async fn test_start_ideation_initial_prompt_backward_compat() {
    let state = setup_test_state().await;

    let project_id = "proj-initial-prompt";
    let p = make_project(project_id, "Initial Prompt Project");
    state.app_state.project_repo.create(p).await.unwrap();

    // Use `initial_prompt` (legacy field) but NOT `prompt`
    let result = start_ideation_http(
        State(state.clone()),
        unrestricted_scope(),
        axum::http::HeaderMap::new(),
        Json(StartIdeationRequest {
            project_id: project_id.to_string(),
            title: None,
            prompt: None,
            initial_prompt: Some("Legacy initial prompt text".to_string()),
            idempotency_key: None,
        }),
    )
    .await;

    assert!(
        result.is_ok(),
        "initial_prompt should be accepted as a valid prompt"
    );
    let response = result.unwrap().0;
    assert!(!response.session_id.is_empty());
    assert_eq!(response.status, "ideating");
    // Session must have been created
    let session_id = IdeationSessionId::from_string(response.session_id.clone());
    let fetched = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .unwrap();
    assert!(
        fetched.is_some(),
        "Session should be persisted when initial_prompt is used"
    );
}

/// `prompt` field takes precedence over `initial_prompt` when both are supplied.
/// The session is created and 200 is returned regardless of spawn outcome.
#[tokio::test]
async fn test_start_ideation_prompt_takes_precedence_over_initial_prompt() {
    let state = setup_test_state().await;

    let project_id = "proj-prompt-precedence";
    let p = make_project(project_id, "Prompt Precedence Project");
    state.app_state.project_repo.create(p).await.unwrap();

    // Both fields supplied — handler should prefer `prompt`
    let result = start_ideation_http(
        State(state.clone()),
        unrestricted_scope(),
        axum::http::HeaderMap::new(),
        Json(StartIdeationRequest {
            project_id: project_id.to_string(),
            title: None,
            prompt: Some("Primary prompt".to_string()),
            initial_prompt: Some("Legacy fallback".to_string()),
            idempotency_key: None,
        }),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert_eq!(response.status, "ideating");
    // Session was created regardless of which prompt field was used
    let session_id = IdeationSessionId::from_string(response.session_id.clone());
    assert!(
        state
            .app_state
            .ideation_session_repo
            .get_by_id(&session_id)
            .await
            .unwrap()
            .is_some(),
        "Session should exist in repo"
    );
}

/// With prompt present → session created; `agent_spawned` reflects whether the spawn
/// attempt succeeded. In CI/test environments where Claude CLI is unavailable,
/// the handler logs the error and returns agent_spawned=false but still 200.
/// This test verifies the handler never returns an error for a spawn failure.
#[tokio::test]
async fn test_start_ideation_with_prompt_returns_200_regardless_of_spawn_outcome() {
    let state = setup_test_state().await;

    let project_id = "proj-prompt-spawn";
    let p = make_project(project_id, "Prompt Spawn Project");
    state.app_state.project_repo.create(p).await.unwrap();

    let result = start_ideation_http(
        State(state.clone()),
        unrestricted_scope(),
        axum::http::HeaderMap::new(),
        Json(StartIdeationRequest {
            project_id: project_id.to_string(),
            title: None,
            prompt: Some("Please ideate on caching strategies".to_string()),
            initial_prompt: None,
            idempotency_key: None,
        }),
    )
    .await;

    // Must always return 200 — spawn failures are non-fatal
    assert!(
        result.is_ok(),
        "Spawn failure must not cause a non-200 response"
    );
    let response = result.unwrap().0;
    assert!(!response.session_id.is_empty());
    assert_eq!(response.status, "ideating");
    // Session persisted regardless of spawn outcome
    let session_id = IdeationSessionId::from_string(response.session_id.clone());
    assert!(
        state
            .app_state
            .ideation_session_repo
            .get_by_id(&session_id)
            .await
            .unwrap()
            .is_some(),
        "Session must be persisted even when agent spawn fails"
    );
}

/// Scope 403 — API key scoped to a different project cannot create an ideation session
/// for a project it has no access to. (Explicit named test for start_ideation_http scope check.)
#[tokio::test]
async fn test_start_ideation_scope_mismatch_returns_403() {
    let state = setup_test_state().await;

    let project_id = "proj-scope-mismatch";
    let p = make_project(project_id, "Scope Mismatch Project");
    state.app_state.project_repo.create(p).await.unwrap();

    // Key scoped to a different project
    let result = start_ideation_http(
        State(state),
        scoped(&["proj-different-from-target"]),
        axum::http::HeaderMap::new(),
        Json(StartIdeationRequest {
            project_id: project_id.to_string(),
            title: None,
            prompt: Some("Should be blocked".to_string()),
            initial_prompt: None,
            idempotency_key: None,
        }),
    )
    .await;

    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().status,
        axum::http::StatusCode::FORBIDDEN
    );
}

/// Session created even when multiple sessions exist — no hard cap anymore.
/// Gate was removed; second session creates successfully.
#[tokio::test]
async fn test_start_ideation_no_session_cap() {
    let state = setup_test_state().await;

    let project_id = "proj-no-cap";
    let p = make_project(project_id, "No Cap Project");
    state.app_state.project_repo.create(p).await.unwrap();

    // Create first session
    let existing = IdeationSession::new_with_title(
        ProjectId::from_string(project_id.to_string()),
        "First session",
    );
    state
        .app_state
        .ideation_session_repo
        .create(existing)
        .await
        .unwrap();

    // Second request must succeed — no session cap
    let result = start_ideation_http(
        State(state),
        unrestricted_scope(),
        axum::http::HeaderMap::new(),
        Json(StartIdeationRequest {
            project_id: project_id.to_string(),
            title: Some("Second session".to_string()),
            prompt: None,
            initial_prompt: None,
            idempotency_key: None,
        }),
    )
    .await;

    assert!(
        result.is_ok(),
        "Second session must succeed — cap was removed"
    );
    let response = result.unwrap().0;
    assert!(!response.session_id.is_empty());
    assert_eq!(response.status, "ideating");
}

/// Spawn failure → agent_spawn_blocked_reason populated, session still created (200).
#[tokio::test]
async fn test_start_ideation_spawn_failure_populates_blocked_reason() {
    let state = setup_test_state().await;

    let project_id = "proj-spawn-fail";
    let p = make_project(project_id, "Spawn Fail Project");
    state.app_state.project_repo.create(p).await.unwrap();

    // Provide a prompt so spawn is attempted; in test env Claude CLI is unavailable → SpawnFailed
    let result = start_ideation_http(
        State(state.clone()),
        unrestricted_scope(),
        axum::http::HeaderMap::new(),
        Json(StartIdeationRequest {
            project_id: project_id.to_string(),
            title: None,
            prompt: Some("Trigger spawn attempt".to_string()),
            initial_prompt: None,
            idempotency_key: None,
        }),
    )
    .await;

    // Must return 200 even on spawn failure
    assert!(
        result.is_ok(),
        "Spawn failure must not cause a non-200 response"
    );
    let response = result.unwrap().0;
    assert!(!response.session_id.is_empty());
    assert_eq!(response.status, "ideating");
    // Session was persisted
    let session_id = IdeationSessionId::from_string(response.session_id.clone());
    assert!(
        state
            .app_state
            .ideation_session_repo
            .get_by_id(&session_id)
            .await
            .unwrap()
            .is_some(),
        "Session must be persisted even when spawn fails"
    );
    // In test env where Claude is not available, either:
    // - agent_spawned=false with a blocked reason, OR
    // - agent_spawned=false with no reason (if SpawnFailed is not raised)
    // Either way, agent_spawn_blocked_reason must not be Some("") — it must be None or a real message
    if let Some(reason) = &response.agent_spawn_blocked_reason {
        assert!(
            !reason.is_empty(),
            "Blocked reason must not be empty string"
        );
    }
}

/// Idempotency key match → returns pre-existing session with exists: true instead of creating new.
#[tokio::test]
async fn test_start_ideation_idempotency_returns_existing_session() {
    let state = setup_test_state().await;

    let project_id = "proj-idempotency";
    let p = make_project(project_id, "Idempotency Project");
    state.app_state.project_repo.create(p).await.unwrap();

    // Pre-create a session with api_key_id + idempotency_key
    let mut existing = IdeationSession::new(ProjectId::from_string(project_id.to_string()));
    existing.origin = SessionOrigin::External;
    existing.api_key_id = Some("key-test-idempotency".to_string());
    existing.idempotency_key = Some("idem-key-abc123".to_string());
    let pre = state
        .app_state
        .ideation_session_repo
        .create(existing)
        .await
        .unwrap();
    let expected_session_id = pre.id.to_string();

    // Repeat request with same api_key_id + idempotency_key — must return existing session
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        "x-ralphx-key-id",
        axum::http::HeaderValue::from_static("key-test-idempotency"),
    );
    let result = start_ideation_http(
        State(state),
        unrestricted_scope(),
        headers,
        Json(StartIdeationRequest {
            project_id: project_id.to_string(),
            title: Some("Completely different title".to_string()),
            prompt: Some("Completely different prompt".to_string()),
            initial_prompt: None,
            idempotency_key: Some("idem-key-abc123".to_string()),
        }),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert_eq!(
        response.session_id, expected_session_id,
        "Must return pre-existing session, not create a new one"
    );
    assert_eq!(
        response.exists,
        Some(true),
        "exists must be true on idempotent retry"
    );
    assert_eq!(response.next_action, "poll_status");
    assert!(
        response
            .hint
            .as_deref()
            .unwrap_or("")
            .contains("Idempotent"),
        "hint must indicate idempotent retry"
    );
}

/// Jaccard dedup: active external session with similar text → duplicate_detected: true.
#[tokio::test]
async fn test_start_ideation_jaccard_dedup_returns_duplicate_detected() {
    let state = setup_test_state().await;

    let project_id = "proj-jaccard";
    let p = make_project(project_id, "Jaccard Project");
    state.app_state.project_repo.create(p).await.unwrap();

    // Pre-create an active external session — status defaults to Active
    let mut existing = IdeationSession::new_with_title(
        ProjectId::from_string(project_id.to_string()),
        "design authentication module",
    );
    existing.origin = SessionOrigin::External;
    let pre = state
        .app_state
        .ideation_session_repo
        .create(existing)
        .await
        .unwrap();
    let pre_session_id = pre.id.to_string();

    // Request with identical prompt — Jaccard score = 1.0 → dedup triggered
    let result = start_ideation_http(
        State(state),
        unrestricted_scope(),
        axum::http::HeaderMap::new(),
        Json(StartIdeationRequest {
            project_id: project_id.to_string(),
            title: None,
            prompt: Some("design authentication module".to_string()),
            initial_prompt: None,
            idempotency_key: None,
        }),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert_eq!(
        response.duplicate_detected,
        Some(true),
        "duplicate_detected must be true when Jaccard similarity >= threshold"
    );
    assert!(
        response.similarity_score.is_some(),
        "similarity_score must be present on dedup"
    );
    assert!(
        response.similarity_score.unwrap() >= 0.7,
        "similarity_score must be >= threshold (0.7)"
    );
    assert_eq!(
        response.next_action, "use_existing_session",
        "next_action must be use_existing_session on dedup"
    );
    assert_eq!(
        response.session_id, pre_session_id,
        "Must return the matched session, not create a new one"
    );
    assert!(
        !response.existing_active_sessions.is_empty(),
        "existing_active_sessions must be populated on dedup"
    );
}

/// New external sessions have external_activity_phase = "created" stored in the repository.
#[tokio::test]
async fn test_start_ideation_sets_external_activity_phase_on_new_session() {
    let state = setup_test_state().await;

    let project_id = "proj-ext-phase";
    let p = make_project(project_id, "Phase Project");
    state.app_state.project_repo.create(p).await.unwrap();

    let result = start_ideation_http(
        State(state.clone()),
        unrestricted_scope(),
        axum::http::HeaderMap::new(),
        Json(StartIdeationRequest {
            project_id: project_id.to_string(),
            title: None,
            prompt: None,
            initial_prompt: None,
            idempotency_key: None,
        }),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    let session_id = IdeationSessionId::from_string(response.session_id);
    let session = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .unwrap()
        .expect("Session must exist");
    assert_eq!(
        session.external_activity_phase.as_deref(),
        Some("created"),
        "external_activity_phase must be 'created' on new external sessions"
    );
}

/// api_key_id from X-RalphX-Key-Id header is stored on the new session.
#[tokio::test]
async fn test_start_ideation_stores_api_key_id_from_header() {
    let state = setup_test_state().await;

    let project_id = "proj-apikeyid";
    let p = make_project(project_id, "API Key ID Project");
    state.app_state.project_repo.create(p).await.unwrap();

    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        "x-ralphx-key-id",
        axum::http::HeaderValue::from_static("key-stored-xyz"),
    );

    let result = start_ideation_http(
        State(state.clone()),
        unrestricted_scope(),
        headers,
        Json(StartIdeationRequest {
            project_id: project_id.to_string(),
            title: None,
            prompt: None,
            initial_prompt: None,
            idempotency_key: None,
        }),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    let session_id = IdeationSessionId::from_string(response.session_id);
    let session = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .unwrap()
        .expect("Session must exist");
    assert_eq!(
        session.api_key_id.as_deref(),
        Some("key-stored-xyz"),
        "api_key_id must be stored from X-RalphX-Key-Id header"
    );
}

/// existing_active_sessions in response includes pre-existing active external sessions
/// plus the newly created session.
#[tokio::test]
async fn test_start_ideation_existing_active_sessions_populated() {
    let state = setup_test_state().await;

    let project_id = "proj-active-sessions";
    let p = make_project(project_id, "Active Sessions Project");
    state.app_state.project_repo.create(p).await.unwrap();

    // Pre-create an active external session
    let mut pre_existing = IdeationSession::new_with_title(
        ProjectId::from_string(project_id.to_string()),
        "Pre-existing external session",
    );
    pre_existing.origin = SessionOrigin::External;
    let pre = state
        .app_state
        .ideation_session_repo
        .create(pre_existing)
        .await
        .unwrap();
    let pre_id = pre.id.to_string();

    // Use a title sufficiently different to avoid Jaccard dedup
    let result = start_ideation_http(
        State(state),
        unrestricted_scope(),
        axum::http::HeaderMap::new(),
        Json(StartIdeationRequest {
            project_id: project_id.to_string(),
            title: Some("xyz-fresh-unrelated-session-title-2024".to_string()),
            prompt: None,
            initial_prompt: None,
            idempotency_key: None,
        }),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    // Response must contain at least 2 sessions: the new one + pre-existing
    assert!(
        response.existing_active_sessions.len() >= 2,
        "existing_active_sessions must include both new and pre-existing sessions, got: {:?}",
        response
            .existing_active_sessions
            .iter()
            .map(|s| &s.session_id)
            .collect::<Vec<_>>()
    );
    // The pre-existing external session must appear in the list
    let ids: Vec<&str> = response
        .existing_active_sessions
        .iter()
        .map(|s| s.session_id.as_str())
        .collect();
    assert!(
        ids.contains(&pre_id.as_str()),
        "Pre-existing active external session must appear in existing_active_sessions"
    );
}

// ============================================================================
// poll_events_http
// ============================================================================

#[tokio::test]
async fn test_poll_events_cursor_based() {
    let state = setup_test_state().await;

    let project_id = "proj-events";
    let p = make_project(project_id, "Events Project");
    state.app_state.project_repo.create(p).await.unwrap();

    // Create the external_events table and seed data using the in-memory db
    let proj_id_clone = project_id.to_string();
    state
        .app_state
        .db
        .run(move |conn| {
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS external_events (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    event_type TEXT NOT NULL,
                    project_id TEXT NOT NULL,
                    payload TEXT NOT NULL DEFAULT '{}',
                    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))
                );",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
            conn.execute(
                "INSERT INTO external_events (event_type, project_id, payload) VALUES ('task.created', ?1, '{\"id\":\"t1\"}')",
                rusqlite::params![proj_id_clone],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
            conn.execute(
                "INSERT INTO external_events (event_type, project_id, payload) VALUES ('task.created', ?1, '{\"id\":\"t2\"}')",
                rusqlite::params![proj_id_clone],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
            conn.execute(
                "INSERT INTO external_events (event_type, project_id, payload) VALUES ('task.merged', ?1, '{\"id\":\"t3\"}')",
                rusqlite::params![proj_id_clone],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
            Ok(())
        })
        .await
        .unwrap();

    // Poll without cursor — should return all 3 events
    let result = poll_events_http(
        State(state.clone()),
        unrestricted_scope(),
        Query(PollEventsQuery {
            project_id: project_id.to_string(),
            cursor: None,
            limit: None,
            event_type: None,
        }),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert_eq!(response.events.len(), 3);
    assert!(!response.has_more);
    assert!(response.next_cursor.is_none());

    // Poll with cursor after first event — should return only events 2 and 3
    let first_id = response.events[0].id;
    let result2 = poll_events_http(
        State(state),
        unrestricted_scope(),
        Query(PollEventsQuery {
            project_id: project_id.to_string(),
            cursor: Some(first_id),
            limit: None,
            event_type: None,
        }),
    )
    .await;

    assert!(result2.is_ok());
    let response2 = result2.unwrap().0;
    assert_eq!(response2.events.len(), 2);
    assert!(!response2.has_more);
}

#[tokio::test]
async fn test_poll_events_limit_and_has_more() {
    let state = setup_test_state().await;

    let project_id = "proj-events-limit";
    let p = make_project(project_id, "Events Limit Project");
    state.app_state.project_repo.create(p).await.unwrap();

    // Create table and insert 3 events
    let proj_id_clone = project_id.to_string();
    state
        .app_state
        .db
        .run(move |conn| {
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS external_events (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    event_type TEXT NOT NULL,
                    project_id TEXT NOT NULL,
                    payload TEXT NOT NULL DEFAULT '{}',
                    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))
                );",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
            for i in 0..3 {
                conn.execute(
                    "INSERT INTO external_events (event_type, project_id, payload) VALUES ('ev', ?1, '{}')",
                    rusqlite::params![proj_id_clone],
                )
                .map_err(|e| AppError::Database(e.to_string()))?;
                let _ = i;
            }
            Ok(())
        })
        .await
        .unwrap();

    // Request with limit=2 — should indicate has_more=true
    let result = poll_events_http(
        State(state),
        unrestricted_scope(),
        Query(PollEventsQuery {
            project_id: project_id.to_string(),
            cursor: None,
            limit: Some(2),
            event_type: None,
        }),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert_eq!(response.events.len(), 2);
    assert!(response.has_more);
    assert!(response.next_cursor.is_some());
}

#[tokio::test]
async fn test_poll_events_scope_violation() {
    let state = setup_test_state().await;

    let project_id = "proj-events-secret";
    let p = make_project(project_id, "Secret Events");
    state.app_state.project_repo.create(p).await.unwrap();

    let result = poll_events_http(
        State(state),
        scoped(&["proj-other"]),
        Query(PollEventsQuery {
            project_id: project_id.to_string(),
            cursor: None,
            limit: None,
            event_type: None,
        }),
    )
    .await;

    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), axum::http::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_poll_events_event_type_filter() {
    let state = setup_test_state().await;

    let project_id = "proj-events-filter";
    let p = make_project(project_id, "Events Filter Project");
    state.app_state.project_repo.create(p).await.unwrap();

    let proj_id_clone = project_id.to_string();
    state
        .app_state
        .db
        .run(move |conn| {
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS external_events (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    event_type TEXT NOT NULL,
                    project_id TEXT NOT NULL,
                    payload TEXT NOT NULL DEFAULT '{}',
                    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))
                );",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
            conn.execute(
                "INSERT INTO external_events (event_type, project_id, payload) VALUES ('task:created', ?1, '{}')",
                rusqlite::params![proj_id_clone],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
            conn.execute(
                "INSERT INTO external_events (event_type, project_id, payload) VALUES ('task:status_changed', ?1, '{}')",
                rusqlite::params![proj_id_clone],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
            conn.execute(
                "INSERT INTO external_events (event_type, project_id, payload) VALUES ('task:created', ?1, '{}')",
                rusqlite::params![proj_id_clone],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
            Ok(())
        })
        .await
        .unwrap();

    // Filter by event_type — should return only 2 task:created events
    let result = poll_events_http(
        State(state.clone()),
        unrestricted_scope(),
        Query(PollEventsQuery {
            project_id: project_id.to_string(),
            cursor: None,
            limit: None,
            event_type: Some("task:created".to_string()),
        }),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert_eq!(response.events.len(), 2);
    assert!(response
        .events
        .iter()
        .all(|e| e.event_type == "task:created"));

    // Filter by a type with no matches — should return empty
    let result2 = poll_events_http(
        State(state),
        unrestricted_scope(),
        Query(PollEventsQuery {
            project_id: project_id.to_string(),
            cursor: None,
            limit: None,
            event_type: Some("review:approved".to_string()),
        }),
    )
    .await;

    assert!(result2.is_ok());
    let response2 = result2.unwrap().0;
    assert_eq!(response2.events.len(), 0);
}

// ============================================================================
// external_task_transition_http — pause, cancel, retry
// ============================================================================

#[tokio::test]
async fn test_task_transition_pause() {
    let state = setup_test_state().await;

    let project_id = "proj-transition";
    let p = make_project(project_id, "Transition Test");
    state.app_state.project_repo.create(p).await.unwrap();

    let mut task = Task::new(
        ProjectId::from_string(project_id.to_string()),
        "Running task".to_string(),
    );
    task.internal_status = InternalStatus::Executing;
    state
        .app_state
        .task_repo
        .create(task.clone())
        .await
        .unwrap();

    let result = external_task_transition_http(
        State(state),
        unrestricted_scope(),
        Json(TaskTransitionRequest {
            task_id: task.id.to_string(),
            action: TransitionAction::Pause,
        }),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert!(response.success);
    assert_eq!(response.task_id, task.id.to_string());
}

#[tokio::test]
async fn test_task_transition_cancel() {
    let state = setup_test_state().await;

    let project_id = "proj-cancel";
    let p = make_project(project_id, "Cancel Test");
    state.app_state.project_repo.create(p).await.unwrap();

    let mut task = Task::new(
        ProjectId::from_string(project_id.to_string()),
        "Task to cancel".to_string(),
    );
    task.internal_status = InternalStatus::Ready;
    state
        .app_state
        .task_repo
        .create(task.clone())
        .await
        .unwrap();

    let result = external_task_transition_http(
        State(state),
        unrestricted_scope(),
        Json(TaskTransitionRequest {
            task_id: task.id.to_string(),
            action: TransitionAction::Cancel,
        }),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert!(response.success);
}

#[tokio::test]
async fn test_task_transition_retry_from_terminal() {
    let state = setup_test_state().await;
    state
        .app_state
        .ideation_settings_repo
        .compare_and_set_tasks_feature_state(
            TasksFeatureState::Disabled,
            TasksFeatureState::Enabled,
        )
        .await
        .unwrap();

    let project_id = "proj-retry";
    let p = make_project(project_id, "Retry Test");
    state.app_state.project_repo.create(p).await.unwrap();

    let mut task = Task::new(
        ProjectId::from_string(project_id.to_string()),
        "Stopped task".to_string(),
    );
    task.internal_status = InternalStatus::Stopped;
    state
        .app_state
        .task_repo
        .create(task.clone())
        .await
        .unwrap();

    let result = external_task_transition_http(
        State(state.clone()),
        unrestricted_scope(),
        Json(TaskTransitionRequest {
            task_id: task.id.to_string(),
            action: TransitionAction::Retry,
        }),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert!(response.success);
    assert_eq!(response.new_status, InternalStatus::Ready.as_str());

    let persisted = state
        .app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .expect("retried task should remain persisted");
    assert_eq!(persisted.internal_status, InternalStatus::Ready);
    let history = state
        .app_state
        .task_repo
        .get_status_history(&task.id)
        .await
        .unwrap();
    assert!(history.iter().any(|entry| {
        entry.from == InternalStatus::Stopped && entry.to == InternalStatus::Ready
    }));
}

#[tokio::test]
async fn test_task_transition_retry_non_terminal_fails() {
    let state = setup_test_state().await;

    let project_id = "proj-retry-fail";
    let p = make_project(project_id, "Retry Fail Test");
    state.app_state.project_repo.create(p).await.unwrap();

    let mut task = Task::new(
        ProjectId::from_string(project_id.to_string()),
        "Executing task".to_string(),
    );
    task.internal_status = InternalStatus::Executing;
    state
        .app_state
        .task_repo
        .create(task.clone())
        .await
        .unwrap();

    let result = external_task_transition_http(
        State(state),
        unrestricted_scope(),
        Json(TaskTransitionRequest {
            task_id: task.id.to_string(),
            action: TransitionAction::Retry,
        }),
    )
    .await;

    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().status,
        axum::http::StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn test_task_transition_retry_preserves_tasks_disabled_error() {
    let state = setup_test_state().await;
    state
        .app_state
        .ideation_settings_repo
        .compare_and_set_tasks_feature_state(
            TasksFeatureState::Disabled,
            TasksFeatureState::Enabled,
        )
        .await
        .unwrap();

    let project_id = "proj-retry-tasks-disabled";
    state
        .app_state
        .project_repo
        .create(make_project(project_id, "Retry Tasks Disabled"))
        .await
        .unwrap();
    let mut task = Task::new(
        ProjectId::from_string(project_id.to_string()),
        "Stopped task".to_string(),
    );
    task.internal_status = InternalStatus::Stopped;
    state
        .app_state
        .task_repo
        .create(task.clone())
        .await
        .unwrap();

    state
        .app_state
        .ideation_settings_repo
        .compare_and_set_tasks_feature_state(
            TasksFeatureState::Enabled,
            TasksFeatureState::Disabled,
        )
        .await
        .unwrap();

    let error = external_task_transition_http(
        State(state),
        unrestricted_scope(),
        Json(TaskTransitionRequest {
            task_id: task.id.to_string(),
            action: TransitionAction::Retry,
        }),
    )
    .await
    .expect_err("Tasks-off retry must be rejected");

    assert_eq!(error.status, axum::http::StatusCode::CONFLICT);
    assert!(
        error
            .message
            .as_deref()
            .is_some_and(|message| message.starts_with("ralphx:tasks_disabled")),
        "Tasks-off error must remain available to external callers"
    );
}

#[tokio::test]
async fn test_task_transition_scope_violation() {
    let state = setup_test_state().await;

    let project_id = "proj-transition-secret";
    let p = make_project(project_id, "Secret Transition");
    state.app_state.project_repo.create(p).await.unwrap();

    let task = Task::new(
        ProjectId::from_string(project_id.to_string()),
        "Protected task".to_string(),
    );
    state
        .app_state
        .task_repo
        .create(task.clone())
        .await
        .unwrap();

    let result = external_task_transition_http(
        State(state),
        scoped(&["proj-other"]),
        Json(TaskTransitionRequest {
            task_id: task.id.to_string(),
            action: TransitionAction::Pause,
        }),
    )
    .await;

    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().status,
        axum::http::StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn test_review_action_approve_review_allows_review_passed() {
    let state = setup_test_state().await;

    let project_id = "proj-review-action-approve";
    let p = make_project(project_id, "Review Action Approve");
    state.app_state.project_repo.create(p).await.unwrap();

    let mut task = Task::new(
        ProjectId::from_string(project_id.to_string()),
        "Review passed task".to_string(),
    );
    task.internal_status = InternalStatus::ReviewPassed;
    state
        .app_state
        .task_repo
        .create(task.clone())
        .await
        .unwrap();

    let result = review_action_http(
        State(state.clone()),
        unrestricted_scope(),
        Json(ReviewActionRequest {
            task_id: task.id.to_string(),
            action: ReviewActionType::ApproveReview,
            resolution: None,
            feedback: None,
        }),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert!(response.success);
    assert_eq!(response.task_id, task.id.to_string());
    assert_eq!(response.new_status, InternalStatus::Approved.to_string());

    let updated = state
        .app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    assert_ne!(updated.internal_status, InternalStatus::ReviewPassed);
}

#[tokio::test]
async fn test_review_action_preserves_tasks_disabled_error_without_transition() {
    let state = setup_test_state().await;
    state
        .app_state
        .ideation_settings_repo
        .compare_and_set_tasks_feature_state(
            TasksFeatureState::Disabled,
            TasksFeatureState::Enabled,
        )
        .await
        .unwrap();

    let project_id = "proj-review-action-tasks-disabled";
    state
        .app_state
        .project_repo
        .create(make_project(project_id, "Review Action Tasks Disabled"))
        .await
        .unwrap();
    let mut task = Task::new(
        ProjectId::from_string(project_id.to_string()),
        "Review passed task".to_string(),
    );
    task.internal_status = InternalStatus::ReviewPassed;
    state
        .app_state
        .task_repo
        .create(task.clone())
        .await
        .unwrap();
    let history_before = state
        .app_state
        .task_repo
        .get_status_history(&task.id)
        .await
        .unwrap();

    state
        .app_state
        .ideation_settings_repo
        .compare_and_set_tasks_feature_state(
            TasksFeatureState::Enabled,
            TasksFeatureState::Disabled,
        )
        .await
        .unwrap();

    let error = review_action_http(
        State(state.clone()),
        unrestricted_scope(),
        Json(ReviewActionRequest {
            task_id: task.id.to_string(),
            action: ReviewActionType::ApproveReview,
            resolution: None,
            feedback: None,
        }),
    )
    .await
    .expect_err("Tasks-off review action must be rejected");

    assert_eq!(error.status, axum::http::StatusCode::CONFLICT);
    assert!(
        error
            .message
            .as_deref()
            .is_some_and(|message| message.starts_with("ralphx:tasks_disabled")),
        "Tasks-off error must remain available to external callers"
    );
    assert_eq!(
        state
            .app_state
            .task_repo
            .get_by_id(&task.id)
            .await
            .unwrap()
            .expect("task must remain persisted")
            .internal_status,
        InternalStatus::ReviewPassed
    );
    assert_eq!(
        state
            .app_state
            .task_repo
            .get_status_history(&task.id)
            .await
            .unwrap()
            .len(),
        history_before.len()
    );
}

#[tokio::test]
async fn test_review_action_approve_review_rejects_reviewing() {
    let state = setup_test_state().await;

    let project_id = "proj-review-action-reviewing";
    let p = make_project(project_id, "Review Action Reviewing");
    state.app_state.project_repo.create(p).await.unwrap();

    let mut task = Task::new(
        ProjectId::from_string(project_id.to_string()),
        "Reviewing task".to_string(),
    );
    task.internal_status = InternalStatus::Reviewing;
    state
        .app_state
        .task_repo
        .create(task.clone())
        .await
        .unwrap();

    let result = review_action_http(
        State(state.clone()),
        unrestricted_scope(),
        Json(ReviewActionRequest {
            task_id: task.id.to_string(),
            action: ReviewActionType::ApproveReview,
            resolution: None,
            feedback: None,
        }),
    )
    .await;

    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().status,
        axum::http::StatusCode::UNPROCESSABLE_ENTITY
    );

    let updated = state
        .app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.internal_status, InternalStatus::Reviewing);
}

#[tokio::test]
async fn test_review_action_approve_review_rejects_merged() {
    let state = setup_test_state().await;

    let project_id = "proj-review-action-merged";
    let p = make_project(project_id, "Review Action Merged");
    state.app_state.project_repo.create(p).await.unwrap();

    let mut task = Task::new(
        ProjectId::from_string(project_id.to_string()),
        "Merged task".to_string(),
    );
    task.internal_status = InternalStatus::Merged;
    state
        .app_state
        .task_repo
        .create(task.clone())
        .await
        .unwrap();

    let result = review_action_http(
        State(state.clone()),
        unrestricted_scope(),
        Json(ReviewActionRequest {
            task_id: task.id.to_string(),
            action: ReviewActionType::ApproveReview,
            resolution: None,
            feedback: None,
        }),
    )
    .await;

    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().status,
        axum::http::StatusCode::UNPROCESSABLE_ENTITY
    );

    let updated = state
        .app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.internal_status, InternalStatus::Merged);
}

#[tokio::test]
async fn test_create_task_note_preserves_tasks_disabled_error_without_mutation() {
    let state = setup_test_state().await;
    state
        .app_state
        .ideation_settings_repo
        .compare_and_set_tasks_feature_state(
            TasksFeatureState::Disabled,
            TasksFeatureState::Enabled,
        )
        .await
        .unwrap();

    let project_id = "proj-task-note-tasks-disabled";
    state
        .app_state
        .project_repo
        .create(make_project(project_id, "Task Note Tasks Disabled"))
        .await
        .unwrap();
    let task = Task::new(
        ProjectId::from_string(project_id.to_string()),
        "Task with a note".to_string(),
    );
    state
        .app_state
        .task_repo
        .create(task.clone())
        .await
        .unwrap();

    state
        .app_state
        .ideation_settings_repo
        .compare_and_set_tasks_feature_state(
            TasksFeatureState::Enabled,
            TasksFeatureState::Disabled,
        )
        .await
        .unwrap();

    let error = create_task_note_http(
        State(state.clone()),
        unrestricted_scope(),
        Json(CreateTaskNoteRequest {
            task_id: task.id.to_string(),
            note: "must not be saved".to_string(),
        }),
    )
    .await
    .expect_err("Tasks-off note mutation must be rejected");

    assert_eq!(error.status, axum::http::StatusCode::CONFLICT);
    assert!(
        error
            .message
            .as_deref()
            .is_some_and(|message| message.starts_with("ralphx:tasks_disabled")),
        "Tasks-off error must remain available to external callers"
    );
    assert_eq!(
        state
            .app_state
            .task_repo
            .get_by_id(&task.id)
            .await
            .unwrap()
            .expect("task must remain persisted")
            .description,
        task.description
    );
}

// ============================================================================
// get_task_detail_http
// ============================================================================

#[tokio::test]
async fn test_get_task_detail_returns_task_and_steps() {
    let state = setup_test_state().await;

    let project_id = "proj-detail";
    let p = make_project(project_id, "Detail Project");
    state.app_state.project_repo.create(p).await.unwrap();

    let task = Task::new(
        ProjectId::from_string(project_id.to_string()),
        "Detail task".to_string(),
    );
    state
        .app_state
        .task_repo
        .create(task.clone())
        .await
        .unwrap();

    let result = get_task_detail_http(
        State(state),
        unrestricted_scope(),
        Path(task.id.to_string()),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert_eq!(response.id, task.id.to_string());
    assert_eq!(response.title, "Detail task");
    assert_eq!(response.project_id, project_id);
    assert!(response.steps.is_empty());
}

#[tokio::test]
async fn test_get_task_detail_scope_violation() {
    let state = setup_test_state().await;

    let project_id = "proj-detail-secret";
    let p = make_project(project_id, "Secret Detail");
    state.app_state.project_repo.create(p).await.unwrap();

    let task = Task::new(
        ProjectId::from_string(project_id.to_string()),
        "Secret task".to_string(),
    );
    state
        .app_state
        .task_repo
        .create(task.clone())
        .await
        .unwrap();

    let result = get_task_detail_http(
        State(state),
        scoped(&["proj-other"]),
        Path(task.id.to_string()),
    )
    .await;

    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), axum::http::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_get_task_detail_not_found() {
    let state = setup_test_state().await;

    let result = get_task_detail_http(
        State(state),
        unrestricted_scope(),
        Path("nonexistent-task".to_string()),
    )
    .await;

    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_get_task_diff_uses_captured_base_when_main_advances() {
    let (repo, captured_base_sha) = setup_task_diff_repo_with_captured_base_and_advanced_main();
    let state = setup_test_state().await;

    let mut project = Project::new(
        "External Diff Project".to_string(),
        repo.path().to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    state
        .app_state
        .project_repo
        .create(project.clone())
        .await
        .expect("create project");

    let mut task = Task::new(project.id.clone(), "External diff task".to_string());
    task.task_branch = Some("task/test".to_string());
    task.worktree_path = Some(repo.path().to_string_lossy().to_string());
    task.task_branch_base_ref = Some("main".to_string());
    task.task_branch_base_sha = Some(captured_base_sha);
    state
        .app_state
        .task_repo
        .create(task.clone())
        .await
        .expect("create task");

    let response = get_task_diff_http(
        State(state),
        unrestricted_scope(),
        Path(task.id.to_string()),
    )
    .await
    .expect("external task diff should resolve from captured base")
    .0;

    assert_eq!(response.changed_files, vec!["task.txt"]);
    assert_eq!(response.files_changed, 1);
}

// ============================================================================
// get_task_review_summary_http
// ============================================================================

#[tokio::test]
async fn test_get_review_summary_empty() {
    let state = setup_test_state().await;

    let project_id = "proj-review-sum";
    let p = make_project(project_id, "Review Summary Project");
    state.app_state.project_repo.create(p).await.unwrap();

    let task = Task::new(
        ProjectId::from_string(project_id.to_string()),
        "Review task".to_string(),
    );
    state
        .app_state
        .task_repo
        .create(task.clone())
        .await
        .unwrap();

    let result = get_task_review_summary_http(
        State(state),
        unrestricted_scope(),
        Path(task.id.to_string()),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert_eq!(response.task_id, task.id.to_string());
    assert!(response.review_notes.is_empty());
    assert_eq!(response.revision_count, 0);
}

#[tokio::test]
async fn test_get_review_summary_scope_violation() {
    let state = setup_test_state().await;

    let project_id = "proj-review-sum-secret";
    let p = make_project(project_id, "Secret Review");
    state.app_state.project_repo.create(p).await.unwrap();

    let task = Task::new(
        ProjectId::from_string(project_id.to_string()),
        "Secret review task".to_string(),
    );
    state
        .app_state
        .task_repo
        .create(task.clone())
        .await
        .unwrap();

    let result = get_task_review_summary_http(
        State(state),
        scoped(&["proj-other"]),
        Path(task.id.to_string()),
    )
    .await;

    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), axum::http::StatusCode::FORBIDDEN);
}

// ============================================================================
// get_merge_pipeline_http
// ============================================================================

#[tokio::test]
async fn test_get_merge_pipeline_filters_correctly() {
    let state = setup_test_state().await;

    let project_id = "proj-merge-pipe";
    let p = make_project(project_id, "Merge Pipeline Project");
    state.app_state.project_repo.create(p).await.unwrap();

    let mut task_pending_merge = Task::new(
        ProjectId::from_string(project_id.to_string()),
        "Pending merge task".to_string(),
    );
    task_pending_merge.internal_status = InternalStatus::PendingMerge;

    let mut task_merging = Task::new(
        ProjectId::from_string(project_id.to_string()),
        "Merging task".to_string(),
    );
    task_merging.internal_status = InternalStatus::Merging;

    let task_executing = Task::new(
        ProjectId::from_string(project_id.to_string()),
        "Not in merge".to_string(),
    );

    state
        .app_state
        .task_repo
        .create(task_pending_merge)
        .await
        .unwrap();
    state
        .app_state
        .task_repo
        .create(task_merging)
        .await
        .unwrap();
    state
        .app_state
        .task_repo
        .create(task_executing)
        .await
        .unwrap();

    let result = get_merge_pipeline_http(
        State(state),
        unrestricted_scope(),
        Path(project_id.to_string()),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert_eq!(response.project_id, project_id);
    assert_eq!(response.tasks.len(), 2);
}

#[tokio::test]
async fn test_get_merge_pipeline_scope_violation() {
    let state = setup_test_state().await;

    let project_id = "proj-merge-secret";
    let p = make_project(project_id, "Secret Merge");
    state.app_state.project_repo.create(p).await.unwrap();

    let result = get_merge_pipeline_http(
        State(state),
        scoped(&["proj-other"]),
        Path(project_id.to_string()),
    )
    .await;

    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), axum::http::StatusCode::FORBIDDEN);
}

// ============================================================================
// get_attention_items_http
// ============================================================================

#[tokio::test]
async fn test_get_attention_items_groups_by_category() {
    let state = setup_test_state().await;

    let project_id = "proj-attention";
    let p = make_project(project_id, "Attention Project");
    state.app_state.project_repo.create(p).await.unwrap();

    let mut escalated = Task::new(
        ProjectId::from_string(project_id.to_string()),
        "Escalated review".to_string(),
    );
    escalated.internal_status = InternalStatus::Escalated;

    let mut failed = Task::new(
        ProjectId::from_string(project_id.to_string()),
        "Failed task".to_string(),
    );
    failed.internal_status = InternalStatus::Failed;

    let mut conflict = Task::new(
        ProjectId::from_string(project_id.to_string()),
        "Merge conflict".to_string(),
    );
    conflict.internal_status = InternalStatus::MergeConflict;

    let healthy = Task::new(
        ProjectId::from_string(project_id.to_string()),
        "Healthy task".to_string(),
    );

    state.app_state.task_repo.create(escalated).await.unwrap();
    state.app_state.task_repo.create(failed).await.unwrap();
    state.app_state.task_repo.create(conflict).await.unwrap();
    state.app_state.task_repo.create(healthy).await.unwrap();

    let result = get_attention_items_http(
        State(state),
        unrestricted_scope(),
        Path(project_id.to_string()),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert_eq!(response.escalated_reviews.len(), 1);
    assert_eq!(response.failed_tasks.len(), 1);
    assert_eq!(response.merge_conflicts.len(), 1);
}

#[tokio::test]
async fn test_get_attention_items_scope_violation() {
    let state = setup_test_state().await;

    let project_id = "proj-attention-secret";
    let p = make_project(project_id, "Secret Attention");
    state.app_state.project_repo.create(p).await.unwrap();

    let result = get_attention_items_http(
        State(state),
        scoped(&["proj-other"]),
        Path(project_id.to_string()),
    )
    .await;

    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), axum::http::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_get_attention_items_empty_project() {
    let state = setup_test_state().await;

    let project_id = "proj-attention-empty";
    let p = make_project(project_id, "Empty Attention");
    state.app_state.project_repo.create(p).await.unwrap();

    let result = get_attention_items_http(
        State(state),
        unrestricted_scope(),
        Path(project_id.to_string()),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert!(response.escalated_reviews.is_empty());
    assert!(response.failed_tasks.is_empty());
    assert!(response.merge_conflicts.is_empty());
}

// ============================================================================
// get_execution_capacity_http
// ============================================================================

#[tokio::test]
async fn test_get_execution_capacity_running_and_queued() {
    let state = setup_test_state().await;

    let project_id = "proj-capacity";
    let p = make_project(project_id, "Capacity Project");
    state.app_state.project_repo.create(p).await.unwrap();

    let mut executing = Task::new(
        ProjectId::from_string(project_id.to_string()),
        "Executing task".to_string(),
    );
    executing.internal_status = InternalStatus::Executing;

    let mut reviewing = Task::new(
        ProjectId::from_string(project_id.to_string()),
        "Reviewing task".to_string(),
    );
    reviewing.internal_status = InternalStatus::Reviewing;

    let mut pending_review = Task::new(
        ProjectId::from_string(project_id.to_string()),
        "Pending review task".to_string(),
    );
    pending_review.internal_status = InternalStatus::PendingReview;

    let healthy = Task::new(
        ProjectId::from_string(project_id.to_string()),
        "Blocked task".to_string(),
    );

    state.app_state.task_repo.create(executing).await.unwrap();
    state.app_state.task_repo.create(reviewing).await.unwrap();
    state
        .app_state
        .task_repo
        .create(pending_review)
        .await
        .unwrap();
    state.app_state.task_repo.create(healthy).await.unwrap();

    let result = get_execution_capacity_http(
        State(state),
        unrestricted_scope(),
        Path(project_id.to_string()),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    // executing + reviewing = 2 running
    assert_eq!(response.project_running, 2);
    // pending_review = 1 queued
    assert_eq!(response.project_queued, 1);
}

#[tokio::test]
async fn test_get_execution_capacity_scope_violation() {
    let state = setup_test_state().await;

    let project_id = "proj-capacity-secret";
    let p = make_project(project_id, "Secret Capacity");
    state.app_state.project_repo.create(p).await.unwrap();

    let result = get_execution_capacity_http(
        State(state),
        scoped(&["proj-other"]),
        Path(project_id.to_string()),
    )
    .await;

    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), axum::http::StatusCode::FORBIDDEN);
}

// ============================================================================
// external_apply_proposals (POST /api/external/apply_proposals)
// ============================================================================

fn make_proposal(session_id: IdeationSessionId, title: &str) -> TaskProposal {
    TaskProposal::new(
        session_id,
        title,
        ProposalCategory::Feature,
        Priority::Medium,
    )
}

async fn acknowledge_dependencies(state: &HttpServerState, session_id: &str) {
    let _ = analyze_session_dependencies(State(state.clone()), Path(session_id.to_string()))
        .await
        .expect("Failed to analyze and acknowledge dependencies");
}

/// Creates a project + active ideation session. Returns (project_id_str, session_id_str).
async fn setup_session(
    state: &HttpServerState,
    project_id: &str,
    project_name: &str,
) -> (String, String) {
    let project = make_project(project_id, project_name);
    state.app_state.project_repo.create(project).await.unwrap();

    let pid = ProjectId::from_string(project_id.to_string());
    let session = IdeationSession::new(pid);
    let created = state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    (project_id.to_string(), created.id.as_str().to_string())
}

#[tokio::test]
async fn test_external_apply_proposals_session_not_found() {
    let state = setup_test_state().await;

    let req = ExternalApplyProposalsRequest {
        session_id: "nonexistent-session".to_string(),
        proposal_ids: vec![],
        target_column: "auto".to_string(),
        base_branch_override: None,
    };

    let result = external_apply_proposals(State(state), unrestricted_scope(), Json(req)).await;

    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().status,
        axum::http::StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn test_external_apply_proposals_project_scope_enforced() {
    // External agent scoped to "proj-other" cannot apply proposals to session in "proj-apply"
    let state = setup_test_state().await;
    let (_, session_id) = setup_session(&state, "proj-apply", "Apply Test").await;

    let req = ExternalApplyProposalsRequest {
        session_id,
        proposal_ids: vec![],
        target_column: "auto".to_string(),
        base_branch_override: None,
    };

    let result = external_apply_proposals(
        State(state),
        scoped(&["proj-other"]), // wrong project
        Json(req),
    )
    .await;

    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().status,
        axum::http::StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn test_external_apply_proposals_unrestricted_scope_allowed() {
    // Unrestricted scope (no X-RalphX-Project-Scope header) allows all projects
    let state = setup_sqlite_test_state().await;
    let (_, session_id) = setup_session(&state, "proj-unrestricted", "Unrestricted").await;
    let session_id_typed = IdeationSessionId::from_string(session_id.clone());
    let created = state
        .app_state
        .task_proposal_repo
        .create(make_proposal(session_id_typed, "Scoped Proposal"))
        .await
        .unwrap();

    let req = ExternalApplyProposalsRequest {
        session_id,
        proposal_ids: vec![created.id.as_str().to_string()],
        target_column: "auto".to_string(),
        base_branch_override: None,
    };

    let result = external_apply_proposals(State(state), unrestricted_scope(), Json(req)).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_external_apply_proposals_correct_scope_allowed() {
    // Scoped key can apply proposals when it has access to the session's project
    let state = setup_sqlite_test_state().await;
    let (project_id, session_id) = setup_session(&state, "proj-scoped-ok", "Scoped OK").await;
    let session_id_typed = IdeationSessionId::from_string(session_id.clone());
    let created = state
        .app_state
        .task_proposal_repo
        .create(make_proposal(session_id_typed, "Scoped Proposal"))
        .await
        .unwrap();

    let req = ExternalApplyProposalsRequest {
        session_id,
        proposal_ids: vec![created.id.as_str().to_string()],
        target_column: "auto".to_string(),
        base_branch_override: None,
    };

    let result = external_apply_proposals(
        State(state),
        scoped(&[&project_id]), // correct project scope
        Json(req),
    )
    .await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_external_apply_proposals_creates_tasks_from_proposals() {
    // Full apply: multi-proposal sessions must acknowledge dependency ordering first.
    // This mirrors the real flow after analyze_session_dependencies or explicit dependency edits.
    let state = setup_sqlite_test_state().await;
    let (_, session_id) = setup_session(&state, "proj-full-apply", "Full Apply").await;

    let session_id_typed = IdeationSessionId::from_string(session_id.clone());

    let p1 = make_proposal(session_id_typed.clone(), "Task Alpha");
    let p2 = make_proposal(session_id_typed.clone(), "Task Beta");
    let created_p1 = state.app_state.task_proposal_repo.create(p1).await.unwrap();
    let created_p2 = state.app_state.task_proposal_repo.create(p2).await.unwrap();

    acknowledge_dependencies(&state, &session_id).await;

    let req = ExternalApplyProposalsRequest {
        session_id,
        proposal_ids: vec![
            created_p1.id.as_str().to_string(),
            created_p2.id.as_str().to_string(),
        ],
        target_column: "auto".to_string(),
        base_branch_override: None,
    };

    let result = external_apply_proposals(State(state), unrestricted_scope(), Json(req)).await;

    assert!(
        result.is_ok(),
        "apply should succeed: {:?}",
        result.err().map(|e| e.status)
    );
    let response = result.unwrap().0;
    assert_eq!(response.created_task_ids.len(), 2);
    assert!(response.session_converted, "all proposals applied");
    assert!(response.execution_plan_id.is_some());
    assert!(response.warnings.is_empty());
}

#[tokio::test]
async fn test_external_apply_proposals_blocks_unacknowledged_multi_proposal_session() {
    let state = setup_test_state().await;
    let (_, session_id) = setup_session(&state, "proj-full-apply-gate", "Full Apply Gate").await;

    let session_id_typed = IdeationSessionId::from_string(session_id.clone());

    let p1 = make_proposal(session_id_typed.clone(), "Task Alpha");
    let p2 = make_proposal(session_id_typed, "Task Beta");
    let created_p1 = state.app_state.task_proposal_repo.create(p1).await.unwrap();
    let created_p2 = state.app_state.task_proposal_repo.create(p2).await.unwrap();

    let req = ExternalApplyProposalsRequest {
        session_id,
        proposal_ids: vec![
            created_p1.id.as_str().to_string(),
            created_p2.id.as_str().to_string(),
        ],
        target_column: "auto".to_string(),
        base_branch_override: None,
    };

    let err = external_apply_proposals(State(state), unrestricted_scope(), Json(req))
        .await
        .expect_err("multi-proposal apply must require dependency acknowledgment");

    assert_eq!(err.status, axum::http::StatusCode::UNPROCESSABLE_ENTITY);
    assert!(err
        .message
        .as_deref()
        .unwrap_or_default()
        .contains("dependency ordering has not been reviewed"));
}

// ============================================================================
// list_ideation_sessions_http
// ============================================================================

/// No filter — all sessions for the project returned
#[tokio::test]
async fn test_list_sessions_no_scope_returns_all() {
    let state = setup_test_state().await;

    let project_id = "proj-list-all";
    let p = make_project(project_id, "List All Project");
    state.app_state.project_repo.create(p).await.unwrap();

    let pid = ProjectId::from_string(project_id.to_string());
    let s1 = IdeationSession::new_with_title(pid.clone(), "Session One");
    let s2 = IdeationSession::new_with_title(pid.clone(), "Session Two");
    state
        .app_state
        .ideation_session_repo
        .create(s1)
        .await
        .unwrap();
    state
        .app_state
        .ideation_session_repo
        .create(s2)
        .await
        .unwrap();

    let result = list_ideation_sessions_http(
        State(state),
        unrestricted_scope(),
        Path(project_id.to_string()),
        Query(ListSessionsParams {
            status: None,
            limit: None,
            updated_after: None,
        }),
    )
    .await;

    assert!(result.is_ok(), "expected Ok, got: {:?}", result.err());
    let response = result.unwrap().0;
    assert_eq!(response.sessions.len(), 2);
    let titles: Vec<_> = response
        .sessions
        .iter()
        .filter_map(|s| s.title.as_deref())
        .collect();
    assert!(titles.contains(&"Session One"));
    assert!(titles.contains(&"Session Two"));
}

/// Filter by status=active — only active sessions returned
#[tokio::test]
async fn test_list_sessions_filter_active() {
    let state = setup_test_state().await;

    let project_id = "proj-list-active";
    let p = make_project(project_id, "List Active Project");
    state.app_state.project_repo.create(p).await.unwrap();

    let pid = ProjectId::from_string(project_id.to_string());

    // Create one active session
    let active = IdeationSession::new_with_title(pid.clone(), "Active Session");
    let created_active = state
        .app_state
        .ideation_session_repo
        .create(active)
        .await
        .unwrap();

    // Create one archived session
    let archived = IdeationSession::new_with_title(pid.clone(), "Archived Session");
    let created_archived = state
        .app_state
        .ideation_session_repo
        .create(archived)
        .await
        .unwrap();
    state
        .app_state
        .ideation_session_repo
        .update_status(&created_archived.id, IdeationSessionStatus::Archived)
        .await
        .unwrap();

    let result = list_ideation_sessions_http(
        State(state),
        unrestricted_scope(),
        Path(project_id.to_string()),
        Query(ListSessionsParams {
            status: Some("active".to_string()),
            limit: None,
            updated_after: None,
        }),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert_eq!(response.sessions.len(), 1);
    assert_eq!(response.sessions[0].id, created_active.id.as_str());
    assert_eq!(response.sessions[0].status, "active");
}

/// Filter by status=accepted — only accepted sessions returned
#[tokio::test]
async fn test_list_sessions_filter_accepted() {
    let state = setup_test_state().await;

    let project_id = "proj-list-accepted";
    let p = make_project(project_id, "List Accepted Project");
    state.app_state.project_repo.create(p).await.unwrap();

    let pid = ProjectId::from_string(project_id.to_string());

    // Create active session
    let active = IdeationSession::new_with_title(pid.clone(), "Still Active");
    state
        .app_state
        .ideation_session_repo
        .create(active)
        .await
        .unwrap();

    // Create accepted session
    let accepted_sess = IdeationSession::new_with_title(pid.clone(), "Accepted Session");
    let created_accepted = state
        .app_state
        .ideation_session_repo
        .create(accepted_sess)
        .await
        .unwrap();
    state
        .app_state
        .ideation_session_repo
        .update_status(&created_accepted.id, IdeationSessionStatus::Accepted)
        .await
        .unwrap();

    let result = list_ideation_sessions_http(
        State(state),
        unrestricted_scope(),
        Path(project_id.to_string()),
        Query(ListSessionsParams {
            status: Some("accepted".to_string()),
            limit: None,
            updated_after: None,
        }),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert_eq!(response.sessions.len(), 1);
    assert_eq!(response.sessions[0].status, "accepted");
}

/// Filter by status=archived — only archived sessions returned
#[tokio::test]
async fn test_list_sessions_filter_archived() {
    let state = setup_test_state().await;

    let project_id = "proj-list-archived";
    let p = make_project(project_id, "List Archived Project");
    state.app_state.project_repo.create(p).await.unwrap();

    let pid = ProjectId::from_string(project_id.to_string());

    // Create active session
    let active = IdeationSession::new_with_title(pid.clone(), "Active Remains");
    state
        .app_state
        .ideation_session_repo
        .create(active)
        .await
        .unwrap();

    // Create archived session
    let archived = IdeationSession::new_with_title(pid.clone(), "Archived Session");
    let created_archived = state
        .app_state
        .ideation_session_repo
        .create(archived)
        .await
        .unwrap();
    state
        .app_state
        .ideation_session_repo
        .update_status(&created_archived.id, IdeationSessionStatus::Archived)
        .await
        .unwrap();

    let result = list_ideation_sessions_http(
        State(state),
        unrestricted_scope(),
        Path(project_id.to_string()),
        Query(ListSessionsParams {
            status: Some("archived".to_string()),
            limit: None,
            updated_after: None,
        }),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert_eq!(response.sessions.len(), 1);
    assert_eq!(response.sessions[0].status, "archived");
}

/// Filter by status=all — same as no filter, returns all sessions
#[tokio::test]
async fn test_list_sessions_filter_all() {
    let state = setup_test_state().await;

    let project_id = "proj-list-filter-all";
    let p = make_project(project_id, "List Filter All Project");
    state.app_state.project_repo.create(p).await.unwrap();

    let pid = ProjectId::from_string(project_id.to_string());

    let s1 = IdeationSession::new_with_title(pid.clone(), "Session A");
    let s2 = IdeationSession::new_with_title(pid.clone(), "Session B");
    let created_s2 = state
        .app_state
        .ideation_session_repo
        .create(s2)
        .await
        .unwrap();
    state
        .app_state
        .ideation_session_repo
        .create(s1)
        .await
        .unwrap();
    state
        .app_state
        .ideation_session_repo
        .update_status(&created_s2.id, IdeationSessionStatus::Archived)
        .await
        .unwrap();

    let result = list_ideation_sessions_http(
        State(state),
        unrestricted_scope(),
        Path(project_id.to_string()),
        Query(ListSessionsParams {
            status: Some("all".to_string()),
            limit: None,
            updated_after: None,
        }),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert_eq!(
        response.sessions.len(),
        2,
        "status=all must return all sessions regardless of status"
    );
}

/// Invalid status filter → 400
#[tokio::test]
async fn test_list_sessions_invalid_status_returns_400() {
    let state = setup_test_state().await;

    let project_id = "proj-list-bad-status";
    let p = make_project(project_id, "Bad Status Project");
    state.app_state.project_repo.create(p).await.unwrap();

    let result = list_ideation_sessions_http(
        State(state),
        unrestricted_scope(),
        Path(project_id.to_string()),
        Query(ListSessionsParams {
            status: Some("invalid_xyz".to_string()),
            limit: None,
            updated_after: None,
        }),
    )
    .await;

    assert!(result.is_err());
    let (status, _body) = result.unwrap_err();
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
}

/// Scope violation — API key scoped to different project returns 403
#[tokio::test]
async fn test_list_sessions_scope_violation_returns_403() {
    let state = setup_test_state().await;

    let project_id = "proj-list-scope-a";
    let p = make_project(project_id, "Scope A Project");
    state.app_state.project_repo.create(p).await.unwrap();

    let result = list_ideation_sessions_http(
        State(state),
        scoped(&["proj-list-scope-b"]),
        Path(project_id.to_string()),
        Query(ListSessionsParams {
            status: None,
            limit: None,
            updated_after: None,
        }),
    )
    .await;

    assert!(result.is_err());
    let (status, _body) = result.unwrap_err();
    assert_eq!(status, axum::http::StatusCode::FORBIDDEN);
}

/// Empty project — no sessions → empty array returned
#[tokio::test]
async fn test_list_sessions_empty_project() {
    let state = setup_test_state().await;

    let project_id = "proj-list-empty";
    let p = make_project(project_id, "Empty Sessions Project");
    state.app_state.project_repo.create(p).await.unwrap();

    let result = list_ideation_sessions_http(
        State(state),
        unrestricted_scope(),
        Path(project_id.to_string()),
        Query(ListSessionsParams {
            status: None,
            limit: None,
            updated_after: None,
        }),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert!(
        response.sessions.is_empty(),
        "expected empty sessions array for project with no sessions"
    );
}

/// Session with proposals — proposal_count reflects the actual count
#[tokio::test]
async fn test_list_sessions_includes_proposal_count() {
    let state = setup_test_state().await;

    let project_id = "proj-list-proposals";
    let p = make_project(project_id, "Proposals Project");
    state.app_state.project_repo.create(p).await.unwrap();

    let pid = ProjectId::from_string(project_id.to_string());
    let session = IdeationSession::new_with_title(pid.clone(), "Session With Proposals");
    let created = state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    // Add 2 proposals to the session
    let prop1 = make_proposal(created.id.clone(), "Feature Alpha");
    let prop2 = make_proposal(created.id.clone(), "Feature Beta");
    state
        .app_state
        .task_proposal_repo
        .create(prop1)
        .await
        .unwrap();
    state
        .app_state
        .task_proposal_repo
        .create(prop2)
        .await
        .unwrap();

    let result = list_ideation_sessions_http(
        State(state),
        unrestricted_scope(),
        Path(project_id.to_string()),
        Query(ListSessionsParams {
            status: None,
            limit: None,
            updated_after: None,
        }),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert_eq!(response.sessions.len(), 1);
    assert_eq!(
        response.sessions[0].proposal_count, 2,
        "proposal_count must reflect actual number of proposals"
    );
}

/// updated_after filter — only returns sessions updated after the given timestamp
#[tokio::test]
async fn test_list_sessions_updated_after_filter() {
    let state = setup_test_state().await;

    let project_id = "proj-list-updated-after";
    let p = make_project(project_id, "Updated After Project");
    state.app_state.project_repo.create(p).await.unwrap();

    let pid = ProjectId::from_string(project_id.to_string());

    // Create two sessions with different updated_at timestamps
    let mut old_session = IdeationSession::new_with_title(pid.clone(), "Old Session");
    old_session.updated_at = chrono::DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let mut new_session = IdeationSession::new_with_title(pid.clone(), "New Session");
    new_session.updated_at = chrono::DateTime::parse_from_rfc3339("2025-06-01T00:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    state
        .app_state
        .ideation_session_repo
        .create(old_session)
        .await
        .unwrap();
    state
        .app_state
        .ideation_session_repo
        .create(new_session)
        .await
        .unwrap();

    let result = list_ideation_sessions_http(
        State(state),
        unrestricted_scope(),
        Path(project_id.to_string()),
        Query(ListSessionsParams {
            status: None,
            limit: None,
            updated_after: Some("2025-01-01T00:00:00Z".to_string()),
        }),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert_eq!(
        response.sessions.len(),
        1,
        "only the new session should be returned"
    );
    assert_eq!(response.sessions[0].title.as_deref(), Some("New Session"));
}

/// updated_after filter — invalid timestamp returns 400
#[tokio::test]
async fn test_list_sessions_updated_after_invalid_timestamp() {
    let state = setup_test_state().await;

    let project_id = "proj-list-updated-after-invalid";
    let p = make_project(project_id, "Invalid Timestamp Project");
    state.app_state.project_repo.create(p).await.unwrap();

    let result = list_ideation_sessions_http(
        State(state),
        unrestricted_scope(),
        Path(project_id.to_string()),
        Query(ListSessionsParams {
            status: None,
            limit: None,
            updated_after: Some("not-a-timestamp".to_string()),
        }),
    )
    .await;

    assert!(result.is_err());
    let (status, _) = result.unwrap_err();
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
}

/// updated_at field is present in response
#[tokio::test]
async fn test_list_sessions_response_includes_updated_at() {
    let state = setup_test_state().await;

    let project_id = "proj-list-updated-at-field";
    let p = make_project(project_id, "Updated At Field Project");
    state.app_state.project_repo.create(p).await.unwrap();

    let pid = ProjectId::from_string(project_id.to_string());
    let session = IdeationSession::new_with_title(pid.clone(), "Session With Updated At");
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    let result = list_ideation_sessions_http(
        State(state),
        unrestricted_scope(),
        Path(project_id.to_string()),
        Query(ListSessionsParams {
            status: None,
            limit: None,
            updated_after: None,
        }),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert_eq!(response.sessions.len(), 1);
    assert!(
        !response.sessions[0].updated_at.is_empty(),
        "updated_at must be present in session summary"
    );
}

// ============================================================================
// trigger_verification_http
// ============================================================================

/// SQLite-backed setup for trigger_verification tests.
/// trigger_verification_http calls db.run(|conn| SessionRepo::trigger_auto_verify_sync(conn, ...))
/// which requires the session to exist in the SQLite DB, not just in memory.
async fn setup_sqlite_test_state() -> HttpServerState {
    let app_state = Arc::new(AppState::new_sqlite_test());
    let execution_state = Arc::new(ExecutionState::new());
    HttpServerState {
        app_state,
        execution_state,
        delegation_service: Default::default(),
    }
}

async fn seed_running_verification_action(
    state: &HttpServerState,
    session_id: &IdeationSessionId,
    artifact_id: &str,
) -> AgentRun {
    let conversation = state
        .app_state
        .chat_conversation_repo
        .create(ChatConversation::new_ideation(session_id.clone()))
        .await
        .expect("verification conversation should seed");
    let mut run = AgentRun::new(conversation.id);
    run.action_kind = Some(AgentRunActionKind::VerifyPlan);
    run.action_context_id = Some(session_id.as_str().to_string());
    run.action_target_id = Some(artifact_id.to_string());
    state
        .app_state
        .agent_run_repo
        .create(run)
        .await
        .expect("running verification action should seed")
}

#[tokio::test]
async fn test_trigger_verification_no_plan() {
    // Session with no plan_artifact_id → status "no_plan"
    let state = setup_test_state().await;

    let pid = ProjectId::from_string("proj-verify-no-plan".to_string());
    let project = make_project("proj-verify-no-plan", "No Plan Project");
    state.app_state.project_repo.create(project).await.unwrap();

    // Create session with no plan
    let session = IdeationSession::new(pid);
    let created = state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    let result = trigger_verification_http(
        State(state),
        unrestricted_scope(),
        Json(TriggerVerificationRequest {
            session_id: created.id.as_str().to_string(),
        }),
    )
    .await;

    assert!(result.is_ok(), "expected Ok, got: {:?}", result.err());
    let response = result.unwrap().0;
    assert_eq!(response.status, "no_plan");
}

#[tokio::test]
async fn test_trigger_verification_already_running() {
    // A running typed Verify Plan action is the current source of truth.
    let state = setup_sqlite_test_state().await;

    let pid = ProjectId::from_string("proj-verify-running".to_string());
    let project = make_project("proj-verify-running", "Already Running Project");
    state.app_state.project_repo.create(project).await.unwrap();

    let overview = state
        .app_state
        .artifact_repo
        .create(Artifact::new_inline(
            "Overview",
            ArtifactType::Specification,
            "# Overview",
            "test",
        ))
        .await
        .unwrap();
    let blueprint = state
        .app_state
        .artifact_repo
        .create(Artifact::new_inline(
            "Blueprint",
            ArtifactType::Specification,
            "# Blueprint",
            "test",
        ))
        .await
        .unwrap();

    // Create session with the complete v2 plan bundle.
    let mut session = IdeationSession::builder()
        .project_id(pid.clone())
        .status(IdeationSessionStatus::Active)
        .plan_artifact_id(overview.id.clone())
        .build();
    session.plan_blueprint_artifact_id = Some(blueprint.id.clone());
    let created = state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    seed_running_verification_action(
        &state,
        &created.id,
        &format!("plan_bundle:v2:{}:{}", overview.id, blueprint.id),
    )
    .await;

    let result = trigger_verification_http(
        State(state),
        unrestricted_scope(),
        Json(TriggerVerificationRequest {
            session_id: created.id.as_str().to_string(),
        }),
    )
    .await;

    assert!(result.is_ok(), "expected Ok, got: {:?}", result.err());
    let response = result.unwrap().0;
    assert_eq!(response.status, "already_running");
}

#[tokio::test]
async fn test_trigger_verification_uses_typed_action_truth_when_legacy_summary_is_stale() {
    let state = setup_sqlite_test_state().await;

    let pid = ProjectId::from_string("proj-verify-stale-snapshot".to_string());
    let project = make_project("proj-verify-stale-snapshot", "Stale Snapshot Project");
    state.app_state.project_repo.create(project).await.unwrap();

    let overview = state
        .app_state
        .artifact_repo
        .create(Artifact::new_inline(
            "Plan Overview",
            ArtifactType::Specification,
            "# Overview",
            "test",
        ))
        .await
        .unwrap();
    let blueprint = state
        .app_state
        .artifact_repo
        .create(Artifact::new_inline(
            "Plan Blueprint",
            ArtifactType::Specification,
            "# Blueprint",
            "test",
        ))
        .await
        .unwrap();
    let mut session = IdeationSession::builder()
        .project_id(pid)
        .status(IdeationSessionStatus::Active)
        .verification_generation(2)
        .plan_artifact_id(overview.id.clone())
        .build();
    session.plan_blueprint_artifact_id = Some(blueprint.id.clone());
    let session_id = session.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    state
        .app_state
        .ideation_session_repo
        .save_verification_run_snapshot(
            &session_id,
            &VerificationRunSnapshot {
                generation: 2,
                status: VerificationStatus::Reviewing,
                in_progress: true,
                current_round: 1,
                max_rounds: 5,
                best_round_index: None,
                convergence_reason: None,
                current_gaps: vec![],
                rounds: vec![],
            },
        )
        .await
        .unwrap();
    state
        .app_state
        .ideation_session_repo
        .update_verification_state(&session_id, VerificationStatus::Unverified, false)
        .await
        .unwrap();
    let running = seed_running_verification_action(
        &state,
        &session_id,
        &format!("plan_bundle:v2:{}:{}", overview.id, blueprint.id),
    )
    .await;

    let result = trigger_verification_http(
        State(state.clone()),
        unrestricted_scope(),
        Json(TriggerVerificationRequest {
            session_id: session_id.as_str().to_string(),
        }),
    )
    .await;

    assert!(result.is_ok(), "expected Ok, got: {:?}", result.err());
    let response = result.unwrap().0;
    assert_eq!(
        response.status, "already_running",
        "the current typed action must block duplicate external verification starts"
    );

    let refreshed = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .unwrap()
        .expect("session should still exist");
    assert_eq!(
        refreshed.verification_generation, 2,
        "legacy snapshot/summary fields must not trigger a new verification generation"
    );
    let persisted_run = state
        .app_state
        .agent_run_repo
        .get_by_id(&running.id)
        .await
        .unwrap()
        .expect("running action must remain authoritative");
    assert_eq!(persisted_run.status, running.status);
}

#[tokio::test]
async fn test_trigger_verification_scope_403() {
    // Session from different project → 403
    let state = setup_test_state().await;
    let (_, session_id) = setup_session(&state, "proj-verify-a", "Project A").await;

    let result = trigger_verification_http(
        State(state),
        scoped(&["proj-verify-b"]),
        Json(TriggerVerificationRequest { session_id }),
    )
    .await;

    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), axum::http::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_trigger_verification_session_not_found() {
    // Non-existent session → 404
    let state = setup_test_state().await;

    let result = trigger_verification_http(
        State(state),
        unrestricted_scope(),
        Json(TriggerVerificationRequest {
            session_id: "nonexistent-session-id".to_string(),
        }),
    )
    .await;

    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), axum::http::StatusCode::NOT_FOUND);
}

// ============================================================================
// get_plan_verification_external_http
// ============================================================================

#[tokio::test]
async fn test_get_plan_verification_basic() {
    // Session with verification state → reads it correctly
    let state = setup_test_state().await;
    let (_, session_id) = setup_session(&state, "proj-verify-get", "Verify Get Project").await;
    let session_id_obj = IdeationSessionId::from_string(session_id.clone());

    // Set verification state
    state
        .app_state
        .ideation_session_repo
        .update_verification_state(&session_id_obj, VerificationStatus::Verified, false)
        .await
        .unwrap();

    let result =
        get_plan_verification_external_http(State(state), unrestricted_scope(), Path(session_id))
            .await;

    assert!(result.is_ok(), "expected Ok, got: {:?}", result.err());
    let response = result.unwrap().0;
    assert_eq!(
        response.status,
        ralphx_lib::application::plan_verification_service::PlanVerificationStatusKind::Unverified
    );
    assert!(!response.in_progress);
    assert!(response.plan_artifact_id.is_none());
    assert!(response.verified_plan_artifact_id.is_none());
}

#[tokio::test]
async fn test_get_plan_verification_scope_403() {
    // Session from different project → 403
    let state = setup_test_state().await;
    let (_, session_id) = setup_session(&state, "proj-verify-scope", "Scope Project").await;

    let result = get_plan_verification_external_http(
        State(state),
        scoped(&["proj-other-scope"]),
        Path(session_id),
    )
    .await;

    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), axum::http::StatusCode::FORBIDDEN);
}

// ============================================================================
// get_ideation_messages_http
// ============================================================================

async fn create_message(state: &HttpServerState, msg: ChatMessage) {
    state.app_state.chat_message_repo.create(msg).await.unwrap();
}

#[tokio::test]
async fn test_get_ideation_messages_empty() {
    let state = setup_test_state().await;
    let (_, session_id_str) =
        setup_session(&state, "proj-msg-empty", "Empty Messages Session").await;

    let result = get_ideation_messages_http(
        State(state),
        unrestricted_scope(),
        Path(session_id_str),
        axum::extract::Query(GetIdeationMessagesQuery {
            limit: 50,
            offset: 0,
        }),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert!(response.messages.is_empty());
    assert!(!response.has_more);
    assert_eq!(response.agent_status, "idle");
}

#[tokio::test]
async fn test_get_ideation_messages_returns_user_and_orchestrator() {
    let state = setup_test_state().await;
    let (_, session_id_str) = setup_session(&state, "proj-msg-roles", "Role Filter Session").await;
    let session_id = IdeationSessionId::from_string(session_id_str.clone());

    // Add user, orchestrator, and system messages
    create_message(
        &state,
        ChatMessage::user_in_session(session_id.clone(), "user message"),
    )
    .await;
    create_message(
        &state,
        ChatMessage::orchestrator_in_session(session_id.clone(), "orchestrator reply"),
    )
    .await;
    create_message(
        &state,
        ChatMessage::system_in_session(session_id.clone(), "system internal"),
    )
    .await;

    let result = get_ideation_messages_http(
        State(state),
        unrestricted_scope(),
        Path(session_id_str),
        axum::extract::Query(GetIdeationMessagesQuery {
            limit: 50,
            offset: 0,
        }),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    // System message must be excluded
    assert_eq!(response.messages.len(), 2);

    let roles: Vec<&str> = response.messages.iter().map(|m| m.role.as_str()).collect();
    assert!(roles.contains(&"user"));
    assert!(roles.contains(&"assistant")); // Orchestrator maps to "assistant"
    assert!(!roles.contains(&"system"));
}

#[tokio::test]
async fn test_get_ideation_messages_pagination() {
    let state = setup_test_state().await;
    let (_, session_id_str) = setup_session(&state, "proj-msg-page", "Pagination Session").await;
    let session_id = IdeationSessionId::from_string(session_id_str.clone());

    // Insert 5 user messages
    for i in 0..5 {
        create_message(
            &state,
            ChatMessage::user_in_session(session_id.clone(), format!("message {i}")),
        )
        .await;
    }

    // Fetch first 3
    let result = get_ideation_messages_http(
        State(state.clone()),
        unrestricted_scope(),
        Path(session_id_str.clone()),
        axum::extract::Query(GetIdeationMessagesQuery {
            limit: 3,
            offset: 0,
        }),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert_eq!(response.messages.len(), 3);
    assert!(response.has_more);

    // Fetch remaining 2
    let result2 = get_ideation_messages_http(
        State(state),
        unrestricted_scope(),
        Path(session_id_str),
        axum::extract::Query(GetIdeationMessagesQuery {
            limit: 3,
            offset: 3,
        }),
    )
    .await;

    assert!(result2.is_ok());
    let response2 = result2.unwrap().0;
    assert_eq!(response2.messages.len(), 2);
    assert!(!response2.has_more);
}

#[tokio::test]
async fn test_get_ideation_messages_scope_violation() {
    let state = setup_test_state().await;
    let (_, session_id_str) = setup_session(&state, "proj-msg-scope", "Scope Test Session").await;

    let result = get_ideation_messages_http(
        State(state),
        scoped(&["proj-other"]),
        Path(session_id_str),
        axum::extract::Query(GetIdeationMessagesQuery {
            limit: 50,
            offset: 0,
        }),
    )
    .await;

    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), axum::http::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_get_ideation_messages_not_found() {
    let state = setup_test_state().await;

    let result = get_ideation_messages_http(
        State(state),
        unrestricted_scope(),
        Path("nonexistent-session".to_string()),
        axum::extract::Query(GetIdeationMessagesQuery {
            limit: 50,
            offset: 0,
        }),
    )
    .await;

    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_get_ideation_messages_agent_status_generating() {
    let state = setup_test_state().await;
    let (_, session_id_str) = setup_session(&state, "proj-msg-agent", "Agent Status Session").await;

    // Register a running agent with "ideation" context_type (regression test: was "session")
    let agent_key = RunningAgentKey::new("ideation", &session_id_str);
    state
        .app_state
        .running_agent_registry
        .register(
            agent_key,
            0,
            "conv-id".to_string(),
            "run-id".to_string(),
            None,
            None,
        )
        .await;

    let result = get_ideation_messages_http(
        State(state),
        unrestricted_scope(),
        Path(session_id_str),
        axum::extract::Query(GetIdeationMessagesQuery {
            limit: 50,
            offset: 0,
        }),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert_eq!(response.agent_status, "generating");
}

#[tokio::test]
async fn test_get_ideation_messages_agent_status_idle() {
    // No agent registered → agent_status must be "idle"
    let state = setup_test_state().await;
    let (_, session_id_str) =
        setup_session(&state, "proj-msg-agent-idle", "Idle Status Session").await;

    let result = get_ideation_messages_http(
        State(state),
        unrestricted_scope(),
        Path(session_id_str),
        axum::extract::Query(GetIdeationMessagesQuery {
            limit: 50,
            offset: 0,
        }),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert_eq!(response.agent_status, "idle");
}

#[tokio::test]
async fn test_get_ideation_messages_agent_status_waiting_for_input() {
    // Register agent in RunningAgentRegistry AND stdin in InteractiveProcessRegistry
    // with context_type = "ideation" → agent_status: "waiting_for_input"
    let state = setup_test_state().await;
    let (_, session_id_str) =
        setup_session(&state, "proj-msg-agent-wait", "Waiting For Input Session").await;

    // Register running agent
    let agent_key = RunningAgentKey::new("ideation", &session_id_str);
    state
        .app_state
        .running_agent_registry
        .register(
            agent_key,
            0,
            "conv-id-wait".to_string(),
            "run-id-wait".to_string(),
            None,
            None,
        )
        .await;

    // Register interactive process (stdin open = waiting_for_input)
    let mut child = tokio::process::Command::new("cat")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .expect("failed to spawn cat for test");
    let stdin = child.stdin.take().expect("no stdin on cat process");

    let ipr_key = InteractiveProcessKey::new("ideation", &session_id_str);
    state
        .app_state
        .interactive_process_registry
        .register(ipr_key, stdin)
        .await;

    let result = get_ideation_messages_http(
        State(state),
        unrestricted_scope(),
        Path(session_id_str),
        axum::extract::Query(GetIdeationMessagesQuery {
            limit: 50,
            offset: 0,
        }),
    )
    .await;

    drop(child);

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert_eq!(response.agent_status, "waiting_for_input");
}

// ============================================================================
// get_ideation_status_http — agent_running field
// ============================================================================

#[tokio::test]
async fn test_get_ideation_status_agent_running() {
    // Register agent with context_type = "ideation" → agent_running: true
    let state = setup_test_state().await;
    let (_, session_id_str) =
        setup_session(&state, "proj-status-agent", "Agent Running Status Session").await;

    let agent_key = RunningAgentKey::new("ideation", &session_id_str);
    state
        .app_state
        .running_agent_registry
        .register(
            agent_key,
            0,
            "conv-id-status".to_string(),
            "run-id-status".to_string(),
            None,
            None,
        )
        .await;

    let result =
        get_ideation_status_http(State(state), unrestricted_scope(), Path(session_id_str)).await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert!(response.agent_running, "expected agent_running: true");
    assert_eq!(
        response.agent_status, "generating",
        "expected agent_status: generating when agent is running"
    );
}

#[tokio::test]
async fn test_get_ideation_status_agent_not_running() {
    // No agent registered → agent_running: false
    let state = setup_test_state().await;
    let (_, session_id_str) =
        setup_session(&state, "proj-status-no-agent", "No Agent Status Session").await;

    let result =
        get_ideation_status_http(State(state), unrestricted_scope(), Path(session_id_str)).await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert!(!response.agent_running, "expected agent_running: false");
    assert_eq!(
        response.agent_status, "idle",
        "expected agent_status: idle when agent not running"
    );
}

#[tokio::test]
async fn test_get_ideation_status_agent_waiting_for_input() {
    // Register agent AND InteractiveProcess → agent_status: "waiting_for_input"
    let state = setup_test_state().await;
    let (_, session_id_str) =
        setup_session(&state, "proj-status-waiting", "Waiting Status Session").await;

    // Register running agent
    let agent_key = RunningAgentKey::new("ideation", &session_id_str);
    state
        .app_state
        .running_agent_registry
        .register(
            agent_key,
            0,
            "conv-id-waiting".to_string(),
            "run-id-waiting".to_string(),
            None,
            None,
        )
        .await;

    // Spawn a `cat` process to act as the interactive process (indicates waiting_for_input)
    let mut child = tokio::process::Command::new("cat")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .expect("failed to spawn cat for test");
    let stdin = child.stdin.take().expect("no stdin on cat process");

    let ipr_key = InteractiveProcessKey::new("ideation", &session_id_str);
    state
        .app_state
        .interactive_process_registry
        .register(ipr_key, stdin)
        .await;

    let result =
        get_ideation_status_http(State(state), unrestricted_scope(), Path(session_id_str)).await;

    drop(child);

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert!(response.agent_running, "expected agent_running: true");
    assert_eq!(
        response.agent_status, "waiting_for_input",
        "expected agent_status: waiting_for_input when interactive process registered"
    );
}

#[tokio::test]
async fn test_get_ideation_status_includes_verification_state() {
    // Session with default verification state → verification_status: "unverified", verification_in_progress: false
    let state = setup_test_state().await;
    let (_, session_id_str) =
        setup_session(&state, "proj-status-verif", "Verification State Session").await;

    let result =
        get_ideation_status_http(State(state), unrestricted_scope(), Path(session_id_str)).await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert!(
        !response.verification_status.is_empty(),
        "expected non-empty verification_status"
    );
    assert!(
        !response.verification_in_progress,
        "expected verification_in_progress: false for new session"
    );
}

// ============================================================================
// batch_task_status_http
// ============================================================================

async fn seed_task(state: &HttpServerState, project_id: &str, title: &str) -> Task {
    let task = Task::new(
        ProjectId::from_string(project_id.to_string()),
        title.to_string(),
    );
    state
        .app_state
        .task_repo
        .create(task.clone())
        .await
        .unwrap();
    task
}

#[tokio::test]
async fn test_batch_task_status_multiple_tasks() {
    let state = setup_test_state().await;
    let t1 = seed_task(&state, "proj-batch-1", "Task Alpha").await;
    let t2 = seed_task(&state, "proj-batch-1", "Task Beta").await;

    let req = BatchTaskStatusRequest {
        task_ids: vec![t1.id.to_string(), t2.id.to_string()],
    };

    let result = batch_task_status_http(State(state), unrestricted_scope(), Json(req)).await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert_eq!(response.requested_count, 2);
    assert_eq!(response.returned_count, 2);
    assert_eq!(response.tasks.len(), 2);
    assert!(response.errors.is_empty());

    let titles: Vec<&str> = response.tasks.iter().map(|t| t.title.as_str()).collect();
    assert!(titles.contains(&"Task Alpha"));
    assert!(titles.contains(&"Task Beta"));
}

#[tokio::test]
async fn test_batch_task_status_not_found() {
    let state = setup_test_state().await;
    let t1 = seed_task(&state, "proj-batch-nf", "Real Task").await;

    let req = BatchTaskStatusRequest {
        task_ids: vec![t1.id.to_string(), "nonexistent-task-id".to_string()],
    };

    let result = batch_task_status_http(State(state), unrestricted_scope(), Json(req)).await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert_eq!(response.requested_count, 2);
    assert_eq!(response.returned_count, 1);
    assert_eq!(response.tasks.len(), 1);
    assert_eq!(response.errors.len(), 1);
    assert_eq!(response.errors[0].id, "nonexistent-task-id");
    assert_eq!(response.errors[0].reason, "not_found");
}

#[tokio::test]
async fn test_batch_task_status_access_denied() {
    let state = setup_test_state().await;
    let t1 = seed_task(&state, "proj-batch-scoped", "Scoped Task").await;
    let t2 = seed_task(&state, "proj-batch-other", "Other Task").await;

    // Scope to only proj-batch-scoped
    let req = BatchTaskStatusRequest {
        task_ids: vec![t1.id.to_string(), t2.id.to_string()],
    };

    let result =
        batch_task_status_http(State(state), scoped(&["proj-batch-scoped"]), Json(req)).await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert_eq!(response.requested_count, 2);
    assert_eq!(response.returned_count, 1);
    assert_eq!(response.tasks.len(), 1);
    assert_eq!(response.tasks[0].title, "Scoped Task");
    assert_eq!(response.errors.len(), 1);
    assert_eq!(response.errors[0].id, t2.id.to_string());
    assert_eq!(response.errors[0].reason, "access_denied");
}

#[tokio::test]
async fn test_batch_task_status_max_50_enforced() {
    let state = setup_test_state().await;

    // Submit 51 task IDs
    let task_ids: Vec<String> = (0..51).map(|i| format!("task-id-{i}")).collect();
    let req = BatchTaskStatusRequest { task_ids };

    let result = batch_task_status_http(State(state), unrestricted_scope(), Json(req)).await;

    assert!(result.is_err());
    let (status, msg) = result.unwrap_err();
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
    assert!(msg.contains("Maximum is 50"));
}

#[tokio::test]
async fn test_batch_task_status_requested_count_and_returned_count() {
    let state = setup_test_state().await;
    let t1 = seed_task(&state, "proj-batch-counts", "Task One").await;

    let req = BatchTaskStatusRequest {
        task_ids: vec![
            t1.id.to_string(),
            "ghost-1".to_string(),
            "ghost-2".to_string(),
        ],
    };

    let result = batch_task_status_http(State(state), unrestricted_scope(), Json(req)).await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert_eq!(response.requested_count, 3);
    assert_eq!(response.returned_count, 1);
    assert_eq!(response.errors.len(), 2);
    for err in &response.errors {
        assert_eq!(err.reason, "not_found");
    }
}

#[tokio::test]
async fn test_batch_task_status_empty_request() {
    let state = setup_test_state().await;

    let req = BatchTaskStatusRequest { task_ids: vec![] };

    let result = batch_task_status_http(State(state), unrestricted_scope(), Json(req)).await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert_eq!(response.requested_count, 0);
    assert_eq!(response.returned_count, 0);
    assert!(response.tasks.is_empty());
    assert!(response.errors.is_empty());
}

// ============================================================================
// get_session_tasks_http
// ============================================================================

/// Session with no linked tasks returns not_scheduled + empty task list.
#[tokio::test]
async fn test_get_session_tasks_empty_session_returns_not_scheduled() {
    let state = setup_test_state().await;

    let project_id = "proj-session-tasks-empty";
    let p = make_project(project_id, "Session Tasks Empty");
    state.app_state.project_repo.create(p).await.unwrap();

    let session = IdeationSession::new_with_title(
        ProjectId::from_string(project_id.to_string()),
        "Empty Session",
    );
    let session_id = session.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    let result = get_session_tasks_http(
        State(state),
        unrestricted_scope(),
        Path(session_id.as_str().to_string()),
        Query(GetSessionTasksParams {
            changed_since: None,
        }),
    )
    .await;

    assert!(result.is_ok(), "Expected Ok, got: {:?}", result.err());
    let response = result.unwrap().0;
    assert_eq!(response.session_id, session_id.as_str());
    assert!(response.tasks.is_empty(), "Expected no tasks");
    assert_eq!(response.delivery_status, "not_scheduled");
    assert_eq!(response.task_count, 0);
}

/// Session with tasks returns correct task list, delivery_status, and task_count.
#[tokio::test]
async fn test_get_session_tasks_with_tasks_returns_task_list() {
    let state = setup_test_state().await;

    let project_id = "proj-session-tasks-list";
    let p = make_project(project_id, "Session Tasks List");
    state.app_state.project_repo.create(p).await.unwrap();

    let session = IdeationSession::new_with_title(
        ProjectId::from_string(project_id.to_string()),
        "Session With Tasks",
    );
    let session_id = session.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    // Create two tasks linked to the session
    let mut task1 = Task::new(
        ProjectId::from_string(project_id.to_string()),
        "Task One".to_string(),
    );
    task1.ideation_session_id = Some(session_id.clone());
    task1.internal_status = InternalStatus::Backlog;

    let mut task2 = Task::new(
        ProjectId::from_string(project_id.to_string()),
        "Task Two".to_string(),
    );
    task2.ideation_session_id = Some(session_id.clone());
    task2.internal_status = InternalStatus::Executing;

    state.app_state.task_repo.create(task1).await.unwrap();
    state.app_state.task_repo.create(task2).await.unwrap();

    let result = get_session_tasks_http(
        State(state),
        unrestricted_scope(),
        Path(session_id.as_str().to_string()),
        Query(GetSessionTasksParams {
            changed_since: None,
        }),
    )
    .await;

    assert!(result.is_ok(), "Expected Ok, got: {:?}", result.err());
    let response = result.unwrap().0;
    assert_eq!(response.session_id, session_id.as_str());
    assert_eq!(response.task_count, 2);
    assert_eq!(response.tasks.len(), 2);
    assert_eq!(response.delivery_status, "in_progress");

    let titles: Vec<&str> = response.tasks.iter().map(|t| t.title.as_str()).collect();
    assert!(titles.contains(&"Task One"), "Missing 'Task One'");
    assert!(titles.contains(&"Task Two"), "Missing 'Task Two'");
}

/// Unlinked tasks (different project, no session_id) are excluded from results.
#[tokio::test]
async fn test_get_session_tasks_excludes_unlinked_tasks() {
    let state = setup_test_state().await;

    let project_id = "proj-session-tasks-excl";
    let p = make_project(project_id, "Session Tasks Exclude");
    state.app_state.project_repo.create(p).await.unwrap();

    let session = IdeationSession::new_with_title(
        ProjectId::from_string(project_id.to_string()),
        "Session A",
    );
    let session_id = session.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    // Task linked to this session
    let mut linked_task = Task::new(
        ProjectId::from_string(project_id.to_string()),
        "Linked Task".to_string(),
    );
    linked_task.ideation_session_id = Some(session_id.clone());

    // Task with no session_id — should be excluded
    let unlinked_task = Task::new(
        ProjectId::from_string(project_id.to_string()),
        "Unlinked Task".to_string(),
    );

    state.app_state.task_repo.create(linked_task).await.unwrap();
    state
        .app_state
        .task_repo
        .create(unlinked_task)
        .await
        .unwrap();

    let result = get_session_tasks_http(
        State(state),
        unrestricted_scope(),
        Path(session_id.as_str().to_string()),
        Query(GetSessionTasksParams {
            changed_since: None,
        }),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert_eq!(
        response.task_count, 1,
        "Only linked task should be returned"
    );
    assert_eq!(response.tasks[0].title, "Linked Task");
}

/// Session not found returns 404.
#[tokio::test]
async fn test_get_session_tasks_nonexistent_session_returns_404() {
    let state = setup_test_state().await;

    let result = get_session_tasks_http(
        State(state),
        unrestricted_scope(),
        Path("nonexistent-session-id".to_string()),
        Query(GetSessionTasksParams {
            changed_since: None,
        }),
    )
    .await;

    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().status,
        axum::http::StatusCode::NOT_FOUND
    );
}

/// ProjectScope violation returns 403 when API key is scoped to different project.
#[tokio::test]
async fn test_get_session_tasks_scope_violation_returns_403() {
    let state = setup_test_state().await;

    let project_id = "proj-scope-session-tasks";
    let p = make_project(project_id, "Scope Session Tasks");
    state.app_state.project_repo.create(p).await.unwrap();

    let session = IdeationSession::new_with_title(
        ProjectId::from_string(project_id.to_string()),
        "Scoped Session",
    );
    let session_id = session.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    // Scope to a different project — must be rejected
    let result = get_session_tasks_http(
        State(state),
        scoped(&["proj-different"]),
        Path(session_id.as_str().to_string()),
        Query(GetSessionTasksParams {
            changed_since: None,
        }),
    )
    .await;

    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().status,
        axum::http::StatusCode::FORBIDDEN
    );
}

// ============================================================================
// delivery_status derivation logic
// ============================================================================

/// All tasks merged → "delivered".
#[tokio::test]
async fn test_get_session_tasks_delivery_status_all_merged_is_delivered() {
    let state = setup_test_state().await;

    let project_id = "proj-ds-delivered";
    let p = make_project(project_id, "DS Delivered");
    state.app_state.project_repo.create(p).await.unwrap();

    let session = IdeationSession::new_with_title(
        ProjectId::from_string(project_id.to_string()),
        "All Merged Session",
    );
    let session_id = session.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    for title in &["Task A", "Task B"] {
        let mut task = Task::new(
            ProjectId::from_string(project_id.to_string()),
            title.to_string(),
        );
        task.ideation_session_id = Some(session_id.clone());
        task.internal_status = InternalStatus::Merged;
        state.app_state.task_repo.create(task).await.unwrap();
    }

    let result = get_session_tasks_http(
        State(state),
        unrestricted_scope(),
        Path(session_id.as_str().to_string()),
        Query(GetSessionTasksParams {
            changed_since: None,
        }),
    )
    .await;

    assert!(result.is_ok());
    assert_eq!(result.unwrap().0.delivery_status, "delivered");
}

/// Mixed merged + failed tasks → "partial".
#[tokio::test]
async fn test_get_session_tasks_delivery_status_mixed_terminal_is_partial() {
    let state = setup_test_state().await;

    let project_id = "proj-ds-partial";
    let p = make_project(project_id, "DS Partial");
    state.app_state.project_repo.create(p).await.unwrap();

    let session = IdeationSession::new_with_title(
        ProjectId::from_string(project_id.to_string()),
        "Mixed Terminal Session",
    );
    let session_id = session.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    let mut merged = Task::new(
        ProjectId::from_string(project_id.to_string()),
        "Merged Task".to_string(),
    );
    merged.ideation_session_id = Some(session_id.clone());
    merged.internal_status = InternalStatus::Merged;

    let mut failed = Task::new(
        ProjectId::from_string(project_id.to_string()),
        "Failed Task".to_string(),
    );
    failed.ideation_session_id = Some(session_id.clone());
    failed.internal_status = InternalStatus::Failed;

    state.app_state.task_repo.create(merged).await.unwrap();
    state.app_state.task_repo.create(failed).await.unwrap();

    let result = get_session_tasks_http(
        State(state),
        unrestricted_scope(),
        Path(session_id.as_str().to_string()),
        Query(GetSessionTasksParams {
            changed_since: None,
        }),
    )
    .await;

    assert!(result.is_ok());
    assert_eq!(result.unwrap().0.delivery_status, "partial");
}

/// Tasks in review states (no active tasks) → "pending_review".
#[tokio::test]
async fn test_get_session_tasks_delivery_status_in_review_is_pending_review() {
    let state = setup_test_state().await;

    let project_id = "proj-ds-review";
    let p = make_project(project_id, "DS Review");
    state.app_state.project_repo.create(p).await.unwrap();

    let session = IdeationSession::new_with_title(
        ProjectId::from_string(project_id.to_string()),
        "In Review Session",
    );
    let session_id = session.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    let mut reviewing = Task::new(
        ProjectId::from_string(project_id.to_string()),
        "Reviewing Task".to_string(),
    );
    reviewing.ideation_session_id = Some(session_id.clone());
    reviewing.internal_status = InternalStatus::Reviewing;

    state.app_state.task_repo.create(reviewing).await.unwrap();

    let result = get_session_tasks_http(
        State(state),
        unrestricted_scope(),
        Path(session_id.as_str().to_string()),
        Query(GetSessionTasksParams {
            changed_since: None,
        }),
    )
    .await;

    assert!(result.is_ok());
    assert_eq!(result.unwrap().0.delivery_status, "pending_review");
}

/// Any active task (executing, backlog, ready) → "in_progress".
#[tokio::test]
async fn test_get_session_tasks_delivery_status_active_tasks_is_in_progress() {
    let state = setup_test_state().await;

    let project_id = "proj-ds-active";
    let p = make_project(project_id, "DS Active");
    state.app_state.project_repo.create(p).await.unwrap();

    let session = IdeationSession::new_with_title(
        ProjectId::from_string(project_id.to_string()),
        "Active Session",
    );
    let session_id = session.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    let mut executing = Task::new(
        ProjectId::from_string(project_id.to_string()),
        "Executing Task".to_string(),
    );
    executing.ideation_session_id = Some(session_id.clone());
    executing.internal_status = InternalStatus::Executing;

    // Also a merged task — but presence of active makes it in_progress
    let mut merged = Task::new(
        ProjectId::from_string(project_id.to_string()),
        "Merged Task".to_string(),
    );
    merged.ideation_session_id = Some(session_id.clone());
    merged.internal_status = InternalStatus::Merged;

    state.app_state.task_repo.create(executing).await.unwrap();
    state.app_state.task_repo.create(merged).await.unwrap();

    let result = get_session_tasks_http(
        State(state),
        unrestricted_scope(),
        Path(session_id.as_str().to_string()),
        Query(GetSessionTasksParams {
            changed_since: None,
        }),
    )
    .await;

    assert!(result.is_ok());
    assert_eq!(result.unwrap().0.delivery_status, "in_progress");
}

/// Session tasks response includes updated_at on each task.
#[tokio::test]
async fn test_get_session_tasks_includes_updated_at() {
    let state = setup_test_state().await;

    let project_id = "proj-session-tasks-updated-at";
    let p = make_project(project_id, "Session Tasks UpdatedAt");
    state.app_state.project_repo.create(p).await.unwrap();

    let session = IdeationSession::new_with_title(
        ProjectId::from_string(project_id.to_string()),
        "Session UpdatedAt",
    );
    let session_id = session.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    let mut task = Task::new(
        ProjectId::from_string(project_id.to_string()),
        "Task With Timestamp".to_string(),
    );
    task.ideation_session_id = Some(session_id.clone());
    state.app_state.task_repo.create(task).await.unwrap();

    let result = get_session_tasks_http(
        State(state),
        unrestricted_scope(),
        Path(session_id.as_str().to_string()),
        Query(GetSessionTasksParams {
            changed_since: None,
        }),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert_eq!(response.task_count, 1);
    let task_item = &response.tasks[0];
    // updated_at must be a non-empty RFC3339 string
    assert!(
        !task_item.updated_at.is_empty(),
        "updated_at should be present"
    );
    assert!(
        chrono::DateTime::parse_from_rfc3339(&task_item.updated_at).is_ok(),
        "updated_at should be valid RFC3339"
    );
}

/// changed_since filters out tasks updated before the cutoff.
#[tokio::test]
async fn test_get_session_tasks_changed_since_filters_older_tasks() {
    let state = setup_test_state().await;

    let project_id = "proj-session-tasks-changed-since";
    let p = make_project(project_id, "Session Tasks ChangedSince");
    state.app_state.project_repo.create(p).await.unwrap();

    let session = IdeationSession::new_with_title(
        ProjectId::from_string(project_id.to_string()),
        "Session ChangedSince",
    );
    let session_id = session.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    let mut task = Task::new(
        ProjectId::from_string(project_id.to_string()),
        "Old Task".to_string(),
    );
    task.ideation_session_id = Some(session_id.clone());
    state.app_state.task_repo.create(task).await.unwrap();

    // Use a far-future cutoff — all tasks should be filtered out
    let future_cutoff = "2099-01-01T00:00:00Z";
    let result = get_session_tasks_http(
        State(state),
        unrestricted_scope(),
        Path(session_id.as_str().to_string()),
        Query(GetSessionTasksParams {
            changed_since: Some(future_cutoff.to_string()),
        }),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert_eq!(
        response.task_count, 0,
        "All tasks should be filtered out by future cutoff"
    );
    assert!(response.tasks.is_empty());
}

/// changed_since with a past cutoff returns all tasks.
#[tokio::test]
async fn test_get_session_tasks_changed_since_past_cutoff_returns_all() {
    let state = setup_test_state().await;

    let project_id = "proj-session-tasks-past-cutoff";
    let p = make_project(project_id, "Session Tasks PastCutoff");
    state.app_state.project_repo.create(p).await.unwrap();

    let session = IdeationSession::new_with_title(
        ProjectId::from_string(project_id.to_string()),
        "Session PastCutoff",
    );
    let session_id = session.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    let mut task = Task::new(
        ProjectId::from_string(project_id.to_string()),
        "Recent Task".to_string(),
    );
    task.ideation_session_id = Some(session_id.clone());
    state.app_state.task_repo.create(task).await.unwrap();

    // Use a past cutoff — task should be included
    let past_cutoff = "2000-01-01T00:00:00Z";
    let result = get_session_tasks_http(
        State(state),
        unrestricted_scope(),
        Path(session_id.as_str().to_string()),
        Query(GetSessionTasksParams {
            changed_since: Some(past_cutoff.to_string()),
        }),
    )
    .await;

    assert!(result.is_ok());
    let response = result.unwrap().0;
    assert_eq!(
        response.task_count, 1,
        "Task should be included with past cutoff"
    );
}

/// changed_since with invalid value returns 400.
#[tokio::test]
async fn test_get_session_tasks_invalid_changed_since_returns_400() {
    let state = setup_test_state().await;

    let project_id = "proj-session-tasks-bad-cs";
    let p = make_project(project_id, "Session Tasks BadChangedSince");
    state.app_state.project_repo.create(p).await.unwrap();

    let session = IdeationSession::new_with_title(
        ProjectId::from_string(project_id.to_string()),
        "Session BadChangedSince",
    );
    let session_id = session.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    let result = get_session_tasks_http(
        State(state),
        unrestricted_scope(),
        Path(session_id.as_str().to_string()),
        Query(GetSessionTasksParams {
            changed_since: Some("not-a-date".to_string()),
        }),
    )
    .await;

    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().status,
        axum::http::StatusCode::BAD_REQUEST
    );
}

// ── verification_child continuity (external endpoint) ────────────────────────

/// External endpoint returns verification_child block when a child session exists.
/// active_child_session_id is populated when in_progress=true and child not archived.
#[tokio::test]
#[cfg(any())]
async fn test_get_plan_verification_external_verification_child_shape() {
    let state = setup_test_state().await;
    let (project_id_str, parent_id_str) =
        setup_session(&state, "proj-vc-ext", "Verification Child External").await;
    let project_id = ProjectId::from_string(project_id_str);
    let parent_id = IdeationSessionId::from_string(parent_id_str.clone());

    // Create a verification child session
    let child = IdeationSessionBuilder::new()
        .project_id(project_id)
        .parent_session_id(parent_id.clone())
        .session_purpose(SessionPurpose::Verification)
        .build();
    let child_id_str = child.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(child.clone())
        .await
        .unwrap();

    // Seed an orchestrator message in the child
    let msg = ChatMessage::orchestrator_in_session(child.id.clone(), "Round 1 done.");
    state.app_state.chat_message_repo.create(msg).await.unwrap();

    // Set parent in_progress=true
    state
        .app_state
        .ideation_session_repo
        .update_verification_state(&parent_id, VerificationStatus::Reviewing, true)
        .await
        .unwrap();

    let result = get_plan_verification_external_http(
        State(state),
        unrestricted_scope(),
        Path(parent_id_str),
    )
    .await
    .expect("handler must succeed");

    let child_info = result
        .0
        .verification_child
        .expect("external endpoint must return verification_child block");

    assert_eq!(child_info.latest_child_session_id, child_id_str);
    assert!(!child_info.latest_child_archived);
    assert_eq!(
        child_info.active_child_session_id.as_deref(),
        Some(child_id_str.as_str()),
        "in_progress=true + non-archived → active_child_session_id must be set"
    );
    assert_eq!(
        child_info.last_assistant_message.as_deref(),
        Some("Round 1 done."),
        "orchestrator message must be surfaced on external endpoint"
    );
}

/// External endpoint: verification_child is null when no child exists.
#[tokio::test]
#[cfg(any())]
async fn test_get_plan_verification_external_no_child_returns_null() {
    let state = setup_test_state().await;
    let (_, session_id) = setup_session(&state, "proj-vc-null", "No Child Project").await;
    let session_id_obj = IdeationSessionId::from_string(session_id.clone());

    state
        .app_state
        .ideation_session_repo
        .update_verification_state(&session_id_obj, VerificationStatus::Reviewing, true)
        .await
        .unwrap();

    let result =
        get_plan_verification_external_http(State(state), unrestricted_scope(), Path(session_id))
            .await
            .expect("handler must succeed");

    assert!(
        result.0.verification_child.is_none(),
        "no child → verification_child must be null"
    );
}

// ============================================================================
// Plan import integration tests
// ============================================================================

/// Shared setup for plan import tests: creates a project with git repo,
/// a source ideation session with a Specification artifact, a parent
/// conversation with an Ideation workspace, and a user message containing
/// a plan artifact reference in metadata.
async fn setup_plan_import_test() -> PlanImportTestFixture {
    setup_plan_import_test_with_state(setup_test_state().await).await
}

async fn setup_plan_import_test_sqlite() -> PlanImportTestFixture {
    setup_plan_import_test_with_state(setup_sqlite_test_state().await).await
}

async fn setup_plan_import_test_with_state(state: HttpServerState) -> PlanImportTestFixture {
    use ralphx_lib::application::agent_conversation_workspace::{
        prepare_agent_conversation_workspace, AgentConversationWorkspaceBaseSelection,
    };
    use ralphx_lib::domain::entities::artifact::{Artifact, ArtifactType};
    use ralphx_lib::domain::entities::{
        AgentConversationWorkspaceMode, ChatConversation, IdeationAnalysisBaseRefKind,
    };

    let repo_dir = tempfile::TempDir::new().unwrap();
    let repo_path = repo_dir.path();
    std::process::Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(repo_path)
        .output()
        .expect("git init");
    std::process::Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(repo_path)
        .output()
        .expect("git config email");
    std::process::Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(repo_path)
        .output()
        .expect("git config name");
    std::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(repo_path)
        .output()
        .expect("git commit");
    let worktree_parent = tempfile::TempDir::new().unwrap();

    let project_id = ProjectId::from_string(format!("proj-plan-import-{}", uuid::Uuid::new_v4()));
    let mut project = Project::new(
        "Plan Import Project".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.id = project_id.clone();
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktree_parent.path().to_string_lossy().to_string());
    let project = state.app_state.project_repo.create(project).await.unwrap();

    let source_artifact = Artifact::new_inline(
        "Source Plan",
        ArtifactType::Specification,
        "# Plan Content\n\n## Task 1\nImplement feature X\n\n## Task 2\nAdd tests",
        "test",
    );
    let source_artifact = state
        .app_state
        .artifact_repo
        .create(source_artifact)
        .await
        .unwrap();

    let source_session = IdeationSessionBuilder::new()
        .project_id(project_id.clone())
        .title("Source Session".to_string())
        .status(IdeationSessionStatus::Active)
        .plan_artifact_id(source_artifact.id.clone())
        .build();
    let source_session = state
        .app_state
        .ideation_session_repo
        .create(source_session)
        .await
        .unwrap();

    let conversation = state
        .app_state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project_id.clone()))
        .await
        .expect("create project conversation");

    let workspace = prepare_agent_conversation_workspace(
        &project,
        &conversation.id,
        AgentConversationWorkspaceMode::Ideation,
        AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
            branch_mode: None,
            base_ref: Some("main".to_string()),
            display_name: Some("Project default (main)".to_string()),
            source_pull_request: None,
        },
    )
    .await
    .expect("prepare agent workspace");
    state
        .app_state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("save agent workspace");

    let mut msg = ChatMessage::user_in_session(
        IdeationSessionId::from_string("dummy".to_string()),
        "Start from plan",
    );
    msg.conversation_id = Some(conversation.id);
    msg.metadata = Some(
        serde_json::json!({
            "composer_artifact_references": [{
                "artifactId": source_artifact.id.as_str(),
                "kind": "plan",
                "sessionId": source_session.id.as_str(),
                "title": "Source Plan"
            }]
        })
        .to_string(),
    );
    state.app_state.chat_message_repo.create(msg).await.unwrap();

    PlanImportTestFixture {
        state,
        project_id,
        source_session_id: source_session.id.clone(),
        source_artifact_id: source_artifact.id.clone(),
        conversation_id: conversation.id,
        _repo_dir: repo_dir,
        _worktree_parent: worktree_parent,
    }
}

struct PlanImportTestFixture {
    state: HttpServerState,
    project_id: ProjectId,
    source_session_id: IdeationSessionId,
    source_artifact_id: ralphx_lib::domain::entities::artifact::ArtifactId,
    conversation_id: ralphx_lib::domain::entities::ChatConversationId,
    _repo_dir: tempfile::TempDir,
    _worktree_parent: tempfile::TempDir,
}

impl PlanImportTestFixture {
    fn make_headers(&self) -> axum::http::HeaderMap {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("x-ralphx-tauri-mcp", "1".parse().unwrap());
        headers.insert(
            "x-ralphx-parent-conversation-id",
            self.conversation_id.as_str().parse().unwrap(),
        );
        headers
    }

    fn make_request(&self) -> StartIdeationRequest {
        StartIdeationRequest {
            project_id: self.project_id.as_str().to_string(),
            title: Some("Plan import test".to_string()),
            prompt: None,
            initial_prompt: None,
            idempotency_key: None,
        }
    }
}

#[tokio::test]
async fn test_plan_import_happy_path() {
    let fix = setup_plan_import_test().await;

    let result = start_ideation_http(
        State(fix.state.clone()),
        unrestricted_scope(),
        fix.make_headers(),
        Json(fix.make_request()),
    )
    .await
    .expect("start ideation with plan import should succeed")
    .0;

    assert_eq!(result.plan_imported, Some(true));
    assert_eq!(
        result.source_plan_artifact_id.as_deref(),
        Some(fix.source_artifact_id.as_str())
    );
    let cloned_id = result
        .cloned_plan_artifact_id
        .as_ref()
        .expect("cloned_plan_artifact_id should be set");
    assert_ne!(
        cloned_id.as_str(),
        fix.source_artifact_id.as_str(),
        "cloned artifact must have a different ID from source"
    );

    let cloned_artifact = fix
        .state
        .app_state
        .artifact_repo
        .get_by_id(
            &ralphx_lib::domain::entities::artifact::ArtifactId::from_string(cloned_id.clone()),
        )
        .await
        .unwrap()
        .expect("cloned artifact should exist");
    let source_artifact = fix
        .state
        .app_state
        .artifact_repo
        .get_by_id(&fix.source_artifact_id)
        .await
        .unwrap()
        .expect("source artifact should exist");
    assert_eq!(cloned_artifact.content, source_artifact.content);
    assert!(
        cloned_artifact
            .derived_from
            .contains(&fix.source_artifact_id),
        "cloned artifact should be derived from source"
    );

    let new_session = fix
        .state
        .app_state
        .ideation_session_repo
        .get_by_id(&IdeationSessionId::from_string(result.session_id.clone()))
        .await
        .unwrap()
        .expect("new session should exist");
    assert_eq!(
        new_session.plan_artifact_id.as_ref().map(|a| a.as_str()),
        Some(cloned_id.as_str()),
        "new session's plan_artifact_id should point to the clone"
    );
}

#[tokio::test]
async fn test_plan_import_isolation_no_inherited_state() {
    let fix = setup_plan_import_test().await;

    let result = start_ideation_http(
        State(fix.state.clone()),
        unrestricted_scope(),
        fix.make_headers(),
        Json(fix.make_request()),
    )
    .await
    .expect("plan import should succeed")
    .0;

    let new_session = fix
        .state
        .app_state
        .ideation_session_repo
        .get_by_id(&IdeationSessionId::from_string(result.session_id.clone()))
        .await
        .unwrap()
        .expect("new session should exist");

    assert!(
        new_session.inherited_plan_artifact_id.is_none(),
        "imported session must not inherit plan artifact"
    );
    assert!(
        new_session.parent_session_id.is_none(),
        "imported session must not have parent_session_id"
    );
    assert_eq!(
        new_session.verification_status,
        VerificationStatus::Unverified,
        "imported session must start with unverified status"
    );
    assert!(
        !new_session.verification_in_progress,
        "imported session must not have verification in progress"
    );
    assert_eq!(
        new_session.verification_generation, 0,
        "imported session must start with verification_generation 0"
    );
}

#[tokio::test]
async fn test_plan_import_source_unchanged() {
    let fix = setup_plan_import_test().await;

    let source_before = fix
        .state
        .app_state
        .artifact_repo
        .get_by_id(&fix.source_artifact_id)
        .await
        .unwrap()
        .expect("source artifact before import");
    let source_session_before = fix
        .state
        .app_state
        .ideation_session_repo
        .get_by_id(&fix.source_session_id)
        .await
        .unwrap()
        .expect("source session before import");

    let _result = start_ideation_http(
        State(fix.state.clone()),
        unrestricted_scope(),
        fix.make_headers(),
        Json(fix.make_request()),
    )
    .await
    .expect("plan import should succeed");

    let source_after = fix
        .state
        .app_state
        .artifact_repo
        .get_by_id(&fix.source_artifact_id)
        .await
        .unwrap()
        .expect("source artifact after import");
    let source_session_after = fix
        .state
        .app_state
        .ideation_session_repo
        .get_by_id(&fix.source_session_id)
        .await
        .unwrap()
        .expect("source session after import");

    assert_eq!(source_before.content, source_after.content);
    assert_eq!(
        source_before.id, source_after.id,
        "source artifact ID must not change"
    );
    assert_eq!(
        source_session_before.plan_artifact_id, source_session_after.plan_artifact_id,
        "source session plan_artifact_id must not change"
    );
    assert_eq!(
        source_session_before.status, source_session_after.status,
        "source session status must not change"
    );
}

#[tokio::test]
async fn test_plan_import_approved_source_propagates_approval() {
    let fix = setup_plan_import_test_sqlite().await;

    // Update source session to Accepted status
    fix.state
        .app_state
        .ideation_session_repo
        .update_status(&fix.source_session_id, IdeationSessionStatus::Accepted)
        .await
        .unwrap();

    // Insert source approval into plan_artifact_approvals table
    let source_sid = fix.source_session_id.as_str().to_string();
    let source_aid = fix.source_artifact_id.as_str().to_string();
    fix.state
        .app_state
        .db
        .run(move |conn| {
            conn.execute(
                "INSERT INTO plan_artifact_approvals (session_id, artifact_id, artifact_version, status, approved_at) VALUES (?1, ?2, 1, 'approved', datetime('now'))",
                rusqlite::params![source_sid, source_aid],
            )?;
            Ok(())
        })
        .await
        .unwrap();

    let result = start_ideation_http(
        State(fix.state.clone()),
        unrestricted_scope(),
        fix.make_headers(),
        Json(fix.make_request()),
    )
    .await
    .expect("plan import should succeed")
    .0;

    let new_session_id = result.session_id.clone();
    let approval_count: i64 = fix
        .state
        .app_state
        .db
        .run(move |conn| {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM plan_artifact_approvals WHERE session_id = ?1 AND status = 'approved'",
                    [new_session_id.as_str()],
                    |row| row.get(0),
                )
                .unwrap_or(0);
            Ok(count)
        })
        .await
        .unwrap();

    assert_eq!(
        approval_count, 1,
        "accepted source should propagate approval to cloned plan"
    );
}

#[tokio::test]
async fn test_plan_import_draft_source_no_approval() {
    let fix = setup_plan_import_test_sqlite().await;

    let result = start_ideation_http(
        State(fix.state.clone()),
        unrestricted_scope(),
        fix.make_headers(),
        Json(fix.make_request()),
    )
    .await
    .expect("plan import should succeed")
    .0;

    let new_session_id = result.session_id.clone();
    let approval_count: i64 = fix
        .state
        .app_state
        .db
        .run(move |conn| {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM plan_artifact_approvals WHERE session_id = ?1 AND status = 'approved'",
                    [new_session_id.as_str()],
                    |row| row.get(0),
                )
                .unwrap_or(0);
            Ok(count)
        })
        .await
        .unwrap();

    assert_eq!(
        approval_count, 0,
        "draft source should not propagate approval"
    );
}

#[tokio::test]
async fn test_plan_import_multiple_refs_error() {
    use ralphx_lib::domain::entities::artifact::{Artifact, ArtifactType};

    let fix = setup_plan_import_test().await;

    let second_artifact = Artifact::new_inline(
        "Second Plan",
        ArtifactType::Specification,
        "# Second plan content",
        "test",
    );
    let second_artifact = fix
        .state
        .app_state
        .artifact_repo
        .create(second_artifact)
        .await
        .unwrap();

    // Replace the message with one containing two plan references
    let mut msg = ChatMessage::user_in_session(
        IdeationSessionId::from_string("dummy".to_string()),
        "Start from two plans",
    );
    msg.conversation_id = Some(fix.conversation_id);
    msg.metadata = Some(
        serde_json::json!({
            "composer_artifact_references": [
                {
                    "artifactId": fix.source_artifact_id.as_str(),
                    "kind": "plan",
                    "sessionId": fix.source_session_id.as_str(),
                    "title": "Plan 1"
                },
                {
                    "artifactId": second_artifact.id.as_str(),
                    "kind": "plan",
                    "sessionId": fix.source_session_id.as_str(),
                    "title": "Plan 2"
                }
            ]
        })
        .to_string(),
    );
    fix.state
        .app_state
        .chat_message_repo
        .create(msg)
        .await
        .unwrap();

    let err = start_ideation_http(
        State(fix.state.clone()),
        unrestricted_scope(),
        fix.make_headers(),
        Json(fix.make_request()),
    )
    .await
    .expect_err("multiple plan references should fail");

    assert_eq!(err.status, axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_plan_import_missing_artifact_error() {
    use ralphx_lib::domain::entities::ChatConversation;

    let fix = setup_plan_import_test().await;

    // Create a new conversation with a message pointing to a non-existent artifact
    let conv2 = fix
        .state
        .app_state
        .chat_conversation_repo
        .create(ChatConversation::new_project(fix.project_id.clone()))
        .await
        .unwrap();

    use ralphx_lib::application::agent_conversation_workspace::{
        prepare_agent_conversation_workspace, AgentConversationWorkspaceBaseSelection,
    };
    use ralphx_lib::domain::entities::{
        AgentConversationWorkspaceMode, IdeationAnalysisBaseRefKind,
    };
    let project = fix
        .state
        .app_state
        .project_repo
        .get_by_id(&fix.project_id)
        .await
        .unwrap()
        .unwrap();
    let ws2 = prepare_agent_conversation_workspace(
        &project,
        &conv2.id,
        AgentConversationWorkspaceMode::Ideation,
        AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
            branch_mode: None,
            base_ref: Some("main".to_string()),
            display_name: Some("Project default (main)".to_string()),
            source_pull_request: None,
        },
    )
    .await
    .unwrap();
    fix.state
        .app_state
        .agent_conversation_workspace_repo
        .create_or_update(ws2)
        .await
        .unwrap();

    let mut msg = ChatMessage::user_in_session(
        IdeationSessionId::from_string("dummy".to_string()),
        "Missing artifact",
    );
    msg.conversation_id = Some(conv2.id);
    msg.metadata = Some(
        serde_json::json!({
            "composer_artifact_references": [{
                "artifactId": "nonexistent-artifact-id",
                "kind": "plan",
                "sessionId": fix.source_session_id.as_str(),
                "title": "Ghost Plan"
            }]
        })
        .to_string(),
    );
    fix.state
        .app_state
        .chat_message_repo
        .create(msg)
        .await
        .unwrap();

    let mut headers = axum::http::HeaderMap::new();
    headers.insert("x-ralphx-tauri-mcp", "1".parse().unwrap());
    headers.insert(
        "x-ralphx-parent-conversation-id",
        conv2.id.as_str().parse().unwrap(),
    );

    let err = start_ideation_http(
        State(fix.state.clone()),
        unrestricted_scope(),
        headers,
        Json(StartIdeationRequest {
            project_id: fix.project_id.as_str().to_string(),
            title: Some("Missing artifact test".to_string()),
            prompt: None,
            initial_prompt: None,
            idempotency_key: None,
        }),
    )
    .await
    .expect_err("missing source artifact should fail");

    assert_eq!(err.status, axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_plan_import_wrong_project_error() {
    let fix = setup_plan_import_test().await;

    // Create a session in a different project
    let other_project_id = ProjectId::from_string("proj-other-project".to_string());
    let mut other_project = Project::new("Other Project".to_string(), "/tmp/other".to_string());
    other_project.id = other_project_id.clone();
    fix.state
        .app_state
        .project_repo
        .create(other_project)
        .await
        .unwrap();

    let other_session = IdeationSessionBuilder::new()
        .project_id(other_project_id)
        .title("Other Project Session".to_string())
        .plan_artifact_id(fix.source_artifact_id.clone())
        .build();
    let other_session = fix
        .state
        .app_state
        .ideation_session_repo
        .create(other_session)
        .await
        .unwrap();

    // Create a new conversation with reference to the other-project session
    use ralphx_lib::domain::entities::ChatConversation;
    let conv2 = fix
        .state
        .app_state
        .chat_conversation_repo
        .create(ChatConversation::new_project(fix.project_id.clone()))
        .await
        .unwrap();

    use ralphx_lib::application::agent_conversation_workspace::{
        prepare_agent_conversation_workspace, AgentConversationWorkspaceBaseSelection,
    };
    use ralphx_lib::domain::entities::{
        AgentConversationWorkspaceMode, IdeationAnalysisBaseRefKind,
    };
    let project = fix
        .state
        .app_state
        .project_repo
        .get_by_id(&fix.project_id)
        .await
        .unwrap()
        .unwrap();
    let ws2 = prepare_agent_conversation_workspace(
        &project,
        &conv2.id,
        AgentConversationWorkspaceMode::Ideation,
        AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
            branch_mode: None,
            base_ref: Some("main".to_string()),
            display_name: Some("Project default (main)".to_string()),
            source_pull_request: None,
        },
    )
    .await
    .unwrap();
    fix.state
        .app_state
        .agent_conversation_workspace_repo
        .create_or_update(ws2)
        .await
        .unwrap();

    let mut msg = ChatMessage::user_in_session(
        IdeationSessionId::from_string("dummy".to_string()),
        "Wrong project",
    );
    msg.conversation_id = Some(conv2.id);
    msg.metadata = Some(
        serde_json::json!({
            "composer_artifact_references": [{
                "artifactId": fix.source_artifact_id.as_str(),
                "kind": "plan",
                "sessionId": other_session.id.as_str(),
                "title": "Cross-project Plan"
            }]
        })
        .to_string(),
    );
    fix.state
        .app_state
        .chat_message_repo
        .create(msg)
        .await
        .unwrap();

    let mut headers = axum::http::HeaderMap::new();
    headers.insert("x-ralphx-tauri-mcp", "1".parse().unwrap());
    headers.insert(
        "x-ralphx-parent-conversation-id",
        conv2.id.as_str().parse().unwrap(),
    );

    let err = start_ideation_http(
        State(fix.state.clone()),
        unrestricted_scope(),
        headers,
        Json(StartIdeationRequest {
            project_id: fix.project_id.as_str().to_string(),
            title: Some("Wrong project test".to_string()),
            prompt: None,
            initial_prompt: None,
            idempotency_key: None,
        }),
    )
    .await
    .expect_err("cross-project plan import should fail");

    assert_eq!(err.status, axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_plan_import_archived_source_error() {
    let fix = setup_plan_import_test().await;

    fix.state
        .app_state
        .ideation_session_repo
        .update_status(&fix.source_session_id, IdeationSessionStatus::Archived)
        .await
        .unwrap();

    let err = start_ideation_http(
        State(fix.state.clone()),
        unrestricted_scope(),
        fix.make_headers(),
        Json(fix.make_request()),
    )
    .await
    .expect_err("archived source should fail");

    assert_eq!(err.status, axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_plan_import_verification_child_error() {
    use ralphx_lib::application::agent_conversation_workspace::{
        prepare_agent_conversation_workspace, AgentConversationWorkspaceBaseSelection,
    };
    use ralphx_lib::domain::entities::artifact::{Artifact, ArtifactType};
    use ralphx_lib::domain::entities::{
        AgentConversationWorkspaceMode, ChatConversation, IdeationAnalysisBaseRefKind,
    };

    let state = setup_test_state().await;

    let repo_dir = tempfile::TempDir::new().unwrap();
    let repo_path = repo_dir.path();
    std::process::Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(repo_path)
        .output()
        .expect("git init");
    std::process::Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(repo_path)
        .output()
        .expect("git config email");
    std::process::Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(repo_path)
        .output()
        .expect("git config name");
    std::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(repo_path)
        .output()
        .expect("git commit");
    let worktree_parent = tempfile::TempDir::new().unwrap();

    let project_id = ProjectId::from_string(format!("proj-verif-child-{}", uuid::Uuid::new_v4()));
    let mut project = Project::new(
        "Verif Child Project".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.id = project_id.clone();
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktree_parent.path().to_string_lossy().to_string());
    let project = state.app_state.project_repo.create(project).await.unwrap();

    let source_artifact = Artifact::new_inline(
        "Verif Plan",
        ArtifactType::Specification,
        "# Verification plan",
        "test",
    );
    let source_artifact = state
        .app_state
        .artifact_repo
        .create(source_artifact)
        .await
        .unwrap();

    // Create source session as a verification child
    let verif_session = IdeationSessionBuilder::new()
        .project_id(project_id.clone())
        .title("Verification Session".to_string())
        .plan_artifact_id(source_artifact.id.clone())
        .session_purpose(SessionPurpose::Verification)
        .build();
    let verif_session = state
        .app_state
        .ideation_session_repo
        .create(verif_session)
        .await
        .unwrap();

    let conversation = state
        .app_state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project_id.clone()))
        .await
        .unwrap();

    let workspace = prepare_agent_conversation_workspace(
        &project,
        &conversation.id,
        AgentConversationWorkspaceMode::Ideation,
        AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
            branch_mode: None,
            base_ref: Some("main".to_string()),
            display_name: Some("Project default (main)".to_string()),
            source_pull_request: None,
        },
    )
    .await
    .unwrap();
    state
        .app_state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();

    let mut msg = ChatMessage::user_in_session(
        IdeationSessionId::from_string("dummy".to_string()),
        "From verification child",
    );
    msg.conversation_id = Some(conversation.id);
    msg.metadata = Some(
        serde_json::json!({
            "composer_artifact_references": [{
                "artifactId": source_artifact.id.as_str(),
                "kind": "plan",
                "sessionId": verif_session.id.as_str(),
                "title": "Verif Plan"
            }]
        })
        .to_string(),
    );
    state.app_state.chat_message_repo.create(msg).await.unwrap();

    let mut headers = axum::http::HeaderMap::new();
    headers.insert("x-ralphx-tauri-mcp", "1".parse().unwrap());
    headers.insert(
        "x-ralphx-parent-conversation-id",
        conversation.id.as_str().parse().unwrap(),
    );

    let err = start_ideation_http(
        State(state),
        unrestricted_scope(),
        headers,
        Json(StartIdeationRequest {
            project_id: project_id.as_str().to_string(),
            title: Some("Verif child test".to_string()),
            prompt: None,
            initial_prompt: None,
            idempotency_key: None,
        }),
    )
    .await
    .expect_err("verification child source should fail");

    assert_eq!(err.status, axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_plan_import_non_spec_artifact_error() {
    use ralphx_lib::domain::entities::artifact::{Artifact, ArtifactType};
    use ralphx_lib::domain::entities::ChatConversation;

    let fix = setup_plan_import_test().await;

    // Create a non-specification artifact
    let research_artifact = Artifact::new_inline(
        "Research Doc",
        ArtifactType::ResearchDocument,
        "# Research findings",
        "test",
    );
    let research_artifact = fix
        .state
        .app_state
        .artifact_repo
        .create(research_artifact)
        .await
        .unwrap();

    let conv2 = fix
        .state
        .app_state
        .chat_conversation_repo
        .create(ChatConversation::new_project(fix.project_id.clone()))
        .await
        .unwrap();

    use ralphx_lib::application::agent_conversation_workspace::{
        prepare_agent_conversation_workspace, AgentConversationWorkspaceBaseSelection,
    };
    use ralphx_lib::domain::entities::{
        AgentConversationWorkspaceMode, IdeationAnalysisBaseRefKind,
    };
    let project = fix
        .state
        .app_state
        .project_repo
        .get_by_id(&fix.project_id)
        .await
        .unwrap()
        .unwrap();
    let ws2 = prepare_agent_conversation_workspace(
        &project,
        &conv2.id,
        AgentConversationWorkspaceMode::Ideation,
        AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
            branch_mode: None,
            base_ref: Some("main".to_string()),
            display_name: Some("Project default (main)".to_string()),
            source_pull_request: None,
        },
    )
    .await
    .unwrap();
    fix.state
        .app_state
        .agent_conversation_workspace_repo
        .create_or_update(ws2)
        .await
        .unwrap();

    let mut msg = ChatMessage::user_in_session(
        IdeationSessionId::from_string("dummy".to_string()),
        "Non-spec artifact",
    );
    msg.conversation_id = Some(conv2.id);
    msg.metadata = Some(
        serde_json::json!({
            "composer_artifact_references": [{
                "artifactId": research_artifact.id.as_str(),
                "kind": "plan",
                "sessionId": fix.source_session_id.as_str(),
                "title": "Research Doc"
            }]
        })
        .to_string(),
    );
    fix.state
        .app_state
        .chat_message_repo
        .create(msg)
        .await
        .unwrap();

    let mut headers = axum::http::HeaderMap::new();
    headers.insert("x-ralphx-tauri-mcp", "1".parse().unwrap());
    headers.insert(
        "x-ralphx-parent-conversation-id",
        conv2.id.as_str().parse().unwrap(),
    );

    let err = start_ideation_http(
        State(fix.state.clone()),
        unrestricted_scope(),
        headers,
        Json(StartIdeationRequest {
            project_id: fix.project_id.as_str().to_string(),
            title: Some("Non-spec test".to_string()),
            prompt: None,
            initial_prompt: None,
            idempotency_key: None,
        }),
    )
    .await
    .expect_err("non-specification artifact should fail");

    assert_eq!(err.status, axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_plan_import_idempotency() {
    let fix = setup_plan_import_test_sqlite().await;

    let make_idem_request = || StartIdeationRequest {
        project_id: fix.project_id.as_str().to_string(),
        title: Some("Plan import test".to_string()),
        prompt: None,
        initial_prompt: None,
        idempotency_key: Some("plan-import-idem-key".to_string()),
    };

    let make_idem_headers = || {
        let mut headers = fix.make_headers();
        headers.insert("x-ralphx-key-id", "test-api-key".parse().unwrap());
        headers
    };

    let result1 = start_ideation_http(
        State(fix.state.clone()),
        unrestricted_scope(),
        make_idem_headers(),
        Json(make_idem_request()),
    )
    .await
    .expect("first plan import should succeed")
    .0;

    assert_eq!(result1.plan_imported, Some(true));

    let result2 = start_ideation_http(
        State(fix.state.clone()),
        unrestricted_scope(),
        make_idem_headers(),
        Json(make_idem_request()),
    )
    .await
    .expect("second plan import with same key should succeed")
    .0;

    assert_eq!(
        result1.session_id, result2.session_id,
        "idempotent retry must return the same session"
    );
    assert_eq!(
        result2.exists,
        Some(true),
        "idempotent retry should set exists=true"
    );
}

#[tokio::test]
async fn test_plan_import_bypasses_similarity_dedup() {
    let fix = setup_plan_import_test().await;

    // Create an existing active session with similar title
    let existing_session = IdeationSessionBuilder::new()
        .project_id(fix.project_id.clone())
        .title("Plan import test".to_string())
        .status(IdeationSessionStatus::Active)
        .origin(SessionOrigin::External)
        .build();
    let existing_session = fix
        .state
        .app_state
        .ideation_session_repo
        .create(existing_session)
        .await
        .unwrap();

    // Seed a first user message for the existing session so similarity has something to compare
    let seed_msg = ChatMessage::user_in_session(existing_session.id.clone(), "Plan import test");
    fix.state
        .app_state
        .chat_message_repo
        .create(seed_msg)
        .await
        .unwrap();

    let mut req = fix.make_request();
    req.title = Some("Plan import test".to_string());
    req.prompt = Some("Plan import test".to_string());

    let result = start_ideation_http(
        State(fix.state.clone()),
        unrestricted_scope(),
        fix.make_headers(),
        Json(req),
    )
    .await
    .expect("plan import should bypass similarity dedup")
    .0;

    assert_eq!(result.plan_imported, Some(true));
    assert_ne!(
        result.session_id,
        existing_session.id.as_str(),
        "plan import must create a new session, not reuse the similar one"
    );
    assert!(
        result.duplicate_detected.is_none() || result.duplicate_detected == Some(false),
        "duplicate_detected should not be set for plan imports"
    );
}
