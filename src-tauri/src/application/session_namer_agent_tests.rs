use std::pin::Pin;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use async_trait::async_trait;
use futures::Stream;

use super::session_namer_agent::{
    build_session_namer_agent_spawn, extract_session_namer_title, spawn_session_namer_agent,
    SessionNamerTarget,
};
use super::AppState;
use crate::application::app_paths::AppPaths;
use crate::application::harness_runtime_registry::default_repo_root_working_directory;
use crate::domain::agents::{
    AgentConfig, AgentError, AgentHandle, AgentHarnessKind, AgentLane, AgentLaneSettings,
    AgentOutput, AgentResponse, AgentResult, AgenticClient, ClientCapabilities, LogicalEffort,
    ResponseChunk,
};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentWorkspaceSourcePullRequest,
    ChatConversation, ChatMessage, DelegatedSession, IdeationAnalysisBaseRefKind, IdeationSession,
    IdeationSessionFlow, IdeationSessionId, MessageRole, Project, Task,
};
use crate::infrastructure::agents::claude::agent_names;
use crate::infrastructure::{MockAgenticClient, MockCallType};

fn conversation_initial(
    conversation_id: impl Into<String>,
    user_message: impl Into<String>,
) -> SessionNamerTarget {
    SessionNamerTarget::from_initial_request(
        None,
        Some(conversation_id.into()),
        user_message.into(),
        None,
        None,
    )
    .expect("conversation target")
}

fn conversation_initial_with_harness(
    conversation_id: impl Into<String>,
    user_message: impl Into<String>,
    requested_harness: AgentHarnessKind,
) -> SessionNamerTarget {
    SessionNamerTarget::from_initial_request(
        None,
        Some(conversation_id.into()),
        user_message.into(),
        Some(requested_harness),
        None,
    )
    .expect("conversation target")
}

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

struct SuccessfulSessionNamerClient {
    output_content: String,
    capabilities: ClientCapabilities,
    spawn_count: AtomicUsize,
    wait_count: AtomicUsize,
}

impl SuccessfulSessionNamerClient {
    fn new(output_content: impl Into<String>) -> Self {
        Self {
            output_content: output_content.into(),
            capabilities: ClientCapabilities::mock(),
            spawn_count: AtomicUsize::new(0),
            wait_count: AtomicUsize::new(0),
        }
    }
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
impl AgenticClient for SuccessfulSessionNamerClient {
    async fn spawn_agent(&self, config: AgentConfig) -> AgentResult<AgentHandle> {
        self.spawn_count.fetch_add(1, Ordering::SeqCst);
        Ok(AgentHandle::mock(config.role))
    }

    async fn stop_agent(&self, _handle: &AgentHandle) -> AgentResult<()> {
        Ok(())
    }

