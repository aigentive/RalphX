use super::*;
use crate::application::agent_conversation_workspace::resolve_agent_conversation_workspace_path;
use crate::application::AppState;
use crate::domain::agents::{AgentHarnessKind, ProviderSessionRef};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentRun, AgentRunAttribution,
    AgentRunId, AgentRunStatus, AgentRunUsage, ChatConversation, ChatConversationId,
    IdeationAnalysisBaseRefKind, InternalStatus, Project, Task,
};
use crate::domain::repositories::AgentRunRepository;
use crate::domain::services::{QueuedMessage, RunningAgentKey};
use crate::error::AppResult;

/// Helper to create test state
async fn setup_test_state() -> (Arc<ExecutionState>, AppState) {
    let execution_state = Arc::new(ExecutionState::new());
    let app_state = AppState::new_test();
    (execution_state, app_state)
}

async fn spawn_test_stdin() -> (tokio::process::ChildStdin, tokio::process::Child) {
    let mut child = tokio::process::Command::new("cat")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("failed to spawn cat");
    let stdin = child.stdin.take().expect("no stdin");
    (stdin, child)
}

/// Helper to build a ChatResumptionRunner from test state
fn build_runner(
    app_state: &AppState,
    execution_state: &Arc<ExecutionState>,
) -> ChatResumptionRunner<tauri::Wry> {
    build_runner_with_agent_run_repo(
        app_state,
        execution_state,
        Arc::clone(&app_state.agent_run_repo),
    )
}

fn build_runner_with_agent_run_repo(
    app_state: &AppState,
    execution_state: &Arc<ExecutionState>,
    agent_run_repo: Arc<dyn AgentRunRepository>,
) -> ChatResumptionRunner<tauri::Wry> {
    ChatResumptionRunner::new(
        agent_run_repo,
        Arc::clone(&app_state.task_repo),
        Arc::clone(execution_state),
        crate::application::runtime_factory::ChatRuntimeFactoryDeps::from_core(
            Arc::clone(&app_state.chat_message_repo),
            Arc::clone(&app_state.chat_attachment_repo),
            Arc::clone(&app_state.artifact_repo),
            Arc::clone(&app_state.chat_conversation_repo),
            Arc::clone(&app_state.agent_run_repo),
            Arc::clone(&app_state.project_repo),
            Arc::clone(&app_state.task_repo),
            Arc::clone(&app_state.task_dependency_repo),
            Arc::clone(&app_state.ideation_session_repo),
            Arc::clone(&app_state.activity_event_repo),
            Arc::clone(&app_state.message_queue),
            Arc::clone(&app_state.running_agent_registry),
            Arc::clone(&app_state.memory_event_repo),
        )
        .with_agent_conversation_workspace_repo(Some(Arc::clone(
            &app_state.agent_conversation_workspace_repo,
        ))),
    )
}

#[tokio::test]
async fn chat_resumption_runner_builder_attaches_runtime_support_repos() {
    let (execution_state, app_state) = setup_test_state().await;

    let runner = build_runner(&app_state, &execution_state)
        .with_plan_branch_repo(Arc::clone(&app_state.plan_branch_repo))
        .with_execution_settings_repo(Arc::clone(&app_state.execution_settings_repo))
        .with_agent_lane_settings_repo(Arc::clone(&app_state.agent_lane_settings_repo))
        .with_agent_provider_settings_repo(Arc::clone(&app_state.agent_provider_settings_repo))
        .with_interactive_process_registry(Arc::clone(&app_state.interactive_process_registry));

    let _chat_service = runner.create_chat_service();
}

struct InterruptedAgentRunRepo {
    runs: Vec<AgentRun>,
    interrupted: Vec<InterruptedConversation>,
}

impl InterruptedAgentRunRepo {
    fn new(runs: Vec<AgentRun>, interrupted: Vec<InterruptedConversation>) -> Self {
        Self { runs, interrupted }
    }
}

#[async_trait::async_trait]
impl AgentRunRepository for InterruptedAgentRunRepo {
    async fn create(&self, run: AgentRun) -> AppResult<AgentRun> {
        Ok(run)
    }

    async fn get_by_id(&self, id: &AgentRunId) -> AppResult<Option<AgentRun>> {
        Ok(self.runs.iter().find(|run| run.id == *id).cloned())
    }

    async fn get_latest_for_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentRun>> {
        Ok(self
            .runs
            .iter()
            .filter(|run| run.conversation_id == *conversation_id)
            .max_by_key(|run| run.started_at)
            .cloned())
    }

    async fn get_active_for_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentRun>> {
        Ok(self
            .runs
            .iter()
            .find(|run| run.conversation_id == *conversation_id && run.is_active())
            .cloned())
    }

