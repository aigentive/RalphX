use crate::application::app_state::ResolvedBackgroundAgentRuntime;
use crate::application::memory_capture_service::{
    MemoryCaptureInput, MemoryCaptureService, MemoryCaptureUpsertCommand, MemoryCaptureUpsertPort,
    MemoryCaptureUpsertResult,
};
use crate::application::memory_orchestration::*;
use crate::domain::agents::{
    AgentConfig, AgentHandle, AgentHarnessKind, AgentOutput, AgentResponse, AgentResult,
    AgenticClient, ClientCapabilities, ResponseChunk,
};
use crate::domain::entities::{
    ChatContextType, ChatConversationId, MemoryActorType, ProjectId, ProjectMemorySettings,
};
use crate::domain::repositories::{
    MemoryEntryRepository, MemoryEventRepository, ProjectMemorySettingsRepository,
};
use crate::error::{AppError, AppResult};
use crate::infrastructure::agents::mcp_runtime_context::append_mcp_runtime_args;
use crate::infrastructure::memory::{
    InMemoryMemoryEntryRepository, InMemoryMemoryEventRepository,
    MemoryProjectMemorySettingsRepository,
};
use crate::infrastructure::sqlite::{SqliteMemoryEntryRepository, SqliteMemoryEventRepository};
use crate::testing::SqliteTestDb;
use async_trait::async_trait;
use futures::Stream;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, Notify};

#[test]
fn test_context_to_category_mapping() {
    assert_eq!(
        MemoryCategory::from_context_type(ChatContextType::Ideation),
        MemoryCategory::Planning
    );
    assert_eq!(
        MemoryCategory::from_context_type(ChatContextType::Task),
        MemoryCategory::Execution
    );
    assert_eq!(
        MemoryCategory::from_context_type(ChatContextType::TaskExecution),
        MemoryCategory::Execution
    );
    assert_eq!(
        MemoryCategory::from_context_type(ChatContextType::Delegation),
        MemoryCategory::Execution
    );
    assert_eq!(
        MemoryCategory::from_context_type(ChatContextType::Review),
        MemoryCategory::Review
    );
    assert_eq!(
        MemoryCategory::from_context_type(ChatContextType::Merge),
        MemoryCategory::Merge
    );
    assert_eq!(
        MemoryCategory::from_context_type(ChatContextType::Project),
        MemoryCategory::ProjectChat
    );
}

#[test]
fn test_skip_reason_as_str() {
    assert_eq!(
        MemoryPipelineSkipReason::NoProjectId.as_str(),
        "no_project_id"
    );
    assert_eq!(
        MemoryPipelineSkipReason::RecursionGuard.as_str(),
        "recursion_guard"
    );
    assert_eq!(MemoryPipelineSkipReason::Disabled.as_str(), "disabled");
    assert_eq!(
        MemoryPipelineSkipReason::NoEnabledCategory.as_str(),
        "no_enabled_category"
    );
}

#[test]
fn test_category_as_str() {
    assert_eq!(MemoryCategory::Planning.as_str(), "planning");
    assert_eq!(MemoryCategory::Execution.as_str(), "execution");
    assert_eq!(MemoryCategory::Review.as_str(), "review");
    assert_eq!(MemoryCategory::Merge.as_str(), "merge");
    assert_eq!(MemoryCategory::ProjectChat.as_str(), "project_chat");
}

#[test]
fn test_default_settings() {
    let project_id = ProjectId::from_string("proj-default".to_string());
    let settings = ProjectMemorySettings::default_for_project(project_id.clone());
    assert_eq!(settings.project_id, project_id);
    assert!(settings.enabled);
    assert!(settings
        .maintenance_categories
        .contains(&"execution".to_string()));
    assert!(settings
        .maintenance_categories
        .contains(&"review".to_string()));
    assert!(settings
        .maintenance_categories
        .contains(&"merge".to_string()));
    assert!(settings
        .capture_categories
        .contains(&"planning".to_string()));
    assert!(settings
        .capture_categories
        .contains(&"execution".to_string()));
    assert!(settings.capture_categories.contains(&"review".to_string()));
}

#[test]
fn test_default_settings_maintenance_categories_count() {
    let settings = ProjectMemorySettings::default_for_project(ProjectId::from_string(
        "proj-default".to_string(),
    ));
    assert_eq!(settings.maintenance_categories.len(), 3);
}

