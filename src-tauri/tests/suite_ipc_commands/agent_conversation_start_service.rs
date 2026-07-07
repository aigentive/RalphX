use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use ralphx_lib::application::agent_conversation_start_service::{
    AgentConversationStartDeps, AgentConversationStartResult, AgentConversationStartService,
    AgentWorkspaceSourcePullRequestInput, StartAgentConversationInput,
};
use ralphx_lib::application::agent_conversation_workspace::{
    prepare_agent_conversation_workspace, AgentConversationWorkspaceBaseSelection,
};
use ralphx_lib::application::{AppState, TeamService, TeamStateTracker};
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::entities::{
    AgentConversationWorkspaceBranchMode, AgentConversationWorkspaceMode, ChatContextType,
    ChatConversation, ChatConversationId, IdeationAnalysisBaseRefKind, Project, ProjectId,
};
use tauri::test::{mock_builder, mock_context, noop_assets};
use tauri::Manager;

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git command should run");
    assert!(
        output.status.success(),
        "git {:?} failed: {}{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn setup_repo(repo: &Path) {
    std::fs::create_dir_all(repo).expect("repo dir should be created");
    git(repo, &["init", "-b", "main"]);
    git(repo, &["config", "user.email", "test@example.com"]);
    git(repo, &["config", "user.name", "Test User"]);
    std::fs::write(repo.join("README.md"), "hello\n").expect("fixture file should be written");
    git(repo, &["add", "README.md"]);
    git(repo, &["commit", "-m", "initial"]);
}

async fn seed_project(
    state: &AppState,
    project_id: &str,
    repo_path: &Path,
    worktree_parent: &Path,
) -> Project {
    let mut project = Project::new(
        format!("Start service {project_id}"),
        repo_path.to_string_lossy().to_string(),
    );
    project.id = ProjectId::from_string(project_id.to_string());
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
    state
        .project_repo
        .create(project)
        .await
        .expect("project should persist")
}

fn build_app(
    state: AppState,
    execution_state: Arc<ExecutionState>,
) -> tauri::App<tauri::test::MockRuntime> {
    mock_builder()
        .manage(state)
        .manage(execution_state)
        .manage(Arc::new(TeamService::new_without_events(Arc::new(
            TeamStateTracker::new(),
        ))))
        .build(mock_context(noop_assets()))
        .expect("mock app should build")
}

fn service_start_input(
    project_id: &ProjectId,
    content: &str,
    mode: &str,
    base_ref: Option<&str>,
    branch_mode: Option<&str>,
    conversation_id: Option<&ChatConversationId>,
    source_pull_request: Option<AgentWorkspaceSourcePullRequestInput>,
) -> StartAgentConversationInput {
    StartAgentConversationInput {
        project_id: project_id.as_str().to_string(),
        content: content.to_string(),
        conversation_id: conversation_id.map(ChatConversationId::as_str),
        parent_conversation_id: None,
        title: None,
        provider_harness: None,
        model_override: None,
        logical_effort: None,
        codex_fast_mode: None,
        mode: Some(mode.to_string()),
        base_ref_kind: Some("local_branch".to_string()),
        base_branch_mode: branch_mode.map(str::to_string),
        base_ref: base_ref.map(str::to_string),
        base_display_name: base_ref.map(str::to_string),
        base_source_pull_request: source_pull_request,
        composer_project_references: Vec::new(),
        composer_integration_references: Vec::new(),
        composer_artifact_references: Vec::new(),
        team_intent: None,
    }
}

async fn start_with_app(
    app: &tauri::App<tauri::test::MockRuntime>,
    input: StartAgentConversationInput,
) -> Result<AgentConversationStartResult, String> {
    let state = app.state::<AppState>();
    let execution_state = app.state::<Arc<ExecutionState>>();
    let team_service = app.state::<Arc<TeamService>>();
    AgentConversationStartService::new(AgentConversationStartDeps {
        state: state.inner(),
        execution_state: execution_state.inner(),
        team_service: Some(team_service.inner().clone()),
        app_handle: app.handle().clone(),
    })
    .start(input)
    .await
}

