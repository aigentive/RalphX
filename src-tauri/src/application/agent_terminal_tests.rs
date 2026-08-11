use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};

use super::agent_terminal::{
    build_terminal_command_for_test, terminal_env_path_from_parts_for_test, AgentTerminalEventSink,
    AgentTerminalOpenRequest, AgentTerminalProcess, AgentTerminalProcessFactory,
    AgentTerminalService, AgentTerminalWorkspaceDeps, PtySpawnRequest,
    TERMINAL_CLOSED_WORKSPACE_REASON, TERMINAL_MERGED_WORKSPACE_REASON,
};
use crate::application::agent_conversation_workspace::{
    resolve_agent_conversation_workspace_path, resolve_linked_plan_branch_agent_worktree_path,
};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, ArtifactId, ChatConversation,
    ChatConversationId, IdeationAnalysisBaseRefKind, IdeationSessionId, PlanBranch, Project,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, ChatConversationRepository, PlanBranchRepository,
    ProjectRepository,
};
use crate::error::{AppError, AppResult};
use crate::infrastructure::memory::{
    MemoryAgentConversationWorkspaceRepository, MemoryChatConversationRepository,
    MemoryPlanBranchRepository, MemoryProjectRepository,
};

#[derive(Default)]
struct CountingTerminalProcessFactory {
    spawn_count: AtomicUsize,
}

impl CountingTerminalProcessFactory {
    fn spawn_count(&self) -> usize {
        self.spawn_count.load(Ordering::SeqCst)
    }
}

impl AgentTerminalProcessFactory for CountingTerminalProcessFactory {
    fn spawn(
        &self,
        _request: PtySpawnRequest,
        _sink: AgentTerminalEventSink,
    ) -> AppResult<Arc<dyn AgentTerminalProcess>> {
        self.spawn_count.fetch_add(1, Ordering::SeqCst);
        Err(AppError::Infrastructure(
            "unexpected terminal process spawn".to_string(),
        ))
    }
}

struct TerminalPublishedTestContext {
    service: AgentTerminalService,
    process_factory: Arc<CountingTerminalProcessFactory>,
    conversation_id: ChatConversationId,
    chat_conversation_repo: Arc<dyn ChatConversationRepository>,
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    plan_branch_repo: Arc<dyn PlanBranchRepository>,
    project_repo: Arc<dyn ProjectRepository>,
}

impl TerminalPublishedTestContext {
    fn deps(&self) -> AgentTerminalWorkspaceDeps<'_> {
        AgentTerminalWorkspaceDeps {
            chat_conversation_repo: &self.chat_conversation_repo,
            workspace_repo: &self.workspace_repo,
            plan_branch_repo: &self.plan_branch_repo,
            project_repo: &self.project_repo,
        }
    }
}

#[derive(Default)]
struct RecordingTerminalProcessFactory {
    spawn_count: AtomicUsize,
    last_cwd: Mutex<Option<PathBuf>>,
}

impl RecordingTerminalProcessFactory {
    fn spawn_count(&self) -> usize {
        self.spawn_count.load(Ordering::SeqCst)
    }

    fn last_cwd(&self) -> Option<PathBuf> {
        self.last_cwd.lock().expect("cwd lock").clone()
    }
}

impl AgentTerminalProcessFactory for RecordingTerminalProcessFactory {
    fn spawn(
        &self,
        request: PtySpawnRequest,
        _sink: AgentTerminalEventSink,
    ) -> AppResult<Arc<dyn AgentTerminalProcess>> {
        self.spawn_count.fetch_add(1, Ordering::SeqCst);
        *self.last_cwd.lock().expect("cwd lock") = Some(request.cwd);
        Ok(Arc::new(NoopTerminalProcess))
    }
}

struct NoopTerminalProcess;

impl AgentTerminalProcess for NoopTerminalProcess {
    fn pid(&self) -> Option<u32> {
        Some(4242)
    }

    fn write(&self, _data: &[u8]) -> AppResult<()> {
        Ok(())
    }

    fn resize(&self, _cols: u16, _rows: u16) -> AppResult<()> {
        Ok(())
    }

