use std::path::Path;
use std::sync::Arc;

use crate::application::agent_conversation_workspace::resolve_agent_conversation_workspace_path;
use crate::application::agent_workspace_pr_supervision_recovery::{
    recover_agent_workspace_pr_supervision, AgentWorkspacePrSupervisionRecoveryDeps,
    AgentWorkspacePrSupervisionRecoveryOutcome, AgentWorkspacePrSupervisionRecoveryTrigger,
};
use crate::application::chat_service::MockChatService;
use crate::application::git_service::GitService;
use crate::application::services::PrPollerRegistry;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, ChatConversationId,
    IdeationAnalysisBaseRefKind, Project,
};
use crate::domain::repositories::AgentConversationWorkspaceRepository;
use crate::domain::services::github_service::{
    PrMergeStateStatus, PrMergeableState, PrStatus, PrSyncState,
};
use crate::domain::services::GithubServiceTrait;
use crate::infrastructure::memory::{
    MemoryAgentConversationWorkspaceRepository, MemoryAgentRunRepository,
    MemoryPlanBranchRepository, MemoryProjectRepository,
};
use crate::tests::mock_github_service::MockGithubService;

fn run_git(repo: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git command should run");
    assert!(
        output.status.success(),
        "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn init_repo(repo_path: &Path) {
    std::fs::create_dir_all(repo_path).expect("create repo dir");
    run_git(repo_path, &["init"]);
    run_git(repo_path, &["config", "user.email", "test@example.com"]);
    run_git(repo_path, &["config", "user.name", "Test User"]);
    run_git(repo_path, &["checkout", "-b", "main"]);
    std::fs::write(repo_path.join("README.md"), "base\n").expect("write readme");
    run_git(repo_path, &["add", "."]);
    run_git(repo_path, &["commit", "-m", "initial"]);
}

fn recovery_project(temp_dir: &tempfile::TempDir, repo_path: &Path, name: &str) -> Project {
    let mut project = Project::new(name.to_string(), repo_path.to_string_lossy().to_string());
    project.base_branch = Some("main".to_string());
    project.github_pr_enabled = true;
    project.worktree_parent_directory = Some(
        temp_dir
            .path()
            .join("worktrees")
            .to_string_lossy()
            .to_string(),
    );
    project
}

fn blocked_workspace(
    project: &Project,
    conversation_id: ChatConversationId,
    branch_name: &str,
) -> AgentConversationWorkspace {
    let worktree_path = resolve_agent_conversation_workspace_path(project, &conversation_id)
        .expect("workspace path should resolve");
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id,
        project.id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some("base-sha".to_string()),
        branch_name.to_string(),
        worktree_path.to_string_lossy().to_string(),
    );
    workspace.publication_pr_number = Some(257);
    workspace.publication_pr_url = Some("https://github.com/owner/repo/pull/257".to_string());
    workspace.publication_pr_status = Some("failed".to_string());
    workspace.publication_push_status = Some("failed".to_string());
    workspace.pr_supervision_status = Some("blocked".to_string());
    workspace.pr_autofix_enabled = true;
    workspace
}

fn open_sync_state(branch_name: &str, head_sha: &str) -> PrSyncState {
    PrSyncState {
        status: PrStatus::Open,
        merge_state_status: Some(PrMergeStateStatus::Clean),
        mergeable: Some(PrMergeableState::Mergeable),
        is_draft: false,
        head_ref_name: branch_name.to_string(),
        base_ref_name: "main".to_string(),
        head_ref_oid: Some(head_sha.to_string()),
        base_ref_oid: None,
    }
}

async fn setup_recovery_workspace(
    name: &str,
) -> (
    tempfile::TempDir,
    Project,
    AgentConversationWorkspace,
    String,
) {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let repo_path = temp_dir.path().join("repo");
    init_repo(&repo_path);
    let project = recovery_project(&temp_dir, &repo_path, name);
    let conversation_id = ChatConversationId::new();
    let branch_name = format!("ralphx/test/{name}");
    let workspace = blocked_workspace(&project, conversation_id, &branch_name);
    GitService::create_worktree(
        &repo_path,
        Path::new(&workspace.worktree_path),
        &branch_name,
        "main",
    )
    .await
    .expect("create workspace worktree");
    let head_sha = GitService::get_head_sha(Path::new(&workspace.worktree_path))
        .await
        .expect("read workspace head");
    (temp_dir, project, workspace, head_sha)
}