    async fn wait_for_completion(&self, _handle: &AgentHandle) -> AgentResult<AgentOutput> {
        self.wait_count.fetch_add(1, Ordering::SeqCst);
        Ok(AgentOutput {
            success: true,
            content: self.output_content.clone(),
            exit_code: Some(0),
            duration_ms: Some(25),
        })
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

#[test]
fn session_namer_extracts_title_from_plain_title_output() {
    assert_eq!(
        extract_session_namer_title("Fix stuck auto rename flow").as_deref(),
        Some("Fix stuck auto rename flow")
    );
}

#[test]
fn session_namer_extracts_title_from_claude_pseudo_tool_output() {
    let output = r##"{"type":"result","subtype":"success","result":"# Session Title Generation\n\n**Generated Title:** `Test session namer`\n\n<invoke name=\"update_session_title\">\n<parameter name=\"conversation_id\">conversation-1</parameter>\n<parameter name=\"title\">Test session namer</parameter>\n</invoke>"}"##;

    assert_eq!(
        extract_session_namer_title(output).as_deref(),
        Some("Test session namer")
    );
}

#[test]
fn session_namer_extracts_title_from_structured_and_defensive_output_shapes() {
    let assistant_text =
        r#"{"message":{"content":[{"type":"text","text":"Title: Build retry diagnostics"}]}}"#;
    assert_eq!(
        extract_session_namer_title(assistant_text).as_deref(),
        Some("Build retry diagnostics")
    );

    let tool_use = r#"{"message":{"content":[{"type":"tool_use","input":{"title":"Repair generated plugin hooks"}}]}}"#;
    assert_eq!(
        extract_session_namer_title(tool_use).as_deref(),
        Some("Repair generated plugin hooks")
    );

    let line_delimited = "noise\n{\"title\":\"Name Codex conversations\"}\nmore noise";
    assert_eq!(
        extract_session_namer_title(line_delimited).as_deref(),
        Some("Name Codex conversations")
    );

    assert_eq!(
        extract_session_namer_title(r#"{"title":"Direct JSON Title"}"#).as_deref(),
        Some("Direct JSON Title")
    );

    assert_eq!(
        extract_session_namer_title(r#"{"result":"Generated Title: Result JSON Title"}"#)
            .as_deref(),
        Some("Result JSON Title")
    );

    assert_eq!(
        extract_session_namer_title(
            r#"{"message":{"content":[{"type":"tool_use","input":{"title":"Tool JSON Title"}}]}}"#
        )
        .as_deref(),
        Some("Tool JSON Title")
    );

    assert_eq!(
        extract_session_namer_title(
            r#"{"message":{"content":[{"type":"text","text":"Title: Text JSON Title"}]}}"#
        )
        .as_deref(),
        Some("Text JSON Title")
    );

    assert!(extract_session_namer_title(
        "{\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"\"}]}}\nmore output"
    )
    .is_none());

    let tool_parameter = r#"<invoke name="update_session_title">
<parameter name="title">Retry Claude Auto Rename</parameter>
</invoke>"#;
    assert_eq!(
        extract_session_namer_title(tool_parameter).as_deref(),
        Some("Retry Claude Auto Rename")
    );

    let generated_title_without_backticks = "Generated Title: Repair Claude Rename";
    assert_eq!(
        extract_session_namer_title(generated_title_without_backticks).as_deref(),
        Some("Repair Claude Rename")
    );

    assert_eq!(
        extract_session_namer_title("\nGenerated Title: Title After Blank").as_deref(),
        Some("Title After Blank")
    );

    assert_eq!(
        extract_session_namer_title("\nTitle: Plain Title After Blank").as_deref(),
        Some("Plain Title After Blank")
    );

    let long_title =
        "Add reliable session namer persistence despite utility agent pseudo tool output";
    assert_eq!(
        extract_session_namer_title(long_title).as_deref(),
        Some("Add reliable session namer persistence despite uti")
    );

    assert!(extract_session_namer_title("").is_none());
    assert!(extract_session_namer_title("first line\nsecond line").is_none());
    assert!(extract_session_namer_title("<invoke name=\"update_session_title\">").is_none());
    assert!(extract_session_namer_title(&"x".repeat(81)).is_none());
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
        conversation_initial(conversation.id.as_str(), "Name this Codex conversation"),
    )
    .await
    .unwrap();

    assert!(Arc::ptr_eq(&spawn.client, &codex_client));
    assert_eq!(spawn.config.harness, Some(AgentHarnessKind::Codex));
    assert_eq!(spawn.config.model.as_deref(), Some("gpt-5.4-mini"));
    assert_eq!(spawn.config.logical_effort, Some(LogicalEffort::Medium));
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
async fn session_namer_conversation_spawn_prefers_requested_harness_before_persisted_provider() {
    let default_client: Arc<dyn AgenticClient> = Arc::new(MockAgenticClient::new());
    let codex_client: Arc<dyn AgenticClient> = Arc::new(MockAgenticClient::new());
    let state = AppState::new_test()
        .with_agent_client(default_client)
        .with_harness_agent_client(AgentHarnessKind::Codex, codex_client.clone());

    let project_dir = tempfile::tempdir().unwrap();
    let project = Project::new(
        "Codex Provider Override Project".to_string(),
        project_dir.path().display().to_string(),
    );
    state.project_repo.create(project.clone()).await.unwrap();

    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project.id.clone()))
        .await
        .unwrap();

    let spawn = build_session_namer_agent_spawn(
        &state,
        conversation_initial_with_harness(
            conversation.id.as_str(),
            "Name this Codex conversation before provider persistence",
            AgentHarnessKind::Codex,
        ),
    )
    .await
    .unwrap();

    assert!(Arc::ptr_eq(&spawn.client, &codex_client));
    assert_eq!(spawn.config.harness, Some(AgentHarnessKind::Codex));
    assert_eq!(spawn.config.model.as_deref(), Some("gpt-5.4-mini"));
    assert_eq!(spawn.config.logical_effort, Some(LogicalEffort::Medium));
    assert_eq!(spawn.config.working_directory, project_dir.path());
}

#[tokio::test]
async fn session_namer_conversation_spawn_includes_review_pr_context() {
    let default_client: Arc<dyn AgenticClient> = Arc::new(MockAgenticClient::new());
    let state = AppState::new_test().with_agent_client(default_client);

    let project_dir = tempfile::tempdir().unwrap();
    let project = Project::new(
        "Review PR Naming Project".to_string(),
        project_dir.path().display().to_string(),
    );
    state.project_repo.create(project.clone()).await.unwrap();

    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project.id.clone()))
        .await
        .unwrap();
    let mut workspace = AgentConversationWorkspace::new(
        conversation.id.clone(),
        project.id.clone(),
        AgentConversationWorkspaceMode::ReviewPr,
        IdeationAnalysisBaseRefKind::LocalBranch,
        "feature/pr-review-title".to_string(),
        Some("PR #411: Add branch review metadata".to_string()),
        Some("base-sha".to_string()),
        "ralphx/project/review-pr-title".to_string(),
        project_dir.path().display().to_string(),
    );
    workspace.source_pull_request = Some(AgentWorkspaceSourcePullRequest {
        number: 411,
        url: Some("https://github.com/aigentive/ralphx.app/pull/411".to_string()),
        title: Some("Add branch review metadata".to_string()),
        head_ref_name: "feature/pr-review-title".to_string(),
        base_ref_name: Some("main".to_string()),
        head_ref_oid: Some("abcdef1234567890".to_string()),
    });
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();

