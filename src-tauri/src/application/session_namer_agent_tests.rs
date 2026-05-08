use std::pin::Pin;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use async_trait::async_trait;
use futures::Stream;

use super::session_namer_agent::{
    build_session_namer_agent_spawn, spawn_session_namer_agent, SessionNamerTarget,
};
use super::AppState;
use crate::application::harness_runtime_registry::default_repo_root_working_directory;
use crate::domain::agents::{
    AgentConfig, AgentError, AgentHandle, AgentHarnessKind, AgentLane, AgentLaneSettings,
    AgentOutput, AgentResponse, AgentResult, AgenticClient, ClientCapabilities, LogicalEffort,
    ResponseChunk,
};
use crate::domain::entities::{ChatConversation, DelegatedSession, IdeationSession, Project, Task};
use crate::infrastructure::agents::claude::agent_names;
use crate::infrastructure::{MockAgenticClient, MockCallType};

#[derive(Debug, Clone, Copy)]
enum FailingAgentMode {
    Spawn,
    Wait,
}

struct FailingSessionNamerClient {
    mode: FailingAgentMode,
    capabilities: ClientCapabilities,
    spawn_count: AtomicUsize,
    wait_count: AtomicUsize,
}

impl FailingSessionNamerClient {
    fn new(mode: FailingAgentMode) -> Self {
        Self {
            mode,
            capabilities: ClientCapabilities::mock(),
            spawn_count: AtomicUsize::new(0),
            wait_count: AtomicUsize::new(0),
        }
    }

    fn spawn_count(&self) -> usize {
        self.spawn_count.load(Ordering::SeqCst)
    }