    async fn get_by_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Vec<AgentRun>> {
        Ok(self
            .runs
            .iter()
            .filter(|run| run.conversation_id == *conversation_id)
            .cloned()
            .collect())
    }

    async fn update_status(&self, _id: &AgentRunId, _status: AgentRunStatus) -> AppResult<()> {
        Ok(())
    }

    async fn update_usage(&self, _id: &AgentRunId, _usage: &AgentRunUsage) -> AppResult<()> {
        Ok(())
    }

    async fn update_attribution(
        &self,
        _id: &AgentRunId,
        _attribution: &AgentRunAttribution,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn complete(&self, _id: &AgentRunId) -> AppResult<()> {
        Ok(())
    }

    async fn fail(&self, _id: &AgentRunId, _error_message: &str) -> AppResult<()> {
        Ok(())
    }

    async fn cancel(&self, _id: &AgentRunId) -> AppResult<()> {
        Ok(())
    }

    async fn delete(&self, _id: &AgentRunId) -> AppResult<()> {
        Ok(())
    }

    async fn delete_by_conversation(&self, _conversation_id: &ChatConversationId) -> AppResult<()> {
        Ok(())
    }

    async fn count_by_status(
        &self,
        conversation_id: &ChatConversationId,
        status: AgentRunStatus,
    ) -> AppResult<u32> {
        Ok(self
            .runs
            .iter()
            .filter(|run| run.conversation_id == *conversation_id && run.status == status)
            .count() as u32)
    }

    async fn cancel_all_running(&self) -> AppResult<u32> {
        Ok(0)
    }

    async fn cancel_running_started_before(
        &self,
        _cutoff: chrono::DateTime<chrono::Utc>,
    ) -> AppResult<u32> {
        Ok(0)
    }

    async fn get_interrupted_conversations(&self) -> AppResult<Vec<InterruptedConversation>> {
        Ok(self
            .interrupted
            .iter()
            .map(|conversation| InterruptedConversation {
                conversation: conversation.conversation.clone(),
                last_run: conversation.last_run.clone(),
            })
            .collect())
    }
}

#[test]
fn test_context_type_priority_ordering() {
    // TaskExecution should have highest priority (lowest number)
    assert!(
        context_type_priority(ChatContextType::TaskExecution)
            < context_type_priority(ChatContextType::Review)
    );
    assert!(
        context_type_priority(ChatContextType::Review)
            < context_type_priority(ChatContextType::Merge)
    );
    assert!(
        context_type_priority(ChatContextType::Merge)
            < context_type_priority(ChatContextType::Task)
    );
    assert!(
        context_type_priority(ChatContextType::Task)
            < context_type_priority(ChatContextType::Ideation)
    );
    assert!(
        context_type_priority(ChatContextType::Ideation)
            < context_type_priority(ChatContextType::Delegation)
    );
    assert!(
        context_type_priority(ChatContextType::Delegation)
            < context_type_priority(ChatContextType::Project)
    );
}

#[test]
fn test_prioritize_resumptions_sorts_correctly() {
    // Create test conversations with different context types
    let create_interrupted = |context_type: ChatContextType| -> InterruptedConversation {
        let mut conv =
            ChatConversation::new_ideation(crate::domain::entities::IdeationSessionId::new());
        // Override context_type for testing (normally set by constructor)
        conv.context_type = context_type;
        conv.context_id = "test-id".to_string();
        conv.claude_session_id = Some("test-session".to_string());

        let run = AgentRun::new(conv.id);

        InterruptedConversation {
            conversation: conv,
            last_run: run,
        }
    };

    let conversations = vec![
        create_interrupted(ChatContextType::Project), // Lowest priority
        create_interrupted(ChatContextType::TaskExecution), // Highest priority
        create_interrupted(ChatContextType::Merge),
        create_interrupted(ChatContextType::Delegation),
        create_interrupted(ChatContextType::Ideation),
        create_interrupted(ChatContextType::Review),
        create_interrupted(ChatContextType::Task),
    ];

    // Use a temporary runner just for the sort function
    let sorted = {
        let mut convs = conversations;
        convs.sort_by_key(|conv| context_type_priority(conv.conversation.context_type));
        convs
    };

    // Verify order: TaskExecution, Review, Task, Ideation, Project
    assert_eq!(
        sorted[0].conversation.context_type,
        ChatContextType::TaskExecution
    );
    assert_eq!(sorted[1].conversation.context_type, ChatContextType::Review);
    assert_eq!(sorted[2].conversation.context_type, ChatContextType::Merge);
    assert_eq!(sorted[3].conversation.context_type, ChatContextType::Task);
    assert_eq!(
        sorted[4].conversation.context_type,
        ChatContextType::Ideation
    );
    assert_eq!(
        sorted[5].conversation.context_type,
        ChatContextType::Delegation
    );
    assert_eq!(
        sorted[6].conversation.context_type,
        ChatContextType::Project
    );
}

#[test]
fn startup_resumption_send_options_preserves_interrupted_agent_conversation_for_all_modes() {
    let project_id =
        crate::domain::entities::ProjectId::from_string("project-resume-options".to_string());
    for mode in [
        AgentConversationWorkspaceMode::Chat,
        AgentConversationWorkspaceMode::Edit,
        AgentConversationWorkspaceMode::Plan,
        AgentConversationWorkspaceMode::Ideation,
        AgentConversationWorkspaceMode::ReviewPr,
    ] {
        let mut conversation = ChatConversation::new_project(project_id.clone());
        conversation.set_agent_mode(Some(mode));

        let options = startup_resumption_send_options(&conversation);

        assert_eq!(
            options.conversation_id_override,
            Some(conversation.id),
            "startup resumption must resume the exact interrupted conversation for {mode:?} mode so agent_mode/workspace linkage are preserved"
        );
    }
}

fn temp_project(temp: &tempfile::TempDir, name: &str) -> Project {
    let project_root = temp.path().join("project-root");
    std::fs::create_dir_all(&project_root).expect("project root should be created");
    let mut project = Project::new(name.to_string(), project_root.to_string_lossy().to_string());
    project.worktree_parent_directory =
        Some(temp.path().join("worktrees").to_string_lossy().to_string());
    project
}

fn workspace_for_conversation(
    project: &Project,
    conversation_id: ChatConversationId,
    mode: AgentConversationWorkspaceMode,
    publication_pr_status: Option<&str>,
) -> AgentConversationWorkspace {
    let expected_path = resolve_agent_conversation_workspace_path(project, &conversation_id)
        .expect("expected workspace path should resolve");
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id,
        project.id.clone(),
        mode,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        Some("base-sha".to_string()),
        "ralphx/cleaned/agent-workspace".to_string(),
        expected_path.to_string_lossy().to_string(),
    );
    workspace.publication_pr_status = publication_pr_status.map(str::to_string);
    workspace
}