#[test]
fn test_default_settings_capture_categories_count() {
    let settings = ProjectMemorySettings::default_for_project(ProjectId::from_string(
        "proj-default".to_string(),
    ));
    assert_eq!(settings.capture_categories.len(), 3);
}

#[tokio::test]
async fn test_trigger_memory_pipelines_no_project_id() {
    let conv_id = ChatConversationId::from_string("conv-123".to_string());
    let cli_path = PathBuf::from("/usr/bin/claude");
    let plugin_dir = PathBuf::from("/plugins");
    let wd = PathBuf::from("/tmp");

    let event_repo = Arc::new(InMemoryMemoryEventRepository::new());
    let entry_repo = Arc::new(InMemoryMemoryEntryRepository::new());
    let (capture_client, runtime) = capture_test_runtime(entry_repo.clone(), event_repo.clone());

    trigger_memory_pipelines(
        ChatContextType::TaskExecution,
        "task-123",
        &conv_id,
        None, // No project ID
        None,
        &cli_path,
        &plugin_dir,
        &wd,
        None,
        Some(event_repo.clone()),
        None,
        Some(runtime),
        None,
    )
    .await;

    let unused_project = ProjectId::from_string("unused".to_string());
    assert!(event_repo
        .get_by_type("memory_pipeline_skipped")
        .await
        .unwrap()
        .is_empty());
    assert!(event_repo
        .get_by_project(&unused_project)
        .await
        .unwrap()
        .is_empty());
    assert!(entry_repo
        .get_by_project(&unused_project)
        .await
        .unwrap()
        .is_empty());
    assert_eq!(capture_client.spawn_count(), 0);
    assert_eq!(capture_client.upsert_count(), 0);
}

#[tokio::test]
async fn test_trigger_memory_pipelines_recursion_guard_normalizes_memory_agent_names() {
    let project_id = ProjectId::from_string("proj-recursion".to_string());
    let conv_id = ChatConversationId::from_string("conv-recursion".to_string());
    let cli_path = PathBuf::from("/usr/bin/claude");
    let plugin_dir = PathBuf::from("/plugins");
    let wd = PathBuf::from("/tmp");
    let settings = ProjectMemorySettings::default_for_project(project_id.clone());

    for name in [
        "ralphx-memory-maintainer",
        "ralphx:ralphx-memory-maintainer",
        "ralphx-memory-capture",
        "ralphx:ralphx-memory-capture",
    ] {
        let event_repo = Arc::new(InMemoryMemoryEventRepository::new());
        let entry_repo = Arc::new(InMemoryMemoryEntryRepository::new());
        let (capture_client, runtime) =
            capture_test_runtime(entry_repo.clone(), event_repo.clone());

        trigger_memory_pipelines(
            ChatContextType::TaskExecution,
            "task-recursion",
            &conv_id,
            Some(&project_id),
            Some(name),
            &cli_path,
            &plugin_dir,
            &wd,
            Some(settings.clone()),
            Some(event_repo.clone()),
            None,
            Some(runtime),
            None,
        )
        .await;

        let events = event_repo
            .get_by_type("memory_pipeline_skipped")
            .await
            .unwrap();
        assert_eq!(events.len(), 1, "expected one skip for {name}");
        assert_eq!(events[0].details["reason"], "recursion_guard");
        assert_eq!(capture_client.spawn_count(), 0, "spawned for {name}");
        assert_eq!(capture_client.upsert_count(), 0, "upserted for {name}");
        assert!(entry_repo
            .get_by_project(&project_id)
            .await
            .unwrap()
            .is_empty());
        assert!(event_repo
            .get_by_type("memory_capture_decision")
            .await
            .unwrap()
            .is_empty());
    }
}

#[test]
fn test_resolve_pipelines_parallel_spawn_both_enabled() {
    // "execution" is in both maintenance_categories AND capture_categories by default
    let project_id = ProjectId::from_string("proj-123".to_string());
    let settings = ProjectMemorySettings::default_for_project(project_id.clone());

    let result = resolve_pipelines(
        ChatContextType::TaskExecution,
        Some(&project_id),
        Some("ralphx:ralphx-execution-worker"),
        &settings,
    );

    assert!(
        result.is_some(),
        "Should return Some when category is enabled"
    );
    let (should_maintain, should_capture) = result.unwrap();
    assert!(
        should_maintain,
        "execution should be in maintenance_categories"
    );
    assert!(should_capture, "execution should be in capture_categories");
}

