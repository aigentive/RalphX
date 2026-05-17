use std::process::Command;
use std::sync::Arc;

use axum::{extract::Path, Json};
mod common;
use common::MockGithubService;
use ralphx_lib::application::agent_conversation_workspace::resolve_agent_conversation_workspace_path;
use ralphx_lib::application::{AppState, TeamService, TeamStateTracker};
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::entities::plan_branch::{PrPushStatus, PrStatus};
use ralphx_lib::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, ArtifactId, ChatContextType,
    ChatConversation, ChatConversationId, IdeationAnalysisBaseRefKind, IdeationSessionId,
    PlanBranch, PlanBranchId, Project, ProjectId,
};
use ralphx_lib::domain::services::github_service::GithubServiceTrait;
use ralphx_lib::http_server::handlers::agent_workspaces::{
    complete_agent_workspace_repair, CompleteAgentWorkspaceRepairRequest,
};
use ralphx_lib::http_server::types::HttpServerState;

fn git(repo: impl AsRef<std::path::Path>, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git command should spawn");
    assert!(
        output.status.success(),
        "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn make_http_state(app_state: AppState) -> HttpServerState {
    let team_tracker = TeamStateTracker::new();
    HttpServerState {
        app_state: Arc::new(app_state),
        execution_state: Arc::new(ExecutionState::new()),
        team_tracker: team_tracker.clone(),
        team_service: Arc::new(TeamService::new_without_events(Arc::new(team_tracker))),
        delegation_service: Default::default(),
    }
}

#[tokio::test]
async fn complete_repair_attempts_publish_without_waiting_for_user_click() {
    let repo = tempfile::TempDir::new().expect("repo tempdir");
    let worktrees = tempfile::TempDir::new().expect("worktree tempdir");

    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@example.com"]);
    git(repo.path(), &["config", "user.name", "RalphX Test"]);
    std::fs::write(repo.path().join("README.md"), "base\n").expect("write base file");
    git(repo.path(), &["add", "README.md"]);
    git(repo.path(), &["commit", "-m", "base"]);
    let base_sha = git(repo.path(), &["rev-parse", "HEAD"]);

    let conversation_id = ChatConversationId::from_string("11111111-1111-1111-1111-111111111111");
    let mut project = Project::new(
        "Agent Workspace Auto Publish".to_string(),
        repo.path().to_string_lossy().to_string(),
    );
    project.id = ProjectId::from_string("project-auto-publish".to_string());
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktrees.path().to_string_lossy().to_string());

    let workspace_path =
        resolve_agent_conversation_workspace_path(&project, &conversation_id).unwrap();
    let branch_name = "ralphx/test/agent-auto-publish";
    git(
        repo.path(),
        &[
            "worktree",
            "add",
            "-b",
            branch_name,
            workspace_path.to_str().unwrap(),
            "main",
        ],
    );
    std::fs::write(workspace_path.join("repair.txt"), "repair\n").expect("write repair file");
    git(&workspace_path, &["add", "repair.txt"]);
    git(&workspace_path, &["commit", "-m", "repair workspace"]);
    let repair_sha = git(&workspace_path, &["rev-parse", "HEAD"]);

    let app_state = AppState::new_test();
    app_state
        .project_repo
        .create(project.clone())
        .await
        .expect("seed project");
    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.id = conversation_id;
    conversation.context_type = ChatContextType::Project;
    conversation.context_id = project.id.as_str().to_string();
    app_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("seed conversation");

    let mut workspace = AgentConversationWorkspace::new(
        conversation_id,
        project.id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some(base_sha.clone()),
        branch_name.to_string(),
        workspace_path.to_string_lossy().to_string(),
    );
    workspace.publication_push_status = Some("needs_agent".to_string());
    app_state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("seed workspace");

    let state = make_http_state(app_state);
    let response = complete_agent_workspace_repair(
        axum::extract::State(state.clone()),
        Path(conversation_id.as_str().to_string()),
        Json(CompleteAgentWorkspaceRepairRequest {
            repair_commit_sha: repair_sha.clone(),
            resolved_base_ref: "main".to_string(),
            resolved_base_commit: base_sha,
            summary: "Resolved the stale base repair".to_string(),
        }),
    )
    .await
    .expect("repair completion should succeed")
    .0;

    assert_eq!(response.new_status, "failed");
    assert_eq!(response.auto_publish_status.as_deref(), Some("failed"));
    assert!(response
        .auto_publish_error
        .as_deref()
        .is_some_and(|error| error.contains("GitHub integration is not available")));

    let refreshed = state
        .app_state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("query workspace")
        .expect("workspace exists");
    assert_eq!(refreshed.publication_push_status.as_deref(), Some("failed"));

    let events = state
        .app_state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("query events");
    assert!(events
        .iter()
        .any(|event| event.step == "repair_completed" && event.status == "succeeded"));
    assert!(events.iter().any(|event| {
        event.step == "failed"
            && event.status == "failed"
            && event
                .summary
                .contains("GitHub integration is not available")
    }));
}