#[tokio::test]
async fn blocked_agent_workspace_resume_reason_blocks_cleaned_terminal_workspace() {
    let (execution_state, app_state) = setup_test_state().await;
    let temp = tempfile::tempdir().expect("temp dir should be created");
    let project = temp_project(&temp, "Cleaned Agent Workspace");
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.set_agent_mode(Some(AgentConversationWorkspaceMode::Plan));
    let workspace = workspace_for_conversation(
        &project,
        conversation.id,
        AgentConversationWorkspaceMode::Plan,
        Some("merged"),
    );
    app_state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();

    let runner = build_runner(&app_state, &execution_state);
    let reason = runner
        .blocked_agent_workspace_resume_reason(&conversation)
        .await;

    assert_eq!(
        reason,
        Some(AgentWorkspaceContinuationBlock::CleanedAfterTerminal)
    );
}

#[tokio::test]
async fn blocked_agent_workspace_resume_reason_allows_non_project_and_unlinked_project() {
    let (execution_state, app_state) = setup_test_state().await;
    let runner = build_runner(&app_state, &execution_state);
    let ideation =
        ChatConversation::new_ideation(crate::domain::entities::IdeationSessionId::new());
    let project = ChatConversation::new_project(crate::domain::entities::ProjectId::new());

    assert_eq!(
        runner
            .blocked_agent_workspace_resume_reason(&ideation)
            .await,
        None
    );
    assert_eq!(
        runner.blocked_agent_workspace_resume_reason(&project).await,
        None
    );
}

#[tokio::test]
async fn blocked_agent_workspace_resume_reason_requires_manual_check_when_project_is_missing() {
    let (execution_state, app_state) = setup_test_state().await;
    let temp = tempfile::tempdir().expect("temp dir should be created");
    let project = temp_project(&temp, "Missing Project Agent Workspace");
    let conversation = ChatConversation::new_project(project.id.clone());
    let workspace = workspace_for_conversation(
        &project,
        conversation.id,
        AgentConversationWorkspaceMode::Edit,
        Some("merged"),
    );
    app_state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();

    let runner = build_runner(&app_state, &execution_state);
    let reason = runner
        .blocked_agent_workspace_resume_reason(&conversation)
        .await;

    assert_eq!(
        reason,
        Some(AgentWorkspaceContinuationBlock::UnknownRequiresManualCheck(
            "project not found".to_string()
        ))
    );
}