    fn kill(&self) -> AppResult<()> {
        Ok(())
    }
}

async fn setup_terminal_published_context(status: &str) -> TerminalPublishedTestContext {
    let project = Project::new("RalphX".to_string(), "/tmp/ralphx".to_string());
    let project_id = project.id.clone();
    let conversation = ChatConversation::new_project(project_id.clone());
    let conversation_id = conversation.id;
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id,
        project_id,
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        None,
        "ralphx/ralphx/agent-conversation-1".to_string(),
        "/tmp/ralphx-worktrees/agent-conversation-1".to_string(),
    );
    workspace.publication_pr_number = Some(42);
    workspace.publication_pr_status = Some(status.to_string());

    let chat_conversation_repo: Arc<dyn ChatConversationRepository> =
        Arc::new(MemoryChatConversationRepository::new());
    chat_conversation_repo
        .create(conversation)
        .await
        .expect("seed conversation");
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("seed workspace");
    let plan_branch_repo: Arc<dyn PlanBranchRepository> =
        Arc::new(MemoryPlanBranchRepository::new());
    let project_repo: Arc<dyn ProjectRepository> =
        Arc::new(MemoryProjectRepository::with_projects(vec![project]));
    let process_factory = Arc::new(CountingTerminalProcessFactory::default());
    let service = AgentTerminalService::with_process_factory(process_factory.clone());

    TerminalPublishedTestContext {
        service,
        process_factory,
        conversation_id,
        chat_conversation_repo,
        workspace_repo,
        plan_branch_repo,
        project_repo,
    }
}

fn terminal_open_request(conversation_id: ChatConversationId) -> AgentTerminalOpenRequest {
    AgentTerminalOpenRequest {
        conversation_id,
        terminal_id: "default".to_string(),
        cols: 80,
        rows: 24,
    }
}

fn assert_validation_reason(error: AppError, expected: &str) {
    match error {
        AppError::Validation(message) => assert_eq!(message, expected),
        other => panic!("expected validation error, got {other:?}"),
    }
}

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git command should spawn");
    assert!(
        output.status.success(),
        "git {:?} failed: {}{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn setup_repo(root: &Path) {
    std::fs::create_dir_all(root).expect("repo root should be created");
    git(root, &["init", "-b", "main"]);
    git(root, &["config", "user.email", "test@example.com"]);
    git(root, &["config", "user.name", "Test User"]);
    std::fs::write(root.join("README.md"), "hello\n").expect("fixture file should be written");
    git(root, &["add", "README.md"]);
    git(root, &["commit", "-m", "initial"]);
}

#[test]
fn terminal_command_sets_real_pty_environment() {
    let command = build_terminal_command_for_test(std::path::Path::new("/tmp/project"));

    assert_eq!(command.get_env("TERM"), Some(OsStr::new("xterm-256color")));
    assert_eq!(command.get_env("COLORTERM"), Some(OsStr::new("truecolor")));
    assert_eq!(command.get_env("PWD"), Some(OsStr::new("/tmp/project")));
    for key in [
        "GH_TOKEN",
        "GITHUB_TOKEN",
        "GH_ENTERPRISE_TOKEN",
        "GITHUB_ENTERPRISE_TOKEN",
    ] {
        assert_eq!(command.get_env(key), None, "{key} must not reach the PTY");
    }
}

#[cfg(target_os = "macos")]
#[test]
fn terminal_command_launches_zsh_as_login_shell() {
    let command = build_terminal_command_for_test(std::path::Path::new("/tmp/project"));
    let argv = command.get_argv();

    assert_eq!(
        argv.first().and_then(|value| value.to_str()),
        Some("/bin/zsh")
    );
    assert_eq!(argv.get(1).and_then(|value| value.to_str()), Some("-l"));
}

#[test]
fn terminal_path_preserves_existing_path_and_adds_common_dev_bins() {
    let path = terminal_env_path_from_parts_for_test(
        Some(OsStr::new("/existing/bin:/usr/bin")),
        Some(std::path::Path::new("/Users/example")),
    );
    let path = path.to_string_lossy();

    assert!(path.contains("/existing/bin"));
    assert!(path.contains("/opt/homebrew/bin"));
    assert!(path.contains("/Users/example/.rbenv/bin"));
    assert!(path.contains("/Users/example/.asdf/shims"));
}