    let spawn = build_session_namer_agent_spawn(
        &state,
        conversation_initial(conversation.id.as_str(), "Review this PR"),
    )
    .await
    .unwrap();

    assert!(spawn.config.prompt.contains("<review_pull_request>"));
    assert!(spawn.config.prompt.contains("<number>411</number>"));
    assert!(spawn
        .config
        .prompt
        .contains("<title>Add branch review metadata</title>"));
    assert!(spawn
        .config
        .prompt
        .contains("<head_ref_name>feature/pr-review-title</head_ref_name>"));
    assert!(spawn
        .config
        .prompt
        .contains("<base_ref_name>main</base_ref_name>"));
}

#[tokio::test]
async fn session_namer_conversation_spawn_includes_existing_context_for_forked_conversation() {
    let default_client: Arc<dyn AgenticClient> = Arc::new(MockAgenticClient::new());
    let state = AppState::new_test().with_agent_client(default_client);

    let project_dir = tempfile::tempdir().unwrap();
    let project = Project::new(
        "Forked Conversation Context Project".to_string(),
        project_dir.path().display().to_string(),
    );
    state.project_repo.create(project.clone()).await.unwrap();

    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.parent_conversation_id = Some("parent-conversation-1".to_string());
    conversation.set_title("[Fork] Stabilize workspace publish".to_string());
    let conversation = state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();

    let mut user = ChatMessage::user_in_project(
        project.id.clone(),
        "The merged run still has stale PR status in the sidebar.",
    );
    user.conversation_id = Some(conversation.id.clone());
    state.chat_message_repo.create(user).await.unwrap();

    let mut assistant = ChatMessage::user_in_project(
        project.id.clone(),
        "The prior fix updated publication polling but did not rename the fork.",
    );
    assistant.role = MessageRole::Orchestrator;
    assistant.conversation_id = Some(conversation.id.clone());
    state.chat_message_repo.create(assistant).await.unwrap();

    let spawn = build_session_namer_agent_spawn(
        &state,
        conversation_initial(
            conversation.id.as_str(),
            "Please continue from the merged run and fix the naming fallback.",
        ),
    )
    .await
    .unwrap();

    assert!(spawn.config.prompt.contains("<conversation_context>"));
    assert!(spawn
        .config
        .prompt
        .contains("<parent_conversation_id>parent-conversation-1</parent_conversation_id>"));
    assert!(spawn
        .config
        .prompt
        .contains("<current_title>[Fork] Stabilize workspace publish</current_title>"));
    assert!(spawn
        .config
        .prompt
        .contains("<content>The merged run still has stale PR status in the sidebar.</content>"));
    assert!(spawn.config.prompt.contains(
        "<content>The prior fix updated publication polling but did not rename the fork.</content>"
    ));
    assert!(spawn.config.prompt.contains(
        "<user_message>Please continue from the merged run and fix the naming fallback.</user_message>"
    ));
}

#[tokio::test]
async fn session_namer_conversation_context_skips_empty_and_truncates_long_messages() {
    let default_client: Arc<dyn AgenticClient> = Arc::new(MockAgenticClient::new());
    let state = AppState::new_test().with_agent_client(default_client);

    let project_dir = tempfile::tempdir().unwrap();
    let project = Project::new(
        "Forked Conversation Long Context Project".to_string(),
        project_dir.path().display().to_string(),
    );
    state.project_repo.create(project.clone()).await.unwrap();

    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.parent_conversation_id = Some("parent-conversation-long".to_string());
    let conversation = state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();

    let mut empty = ChatMessage::user_in_project(project.id.clone(), " \n\t ");
    empty.conversation_id = Some(conversation.id.clone());
    state.chat_message_repo.create(empty).await.unwrap();

    let long_content = format!("{} tail-marker", "alpha ".repeat(160));
    let mut long_message = ChatMessage::user_in_project(project.id.clone(), long_content);
    long_message.conversation_id = Some(conversation.id.clone());
    state.chat_message_repo.create(long_message).await.unwrap();

    let spawn = build_session_namer_agent_spawn(
        &state,
        conversation_initial(conversation.id.as_str(), "Name the fork after a follow-up"),
    )
    .await
    .unwrap();

    assert!(spawn.config.prompt.contains("<recent_messages>"));
    assert!(!spawn.config.prompt.contains("<content></content>"));
    assert!(spawn.config.prompt.contains("alpha alpha alpha"));
    assert!(spawn.config.prompt.contains("...</content>"));
    assert!(!spawn.config.prompt.contains("tail-marker"));
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
    assert_eq!(spawn.config.model.as_deref(), Some("gpt-5.4-mini"));
    assert_eq!(spawn.config.logical_effort, Some(LogicalEffort::Medium));
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
async fn session_namer_fire_and_forget_persists_generated_conversation_title() {
    let concrete_client = Arc::new(SuccessfulSessionNamerClient::new(
        r#"{"type":"result","result":"**Generated Title:** `Test session namer`"}"#,
    ));
    let agent_client: Arc<dyn AgenticClient> = concrete_client;
    let state = AppState::new_test().with_agent_client(agent_client);

    let project_dir = tempfile::tempdir().unwrap();
    let project = Project::new(
        "Generated Conversation Title Project".to_string(),
        project_dir.path().display().to_string(),
    );
    state.project_repo.create(project.clone()).await.unwrap();
    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.set_title("Discuss just a test".to_string());
    let conversation = state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();

    spawn_session_namer_agent(
        &state,
        conversation_initial(conversation.id.as_str(), "just a test"),
    )
    .await
    .unwrap();

    let mut updated_title = None;
    for _ in 0..20 {
        updated_title = state
            .chat_conversation_repo
            .get_by_id(&conversation.id)
            .await
            .unwrap()
            .and_then(|conversation| conversation.title);
        if updated_title.as_deref() == Some("Test session namer") {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    assert_eq!(updated_title.as_deref(), Some("Test session namer"));
}

#[tokio::test]
async fn session_namer_fire_and_forget_persists_generated_session_title() {
    let concrete_client = Arc::new(SuccessfulSessionNamerClient::new(
        r#"{"title":"Repair session naming"}"#,
    ));
    let agent_client: Arc<dyn AgenticClient> = concrete_client;
    let state = AppState::new_test().with_agent_client(agent_client);

    let project_dir = tempfile::tempdir().unwrap();
    let project = Project::new(
        "Generated Session Title Project".to_string(),
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
        SessionNamerTarget::session_initial(session.id.as_str(), "session namer issue"),
    )
    .await
    .unwrap();

    let mut updated = None;
    for _ in 0..20 {
        updated = state
            .ideation_session_repo
            .get_by_id(&session.id)
            .await
            .unwrap();
        if updated
            .as_ref()
            .and_then(|session| session.title.as_deref())
            == Some("Repair session naming")
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    let updated = updated.expect("session remains persisted");
    assert_eq!(updated.title.as_deref(), Some("Repair session naming"));
    assert_eq!(updated.title_source.as_deref(), Some("auto"));
}

#[tokio::test]
async fn session_namer_fire_and_forget_ignores_unparseable_output_without_overwriting_title() {
    let concrete_client = Arc::new(SuccessfulSessionNamerClient::new(
        "first line\nsecond line without a title",
    ));
    let agent_client: Arc<dyn AgenticClient> = concrete_client;
    let state = AppState::new_test().with_agent_client(agent_client);

    let project_dir = tempfile::tempdir().unwrap();
    let project = Project::new(
        "Unparseable Session Title Project".to_string(),
        project_dir.path().display().to_string(),
    );
    state.project_repo.create(project.clone()).await.unwrap();
    let session = state
        .ideation_session_repo
        .create(
            IdeationSession::builder()
                .project_id(project.id.clone())
                .title("Existing Session Title")
                .title_source("user")
                .build(),
        )
        .await
        .unwrap();

    spawn_session_namer_agent(
        &state,
        SessionNamerTarget::session_initial(session.id.as_str(), "keep existing title"),
    )
    .await
    .unwrap();

    for _ in 0..20 {
        let updated = state
            .ideation_session_repo
            .get_by_id(&session.id)
            .await
            .unwrap()
            .expect("session remains persisted");
        if updated.title.as_deref() != Some("Existing Session Title") {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    let updated = state
        .ideation_session_repo
        .get_by_id(&session.id)
        .await
        .unwrap()
        .expect("session remains persisted");
    assert_eq!(updated.title.as_deref(), Some("Existing Session Title"));
    assert_eq!(updated.title_source.as_deref(), Some("user"));
}

#[tokio::test]
async fn session_namer_fire_and_forget_syncs_linked_planning_session_title() {
    let concrete_client = Arc::new(SuccessfulSessionNamerClient::new("Review CLI gaps"));
    let agent_client: Arc<dyn AgenticClient> = concrete_client;
    let state = AppState::new_test().with_agent_client(agent_client);

    let project_dir = tempfile::tempdir().unwrap();
    let project = Project::new(
        "Linked Planning Title Project".to_string(),
        project_dir.path().display().to_string(),
    );
    state.project_repo.create(project.clone()).await.unwrap();
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project.id.clone()))
        .await
        .unwrap();
    let session = state
        .ideation_session_repo
        .create(
            IdeationSession::builder()
                .project_id(project.id.clone())
                .session_flow(IdeationSessionFlow::Planning)
                .source_context_type("agent_conversation")
                .source_context_id(conversation.id.as_str())
                .build(),
        )
        .await
        .unwrap();
    let mut workspace = AgentConversationWorkspace::new(
        conversation.id.clone(),
        project.id.clone(),
        AgentConversationWorkspaceMode::Plan,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        Some("base-sha".to_string()),
        "ralphx/project/linked-planning-title".to_string(),
        project_dir.path().display().to_string(),
    );
    workspace.linked_ideation_session_id = Some(session.id.clone());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();

    spawn_session_namer_agent(
        &state,
        conversation_initial(conversation.id.as_str(), "Review CLI coverage gaps"),
    )
    .await
    .unwrap();

    let mut conversation_title = None;
    let mut session_title = None;
    for _ in 0..20 {
        conversation_title = state
            .chat_conversation_repo
            .get_by_id(&conversation.id)
            .await
            .unwrap()
            .and_then(|conversation| conversation.title);
        session_title = state
            .ideation_session_repo
            .get_by_id(&session.id)
            .await
            .unwrap()
            .and_then(|session| session.title);
        if conversation_title.as_deref() == Some("Review CLI gaps")
            && session_title.as_deref() == Some("Review CLI gaps")
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    assert_eq!(conversation_title.as_deref(), Some("Review CLI gaps"));
    assert_eq!(session_title.as_deref(), Some("Review CLI gaps"));
}

#[tokio::test]
async fn session_namer_fire_and_forget_keeps_user_named_linked_planning_session_title() {
    let concrete_client = Arc::new(SuccessfulSessionNamerClient::new(
        "Fresh Conversation Title",
    ));
    let agent_client: Arc<dyn AgenticClient> = concrete_client;
    let state = AppState::new_test().with_agent_client(agent_client);

    let project_dir = tempfile::tempdir().unwrap();
    let project = Project::new(
        "User Named Linked Planning Title Project".to_string(),
        project_dir.path().display().to_string(),
    );
    state.project_repo.create(project.clone()).await.unwrap();
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project.id.clone()))
        .await
        .unwrap();
    let session = state
        .ideation_session_repo
        .create(
            IdeationSession::builder()
                .project_id(project.id.clone())
                .title("User Plan Title")
                .title_source("user")
                .session_flow(IdeationSessionFlow::Planning)
                .source_context_type("agent_conversation")
                .source_context_id(conversation.id.as_str())
                .build(),
        )
        .await
        .unwrap();
    let mut workspace = AgentConversationWorkspace::new(
        conversation.id.clone(),
        project.id.clone(),
        AgentConversationWorkspaceMode::Plan,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        Some("base-sha".to_string()),
        "ralphx/project/user-named-linked-planning-title".to_string(),
        project_dir.path().display().to_string(),
    );
    workspace.linked_ideation_session_id = Some(session.id.clone());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();

    spawn_session_namer_agent(
        &state,
        conversation_initial(conversation.id.as_str(), "Retitle conversation only"),
    )
    .await
    .unwrap();

    let mut conversation_title = None;
    for _ in 0..20 {
        conversation_title = state
            .chat_conversation_repo
            .get_by_id(&conversation.id)
            .await
            .unwrap()
            .and_then(|conversation| conversation.title);
        if conversation_title.as_deref() == Some("Fresh Conversation Title") {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    let session = state
        .ideation_session_repo
        .get_by_id(&session.id)
        .await
        .unwrap()
        .expect("session remains persisted");
    assert_eq!(
        conversation_title.as_deref(),
        Some("Fresh Conversation Title")
    );
    assert_eq!(session.title.as_deref(), Some("User Plan Title"));
    assert_eq!(session.title_source.as_deref(), Some("user"));
}

#[tokio::test]
async fn session_namer_fire_and_forget_skips_linked_session_when_workspace_has_no_planning_session()
{
    for case in [
        "missing_link",
        "missing_session",
        "non_planning",
        "non_agent_source",
    ] {
        let concrete_client = Arc::new(SuccessfulSessionNamerClient::new(format!(
            "Conversation Title {case}"
        )));
        let agent_client: Arc<dyn AgenticClient> = concrete_client;
        let state = AppState::new_test().with_agent_client(agent_client);

        let project_dir = tempfile::tempdir().unwrap();
        let project = Project::new(
            format!("Linked Session Skip Project {case}"),
            project_dir.path().display().to_string(),
        );
        state.project_repo.create(project.clone()).await.unwrap();
        let conversation = state
            .chat_conversation_repo
            .create(ChatConversation::new_project(project.id.clone()))
            .await
            .unwrap();

        let linked_session = match case {
            "missing_link" | "missing_session" => None,
            "non_planning" => Some(
                state
                    .ideation_session_repo
                    .create(
                        IdeationSession::builder()
                            .project_id(project.id.clone())
                            .session_flow(IdeationSessionFlow::Ideation)
                            .source_context_type("agent_conversation")
                            .source_context_id(conversation.id.as_str())
                            .build(),
                    )
                    .await
                    .unwrap(),
            ),
            "non_agent_source" => Some(
                state
                    .ideation_session_repo
                    .create(
                        IdeationSession::builder()
                            .project_id(project.id.clone())
                            .session_flow(IdeationSessionFlow::Planning)
                            .source_context_type("project")
                            .source_context_id(project.id.as_str())
                            .build(),
                    )
                    .await
                    .unwrap(),
            ),
            _ => unreachable!(),
        };

        let mut workspace = AgentConversationWorkspace::new(
            conversation.id.clone(),
            project.id.clone(),
            AgentConversationWorkspaceMode::Plan,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("main".to_string()),
            Some("base-sha".to_string()),
            format!("ralphx/project/linked-session-skip-{case}"),
            project_dir.path().display().to_string(),
        );
        workspace.linked_ideation_session_id = match (case, linked_session.as_ref()) {
            ("missing_link", _) => None,
            ("missing_session", _) => Some(IdeationSessionId::from_string(
                "missing-linked-session".to_string(),
            )),
            (_, Some(session)) => Some(session.id.clone()),
            _ => None,
        };
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .unwrap();

        spawn_session_namer_agent(
            &state,
            conversation_initial(
                conversation.id.as_str(),
                format!("rename conversation with {case} linked session"),
            ),
        )
        .await
        .unwrap();

        let expected_title = format!("Conversation Title {case}");
        for _ in 0..20 {
            let conversation_title = state
                .chat_conversation_repo
                .get_by_id(&conversation.id)
                .await
                .unwrap()
                .and_then(|conversation| conversation.title);
            if conversation_title.as_deref() == Some(expected_title.as_str()) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        let conversation_title = state
            .chat_conversation_repo
            .get_by_id(&conversation.id)
            .await
            .unwrap()
            .and_then(|conversation| conversation.title);
        assert_eq!(conversation_title.as_deref(), Some(expected_title.as_str()));

        if let Some(session) = linked_session {
            let updated_session = state
                .ideation_session_repo
                .get_by_id(&session.id)
                .await
                .unwrap()
                .expect("linked session remains persisted");
            assert_eq!(updated_session.title, None);
            assert_eq!(updated_session.title_source, None);
        }
    }
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
        conversation_initial(conversation.id.as_str(), "Name this ideation conversation"),
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
        conversation_initial(conversation.id.as_str(), "Name this task conversation"),
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
        conversation_initial(conversation.id.as_str(), "Name this delegated conversation"),
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
        conversation_initial("missing-conversation", "Name this"),
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
        conversation_initial(conversation.id.as_str(), "Name this legacy conversation"),
    )
    .await
    .unwrap();

    assert_eq!(
        spawn.config.working_directory,
        default_repo_root_working_directory()
    );
    assert_eq!(spawn.project_id, None);
}

#[tokio::test]
async fn session_namer_standalone_conversation_resolves_with_project_id_none() {
    let default_client: Arc<dyn AgenticClient> = Arc::new(MockAgenticClient::new());
    let app_data_dir = tempfile::tempdir().unwrap();
    let mut state = AppState::new_test().with_agent_client(default_client);
    state.app_paths = AppPaths::new(app_data_dir.path(), None);
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_standalone())
        .await
        .unwrap();
    let expected = crate::application::standalone_workspace::create_workspace(
        app_data_dir.path(),
        &conversation.id.as_str(),
    )
    .unwrap();

    let spawn = build_session_namer_agent_spawn(
        &state,
        conversation_initial(conversation.id.as_str(), "Name this standalone chat"),
    )
    .await
    .unwrap();

    assert_eq!(spawn.config.working_directory, expected);
    assert_eq!(spawn.project_id, None);
}

#[tokio::test]
async fn session_namer_standalone_workspace_failure_skips_spawn() {
    let concrete_client = Arc::new(MockAgenticClient::new());
    let agent_client: Arc<dyn AgenticClient> = concrete_client.clone();
    let temp = tempfile::tempdir().unwrap();
    let blocked_app_data_dir = temp.path().join("blocked-app-data");
    std::fs::write(&blocked_app_data_dir, b"not a directory").unwrap();
    let mut state = AppState::new_test().with_agent_client(agent_client);
    state.app_paths = AppPaths::new(blocked_app_data_dir, None);
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_standalone())
        .await
        .unwrap();

    spawn_session_namer_agent(
        &state,
        conversation_initial(conversation.id.as_str(), "Do not spawn from process CWD"),
    )
    .await
    .expect("unavailable standalone workspace should be a logged skip");

    tokio::task::yield_now().await;
    assert!(concrete_client.get_calls().await.is_empty());
}

#[test]
fn session_namer_initial_request_target_requires_exactly_one_target_id() {
    let session_target = SessionNamerTarget::from_initial_request(
        Some("session-1".to_string()),
        None,
        "Name session".to_string(),
        None,
        None,
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
        None,
        None,
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
        None,
        None,
    )
    .is_err());
    assert!(SessionNamerTarget::from_initial_request(
        None,
        None,
        "missing".to_string(),
        None,
        None
    )
    .is_err());
}