#[test]
fn test_resolve_pipelines_disabled_project_skips_spawn() {
    let project_id = ProjectId::from_string("proj-123".to_string());
    let settings = ProjectMemorySettings {
        project_id: project_id.clone(),
        enabled: false,
        maintenance_categories: vec!["execution".to_string()],
        capture_categories: vec!["execution".to_string()],
    };

    let result = resolve_pipelines(
        ChatContextType::TaskExecution,
        Some(&project_id),
        Some("ralphx:ralphx-execution-worker"),
        &settings,
    );

    assert!(
        result.is_none(),
        "Should return None when memory is disabled"
    );
}

#[test]
fn test_resolve_pipelines_with_reason_reports_skip_reasons() {
    let project_id = ProjectId::from_string("proj-skip-reasons".to_string());
    let enabled_settings = ProjectMemorySettings {
        project_id: project_id.clone(),
        enabled: true,
        maintenance_categories: vec!["execution".to_string()],
        capture_categories: Vec::new(),
    };

    assert_eq!(
        resolve_pipelines_with_reason(
            ChatContextType::TaskExecution,
            None,
            Some("ralphx:ralphx-execution-worker"),
            &enabled_settings,
        ),
        Err(MemoryPipelineSkipReason::NoProjectId)
    );
    assert_eq!(
        resolve_pipelines_with_reason(
            ChatContextType::TaskExecution,
            Some(&project_id),
            Some("ralphx-memory-capture"),
            &enabled_settings,
        ),
        Err(MemoryPipelineSkipReason::RecursionGuard)
    );

    let disabled_settings = ProjectMemorySettings {
        enabled: false,
        ..enabled_settings.clone()
    };
    assert_eq!(
        resolve_pipelines_with_reason(
            ChatContextType::TaskExecution,
            Some(&project_id),
            Some("ralphx:ralphx-execution-worker"),
            &disabled_settings,
        ),
        Err(MemoryPipelineSkipReason::Disabled)
    );

    assert_eq!(
        resolve_pipelines_with_reason(
            ChatContextType::Project,
            Some(&project_id),
            Some("ralphx:ralphx-chat"),
            &enabled_settings,
        ),
        Err(MemoryPipelineSkipReason::NoEnabledCategory)
    );
}

#[test]
fn test_resolve_pipelines_separates_maintenance_and_capture_categories() {
    let project_id = ProjectId::from_string("proj-category-split".to_string());
    let settings = ProjectMemorySettings {
        project_id: project_id.clone(),
        enabled: true,
        maintenance_categories: vec!["review".to_string()],
        capture_categories: vec!["project_chat".to_string()],
    };

    assert_eq!(
        resolve_pipelines_with_reason(
            ChatContextType::Review,
            Some(&project_id),
            Some("ralphx:ralphx-execution-reviewer"),
            &settings,
        ),
        Ok((true, false))
    );
    assert_eq!(
        resolve_pipelines_with_reason(
            ChatContextType::Project,
            Some(&project_id),
            Some("ralphx:ralphx-chat"),
            &settings,
        ),
        Ok((false, true))
    );
}