#[tokio::test]
async fn run_skips_interrupted_non_resumable_agent_workspace() {
    let (execution_state, app_state) = setup_test_state().await;
    let temp = tempfile::tempdir().expect("temp dir should be created");
    let project = temp_project(&temp, "Interrupted Cleaned Agent Workspace");
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.set_agent_mode(Some(AgentConversationWorkspaceMode::Edit));
    conversation.claude_session_id = Some("provider-session".to_string());
    let workspace = workspace_for_conversation(
        &project,
        conversation.id,
        AgentConversationWorkspaceMode::Edit,
        Some("merged"),
    );
    app_state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();
    let run = AgentRun::new(conversation.id);
    let agent_run_repo: Arc<dyn AgentRunRepository> = Arc::new(InterruptedAgentRunRepo::new(
        vec![run.clone()],
        vec![InterruptedConversation {
            conversation: conversation.clone(),
            last_run: run,
        }],
    ));
    let runner = build_runner_with_agent_run_repo(&app_state, &execution_state, agent_run_repo);

    runner.run().await;

    assert!(app_state
        .message_queue
        .get_queued(ChatContextType::Project, &conversation.id.as_str())
        .is_empty());
}

#[tokio::test]
async fn durable_silent_completion_recovery_skips_non_resumable_agent_workspace() {
    let (execution_state, app_state) = setup_test_state().await;
    let temp = tempfile::tempdir().expect("temp dir should be created");
    let project = temp_project(&temp, "Durable Cleaned Agent Workspace");
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.set_provider_session_ref(ProviderSessionRef {
        harness: AgentHarnessKind::Codex,
        provider_session_id: "codex-session-1".to_string(),
    });
    let conversation_id = conversation.id;
    app_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();

    let mut run = AgentRun::new(conversation_id);
    run.complete();
    app_state.agent_run_repo.create(run).await.unwrap();

    let mut message = silent_tool_message();
    message.conversation_id = Some(conversation_id);
    app_state.chat_message_repo.create(message).await.unwrap();

    let workspace = workspace_for_conversation(
        &project,
        conversation_id,
        AgentConversationWorkspaceMode::Edit,
        Some("merged"),
    );
    app_state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();

    let runner = build_runner(&app_state, &execution_state);

    assert_eq!(runner.recover_durable_silent_completions().await, 0);
    assert!(app_state
        .message_queue
        .get_queued(ChatContextType::Project, &conversation_id.as_str())
        .is_empty());
}

#[tokio::test]
async fn test_resumption_skipped_when_paused() {
    let (execution_state, app_state) = setup_test_state().await;

    // Pause execution
    execution_state.pause();

    let runner = build_runner(&app_state, &execution_state);

    // Run should skip because paused - just verify it doesn't panic
    runner.run().await;

    // Verify no conversations were created (nothing resumed)
    // The mock repo returns empty for get_interrupted_conversations, so this is a no-op
}

#[tokio::test]
async fn test_resumption_run_skips_interrupted_conversations_owned_by_other_recovery_paths() {
    let (execution_state, app_state) = setup_test_state().await;

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();
    let mut task = Task::new(project.id.clone(), "Executing Task".to_string());
    task.internal_status = InternalStatus::Executing;
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    let mut task_conversation = ChatConversation::new_task_execution(task_id);
    task_conversation.claude_session_id = Some("task-session".to_string());
    let task_run = AgentRun::new(task_conversation.id);

    let session_id = crate::domain::entities::IdeationSessionId::new();
    let mut ideation_conversation = ChatConversation::new_ideation(session_id);
    ideation_conversation.claude_session_id = Some("ideation-session".to_string());
    let ideation_run = AgentRun::new(ideation_conversation.id);

    let agent_run_repo: Arc<dyn AgentRunRepository> = Arc::new(InterruptedAgentRunRepo::new(
        vec![task_run.clone(), ideation_run.clone()],
        vec![
            InterruptedConversation {
                conversation: ideation_conversation,
                last_run: ideation_run,
            },
            InterruptedConversation {
                conversation: task_conversation,
                last_run: task_run,
            },
        ],
    ));
    let runner = build_runner_with_agent_run_repo(&app_state, &execution_state, agent_run_repo);

    runner.run().await;
}

#[tokio::test]
async fn test_is_handled_by_task_resumption_for_agent_active_task() {
    let (execution_state, app_state) = setup_test_state().await;

    // Create a project and task in Executing state
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Executing Task".to_string());
    task.internal_status = InternalStatus::Executing;
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    // Create an interrupted conversation for TaskExecution
    let mut conv = ChatConversation::new_task_execution(task_id.clone());
    conv.claude_session_id = Some("test-session".to_string());

    let run = AgentRun::new(conv.id);

    let interrupted = InterruptedConversation {
        conversation: conv,
        last_run: run,
    };

    let runner = build_runner(&app_state, &execution_state);

    // Should be handled by task resumption (task is in Executing status)
    let is_handled = runner.is_handled_by_task_resumption(&interrupted).await;
    assert!(
        is_handled,
        "TaskExecution with Executing task should be handled by StartupJobRunner"
    );
}

