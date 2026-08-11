use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use crate::application::agent_workspace_pr_supervision_recovery::{
    recover_agent_workspace_pr_supervision, AgentWorkspacePrSupervisionRecoveryDeps,
    AgentWorkspacePrSupervisionRecoveryOutcome, AgentWorkspacePrSupervisionRecoveryTrigger,
};
use crate::application::agent_workspace_publish_recovery::{
    recover_stale_agent_workspace_publish_repairs_for_state,
    recover_stale_publish_repair_for_workspace_with_project_repo,
};
use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode,
    AgentConversationWorkspacePublicationEvent, AgentRun, AgentRunStatus, ChatConversationId,
    IdeationAnalysisBaseRefKind, Project,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, AgentRunRepository, AgentWorkspaceRepairRepository,
    PlanBranchRepository,
};
use crate::domain::services::GithubServiceTrait;
use crate::infrastructure::memory::{
    MemoryAgentConversationWorkspaceRepository, MemoryAgentRunRepository,
    MemoryPlanBranchRepository, MemoryProjectRepository,
};
use crate::tests::mock_github_service::MockGithubService;

fn git(repo: &Path, args: &[&str]) -> String {
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

fn unfinished_recovery_fixture(
    root: &Path,
    conversation_id: ChatConversationId,
) -> (Project, AgentConversationWorkspace) {
    let repo = root.join("repo");
    std::fs::create_dir_all(&repo).expect("repo directory");
    git(&repo, &["init", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test User"]);
    std::fs::write(repo.join("README.md"), "base\n").expect("base file");
    git(&repo, &["add", "README.md"]);
    git(&repo, &["commit", "-m", "base"]);
    let base_sha = git(&repo, &["rev-parse", "HEAD"]);
    std::fs::write(repo.join("fix.txt"), "pending fix\n").expect("pending fix");
    std::fs::write(repo.join(".git").join("MERGE_HEAD"), "unfinished\n").expect("merge metadata");

    let mut project = Project::new(
        "Unfinished Review Recovery".to_string(),
        repo.to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    project.github_pr_enabled = true;
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id,
        project.id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some(base_sha),
        "ralphx/test/unfinished-recovery".to_string(),
        repo.to_string_lossy().to_string(),
    );
    workspace.publication_pr_number = Some(42);
    workspace.publication_pr_status = Some("open".to_string());
    workspace.publication_push_status = Some("needs_agent".to_string());
    workspace.pr_supervision_status = Some("fixing".to_string());
    workspace.pr_autofix_enabled = true;
    workspace.auto_publish_enabled = true;
    (project, workspace)
}

async fn seed_pending_handoff(
    repo: &dyn AgentConversationWorkspaceRepository,
    conversation_id: ChatConversationId,
) {
    repo.append_publication_event(AgentConversationWorkspacePublicationEvent::new(
        conversation_id,
        "pr_autofix_workspace_review",
        "reviewing",
        "PR fix completed; Workspace Review started before publishing resumes.",
        Some("workspace_review_started".to_string()),
    ))
    .await
    .expect("seed pending handoff");
}

#[tokio::test]
async fn workspace_review_unfinished_git_recovery_aborts_stale_publish_handoff() {
    let root = tempfile::tempdir().expect("fixture root");
    let conversation_id = ChatConversationId::new();
    let (project, workspace) = unfinished_recovery_fixture(root.path(), conversation_id);
    let state = AppState::new_test();
    state
        .project_repo
        .create(project)
        .await
        .expect("seed project");
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed workspace");
    seed_pending_handoff(
        state.agent_conversation_workspace_repo.as_ref(),
        conversation_id,
    )
    .await;
    let mut terminal_run = AgentRun::new(conversation_id.clone());
    terminal_run.status = AgentRunStatus::Failed;
    terminal_run.completed_at = Some(chrono::Utc::now());
    state
        .agent_run_repo
        .create(terminal_run)
        .await
        .expect("seed terminal repair run");

    let recovered = recover_stale_agent_workspace_publish_repairs_for_state(&state)
        .await
        .expect("unfinished handoff should recover");

    assert_eq!(recovered, 1);
    let after = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load workspace")
        .expect("workspace exists");
    assert_eq!(after.publication_push_status.as_deref(), Some("failed"));
    assert_eq!(after.pr_supervision_status.as_deref(), Some("blocked"));
    let after_events = state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("list events");
    assert_eq!(
        after_events
            .iter()
            .filter(|event| {
                event.step == "legacy_repair_import_blocked"
                    && event.status == "blocked"
                    && event.classification.as_deref() == Some("legacy_repair_import_ambiguous")
            })
            .count(),
        1
    );

    assert_eq!(
        recover_stale_agent_workspace_publish_repairs_for_state(&state)
            .await
            .expect("repeat recovery is idempotent"),
        0
    );
}

#[tokio::test]
async fn workspace_review_unfinished_git_recovery_stops_pr_supervision_before_side_effects() {
    let root = tempfile::tempdir().expect("fixture root");
    let conversation_id = ChatConversationId::new();
    let (project, mut workspace) = unfinished_recovery_fixture(root.path(), conversation_id);
    workspace.publication_push_status = Some("refreshed".to_string());
    workspace.pr_supervision_status = Some("reviewing".to_string());
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed workspace");
    seed_pending_handoff(workspace_repo.as_ref(), conversation_id).await;
    let github = Arc::new(MockGithubService::new());
    let run_repo = Arc::new(MemoryAgentRunRepository::new());
    let mut terminal_run = AgentRun::new(conversation_id.clone());
    terminal_run.status = AgentRunStatus::Failed;
    terminal_run.completed_at = Some(chrono::Utc::now());
    run_repo
        .create(terminal_run)
        .await
        .expect("seed terminal repair run");

    let outcome = recover_agent_workspace_pr_supervision(
        AgentWorkspacePrSupervisionRecoveryDeps {
            workspace_repo: Arc::clone(&workspace_repo)
                as Arc<dyn AgentConversationWorkspaceRepository>,
            project_repo: Arc::new(MemoryProjectRepository::with_projects(vec![project])),
            plan_branch_repo: Arc::new(MemoryPlanBranchRepository::new())
                as Arc<dyn PlanBranchRepository>,
            github: Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
            pr_poller_registry: None,
            transition_service: None,
            chat_service: None,
            agent_run_repo: run_repo as Arc<dyn AgentRunRepository>,
            agent_workspace_repair_repo: workspace_repo.clone(),
            events: Arc::new(ralphx_events::NullEventSink),
            pr_fix_review_publish_resumer: None,
            durable_recovery_state: None,
        },
        conversation_id,
        AgentWorkspacePrSupervisionRecoveryTrigger::Startup,
    )
    .await
    .expect("unsettled handoff should recover before PR supervision");

    assert!(matches!(
        outcome,
        AgentWorkspacePrSupervisionRecoveryOutcome::Skipped(_)
    ));
    let after = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load workspace")
        .expect("workspace exists");
    assert_eq!(after.publication_push_status.as_deref(), Some("failed"));
    assert_eq!(after.pr_supervision_status.as_deref(), Some("blocked"));
    let after_events = workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("list events");
    assert_eq!(
        after_events
            .iter()
            .filter(|event| event.step == "pr_autofix_workspace_review_aborted")
            .count(),
        1
    );
    assert_eq!(github.state().check_pr_sync_state_calls, 0);
    assert_eq!(github.state().fetch_pr_health_calls, 0);
}

#[tokio::test]
async fn pending_review_handoff_without_current_attempt_evidence_aborts_before_target_recovery() {
    let root = tempfile::tempdir().expect("fixture root");
    let conversation_id = ChatConversationId::new();
    let (project, mut workspace) = unfinished_recovery_fixture(root.path(), conversation_id);
    workspace.publication_push_status = Some("refreshed".to_string());
    workspace.pr_supervision_status = Some("reviewing".to_string());
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed workspace");
    seed_pending_handoff(workspace_repo.as_ref(), conversation_id).await;

    let (after, recovered) = recover_stale_publish_repair_for_workspace_with_project_repo(
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        Arc::clone(&workspace_repo) as Arc<dyn AgentWorkspaceRepairRepository>,
        Arc::new(MemoryAgentRunRepository::new()) as Arc<dyn AgentRunRepository>,
        Arc::new(MemoryProjectRepository::with_projects(vec![project])),
        workspace,
    )
    .await
    .expect("legacy handoff should terminalize");

    assert!(recovered);
    assert_eq!(after.publication_push_status.as_deref(), Some("failed"));
    assert_eq!(after.pr_supervision_status.as_deref(), Some("blocked"));
    assert!(after
        .pr_supervision_summary
        .as_deref()
        .unwrap()
        .contains("terminal repair evidence"));
}