#[tokio::test]
async fn terminal_opens_linked_plan_branch_workspace() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_repo(&repo_path);
    let branch_name = "feature/terminal-linked-plan";
    git(&repo_path, &["checkout", "-b", branch_name]);
    git(&repo_path, &["checkout", "main"]);

    let mut project = Project::new(
        "Terminal Linked Plan".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
    let project_id = project.id.clone();
    let conversation = ChatConversation::new_project(project_id.clone());
    let conversation_id = conversation.id.clone();
    let session_id = IdeationSessionId::from_string("session-terminal-linked-plan");
    let mut plan_branch = PlanBranch::new(
        ArtifactId::from_string("artifact-terminal-linked-plan"),
        session_id.clone(),
        project_id.clone(),
        branch_name.to_string(),
        "main".to_string(),
    );
    plan_branch.pr_eligible = true;
    let expected_worktree = resolve_linked_plan_branch_agent_worktree_path(&project, &plan_branch)
        .expect("linked plan branch path should resolve");
    let stale_direct_path = resolve_agent_conversation_workspace_path(&project, &conversation_id)
        .expect("direct path should resolve");
    assert!(!stale_direct_path.exists());

    let mut workspace = AgentConversationWorkspace::new(
        conversation_id.clone(),
        project_id,
        AgentConversationWorkspaceMode::Ideation,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        None,
        branch_name.to_string(),
        stale_direct_path.to_string_lossy().to_string(),
    );
    workspace.linked_ideation_session_id = Some(session_id);
    workspace.linked_plan_branch_id = Some(plan_branch.id.clone());

    let chat_conversation_repo: Arc<dyn ChatConversationRepository> =
        Arc::new(MemoryChatConversationRepository::new());
    chat_conversation_repo
        .create(conversation)
        .await
        .expect("seed conversation");
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("seed workspace");
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    plan_branch_repo
        .create(plan_branch)
        .await
        .expect("seed plan branch");
    let plan_branch_repo: Arc<dyn PlanBranchRepository> = plan_branch_repo;
    let project_repo: Arc<dyn ProjectRepository> =
        Arc::new(MemoryProjectRepository::with_projects(vec![project]));
    let process_factory = Arc::new(RecordingTerminalProcessFactory::default());
    let service = AgentTerminalService::with_process_factory(process_factory.clone());

    let snapshot = service
        .open(
            terminal_open_request(conversation_id),
            AgentTerminalWorkspaceDeps {
                chat_conversation_repo: &chat_conversation_repo,
                workspace_repo: &workspace_repo,
                plan_branch_repo: &plan_branch_repo,
                project_repo: &project_repo,
            },
            None,
        )
        .await
        .expect("linked plan terminal should open");

    assert_eq!(process_factory.spawn_count(), 1);
    assert_eq!(process_factory.last_cwd(), Some(expected_worktree.clone()));
    assert_eq!(PathBuf::from(snapshot.cwd), expected_worktree);
    assert_eq!(snapshot.workspace_branch, branch_name);
}

#[tokio::test]
async fn terminal_published_workspaces_reject_open_and_restart_before_spawn() {
    for (status, expected_reason) in [
        ("merged", TERMINAL_MERGED_WORKSPACE_REASON),
        ("closed", TERMINAL_CLOSED_WORKSPACE_REASON),
    ] {
        let context = setup_terminal_published_context(status).await;

        let open_error = context
            .service
            .open(
                terminal_open_request(context.conversation_id),
                context.deps(),
                None,
            )
            .await
            .expect_err("terminal open should reject terminal-published workspace");
        assert_validation_reason(open_error, expected_reason);

        let restart_error = context
            .service
            .restart(
                terminal_open_request(context.conversation_id),
                context.deps(),
                None,
            )
            .await
            .expect_err("terminal restart should reject terminal-published workspace");
        assert_validation_reason(restart_error, expected_reason);
        assert_eq!(context.process_factory.spawn_count(), 0);
    }
}