#[tokio::test]
async fn test_is_handled_by_task_resumption_for_non_agent_active_task() {
    let (execution_state, app_state) = setup_test_state().await;

    // Create a project and task in Ready state (NOT agent-active)
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Ready Task".to_string());
    task.internal_status = InternalStatus::Ready;
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    // Create an interrupted conversation for TaskExecution
    let mut conv = ChatConversation::new_task_execution(task_id.clone());
    conv.claude_session_id = Some("test-session".to_string());

    let run = AgentRun::new(conv.id);

    let interrupted = InterruptedConversation {
        conversation: conv,
        last_run: run,
    };

    let runner = build_runner(&app_state, &execution_state);

    // Should NOT be handled by task resumption (task is in Ready status)
    let is_handled = runner.is_handled_by_task_resumption(&interrupted).await;
    assert!(
        !is_handled,
        "TaskExecution with Ready task should NOT be handled by StartupJobRunner"
    );
}

#[tokio::test]
async fn test_is_handled_by_task_resumption_for_ideation() {
    let (execution_state, app_state) = setup_test_state().await;

    // Create an interrupted conversation for Ideation
    let session_id = crate::domain::entities::IdeationSessionId::new();
    let mut conv = ChatConversation::new_ideation(session_id);
    conv.claude_session_id = Some("test-session".to_string());

    let run = AgentRun::new(conv.id);

    let interrupted = InterruptedConversation {
        conversation: conv,
        last_run: run,
    };

    let runner = build_runner(&app_state, &execution_state);

    // Ideation IS handled by the dedicated recovery loop (Phase N+1 in StartupJobRunner).
    // ChatResumptionRunner must unconditionally skip ideation to prevent double-spawn.
    let is_handled = runner.is_handled_by_task_resumption(&interrupted).await;
    assert!(
        is_handled,
        "Ideation should be handled by dedicated recovery loop, not ChatResumptionRunner"
    );
}

#[tokio::test]
async fn test_is_handled_by_task_resumption_for_missing_merge_task() {
    let (execution_state, app_state) = setup_test_state().await;
    let missing_task_id = TaskId::new();

    let mut conv = ChatConversation::new_task_execution(missing_task_id.clone());
    conv.context_type = ChatContextType::Merge;
    conv.context_id = missing_task_id.as_str().to_string();
    conv.claude_session_id = Some("test-session".to_string());

    let run = AgentRun::new(conv.id);
    let interrupted = InterruptedConversation {
        conversation: conv,
        last_run: run,
    };

    let runner = build_runner(&app_state, &execution_state);

    let is_handled = runner.is_handled_by_task_resumption(&interrupted).await;
    assert!(
        is_handled,
        "Merge conversation without a task should be skipped by chat resumption"
    );
}

#[tokio::test]
async fn test_is_handled_by_task_resumption_for_delegation() {
    let (execution_state, app_state) = setup_test_state().await;
    let project_id = crate::domain::entities::ProjectId::new();

    let mut conv = ChatConversation::new_project(project_id);
    conv.context_type = ChatContextType::Delegation;
    conv.context_id = "delegated-session-1".to_string();
    conv.claude_session_id = Some("test-session".to_string());

    let run = AgentRun::new(conv.id);
    let interrupted = InterruptedConversation {
        conversation: conv,
        last_run: run,
    };

    let runner = build_runner(&app_state, &execution_state);

    let is_handled = runner.is_handled_by_task_resumption(&interrupted).await;
    assert!(
        !is_handled,
        "Delegation conversations are not owned by task resumption"
    );
}

#[tokio::test]
async fn test_is_handled_by_task_resumption_for_project() {
    let (execution_state, app_state) = setup_test_state().await;

    // Create an interrupted conversation for Project
    let project_id = crate::domain::entities::ProjectId::new();
    let mut conv = ChatConversation::new_project(project_id);
    conv.claude_session_id = Some("test-session".to_string());

    let run = AgentRun::new(conv.id);

    let interrupted = InterruptedConversation {
        conversation: conv,
        last_run: run,
    };

    let runner = build_runner(&app_state, &execution_state);

    // Project should NOT be handled by task resumption
    let is_handled = runner.is_handled_by_task_resumption(&interrupted).await;
    assert!(
        !is_handled,
        "Project should NOT be handled by StartupJobRunner"
    );
}

async fn create_terminal_state_test(status: InternalStatus) -> bool {
    let (execution_state, app_state) = setup_test_state().await;

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), format!("{:?} Task", status));
    task.internal_status = status;
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    let mut conv = ChatConversation::new_task_execution(task_id);
    conv.claude_session_id = Some("test-session".to_string());

    let run = AgentRun::new(conv.id);

    let interrupted = InterruptedConversation {
        conversation: conv,
        last_run: run,
    };

    let runner = build_runner(&app_state, &execution_state);
    runner.is_handled_by_task_resumption(&interrupted).await
}