#[tokio::test]
async fn test_trigger_memory_pipelines_uses_repository_settings_and_logs_disabled_skip() {
    let project_id = ProjectId::from_string("proj-repo-settings".to_string());
    let conv_id = ChatConversationId::from_string("conv-repo-settings".to_string());
    let cli_path = PathBuf::from("/usr/bin/claude");
    let plugin_dir = PathBuf::from("/plugins");
    let wd = PathBuf::from("/tmp");

    let settings_repo = Arc::new(MemoryProjectMemorySettingsRepository::new());
    settings_repo
        .insert(ProjectMemorySettings {
            project_id: project_id.clone(),
            enabled: false,
            maintenance_categories: vec!["execution".to_string()],
            capture_categories: vec!["execution".to_string()],
        })
        .await;
    let event_repo = Arc::new(InMemoryMemoryEventRepository::new());
    let entry_repo = Arc::new(InMemoryMemoryEntryRepository::new());
    let (capture_client, runtime) = capture_test_runtime(entry_repo.clone(), event_repo.clone());

    trigger_memory_pipelines(
        ChatContextType::TaskExecution,
        "task-123",
        &conv_id,
        Some(&project_id),
        Some("ralphx:ralphx-execution-worker"),
        &cli_path,
        &plugin_dir,
        &wd,
        None,
        Some(event_repo.clone()),
        Some(settings_repo),
        Some(runtime),
        None,
    )
    .await;

    let events = event_repo
        .get_by_type("memory_pipeline_skipped")
        .await
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].project_id, project_id);
    assert_eq!(events[0].details["reason"], "disabled");
    assert_eq!(events[0].details["conversation_id"], conv_id.as_str());
    assert_eq!(capture_client.spawn_count(), 0);
    assert_eq!(capture_client.upsert_count(), 0);
    assert!(entry_repo
        .get_by_project(&project_id)
        .await
        .unwrap()
        .is_empty());
    assert!(event_repo
        .get_by_type("memory_capture_decision")
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn test_trigger_memory_pipelines_logs_no_enabled_category_skip() {
    let project_id = ProjectId::from_string("proj-no-category".to_string());
    let conv_id = ChatConversationId::from_string("conv-no-category".to_string());
    let cli_path = PathBuf::from("/usr/bin/claude");
    let plugin_dir = PathBuf::from("/plugins");
    let wd = PathBuf::from("/tmp");
    let event_repo = Arc::new(InMemoryMemoryEventRepository::new());
    let entry_repo = Arc::new(InMemoryMemoryEntryRepository::new());
    let (capture_client, runtime) = capture_test_runtime(entry_repo.clone(), event_repo.clone());

    trigger_memory_pipelines(
        ChatContextType::Project,
        "conversation-project",
        &conv_id,
        Some(&project_id),
        Some("ralphx:ralphx-chat"),
        &cli_path,
        &plugin_dir,
        &wd,
        Some(ProjectMemorySettings {
            project_id: project_id.clone(),
            enabled: true,
            maintenance_categories: vec!["execution".to_string()],
            capture_categories: Vec::new(),
        }),
        Some(event_repo.clone()),
        None,
        Some(runtime),
        None,
    )
    .await;

    let events = event_repo
        .get_by_type("memory_pipeline_skipped")
        .await
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].details["reason"], "no_enabled_category");
    assert_eq!(events[0].details["context_type"], "project");
    assert_eq!(capture_client.spawn_count(), 0);
    assert_eq!(capture_client.upsert_count(), 0);
    assert!(entry_repo
        .get_by_project(&project_id)
        .await
        .unwrap()
        .is_empty());
    assert!(event_repo
        .get_by_type("memory_capture_decision")
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn test_trigger_memory_pipelines_uses_provided_settings_before_repository_settings() {
    let project_id = ProjectId::from_string("proj-provided-settings".to_string());
    let conv_id = ChatConversationId::from_string("conv-provided-settings".to_string());
    let cli_path = PathBuf::from("/usr/bin/claude");
    let plugin_dir = PathBuf::from("/plugins");
    let wd = PathBuf::from("/tmp");
    let settings_repo = Arc::new(MemoryProjectMemorySettingsRepository::new());
    settings_repo
        .insert(ProjectMemorySettings {
            project_id: project_id.clone(),
            enabled: false,
            maintenance_categories: vec!["planning".to_string()],
            capture_categories: vec!["planning".to_string()],
        })
        .await;
    let event_repo = Arc::new(InMemoryMemoryEventRepository::new());

    trigger_memory_pipelines(
        ChatContextType::Ideation,
        "session-provided",
        &conv_id,
        Some(&project_id),
        Some("ralphx:ralphx-planner"),
        &cli_path,
        &plugin_dir,
        &wd,
        Some(ProjectMemorySettings {
            project_id: project_id.clone(),
            enabled: true,
            maintenance_categories: Vec::new(),
            capture_categories: vec!["planning".to_string()],
        }),
        Some(event_repo.clone()),
        Some(settings_repo),
        None,
        None,
    )
    .await;

    let skipped = event_repo
        .get_by_type("memory_pipeline_skipped")
        .await
        .unwrap();
    assert!(skipped.is_empty());
    let spawned = event_repo
        .get_by_type("memory_pipeline_spawn_requested")
        .await
        .unwrap();
    assert_eq!(spawned.len(), 1);
    assert_eq!(spawned[0].actor_type, MemoryActorType::MemoryCapture);
}

#[tokio::test]
async fn test_trigger_memory_pipelines_logs_enabled_capture_spawn_request() {
    let project_id = ProjectId::from_string("proj-capture-enabled".to_string());
    let conv_id = ChatConversationId::from_string("conv-capture-enabled".to_string());
    let cli_path = PathBuf::from("/usr/bin/claude");
    let plugin_dir = PathBuf::from("/plugins");
    let wd = PathBuf::from("/tmp");
    let event_repo = Arc::new(InMemoryMemoryEventRepository::new());

    trigger_memory_pipelines(
        ChatContextType::Ideation,
        "session-123",
        &conv_id,
        Some(&project_id),
        Some("ralphx:ralphx-planner"),
        &cli_path,
        &plugin_dir,
        &wd,
        Some(ProjectMemorySettings {
            project_id: project_id.clone(),
            enabled: true,
            maintenance_categories: Vec::new(),
            capture_categories: vec!["planning".to_string()],
        }),
        Some(event_repo.clone()),
        None,
        None,
        None,
    )
    .await;

    let events = event_repo
        .get_by_type("memory_pipeline_spawn_requested")
        .await
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].project_id, project_id);
    assert_eq!(events[0].actor_type, MemoryActorType::MemoryCapture);
    assert_eq!(events[0].details["agent"], "ralphx-memory-capture");
    assert_eq!(events[0].details["conversation_id"], conv_id.as_str());
    assert_eq!(events[0].details["context_type"], "ideation");
    assert_eq!(events[0].details["context_id"], "session-123");
}

