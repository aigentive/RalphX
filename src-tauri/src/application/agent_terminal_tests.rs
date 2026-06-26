use std::ffi::OsStr;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use super::agent_terminal::{
    build_terminal_command_for_test, terminal_env_path_from_parts_for_test, AgentTerminalEventSink,
    AgentTerminalOpenRequest, AgentTerminalProcess, AgentTerminalProcessFactory,
    AgentTerminalService, AgentTerminalWorkspaceDeps, PtySpawnRequest,
    TERMINAL_CLOSED_WORKSPACE_REASON, TERMINAL_MERGED_WORKSPACE_REASON,
};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, ChatConversation,
    ChatConversationId, IdeationAnalysisBaseRefKind, Project,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, ChatConversationRepository, ProjectRepository,
};
use crate::error::{AppError, AppResult};
use crate::infrastructure::memory::{
    MemoryAgentConversationWorkspaceRepository, MemoryChatConversationRepository,
    MemoryProjectRepository,
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
    project_repo: Arc<dyn ProjectRepository>,
}

impl TerminalPublishedTestContext {
    fn deps(&self) -> AgentTerminalWorkspaceDeps<'_> {
        AgentTerminalWorkspaceDeps {
            chat_conversation_repo: &self.chat_conversation_repo,
            workspace_repo: &self.workspace_repo,
            project_repo: &self.project_repo,
        }
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

#[test]
fn terminal_command_sets_real_pty_environment() {
    let command = build_terminal_command_for_test(std::path::Path::new("/tmp/project"));

    assert_eq!(command.get_env("TERM"), Some(OsStr::new("xterm-256color")));
    assert_eq!(command.get_env("COLORTERM"), Some(OsStr::new("truecolor")));
    assert_eq!(command.get_env("PWD"), Some(OsStr::new("/tmp/project")));
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