#[tokio::test]
async fn start_service_pr_backed_local_branch_prepares_isolated_workspace() {
    ralphx_lib::testing::seed_available_harness_probes_for_test();
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_repo(&repo_path);
    let branch = "feature/service-source-pr";
    git(&repo_path, &["checkout", "-b", branch]);
    std::fs::write(repo_path.join("README.md"), "source pr\n")
        .expect("fixture update should be written");
    git(&repo_path, &["add", "README.md"]);
    git(&repo_path, &["commit", "-m", "source pr"]);
    let head_sha = git(&repo_path, &["rev-parse", "HEAD"]);
    git(&repo_path, &["checkout", "main"]);

    let state = AppState::new_test();
    let project = seed_project(
        &state,
        "project-start-service-success",
        &repo_path,
        &worktree_parent,
    )
    .await;
    let execution_state = Arc::new(ExecutionState::new());
    execution_state.pause();
    let app = build_app(state, execution_state);

    let result = start_with_app(
        &app,
        service_start_input(
            &project.id,
            "Start from PR",
            "edit",
            Some(branch),
            None,
            None,
            Some(AgentWorkspaceSourcePullRequestInput {
                number: 42,
                url: Some("https://github.com/owner/repo/pull/42".to_string()),
                title: Some("Service source PR".to_string()),
                head_ref_name: branch.to_string(),
                base_ref_name: Some("main".to_string()),
                head_ref_oid: Some(head_sha.clone()),
            }),
        ),
    )
    .await
    .expect("service start should queue while execution is paused");

    assert!(result.send_result.was_queued);
    let workspace = result.workspace.expect("edit mode creates workspace");
    assert_eq!(
        workspace.branch_mode,
        AgentConversationWorkspaceBranchMode::Isolated
    );
    assert_eq!(
        workspace.base_ref_kind,
        IdeationAnalysisBaseRefKind::LocalBranch
    );
    assert_eq!(workspace.base_ref, branch);
    assert_ne!(workspace.branch_name, branch);
    assert_eq!(workspace.publication_pr_number, None);
    assert_eq!(
        workspace
            .source_pull_request
            .as_ref()
            .map(|source| source.number),
        Some(42)
    );
}

#[tokio::test]
async fn start_service_linked_workspace_conflict_returns_retryable_error() {
    ralphx_lib::testing::seed_available_harness_probes_for_test();
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_repo(&repo_path);
    let branch = "feature/service-linked-conflict";
    git(&repo_path, &["checkout", "-b", branch]);
    git(&repo_path, &["checkout", "main"]);

    let state = AppState::new_test();
    let project = seed_project(
        &state,
        "project-start-service-conflict",
        &repo_path,
        &worktree_parent,
    )
    .await;
    let existing = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project.id.clone()))
        .await
        .expect("existing conversation should persist");
    let workspace = prepare_agent_conversation_workspace(
        &project,
        &existing.id,
        AgentConversationWorkspaceMode::Edit,
        AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::LocalBranch),
            branch_mode: Some(AgentConversationWorkspaceBranchMode::Linked),
            base_ref: Some(branch.to_string()),
            display_name: Some(branch.to_string()),
            source_pull_request: None,
        },
    )
    .await
    .expect("linked workspace should prepare");
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("linked workspace should persist");
    let execution_state = Arc::new(ExecutionState::new());
    execution_state.pause();
    let app = build_app(state, execution_state);

    let error = start_with_app(
        &app,
        service_start_input(
            &project.id,
            "Start linked conflict",
            "edit",
            Some(branch),
            Some("linked"),
            None,
            None,
        ),
    )
    .await
    .expect_err("linked branch conflict should fail before creating a chat");

    assert!(
        error.contains("[ralphx:linked_setup_failure]"),
        "unexpected error: {error}"
    );
    assert!(
        error.contains(branch) && error.contains(&existing.id.as_str()),
        "error should explain the conflict: {error}"
    );
    let conversations = app
        .state::<AppState>()
        .chat_conversation_repo
        .get_by_context(ChatContextType::Project, project.id.as_str())
        .await
        .expect("project conversations should load");
    assert_eq!(conversations.len(), 1);
}

#[tokio::test]
async fn start_service_archives_seeded_draft_on_linked_workspace_setup_failure() {
    ralphx_lib::testing::seed_available_harness_probes_for_test();
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_repo(&repo_path);
    let branch = "feature/service-primary-linked";
    git(&repo_path, &["checkout", "-b", branch]);

    let state = AppState::new_test();
    let project = seed_project(
        &state,
        "project-start-service-archive",
        &repo_path,
        &worktree_parent,
    )
    .await;
    let mut draft = ChatConversation::new_project(project.id.clone());
    draft.set_agent_mode(Some(AgentConversationWorkspaceMode::Edit));
    let draft = state
        .chat_conversation_repo
        .create(draft)
        .await
        .expect("draft conversation should persist");
    let execution_state = Arc::new(ExecutionState::new());
    execution_state.pause();
    let app = build_app(state, execution_state);

    let error = start_with_app(
        &app,
        service_start_input(
            &project.id,
            "Start linked primary checkout",
            "edit",
            Some(branch),
            Some("linked"),
            Some(&draft.id),
            None,
        ),
    )
    .await
    .expect_err("primary checkout linked setup should fail");

    assert!(
        error.contains("[ralphx:linked_setup_failure]"),
        "unexpected error: {error}"
    );
    assert!(
        error.contains("checked out in the project root"),
        "error should explain the checkout conflict: {error}"
    );
    let stored = app
        .state::<AppState>()
        .chat_conversation_repo
        .get_by_id(&draft.id)
        .await
        .expect("draft should load")
        .expect("draft should still exist");
    assert!(stored.archived_at.is_some());
    let workspace = app
        .state::<AppState>()
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&draft.id)
        .await
        .expect("workspace lookup should succeed");
    assert!(workspace.is_none());
}
