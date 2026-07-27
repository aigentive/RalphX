use super::*;
use crate::application::agent_conversation_workspace::resolve_agent_conversation_workspace_path;
use crate::application::AppState;
use crate::domain::agents::{AgentHarnessKind, ProviderSessionRef};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentConversationWorkspaceStatus,
    AgentRun, AgentRunAttribution, AgentRunId, AgentRunStatus, AgentRunUsage, AutomationId,
    AutomationJudgeState, AutomationPlanJudgeState, AutomationPromptAuthor, AutomationRun,
    AutomationRunId, AutomationRunStatus, ChatConversation, ChatConversationId, CoordinationMode,
    IdeationAnalysisBaseRefKind, InternalStatus, Project, ProjectId, Task, TeamIntent,
};
use crate::domain::repositories::{AgentRunRepository, AutomationRunRepository};
use crate::domain::services::{QueuedMessage, RunningAgentKey};
use crate::error::AppResult;
use crate::infrastructure::memory::{MemoryAutomationRepository, MemoryAutomationRunRepository};

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
) -> ChatResumptionRunner {
    build_runner_with_agent_run_repo(
        app_state,
        execution_state,
        Arc::clone(&app_state.agent_run_repo),
        Arc::clone(&app_state.automation_run_repo),
    )
}