#[tokio::test]
async fn recovers_blocked_pr_supervision_when_remote_head_matches_local_workspace() {
    let (_temp_dir, project, workspace, head_sha) =
        setup_recovery_workspace("pr-supervision-recover").await;
    let conversation_id = workspace.conversation_id.clone();
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed workspace");
    let project_repo = Arc::new(MemoryProjectRepository::with_projects(vec![project]));
    let github = Arc::new(MockGithubService::new());
    github.will_return_sync_state(open_sync_state(&workspace.branch_name, &head_sha));
    let registry = Arc::new(PrPollerRegistry::new(
        Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>),
        Arc::new(MemoryPlanBranchRepository::new()),
    ));

    let outcome = recover_agent_workspace_pr_supervision(
        AgentWorkspacePrSupervisionRecoveryDeps {
            workspace_repo: Arc::clone(&workspace_repo)
                as Arc<dyn AgentConversationWorkspaceRepository>,
            project_repo,
            github,
            pr_poller_registry: Some(Arc::clone(&registry)),
            chat_service: Some(Arc::new(MockChatService::new())),
            agent_run_repo: Arc::new(MemoryAgentRunRepository::new()),
            app_handle: None,
        },
        conversation_id.clone(),
        AgentWorkspacePrSupervisionRecoveryTrigger::Startup,
    )
    .await
    .expect("recover supervision");

    assert_eq!(
        outcome,
        AgentWorkspacePrSupervisionRecoveryOutcome::Recovered {
            pr_number: 257,
            head_sha,
        }
    );
    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .expect("workspace should still exist");
    assert_eq!(updated.publication_push_status.as_deref(), Some("pushed"));
    assert_eq!(updated.publication_pr_status.as_deref(), Some("open"));
    assert_eq!(updated.pr_supervision_status.as_deref(), Some("monitoring"));
    let events = workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].step, "pr_supervision_recovered");
    assert!(registry.is_agent_workspace_polling(&conversation_id));
    registry.stop_agent_workspace_polling(&conversation_id);
}

#[tokio::test]
async fn skips_blocked_pr_supervision_recovery_when_worktree_is_dirty() {
    let (_temp_dir, project, workspace, _head_sha) =
        setup_recovery_workspace("pr-supervision-dirty").await;
    let conversation_id = workspace.conversation_id.clone();
    std::fs::write(
        Path::new(&workspace.worktree_path).join("dirty.txt"),
        "uncommitted\n",
    )
    .expect("write dirty file");
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("seed workspace");
    let project_repo = Arc::new(MemoryProjectRepository::with_projects(vec![project]));
    let github = Arc::new(MockGithubService::new());

    let outcome = recover_agent_workspace_pr_supervision(
        AgentWorkspacePrSupervisionRecoveryDeps {
            workspace_repo: Arc::clone(&workspace_repo)
                as Arc<dyn AgentConversationWorkspaceRepository>,
            project_repo,
            github: Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
            pr_poller_registry: None,
            chat_service: None,
            agent_run_repo: Arc::new(MemoryAgentRunRepository::new()),
            app_handle: None,
        },
        conversation_id.clone(),
        AgentWorkspacePrSupervisionRecoveryTrigger::WorkspaceLoad,
    )
    .await
    .expect("skip dirty recovery");

    assert_eq!(
        outcome,
        AgentWorkspacePrSupervisionRecoveryOutcome::Skipped("worktree_dirty")
    );
    assert_eq!(github.state().check_pr_sync_state_calls, 0);
    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .expect("workspace should still exist");
    assert_eq!(updated.publication_push_status.as_deref(), Some("failed"));
    assert_eq!(updated.pr_supervision_status.as_deref(), Some("blocked"));
}