#[tokio::test]
async fn test_is_handled_for_merged_task() {
    let is_handled = create_terminal_state_test(InternalStatus::Merged).await;
    assert!(is_handled, "Merged task should be skipped (terminal state)");
}

#[tokio::test]
async fn test_is_handled_for_failed_task() {
    let is_handled = create_terminal_state_test(InternalStatus::Failed).await;
    assert!(is_handled, "Failed task should be skipped (terminal state)");
}

#[tokio::test]
async fn test_is_handled_for_cancelled_task() {
    let is_handled = create_terminal_state_test(InternalStatus::Cancelled).await;
    assert!(
        is_handled,
        "Cancelled task should be skipped (terminal state)"
    );
}

#[tokio::test]
async fn test_is_handled_for_stopped_task() {
    let is_handled = create_terminal_state_test(InternalStatus::Stopped).await;
    assert!(
        is_handled,
        "Stopped task should be skipped (terminal state)"
    );
}

fn silent_tool_message() -> crate::domain::entities::ChatMessage {
    let project_id = crate::domain::entities::ProjectId::from_string("project-1".to_string());
    let mut message = crate::domain::entities::ChatMessage::user_in_project(project_id, "");
    message.role = crate::domain::entities::MessageRole::Orchestrator;
    message.tool_calls = Some(
        serde_json::json!([
            {
                "id": "tool-1",
                "name": "apply_patch",
                "arguments": {},
                "result": null
            }
        ])
        .to_string(),
    );
    message.content_blocks = Some(
        serde_json::json!([
            {
                "type": "tool_use",
                "id": "tool-1",
                "name": "apply_patch",
                "arguments": {},
                "result": null
            }
        ])
        .to_string(),
    );
    message
}

async fn create_durable_recovery_candidate(
    app_state: &AppState,
) -> crate::domain::entities::ChatConversationId {
    create_durable_recovery_candidate_with_status(app_state, AgentRunStatus::Completed).await
}

async fn create_durable_recovery_candidate_with_status(
    app_state: &AppState,
    run_status: AgentRunStatus,
) -> crate::domain::entities::ChatConversationId {
    let project_id = crate::domain::entities::ProjectId::from_string("project-1".to_string());
    let mut conversation = ChatConversation::new_project(project_id);
    conversation.set_provider_session_ref(ProviderSessionRef {
        harness: AgentHarnessKind::Codex,
        provider_session_id: "codex-session-1".to_string(),
    });
    let conversation_id = conversation.id;
    app_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();

    let mut run = AgentRun::new(conversation_id);
    if run_status == AgentRunStatus::Completed {
        run.complete();
    } else {
        run.status = run_status;
    }
    app_state.agent_run_repo.create(run).await.unwrap();

    let mut message = silent_tool_message();
    message.conversation_id = Some(conversation_id);
    app_state.chat_message_repo.create(message).await.unwrap();

    conversation_id
}

#[tokio::test]
async fn durable_silent_completion_run_scans_when_no_interrupted_conversations() {
    let (execution_state, app_state) = setup_test_state().await;
    let runner = build_runner(&app_state, &execution_state);

    runner.run().await;
}

#[tokio::test]
async fn durable_silent_completion_recovery_skips_active_runtime() {
    let (execution_state, app_state) = setup_test_state().await;
    let conversation_id = create_durable_recovery_candidate(&app_state).await;
    app_state
        .running_agent_registry
        .register(
            RunningAgentKey::new("project", conversation_id.as_str()),
            0,
            conversation_id.as_str(),
            "agent-run-1".to_string(),
            None,
            None,
        )
        .await;

    let runner = build_runner(&app_state, &execution_state);

    assert_eq!(runner.recover_durable_silent_completions().await, 0);
    assert!(app_state
        .message_queue
        .get_queued(ChatContextType::Project, &conversation_id.as_str())
        .is_empty());
}

#[tokio::test]
async fn durable_silent_completion_recovery_skips_interactive_registry_runtime() {
    let (execution_state, app_state) = setup_test_state().await;
    let conversation_id = create_durable_recovery_candidate(&app_state).await;
    let (stdin, _child) = spawn_test_stdin().await;
    let key = InteractiveProcessKey::new(
        ChatContextType::Project.to_string(),
        conversation_id.as_str(),
    );
    app_state
        .interactive_process_registry
        .register(key.clone(), stdin)
        .await;

    let runner = build_runner(&app_state, &execution_state)
        .with_interactive_process_registry(Arc::clone(&app_state.interactive_process_registry));

    assert_eq!(runner.recover_durable_silent_completions().await, 0);
    assert!(
        app_state
            .interactive_process_registry
            .has_process(&key)
            .await
    );
    assert!(app_state
        .message_queue
        .get_queued(ChatContextType::Project, &conversation_id.as_str())
        .is_empty());
}