fn build_runner_with_agent_run_repo(
    app_state: &AppState,
    execution_state: &Arc<ExecutionState>,
    agent_run_repo: Arc<dyn AgentRunRepository>,
    automation_run_repo: Arc<dyn AutomationRunRepository>,
) -> ChatResumptionRunner {
    ChatResumptionRunner::new(
        agent_run_repo,
        Arc::clone(&automation_run_repo),
        Arc::clone(&app_state.task_repo),
        Arc::clone(execution_state),
        crate::application::runtime_factory::ChatRuntimeFactoryDeps::from_core(
            Arc::clone(&app_state.chat_message_repo),
            Arc::clone(&app_state.chat_attachment_repo),
            Arc::clone(&app_state.artifact_repo),
            Arc::clone(&app_state.chat_conversation_repo),
            Arc::clone(&app_state.agent_run_repo),
            Arc::clone(&automation_run_repo),
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

#[test]
fn startup_resumption_send_options_carries_persisted_team_intent() {
    let mut conversation = ChatConversation::new_project(ProjectId::from_string(
        "project-team-resumption".to_string(),
    ));
    conversation.set_coordination_mode(CoordinationMode::RxNativeTeam);
    let options = startup_resumption_send_options(&conversation);

    assert_eq!(options.conversation_id_override, Some(conversation.id));
    assert_eq!(options.team_intent, Some(TeamIntent::rx_native(None)));
    assert_eq!(options.caller_context, SendCallerContext::StartupResumption);
}

#[test]
fn durable_silent_completion_recovery_send_options_carries_persisted_team_intent() {
    let mut conversation = ChatConversation::new_project(ProjectId::from_string(
        "project-team-durable-recovery".to_string(),
    ));
    conversation.set_coordination_mode(CoordinationMode::RxNativeTeam);
    let options = durable_silent_completion_recovery_send_options(
        &conversation,
        "{\"source\":\"test\"}".to_string(),
    );

    assert_eq!(options.conversation_id_override, Some(conversation.id));
    assert_eq!(options.team_intent, Some(TeamIntent::rx_native(None)));
    assert_eq!(options.caller_context, SendCallerContext::StartupResumption);
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

fn automation_run_for_conversation(conversation_id: ChatConversationId) -> AutomationRun {
    let now = chrono::Utc::now();
    AutomationRun {
        id: AutomationRunId::new(),
        automation_id: AutomationId::new(),
        run_index: 1,
        status: AutomationRunStatus::Running,
        judge_state: AutomationJudgeState::None,
        judge_lease_expires_at: None,
        plan_judge_state: AutomationPlanJudgeState::None,
        plan_judge_lease_expires_at: None,
        plan_judge_verdict_json: None,
        plan_revision_round: 0,
        plan_reminder_count: 0,
        plan_pending_instructions: None,
        plan_last_parked_artifact_id: None,
        plan_last_parked_blueprint_artifact_id: None,
        agent_phase_started_at: None,
        conversation_id: Some(conversation_id),
        run_prompt: "automation run".to_string(),
        prompt_author: AutomationPromptAuthor::SetupAgent,
        base_ref_kind: "project_default".to_string(),
        base_ref_used: "main".to_string(),
        base_from_run_id: None,
        goal_item_id: None,
        branch_name: None,
        pr_number: None,
        pr_url: None,
        pr_title: None,
        pr_head_ref_name: None,
        pr_base_ref_name: None,
        pr_merged_at: None,
        merge_commit_sha: None,
        diff_stats_json: None,
        agent_summary: None,
        judge_verdict_json: None,
        judge_model_id: None,
        error_code: None,
        error_detail: None,
        signal_check_failures: 0,
        started_at: Some(now),
        finished_at: None,
        created_at: now,
        updated_at: now,
    }
}

fn memory_automation_run_repo() -> Arc<MemoryAutomationRunRepository> {
    Arc::new(MemoryAutomationRunRepository::new(
        MemoryAutomationRepository::new_shared_state(),
    ))
}

async fn create_interrupted_project_conversation(
    app_state: &AppState,
    project_id: ProjectId,
    automation_id: Option<AutomationId>,
    automation_run_id: Option<AutomationRunId>,
) -> (ChatConversation, AgentRun) {
    let mut conversation = ChatConversation::new_project(project_id);
    conversation.set_provider_session_ref(ProviderSessionRef {
        harness: AgentHarnessKind::Codex,
        provider_session_id: format!("codex-{}", conversation.id.as_str()),
    });
    conversation.automation_id = automation_id;
    conversation.automation_run_id = automation_run_id;
    app_state
        .chat_conversation_repo
        .create(conversation.clone())
        .await
        .expect("conversation should persist");
    let run = AgentRun::new(conversation.id);
    (conversation, run)
}

async fn assert_not_resumed(app_state: &AppState, conversation: &ChatConversation) {
    assert!(
        app_state
            .message_queue
            .get_queued(ChatContextType::Project, &conversation.context_id)
            .is_empty(),
        "automation conversation must not receive a queued generic recovery message"
    );
    assert!(
        !app_state
            .running_agent_registry
            .is_running(&RunningAgentKey::new(
                ChatContextType::Project.to_string(),
                conversation.context_id.clone(),
            ))
            .await,
        "automation conversation must not spawn a generic resumption runtime"
    );
    assert!(
        app_state
            .chat_message_repo
            .get_by_conversation(&conversation.id)
            .await
            .expect("messages should load")
            .is_empty(),
        "automation conversation must not persist a generic recovery message"
    );
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
    // Standalone conversations are projectless chats and share Project's
    // (lowest) resumption priority.
    assert_eq!(
        context_type_priority(ChatContextType::Standalone),
        context_type_priority(ChatContextType::Project)
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
        AgentConversationWorkspaceMode::Automation,
    ] {
        let mut conversation = ChatConversation::new_project(project_id.clone());
        conversation.set_agent_mode(Some(mode));

        let options = startup_resumption_send_options(&conversation);

        assert_eq!(
            options.conversation_id_override,
            Some(conversation.id),
            "startup resumption must resume the exact interrupted conversation for {mode:?} mode so agent_mode/workspace linkage are preserved"
        );
        assert_eq!(
            options.caller_context,
            SendCallerContext::StartupResumption,
            "startup resumption must be distinguishable from user-initiated project chat sends for {mode:?} mode"
        );
    }
}

#[test]
fn durable_silent_completion_recovery_send_options_marks_startup_recovery() {
    let project_id =
        crate::domain::entities::ProjectId::from_string("project-durable-recovery".to_string());
    let conversation = ChatConversation::new_project(project_id);
    let metadata = silent_completion_recovery_metadata(2, 4_000);

    let options = durable_silent_completion_recovery_send_options(&conversation, metadata.clone());

    assert_eq!(options.metadata.as_deref(), Some(metadata.as_str()));
    assert_eq!(options.conversation_id_override, Some(conversation.id));
    assert_eq!(
        options.caller_context,
        SendCallerContext::StartupResumption,
        "durable recovery must use startup resumption guards instead of user-send rollover"
    );
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
    let runner = build_runner_with_agent_run_repo(
        &app_state,
        &execution_state,
        agent_run_repo,
        Arc::clone(&app_state.automation_run_repo),
    );

    runner.run().await;

    assert!(app_state
        .message_queue
        .get_queued(ChatContextType::Project, &conversation.id.as_str())
        .is_empty());
}

#[tokio::test]
async fn startup_resumption_without_override_blocks_terminal_workspace_before_transcript() {
    let (execution_state, app_state) = setup_test_state().await;
    let temp = tempfile::tempdir().expect("temp dir should be created");
    let project = temp_project(&temp, "Terminal Active Agent Workspace");
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.set_agent_mode(Some(AgentConversationWorkspaceMode::Edit));
    let conversation_id = conversation.id;
    app_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();

    let mut workspace = workspace_for_conversation(
        &project,
        conversation_id,
        AgentConversationWorkspaceMode::Edit,
        Some("merged"),
    );
    workspace.status = AgentConversationWorkspaceStatus::Missing;
    app_state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();

    let service = build_runner(&app_state, &execution_state).create_chat_service();
    let error = service
        .send_message(
            ChatContextType::Project,
            project.id.as_str(),
            "Continue where you left off.",
            SendMessageOptions {
                caller_context: SendCallerContext::StartupResumption,
                ..Default::default()
            },
        )
        .await
        .expect_err("startup resumption should not spawn a missing terminal workspace");

    assert!(
        error.to_string().contains("missing locally"),
        "blocked startup resumption should explain the missing workspace: {error}"
    );
    let messages = app_state
        .chat_message_repo
        .get_by_conversation(&conversation_id)
        .await
        .expect("messages should load");
    assert!(
        messages.is_empty(),
        "startup resumption guard should fire before hidden resume or error messages are persisted"
    );
    assert!(
        !app_state
            .running_agent_registry
            .is_running(&RunningAgentKey::new(
                ChatContextType::Project.to_string(),
                project.id.as_str(),
            ))
            .await
    );
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
async fn chat_resumption_skips_automation_owned_interrupted_conversation_and_resumes_sibling() {
    let (execution_state, app_state) = setup_test_state().await;
    let project = Project::new(
        "Automation resumption".to_string(),
        "/test/path".to_string(),
    );
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();
    let sibling_project = Project::new(
        "Non-automation resumption".to_string(),
        "/test/sibling-path".to_string(),
    );
    app_state
        .project_repo
        .create(sibling_project.clone())
        .await
        .unwrap();

    let automation_id = AutomationId::new();
    let automation_run_id = AutomationRunId::new();
    let (automation_conversation, automation_agent_run) = create_interrupted_project_conversation(
        &app_state,
        project.id.clone(),
        Some(automation_id),
        Some(automation_run_id),
    )
    .await;
    let (sibling_conversation, sibling_agent_run) =
        create_interrupted_project_conversation(&app_state, sibling_project.id, None, None).await;
    let agent_run_repo: Arc<dyn AgentRunRepository> = Arc::new(InterruptedAgentRunRepo::new(
        vec![automation_agent_run.clone(), sibling_agent_run.clone()],
        vec![
            InterruptedConversation {
                conversation: automation_conversation.clone(),
                last_run: automation_agent_run,
            },
            InterruptedConversation {
                conversation: sibling_conversation.clone(),
                last_run: sibling_agent_run,
            },
        ],
    ));
    let automation_run_repo = memory_automation_run_repo();
    let runner = build_runner_with_agent_run_repo(
        &app_state,
        &execution_state,
        agent_run_repo,
        automation_run_repo,
    );

    runner.run().await;

    assert_not_resumed(&app_state, &automation_conversation).await;
    assert!(
        !app_state
            .chat_message_repo
            .get_by_conversation(&sibling_conversation.id)
            .await
            .expect("sibling messages should load")
            .is_empty(),
        "non-automation sibling must still be resumed in the same run"
    );
}

#[tokio::test]
async fn durable_silent_completion_recovery_skips_automation_owned_conversation_and_recovers_sibling(
) {
    let (execution_state, app_state) = setup_test_state().await;
    // Pause launches so the sibling recovery takes the queue path; the spawn path
    // resolves the real Codex CLI, which is absent on CI runners.
    execution_state.pause();
    let project_id = ProjectId::from_string("durable-automation-project".to_string());
    let (automation_conversation, _) = create_interrupted_project_conversation(
        &app_state,
        project_id.clone(),
        Some(AutomationId::new()),
        Some(AutomationRunId::new()),
    )
    .await;
    let (sibling_conversation, _) =
        create_interrupted_project_conversation(&app_state, project_id, None, None).await;
    for conversation in [&automation_conversation, &sibling_conversation] {
        let mut run = AgentRun::new(conversation.id);
        run.complete();
        app_state.agent_run_repo.create(run).await.unwrap();
        let mut message = silent_tool_message();
        message.conversation_id = Some(conversation.id);
        app_state.chat_message_repo.create(message).await.unwrap();
    }
    let automation_run_repo = memory_automation_run_repo();
    let runner = build_runner_with_agent_run_repo(
        &app_state,
        &execution_state,
        Arc::clone(&app_state.agent_run_repo),
        automation_run_repo,
    );

    assert_eq!(runner.recover_durable_silent_completions().await, 1);
    assert!(
        app_state
            .message_queue
            .get_queued(
                ChatContextType::Project,
                &automation_conversation.id.as_str()
            )
            .is_empty(),
        "automation conversation must not queue durable silent-completion recovery"
    );
    assert_eq!(
        app_state
            .chat_message_repo
            .get_by_conversation(&automation_conversation.id)
            .await
            .expect("automation messages should load")
            .len(),
        1,
        "automation conversation must not persist a durable recovery message"
    );
    assert!(
        !app_state
            .running_agent_registry
            .is_running(&RunningAgentKey::new(
                ChatContextType::Project.to_string(),
                automation_conversation.id.as_str(),
            ))
            .await,
        "automation conversation must not spawn a durable recovery runtime"
    );
    let sibling_queued = app_state
        .message_queue
        .get_queued(ChatContextType::Project, &sibling_conversation.id.as_str());
    assert_eq!(
        sibling_queued.len(),
        1,
        "non-automation sibling must queue exactly one durable recovery message"
    );
}

#[tokio::test]
async fn chat_resumption_fails_closed_when_automation_lookup_errors_without_starving_siblings() {
    let (execution_state, app_state) = setup_test_state().await;
    let project = Project::new(
        "Lookup error resumption".to_string(),
        "/test/path".to_string(),
    );
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();
    let (failed_conversation, failed_agent_run) =
        create_interrupted_project_conversation(&app_state, project.id.clone(), None, None).await;
    let (sibling_conversation, sibling_agent_run) =
        create_interrupted_project_conversation(&app_state, project.id, None, None).await;
    let agent_run_repo: Arc<dyn AgentRunRepository> = Arc::new(InterruptedAgentRunRepo::new(
        vec![failed_agent_run.clone(), sibling_agent_run.clone()],
        vec![
            InterruptedConversation {
                conversation: failed_conversation.clone(),
                last_run: failed_agent_run,
            },
            InterruptedConversation {
                conversation: sibling_conversation.clone(),
                last_run: sibling_agent_run,
            },
        ],
    ));
    let automation_run_repo = memory_automation_run_repo();
    automation_run_repo.fail_find_run_for_conversation(&failed_conversation.id);
    let runner = build_runner_with_agent_run_repo(
        &app_state,
        &execution_state,
        agent_run_repo,
        automation_run_repo,
    );

    runner.run().await;

    assert_not_resumed(&app_state, &failed_conversation).await;
    assert!(
        !app_state
            .chat_message_repo
            .get_by_conversation(&sibling_conversation.id)
            .await
            .expect("sibling messages should load")
            .is_empty(),
        "a lookup error must skip only its candidate"
    );
}

#[tokio::test]
async fn chat_resumption_skips_marker_divergence_when_automation_run_owns_conversation() {
    let (execution_state, app_state) = setup_test_state().await;
    let project = Project::new("Marker divergence".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();
    let (conversation, agent_run) =
        create_interrupted_project_conversation(&app_state, project.id, None, None).await;
    let agent_run_repo: Arc<dyn AgentRunRepository> = Arc::new(InterruptedAgentRunRepo::new(
        vec![agent_run.clone()],
        vec![InterruptedConversation {
            conversation: conversation.clone(),
            last_run: agent_run,
        }],
    ));
    let automation_run_repo = memory_automation_run_repo();
    automation_run_repo
        .create_run(automation_run_for_conversation(conversation.id))
        .await
        .unwrap();
    let runner = build_runner_with_agent_run_repo(
        &app_state,
        &execution_state,
        agent_run_repo,
        automation_run_repo,
    );

    runner.run().await;

    assert_not_resumed(&app_state, &conversation).await;
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
    let runner = build_runner_with_agent_run_repo(
        &app_state,
        &execution_state,
        agent_run_repo,
        Arc::clone(&app_state.automation_run_repo),
    );

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

#[tokio::test]
async fn test_is_handled_by_task_resumption_for_standalone() {
    let (execution_state, app_state) = setup_test_state().await;

    // Create an interrupted conversation for a self-keyed Standalone conversation.
    let mut conv = ChatConversation::new_project(crate::domain::entities::ProjectId::new());
    conv.context_type = ChatContextType::Standalone;
    conv.context_id = conv.id.as_str();
    conv.claude_session_id = Some("test-session".to_string());

    let run = AgentRun::new(conv.id);

    let interrupted = InterruptedConversation {
        conversation: conv,
        last_run: run,
    };

    let runner = build_runner(&app_state, &execution_state);

    // Standalone must be enumerated for restart recovery the same way Project is
    // (not silently excluded by the StartupJobRunner-owned gate).
    let is_handled = runner.is_handled_by_task_resumption(&interrupted).await;
    assert!(
        !is_handled,
        "Standalone should NOT be handled by StartupJobRunner"
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

/// Standalone equivalent of `create_durable_recovery_candidate`: self-keyed
/// (`context_id == conversation.id`), no project affiliation.
async fn create_durable_recovery_candidate_standalone(
    app_state: &AppState,
) -> crate::domain::entities::ChatConversationId {
    let mut conversation = ChatConversation::new_project(crate::domain::entities::ProjectId::new());
    conversation.context_type = ChatContextType::Standalone;
    conversation.set_provider_session_ref(ProviderSessionRef {
        harness: AgentHarnessKind::Codex,
        provider_session_id: "codex-standalone-session-1".to_string(),
    });
    let conversation_id = conversation.id;
    conversation.context_id = conversation_id.as_str();
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

    conversation_id
}

#[tokio::test]
async fn durable_silent_completion_recovery_scans_both_project_and_standalone() {
    let (execution_state, app_state) = setup_test_state().await;
    // Keep this scan/recovery test on the established deterministic queue path.
    // An active launch would resolve a real provider CLI, making this repository
    // and recovery-policy test depend on machine-specific harness availability.
    execution_state.pause();
    let runner = build_runner(&app_state, &execution_state);

    let project_conversation_id = create_durable_recovery_candidate(&app_state).await;
    let standalone_conversation_id = create_durable_recovery_candidate_standalone(&app_state).await;

    // Prove the restart-recovery *scan* itself finds a candidate in both
    // directions: the Project-context and Standalone-context durable recovery
    // scans are two separate `list_recent_resumable_by_context_type` calls
    // (see `recover_durable_silent_completions`), and both must return their
    // seeded conversation.
    let project_candidates = app_state
        .chat_conversation_repo
        .list_recent_resumable_by_context_type(
            ChatContextType::Project,
            DURABLE_SILENT_COMPLETION_RECOVERY_SCAN_LIMIT,
        )
        .await
        .expect("project scan succeeds");
    let standalone_candidates = app_state
        .chat_conversation_repo
        .list_recent_resumable_by_context_type(
            ChatContextType::Standalone,
            DURABLE_SILENT_COMPLETION_RECOVERY_SCAN_LIMIT,
        )
        .await
        .expect("standalone scan succeeds");
    assert_eq!(
        project_candidates.len(),
        1,
        "Project candidate must be enumerated"
    );
    assert_eq!(
        standalone_candidates.len(),
        1,
        "Standalone candidate must be enumerated"
    );

    assert_eq!(runner.recover_durable_silent_completions().await, 2);
    for (context_type, conversation_id) in [
        (ChatContextType::Project, project_conversation_id),
        (ChatContextType::Standalone, standalone_conversation_id),
    ] {
        let queued = app_state
            .message_queue
            .get_queued(context_type, &conversation_id.as_str());
        assert_eq!(
            queued.len(),
            1,
            "{context_type} recovery must queue exactly one message"
        );
        assert!(
            silent_completion_recovery_attempt(queued[0].metadata_override.as_deref()) > 0,
            "{context_type} recovery queue entry must retain its durable attempt marker"
        );
        assert!(
            !app_state
                .running_agent_registry
                .is_running(&RunningAgentKey::new(
                    context_type.to_string(),
                    conversation_id.as_str(),
                ))
                .await,
            "{context_type} recovery must not dispatch while execution is paused"
        );
    }
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