#[tokio::test]
async fn complete_repair_uses_linked_plan_branch_for_ideation_workspace() {
    let repo = tempfile::TempDir::new().expect("repo tempdir");
    let worktrees = tempfile::TempDir::new().expect("worktree tempdir");

    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@example.com"]);
    git(repo.path(), &["config", "user.name", "RalphX Test"]);
    std::fs::write(repo.path().join("README.md"), "base\n").expect("write base file");
    git(repo.path(), &["add", "README.md"]);
    git(repo.path(), &["commit", "-m", "base"]);
    let base_sha = git(repo.path(), &["rev-parse", "HEAD"]);

    let plan_branch_name = "ralphx/test/plan-repair";
    git(repo.path(), &["checkout", "-b", plan_branch_name, "main"]);
    std::fs::write(repo.path().join("plan.txt"), "repair\n").expect("write plan repair");
    git(repo.path(), &["add", "plan.txt"]);
    git(repo.path(), &["commit", "-m", "repair linked plan"]);
    let repair_sha = git(repo.path(), &["rev-parse", "HEAD"]);

    let conversation_id = ChatConversationId::from_string("22222222-2222-2222-2222-222222222222");
    let mut project = Project::new(
        "Ideation Workspace Repair".to_string(),
        repo.path().to_string_lossy().to_string(),
    );
    project.id = ProjectId::from_string("project-ideation-repair".to_string());
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktrees.path().to_string_lossy().to_string());

    let workspace_path =
        resolve_agent_conversation_workspace_path(&project, &conversation_id).unwrap();
    let shell_branch_name = "ralphx/test/agent-shell";
    git(
        repo.path(),
        &[
            "worktree",
            "add",
            "-b",
            shell_branch_name,
            workspace_path.to_str().unwrap(),
            "main",
        ],
    );

    let mock_github = Arc::new(MockGithubService::new());
    let github_trait: Arc<dyn GithubServiceTrait> = mock_github.clone();
    let mut app_state = AppState::new_test();
    app_state.github_service = Some(github_trait);
    app_state
        .project_repo
        .create(project.clone())
        .await
        .expect("seed project");
    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.id = conversation_id;
    conversation.context_type = ChatContextType::Project;
    conversation.context_id = project.id.as_str().to_string();
    app_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("seed conversation");

    let session_id = IdeationSessionId::from_string("session-ideation-repair");
    let plan_branch_id = PlanBranchId::from_string("plan-branch-ideation-repair");
    let mut plan_branch = PlanBranch::new(
        ArtifactId::from_string("artifact-ideation-repair"),
        session_id.clone(),
        project.id.clone(),
        plan_branch_name.to_string(),
        "main".to_string(),
    );
    plan_branch.id = plan_branch_id.clone();
    plan_branch.pr_number = Some(90);
    plan_branch.pr_url = Some("https://github.com/mock/project/pull/90".to_string());
    plan_branch.pr_status = Some(PrStatus::Open);
    plan_branch.pr_push_status = PrPushStatus::Failed;
    app_state
        .plan_branch_repo
        .create(plan_branch)
        .await
        .expect("seed plan branch");

    let mut workspace = AgentConversationWorkspace::new(
        conversation_id,
        project.id.clone(),
        AgentConversationWorkspaceMode::Ideation,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some(base_sha.clone()),
        shell_branch_name.to_string(),
        workspace_path.to_string_lossy().to_string(),
    );
    workspace.linked_ideation_session_id = Some(session_id);
    workspace.linked_plan_branch_id = Some(plan_branch_id.clone());
    workspace.publication_push_status = Some("needs_agent".to_string());
    app_state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("seed workspace");

    let state = make_http_state(app_state);
    let response = complete_agent_workspace_repair(
        axum::extract::State(state.clone()),
        Path(conversation_id.as_str().to_string()),
        Json(CompleteAgentWorkspaceRepairRequest {
            repair_commit_sha: repair_sha,
            resolved_base_ref: "main".to_string(),
            resolved_base_commit: base_sha,
            summary: "Resolved the linked plan branch repair".to_string(),
        }),
    )
    .await
    .expect("ideation repair completion should succeed")
    .0;

    assert_eq!(response.new_status, "pushed");
    assert_eq!(response.auto_publish_status.as_deref(), Some("succeeded"));
    assert_eq!(response.auto_publish_error, None);
    assert_eq!(response.pr_number, Some(90));
    assert_eq!(mock_github.push_calls(), 1);

    let refreshed_plan_branch = state
        .app_state
        .plan_branch_repo
        .get_by_id(&plan_branch_id)
        .await
        .expect("query plan branch")
        .expect("plan branch exists");
    assert_eq!(refreshed_plan_branch.pr_push_status, PrPushStatus::Pushed);
}