#[tokio::test]
async fn durable_silent_completion_recovery_skips_non_completed_latest_run() {
    let (execution_state, app_state) = setup_test_state().await;
    let conversation_id =
        create_durable_recovery_candidate_with_status(&app_state, AgentRunStatus::Running).await;

    let runner = build_runner(&app_state, &execution_state);

    assert_eq!(runner.recover_durable_silent_completions().await, 0);
    assert!(app_state
        .message_queue
        .get_queued(ChatContextType::Project, &conversation_id.as_str())
        .is_empty());
}

#[tokio::test]
async fn durable_silent_completion_recovery_skips_when_latest_assistant_has_final_text() {
    let (execution_state, app_state) = setup_test_state().await;
    let conversation_id = create_durable_recovery_candidate(&app_state).await;
    let mut message = silent_tool_message();
    message.conversation_id = Some(conversation_id);
    message.content = "Done and validated.".to_string();
    message.created_at += chrono::Duration::seconds(1);
    message.content_blocks = Some(
        serde_json::json!([
            {
                "type": "tool_use",
                "id": "tool-1",
                "name": "apply_patch",
                "arguments": {},
                "result": null
            },
            {
                "type": "text",
                "text": "Done and validated."
            }
        ])
        .to_string(),
    );
    app_state.chat_message_repo.create(message).await.unwrap();

    let runner = build_runner(&app_state, &execution_state);

    assert_eq!(runner.recover_durable_silent_completions().await, 0);
    assert!(app_state
        .message_queue
        .get_queued(ChatContextType::Project, &conversation_id.as_str())
        .is_empty());
}

#[tokio::test]
async fn durable_silent_completion_recovery_skips_when_recovery_is_already_queued() {
    let (execution_state, app_state) = setup_test_state().await;
    let conversation_id = create_durable_recovery_candidate(&app_state).await;
    let mut queued = QueuedMessage::new("hidden recovery".to_string());
    queued.metadata_override = Some(silent_completion_recovery_metadata(1, 1_000));
    app_state.message_queue.queue_front_existing(
        ChatContextType::Project,
        conversation_id.as_str(),
        queued,
    );

    let runner = build_runner(&app_state, &execution_state);

    assert_eq!(runner.recover_durable_silent_completions().await, 0);
    assert_eq!(
        app_state
            .message_queue
            .get_queued(ChatContextType::Project, &conversation_id.as_str())
            .len(),
        1
    );
}

#[tokio::test]
async fn durable_silent_completion_recovery_skips_exhausted_attempts() {
    let (execution_state, app_state) = setup_test_state().await;
    let conversation_id = create_durable_recovery_candidate(&app_state).await;
    let mut marker = crate::domain::entities::ChatMessage::user_in_project(
        crate::domain::entities::ProjectId::from_string("project-1".to_string()),
        "RalphX hidden resume-in-place message was delivered.",
    );
    marker.role = crate::domain::entities::MessageRole::System;
    marker.conversation_id = Some(conversation_id);
    marker.created_at += chrono::Duration::seconds(1);
    marker.metadata = Some(silent_completion_recovery_metadata(3, 4_000));
    app_state.chat_message_repo.create(marker).await.unwrap();

    let runner = build_runner(&app_state, &execution_state);

    assert_eq!(runner.recover_durable_silent_completions().await, 0);
    assert!(app_state
        .message_queue
        .get_queued(ChatContextType::Project, &conversation_id.as_str())
        .is_empty());
}

#[tokio::test]
async fn durable_silent_completion_recovery_skips_without_latest_run() {
    let (execution_state, app_state) = setup_test_state().await;
    let project_id = crate::domain::entities::ProjectId::from_string("project-1".to_string());
    let mut conversation = ChatConversation::new_project(project_id);
    conversation.set_provider_session_ref(ProviderSessionRef {
        harness: AgentHarnessKind::Codex,
        provider_session_id: "codex-session-1".to_string(),
    });
    let conversation_id = conversation.id;
    app_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();

    let runner = build_runner(&app_state, &execution_state);

    assert_eq!(runner.recover_durable_silent_completions().await, 0);
    assert!(app_state
        .message_queue
        .get_queued(ChatContextType::Project, &conversation_id.as_str())
        .is_empty());
}

#[test]
fn durable_silent_completion_recovery_decision_skips_non_completed_run() {
    assert_eq!(
        durable_silent_completion_recovery_decision(
            ChatContextType::Project,
            true,
            AgentRunStatus::Running,
            &[silent_tool_message()],
            false,
        ),
        DurableSilentCompletionRecoveryDecision::NotNeeded
    );
}