    fn wait_count(&self) -> usize {
        self.wait_count.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl AgenticClient for FailingSessionNamerClient {
    async fn spawn_agent(&self, config: AgentConfig) -> AgentResult<AgentHandle> {
        self.spawn_count.fetch_add(1, Ordering::SeqCst);
        match self.mode {
            FailingAgentMode::Spawn => Err(AgentError::SpawnFailed(
                "session namer spawn failed".to_string(),
            )),
            FailingAgentMode::Wait => Ok(AgentHandle::mock(config.role)),
        }
    }

    async fn stop_agent(&self, _handle: &AgentHandle) -> AgentResult<()> {
        Ok(())
    }

    async fn wait_for_completion(&self, _handle: &AgentHandle) -> AgentResult<AgentOutput> {
        self.wait_count.fetch_add(1, Ordering::SeqCst);
        Err(AgentError::CommunicationFailed(
            "session namer wait failed".to_string(),
        ))
    }

    async fn send_prompt(&self, _handle: &AgentHandle, prompt: &str) -> AgentResult<AgentResponse> {
        Ok(AgentResponse::new(prompt))
    }

    fn stream_response(
        &self,
        _handle: &AgentHandle,
        _prompt: &str,
    ) -> Pin<Box<dyn Stream<Item = AgentResult<ResponseChunk>> + Send>> {
        Box::pin(futures::stream::empty())
    }

    fn capabilities(&self) -> &ClientCapabilities {
        &self.capabilities
    }

    async fn is_available(&self) -> AgentResult<bool> {
        Ok(true)
    }
}

#[tokio::test]
async fn session_namer_conversation_spawn_uses_active_project_cwd_and_conversation_harness() {
    let default_client: Arc<dyn AgenticClient> = Arc::new(MockAgenticClient::new());
    let codex_client: Arc<dyn AgenticClient> = Arc::new(MockAgenticClient::new());
    let state = AppState::new_test()
        .with_agent_client(default_client)
        .with_harness_agent_client(AgentHarnessKind::Codex, codex_client.clone());

    let project_dir = tempfile::tempdir().unwrap();
    let project = Project::new(
        "Codex Conversation Project".to_string(),
        project_dir.path().display().to_string(),
    );
    state.project_repo.create(project.clone()).await.unwrap();

    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.provider_harness = Some(AgentHarnessKind::Codex);
    let conversation = state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();

    let spawn = build_session_namer_agent_spawn(
        &state,
        SessionNamerTarget::conversation_initial(
            conversation.id.as_str(),
            "Name this Codex conversation",
        ),
    )
    .await
    .unwrap();

    assert!(Arc::ptr_eq(&spawn.client, &codex_client));
    assert_eq!(spawn.config.harness, Some(AgentHarnessKind::Codex));
    assert_eq!(spawn.config.model, None);
    assert_eq!(spawn.config.approval_policy.as_deref(), Some("never"));
    assert_eq!(
        spawn.config.sandbox_mode.as_deref(),
        Some("danger-full-access")
    );
    assert_eq!(spawn.config.working_directory, project_dir.path());
    assert_eq!(
        spawn.config.agent.as_deref(),
        Some(agent_names::AGENT_SESSION_NAMER)
    );
    assert!(spawn.config.prompt.contains("Name this Codex conversation"));
}

#[tokio::test]
async fn session_namer_session_spawn_uses_active_project_cwd_and_project_ideation_harness() {
    let default_client: Arc<dyn AgenticClient> = Arc::new(MockAgenticClient::new());
    let codex_client: Arc<dyn AgenticClient> = Arc::new(MockAgenticClient::new());
    let state = AppState::new_test()
        .with_agent_client(default_client)
        .with_harness_agent_client(AgentHarnessKind::Codex, codex_client.clone());

    let project_dir = tempfile::tempdir().unwrap();
    let project = Project::new(
        "Codex Session Project".to_string(),
        project_dir.path().display().to_string(),
    );
    state.project_repo.create(project.clone()).await.unwrap();
    let mut codex_lane = AgentLaneSettings::new(AgentHarnessKind::Codex);
    codex_lane.model = Some("gpt-5.4".to_string());
    codex_lane.effort = Some(LogicalEffort::XHigh);
    state
        .agent_lane_settings_repo
        .upsert_for_project(project.id.as_str(), AgentLane::IdeationPrimary, &codex_lane)
        .await
        .unwrap();

    let session = state
        .ideation_session_repo
        .create(IdeationSession::new(project.id.clone()))
        .await
        .unwrap();

    let spawn = build_session_namer_agent_spawn(
        &state,
        SessionNamerTarget::session_initial(session.id.as_str(), "Build the settings analyzer"),
    )
    .await
    .unwrap();

    assert!(Arc::ptr_eq(&spawn.client, &codex_client));
    assert_eq!(spawn.config.harness, Some(AgentHarnessKind::Codex));
    assert_eq!(spawn.config.model.as_deref(), Some("gpt-5.4"));
    assert_eq!(spawn.config.logical_effort, Some(LogicalEffort::XHigh));
    assert_eq!(spawn.config.approval_policy.as_deref(), Some("never"));
    assert_eq!(
        spawn.config.sandbox_mode.as_deref(),
        Some("danger-full-access")
    );
    assert_eq!(spawn.config.working_directory, project_dir.path());
    assert_eq!(
        spawn.config.agent.as_deref(),
        Some(agent_names::AGENT_SESSION_NAMER)
    );
    assert!(spawn.config.prompt.contains("Build the settings analyzer"));
}

#[tokio::test]
async fn session_namer_fire_and_forget_spawns_and_waits_for_accepted_session() {
    let concrete_client = Arc::new(MockAgenticClient::new());
    let agent_client: Arc<dyn AgenticClient> = concrete_client.clone();
    let state = AppState::new_test().with_agent_client(agent_client);

    let project_dir = tempfile::tempdir().unwrap();
    let project = Project::new(
        "Accepted Session Project".to_string(),
        project_dir.path().display().to_string(),
    );
    state.project_repo.create(project.clone()).await.unwrap();
    let session = state
        .ideation_session_repo
        .create(IdeationSession::new(project.id.clone()))
        .await
        .unwrap();

    spawn_session_namer_agent(
        &state,
        SessionNamerTarget::accepted_session(
            session.id.as_str(),
            "Ship utility agent runtime fixes",
        ),
    )
    .await
    .unwrap();

    for _ in 0..20 {
        if concrete_client.get_calls().await.len() >= 2 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    let calls = concrete_client.get_calls().await;
    assert!(
        calls.iter().any(|call| matches!(
            &call.call_type,
            MockCallType::Spawn { prompt, .. }
                if prompt.contains("<accepted_proposals>Ship utility agent runtime fixes</accepted_proposals>")
        )),
        "session namer should spawn with the accepted-proposals prompt"
    );
    assert!(
        calls
            .iter()
            .any(|call| matches!(call.call_type, MockCallType::WaitForCompletion { .. })),
        "fire-and-forget session namer task should wait for the helper to complete"
    );
}

#[tokio::test]
async fn session_namer_fire_and_forget_logs_spawn_and_wait_failures_without_erroring() {
    for mode in [FailingAgentMode::Spawn, FailingAgentMode::Wait] {
        let concrete_client = Arc::new(FailingSessionNamerClient::new(mode));
        let agent_client: Arc<dyn AgenticClient> = concrete_client.clone();
        let state = AppState::new_test().with_agent_client(agent_client);

        let project = Project::new(
            format!("Failure Mode Project {mode:?}"),
            tempfile::tempdir().unwrap().path().display().to_string(),
        );
        state.project_repo.create(project.clone()).await.unwrap();
        let session = state
            .ideation_session_repo
            .create(IdeationSession::new(project.id.clone()))
            .await
            .unwrap();

        spawn_session_namer_agent(
            &state,
            SessionNamerTarget::accepted_session(session.id.as_str(), "Rejected helper branch"),
        )
        .await
        .unwrap();

        for _ in 0..20 {
            let observed = match mode {
                FailingAgentMode::Spawn => concrete_client.spawn_count() >= 1,
                FailingAgentMode::Wait => concrete_client.wait_count() >= 1,
            };
            if observed {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        assert_eq!(concrete_client.spawn_count(), 1);
        match mode {
            FailingAgentMode::Spawn => assert_eq!(concrete_client.wait_count(), 0),
            FailingAgentMode::Wait => assert_eq!(concrete_client.wait_count(), 1),
        }
    }
}

#[tokio::test]
async fn session_namer_ideation_conversation_spawn_uses_session_project_cwd() {
    let default_client: Arc<dyn AgenticClient> = Arc::new(MockAgenticClient::new());
    let state = AppState::new_test().with_agent_client(default_client);

    let project_dir = tempfile::tempdir().unwrap();
    let project = Project::new(
        "Ideation Conversation Project".to_string(),
        project_dir.path().display().to_string(),
    );
    state.project_repo.create(project.clone()).await.unwrap();
    let session = state
        .ideation_session_repo
        .create(IdeationSession::new(project.id.clone()))
        .await
        .unwrap();
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_ideation(session.id.clone()))
        .await
        .unwrap();

    let spawn = build_session_namer_agent_spawn(
        &state,
        SessionNamerTarget::conversation_initial(
            conversation.id.as_str(),
            "Name this ideation conversation",
        ),
    )
    .await
    .unwrap();

    assert_eq!(spawn.config.working_directory, project_dir.path());
    assert!(spawn
        .config
        .prompt
        .contains("Name this ideation conversation"));
}

#[tokio::test]
async fn session_namer_task_conversation_spawn_uses_task_project_cwd() {
    let default_client: Arc<dyn AgenticClient> = Arc::new(MockAgenticClient::new());
    let state = AppState::new_test().with_agent_client(default_client);

    let project_dir = tempfile::tempdir().unwrap();
    let project = Project::new(
        "Task Conversation Project".to_string(),
        project_dir.path().display().to_string(),
    );
    state.project_repo.create(project.clone()).await.unwrap();
    let task = state
        .task_repo
        .create(Task::new(
            project.id.clone(),
            "Task conversation".to_string(),
        ))
        .await
        .unwrap();
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_task(task.id.clone()))
        .await
        .unwrap();

    let spawn = build_session_namer_agent_spawn(
        &state,
        SessionNamerTarget::conversation_initial(
            conversation.id.as_str(),
            "Name this task conversation",
        ),
    )
    .await
    .unwrap();

    assert_eq!(spawn.config.working_directory, project_dir.path());
    assert_eq!(spawn.project_id.as_deref(), Some(project.id.as_str()));
}

#[tokio::test]
async fn session_namer_delegation_conversation_spawn_uses_delegated_project_cwd() {
    let default_client: Arc<dyn AgenticClient> = Arc::new(MockAgenticClient::new());
    let state = AppState::new_test().with_agent_client(default_client);

    let project_dir = tempfile::tempdir().unwrap();
    let project = Project::new(
        "Delegation Conversation Project".to_string(),
        project_dir.path().display().to_string(),
    );
    state.project_repo.create(project.clone()).await.unwrap();
    let delegated = state
        .delegated_session_repo
        .create(DelegatedSession::new(
            project.id.clone(),
            "task",
            "task-1",
            "ralphx-execution-reviewer",
            AgentHarnessKind::Codex,
        ))
        .await
        .unwrap();
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_delegation(delegated.id.clone()))
        .await
        .unwrap();

    let spawn = build_session_namer_agent_spawn(
        &state,
        SessionNamerTarget::conversation_initial(
            conversation.id.as_str(),
            "Name this delegated conversation",
        ),
    )
    .await
    .unwrap();

    assert_eq!(spawn.config.working_directory, project_dir.path());
    assert_eq!(spawn.project_id.as_deref(), Some(project.id.as_str()));
}

#[tokio::test]
async fn session_namer_missing_conversation_returns_not_found() {
    let default_client: Arc<dyn AgenticClient> = Arc::new(MockAgenticClient::new());
    let state = AppState::new_test().with_agent_client(default_client);

    let error = match build_session_namer_agent_spawn(
        &state,
        SessionNamerTarget::conversation_initial("missing-conversation", "Name this"),
    )
    .await
    {
        Ok(_) => panic!("missing conversation should not build a session namer spawn"),
        Err(error) => error,
    };

    assert!(
        error.to_string().contains("Conversation not found"),
        "unexpected error: {error}"
    );
}

#[tokio::test]
async fn session_namer_conversation_without_project_uses_runtime_root_fallback() {
    let default_client: Arc<dyn AgenticClient> = Arc::new(MockAgenticClient::new());
    let state = AppState::new_test().with_agent_client(default_client);
    let missing_task = Task::new(
        crate::domain::entities::ProjectId::from_string("missing-project".to_string()),
        "Missing task".to_string(),
    );
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_task(missing_task.id.clone()))
        .await
        .unwrap();

    let spawn = build_session_namer_agent_spawn(
        &state,
        SessionNamerTarget::conversation_initial(
            conversation.id.as_str(),
            "Name this legacy conversation",
        ),
    )
    .await
    .unwrap();

    assert_eq!(
        spawn.config.working_directory,
        default_repo_root_working_directory()
    );
    assert_eq!(spawn.project_id, None);
}

#[test]
fn session_namer_initial_request_target_requires_exactly_one_target_id() {
    let session_target = SessionNamerTarget::from_initial_request(
        Some("session-1".to_string()),
        None,
        "Name session".to_string(),
    )
    .unwrap();
    assert!(matches!(
        session_target,
        SessionNamerTarget::SessionInitial { .. }
    ));

    let conversation_target = SessionNamerTarget::from_initial_request(
        None,
        Some("conversation-1".to_string()),
        "Name conversation".to_string(),
    )
    .unwrap();
    assert!(matches!(
        conversation_target,
        SessionNamerTarget::ConversationInitial { .. }
    ));

    assert!(SessionNamerTarget::from_initial_request(
        Some("session-1".to_string()),
        Some("conversation-1".to_string()),
        "ambiguous".to_string(),
    )
    .is_err());
    assert!(SessionNamerTarget::from_initial_request(None, None, "missing".to_string()).is_err());
}