struct CaptureCompletingClient {
    config: Mutex<Option<AgentConfig>>,
    upsert_port: Arc<dyn MemoryCaptureUpsertPort>,
    completion_result: Mutex<Option<AppResult<MemoryCaptureUpsertResult>>>,
    completion: Notify,
    capabilities: ClientCapabilities,
    spawn_calls: AtomicUsize,
    wait_calls: AtomicUsize,
    upsert_calls: AtomicUsize,
}

impl CaptureCompletingClient {
    fn new(upsert_port: Arc<dyn MemoryCaptureUpsertPort>) -> Self {
        Self {
            config: Mutex::new(None),
            upsert_port,
            completion_result: Mutex::new(None),
            completion: Notify::new(),
            capabilities: ClientCapabilities::mock(),
            spawn_calls: AtomicUsize::new(0),
            wait_calls: AtomicUsize::new(0),
            upsert_calls: AtomicUsize::new(0),
        }
    }

    async fn wait_for_capture(&self) -> AppResult<MemoryCaptureUpsertResult> {
        tokio::time::timeout(Duration::from_secs(2), self.completion.notified())
            .await
            .expect("memory capture completion timed out");
        self.completion_result
            .lock()
            .await
            .take()
            .expect("capture client did not store its upsert result")
    }

    fn spawn_count(&self) -> usize {
        self.spawn_calls.load(Ordering::SeqCst)
    }

    fn wait_count(&self) -> usize {
        self.wait_calls.load(Ordering::SeqCst)
    }