#[test]
fn durable_silent_completion_recovery_decision_skips_without_assistant_message() {
    let project_id = crate::domain::entities::ProjectId::from_string("project-1".to_string());
    let message = crate::domain::entities::ChatMessage::user_in_project(project_id, "hello");

    assert_eq!(
        durable_silent_completion_recovery_decision(
            ChatContextType::Project,
            true,
            AgentRunStatus::Completed,
            &[message],
            false,
        ),
        DurableSilentCompletionRecoveryDecision::NotNeeded
    );
}

#[test]
fn durable_silent_completion_recovery_decision_skips_invalid_serialized_tool_payloads() {
    let mut message = silent_tool_message();
    message.tool_calls = Some("not json".to_string());
    message.content_blocks = Some("not json".to_string());

    assert_eq!(
        durable_silent_completion_recovery_decision(
            ChatContextType::Project,
            true,
            AgentRunStatus::Completed,
            &[message],
            false,
        ),
        DurableSilentCompletionRecoveryDecision::NotNeeded
    );
}

#[test]
fn durable_silent_completion_recovery_decision_recovers_after_terminal_tool() {
    let messages = vec![silent_tool_message()];

    let decision = durable_silent_completion_recovery_decision(
        ChatContextType::Project,
        true,
        AgentRunStatus::Completed,
        &messages,
        false,
    );

    let DurableSilentCompletionRecoveryDecision::Recover {
        attempt,
        metadata,
        prompt,
    } = decision
    else {
        panic!("expected recover decision");
    };
    assert_eq!(attempt, 1);
    assert!(prompt.contains("RalphX internal recovery message"));
    let metadata: serde_json::Value =
        serde_json::from_str(&metadata).expect("metadata should parse");
    assert_eq!(metadata["recovery_attempt"], 1);
    assert_eq!(metadata["resume_in_place"], true);
    assert_eq!(metadata["persist_hidden_marker"], true);
}

#[test]
fn durable_silent_completion_recovery_decision_skips_after_final_text() {
    let mut message = silent_tool_message();
    message.content = "Done".to_string();
    message.content_blocks = Some(
        serde_json::json!([
            {
                "type": "tool_use",
                "id": "tool-1",
                "name": "apply_patch",
                "arguments": {},
                "result": null
            },
            {
                "type": "text",
                "text": "Done"
            }
        ])
        .to_string(),
    );
    let messages = vec![message];

    assert_eq!(
        durable_silent_completion_recovery_decision(
            ChatContextType::Project,
            true,
            AgentRunStatus::Completed,
            &messages,
            false,
        ),
        DurableSilentCompletionRecoveryDecision::NotNeeded
    );
}

#[test]
fn durable_silent_completion_recovery_decision_stops_after_max_attempts() {
    let mut marker = crate::domain::entities::ChatMessage::user_in_project(
        crate::domain::entities::ProjectId::from_string("project-1".to_string()),
        "RalphX hidden resume-in-place message was delivered.",
    );
    marker.role = crate::domain::entities::MessageRole::System;
    marker.metadata = Some(silent_completion_recovery_metadata(3, 4_000));
    let messages = vec![silent_tool_message(), marker];

    assert_eq!(
        durable_silent_completion_recovery_decision(
            ChatContextType::Project,
            true,
            AgentRunStatus::Completed,
            &messages,
            false,
        ),
        DurableSilentCompletionRecoveryDecision::Exhausted { attempts: 3 }
    );
}

#[test]
fn durable_silent_completion_recovery_decision_uses_next_attempt_after_prior_marker() {
    let mut marker = crate::domain::entities::ChatMessage::user_in_project(
        crate::domain::entities::ProjectId::from_string("project-1".to_string()),
        "RalphX hidden resume-in-place message was delivered.",
    );
    marker.role = crate::domain::entities::MessageRole::System;
    marker.metadata = Some(silent_completion_recovery_metadata(1, 1_000));
    let messages = vec![marker, silent_tool_message()];

    let DurableSilentCompletionRecoveryDecision::Recover {
        attempt, metadata, ..
    } = durable_silent_completion_recovery_decision(
        ChatContextType::Project,
        true,
        AgentRunStatus::Completed,
        &messages,
        false,
    )
    else {
        panic!("expected second recovery attempt");
    };

    assert_eq!(attempt, 2);
    let metadata: serde_json::Value =
        serde_json::from_str(&metadata).expect("metadata should parse");
    assert_eq!(metadata["recovery_attempt"], 2);
    assert_eq!(metadata["recovery_backoff_ms"], 2_000);
}

#[test]
fn durable_silent_completion_recovery_decision_skips_when_already_queued() {
    assert_eq!(
        durable_silent_completion_recovery_decision(
            ChatContextType::Project,
            true,
            AgentRunStatus::Completed,
            &[silent_tool_message()],
            true,
        ),
        DurableSilentCompletionRecoveryDecision::AlreadyQueued
    );
}
