use std::process::Command;
use std::sync::Arc;

use crate::common::MockGithubService;
use axum::{extract::Path, http::StatusCode, Json};
use chrono::Utc;
use ralphx_lib::application::agent_conversation_workspace::resolve_agent_conversation_workspace_path;
use ralphx_lib::application::{AppState, TeamService, TeamStateTracker};
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::entities::plan_branch::{PrPushStatus, PrStatus};
use ralphx_lib::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode,
    AgentConversationWorkspacePublicationEvent, ArtifactId, ChatContextType, ChatConversation,
    ChatConversationId, IdeationAnalysisBaseRefKind, IdeationSessionId, PlanBranch, PlanBranchId,
    Project, ProjectId, TicketCanonicalBranch, TicketCanonicalBranchCycleState,
    TicketGitConventionSnapshot,
};
use ralphx_lib::domain::review::ReviewSettings;
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

async fn disable_workspace_review_gate(app_state: &AppState) {
    app_state
        .review_settings_repo
        .update_settings(&ReviewSettings {
            require_workspace_review: false,
            ..ReviewSettings::default()
        })
        .await
        .expect("disable workspace review policy for auto-publish fixture");
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
    disable_workspace_review_gate(&app_state).await;

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
async fn complete_repair_rejects_nonconforming_strict_ticket_commit_before_state_mutation() {
    let repo = tempfile::TempDir::new().expect("repo tempdir");
    let origin = tempfile::TempDir::new().expect("origin tempdir");
    let worktrees = tempfile::TempDir::new().expect("worktree tempdir");
    git(origin.path(), &["init", "--bare"]);
    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@example.com"]);
    git(repo.path(), &["config", "user.name", "RalphX Test"]);
    std::fs::write(repo.path().join("README.md"), "base\n").expect("write base file");
    git(repo.path(), &["add", "README.md"]);
    git(repo.path(), &["commit", "-m", "base"]);
    let base_sha = git(repo.path(), &["rev-parse", "HEAD"]);
    git(
        repo.path(),
        &["remote", "add", "origin", origin.path().to_str().unwrap()],
    );
    git(repo.path(), &["push", "-u", "origin", "main"]);

    let conversation_id = ChatConversationId::from_string("12121212-1212-1212-1212-121212121212");
    let mut project = Project::new(
        "Strict Repair Validation".to_string(),
        repo.path().to_string_lossy().to_string(),
    );
    project.id = ProjectId::from_string("project-strict-repair".to_string());
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktrees.path().to_string_lossy().to_string());
    let workspace_path =
        resolve_agent_conversation_workspace_path(&project, &conversation_id).unwrap();
    let branch_name = "eng-42_ticket_ada";
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
    git(&workspace_path, &["push", "-u", "origin", branch_name]);
    std::fs::write(workspace_path.join("repair.txt"), "repair\n").expect("write repair file");
    git(&workspace_path, &["add", "repair.txt"]);
    git(&workspace_path, &["commit", "-m", "repair workspace"]);
    let repair_sha = git(&workspace_path, &["rev-parse", "HEAD"]);

    let app_state = AppState::new_test();
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();
    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.id = conversation_id;
    conversation.context_type = ChatContextType::Project;
    conversation.context_id = project.id.as_str().to_string();
    app_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();
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
        .unwrap();
    let mut binding = TicketCanonicalBranch::new_strict(
        project.id.clone(),
        "clickup",
        "ENG-42",
        branch_name,
        "main",
        Some(base_sha.clone()),
        TicketGitConventionSnapshot {
            policy_version: 1,
            task_title: "Ticket".to_string(),
            username: Some("Ada".to_string()),
            commit_subject_rule: "ENG-42 - :summary:".to_string(),
            pr_title: "ENG-42 - Ticket".to_string(),
        },
        Utc::now(),
    );
    binding.origin_pushed = true;
    binding.cycle.state = TicketCanonicalBranchCycleState::Active;
    binding.cycle.effective_merge_base = Some(base_sha.clone());
    app_state
        .ticket_canonical_branch_repo
        .create_if_absent(binding)
        .await
        .unwrap();

    let state = make_http_state(app_state);
    let error = complete_agent_workspace_repair(
        axum::extract::State(state.clone()),
        Path(conversation_id.as_str().to_string()),
        Json(CompleteAgentWorkspaceRepairRequest {
            repair_commit_sha: repair_sha,
            resolved_base_ref: "main".to_string(),
            resolved_base_commit: base_sha,
            summary: "Resolved the strict repair".to_string(),
        }),
    )
    .await
    .expect_err("strict repair must reject a nonconforming commit");

    assert_eq!(error.0, StatusCode::CONFLICT);
    let error_body = error.1 .0;
    assert!(error_body["error"]
        .as_str()
        .is_some_and(|message| message.contains("ENG-42 - :summary:")));
    let refreshed = state
        .app_state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        refreshed.publication_push_status.as_deref(),
        Some("needs_agent")
    );
    assert!(state
        .app_state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn complete_update_only_repair_auto_publishes_when_enabled() {
    let repo = tempfile::TempDir::new().expect("repo tempdir");
    let worktrees = tempfile::TempDir::new().expect("worktree tempdir");

    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@example.com"]);
    git(repo.path(), &["config", "user.name", "RalphX Test"]);
    std::fs::write(repo.path().join("README.md"), "base\n").expect("write base file");
    git(repo.path(), &["add", "README.md"]);
    git(repo.path(), &["commit", "-m", "base"]);
    let base_sha = git(repo.path(), &["rev-parse", "HEAD"]);

    let conversation_id = ChatConversationId::from_string("33333333-3333-3333-3333-333333333333");
    let mut project = Project::new(
        "Agent Workspace Update Repair".to_string(),
        repo.path().to_string_lossy().to_string(),
    );
    project.id = ProjectId::from_string("project-update-repair".to_string());
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktrees.path().to_string_lossy().to_string());

    let workspace_path =
        resolve_agent_conversation_workspace_path(&project, &conversation_id).unwrap();
    let branch_name = "ralphx/test/agent-update-repair";
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
    workspace.publication_pr_number = Some(391);
    workspace.publication_pr_url = Some("https://github.com/example/ralphx/pull/391".to_string());
    workspace.publication_pr_status = Some("open".to_string());
    workspace.pr_autofix_enabled = true;
    workspace.pr_auto_merge_desired = true;
    workspace.pr_auto_merge_current = Some(true);
    workspace.pr_supervision_status = Some("fixing".to_string());
    workspace.pr_supervision_summary = Some("Workspace repair is in progress.".to_string());
    app_state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("seed workspace");
    app_state
        .agent_conversation_workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id,
            "repair_requested",
            "started",
            "Workspace agent repair requested before the base update can complete",
            Some("agent_fixable:update_only".to_string()),
        ))
        .await
        .expect("seed update-only repair request");
    disable_workspace_review_gate(&app_state).await;

    let state = make_http_state(app_state);
    let response = complete_agent_workspace_repair(
        axum::extract::State(state.clone()),
        Path(conversation_id.as_str().to_string()),
        Json(CompleteAgentWorkspaceRepairRequest {
            repair_commit_sha: repair_sha,
            resolved_base_ref: "main".to_string(),
            resolved_base_commit: base_sha,
            summary: "Resolved the stale base repair".to_string(),
        }),
    )
    .await
    .expect("update-only repair completion should succeed")
    .0;

    assert_eq!(response.new_status, "failed");
    assert_eq!(response.auto_publish_status.as_deref(), Some("failed"));
    assert!(response
        .auto_publish_error
        .as_deref()
        .is_some_and(|error| error.contains("GitHub integration is not available")));
    assert_eq!(response.pr_number, Some(391));
    assert_eq!(
        response.pr_url.as_deref(),
        Some("https://github.com/example/ralphx/pull/391")
    );

    let refreshed = state
        .app_state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("query workspace")
        .expect("workspace exists");
    assert_eq!(refreshed.publication_push_status.as_deref(), Some("failed"));
    assert_eq!(refreshed.pr_supervision_status.as_deref(), Some("fixing"));
    assert_eq!(refreshed.pr_auto_merge_current, Some(true));

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
    git(repo.path(), &["checkout", "main"]);

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
    let events = state
        .app_state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("query publication events");
    assert!(events.iter().any(|event| {
        event.step == "published"
            && event.status == "succeeded"
            && event.classification.as_deref() == Some("published:90")
    }));
}