    fn upsert_count(&self) -> usize {
        self.upsert_calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl AgenticClient for CaptureCompletingClient {
    async fn spawn_agent(&self, config: AgentConfig) -> AgentResult<AgentHandle> {
        self.spawn_calls.fetch_add(1, Ordering::SeqCst);
        let role = config.role.clone();
        *self.config.lock().await = Some(config);
        Ok(AgentHandle::mock(role))
    }

    async fn stop_agent(&self, _handle: &AgentHandle) -> AgentResult<()> {
        Ok(())
    }

    async fn wait_for_completion(&self, _handle: &AgentHandle) -> AgentResult<AgentOutput> {
        self.wait_calls.fetch_add(1, Ordering::SeqCst);
        let result =
            async {
                let config =
                    self.config.lock().await.take().ok_or_else(|| {
                        AppError::Validation("capture config missing".to_string())
                    })?;
                if config.agent.as_deref() != Some("ralphx:ralphx-memory-capture") {
                    return Err(AppError::Validation(format!(
                        "unexpected capture agent: {:?}",
                        config.agent
                    )));
                }

                let env_value = |key: &str| {
                    config
                        .env
                        .get(key)
                        .cloned()
                        .ok_or_else(|| AppError::Validation(format!("missing {key}")))
                };
                let project_id = ProjectId::from_string(env_value("RALPHX_PROJECT_ID")?);
                let context_type = env_value("RALPHX_CONTEXT_TYPE")?;
                let context_id = env_value("RALPHX_CONTEXT_ID")?;
                let conversation_id = env_value("RALPHX_CONVERSATION_ID")?;

                self.upsert_calls.fetch_add(1, Ordering::SeqCst);
                self.upsert_port
                    .upsert_memories(MemoryCaptureUpsertCommand {
                        project_id,
                        memories: vec![MemoryCaptureInput {
                        bucket: "implementation_discoveries".to_string(),
                        title: "Production-triggered memory capture".to_string(),
                        summary: "A capture completed through the spawned runtime config."
                            .to_string(),
                        details_markdown:
                            "The application production trigger supplied the capture source fields."
                                .to_string(),
                        scope_paths: vec!["src-tauri/src/application/**".to_string()],
                        source_context_type: Some(context_type),
                        source_context_id: Some(context_id),
                        source_conversation_id: Some(conversation_id),
                        quality_score: Some(0.95),
                    }],
                    })
                    .await
            }
            .await;

        *self.completion_result.lock().await = Some(result);
        self.completion.notify_one();
        Ok(AgentOutput::success("capture completed"))
    }

    async fn send_prompt(
        &self,
        _handle: &AgentHandle,
        _prompt: &str,
    ) -> AgentResult<AgentResponse> {
        Ok(AgentResponse::new("unused"))
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

fn capture_only_settings(project_id: &ProjectId) -> ProjectMemorySettings {
    ProjectMemorySettings {
        project_id: project_id.clone(),
        enabled: true,
        maintenance_categories: Vec::new(),
        capture_categories: vec!["execution".to_string()],
    }
}

fn capture_test_runtime(
    entry_repo: Arc<InMemoryMemoryEntryRepository>,
    event_repo: Arc<InMemoryMemoryEventRepository>,
) -> (Arc<CaptureCompletingClient>, ResolvedBackgroundAgentRuntime) {
    let port: Arc<dyn MemoryCaptureUpsertPort> =
        Arc::new(MemoryCaptureService::new(entry_repo, event_repo));
    let capture_client = Arc::new(CaptureCompletingClient::new(port));
    let client: Arc<dyn AgenticClient> = capture_client.clone();
    let runtime = ResolvedBackgroundAgentRuntime {
        client,
        harness: Some(AgentHarnessKind::Codex),
        model: None,
        cli_path_override: None,
        logical_effort: None,
        approval_policy: None,
        sandbox_mode: None,
        service_tier: None,
        env: Default::default(),
    };
    (capture_client, runtime)
}

async fn assert_production_trigger_captures(context_type: ChatContextType, context_id: &str) {
    let db = SqliteTestDb::new("memory-orchestration-capture");
    let project = db.seed_project("A1 memory capture proof");
    let project_id = project.id;
    let conversation_id = ChatConversationId::from_string(format!(
        "conversation-{}",
        context_type.to_string().replace('_', "-")
    ));
    let entry_repo = Arc::new(SqliteMemoryEntryRepository::from_shared(db.shared_conn()));
    let event_repo = Arc::new(SqliteMemoryEventRepository::from_shared(db.shared_conn()));
    let port: Arc<dyn MemoryCaptureUpsertPort> = Arc::new(MemoryCaptureService::new(
        entry_repo.clone(),
        event_repo.clone(),
    ));
    let capture_client = Arc::new(CaptureCompletingClient::new(port));
    let client: Arc<dyn AgenticClient> = capture_client.clone();
    let runtime = ResolvedBackgroundAgentRuntime {
        client,
        harness: Some(AgentHarnessKind::Codex),
        model: Some("gpt-5.4-mini".to_string()),
        cli_path_override: None,
        logical_effort: None,
        approval_policy: Some("never".to_string()),
        sandbox_mode: Some("danger-full-access".to_string()),
        service_tier: None,
        env: Default::default(),
    };
    let entries_before = entry_repo.get_by_project(&project_id).await.unwrap().len();
    let events_before = event_repo.get_by_project(&project_id).await.unwrap().len();

    trigger_memory_pipelines(
        context_type,
        context_id,
        &conversation_id,
        Some(&project_id),
        Some("ralphx:ralphx-execution-worker"),
        PathBuf::from("/usr/bin/claude").as_path(),
        PathBuf::from("/plugins").as_path(),
        project.working_directory.as_ref(),
        Some(capture_only_settings(&project_id)),
        Some(event_repo.clone()),
        None,
        Some(runtime),
        None,
    )
    .await;

    let result = capture_client.wait_for_capture().await.unwrap();
    assert_eq!((result.inserted, result.skipped, result.failed), (1, 0, 0));
    assert_eq!(capture_client.spawn_count(), 1);
    assert_eq!(capture_client.wait_count(), 1);
    assert_eq!(capture_client.upsert_count(), 1);

    let entries = entry_repo.get_by_project(&project_id).await.unwrap();
    assert_eq!(entries.len() - entries_before, 1);
    assert_eq!(
        entries[0].source_context_type.as_deref(),
        Some(context_type.to_string().as_str())
    );
    assert_eq!(entries[0].source_context_id.as_deref(), Some(context_id));
    assert_eq!(
        entries[0].source_conversation_id.as_deref(),
        Some(conversation_id.as_str().as_str())
    );
    assert_eq!(entries[0].quality_score, Some(0.95));

    let events = event_repo.get_by_project(&project_id).await.unwrap();
    assert_eq!(events.len() - events_before, 2);
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_type == "memory_pipeline_spawn_requested")
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_type == "memory_capture_decision")
            .count(),
        1
    );
}

#[tokio::test]
async fn production_trigger_captures_execution_memory_after_agent_completion() {
    assert_production_trigger_captures(ChatContextType::TaskExecution, "task-a1").await;
}

#[tokio::test]
async fn production_trigger_captures_branch_update_as_execution_memory() {
    assert_production_trigger_captures(ChatContextType::BranchUpdate, "branch-update-a1").await;
}

struct FailingSettingsRepository;

#[async_trait]
impl ProjectMemorySettingsRepository for FailingSettingsRepository {
    async fn get_for_project(
        &self,
        _project_id: &ProjectId,
    ) -> AppResult<Option<ProjectMemorySettings>> {
        Err(AppError::Database(
            "forced settings read failure".to_string(),
        ))
    }
}

#[tokio::test]
async fn settings_load_failure_is_typed_durable_and_fails_closed() {
    let project_id = ProjectId::from_string("project-settings-failure".to_string());
    let conversation_id =
        ChatConversationId::from_string("conversation-settings-failure".to_string());
    let entry_repo = Arc::new(InMemoryMemoryEntryRepository::new());
    let event_repo = Arc::new(InMemoryMemoryEventRepository::new());
    let port: Arc<dyn MemoryCaptureUpsertPort> = Arc::new(MemoryCaptureService::new(
        entry_repo.clone(),
        event_repo.clone(),
    ));
    let capture_client = Arc::new(CaptureCompletingClient::new(port));
    let client: Arc<dyn AgenticClient> = capture_client.clone();
    let runtime = ResolvedBackgroundAgentRuntime {
        client,
        harness: Some(AgentHarnessKind::Codex),
        model: None,
        cli_path_override: None,
        logical_effort: None,
        approval_policy: None,
        sandbox_mode: None,
        service_tier: None,
        env: Default::default(),
    };

    trigger_memory_pipelines(
        ChatContextType::TaskExecution,
        "task-settings-failure",
        &conversation_id,
        Some(&project_id),
        Some("ralphx:ralphx-execution-worker"),
        PathBuf::from("/usr/bin/claude").as_path(),
        PathBuf::from("/plugins").as_path(),
        PathBuf::from("/tmp").as_path(),
        None,
        Some(event_repo.clone()),
        Some(Arc::new(FailingSettingsRepository)),
        Some(runtime),
        None,
    )
    .await;

    assert_eq!(
        MemoryPipelineSkipReason::SettingsLoadFailed.as_str(),
        "settings_load_failed"
    );
    let skipped = event_repo
        .get_by_type("memory_pipeline_skipped")
        .await
        .unwrap();
    assert_eq!(skipped.len(), 1);
    assert_eq!(skipped[0].details["reason"], "settings_load_failed");
    assert_eq!(
        skipped[0].details["error"],
        "Database error: forced settings read failure"
    );
    assert_eq!(capture_client.spawn_count(), 0);
    assert_eq!(capture_client.wait_count(), 0);
    assert_eq!(capture_client.upsert_count(), 0);
    assert!(entry_repo
        .get_by_project(&project_id)
        .await
        .unwrap()
        .is_empty());
    assert!(event_repo
        .get_by_type("memory_capture_decision")
        .await
        .unwrap()
        .is_empty());
}

#[test]
fn memory_launch_context_propagates_parent_through_env_and_explicit_mcp_args() {
    let parent_conversation_id = "11111111-1111-4111-8111-111111111111";
    let conversation_id = ChatConversationId::from_string(parent_conversation_id.to_string());
    let project_id = ProjectId::from_string("project-1".to_string());
    let launch = prepare_memory_agent_launch(
        &conversation_id,
        ChatContextType::TaskExecution,
        "task-1",
        &project_id,
        PathBuf::from("/trusted/workspace").as_path(),
        Some("memory_capture"),
    )
    .expect("memory launch context");

    assert_eq!(
        launch
            .env
            .get("RALPHX_PARENT_CONVERSATION_ID")
            .map(String::as_str),
        Some(parent_conversation_id)
    );
    assert_eq!(
        launch.runtime_context.parent_conversation_id.as_deref(),
        Some(parent_conversation_id)
    );
    assert_eq!(
        launch.runtime_context.conversation_id.as_deref(),
        Some(parent_conversation_id)
    );
    assert_eq!(
        launch.runtime_context.pipeline_role.as_deref(),
        Some("memory_capture")
    );
    assert_eq!(
        launch.env.get("RALPHX_PIPELINE_ROLE").map(String::as_str),
        Some("memory_capture")
    );

    let mut args = Vec::new();
    append_mcp_runtime_args(&mut args, Some(&launch.runtime_context));
    assert!(args
        .windows(2)
        .any(|pair| { pair == ["--parent-conversation-id", parent_conversation_id] }));
    assert!(args
        .windows(2)
        .any(|pair| { pair == ["--conversation-id", parent_conversation_id] }));
    assert!(args
        .windows(2)
        .any(|pair| { pair == ["--pipeline-role", "memory_capture"] }));
    assert!(
        !launch.env.contains_key("RALPHX_TASK_ID"),
        "memory context IDs must not be reclassified as task identity"
    );
}

#[test]
fn memory_runtime_configs_propagate_parent_for_all_memory_roles() {
    let entry_repo = Arc::new(InMemoryMemoryEntryRepository::new());
    let event_repo = Arc::new(InMemoryMemoryEventRepository::new());
    let (_client, mut runtime) = capture_test_runtime(entry_repo, event_repo);
    runtime
        .env
        .insert("CUSTOM_PROVIDER_TOKEN".to_string(), "preserved".to_string());
    runtime.env.insert(
        "RALPHX_PARENT_CONVERSATION_ID".to_string(),
        "spoofed-parent".to_string(),
    );
    let parent_conversation_id = "11111111-1111-4111-8111-111111111111";
    let conversation_id = ChatConversationId::from_string(parent_conversation_id.to_string());
    let project_id = ProjectId::from_string("project-1".to_string());

    for kind in [
        MemoryAgentKind::Maintainer,
        MemoryAgentKind::Capture,
        MemoryAgentKind::Distiller,
    ] {
        let config = build_memory_agent_config(
            kind,
            &runtime,
            "memory prompt".to_string(),
            &conversation_id,
            ChatContextType::TaskExecution,
            "task-1",
            &project_id,
            PathBuf::from("/trusted/workspace").as_path(),
        )
        .expect("memory runtime config");

        assert_eq!(config.agent.as_deref(), Some(kind.agent_name()));
        assert_eq!(
            config.env.get("CUSTOM_PROVIDER_TOKEN").map(String::as_str),
            Some("preserved")
        );
        assert_eq!(
            config
                .env
                .get("RALPHX_PARENT_CONVERSATION_ID")
                .map(String::as_str),
            Some(parent_conversation_id)
        );
        assert_eq!(
            config.env.get("RALPHX_PIPELINE_ROLE").map(String::as_str),
            Some(kind.pipeline_role())
        );
    }
}
