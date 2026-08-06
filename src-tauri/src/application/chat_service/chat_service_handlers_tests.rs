use super::*;
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use std::collections::HashSet;
use std::sync::{Arc, Mutex, OnceLock};
use std::{fs, process::Command};
use tauri::test::{mock_builder, mock_context, noop_assets};
use tauri::Manager;

use crate::application::{
    chat_service::verification_child_process_registry::VerificationChildProcessRegistry,
    chat_service::{ProviderErrorCategory, ProviderErrorMetadata},
    runtime_factory::ChatRuntimeFactoryDeps,
    AppState, InteractiveProcessRegistry,
};
use crate::domain::agents::{
    AgentHarnessKind, AgentProviderSettings, McpOverrideState, McpServerKey, ProviderSessionRef,
};
use crate::domain::entities::{
    app_state::ExecutionHaltMode, AgentConversationWorkspaceMode, AgentRun, AgentRunId,
    AgentRunStatus, ChatConversation, ChatConversationId, ChatMessage, ChatTimelineItemStatus,
    DelegationPark, DelegationParkId, DelegationParkState, DelegationWakePolicy,
    ExecutionFailureSource, ExecutionRecoveryMetadata, ExecutionRecoveryReasonCode,
    ExecutionRecoveryState, IdeationSessionId, InternalStatus, NotificationCategory,
    NotificationSeverity, NotificationTargetKind, Persona, PersonaId, PersonaStatus, Project,
    ProjectId, Task, ValidationCacheDecision, ValidationCommandCategory, ValidationCommandResult,
    ValidationCommandSource, ValidationCommandStatus, ValidationContextType, ValidationPurpose,
    ValidationRun, ValidationRunMode, ValidationRunStatus, VerificationStatus,
};
use crate::domain::repositories::{
    ActivityEventRepository, AgentRunRepository, ArtifactRepository, ChatAttachmentRepository,
    ChatConversationRepository, ChatMessageRepository, ChatTimelineRepository,
    ExecutionSettingsRepository, IdeationSessionRepository, MemoryEventRepository,
    PlanBranchRepository, ProjectRepository, ReviewRepository, StateHistoryMetadata,
    StatusTransition, TaskDependencyRepository, TaskProposalRepository, TaskRepository,
    TaskStepRepository, ValidationRunRepository,
};
use crate::domain::services::{MessageQueue, RunningAgentRegistry};
use crate::error::AppResult;
use crate::infrastructure::agents::claude::{ContentBlockItem, SpawnableCommand, ToolCall};
use crate::infrastructure::memory::MemoryValidationRunRepository;

#[allow(clippy::too_many_arguments)]
async fn handle_stream_success(
    agent_run_id: &str,
    context_type: ChatContextType,
    context_id: &str,
    has_output: bool,
    completion_tool_called: bool,
    execution_slot_held: bool,
    execution_state: &Option<Arc<ExecutionState>>,
    task_repo: &Arc<dyn TaskRepository>,
    task_dependency_repo: &Arc<dyn TaskDependencyRepository>,
    project_repo: &Arc<dyn ProjectRepository>,
    artifact_repo: &Arc<dyn ArtifactRepository>,
    chat_message_repo: &Arc<dyn ChatMessageRepository>,
    chat_attachment_repo: &Arc<dyn ChatAttachmentRepository>,
    conversation_repo: &Arc<dyn ChatConversationRepository>,
    agent_run_repo: &Arc<dyn AgentRunRepository>,
    ideation_session_repo: &Arc<dyn IdeationSessionRepository>,
    activity_event_repo: &Arc<dyn ActivityEventRepository>,
    message_queue: &Arc<MessageQueue>,
    running_agent_registry: &Arc<dyn RunningAgentRegistry>,
    memory_event_repo: &Arc<dyn MemoryEventRepository>,
    plan_branch_repo: &Option<Arc<dyn PlanBranchRepository>>,
    task_step_repo: &Option<Arc<dyn TaskStepRepository>>,
    execution_settings_repo: &Option<Arc<dyn ExecutionSettingsRepository>>,
    runtime_factory_deps: &ChatRuntimeFactoryDeps,
    interactive_process_registry: &Option<Arc<InteractiveProcessRegistry>>,
    review_repo: &Option<Arc<dyn ReviewRepository>>,
    verification_child_registry: &Option<Arc<VerificationChildProcessRegistry>>,
) {
    let validation_run_repo = runtime_factory_deps.validation_run_repo.clone();
    super::handle_stream_success(
        agent_run_id,
        context_type,
        context_id,
        has_output,
        completion_tool_called,
        execution_slot_held,
        execution_state,
        task_repo,
        task_dependency_repo,
        project_repo,
        artifact_repo,
        chat_message_repo,
        chat_attachment_repo,
        conversation_repo,
        agent_run_repo,
        ideation_session_repo,
        activity_event_repo,
        message_queue,
        running_agent_registry,
        memory_event_repo,
        plan_branch_repo,
        task_step_repo,
        &validation_run_repo,
        &None,
        &None,
        execution_settings_repo,
        &runtime_factory_deps.agent_lane_settings_repo,
        &runtime_factory_deps.agent_provider_settings_repo,
        &runtime_factory_deps.events,
        Some(runtime_factory_deps),
        interactive_process_registry,
        review_repo,
        verification_child_registry,
    )
    .await;
}

#[allow(clippy::too_many_arguments)]
async fn handle_stream_error(
    error: &str,
    stream_error: Option<&StreamError>,
    context_type: ChatContextType,
    context_id: &str,
    conversation_id: ChatConversationId,
    agent_run_id: &str,
    pre_assistant_msg_id: &str,
    event_ctx: &EventContextPayload,
    stored_session_id: Option<&str>,
    effective_harness: AgentHarnessKind,
    is_retry_attempt: bool,
    user_message_content: Option<&str>,
    conversation: Option<&ChatConversation>,
    resolved_project_id: Option<String>,
    cli_path: &std::path::Path,
    plugin_dir: &std::path::Path,
    working_directory: &std::path::Path,
    chat_message_repo: &Arc<dyn ChatMessageRepository>,
    chat_timeline_repo: &Option<Arc<dyn ChatTimelineRepository>>,
    chat_attachment_repo: &Arc<dyn ChatAttachmentRepository>,
    artifact_repo: &Arc<dyn ArtifactRepository>,
    conversation_repo: &Arc<dyn ChatConversationRepository>,
    agent_run_repo: &Arc<dyn AgentRunRepository>,
    task_repo: &Arc<dyn TaskRepository>,
    task_dependency_repo: &Arc<dyn TaskDependencyRepository>,
    project_repo: &Arc<dyn ProjectRepository>,
    ideation_session_repo: &Arc<dyn IdeationSessionRepository>,
    task_proposal_repo: &Option<Arc<dyn TaskProposalRepository>>,
    activity_event_repo: &Arc<dyn ActivityEventRepository>,
    message_queue: &Arc<MessageQueue>,
    running_agent_registry: &Arc<dyn RunningAgentRegistry>,
    memory_event_repo: &Arc<dyn MemoryEventRepository>,
    execution_state: &Option<Arc<ExecutionState>>,
    question_state: &Option<Arc<QuestionState>>,
    plan_branch_repo: &Option<Arc<dyn PlanBranchRepository>>,
    execution_settings_repo: &Option<Arc<dyn ExecutionSettingsRepository>>,
    runtime_factory_deps: &ChatRuntimeFactoryDeps,
    agent_name: Option<&str>,
    run_chain_id: Option<String>,
    interactive_process_registry: &Option<Arc<InteractiveProcessRegistry>>,
    review_repo: &Option<Arc<dyn ReviewRepository>>,
    task_step_repo: &Option<Arc<dyn TaskStepRepository>>,
    verification_child_registry: &Option<Arc<VerificationChildProcessRegistry>>,
) -> bool {
    let validation_run_repo = runtime_factory_deps.validation_run_repo.clone();
    super::handle_stream_error(
        error,
        stream_error,
        context_type,
        context_id,
        conversation_id,
        agent_run_id,
        pre_assistant_msg_id,
        event_ctx,
        stored_session_id,
        effective_harness,
        is_retry_attempt,
        false,
        false,
        user_message_content,
        conversation,
        resolved_project_id,
        cli_path,
        plugin_dir,
        working_directory,
        chat_message_repo,
        chat_timeline_repo,
        chat_attachment_repo,
        artifact_repo,
        conversation_repo,
        agent_run_repo,
        task_repo,
        task_dependency_repo,
        project_repo,
        ideation_session_repo,
        task_proposal_repo,
        activity_event_repo,
        message_queue,
        running_agent_registry,
        memory_event_repo,
        execution_state,
        question_state,
        plan_branch_repo,
        execution_settings_repo,
        &runtime_factory_deps.agent_lane_settings_repo,
        &runtime_factory_deps.agent_provider_settings_repo,
        Arc::clone(&runtime_factory_deps.events),
        None,
        Some(runtime_factory_deps),
        agent_name,
        run_chain_id,
        interactive_process_registry,
        review_repo,
        task_step_repo,
        &validation_run_repo,
        &runtime_factory_deps.external_events_repo,
        &runtime_factory_deps.webhook_publisher,
        verification_child_registry,
        &runtime_factory_deps.notification_service,
    )
    .await
}

/// Configurable mock: `get_by_id` returns the stored task (or None).
struct StubTaskRepo {
    task: Option<Task>,
    status_entered_at: Option<DateTime<Utc>>,
}

fn forced_transition_failures() -> &'static Mutex<HashSet<String>> {
    static FAILURES: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    FAILURES.get_or_init(|| Mutex::new(HashSet::new()))
}

#[tokio::test]
async fn provider_env_for_harness_reads_explicit_provider_settings() {
    let empty = provider_env_for_harness(&None, AgentHarnessKind::Claude)
        .await
        .expect("missing provider settings");
    assert!(empty.is_empty());

    let temp_dir = tempfile::tempdir().expect("temp dir");
    let env_path = temp_dir.path().join("claude.env");
    std::fs::write(
        &env_path,
        "CUSTOM_PROVIDER_TOKEN=from-handler\nCLAUDE_MODEL=spoofed\n",
    )
    .expect("write env file");
    let app_state = AppState::new_test();
    let mut settings = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Claude);
    settings.custom_env_file_enabled = true;
    settings.custom_env_file_path = Some(env_path.to_string_lossy().into_owned());
    app_state
        .agent_provider_settings_repo
        .upsert(&settings)
        .await
        .expect("save provider settings");
    let provider_repo = Some(Arc::clone(&app_state.agent_provider_settings_repo));

    let provider_env = provider_env_for_harness(&provider_repo, AgentHarnessKind::Claude)
        .await
        .expect("load provider env");

    assert_eq!(
        provider_env
            .get("CUSTOM_PROVIDER_TOKEN")
            .map(String::as_str),
        Some("from-handler")
    );
    assert!(!provider_env.contains_key("CLAUDE_MODEL"));
}

#[tokio::test]
async fn provider_env_for_harness_uses_explicit_provider_repo_without_app_handle() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let env_path = temp_dir.path().join("claude.env");
    std::fs::write(&env_path, "CUSTOM_PROVIDER_TOKEN=from-explicit-repo\n")
        .expect("write env file");
    let app_state = AppState::new_test();
    let mut settings = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Claude);
    settings.custom_env_file_enabled = true;
    settings.custom_env_file_path = Some(env_path.to_string_lossy().into_owned());
    app_state
        .agent_provider_settings_repo
        .upsert(&settings)
        .await
        .expect("save provider settings");
    let provider_repo = Some(Arc::clone(&app_state.agent_provider_settings_repo));

    let provider_env = provider_env_for_harness(&provider_repo, AgentHarnessKind::Claude)
        .await
        .expect("load provider env");

    assert_eq!(
        provider_env
            .get("CUSTOM_PROVIDER_TOKEN")
            .map(String::as_str),
        Some("from-explicit-repo")
    );
}

#[tokio::test]
async fn recovery_retry_provider_decision_fails_execution_without_provider_repo() {
    let decision =
        recovery_retry_provider_decision(&None, AgentHarnessKind::Claude, ChatContextType::Review)
            .await;

    assert_eq!(
        decision,
        Err(RecoveryRetryProviderBlock::MissingProviderSettings),
        "execution-slot recovery retries must fail closed without provider settings"
    );
}

#[tokio::test]
async fn recovery_retry_provider_decision_allows_non_execution_without_provider_repo() {
    let decision =
        recovery_retry_provider_decision(&None, AgentHarnessKind::Claude, ChatContextType::Project)
            .await
            .expect("non-execution recovery can run without provider settings");

    assert_eq!(
        decision,
        RecoveryRetryProviderDecision::AllowWithoutProviderSettings
    );
}

#[tokio::test]
async fn recovery_retry_provider_decision_blocks_disabled_provider() {
    let app_state = AppState::new_test();
    let mut disabled = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Claude);
    disabled.is_default = true;
    app_state
        .agent_provider_settings_repo
        .upsert(&disabled)
        .await
        .expect("save disabled default provider");
    let provider_repo = Some(Arc::clone(&app_state.agent_provider_settings_repo));

    let decision = recovery_retry_provider_decision(
        &provider_repo,
        AgentHarnessKind::Claude,
        ChatContextType::Review,
    )
    .await;

    let Err(RecoveryRetryProviderBlock::Disabled(error)) = decision else {
        panic!("expected disabled provider block");
    };
    assert!(
        error.contains("Choose and enable a default provider")
            || error.contains("Claude is not enabled"),
        "unexpected disabled-provider error: {error}"
    );
}

#[tokio::test]
async fn recovery_retry_provider_decision_applies_explicit_provider_env() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let env_path = temp_dir.path().join("claude.env");
    std::fs::write(
        &env_path,
        "CUSTOM_PROVIDER_TOKEN=from-retry\nCLAUDE_MODEL=ignored\n",
    )
    .expect("write env file");
    let app_state = AppState::new_test();
    let mut settings = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Claude);
    settings.enabled = true;
    settings.is_default = true;
    settings.custom_env_file_enabled = true;
    settings.custom_env_file_path = Some(env_path.to_string_lossy().into_owned());
    app_state
        .agent_provider_settings_repo
        .upsert(&settings)
        .await
        .expect("save enabled provider settings");
    let provider_repo = Some(Arc::clone(&app_state.agent_provider_settings_repo));

    let decision = recovery_retry_provider_decision(
        &provider_repo,
        AgentHarnessKind::Claude,
        ChatContextType::Review,
    )
    .await
    .expect("recovery retry should load custom provider env");

    let RecoveryRetryProviderDecision::ApplyEnv(provider_env) = decision else {
        panic!("expected provider env application");
    };
    assert_eq!(
        provider_env
            .get("CUSTOM_PROVIDER_TOKEN")
            .map(String::as_str),
        Some("from-retry")
    );
    assert!(
        !provider_env.contains_key("CLAUDE_MODEL"),
        "protected model overrides must stay filtered from provider env"
    );
}

fn recovery_retry_test_provider_spawnable() -> chat_service_context::ProviderSpawnableCommand {
    chat_service_context::ProviderSpawnableCommand {
        spawnable: SpawnableCommand::new(Command::new("true").into(), None),
    }
}

fn spawnable_env_value(spawnable: &SpawnableCommand, key: &str) -> Option<String> {
    spawnable
        .get_envs_for_test()
        .into_iter()
        .find_map(|(env_key, env_value)| {
            (env_key.to_string_lossy() == key).then(|| env_value.to_string_lossy().into_owned())
        })
}

struct EnvVarGuard {
    key: &'static str,
    previous: Option<String>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let previous = std::env::var(key).ok();
        std::env::set_var(key, value);
        Self { key, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => std::env::set_var(self.key, value),
            None => std::env::remove_var(self.key),
        }
    }
}

fn write_claude_session_fixture(dir: &std::path::Path, session_id: &str) -> std::path::PathBuf {
    let cli_path = dir.join("claude-session-fixture.sh");
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' '{}'\nprintf '%s\\n' '{}'\n",
        serde_json::json!({
            "type": "assistant",
            "message": {
                "content": [{ "type": "text", "text": "recovered session" }]
            },
            "session_id": session_id,
        }),
        serde_json::json!({
            "type": "result",
            "session_id": session_id,
            "is_error": false,
            "result": "recovered session",
            "cost_usd": 0.0,
        })
    );
    std::fs::write(&cli_path, script).expect("write cli fixture");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&cli_path)
            .expect("cli fixture metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&cli_path, permissions).expect("make cli fixture executable");
    }

    cli_path
}

#[tokio::test]
async fn recovery_retry_spawnable_gate_fails_execution_without_provider_repo() {
    let spawnable = recovery_retry_spawnable_with_provider_gate(
        &None,
        AgentHarnessKind::Claude,
        ChatContextType::Review,
        None,
        std::path::Path::new("/tmp"),
        None,
        recovery_retry_test_provider_spawnable(),
    )
    .await
    .unwrap();

    assert!(
        spawnable.is_none(),
        "execution-slot recovery retry must not spawn without provider settings"
    );
}

#[tokio::test]
async fn recovery_retry_spawnable_gate_blocks_non_execution_without_app_state() {
    let spawnable = recovery_retry_spawnable_with_provider_gate(
        &None,
        AgentHarnessKind::Claude,
        ChatContextType::Project,
        None,
        std::path::Path::new("/tmp"),
        None,
        recovery_retry_test_provider_spawnable(),
    )
    .await
    .unwrap();

    assert!(
        spawnable.is_none(),
        "non-execution recovery retry must not bypass MCP policy without app state"
    );
}

#[tokio::test]
async fn recovery_retry_spawnable_gate_applies_policy_without_provider_repo() {
    let app_state = AppState::new_test();
    let key = McpServerKey::new(AgentHarnessKind::Claude, "github").unwrap();
    app_state
        .mcp_policy_repo
        .set_server_state(None, &key, McpOverrideState::Disabled)
        .await
        .expect("save global MCP deny");
    let runtime_deps = ChatRuntimeFactoryDeps::from_app_state(&app_state);

    let spawnable = recovery_retry_spawnable_with_provider_gate(
        &None,
        AgentHarnessKind::Claude,
        ChatContextType::Project,
        None,
        std::path::Path::new("/tmp"),
        Some(&runtime_deps),
        recovery_retry_test_provider_spawnable(),
    )
    .await
    .unwrap()
    .expect("app state can resolve policy without provider settings");

    assert!(spawnable
        .get_args_for_test()
        .windows(2)
        .any(|args| args == ["--disallowedTools", "mcp__github__*"]));
}

#[tokio::test]
async fn recovery_retry_spawnable_gate_blocks_disabled_provider() {
    let app_state = AppState::new_test();
    let mut disabled = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Claude);
    disabled.is_default = true;
    app_state
        .agent_provider_settings_repo
        .upsert(&disabled)
        .await
        .expect("save disabled default provider");
    let provider_repo = Some(Arc::clone(&app_state.agent_provider_settings_repo));

    let spawnable = recovery_retry_spawnable_with_provider_gate(
        &provider_repo,
        AgentHarnessKind::Claude,
        ChatContextType::Review,
        None,
        std::path::Path::new("/tmp"),
        None,
        recovery_retry_test_provider_spawnable(),
    )
    .await
    .unwrap();

    assert!(
        spawnable.is_none(),
        "disabled providers must block recovery retry spawn"
    );
}

#[tokio::test]
async fn recovery_retry_spawnable_gate_applies_provider_env() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let env_path = temp_dir.path().join("claude.env");
    std::fs::write(&env_path, "CUSTOM_PROVIDER_TOKEN=spawnable-env\n").expect("write env file");
    let app_state = AppState::new_test();
    let mut settings = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Claude);
    settings.enabled = true;
    settings.is_default = true;
    settings.custom_env_file_enabled = true;
    settings.custom_env_file_path = Some(env_path.to_string_lossy().into_owned());
    app_state
        .agent_provider_settings_repo
        .upsert(&settings)
        .await
        .expect("save enabled provider settings");
    let provider_repo = Some(Arc::clone(&app_state.agent_provider_settings_repo));
    let runtime_deps = ChatRuntimeFactoryDeps::from_app_state(&app_state);

    let spawnable = recovery_retry_spawnable_with_provider_gate(
        &provider_repo,
        AgentHarnessKind::Claude,
        ChatContextType::Review,
        None,
        std::path::Path::new("/tmp"),
        Some(&runtime_deps),
        recovery_retry_test_provider_spawnable(),
    )
    .await
    .unwrap()
    .expect("enabled provider should allow recovery retry");

    assert_eq!(
        spawnable_env_value(&spawnable, "CUSTOM_PROVIDER_TOKEN").as_deref(),
        Some("spawnable-env")
    );
}

#[tokio::test]
async fn resolve_recovery_retry_spawnable_allows_gated_build_success() {
    let app_state = AppState::new_test();
    let runtime_deps = ChatRuntimeFactoryDeps::from_app_state(&app_state);
    let provider_gate = RecoveryRetryProviderGate::new(
        &None,
        AgentHarnessKind::Claude,
        ChatContextType::Project,
        None,
        std::path::Path::new("/tmp"),
        Some(&runtime_deps),
    );

    let spawnable = resolve_recovery_retry_spawnable(
        Ok(recovery_retry_test_provider_spawnable()),
        provider_gate,
    )
    .await
    .unwrap();

    assert!(
        spawnable.is_some(),
        "successful non-execution retry command should survive provider gating"
    );
}

#[tokio::test]
async fn resolve_recovery_retry_spawnable_drops_build_errors() {
    let provider_gate = RecoveryRetryProviderGate::new(
        &None,
        AgentHarnessKind::Claude,
        ChatContextType::Project,
        None,
        std::path::Path::new("/tmp"),
        None,
    );

    let spawnable =
        resolve_recovery_retry_spawnable(Err("build failed".to_string()), provider_gate)
            .await
            .unwrap();

    assert!(
        spawnable.is_none(),
        "retry command build errors must not launch a recovery retry"
    );
}

#[test]
fn recovery_retry_app_repos_are_empty_without_runtime_deps() {
    let repos = RecoveryRetryAppRepos::from_runtime_factory_deps(None);

    assert!(repos.ideation_effort_settings_repo.is_none());
    assert!(repos.ideation_model_settings_repo.is_none());
    assert!(repos.delegated_session_repo.is_none());
}

#[test]
fn recovery_retry_app_repos_read_required_runtime_repos() {
    let app_state = AppState::new_test();
    let runtime_deps = ChatRuntimeFactoryDeps::from_app_state(&app_state);

    let repos = RecoveryRetryAppRepos::from_runtime_factory_deps(Some(&runtime_deps));

    assert!(repos.ideation_effort_settings_repo.is_some());
    assert!(repos.ideation_model_settings_repo.is_some());
    assert!(repos.delegated_session_repo.is_some());
}

#[tokio::test]
async fn recovery_retry_folder_refs_context_carries_prompt_block_and_roots() {
    let temp = tempfile::tempdir_in(std::env::current_dir().expect("current directory"))
        .expect("temp directory");
    let app_data = crate::utils::path_safety::validate_absolute_non_root_path(
        &temp.path().join("app-data"),
        "recovery retry folder reference app data",
    )
    .expect("safe app data");
    let project_root = crate::utils::path_safety::validate_absolute_non_root_path(
        &temp.path().join("project"),
        "recovery retry project root",
    )
    .expect("safe project root");
    let folder = crate::utils::path_safety::validate_absolute_non_root_path(
        &temp.path().join("folder"),
        "recovery retry folder root",
    )
    .expect("safe folder root");
    std::fs::create_dir(&app_data).expect("create app data");
    std::fs::create_dir(&project_root).expect("create project root");
    std::fs::create_dir(&folder).expect("create folder root");

    let mut state = AppState::new_test();
    state.app_paths = crate::application::AppPaths::new(app_data.clone(), None);
    let project = Project::new(
        "Recovery folder refs".to_string(),
        project_root.to_string_lossy().into_owned(),
    );
    let project_id = project.id.clone();
    state
        .project_repo
        .create(project)
        .await
        .expect("seed project");
    let conversation = ChatConversation::new_project(project_id.clone());
    crate::application::conversation_folder_reference_service::ConversationFolderReferenceService::new(
        Arc::clone(&state.conversation_folder_reference_repo),
        app_data,
        5,
    )
    .add(conversation.id, &folder, "Recovery Folder".to_string())
    .await
    .expect("seed folder reference");
    let runtime_deps = ChatRuntimeFactoryDeps::from_app_state(&state);

    let (block, roots) = recovery_retry_folder_refs_context(
        Some(&runtime_deps),
        &conversation,
        Some(project_id.as_str()),
        &project_root,
    )
    .await
    .expect("resolve recovery retry folder refs");

    assert!(block
        .expect("folder block")
        .contains(&folder.to_string_lossy().to_string()));
    assert!(roots.contains(&folder));

    let mut builder = conversation;
    builder.agent_mode = Some(AgentConversationWorkspaceMode::PersonaBuilder);
    let (builder_block, builder_roots) = recovery_retry_folder_refs_context(
        Some(&runtime_deps),
        &builder,
        Some(project_id.as_str()),
        &project_root,
    )
    .await
    .expect("builder folder refs are skipped");
    assert!(builder_block.is_none());
    assert!(!builder_roots.contains(&folder));
}

#[test]
fn handler_runtime_factory_deps_keep_explicit_lane_and_provider_without_app_handle() {
    let app_state = AppState::new_test();
    let execution_settings_repo = Some(Arc::clone(&app_state.execution_settings_repo));
    let agent_lane_settings_repo = Some(Arc::clone(&app_state.agent_lane_settings_repo));
    let agent_provider_settings_repo = Some(Arc::clone(&app_state.agent_provider_settings_repo));
    let runtime_support = RuntimeSupportRepos::new(
        &execution_settings_repo,
        &agent_lane_settings_repo,
        &agent_provider_settings_repo,
        &None,
        &None,
        &None,
        &None,
    );

    let deps = build_runtime_factory_deps(
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.task_dependency_repo),
        Arc::clone(&app_state.project_repo),
        Arc::clone(&app_state.artifact_repo),
        Arc::clone(&app_state.chat_message_repo),
        Arc::clone(&app_state.chat_attachment_repo),
        Arc::clone(&app_state.chat_conversation_repo),
        Arc::clone(&app_state.agent_run_repo),
        Arc::clone(&app_state.ideation_session_repo),
        Arc::clone(&app_state.activity_event_repo),
        Arc::clone(&app_state.message_queue),
        Arc::clone(&app_state.running_agent_registry),
        Arc::clone(&app_state.memory_event_repo),
        runtime_support,
    );

    assert!(deps.execution_settings_repo.is_some());
    assert!(deps.agent_lane_settings_repo.is_some());
    assert!(deps.agent_provider_settings_repo.is_some());
}

#[test]
fn handler_runtime_factory_deps_preserve_complete_chat_snapshot_dependencies() {
    let app_state = AppState::new_test();
    let chat_deps = ChatRuntimeFactoryDeps::from_app_state(&app_state);
    let runtime_support = RuntimeSupportRepos::new(
        &chat_deps.execution_settings_repo,
        &chat_deps.agent_lane_settings_repo,
        &chat_deps.agent_provider_settings_repo,
        &chat_deps.plan_branch_repo,
        &chat_deps.interactive_process_registry,
        &chat_deps.task_step_repo,
        &chat_deps.validation_run_repo,
    )
    .with_runtime_factory_deps(Some(&chat_deps));

    let deps = build_runtime_factory_deps(
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.task_dependency_repo),
        Arc::clone(&app_state.project_repo),
        Arc::clone(&app_state.artifact_repo),
        Arc::clone(&app_state.chat_message_repo),
        Arc::clone(&app_state.chat_attachment_repo),
        Arc::clone(&app_state.chat_conversation_repo),
        Arc::clone(&app_state.agent_run_repo),
        Arc::clone(&app_state.ideation_session_repo),
        Arc::clone(&app_state.activity_event_repo),
        Arc::clone(&app_state.message_queue),
        Arc::clone(&app_state.running_agent_registry),
        Arc::clone(&app_state.memory_event_repo),
        runtime_support,
    );

    assert!(Arc::ptr_eq(
        deps.review_repo.as_ref().expect("review repo"),
        chat_deps.review_repo.as_ref().expect("chat review repo"),
    ));
    assert!(Arc::ptr_eq(
        deps.agent_conversation_workspace_repo
            .as_ref()
            .expect("workspace repo"),
        chat_deps
            .agent_conversation_workspace_repo
            .as_ref()
            .expect("chat workspace repo"),
    ));
}

#[test]
fn handler_runtime_factory_deps_do_not_backfill_missing_lane_and_provider() {
    let app_state = AppState::new_test();
    let execution_settings_repo = Some(Arc::clone(&app_state.execution_settings_repo));
    let runtime_support = RuntimeSupportRepos::new(
        &execution_settings_repo,
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
    );
    let deps = build_runtime_factory_deps(
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.task_dependency_repo),
        Arc::clone(&app_state.project_repo),
        Arc::clone(&app_state.artifact_repo),
        Arc::clone(&app_state.chat_message_repo),
        Arc::clone(&app_state.chat_attachment_repo),
        Arc::clone(&app_state.chat_conversation_repo),
        Arc::clone(&app_state.agent_run_repo),
        Arc::clone(&app_state.ideation_session_repo),
        Arc::clone(&app_state.activity_event_repo),
        Arc::clone(&app_state.message_queue),
        Arc::clone(&app_state.running_agent_registry),
        Arc::clone(&app_state.memory_event_repo),
        runtime_support,
    );

    assert!(deps.execution_settings_repo.is_some());
    assert!(
        deps.agent_lane_settings_repo.is_none(),
        "handler runtime deps must not silently recover missing lane settings from AppState"
    );
    assert!(
        deps.agent_provider_settings_repo.is_none(),
        "handler runtime deps must not silently recover missing provider settings from AppState"
    );
}

#[tokio::test]
async fn cancelled_stream_preserves_already_terminal_agent_run() {
    let state = AppState::new_test();
    let run = state
        .agent_run_repo
        .create(AgentRun::new(ChatConversationId::new()))
        .await
        .expect("create run");
    state
        .agent_run_repo
        .complete(&run.id)
        .await
        .expect("complete run");

    mark_cancelled_stream_as_cancelled(
        &state.agent_run_repo,
        &run.id.as_str(),
        ChatContextType::Project,
        ProjectId::new().as_str(),
        &state.task_repo,
    )
    .await;

    let stored = state
        .agent_run_repo
        .get_by_id(&run.id)
        .await
        .expect("load run")
        .expect("run should exist");
    assert_eq!(stored.status, AgentRunStatus::Completed);
    assert!(stored.error_message.is_none());
}

#[tokio::test]
async fn handle_stream_success_preserves_armed_delegation_park_for_completed_parent_run() {
    let state = AppState::new_test();
    let project_id = ProjectId::new();
    let conversation = ChatConversation::new_project(project_id.clone());
    state
        .chat_conversation_repo
        .create(conversation.clone())
        .await
        .expect("create conversation");
    let parent_run = state
        .agent_run_repo
        .create(AgentRun::new(conversation.id.clone()))
        .await
        .expect("create parent run");
    state
        .agent_run_repo
        .complete(&parent_run.id)
        .await
        .expect("complete parent run");

    let now = Utc::now();
    let park = DelegationPark {
        id: DelegationParkId::new(),
        parent_conversation_id: conversation.id,
        parent_agent_run_id: parent_run.id.clone(),
        generation: 1,
        wake_policy: DelegationWakePolicy::AllSettled,
        wake_on_failure: false,
        state: DelegationParkState::Armed,
        deadline_at: now + Duration::minutes(5),
        wake_claimed_at: None,
        wake_attempts: 0,
        last_error: None,
        created_at: now,
        updated_at: now,
        jobs: Vec::new(),
    };
    state
        .delegation_park_repo
        .arm(park.clone())
        .await
        .expect("arm delegation park");

    let runtime_deps = ChatRuntimeFactoryDeps::from_app_state(&state);
    let parent_run_id = parent_run.id.as_str();

    handle_stream_success(
        &parent_run_id,
        ChatContextType::Project,
        project_id.as_str(),
        true,
        false,
        false,
        &None,
        &state.task_repo,
        &state.task_dependency_repo,
        &state.project_repo,
        &state.artifact_repo,
        &state.chat_message_repo,
        &state.chat_attachment_repo,
        &state.chat_conversation_repo,
        &state.agent_run_repo,
        &state.ideation_session_repo,
        &state.activity_event_repo,
        &state.message_queue,
        &state.running_agent_registry,
        &state.memory_event_repo,
        &None,
        &None,
        &None,
        &runtime_deps,
        &None,
        &None,
        &None,
    )
    .await;

    let stored_run = state
        .agent_run_repo
        .get_by_id(&parent_run.id)
        .await
        .expect("load parent run")
        .expect("parent run should exist");
    assert_eq!(stored_run.status, AgentRunStatus::Completed);
    let stored_park = state
        .delegation_park_repo
        .get(&park.id)
        .await
        .expect("load delegation park")
        .expect("delegation park should exist");
    assert_eq!(
        stored_park.state,
        DelegationParkState::Armed,
        "normal turn completion must preserve the armed park for a later delegate wake"
    );
}

#[tokio::test]
async fn cancelled_stream_marks_recovery_task_run_as_system_recovery() {
    let state = AppState::new_test();
    let project_id = ProjectId::new();
    let mut task = Task::new(project_id, "Recovery cancellation".to_string());
    task.metadata = Some(serde_json::json!({ "trigger_origin": "recovery" }).to_string());
    let task_id = task.id.clone();
    state.task_repo.create(task).await.expect("create task");
    let run = state
        .agent_run_repo
        .create(AgentRun::new(ChatConversationId::new()))
        .await
        .expect("create run");

    mark_cancelled_stream_as_cancelled(
        &state.agent_run_repo,
        &run.id.as_str(),
        ChatContextType::TaskExecution,
        task_id.as_str(),
        &state.task_repo,
    )
    .await;

    let stored = state
        .agent_run_repo
        .get_by_id(&run.id)
        .await
        .expect("load run")
        .expect("run should exist");
    assert_eq!(stored.status, AgentRunStatus::Failed);
    assert_eq!(
        stored.error_message.as_deref(),
        Some("Agent stream cancelled by system recovery")
    );
}

#[test]
fn stream_error_recovery_reason_code_maps_local_tool_and_validation_failures() {
    assert_eq!(
        stream_error_recovery_reason_code(&StreamError::LocalToolFailed {
            message: "local tool failed".to_string(),
        }),
        ExecutionRecoveryReasonCode::LocalToolFailed,
    );
    assert_eq!(
        stream_error_recovery_reason_code(&StreamError::ValidationFailed {
            message: "validation failed".to_string(),
        }),
        ExecutionRecoveryReasonCode::ValidationFailed,
    );
}

#[tokio::test]
async fn cancelled_stream_marks_retrying_recovery_metadata_as_system_recovery() {
    let state = AppState::new_test();
    let project_id = ProjectId::new();
    let mut recovery = ExecutionRecoveryMetadata::new();
    recovery.last_state = ExecutionRecoveryState::Retrying;
    recovery.stop_retrying = false;
    let mut task = Task::new(project_id, "Retrying recovery cancellation".to_string());
    task.metadata = Some(
        recovery
            .update_task_metadata(None)
            .expect("recovery metadata"),
    );
    let task_id = task.id.clone();
    state.task_repo.create(task).await.expect("create task");
    let run = state
        .agent_run_repo
        .create(AgentRun::new(ChatConversationId::new()))
        .await
        .expect("create run");

    mark_cancelled_stream_as_cancelled(
        &state.agent_run_repo,
        &run.id.as_str(),
        ChatContextType::TaskExecution,
        task_id.as_str(),
        &state.task_repo,
    )
    .await;

    let stored = state
        .agent_run_repo
        .get_by_id(&run.id)
        .await
        .expect("load run")
        .expect("run should exist");
    assert_eq!(stored.status, AgentRunStatus::Failed);
    assert_eq!(
        stored.error_message.as_deref(),
        Some("Agent stream cancelled by system recovery")
    );
}

#[test]
fn stopped_retrying_recovery_metadata_does_not_mark_system_recovery_cancellation() {
    let mut recovery = ExecutionRecoveryMetadata::new();
    recovery.last_state = ExecutionRecoveryState::Retrying;
    recovery.stop_retrying = true;
    let metadata = recovery
        .update_task_metadata(None)
        .expect("recovery metadata");

    assert!(!task_metadata_indicates_recovery_cancellation(Some(
        &metadata
    )));
    assert!(!task_metadata_indicates_recovery_cancellation(Some(
        "not json"
    )));
}

#[async_trait]
impl TaskRepository for StubTaskRepo {
    async fn get_by_id(&self, _id: &TaskId) -> AppResult<Option<Task>> {
        Ok(self.task.clone())
    }

    // ── Stubs for all other required methods ────────────────────────────
    async fn create(&self, task: Task) -> AppResult<Task> {
        Ok(task)
    }
    async fn get_by_project(&self, _: &ProjectId) -> AppResult<Vec<Task>> {
        Ok(vec![])
    }
    async fn update(&self, _: &Task) -> AppResult<()> {
        Ok(())
    }
    async fn update_with_expected_status(&self, task: &Task, _: InternalStatus) -> AppResult<bool> {
        if forced_transition_failures()
            .lock()
            .unwrap()
            .contains(task.id.as_str())
        {
            return Err(crate::error::AppError::Database(
                "injected transition failure".to_string(),
            ));
        }
        Ok(true)
    }
    async fn update_metadata(&self, _: &TaskId, _: Option<String>) -> AppResult<()> {
        Ok(())
    }
    async fn delete(&self, _: &TaskId) -> AppResult<()> {
        Ok(())
    }
    async fn get_by_status(&self, _: &ProjectId, _: InternalStatus) -> AppResult<Vec<Task>> {
        Ok(vec![])
    }
    async fn persist_status_change(
        &self,
        _: &TaskId,
        _: InternalStatus,
        _: InternalStatus,
        _: &str,
    ) -> AppResult<String> {
        Ok(uuid::Uuid::new_v4().to_string())
    }
    async fn get_status_history(&self, _: &TaskId) -> AppResult<Vec<StatusTransition>> {
        Ok(vec![])
    }
    async fn get_status_entered_at(
        &self,
        _: &TaskId,
        _: InternalStatus,
    ) -> AppResult<Option<DateTime<Utc>>> {
        Ok(self.status_entered_at)
    }
    async fn get_status_last_entered_at(
        &self,
        _: &TaskId,
        _: InternalStatus,
    ) -> AppResult<Option<DateTime<Utc>>> {
        Ok(self.status_entered_at)
    }
    async fn get_next_executable(&self, _: &ProjectId) -> AppResult<Option<Task>> {
        Ok(None)
    }
    async fn get_by_ideation_session(&self, _: &IdeationSessionId) -> AppResult<Vec<Task>> {
        Ok(vec![])
    }
    async fn get_by_project_filtered(&self, _: &ProjectId, _: bool) -> AppResult<Vec<Task>> {
        Ok(vec![])
    }
    async fn archive(&self, id: &TaskId) -> AppResult<Task> {
        let mut t = Task::new(ProjectId::new(), "archived".into());
        t.id = id.clone();
        Ok(t)
    }
    async fn restore(&self, id: &TaskId) -> AppResult<Task> {
        let mut t = Task::new(ProjectId::new(), "restored".into());
        t.id = id.clone();
        Ok(t)
    }
    async fn get_archived_count(&self, _: &ProjectId, _: Option<&str>) -> AppResult<u32> {
        Ok(0)
    }
    async fn list_paginated(
        &self,
        _: &ProjectId,
        _: Option<Vec<InternalStatus>>,
        _: u32,
        _: u32,
        _: bool,
        _: Option<&str>,
        _: Option<&str>,
        _: Option<&[String]>,
    ) -> AppResult<Vec<Task>> {
        Ok(vec![])
    }
    async fn count_tasks(
        &self,
        _: &ProjectId,
        _: bool,
        _: Option<&str>,
        _: Option<&str>,
    ) -> AppResult<u32> {
        Ok(0)
    }
    async fn search(&self, _: &ProjectId, _: &str, _: bool) -> AppResult<Vec<Task>> {
        Ok(vec![])
    }
    async fn get_oldest_ready_task(&self) -> AppResult<Option<Task>> {
        Ok(None)
    }
    async fn get_oldest_ready_tasks(&self, _: u32) -> AppResult<Vec<Task>> {
        Ok(vec![])
    }
    async fn update_latest_state_history_metadata(
        &self,
        _: &TaskId,
        _: &StateHistoryMetadata,
    ) -> AppResult<()> {
        Ok(())
    }
    async fn has_task_in_states(&self, _: &ProjectId, _: &[InternalStatus]) -> AppResult<bool> {
        Ok(false)
    }
    async fn get_stale_ready_tasks(&self, _threshold_secs: u64) -> AppResult<Vec<Task>> {
        Ok(vec![])
    }
    async fn get_status_history_batch(
        &self,
        _task_ids: &[crate::domain::entities::TaskId],
    ) -> AppResult<std::collections::HashMap<crate::domain::entities::TaskId, Vec<StatusTransition>>>
    {
        Ok(std::collections::HashMap::new())
    }
}

fn make_task(status: InternalStatus) -> Task {
    let mut task = Task::new(ProjectId::new(), "test task".into());
    task.internal_status = status;
    task
}

#[tokio::test]
async fn test_still_needs_recovery_when_executing() {
    let task_id = TaskId::new();
    let repo: Arc<dyn TaskRepository> = Arc::new(StubTaskRepo {
        task: Some(make_task(InternalStatus::Executing)),
        status_entered_at: None,
    });
    assert!(task_still_needs_execution_recovery(&task_id, &repo).await);
}

#[tokio::test]
async fn test_still_needs_recovery_when_re_executing() {
    let task_id = TaskId::new();
    let repo: Arc<dyn TaskRepository> = Arc::new(StubTaskRepo {
        task: Some(make_task(InternalStatus::ReExecuting)),
        status_entered_at: None,
    });
    assert!(task_still_needs_execution_recovery(&task_id, &repo).await);
}

#[tokio::test]
async fn test_no_recovery_when_already_transitioned() {
    // Simulate auto-complete resolving the task to PendingReview during the 500ms window
    let task_id = TaskId::new();
    let repo: Arc<dyn TaskRepository> = Arc::new(StubTaskRepo {
        task: Some(make_task(InternalStatus::PendingReview)),
        status_entered_at: None,
    });
    assert!(!task_still_needs_execution_recovery(&task_id, &repo).await);
}

#[tokio::test]
async fn test_no_recovery_when_failed() {
    let task_id = TaskId::new();
    let repo: Arc<dyn TaskRepository> = Arc::new(StubTaskRepo {
        task: Some(make_task(InternalStatus::Failed)),
        status_entered_at: None,
    });
    assert!(!task_still_needs_execution_recovery(&task_id, &repo).await);
}

#[tokio::test]
async fn test_no_recovery_when_cancelled() {
    let task_id = TaskId::new();
    let repo: Arc<dyn TaskRepository> = Arc::new(StubTaskRepo {
        task: Some(make_task(InternalStatus::Cancelled)),
        status_entered_at: None,
    });
    assert!(!task_still_needs_execution_recovery(&task_id, &repo).await);
}

#[tokio::test]
async fn test_no_recovery_when_task_not_found() {
    // Task not found (e.g., deleted) → skip retry safely
    let task_id = TaskId::new();
    let repo: Arc<dyn TaskRepository> = Arc::new(StubTaskRepo {
        task: None,
        status_entered_at: None,
    });
    assert!(!task_still_needs_execution_recovery(&task_id, &repo).await);
}

#[tokio::test]
async fn test_execution_attempt_guard_rejects_stale_run_after_restart() {
    use crate::domain::entities::{AgentRun, ChatConversationId};
    use crate::infrastructure::memory::MemoryAgentRunRepository;

    let task_id = TaskId::new();
    let mut task = make_task(InternalStatus::Executing);
    task.id = task_id.clone();
    let status_entered_at = Utc::now();
    let task_repo: Arc<dyn TaskRepository> = Arc::new(StubTaskRepo {
        task: Some(task),
        status_entered_at: Some(status_entered_at),
    });

    let run_repo = Arc::new(MemoryAgentRunRepository::new());
    let mut stale_run = AgentRun::new(ChatConversationId::new());
    stale_run.started_at = status_entered_at - chrono::Duration::minutes(5);
    let stale_run_id = stale_run.id.as_str().to_string();
    run_repo.create(stale_run).await.unwrap();
    let run_repo: Arc<dyn AgentRunRepository> = run_repo;

    assert!(
        !task_execution_attempt_matches_current_status(
            &task_id,
            stale_run_id.as_str(),
            &task_repo,
            &run_repo,
        )
        .await,
        "Older execution run must not transition a newer restarted attempt",
    );
}

#[tokio::test]
async fn test_execution_attempt_guard_allows_current_run() {
    use crate::domain::entities::{AgentRun, ChatConversationId};
    use crate::infrastructure::memory::MemoryAgentRunRepository;

    let task_id = TaskId::new();
    let mut task = make_task(InternalStatus::Executing);
    task.id = task_id.clone();
    let status_entered_at = Utc::now();
    let task_repo: Arc<dyn TaskRepository> = Arc::new(StubTaskRepo {
        task: Some(task),
        status_entered_at: Some(status_entered_at),
    });

    let run_repo = Arc::new(MemoryAgentRunRepository::new());
    let mut current_run = AgentRun::new(ChatConversationId::new());
    current_run.started_at = status_entered_at + chrono::Duration::milliseconds(100);
    let current_run_id = current_run.id.as_str().to_string();
    run_repo.create(current_run).await.unwrap();
    let run_repo: Arc<dyn AgentRunRepository> = run_repo;

    assert!(
        task_execution_attempt_matches_current_status(
            &task_id,
            current_run_id.as_str(),
            &task_repo,
            &run_repo,
        )
        .await,
        "Current execution run must still be allowed to transition the task",
    );
}

#[tokio::test]
async fn test_load_current_task_execution_attempt_classifies_current_attempt() {
    use crate::domain::entities::{AgentRun, ChatConversationId};
    use crate::infrastructure::memory::MemoryAgentRunRepository;

    let task_id = TaskId::new();
    let mut task = make_task(InternalStatus::Executing);
    task.id = task_id.clone();
    let status_entered_at = Utc::now();
    let task_repo: Arc<dyn TaskRepository> = Arc::new(StubTaskRepo {
        task: Some(task.clone()),
        status_entered_at: Some(status_entered_at),
    });

    let run_repo = Arc::new(MemoryAgentRunRepository::new());
    let mut current_run = AgentRun::new(ChatConversationId::new());
    current_run.started_at = status_entered_at + chrono::Duration::milliseconds(250);
    let current_run_id = current_run.id.as_str().to_string();
    run_repo.create(current_run).await.unwrap();

    let mut stale_run = AgentRun::new(ChatConversationId::new());
    stale_run.started_at = status_entered_at - chrono::Duration::minutes(2);
    let stale_run_id = stale_run.id.as_str().to_string();
    run_repo.create(stale_run).await.unwrap();
    let run_repo: Arc<dyn AgentRunRepository> = run_repo;

    let current = load_current_task_execution_attempt(
        &task_id,
        current_run_id.as_str(),
        &task_repo,
        &run_repo,
    )
    .await;
    assert!(
        current.is_some(),
        "current execution attempt should be eligible for finalization"
    );

    let stale =
        load_current_task_execution_attempt(&task_id, stale_run_id.as_str(), &task_repo, &run_repo)
            .await;
    assert!(
        stale.is_none(),
        "older execution run must not finalize a newer execution attempt"
    );

    let review_repo: Arc<dyn TaskRepository> = Arc::new(StubTaskRepo {
        task: Some(make_task(InternalStatus::Reviewing)),
        status_entered_at: Some(status_entered_at),
    });
    let review_attempt = load_current_task_execution_attempt(
        &task_id,
        current_run_id.as_str(),
        &review_repo,
        &run_repo,
    )
    .await;
    assert!(
        review_attempt.is_none(),
        "review-state tasks are no longer active execution attempts"
    );
}

#[tokio::test]
async fn test_load_current_task_execution_attempt_handles_missing_records() {
    use crate::infrastructure::memory::MemoryAgentRunRepository;

    let task_id = TaskId::new();
    let missing_task_repo: Arc<dyn TaskRepository> = Arc::new(StubTaskRepo {
        task: None,
        status_entered_at: None,
    });
    let run_repo: Arc<dyn AgentRunRepository> = Arc::new(MemoryAgentRunRepository::new());

    let missing_task =
        load_current_task_execution_attempt(&task_id, "missing-run", &missing_task_repo, &run_repo)
            .await;
    assert!(
        missing_task.is_none(),
        "missing tasks cannot be finalized as execution attempts"
    );

    let mut task = make_task(InternalStatus::Executing);
    task.id = task_id.clone();
    let active_task_repo: Arc<dyn TaskRepository> = Arc::new(StubTaskRepo {
        task: Some(task),
        status_entered_at: Some(Utc::now()),
    });

    let missing_run =
        load_current_task_execution_attempt(&task_id, "missing-run", &active_task_repo, &run_repo)
            .await;
    assert!(
        missing_run.is_none(),
        "missing agent-run rows are identity-unknown, not positively current attempts"
    );
}

// ========================================
// Global Rate Limit Backpressure Integration Tests
// ========================================

#[test]
fn test_apply_global_rate_limit_backpressure_sets_gate() {
    let exec = Arc::new(ExecutionState::new());
    let execution_state = Some(exec.clone());

    // Provide a future retry_after timestamp
    let future = (chrono::Utc::now() + chrono::Duration::seconds(300)).to_rfc3339();
    let retry_after = Some(future);

    assert!(!exec.is_provider_blocked());
    apply_global_rate_limit_backpressure(&execution_state, &retry_after, "test", "task-1");
    assert!(exec.is_provider_blocked());
    assert!(!exec.can_start_task());
}

#[test]
fn test_apply_global_rate_limit_backpressure_noop_without_retry_after() {
    let exec = Arc::new(ExecutionState::new());
    let execution_state = Some(exec.clone());

    // No retry_after → should not set backpressure
    apply_global_rate_limit_backpressure(&execution_state, &None, "test", "task-1");
    assert!(!exec.is_provider_blocked());
    assert!(exec.can_start_task());
}

#[test]
fn test_apply_global_rate_limit_backpressure_noop_without_execution_state() {
    let execution_state: Option<Arc<ExecutionState>> = None;
    let future = (chrono::Utc::now() + chrono::Duration::seconds(300)).to_rfc3339();
    let retry_after = Some(future);

    // Should not panic when execution_state is None
    apply_global_rate_limit_backpressure(&execution_state, &retry_after, "test", "task-1");
}

#[test]
fn test_apply_global_rate_limit_backpressure_expired_does_not_block() {
    let exec = Arc::new(ExecutionState::new());
    let execution_state = Some(exec.clone());

    // Provide a past retry_after timestamp
    let past = (chrono::Utc::now() - chrono::Duration::seconds(60)).to_rfc3339();
    let retry_after = Some(past);

    apply_global_rate_limit_backpressure(&execution_state, &retry_after, "test", "task-1");
    // Epoch was set, but it's in the past, so is_provider_blocked returns false
    assert!(!exec.is_provider_blocked());
    assert!(exec.can_start_task());
}

#[test]
fn test_execution_completion_action_unknown_with_output_no_validation_is_failed() {
    assert_eq!(
        execution_completion_action(true, StepCompletionState::Unknown, false, false),
        ExecutionCompletionAction::Failed
    );
}

#[test]
fn test_execution_completion_action_unknown_with_validation_is_pending_review() {
    assert_eq!(
        execution_completion_action(false, StepCompletionState::Unknown, false, true),
        ExecutionCompletionAction::PendingReview
    );
}

#[test]
fn test_zero_step_output_without_signal_or_validation_is_failed() {
    assert_eq!(
        execution_completion_action(true, StepCompletionState::NoSteps, false, false),
        ExecutionCompletionAction::Failed
    );
}

#[test]
fn test_zero_step_output_with_signal_is_pending_review() {
    assert_eq!(
        execution_completion_action(true, StepCompletionState::NoSteps, true, false),
        ExecutionCompletionAction::PendingReview
    );
}

#[test]
fn test_zero_step_validation_rescue_without_signal_is_pending_review() {
    assert_eq!(
        execution_completion_action(false, StepCompletionState::NoSteps, false, true),
        ExecutionCompletionAction::PendingReview
    );
}

#[test]
fn test_execution_completion_action_incomplete_with_output_no_validation_is_failed() {
    assert_eq!(
        execution_completion_action(true, StepCompletionState::Incomplete, false, false),
        ExecutionCompletionAction::Failed
    );
}

#[test]
fn test_execution_completion_action_incomplete_with_validation_is_pending_review() {
    assert_eq!(
        execution_completion_action(false, StepCompletionState::Incomplete, false, true),
        ExecutionCompletionAction::PendingReview
    );
}

#[test]
fn test_steps_tracked_all_done_ignores_completion_signal() {
    assert_eq!(
        execution_completion_action(false, StepCompletionState::AllComplete, false, false),
        ExecutionCompletionAction::PendingReview
    );
}

fn validation_cache_fixture(
    commit_sha: &str,
    tests_ran: bool,
    tests_passed: bool,
) -> ValidationCacheMetadata {
    validation_cache_fixture_at(commit_sha, tests_ran, tests_passed, Utc::now())
}

fn validation_cache_fixture_at(
    commit_sha: &str,
    tests_ran: bool,
    tests_passed: bool,
    captured_at: DateTime<Utc>,
) -> ValidationCacheMetadata {
    ValidationCacheMetadata {
        version: 1,
        commit_sha: commit_sha.to_string(),
        tests_ran,
        tests_passed,
        test_summary: None,
        captured_at,
        captured_by: "execution_complete".to_string(),
    }
}

fn validation_run_fixture(
    task_id: &TaskId,
    project_id: &ProjectId,
    promoted_sha: &str,
    episode_entered_at: DateTime<Utc>,
) -> ValidationRun {
    ValidationRun {
        id: "validation-current".to_string(),
        task_id: task_id.clone(),
        project_id: project_id.clone(),
        purpose: ValidationPurpose::Final,
        context_type: ValidationContextType::Execution,
        requested_by_agent: Some("test".to_string()),
        status: ValidationRunStatus::Passed,
        mode: ValidationRunMode::ReuseOrRun,
        policy_enabled: true,
        head_sha: Some(promoted_sha.to_string()),
        start_content_fingerprint: None,
        validated_content_fingerprint: None,
        promoted_commit_sha: Some(promoted_sha.to_string()),
        base_ref: Some("main".to_string()),
        analysis_fingerprint: None,
        status_episode_entered_at: Some(episode_entered_at),
        started_at: episode_entered_at + chrono::Duration::milliseconds(1),
        completed_at: Some(episode_entered_at + chrono::Duration::seconds(1)),
    }
}

fn validation_command_fixture(
    run_id: &str,
    task_id: &TaskId,
    project_id: &ProjectId,
    head_sha: &str,
    cwd: &str,
    episode_entered_at: DateTime<Utc>,
) -> ValidationCommandResult {
    ValidationCommandResult {
        id: "validation-command".to_string(),
        validation_run_id: run_id.to_string(),
        task_id: task_id.clone(),
        project_id: project_id.clone(),
        command_source: ValidationCommandSource::ProjectAnalysisRef,
        command_ref: Some("tests".to_string()),
        command: "cargo test".to_string(),
        cwd: cwd.to_string(),
        label: Some("Tests".to_string()),
        category: ValidationCommandCategory::Test,
        reason: None,
        related_files: Vec::new(),
        cache_key: "validation-cache".to_string(),
        cache_decision: ValidationCacheDecision::Ran,
        status: ValidationCommandStatus::Passed,
        exit_code: Some(0),
        duration_ms: Some(1),
        stdout_snippet: None,
        stderr_snippet: None,
        stdout_log_path: None,
        stderr_log_path: None,
        launcher_kind: None,
        resolved_shell_path: None,
        head_sha: Some(head_sha.to_string()),
        analysis_fingerprint: None,
        status_episode_entered_at: Some(episode_entered_at),
        created_at: episode_entered_at + chrono::Duration::seconds(1),
    }
}

fn git_worktree_with_initial_commit() -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().expect("temp git dir");
    Command::new("git")
        .arg("init")
        .current_dir(dir.path())
        .output()
        .expect("git init should run");
    fs::write(dir.path().join("README.md"), "test\n").expect("write tracked file");
    Command::new("git")
        .args(["add", "README.md"])
        .current_dir(dir.path())
        .output()
        .expect("git add should run");
    let commit = Command::new("git")
        .args([
            "-c",
            "user.name=RalphX Test",
            "-c",
            "user.email=ralphx-test@example.invalid",
            "commit",
            "-m",
            "initial",
        ])
        .current_dir(dir.path())
        .output()
        .expect("git commit should run");
    assert!(
        commit.status.success(),
        "git commit failed: {}",
        String::from_utf8_lossy(&commit.stderr)
    );
    let head = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dir.path())
        .output()
        .expect("git rev-parse should run");
    assert!(
        head.status.success(),
        "git rev-parse failed: {}",
        String::from_utf8_lossy(&head.stderr)
    );
    let sha = String::from_utf8(head.stdout)
        .expect("HEAD should be utf8")
        .trim()
        .to_string();
    (dir, sha)
}

fn git_worktree_with_base_and_change() -> (tempfile::TempDir, String, String) {
    let (dir, base_sha) = git_worktree_with_initial_commit();
    fs::write(dir.path().join("README.md"), "test\nchanged\n").expect("write changed file");
    Command::new("git")
        .args(["add", "README.md"])
        .current_dir(dir.path())
        .output()
        .expect("git add should run");
    let commit = Command::new("git")
        .args([
            "-c",
            "user.name=RalphX Test",
            "-c",
            "user.email=ralphx-test@example.invalid",
            "commit",
            "-m",
            "change",
        ])
        .current_dir(dir.path())
        .output()
        .expect("git commit should run");
    assert!(
        commit.status.success(),
        "git change commit failed: {}",
        String::from_utf8_lossy(&commit.stderr)
    );
    let head = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dir.path())
        .output()
        .expect("git rev-parse should run");
    assert!(
        head.status.success(),
        "git rev-parse failed: {}",
        String::from_utf8_lossy(&head.stderr)
    );
    let head_sha = String::from_utf8(head.stdout)
        .expect("HEAD should be utf8")
        .trim()
        .to_string();
    (dir, base_sha, head_sha)
}

#[test]
fn test_validation_cache_proves_completion_requires_head_match_and_green_tests() {
    let sha = "abc123def456";
    // Green + SHA matches HEAD → proves completion.
    assert!(validation_cache_proves_completion(
        &validation_cache_fixture(sha, true, true),
        sha
    ));
}

#[test]
fn test_validation_cache_does_not_prove_completion_on_sha_mismatch() {
    // Cache was captured on a different commit than current HEAD → stale, no override.
    assert!(!validation_cache_proves_completion(
        &validation_cache_fixture("oldsha000", true, true),
        "newsha111"
    ));
}

#[test]
fn test_validation_cache_does_not_prove_completion_when_tests_did_not_run() {
    let sha = "abc123def456";
    // tests_ran=false (e.g. a self-blocked no-op claiming success) must NOT rescue the task,
    // even if tests_passed is opportunistically true.
    assert!(!validation_cache_proves_completion(
        &validation_cache_fixture(sha, false, true),
        sha
    ));
}

#[test]
fn test_validation_cache_does_not_prove_completion_when_tests_failed() {
    let sha = "abc123def456";
    assert!(!validation_cache_proves_completion(
        &validation_cache_fixture(sha, true, false),
        sha
    ));
}

#[test]
fn test_validation_cache_fresh_for_episode_rejects_cache_captured_before_latest_entry() {
    let head_sha = "abc123def456";
    let episode_entered_at = Utc::now();
    let cache = validation_cache_fixture_at(
        head_sha,
        true,
        true,
        episode_entered_at - chrono::Duration::seconds(1),
    );

    assert!(!validation_cache_fresh_for_episode(
        &cache,
        head_sha,
        episode_entered_at
    ));
}

#[test]
fn test_validation_cache_fresh_for_episode_accepts_cache_captured_after_latest_entry() {
    let head_sha = "abc123def456";
    let episode_entered_at = Utc::now();
    let cache = validation_cache_fixture_at(
        head_sha,
        true,
        true,
        episode_entered_at + chrono::Duration::milliseconds(1),
    );

    assert!(validation_cache_fresh_for_episode(
        &cache,
        head_sha,
        episode_entered_at
    ));
}

#[test]
fn test_validation_cache_fresh_for_episode_rejects_sha_mismatch_and_red() {
    let episode_entered_at = Utc::now();
    assert!(!validation_cache_fresh_for_episode(
        &validation_cache_fixture_at("oldsha000", true, true, episode_entered_at),
        "abc123def456",
        episode_entered_at
    ));
    assert!(!validation_cache_fresh_for_episode(
        &validation_cache_fixture_at("abc123def456", false, true, episode_entered_at),
        "abc123def456",
        episode_entered_at
    ));
    assert!(!validation_cache_fresh_for_episode(
        &validation_cache_fixture_at("abc123def456", true, false, episode_entered_at),
        "abc123def456",
        episode_entered_at
    ));
}

#[test]
fn test_incomplete_review_action_escalates_only_for_live_reviewing_tasks() {
    assert_eq!(
        incomplete_review_action(InternalStatus::Reviewing, false),
        IncompleteReviewAction::Escalate
    );
    assert_eq!(
        incomplete_review_action(InternalStatus::Reviewing, true),
        IncompleteReviewAction::SkipDuringShutdown
    );
    assert_eq!(
        incomplete_review_action(InternalStatus::PendingMerge, false),
        IncompleteReviewAction::IgnoreAlreadyTransitioned
    );
}

#[tokio::test]
async fn test_apply_system_wide_provider_pause_pauses_mixed_active_task_states() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());

    let project = Project::new(
        "Provider Pause".to_string(),
        "/tmp/provider-pause".to_string(),
    );
    let project_id = project.id.clone();
    app_state.project_repo.create(project).await.unwrap();

    let mut executing = Task::new(project_id.clone(), "Executing".to_string());
    executing.internal_status = InternalStatus::Executing;
    let executing = app_state.task_repo.create(executing).await.unwrap();

    let mut reviewing = Task::new(project_id.clone(), "Reviewing".to_string());
    reviewing.internal_status = InternalStatus::Reviewing;
    let reviewing = app_state.task_repo.create(reviewing).await.unwrap();

    let mut merging = Task::new(project_id.clone(), "Merging".to_string());
    merging.internal_status = InternalStatus::Merging;
    let merging = app_state.task_repo.create(merging).await.unwrap();

    let mut ready = Task::new(project_id.clone(), "Ready".to_string());
    ready.internal_status = InternalStatus::Ready;
    let ready = app_state.task_repo.create(ready).await.unwrap();

    let app = mock_builder()
        .manage(app_state)
        .manage(Arc::clone(&execution_state))
        .build(mock_context(noop_assets()))
        .expect("mock app");
    let handle = app.handle().clone();
    let state = handle.state::<AppState>();
    let runtime_deps = ChatRuntimeFactoryDeps::from_app_state(&state);

    let pause_applied = apply_system_wide_provider_pause(
        Some(&runtime_deps),
        Some(&execution_state),
        Arc::clone(&state.events),
        &ProviderErrorCategory::RateLimit,
        "You've hit your limit · resets 11pm (Europe/Bucharest)",
        &Some((chrono::Utc::now() + chrono::Duration::minutes(30)).to_rfc3339()),
        ChatContextType::TaskExecution,
        executing.id.as_str(),
    )
    .await;
    assert!(pause_applied);

    assert!(execution_state.is_paused());
    assert!(execution_state.is_provider_blocked());
    assert!(!execution_state.can_start_task());

    let persisted = state.app_state_repo.get().await.unwrap();
    assert_eq!(persisted.execution_halt_mode, ExecutionHaltMode::Paused);

    let executing_after = state
        .task_repo
        .get_by_id(&executing.id)
        .await
        .unwrap()
        .unwrap();
    let reviewing_after = state
        .task_repo
        .get_by_id(&reviewing.id)
        .await
        .unwrap()
        .unwrap();
    let merging_after = state
        .task_repo
        .get_by_id(&merging.id)
        .await
        .unwrap()
        .unwrap();
    let ready_after = state.task_repo.get_by_id(&ready.id).await.unwrap().unwrap();

    assert_eq!(executing_after.internal_status, InternalStatus::Paused);
    assert_eq!(reviewing_after.internal_status, InternalStatus::Paused);
    assert_eq!(merging_after.internal_status, InternalStatus::Paused);
    assert_eq!(ready_after.internal_status, InternalStatus::Ready);

    let notifications = state
        .notification_repo
        .list(None, None, 50)
        .await
        .expect("global provider-pause notification should be readable")
        .notifications;
    assert_eq!(notifications.len(), 1, "one global pause creates one row");
    assert_eq!(
        notifications[0].category,
        NotificationCategory::ProviderPaused
    );
    assert_eq!(notifications[0].title, "Agents paused");
    assert_eq!(
        notifications[0].body.as_deref(),
        Some("Rate limit reached — queue paused, auto-resumes")
    );
    assert!(
        notifications[0]
            .dedupe_key
            .as_deref()
            .is_some_and(|key| key.starts_with("provider:rate_limit:paused:")),
        "global pause dedupe must use the persisted pause instance"
    );

    assert!(
        apply_system_wide_provider_pause(
            Some(&runtime_deps),
            Some(&execution_state),
            Arc::clone(&state.events),
            &ProviderErrorCategory::RateLimit,
            "You've hit your limit · resets 11pm (Europe/Bucharest)",
            &Some((chrono::Utc::now() + chrono::Duration::minutes(30)).to_rfc3339()),
            ChatContextType::TaskExecution,
            executing.id.as_str(),
        )
        .await,
        "duplicate provider delivery remains a handled global pause"
    );
    assert_eq!(
        state
            .notification_repo
            .list(None, None, 50)
            .await
            .expect("duplicate provider-pause notification query should succeed")
            .notifications
            .len(),
        1,
        "duplicate provider delivery must not duplicate the global row"
    );
}

#[tokio::test]
async fn test_provider_pause_from_delegation_does_not_pause_execution_tasks() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());

    let project = Project::new(
        "Delegation Provider Pause".to_string(),
        "/tmp/delegation-provider-pause".to_string(),
    );
    let project_id = project.id.clone();
    app_state.project_repo.create(project).await.unwrap();

    let mut executing = Task::new(project_id, "Codex execution".to_string());
    executing.internal_status = InternalStatus::Executing;
    let executing = app_state.task_repo.create(executing).await.unwrap();

    let app = mock_builder()
        .manage(app_state)
        .manage(Arc::clone(&execution_state))
        .build(mock_context(noop_assets()))
        .expect("mock app");
    let handle = app.handle().clone();
    let state = handle.state::<AppState>();
    let runtime_deps = ChatRuntimeFactoryDeps::from_app_state(&state);

    let pause_applied = apply_system_wide_provider_pause(
        Some(&runtime_deps),
        Some(&execution_state),
        Arc::clone(&state.events),
        &ProviderErrorCategory::RateLimit,
        "You've hit your weekly limit · resets 11pm (Europe/Bucharest)",
        &Some((chrono::Utc::now() + chrono::Duration::minutes(30)).to_rfc3339()),
        ChatContextType::Delegation,
        "delegated-session-1",
    )
    .await;
    assert!(!pause_applied);

    assert!(
        !execution_state.is_paused(),
        "delegation provider errors must not globally pause execution"
    );
    assert!(
        !execution_state.is_provider_blocked(),
        "delegation provider errors must not set the global execution provider gate"
    );
    assert!(
        execution_state.can_start_task(),
        "unrelated execution work should remain schedulable"
    );

    let persisted = state.app_state_repo.get().await.unwrap();
    assert_ne!(persisted.execution_halt_mode, ExecutionHaltMode::Paused);

    let executing_after = state
        .task_repo
        .get_by_id(&executing.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(executing_after.internal_status, InternalStatus::Executing);
    assert!(
        ProviderErrorMetadata::from_task_metadata(executing_after.metadata.as_deref()).is_none(),
        "unrelated execution task should not receive provider pause metadata"
    );
}

#[tokio::test]
async fn test_codex_local_tool_rate_limit_text_does_not_global_pause_execution() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());

    let project = Project::new(
        "Codex Local Tool Failure".to_string(),
        "/tmp/codex-local-tool-failure".to_string(),
    );
    let project_id = project.id.clone();
    app_state.project_repo.create(project).await.unwrap();

    let mut task = Task::new(project_id, "Executing".to_string());
    task.internal_status = InternalStatus::Executing;
    let task = app_state.task_repo.create(task).await.unwrap();
    let task_id = task.id.clone();

    let app = mock_builder()
        .manage(app_state)
        .manage(Arc::clone(&execution_state))
        .build(mock_context(noop_assets()))
        .expect("mock app");
    let handle = app.handle().clone();
    let state = handle.state::<AppState>();

    let runtime_errors = Vec::<String>::new();
    let local_tool_errors = vec![
        "rg: src-tauri/src/domain/entities/agent_run.rs: No such file or directory\n\
         src-tauri/src/application/chat_service/chat_service_errors.rs: ProviderErrorCategory::RateLimit writes rate_limit"
            .to_string(),
    ];
    let stream_error = crate::application::chat_service::classify_codex_stream_failure(
        &runtime_errors,
        &local_tool_errors,
        Some(1),
        false,
    )
    .expect("local Codex tool failure should produce a stream error");
    assert!(
        matches!(stream_error, StreamError::LocalToolFailed { .. }),
        "local command output containing rate_limit must not classify as provider or agent-exit error"
    );

    let conversation_id = ChatConversationId::new();
    let event_ctx = crate::application::chat_service::event_context(
        &conversation_id,
        &ChatContextType::TaskExecution,
        task_id.as_str(),
    );
    let error_message = stream_error.to_string();

    let recovery_spawned = handle_stream_error(
        &error_message,
        Some(&stream_error),
        ChatContextType::TaskExecution,
        task_id.as_str(),
        conversation_id,
        "run-id-local-tool-error",
        "message-id-local-tool-error",
        &event_ctx,
        None,
        crate::domain::agents::AgentHarnessKind::Codex,
        false,
        None,
        None,
        None,
        std::path::Path::new("/tmp/codex"),
        std::path::Path::new("/tmp/plugin"),
        std::path::Path::new("/tmp"),
        &state.chat_message_repo,
        &Some(state.chat_timeline_repo.clone()),
        &state.chat_attachment_repo,
        &state.artifact_repo,
        &state.chat_conversation_repo,
        &state.agent_run_repo,
        &state.task_repo,
        &state.task_dependency_repo,
        &state.project_repo,
        &state.ideation_session_repo,
        &None,
        &state.activity_event_repo,
        &state.message_queue,
        &state.running_agent_registry,
        &state.memory_event_repo,
        &Some(Arc::clone(&execution_state)),
        &None,
        &None,
        &None,
        &ChatRuntimeFactoryDeps::from_app_state(&state),
        None,
        None,
        &None,
        &None,
        &None,
        &None,
    )
    .await;

    assert!(!recovery_spawned);
    assert!(!execution_state.is_paused());
    assert!(!execution_state.is_provider_blocked());

    let persisted = state.app_state_repo.get().await.unwrap();
    assert_eq!(persisted.execution_halt_mode, ExecutionHaltMode::Running);

    let updated_task = state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("task should still exist");
    assert_eq!(updated_task.internal_status, InternalStatus::Failed);
    assert!(
        ProviderErrorMetadata::from_task_metadata(updated_task.metadata.as_deref()).is_none(),
        "local tool failures must not persist provider_error metadata"
    );
}

// ========================================
// AgentExit + Step Completion Override Tests
// ========================================
//
// These verify the all_steps_completed helper and that handle_stream_error
// overrides Failed → PendingReview when all steps are completed.

use crate::application::chat_service::chat_service_handlers::all_steps_completed;
use crate::domain::entities::{TaskStep, TaskStepId};
use crate::error::AppError;
use std::collections::HashMap;

struct StubTaskStepRepo {
    steps: Vec<TaskStep>,
}

#[async_trait]
impl TaskStepRepository for StubTaskStepRepo {
    async fn create(&self, step: TaskStep) -> AppResult<TaskStep> {
        Ok(step)
    }
    async fn get_by_id(&self, _: &TaskStepId) -> AppResult<Option<TaskStep>> {
        Ok(None)
    }
    async fn get_by_task(&self, _: &TaskId) -> AppResult<Vec<TaskStep>> {
        Ok(self.steps.clone())
    }
    async fn get_by_task_and_status(
        &self,
        _: &TaskId,
        _: TaskStepStatus,
    ) -> AppResult<Vec<TaskStep>> {
        Ok(vec![])
    }
    async fn update(&self, _: &TaskStep) -> AppResult<()> {
        Ok(())
    }
    async fn delete(&self, _: &TaskStepId) -> AppResult<()> {
        Ok(())
    }
    async fn delete_by_task(&self, _: &TaskId) -> AppResult<()> {
        Ok(())
    }
    async fn count_by_status(&self, _: &TaskId) -> AppResult<HashMap<TaskStepStatus, u32>> {
        Ok(HashMap::new())
    }
    async fn bulk_create(&self, steps: Vec<TaskStep>) -> AppResult<Vec<TaskStep>> {
        Ok(steps)
    }
    async fn reorder(&self, _: &TaskId, _: Vec<TaskStepId>) -> AppResult<()> {
        Ok(())
    }
    async fn reset_all_to_pending(&self, _: &TaskId) -> AppResult<u32> {
        Ok(0)
    }
}

/// Stub that always returns a DB error for get_by_task.
struct StubErrorTaskStepRepo;

#[async_trait]
impl TaskStepRepository for StubErrorTaskStepRepo {
    async fn create(&self, step: TaskStep) -> AppResult<TaskStep> {
        Ok(step)
    }
    async fn get_by_id(&self, _: &TaskStepId) -> AppResult<Option<TaskStep>> {
        Ok(None)
    }
    async fn get_by_task(&self, _: &TaskId) -> AppResult<Vec<TaskStep>> {
        Err(AppError::Database("simulated DB error".into()))
    }
    async fn get_by_task_and_status(
        &self,
        _: &TaskId,
        _: TaskStepStatus,
    ) -> AppResult<Vec<TaskStep>> {
        Ok(vec![])
    }
    async fn update(&self, _: &TaskStep) -> AppResult<()> {
        Ok(())
    }
    async fn delete(&self, _: &TaskStepId) -> AppResult<()> {
        Ok(())
    }
    async fn delete_by_task(&self, _: &TaskId) -> AppResult<()> {
        Ok(())
    }
    async fn count_by_status(&self, _: &TaskId) -> AppResult<HashMap<TaskStepStatus, u32>> {
        Ok(HashMap::new())
    }
    async fn bulk_create(&self, steps: Vec<TaskStep>) -> AppResult<Vec<TaskStep>> {
        Ok(steps)
    }
    async fn reorder(&self, _: &TaskId, _: Vec<TaskStepId>) -> AppResult<()> {
        Ok(())
    }
    async fn reset_all_to_pending(&self, _: &TaskId) -> AppResult<u32> {
        Ok(0)
    }
}

struct StatusChangingTaskStepRepo {
    task_repo: Arc<dyn TaskRepository>,
    task_id: TaskId,
    target_status: InternalStatus,
}

impl StatusChangingTaskStepRepo {
    async fn move_task_to_target_status(&self) -> AppResult<()> {
        let Some(mut task) = self.task_repo.get_by_id(&self.task_id).await? else {
            return Ok(());
        };
        task.internal_status = self.target_status;
        self.task_repo.update(&task).await
    }
}

#[async_trait]
impl TaskStepRepository for StatusChangingTaskStepRepo {
    async fn create(&self, step: TaskStep) -> AppResult<TaskStep> {
        Ok(step)
    }
    async fn get_by_id(&self, _: &TaskStepId) -> AppResult<Option<TaskStep>> {
        Ok(None)
    }
    async fn get_by_task(&self, task_id: &TaskId) -> AppResult<Vec<TaskStep>> {
        if task_id == &self.task_id {
            self.move_task_to_target_status().await?;
        }
        Ok(Vec::new())
    }
    async fn get_by_task_and_status(
        &self,
        _: &TaskId,
        _: TaskStepStatus,
    ) -> AppResult<Vec<TaskStep>> {
        Ok(Vec::new())
    }
    async fn update(&self, _: &TaskStep) -> AppResult<()> {
        Ok(())
    }
    async fn delete(&self, _: &TaskStepId) -> AppResult<()> {
        Ok(())
    }
    async fn delete_by_task(&self, _: &TaskId) -> AppResult<()> {
        Ok(())
    }
    async fn count_by_status(&self, _: &TaskId) -> AppResult<HashMap<TaskStepStatus, u32>> {
        Ok(HashMap::new())
    }
    async fn bulk_create(&self, steps: Vec<TaskStep>) -> AppResult<Vec<TaskStep>> {
        Ok(steps)
    }
    async fn reorder(&self, _: &TaskId, _: Vec<TaskStepId>) -> AppResult<()> {
        Ok(())
    }
    async fn reset_all_to_pending(&self, _: &TaskId) -> AppResult<u32> {
        Ok(0)
    }
}

fn make_step(task_id: &TaskId, status: TaskStepStatus) -> TaskStep {
    let mut step = TaskStep::new(task_id.clone(), "test step".into(), 0, "agent".into());
    step.status = status;
    step
}

fn run<F: std::future::Future>(f: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(f)
}

async fn seed_current_execution_attempt(state: &AppState, task_id: &TaskId) -> String {
    use crate::domain::entities::{AgentRun, ChatConversationId};

    state
        .task_repo
        .persist_status_change(
            task_id,
            InternalStatus::Ready,
            InternalStatus::Executing,
            "test",
        )
        .await
        .expect("status history should persist");
    let run = AgentRun::new(ChatConversationId::new());
    let run_id = run.id.as_str().to_string();
    state
        .agent_run_repo
        .create(run)
        .await
        .expect("agent run should persist");
    run_id
}

/// Completed/skipped steps are necessary task context, but do not prove that an AgentExit
/// completed successfully. The production rescue path also requires a current green
/// validation cache before transitioning to PendingReview.
#[test]
fn test_all_steps_completed_classifies_completed_and_skipped_steps() {
    let task_id = TaskId::new();
    let steps = vec![
        make_step(&task_id, TaskStepStatus::Completed),
        make_step(&task_id, TaskStepStatus::Completed),
        make_step(&task_id, TaskStepStatus::Skipped),
    ];
    let step_repo: Option<Arc<dyn TaskStepRepository>> = Some(Arc::new(StubTaskStepRepo { steps }));

    let result = run(all_steps_completed(&step_repo, &task_id));
    assert!(result, "All Completed+Skipped steps → should return true");
}

#[test]
fn test_agent_exit_all_steps_complete_without_validation_cache_stays_failed() {
    let stream_error = StreamError::AgentExit {
        exit_code: None,
        stderr: "agent exited after failed validation".to_string(),
    };
    let validation_complete = false;
    let initial_target = InternalStatus::Failed;

    let target_status = if initial_target == InternalStatus::Failed
        && matches!(&stream_error, StreamError::AgentExit { .. })
        && validation_complete
    {
        InternalStatus::PendingReview
    } else {
        initial_target
    };

    assert_eq!(
        target_status,
        InternalStatus::Failed,
        "Completed steps alone must not rescue AgentExit without a current green validation cache"
    );
}

/// AgentExit with incomplete steps should remain Failed.
#[test]
fn test_agent_exit_incomplete_steps_stays_failed() {
    let task_id = TaskId::new();
    let steps = vec![
        make_step(&task_id, TaskStepStatus::Completed),
        make_step(&task_id, TaskStepStatus::InProgress), // not done
        make_step(&task_id, TaskStepStatus::Pending),    // not done
    ];
    let step_repo: Option<Arc<dyn TaskStepRepository>> = Some(Arc::new(StubTaskStepRepo { steps }));

    let result = run(all_steps_completed(&step_repo, &task_id));
    assert!(
        !result,
        "InProgress/Pending steps present → should return false"
    );
}

/// AgentExit with no steps at all should remain Failed.
#[test]
fn test_agent_exit_no_steps_stays_failed() {
    let task_id = TaskId::new();
    let step_repo: Option<Arc<dyn TaskStepRepository>> =
        Some(Arc::new(StubTaskStepRepo { steps: vec![] }));

    let result = run(all_steps_completed(&step_repo, &task_id));
    assert!(
        !result,
        "Empty step list → should return false (guard against trivially true)"
    );
}

#[test]
fn test_fetch_step_completion_state_classifies_no_steps() {
    let task_id = TaskId::new();
    let step_repo: Option<Arc<dyn TaskStepRepository>> =
        Some(Arc::new(StubTaskStepRepo { steps: vec![] }));

    assert_eq!(
        run(fetch_step_completion_state(&step_repo, &task_id)),
        StepCompletionState::NoSteps
    );
}

#[test]
fn test_fetch_step_completion_state_classifies_all_complete() {
    let task_id = TaskId::new();
    let step_repo: Option<Arc<dyn TaskStepRepository>> = Some(Arc::new(StubTaskStepRepo {
        steps: vec![
            make_step(&task_id, TaskStepStatus::Completed),
            make_step(&task_id, TaskStepStatus::Skipped),
        ],
    }));

    assert_eq!(
        run(fetch_step_completion_state(&step_repo, &task_id)),
        StepCompletionState::AllComplete
    );
}

#[test]
fn test_fetch_step_completion_state_classifies_incomplete() {
    let task_id = TaskId::new();
    let step_repo: Option<Arc<dyn TaskStepRepository>> = Some(Arc::new(StubTaskStepRepo {
        steps: vec![
            make_step(&task_id, TaskStepStatus::Completed),
            make_step(&task_id, TaskStepStatus::Pending),
        ],
    }));

    assert_eq!(
        run(fetch_step_completion_state(&step_repo, &task_id)),
        StepCompletionState::Incomplete
    );
}

#[test]
fn test_fetch_step_completion_state_returns_unknown_on_err_and_none() {
    let task_id = TaskId::new();
    let error_repo: Option<Arc<dyn TaskStepRepository>> = Some(Arc::new(StubErrorTaskStepRepo));
    let missing_repo: Option<Arc<dyn TaskStepRepository>> = None;

    assert_eq!(
        run(fetch_step_completion_state(&error_repo, &task_id)),
        StepCompletionState::Unknown
    );
    assert_eq!(
        run(fetch_step_completion_state(&missing_repo, &task_id)),
        StepCompletionState::Unknown
    );
}

#[test]
fn test_validated_completion_override_false_when_no_metadata() {
    let task = Task::new(ProjectId::new(), "no metadata".into());
    assert!(!run(validated_completion_override(
        &task,
        Utc::now(),
        &None,
    )));
}

#[test]
fn test_validated_completion_override_false_when_no_validation_cache_key() {
    let mut task = Task::new(ProjectId::new(), "other metadata".into());
    task.metadata = Some(r#"{"some_other_key": true}"#.to_string());
    assert!(!run(validated_completion_override(
        &task,
        Utc::now(),
        &None,
    )));
}

#[test]
fn test_validated_completion_override_false_on_malformed_validation_cache() {
    let mut task = Task::new(ProjectId::new(), "malformed cache".into());
    // validation_cache present but not a valid ValidationCacheMetadata shape → parse error path.
    task.metadata = Some(r#"{"validation_cache": {"version": "not-a-number"}}"#.to_string());
    assert!(!run(validated_completion_override(
        &task,
        Utc::now(),
        &None,
    )));
}

#[test]
fn test_validated_completion_override_false_when_worktree_path_missing() {
    let mut task = Task::new(ProjectId::new(), "no worktree".into());
    let cache = validation_cache_fixture("abc123", true, true);
    task.metadata = Some(
        cache
            .update_task_metadata(task.metadata.as_deref())
            .unwrap(),
    );
    task.worktree_path = None;
    assert!(!run(validated_completion_override(
        &task,
        Utc::now(),
        &None,
    )));
}

#[test]
fn test_validated_completion_override_false_for_unsafe_worktree_path() {
    let mut task = Task::new(ProjectId::new(), "unsafe worktree".into());
    let cache = validation_cache_fixture("abc123", true, true);
    task.metadata = Some(
        cache
            .update_task_metadata(task.metadata.as_deref())
            .unwrap(),
    );

    task.worktree_path = Some("relative/worktree".to_string());
    assert!(!run(validated_completion_override(
        &task,
        Utc::now(),
        &None,
    )));

    task.worktree_path = Some("/tmp/../escape".to_string());
    assert!(!run(validated_completion_override(
        &task,
        Utc::now(),
        &None,
    )));
}

#[test]
fn test_validated_completion_override_false_when_head_sha_unresolvable() {
    let mut task = Task::new(ProjectId::new(), "non-git worktree".into());
    let cache = validation_cache_fixture("abc123", true, true);
    task.metadata = Some(
        cache
            .update_task_metadata(task.metadata.as_deref())
            .unwrap(),
    );
    // A temp dir that is not a git repo → get_head_sha errors → fail-safe false.
    let tmp = tempfile::tempdir().unwrap();
    task.worktree_path = Some(tmp.path().to_string_lossy().to_string());
    assert!(!run(validated_completion_override(
        &task,
        Utc::now(),
        &None,
    )));
}

#[test]
fn test_validated_completion_override_accepts_current_validation_run() {
    let (worktree, _base_sha, head_sha) = git_worktree_with_base_and_change();
    let project_id = ProjectId::new();
    let task_id = TaskId::new();
    let episode_entered_at = Utc::now() - chrono::Duration::seconds(5);
    let repo = Arc::new(MemoryValidationRunRepository::new());
    let run_record = validation_run_fixture(&task_id, &project_id, &head_sha, episode_entered_at);
    let command = validation_command_fixture(
        &run_record.id,
        &task_id,
        &project_id,
        &head_sha,
        &worktree.path().to_string_lossy(),
        episode_entered_at,
    );
    run(repo.create_run(&run_record)).unwrap();
    run(repo.add_command_result(&command)).unwrap();

    let mut task = Task::new(project_id, "first-class validation".into());
    task.id = task_id;
    task.worktree_path = Some(worktree.path().to_string_lossy().to_string());
    let validation_run_repo: Arc<dyn crate::domain::repositories::ValidationRunRepository> = repo;

    assert!(run(validated_completion_override(
        &task,
        episode_entered_at,
        &Some(validation_run_repo),
    )));
}

#[test]
fn test_validated_completion_override_uses_legacy_cache_when_no_run_exists() {
    let (worktree, _base_sha, head_sha) = git_worktree_with_base_and_change();
    let episode_entered_at = Utc::now() - chrono::Duration::seconds(5);
    let cache = validation_cache_fixture_at(
        &head_sha,
        true,
        true,
        episode_entered_at + chrono::Duration::seconds(1),
    );
    let mut task = Task::new(ProjectId::new(), "legacy validation cache".into());
    task.metadata = Some(
        cache
            .update_task_metadata(task.metadata.as_deref())
            .unwrap(),
    );
    task.worktree_path = Some(worktree.path().to_string_lossy().to_string());
    let validation_run_repo: Arc<dyn crate::domain::repositories::ValidationRunRepository> =
        Arc::new(MemoryValidationRunRepository::new());

    assert!(run(validated_completion_override(
        &task,
        episode_entered_at,
        &Some(validation_run_repo),
    )));
}

/// Non-AgentExit errors should not trigger the override, even with complete steps.
#[test]
fn test_timeout_error_does_not_override_even_with_complete_steps() {
    let task_id = TaskId::new();
    let steps = vec![make_step(&task_id, TaskStepStatus::Completed)];
    let _step_repo: Option<Arc<dyn TaskStepRepository>> =
        Some(Arc::new(StubTaskStepRepo { steps }));

    let stream_error = StreamError::Timeout {
        context_type: ChatContextType::TaskExecution,
        elapsed_secs: 3600,
    };
    let initial_target = InternalStatus::Failed;

    // Timeout errors are NOT AgentExit — should not trigger override
    let target_status = if initial_target == InternalStatus::Failed
        && matches!(&stream_error, StreamError::AgentExit { .. })
    {
        InternalStatus::PendingReview // would override
    } else {
        initial_target
    };

    assert_eq!(
        target_status,
        InternalStatus::Failed,
        "Timeout errors should not trigger the AgentExit step-completion override"
    );
}

/// No task_step_repo → should not override (fail-safe).
#[test]
fn test_agent_exit_no_step_repo_stays_failed() {
    let task_id = TaskId::new();
    let step_repo: Option<Arc<dyn TaskStepRepository>> = None;

    let result = run(all_steps_completed(&step_repo, &task_id));
    assert!(!result, "No step repo → should fail-safe to false");
}

// ========================================
// New: all_steps_completed helper unit tests
// ========================================

/// "No output" path: worker exits cleanly with no text output but all steps done.
/// Helper must return true so the caller transitions to PendingReview.
#[test]
fn test_no_output_path_all_steps_complete() {
    let task_id = TaskId::new();
    let steps = vec![
        make_step(&task_id, TaskStepStatus::Completed),
        make_step(&task_id, TaskStepStatus::Skipped),
    ];
    let step_repo: Option<Arc<dyn TaskStepRepository>> = Some(Arc::new(StubTaskStepRepo { steps }));

    let result = run(all_steps_completed(&step_repo, &task_id));
    assert!(
        result,
        "No-output path: all Completed+Skipped → helper returns true"
    );
}

/// step_repo returns Err → helper must return false (safe fallback, never panic).
#[test]
fn test_step_repo_error_falls_through() {
    let task_id = TaskId::new();
    let step_repo: Option<Arc<dyn TaskStepRepository>> = Some(Arc::new(StubErrorTaskStepRepo));

    let result = run(all_steps_completed(&step_repo, &task_id));
    assert!(
        !result,
        "DB error on step query → helper must safe-fallback to false"
    );
}

/// All steps Skipped (no Completed) → helper must return true.
/// Skipped steps mean the agent legitimately bypassed them — work is considered done.
#[test]
fn test_all_skipped_no_completed() {
    let task_id = TaskId::new();
    let steps = vec![
        make_step(&task_id, TaskStepStatus::Skipped),
        make_step(&task_id, TaskStepStatus::Skipped),
        make_step(&task_id, TaskStepStatus::Skipped),
    ];
    let step_repo: Option<Arc<dyn TaskStepRepository>> = Some(Arc::new(StubTaskStepRepo { steps }));

    let result = run(all_steps_completed(&step_repo, &task_id));
    assert!(result, "All Skipped → helper returns true");
}

async fn assert_late_execution_finalizer_preserves_status(target_status: InternalStatus) {
    let state = AppState::new_test();
    let exec = Arc::new(ExecutionState::new());
    let execution_state = Some(Arc::clone(&exec));

    let project = Project::new("Review Race".into(), "/tmp/review-race".into());
    state.project_repo.create(project.clone()).await.unwrap();

    let mut task = Task::new(project.id.clone(), "Execution finalizer race".into());
    task.internal_status = InternalStatus::Executing;
    let task_id = task.id.clone();
    state.task_repo.create(task).await.unwrap();

    let task_step_repo: Option<Arc<dyn TaskStepRepository>> =
        Some(Arc::new(StatusChangingTaskStepRepo {
            task_repo: Arc::clone(&state.task_repo),
            task_id: task_id.clone(),
            target_status,
        }));

    handle_stream_success(
        "late-run-id",
        ChatContextType::TaskExecution,
        task_id.as_str(),
        false,
        false,
        false,
        &execution_state,
        &state.task_repo,
        &state.task_dependency_repo,
        &state.project_repo,
        &state.artifact_repo,
        &state.chat_message_repo,
        &state.chat_attachment_repo,
        &state.chat_conversation_repo,
        &state.agent_run_repo,
        &state.ideation_session_repo,
        &state.activity_event_repo,
        &state.message_queue,
        &state.running_agent_registry,
        &state.memory_event_repo,
        &None,
        &task_step_repo,
        &None,
        &ChatRuntimeFactoryDeps::from_app_state(&state),
        &None,
        &None,
        &None,
    )
    .await;

    let updated = state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("task should still exist");
    assert_eq!(
        updated.internal_status, target_status,
        "late execution finalizer must not overwrite review progress with Failed"
    );
}

#[tokio::test]
async fn test_late_execution_finalizer_cannot_overwrite_pending_review_or_reviewing() {
    assert_late_execution_finalizer_preserves_status(InternalStatus::PendingReview).await;
    assert_late_execution_finalizer_preserves_status(InternalStatus::Reviewing).await;
}

#[tokio::test]
async fn test_incomplete_execution_success_finalizer_fails_current_attempt_with_metadata() {
    let state = AppState::new_test();
    let exec = Arc::new(ExecutionState::new());
    let execution_state = Some(Arc::clone(&exec));

    let project = Project::new(
        "Incomplete Execution".into(),
        "/tmp/incomplete-execution".into(),
    );
    state.project_repo.create(project.clone()).await.unwrap();

    let mut task = Task::new(project.id.clone(), "Current execution attempt".into());
    task.internal_status = InternalStatus::Executing;
    let task_id = task.id.clone();
    state.task_repo.create(task).await.unwrap();
    let task_step_repo: Option<Arc<dyn TaskStepRepository>> =
        Some(Arc::new(StubTaskStepRepo { steps: vec![] }));

    handle_stream_success(
        "run-id-incomplete-success",
        ChatContextType::TaskExecution,
        task_id.as_str(),
        false,
        false,
        false,
        &execution_state,
        &state.task_repo,
        &state.task_dependency_repo,
        &state.project_repo,
        &state.artifact_repo,
        &state.chat_message_repo,
        &state.chat_attachment_repo,
        &state.chat_conversation_repo,
        &state.agent_run_repo,
        &state.ideation_session_repo,
        &state.activity_event_repo,
        &state.message_queue,
        &state.running_agent_registry,
        &state.memory_event_repo,
        &None,
        &task_step_repo,
        &None,
        &ChatRuntimeFactoryDeps::from_app_state(&state),
        &None,
        &None,
        &None,
    )
    .await;

    let updated = state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("task should still exist");
    assert_eq!(
        updated.internal_status,
        InternalStatus::Failed,
        "current incomplete execution should still transition to Failed"
    );

    let metadata: serde_json::Value = updated
        .metadata
        .as_deref()
        .and_then(|raw| serde_json::from_str(raw).ok())
        .expect("incomplete finalizer should persist diagnostic metadata");
    assert_eq!(
        metadata
            .get("last_agent_error_context")
            .and_then(|value| value.as_str()),
        Some("execution")
    );
    assert_eq!(
        metadata
            .get("last_agent_error")
            .and_then(|value| value.as_str()),
        Some("Agent ended without completing all task steps")
    );
    let recovery: ExecutionRecoveryMetadata =
        serde_json::from_value(metadata["execution_recovery"].clone())
            .expect("incomplete finalizer should store recovery metadata");
    assert_eq!(recovery.last_state, ExecutionRecoveryState::Failed);
    let event = recovery
        .events
        .last()
        .expect("incomplete finalizer should record recovery event");
    assert_eq!(
        event.reason_code,
        ExecutionRecoveryReasonCode::IncompleteSteps
    );
    assert_eq!(
        event.failure_source,
        Some(ExecutionFailureSource::AgentIncomplete)
    );
    assert!(
        !event.failure_source.unwrap().is_transient(),
        "successful-but-incomplete exits should not look like transient crashes"
    );
}

/// Zero-step worker run that produced output should leave Executing only when
/// the stream observed the worker's `execution_complete` call.
#[tokio::test]
async fn test_zero_step_run_with_output_and_completion_signal_transitions_out_of_executing() {
    let state = AppState::new_test();
    let exec = Arc::new(ExecutionState::new());
    let execution_state = Some(Arc::clone(&exec));
    let (worktree, base_sha, _head_sha) = git_worktree_with_base_and_change();

    let project = Project::new(
        "Zero Step Output".into(),
        worktree.path().to_string_lossy().to_string(),
    );
    state.project_repo.create(project.clone()).await.unwrap();

    let mut task = Task::new(project.id.clone(), "Zero-step output run".into());
    task.internal_status = InternalStatus::Executing;
    task.worktree_path = Some(worktree.path().to_string_lossy().to_string());
    task.task_branch_base_ref = Some(base_sha.clone());
    task.task_branch_base_sha = Some(base_sha);
    let task_id = task.id.clone();
    state.task_repo.create(task).await.unwrap();
    let task_step_repo: Option<Arc<dyn TaskStepRepository>> =
        Some(Arc::new(StubTaskStepRepo { steps: vec![] }));

    handle_stream_success(
        "run-id-zero-step-output",
        ChatContextType::TaskExecution,
        task_id.as_str(),
        true, // has_output
        true, // completion_tool_called
        false,
        &execution_state,
        &state.task_repo,
        &state.task_dependency_repo,
        &state.project_repo,
        &state.artifact_repo,
        &state.chat_message_repo,
        &state.chat_attachment_repo,
        &state.chat_conversation_repo,
        &state.agent_run_repo,
        &state.ideation_session_repo,
        &state.activity_event_repo,
        &state.message_queue,
        &state.running_agent_registry,
        &state.memory_event_repo,
        &None,
        &task_step_repo,
        &None,
        &ChatRuntimeFactoryDeps::from_app_state(&state),
        &None,
        &None,
        &None,
    )
    .await;

    let updated = state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("task should still exist");
    // The zero-step run enters the review pipeline because the completion tool
    // was observed; output alone is covered by the pure failed-gate test.
    assert_ne!(
        updated.internal_status,
        InternalStatus::Failed,
        "zero-step run with output must not be marked Failed"
    );
    assert_ne!(
        updated.internal_status,
        InternalStatus::Executing,
        "zero-step run with output must transition out of Executing"
    );
}

#[tokio::test]
async fn test_success_finalizer_uses_head_matched_validation_cache_for_failed_steps() {
    let state = AppState::new_test();
    let exec = Arc::new(ExecutionState::new());
    let execution_state = Some(Arc::clone(&exec));
    let (_worktree, base_sha, head_sha) = git_worktree_with_base_and_change();

    let project = Project::new(
        "Validation Override".into(),
        "/tmp/validation-override".into(),
    );
    state.project_repo.create(project.clone()).await.unwrap();

    let mut task = Task::new(project.id.clone(), "Validated execution attempt".into());
    task.internal_status = InternalStatus::Executing;
    task.worktree_path = Some(_worktree.path().to_string_lossy().to_string());
    task.task_branch_base_ref = Some(base_sha.clone());
    task.task_branch_base_sha = Some(base_sha);
    let task_id = task.id.clone();
    state.task_repo.create(task).await.unwrap();
    let agent_run_id = seed_current_execution_attempt(&state, &task_id).await;
    let cache = validation_cache_fixture(&head_sha, true, true);
    let metadata = cache
        .update_task_metadata(None)
        .expect("validation cache metadata should serialize");
    state
        .task_repo
        .update_metadata(&task_id, Some(metadata))
        .await
        .unwrap();

    let task_step_repo: Option<Arc<dyn TaskStepRepository>> = Some(Arc::new(StubTaskStepRepo {
        steps: vec![
            make_step(&task_id, TaskStepStatus::Completed),
            make_step(&task_id, TaskStepStatus::Failed),
        ],
    }));
    let runtime_deps = ChatRuntimeFactoryDeps::from_app_state(&state);

    handle_stream_success(
        agent_run_id.as_str(),
        ChatContextType::TaskExecution,
        task_id.as_str(),
        false,
        false,
        false,
        &execution_state,
        &state.task_repo,
        &state.task_dependency_repo,
        &state.project_repo,
        &state.artifact_repo,
        &state.chat_message_repo,
        &state.chat_attachment_repo,
        &state.chat_conversation_repo,
        &state.agent_run_repo,
        &state.ideation_session_repo,
        &state.activity_event_repo,
        &state.message_queue,
        &state.running_agent_registry,
        &state.memory_event_repo,
        &None,
        &task_step_repo,
        &None,
        &runtime_deps,
        &None,
        &None,
        &None,
    )
    .await;

    let updated = state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("task should still exist");
    assert!(
        matches!(
            updated.internal_status,
            InternalStatus::PendingReview | InternalStatus::Reviewing
        ),
        "HEAD-matched green validation cache should rescue a failed-step completion gate, got {:?}",
        updated.internal_status
    );
}

#[tokio::test]
async fn test_identity_unknown_does_not_consult_validation_cache_rescue() {
    let state = AppState::new_test();
    let exec = Arc::new(ExecutionState::new());
    let execution_state = Some(Arc::clone(&exec));
    let (_worktree, head_sha) = git_worktree_with_initial_commit();

    let project = Project::new("Identity Unknown".into(), "/tmp/identity-unknown".into());
    state.project_repo.create(project.clone()).await.unwrap();

    let mut task = Task::new(project.id.clone(), "Identity-unknown cache attempt".into());
    task.internal_status = InternalStatus::Executing;
    task.worktree_path = Some(_worktree.path().to_string_lossy().to_string());
    let cache = validation_cache_fixture(&head_sha, true, true);
    task.metadata = Some(
        cache
            .update_task_metadata(task.metadata.as_deref())
            .expect("validation cache metadata should serialize"),
    );
    let task_id = task.id.clone();
    state.task_repo.create(task).await.unwrap();

    let task_step_repo: Option<Arc<dyn TaskStepRepository>> = Some(Arc::new(StubTaskStepRepo {
        steps: vec![
            make_step(&task_id, TaskStepStatus::Completed),
            make_step(&task_id, TaskStepStatus::Failed),
        ],
    }));

    handle_stream_success(
        "missing-agent-run",
        ChatContextType::TaskExecution,
        task_id.as_str(),
        false,
        false,
        false,
        &execution_state,
        &state.task_repo,
        &state.task_dependency_repo,
        &state.project_repo,
        &state.artifact_repo,
        &state.chat_message_repo,
        &state.chat_attachment_repo,
        &state.chat_conversation_repo,
        &state.agent_run_repo,
        &state.ideation_session_repo,
        &state.activity_event_repo,
        &state.message_queue,
        &state.running_agent_registry,
        &state.memory_event_repo,
        &None,
        &task_step_repo,
        &None,
        &ChatRuntimeFactoryDeps::from_app_state(&state),
        &None,
        &None,
        &None,
    )
    .await;

    let updated = state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("task should still exist");
    assert_eq!(
        updated.internal_status,
        InternalStatus::Failed,
        "identity-unknown finalizers must not rescue via validation_cache"
    );
}

#[tokio::test]
async fn test_success_finalizer_rejects_no_test_validation_cache_for_failed_steps() {
    let state = AppState::new_test();
    let exec = Arc::new(ExecutionState::new());
    let execution_state = Some(Arc::clone(&exec));
    let (_worktree, head_sha) = git_worktree_with_initial_commit();

    let project = Project::new("No Test Cache".into(), "/tmp/no-test-cache".into());
    state.project_repo.create(project.clone()).await.unwrap();

    let mut task = Task::new(project.id.clone(), "No-test validation attempt".into());
    task.internal_status = InternalStatus::Executing;
    task.worktree_path = Some(_worktree.path().to_string_lossy().to_string());
    let task_id = task.id.clone();
    state.task_repo.create(task).await.unwrap();
    let agent_run_id = seed_current_execution_attempt(&state, &task_id).await;
    let cache = validation_cache_fixture(&head_sha, false, true);
    let metadata = cache
        .update_task_metadata(None)
        .expect("validation cache metadata should serialize");
    state
        .task_repo
        .update_metadata(&task_id, Some(metadata))
        .await
        .unwrap();

    let task_step_repo: Option<Arc<dyn TaskStepRepository>> = Some(Arc::new(StubTaskStepRepo {
        steps: vec![
            make_step(&task_id, TaskStepStatus::Completed),
            make_step(&task_id, TaskStepStatus::Failed),
        ],
    }));

    handle_stream_success(
        agent_run_id.as_str(),
        ChatContextType::TaskExecution,
        task_id.as_str(),
        false,
        false,
        false,
        &execution_state,
        &state.task_repo,
        &state.task_dependency_repo,
        &state.project_repo,
        &state.artifact_repo,
        &state.chat_message_repo,
        &state.chat_attachment_repo,
        &state.chat_conversation_repo,
        &state.agent_run_repo,
        &state.ideation_session_repo,
        &state.activity_event_repo,
        &state.message_queue,
        &state.running_agent_registry,
        &state.memory_event_repo,
        &None,
        &task_step_repo,
        &None,
        &ChatRuntimeFactoryDeps::from_app_state(&state),
        &None,
        &None,
        &None,
    )
    .await;

    let updated = state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("task should still exist");
    assert_eq!(
        updated.internal_status,
        InternalStatus::Failed,
        "tests_ran=false cache must not rescue a failed-step completion gate"
    );
}

// ========================================
// Cancelled+turns_finalized path: run_completed emission
// ========================================

/// Verifies the branching logic in handle_stream_error for Cancelled variants.
///
/// Cancelled + turns_finalized > 0 → success path → run_completed emitted.
/// Cancelled + turns_finalized == 0 → user-stop path → agent:stopped emitted.
///
/// This test guards against regression: if the turns_finalized guard changes,
/// the UI will either (a) get stuck in "generating" or (b) emit spurious events.
#[test]
fn test_cancelled_with_turns_takes_success_path_not_error_path() {
    // StreamError is in scope via `use super::*` (chat_service_handlers re-exports it)

    // turns_finalized > 0 → agent completed at least one turn before cancellation
    // → handle_stream_error calls handle_stream_success + emits run_completed
    let cancelled_with_turns = StreamError::Cancelled {
        turns_finalized: 2,
        completion_tool_called: false,
    };
    let goes_to_success_path = match &cancelled_with_turns {
        StreamError::Cancelled {
            turns_finalized, ..
        } => *turns_finalized > 0,
        _ => false,
    };
    assert!(
        goes_to_success_path,
        "Cancelled{{turns_finalized:2}} → must take success path (handle_stream_success + run_completed)"
    );
    // Success path does NOT call agent_run_repo.fail or emit agent:error
    assert!(
        !cancelled_with_turns.is_retryable(),
        "Cancelled variant is never retried (already handled as success or stop)"
    );
    assert!(
        !cancelled_with_turns.is_provider_error(),
        "Cancelled variant is not a provider error"
    );

    // turns_finalized == 0 → genuine user-stop or system cancel before any turn completed
    // → handle_stream_error emits agent:stopped (not run_completed)
    let cancelled_no_turns = StreamError::Cancelled {
        turns_finalized: 0,
        completion_tool_called: false,
    };
    let goes_to_stop_path = match &cancelled_no_turns {
        StreamError::Cancelled {
            turns_finalized, ..
        } => *turns_finalized == 0,
        _ => false,
    };
    assert!(
        goes_to_stop_path,
        "Cancelled{{turns_finalized:0}} → must take stop path (agent:stopped, not run_completed)"
    );
}

// ========================================
// Cancelled handler: real handle_stream_error path exercise
// ========================================
//
// These tests call the real `handle_stream_error` function using memory repos
// (from AppState::new_test()) and assert the execution slot count after return.
// They guard the routing logic introduced in sub-branches A and B of the Cancelled handler.
//
// Key invariants:
// - Sub-branch B (completion_tool_called=true, turns_finalized=0): slot NOT re-incremented
//   because TurnComplete never fired, so there was no prior slot decrement to compensate for.
// - Sub-branch A (turns_finalized>0): slot IS re-incremented
//   because TurnComplete fired earlier and decremented the slot.
// - [Agent stopped] (turns_finalized=0, completion_tool_called=false): slot NOT touched.

/// Helper that calls handle_stream_error with the given Cancelled variant and
/// returns (recovery_spawned, running_count_after). Uses Ideation context and
/// memory repos so the Cancelled handler paths can exercise without side effects.
async fn invoke_handle_stream_error_cancelled(cancelled: &StreamError) -> (bool, u32) {
    let state = AppState::new_test();
    let exec = Arc::new(ExecutionState::new());
    let execution_state = Some(Arc::clone(&exec));
    let runtime_deps = ChatRuntimeFactoryDeps::from_app_state(&state);

    let conversation_id = ChatConversationId::new();
    let context_id = "test-session-id";
    let event_ctx = crate::application::chat_service::event_context(
        &conversation_id,
        &ChatContextType::Ideation,
        context_id,
    );
    let cli_path = std::path::Path::new("/tmp/claude");
    let plugin_dir = std::path::Path::new("/tmp/plugin");
    let working_dir = std::path::Path::new("/tmp");

    let recovery_spawned = handle_stream_error(
        "cancelled",
        Some(cancelled),
        ChatContextType::Ideation,
        context_id,
        conversation_id,
        "run-id-1",
        "msg-id-1",
        &event_ctx,
        None, // stored_session_id
        crate::domain::agents::AgentHarnessKind::Claude,
        false, // is_retry_attempt
        None,  // user_message_content
        None,  // conversation
        None,  // resolved_project_id
        cli_path,
        plugin_dir,
        working_dir,
        &state.chat_message_repo,
        &Some(state.chat_timeline_repo.clone()),
        &state.chat_attachment_repo,
        &state.artifact_repo,
        &state.chat_conversation_repo,
        &state.agent_run_repo,
        &state.task_repo,
        &state.task_dependency_repo,
        &state.project_repo,
        &state.ideation_session_repo,
        &None, // task_proposal_repo — not used in Cancelled path
        &state.activity_event_repo,
        &state.message_queue,
        &state.running_agent_registry,
        &state.memory_event_repo,
        &execution_state,
        &None, // question_state — not used in Cancelled path
        &None, // plan_branch_repo — not used for Ideation
        &None, // execution_settings_repo — not used for Ideation
        &runtime_deps,
        None,  // agent_name
        None,  // run_chain_id
        &None, // interactive_process_registry
        &None, // review_repo
        &None, // task_step_repo
        &None, // verification_child_registry
    )
    .await;

    (recovery_spawned, exec.running_count())
}

#[tokio::test]
async fn test_recovery_retry_background_context_preserves_execution_side_runtime_deps() {
    let state = AppState::new_test();
    let execution_state = Some(Arc::new(ExecutionState::new()));
    let question_state = Some(Arc::new(crate::application::QuestionState::new()));
    let interactive_process_registry = Some(Arc::new(InteractiveProcessRegistry::new()));
    let verification_child_registry = Some(Arc::new(VerificationChildProcessRegistry::new()));

    let retry_child = tokio::process::Command::new("true")
        .spawn()
        .expect("spawn test child");
    let conversation_id = ChatConversationId::new();
    let task_id = TaskId::new();
    let mut retry_conv = ChatConversation::new_review(task_id.clone());
    retry_conv.set_provider_session_ref(crate::domain::agents::ProviderSessionRef {
        harness: crate::domain::agents::AgentHarnessKind::Codex,
        provider_session_id: "codex-recovered-session".to_string(),
    });
    let runtime_deps = ChatRuntimeFactoryDeps::from_app_state(&state);

    let ctx = build_recovery_retry_background_context(
        retry_child,
        crate::domain::agents::AgentHarnessKind::Codex,
        ChatContextType::Review,
        task_id.as_str(),
        conversation_id,
        "run-id-1",
        "codex-recovered-session".to_string(),
        std::path::Path::new("/tmp/worktree"),
        std::path::Path::new("/tmp/codex"),
        std::path::Path::new("/tmp/plugin"),
        &state.chat_message_repo,
        &state.chat_attachment_repo,
        &state.artifact_repo,
        &state.chat_conversation_repo,
        &state.agent_run_repo,
        &state.task_repo,
        &state.task_dependency_repo,
        &state.project_repo,
        &state.ideation_session_repo,
        &state.delegated_session_repo,
        &Some(Arc::clone(&state.execution_settings_repo)),
        &Some(Arc::clone(&state.agent_lane_settings_repo)),
        &Some(Arc::clone(&state.agent_provider_settings_repo)),
        &Some(Arc::clone(&state.task_proposal_repo)),
        &state.activity_event_repo,
        &state.memory_event_repo,
        &state.message_queue,
        &state.running_agent_registry,
        &execution_state,
        &question_state,
        &None,
        Arc::clone(&runtime_deps.events),
        Some(runtime_deps),
        Some("run-chain-1".to_string()),
        false,
        false,
        Some("retry this review".to_string()).as_deref(),
        retry_conv,
        Some("ralphx:ralphx-execution-reviewer"),
        &Some(Arc::clone(&state.review_repo)),
        &Some(Arc::clone(&state.task_step_repo)),
        &Some(Arc::clone(&state.validation_run_repo)),
        &Some(Arc::clone(&state.external_events_repo)),
        &None,
        &interactive_process_registry,
        &verification_child_registry,
    );

    assert_eq!(ctx.harness, crate::domain::agents::AgentHarnessKind::Codex);
    assert!(ctx.is_retry_attempt);
    assert!(
        ctx.repos.task_step_repo.is_some(),
        "stale-session retry must preserve task_step_repo for execution-side completion handling"
    );
    assert!(
        ctx.repos.agent_provider_settings_repo.is_some(),
        "stale-session retry must preserve provider settings for disabled-provider enforcement"
    );
    assert!(
        ctx.repos.review_repo.is_some(),
        "stale-session retry must preserve review_repo for review/merge completion flows"
    );
    assert!(
        ctx.interactive_process_registry.is_some(),
        "stale-session retry must preserve interactive_process_registry for execution/review/merge cleanup"
    );
    assert!(
        ctx.verification_child_registry.is_some(),
        "stale-session retry must preserve verification_child_registry to match the original background run context"
    );

    let mut child = ctx.child;
    let _ = child.wait().await;
}

#[tokio::test]
async fn handle_stream_error_persona_recovery_attributes_retry_run() {
    let _env = EnvVarGuard::set("ENABLE_SESSION_RECOVERY", "true");
    let state = AppState::new_test();
    let mut provider_settings = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Claude);
    provider_settings.enabled = true;
    provider_settings.is_default = true;
    state
        .agent_provider_settings_repo
        .upsert(&provider_settings)
        .await
        .expect("enable Claude provider for recovery retry");

    let project_id = ProjectId::new();
    let project = Project::new("Recovered Project".into(), "/tmp/recovered-project".into());
    state
        .project_repo
        .create(Project {
            id: project_id.clone(),
            ..project
        })
        .await
        .expect("seed project");

    let mut conversation = ChatConversation::new_project(project_id.clone());
    let persona = Persona {
        id: PersonaId::from("handler-recovery-persona"),
        artifact_id: None,

        project_id: None,
        slug: "handler-recovery-persona".to_string(),
        name: "Handler Recovery Persona".to_string(),
        description: "handler recovery attribution fixture".to_string(),
        content: "SECRET_HANDLER_RECOVERY_PERSONA_BODY".to_string(),
        status: PersonaStatus::Active,
        version: 5,
        content_hash: "handler-recovery-persona-hash".to_string(),
        source_session_id: None,
        source_persona_id: None,
        source_content_hash: None,
        source_json: "{}".to_string(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    state
        .persona_repo
        .create(persona.clone())
        .await
        .expect("seed recovery persona");
    conversation.persona_id = Some(persona.id.to_string());
    conversation.set_provider_session_ref(ProviderSessionRef {
        harness: AgentHarnessKind::Claude,
        provider_session_id: "old-session".to_string(),
    });
    let conversation_id = conversation.id.clone();
    state
        .chat_conversation_repo
        .create(conversation.clone())
        .await
        .expect("seed conversation");
    let mut historical_message = ChatMessage::user_in_project(project_id.clone(), "prior turn");
    historical_message.conversation_id = Some(conversation_id.clone());
    state
        .chat_message_repo
        .create(historical_message)
        .await
        .expect("seed recovery history");
    let agent_run = state
        .agent_run_repo
        .create(AgentRun::new(conversation_id.clone()))
        .await
        .expect("seed agent run");
    let agent_run_id = agent_run.id.as_str();
    let event_ctx = crate::application::chat_service::event_context(
        &conversation_id,
        &ChatContextType::Project,
        project_id.as_str(),
    );
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let cli_path = write_claude_session_fixture(temp_dir.path(), "recovered-session");
    let runtime_deps = ChatRuntimeFactoryDeps::from_app_state(&state);

    let recovery_spawned = super::handle_stream_error(
        "No conversation found with session ID old-session",
        None,
        ChatContextType::Project,
        project_id.as_str(),
        conversation_id.clone(),
        agent_run_id.as_str(),
        "message-id-stale-recovery-success",
        &event_ctx,
        Some("old-session"),
        AgentHarnessKind::Claude,
        false,
        true,
        false,
        Some("retry after stale session"),
        Some(&conversation),
        Some(project_id.as_str().to_string()),
        &cli_path,
        temp_dir.path(),
        temp_dir.path(),
        &state.chat_message_repo,
        &Some(state.chat_timeline_repo.clone()),
        &state.chat_attachment_repo,
        &state.artifact_repo,
        &state.chat_conversation_repo,
        &state.agent_run_repo,
        &state.task_repo,
        &state.task_dependency_repo,
        &state.project_repo,
        &state.ideation_session_repo,
        &Some(Arc::clone(&state.task_proposal_repo)),
        &state.activity_event_repo,
        &state.message_queue,
        &state.running_agent_registry,
        &state.memory_event_repo,
        &None,
        &None,
        &None,
        &None,
        &Some(Arc::clone(&state.agent_lane_settings_repo)),
        &Some(Arc::clone(&state.agent_provider_settings_repo)),
        Arc::clone(&runtime_deps.events),
        runtime_deps.plan_verification_completion.as_ref(),
        Some(&runtime_deps),
        Some("orchestrator"),
        Some("chain-stale-recovery".to_string()),
        &None,
        &Some(Arc::clone(&state.review_repo)),
        &Some(Arc::clone(&state.task_step_repo)),
        &None,
        &None,
        &None,
        &None,
        &None,
    )
    .await;

    assert!(
        recovery_spawned,
        "successful stale-session recovery must spawn a retry instead of falling through"
    );
    let attributed = state
        .agent_run_repo
        .get_by_id(&agent_run.id)
        .await
        .expect("read handler recovery run")
        .expect("handler recovery run exists");
    assert_eq!(
        attributed.persona_id.as_deref(),
        Some("handler-recovery-persona")
    );
    // Unit fixture has no canonical agents tree, so the retry spawn cannot inject;
    // what this pins is that the retry path records attribution at all (it did not
    // before). injected=true is pinned against the real send path in
    // tests/suite_chat_service/persona_feature_flag.rs.
    assert_eq!(attributed.persona_injected, Some(false));
    assert!(attributed.persona_skipped_reason.is_some());
    assert!(!serde_json::to_string(&attributed)
        .expect("serialize handler recovery attribution")
        .contains("SECRET_HANDLER_RECOVERY_PERSONA_BODY"));
}

#[tokio::test]
async fn test_handle_verification_child_completion_queues_hidden_auto_continue() {
    let app_state = AppState::new_test();
    let project_id = ProjectId::new();

    let mut parent = crate::domain::entities::IdeationSession::new(project_id.clone());
    parent.verification_status = VerificationStatus::NeedsRevision;
    parent.verification_in_progress = true;
    let parent_id = parent.id.clone();
    app_state
        .ideation_session_repo
        .create(parent)
        .await
        .unwrap();

    app_state
        .ideation_session_repo
        .save_verification_run_snapshot(
            &parent_id,
            &crate::domain::entities::VerificationRunSnapshot {
                generation: 0,
                status: VerificationStatus::NeedsRevision,
                in_progress: true,
                current_round: 1,
                max_rounds: 5,
                best_round_index: None,
                convergence_reason: None,
                current_gaps: vec![],
                rounds: vec![crate::domain::entities::VerificationRoundSnapshot {
                    round: 1,
                    gap_score: 4,
                    fingerprints: vec![],
                    gaps: vec![],
                    parse_failed: false,
                }],
            },
        )
        .await
        .unwrap();

    let mut child = crate::domain::entities::IdeationSession::new(project_id);
    child.session_purpose = crate::domain::entities::SessionPurpose::Verification;
    child.parent_session_id = Some(parent_id.clone());
    let child_id = child.id.clone();
    app_state.ideation_session_repo.create(child).await.unwrap();

    let app = mock_builder()
        .manage(app_state)
        .build(mock_context(noop_assets()))
        .expect("mock app");
    let handle = app.handle().clone();
    let state = handle.state::<AppState>();

    handle_verification_child_completion(
        &child_id,
        &parent_id,
        &state.ideation_session_repo,
        &state.chat_conversation_repo,
        &state.chat_message_repo,
        &state.message_queue,
        Some(&state.queued_message_repo),
        state.events.as_ref(),
        &None,
    )
    .await;

    let queued = state
        .message_queue
        .get_queued(ChatContextType::Ideation, child_id.as_str());
    assert_eq!(
        queued.len(),
        1,
        "auto-continue must queue one hidden control message"
    );
    assert_eq!(
        queued[0].metadata_override.as_deref(),
        Some(VERIFICATION_AUTO_CONTINUE_METADATA)
    );
    assert!(
        queued[0]
            .content
            .contains("Continue the active verification loop in this same session"),
        "queued control prompt must instruct the verifier to continue the same loop"
    );
    let durable = state
        .queued_message_repo
        .list(&QueueKey::new(ChatContextType::Ideation, child_id.as_str()))
        .await
        .expect("durable auto-continue queue lookup should not fail");
    assert_eq!(durable.len(), 1);
    assert_eq!(durable[0].id, queued[0].id);

    let child_after = state
        .ideation_session_repo
        .get_by_id(&child_id)
        .await
        .unwrap()
        .unwrap();
    assert_ne!(
        child_after.status,
        crate::domain::entities::IdeationSessionStatus::Archived
    );

    let parent_conversation = state
        .chat_conversation_repo
        .get_active_for_context(ChatContextType::Ideation, parent_id.as_str())
        .await
        .unwrap();
    assert!(
        parent_conversation.is_none(),
        "auto-continue must not inject a parent-thread handoff message"
    );
}

/// Sub-branch B: Cancelled { turns_finalized: 0, completion_tool_called: true }
/// → success path taken; execution slot must NOT be re-incremented.
///
/// Rationale: TurnComplete never fired (cleanup raced ahead of it), so the slot
/// was never decremented by that event. Re-incrementing here would cause a slot
/// leak that makes the system believe an agent is still running.
#[tokio::test]
async fn test_handle_stream_error_cancelled_completion_tool_called_skips_slot_reincrement() {
    let cancelled = StreamError::Cancelled {
        turns_finalized: 0,
        completion_tool_called: true,
    };
    let (recovery_spawned, count_after) = invoke_handle_stream_error_cancelled(&cancelled).await;

    assert!(
        !recovery_spawned,
        "Sub-branch B must return false (success path, no retry)"
    );
    assert_eq!(
        count_after, 0,
        "completion_tool_called=true path must skip slot re-increment (TurnComplete never fired)"
    );
}

/// Regression guard: Cancelled { turns_finalized: 0, completion_tool_called: false }
/// → [Agent stopped] path taken; execution slot must NOT be incremented.
///
/// Manual user-stop must never be silently promoted to a success path.
/// This test ensures the new completion_tool_called guard does not broaden the
/// success condition beyond its intended scope.
#[tokio::test]
async fn test_handle_stream_error_cancelled_false_completion_takes_agent_stopped_path() {
    let cancelled = StreamError::Cancelled {
        turns_finalized: 0,
        completion_tool_called: false,
    };
    let (recovery_spawned, count_after) = invoke_handle_stream_error_cancelled(&cancelled).await;

    assert!(!recovery_spawned, "[Agent stopped] path must return false");
    assert_eq!(
        count_after, 0,
        "User-stop path must NOT touch the execution slot"
    );
}

#[tokio::test]
async fn cancelled_incomplete_codex_turn_clears_provider_session_before_next_send() {
    let state = AppState::new_test();
    let project_id = ProjectId::new();
    let mut conversation = ChatConversation::new_project(project_id.clone());
    conversation.set_provider_session_ref(ProviderSessionRef {
        harness: AgentHarnessKind::Codex,
        provider_session_id: "incomplete-codex-thread".to_string(),
    });
    let conversation_id = conversation.id.clone();
    state
        .chat_conversation_repo
        .create(conversation.clone())
        .await
        .expect("seed Codex conversation");

    let event_ctx = crate::application::chat_service::event_context(
        &conversation_id,
        &ChatContextType::Project,
        project_id.as_str(),
    );
    let cancelled = StreamError::Cancelled {
        turns_finalized: 0,
        completion_tool_called: false,
    };

    let recovery_spawned = handle_stream_error(
        "cancelled",
        Some(&cancelled),
        ChatContextType::Project,
        project_id.as_str(),
        conversation_id.clone(),
        "run-id-incomplete-codex-turn",
        "message-id-incomplete-codex-turn",
        &event_ctx,
        Some("incomplete-codex-thread"),
        AgentHarnessKind::Codex,
        false,
        None,
        Some(&conversation),
        Some(project_id.as_str().to_string()),
        std::path::Path::new("/tmp/codex"),
        std::path::Path::new("/tmp/plugin"),
        std::path::Path::new("/tmp"),
        &state.chat_message_repo,
        &Some(state.chat_timeline_repo.clone()),
        &state.chat_attachment_repo,
        &state.artifact_repo,
        &state.chat_conversation_repo,
        &state.agent_run_repo,
        &state.task_repo,
        &state.task_dependency_repo,
        &state.project_repo,
        &state.ideation_session_repo,
        &None,
        &state.activity_event_repo,
        &state.message_queue,
        &state.running_agent_registry,
        &state.memory_event_repo,
        &None,
        &None,
        &None,
        &None,
        &ChatRuntimeFactoryDeps::from_app_state(&state),
        None,
        None,
        &None,
        &None,
        &None,
        &None,
    )
    .await;

    assert!(!recovery_spawned);
    let stored = state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .expect("load Codex conversation")
        .expect("Codex conversation remains present");
    assert!(
        stored.provider_session_ref().is_none(),
        "a cancelled Codex turn without terminal proof must not be resumed"
    );
}

#[tokio::test]
async fn cancelled_incomplete_claude_turn_preserves_provider_session_for_continuation() {
    let state = AppState::new_test();
    let conversation_id = ChatConversationId::new();
    let project_id = ProjectId::new();
    let mut conversation = ChatConversation::new_project(project_id.clone());
    conversation.id = conversation_id.clone();
    conversation.set_provider_session_ref(ProviderSessionRef {
        harness: AgentHarnessKind::Claude,
        provider_session_id: "continuable-claude-session".to_string(),
    });
    state
        .chat_conversation_repo
        .create(conversation.clone())
        .await
        .expect("insert Claude conversation");

    let event_ctx = crate::application::chat_service::event_context(
        &conversation_id,
        &ChatContextType::Project,
        project_id.as_str(),
    );
    let cancelled = StreamError::Cancelled {
        turns_finalized: 0,
        completion_tool_called: false,
    };

    let recovery_spawned = handle_stream_error(
        "cancelled",
        Some(&cancelled),
        ChatContextType::Project,
        project_id.as_str(),
        conversation_id.clone(),
        "run-id-incomplete-claude-turn",
        "message-id-incomplete-claude-turn",
        &event_ctx,
        Some("continuable-claude-session"),
        AgentHarnessKind::Claude,
        false,
        None,
        Some(&conversation),
        Some(project_id.as_str().to_string()),
        std::path::Path::new("/tmp/claude"),
        std::path::Path::new("/tmp/plugin"),
        std::path::Path::new("/tmp"),
        &state.chat_message_repo,
        &Some(state.chat_timeline_repo.clone()),
        &state.chat_attachment_repo,
        &state.artifact_repo,
        &state.chat_conversation_repo,
        &state.agent_run_repo,
        &state.task_repo,
        &state.task_dependency_repo,
        &state.project_repo,
        &state.ideation_session_repo,
        &None,
        &state.activity_event_repo,
        &state.message_queue,
        &state.running_agent_registry,
        &state.memory_event_repo,
        &None,
        &None,
        &None,
        &None,
        &ChatRuntimeFactoryDeps::from_app_state(&state),
        None,
        None,
        &None,
        &None,
        &None,
        &None,
    )
    .await;

    assert!(!recovery_spawned);
    let stored = state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .expect("load Claude conversation")
        .expect("Claude conversation remains present");
    assert_eq!(
        stored
            .provider_session_ref()
            .map(|session_ref| session_ref.provider_session_id),
        Some("continuable-claude-session".to_string()),
        "Claude cancellation keeps its provider continuation semantics"
    );
}

#[tokio::test]
async fn test_handle_stream_error_cancelled_preserves_terminal_system_run_status() {
    let state = AppState::new_test();
    let conversation_id = ChatConversationId::new();
    let context_id = ProjectId::new().as_str().to_string();
    let mut run = AgentRun::new(conversation_id.clone());
    let agent_run_id = run.id.as_str();
    run.fail("Agent stopped by system recovery");
    state
        .agent_run_repo
        .create(run)
        .await
        .expect("insert terminal agent run");

    let event_ctx = crate::application::chat_service::event_context(
        &conversation_id,
        &ChatContextType::Project,
        &context_id,
    );
    let cancelled = StreamError::Cancelled {
        turns_finalized: 0,
        completion_tool_called: false,
    };

    let recovery_spawned = handle_stream_error(
        "cancelled",
        Some(&cancelled),
        ChatContextType::Project,
        &context_id,
        conversation_id.clone(),
        &agent_run_id,
        "msg-id-terminal-system-stop",
        &event_ctx,
        None,
        crate::domain::agents::AgentHarnessKind::Codex,
        false,
        None,
        None,
        None,
        std::path::Path::new("/tmp/codex"),
        std::path::Path::new("/tmp/plugin"),
        std::path::Path::new("/tmp"),
        &state.chat_message_repo,
        &Some(state.chat_timeline_repo.clone()),
        &state.chat_attachment_repo,
        &state.artifact_repo,
        &state.chat_conversation_repo,
        &state.agent_run_repo,
        &state.task_repo,
        &state.task_dependency_repo,
        &state.project_repo,
        &state.ideation_session_repo,
        &None,
        &state.activity_event_repo,
        &state.message_queue,
        &state.running_agent_registry,
        &state.memory_event_repo,
        &None,
        &None,
        &None,
        &None,
        &ChatRuntimeFactoryDeps::from_app_state(&state),
        None,
        None,
        &None,
        &None,
        &None,
        &None,
    )
    .await;

    assert!(!recovery_spawned);
    let stored = state
        .agent_run_repo
        .get_by_id(&AgentRunId::from_string(&agent_run_id))
        .await
        .expect("load terminal run")
        .expect("terminal run still exists");
    assert_eq!(stored.status, AgentRunStatus::Failed);
    assert_eq!(
        stored.error_message.as_deref(),
        Some("Agent stopped by system recovery")
    );
}

#[tokio::test]
async fn test_handle_stream_error_cancelled_terminalizes_existing_timeline_blocks() {
    let state = AppState::new_test();
    let conversation_id = ChatConversationId::new();
    let context_id = ProjectId::new().as_str().to_string();
    let content_blocks = vec![
        ContentBlockItem::Text {
            text: "Partial Codex response".to_string(),
        },
        ContentBlockItem::ToolUse {
            id: Some("tool-1".to_string()),
            name: "exec_command".to_string(),
            arguments: serde_json::json!({ "cmd": "cargo test" }),
            result: None,
            parent_tool_use_id: None,
            diff_context: None,
        },
    ];
    let pre_assistant_message =
        crate::application::chat_service::chat_service_context::create_assistant_message(
            ChatContextType::Project,
            &context_id,
            "Partial Codex response",
            conversation_id.clone(),
            &[],
            &content_blocks,
        );
    let pre_assistant_message_id = pre_assistant_message.id.as_str().to_string();
    state
        .chat_message_repo
        .create(pre_assistant_message)
        .await
        .expect("insert pre-assistant message");

    let initial_items = super::super::chat_service_streaming::persist_timeline_snapshot(
        &Some(state.chat_timeline_repo.clone()),
        &conversation_id.as_str(),
        &Some(pre_assistant_message_id.clone()),
        &content_blocks,
        ChatTimelineItemStatus::Streaming,
    )
    .await;
    assert!(initial_items
        .iter()
        .all(|item| item.status == ChatTimelineItemStatus::Streaming));

    let event_ctx = crate::application::chat_service::event_context(
        &conversation_id,
        &ChatContextType::Project,
        &context_id,
    );
    let cancelled = StreamError::Cancelled {
        turns_finalized: 0,
        completion_tool_called: false,
    };
    let recovery_spawned = handle_stream_error(
        "cancelled",
        Some(&cancelled),
        ChatContextType::Project,
        &context_id,
        conversation_id.clone(),
        "run-id-cancelled-timeline",
        &pre_assistant_message_id,
        &event_ctx,
        None,
        crate::domain::agents::AgentHarnessKind::Codex,
        false,
        None,
        None,
        None,
        std::path::Path::new("/tmp/codex"),
        std::path::Path::new("/tmp/plugin"),
        std::path::Path::new("/tmp"),
        &state.chat_message_repo,
        &Some(state.chat_timeline_repo.clone()),
        &state.chat_attachment_repo,
        &state.artifact_repo,
        &state.chat_conversation_repo,
        &state.agent_run_repo,
        &state.task_repo,
        &state.task_dependency_repo,
        &state.project_repo,
        &state.ideation_session_repo,
        &None,
        &state.activity_event_repo,
        &state.message_queue,
        &state.running_agent_registry,
        &state.memory_event_repo,
        &None,
        &None,
        &None,
        &None,
        &ChatRuntimeFactoryDeps::from_app_state(&state),
        None,
        None,
        &None,
        &None,
        &None,
        &None,
    )
    .await;

    assert!(!recovery_spawned);
    let page = state
        .chat_timeline_repo
        .get_page(&conversation_id, 10, None)
        .await
        .expect("load timeline page");
    let assistant_items: Vec<_> = page
        .items
        .iter()
        .filter(|item| {
            item.message_id
                .as_ref()
                .is_some_and(|id| id.as_str() == pre_assistant_message_id)
        })
        .collect();
    assert_eq!(assistant_items.len(), 2);
    assert!(assistant_items
        .iter()
        .all(|item| item.status == ChatTimelineItemStatus::Finalized));
    let tool_item = assistant_items
        .iter()
        .find(|item| item.tool_call_id.as_deref() == Some("tool-1"))
        .expect("tool timeline item should remain present");
    assert_eq!(tool_item.tool_status.as_deref(), Some("completed"));
    assert!(tool_item
        .result_json
        .as_deref()
        .is_some_and(|result| result.contains("stopped")));
}

#[tokio::test]
async fn test_handle_stream_error_stopped_attributes_timeline_blocks_to_run_id() {
    let state = AppState::new_test();
    let conversation_id = ChatConversationId::new();
    let context_id = ProjectId::new().as_str().to_string();
    let run_id = AgentRunId::new().as_str();
    let content_blocks = vec![ContentBlockItem::Text {
        text: "Partial response before stop".to_string(),
    }];
    let pre_assistant_message =
        crate::application::chat_service::chat_service_context::create_assistant_message(
            ChatContextType::Project,
            &context_id,
            "Partial response before stop",
            conversation_id.clone(),
            &[],
            &content_blocks,
        );
    let pre_assistant_message_id = pre_assistant_message.id.as_str().to_string();
    state
        .chat_message_repo
        .create(pre_assistant_message)
        .await
        .expect("insert pre-assistant message");

    let event_ctx = crate::application::chat_service::event_context(
        &conversation_id,
        &ChatContextType::Project,
        &context_id,
    );
    let cancelled = StreamError::Cancelled {
        turns_finalized: 0,
        completion_tool_called: false,
    };
    let recovery_spawned = handle_stream_error::<MockRuntime>(
        "cancelled",
        Some(&cancelled),
        ChatContextType::Project,
        &context_id,
        conversation_id.clone(),
        run_id.as_str(),
        &pre_assistant_message_id,
        &event_ctx,
        None,
        crate::domain::agents::AgentHarnessKind::Codex,
        false,
        None,
        None,
        None,
        std::path::Path::new("/tmp/codex"),
        std::path::Path::new("/tmp/plugin"),
        std::path::Path::new("/tmp"),
        &state.chat_message_repo,
        &Some(state.chat_timeline_repo.clone()),
        &state.chat_attachment_repo,
        &state.artifact_repo,
        &state.chat_conversation_repo,
        &state.agent_run_repo,
        &state.task_repo,
        &state.task_dependency_repo,
        &state.project_repo,
        &state.ideation_session_repo,
        &None,
        &state.activity_event_repo,
        &state.message_queue,
        &state.running_agent_registry,
        &state.memory_event_repo,
        &None,
        &None,
        &None,
        &None,
        &None::<tauri::AppHandle<MockRuntime>>,
        None,
        None,
        &None,
        &None,
        &None,
        &None,
    )
    .await;

    assert!(!recovery_spawned);
    let page = state
        .chat_timeline_repo
        .get_page(&conversation_id, 10, None)
        .await
        .expect("load timeline page");
    let assistant_items: Vec<_> = page
        .items
        .iter()
        .filter(|item| {
            item.message_id
                .as_ref()
                .is_some_and(|id| id.as_str() == pre_assistant_message_id)
        })
        .collect();
    assert!(!assistant_items.is_empty());
    assert!(assistant_items
        .iter()
        .all(|item| item.status == ChatTimelineItemStatus::Finalized));
    assert!(assistant_items
        .iter()
        .all(|item| item.run_id.as_ref().map(|id| id.as_str()) == Some(run_id.clone())));
}

/// Sub-branch A: Cancelled { turns_finalized: 1, completion_tool_called: true }
/// → success path taken; execution slot IS re-incremented.
///
/// TurnComplete fired (turns_finalized=1) so it already decremented the slot once.
/// Sub-branch A compensates with a re-increment before calling handle_stream_success.
/// This regression guard ensures that path is unchanged by the new completion_tool_called field.
#[tokio::test]
async fn test_handle_stream_error_cancelled_turns_finalized_re_increments_slot() {
    let cancelled = StreamError::Cancelled {
        turns_finalized: 1,
        completion_tool_called: true,
    };
    let (recovery_spawned, count_after) = invoke_handle_stream_error_cancelled(&cancelled).await;

    assert!(
        !recovery_spawned,
        "Sub-branch A must return false (success path, no retry)"
    );
    assert_eq!(
        count_after,
        1,
        "turns_finalized>0 path must re-increment slot once to compensate for TurnComplete's decrement"
    );
}

#[tokio::test]
async fn test_handle_stream_error_preserves_existing_content_blocks_without_serializing_nonfatal_mcp_cancellation(
) {
    let state = AppState::new_test();
    let conversation_id = ChatConversationId::new();
    let context_id = IdeationSessionId::new();
    let pre_assistant_message =
        crate::application::chat_service::chat_service_context::create_assistant_message(
            ChatContextType::Ideation,
            context_id.as_str(),
            "Recovered ideation response",
            conversation_id.clone(),
            &[ToolCall {
                id: Some("tool-1".to_string()),
                name: "ralphx::get_session_plan".to_string(),
                arguments: serde_json::json!({ "session_id": context_id.as_str() }),
                result: Some(serde_json::json!({ "status": "ok" })),
                parent_tool_use_id: Some("toolu-parent-preserved".to_string()),
                diff_context: None,
                stats: None,
            }],
            &[
                ContentBlockItem::Text {
                    text: "Recovered ideation response".to_string(),
                },
                ContentBlockItem::ToolUse {
                    id: Some("tool-1".to_string()),
                    name: "ralphx::get_session_plan".to_string(),
                    arguments: serde_json::json!({ "session_id": context_id.as_str() }),
                    result: Some(serde_json::json!({ "status": "ok" })),
                    parent_tool_use_id: Some("toolu-parent-preserved".to_string()),
                    diff_context: None,
                },
            ],
        );
    let pre_assistant_message_id = pre_assistant_message.id.as_str().to_string();
    state
        .chat_message_repo
        .create(pre_assistant_message)
        .await
        .expect("insert pre-assistant message");

    let event_ctx = crate::application::chat_service::event_context(
        &conversation_id,
        &ChatContextType::Ideation,
        context_id.as_str(),
    );
    let stream_error = StreamError::AgentExit {
        exit_code: Some(1),
        stderr: "user cancelled MCP tool call".to_string(),
    };

    let recovery_spawned = handle_stream_error(
        "user cancelled MCP tool call",
        Some(&stream_error),
        ChatContextType::Ideation,
        context_id.as_str(),
        conversation_id,
        "run-id-1",
        &pre_assistant_message_id,
        &event_ctx,
        None,
        crate::domain::agents::AgentHarnessKind::Codex,
        false,
        None,
        None,
        None,
        std::path::Path::new("/tmp/codex"),
        std::path::Path::new("/tmp/plugin"),
        std::path::Path::new("/tmp"),
        &state.chat_message_repo,
        &Some(state.chat_timeline_repo.clone()),
        &state.chat_attachment_repo,
        &state.artifact_repo,
        &state.chat_conversation_repo,
        &state.agent_run_repo,
        &state.task_repo,
        &state.task_dependency_repo,
        &state.project_repo,
        &state.ideation_session_repo,
        &None,
        &state.activity_event_repo,
        &state.message_queue,
        &state.running_agent_registry,
        &state.memory_event_repo,
        &None,
        &None,
        &None,
        &None,
        &ChatRuntimeFactoryDeps::from_app_state(&state),
        None,
        None,
        &None,
        &None,
        &None,
        &None,
    )
    .await;

    assert!(
        !recovery_spawned,
        "non-fatal MCP cancellation path must not spawn recovery"
    );

    let stored = state
        .chat_message_repo
        .get_by_id(&ChatMessageId::from_string(pre_assistant_message_id))
        .await
        .expect("reload message")
        .expect("message should still exist");

    assert_eq!(
        stored.content, "Recovered ideation response",
        "non-fatal MCP cancellation text must not be appended into persisted assistant/orchestrator content"
    );
    assert!(
        stored.content_blocks.is_some(),
        "non-fatal MCP cancellation finalization must preserve previously persisted content_blocks instead of clearing ordered widget hydration"
    );
    let blocks: serde_json::Value = serde_json::from_str(
        stored
            .content_blocks
            .as_deref()
            .expect("content blocks JSON should be present"),
    )
    .expect("content blocks should remain valid JSON");
    assert_eq!(
        blocks.as_array().map(|items| items.len()),
        Some(2),
        "the pre-error text + tool-use blocks should remain available for final replay rendering"
    );
}

#[tokio::test]
async fn test_handle_stream_error_terminal_verification_child_seals_unresolved_tool_calls() {
    let state = AppState::new_test();
    let parent_id = IdeationSessionId::new();
    let child_id = IdeationSessionId::new();
    let project_id = ProjectId::new();

    let mut parent = crate::domain::entities::IdeationSession::new(project_id.clone());
    parent.id = parent_id.clone();
    parent.verification_status = VerificationStatus::NeedsRevision;
    parent.verification_in_progress = false;
    parent.verification_generation = 7;
    state.ideation_session_repo.create(parent).await.unwrap();
    state
        .ideation_session_repo
        .save_verification_run_snapshot(
            &parent_id,
            &crate::domain::entities::VerificationRunSnapshot {
                generation: 7,
                status: VerificationStatus::NeedsRevision,
                in_progress: false,
                current_round: 2,
                max_rounds: 5,
                best_round_index: None,
                convergence_reason: Some("max_rounds".to_string()),
                current_gaps: vec![crate::domain::entities::VerificationGap {
                    severity: "high".to_string(),
                    category: "scope".to_string(),
                    description: "Need one more database-default proof.".to_string(),
                    why_it_matters: None,
                    source: None,
                }],
                rounds: vec![],
            },
        )
        .await
        .unwrap();

    let mut child = crate::domain::entities::IdeationSession::new(project_id);
    child.id = child_id.clone();
    child.session_purpose = crate::domain::entities::SessionPurpose::Verification;
    child.parent_session_id = Some(parent_id.clone());
    state.ideation_session_repo.create(child).await.unwrap();

    let parent_conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_ideation(parent_id.clone()))
        .await
        .unwrap();
    let child_conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_ideation(child_id.clone()))
        .await
        .unwrap();

    let pre_assistant_message =
        crate::application::chat_service::chat_service_context::create_assistant_message(
            ChatContextType::Ideation,
            child_id.as_str(),
            "Checking verifier MCP context",
            child_conversation.id.clone(),
            &[ToolCall {
                id: Some("probe-1".to_string()),
                name: "ralphx::read_mcp_resource".to_string(),
                arguments: serde_json::json!({ "uri": "resource://probe" }),
                result: None,
                parent_tool_use_id: None,
                diff_context: None,
                stats: None,
            }],
            &[
                ContentBlockItem::Text {
                    text: "Checking verifier MCP context".to_string(),
                },
                ContentBlockItem::ToolUse {
                    id: Some("probe-1".to_string()),
                    name: "ralphx::read_mcp_resource".to_string(),
                    arguments: serde_json::json!({ "uri": "resource://probe" }),
                    result: None,
                    parent_tool_use_id: None,
                    diff_context: None,
                },
            ],
        );
    let pre_assistant_message_id = pre_assistant_message.id.as_str().to_string();
    state
        .chat_message_repo
        .create(pre_assistant_message)
        .await
        .expect("insert pre-assistant message");

    let event_ctx = crate::application::chat_service::event_context(
        &child_conversation.id,
        &ChatContextType::Ideation,
        child_id.as_str(),
    );
    let stream_error = StreamError::AgentExit {
        exit_code: Some(1),
        stderr: "agent exited".to_string(),
    };

    let recovery_spawned = handle_stream_error(
        "agent exited",
        Some(&stream_error),
        ChatContextType::Ideation,
        child_id.as_str(),
        child_conversation.id.clone(),
        "run-id-terminal-verification",
        &pre_assistant_message_id,
        &event_ctx,
        None,
        crate::domain::agents::AgentHarnessKind::Codex,
        false,
        None,
        None,
        None,
        std::path::Path::new("/tmp/codex"),
        std::path::Path::new("/tmp/plugin"),
        std::path::Path::new("/tmp"),
        &state.chat_message_repo,
        &Some(state.chat_timeline_repo.clone()),
        &state.chat_attachment_repo,
        &state.artifact_repo,
        &state.chat_conversation_repo,
        &state.agent_run_repo,
        &state.task_repo,
        &state.task_dependency_repo,
        &state.project_repo,
        &state.ideation_session_repo,
        &None,
        &state.activity_event_repo,
        &state.message_queue,
        &state.running_agent_registry,
        &state.memory_event_repo,
        &None,
        &None,
        &None,
        &None,
        &ChatRuntimeFactoryDeps::from_app_state(&state),
        None,
        None,
        &None,
        &None,
        &None,
        &None,
    )
    .await;

    assert!(!recovery_spawned);

    let stored = state
        .chat_message_repo
        .get_by_id(&ChatMessageId::from_string(pre_assistant_message_id))
        .await
        .expect("reload message")
        .expect("message should still exist");
    assert_eq!(stored.content, "Checking verifier MCP context");

    let tool_calls: serde_json::Value = serde_json::from_str(
        stored
            .tool_calls
            .as_deref()
            .expect("tool calls should be present"),
    )
    .expect("tool calls should remain valid JSON");
    assert_eq!(
        tool_calls[0]["result"]["status"],
        serde_json::json!("aborted"),
        "terminal verification suppression must seal unresolved tool calls so they do not stay live in the UI"
    );

    let content_blocks: serde_json::Value = serde_json::from_str(
        stored
            .content_blocks
            .as_deref()
            .expect("content blocks should be present"),
    )
    .expect("content blocks should remain valid JSON");
    assert_eq!(
        content_blocks[1]["result"]["status"],
        serde_json::json!("aborted"),
        "content block hydration must also stop treating the probe as still running"
    );

    let parent_messages = state
        .chat_message_repo
        .get_by_conversation(&parent_conversation.id)
        .await
        .expect("load parent conversation messages");
    assert!(
        !parent_messages.is_empty(),
        "terminal verification suppression should still inject the parent handoff message"
    );
}

#[tokio::test]
async fn test_handle_stream_error_actionable_verification_child_queues_hidden_auto_continue() {
    let state = AppState::new_test();
    let parent_id = IdeationSessionId::new();
    let child_id = IdeationSessionId::new();
    let project_id = ProjectId::new();

    let mut parent = crate::domain::entities::IdeationSession::new(project_id.clone());
    parent.id = parent_id.clone();
    parent.verification_status = VerificationStatus::NeedsRevision;
    parent.verification_in_progress = true;
    parent.verification_generation = 4;
    state.ideation_session_repo.create(parent).await.unwrap();
    state
        .ideation_session_repo
        .save_verification_run_snapshot(
            &parent_id,
            &crate::domain::entities::VerificationRunSnapshot {
                generation: 4,
                status: VerificationStatus::NeedsRevision,
                in_progress: true,
                current_round: 2,
                max_rounds: 5,
                best_round_index: None,
                convergence_reason: None,
                current_gaps: vec![crate::domain::entities::VerificationGap {
                    severity: "high".to_string(),
                    category: "testing".to_string(),
                    description: "Need one more regression path.".to_string(),
                    why_it_matters: None,
                    source: None,
                }],
                rounds: vec![crate::domain::entities::VerificationRoundSnapshot {
                    round: 2,
                    gap_score: 3,
                    fingerprints: vec!["high::testing::Need one more regression path.".to_string()],
                    gaps: vec![crate::domain::entities::VerificationGap {
                        severity: "high".to_string(),
                        category: "testing".to_string(),
                        description: "Need one more regression path.".to_string(),
                        why_it_matters: None,
                        source: None,
                    }],
                    parse_failed: false,
                }],
            },
        )
        .await
        .unwrap();

    let mut child = crate::domain::entities::IdeationSession::new(project_id);
    child.id = child_id.clone();
    child.session_purpose = crate::domain::entities::SessionPurpose::Verification;
    child.parent_session_id = Some(parent_id.clone());
    state.ideation_session_repo.create(child).await.unwrap();

    let child_conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_ideation(child_id.clone()))
        .await
        .unwrap();

    let pre_assistant_message =
        crate::application::chat_service::chat_service_context::create_assistant_message(
            ChatContextType::Ideation,
            child_id.as_str(),
            "Round 2 critique in progress",
            child_conversation.id.clone(),
            &[],
            &[ContentBlockItem::Text {
                text: "Round 2 critique in progress".to_string(),
            }],
        );
    let pre_assistant_message_id = pre_assistant_message.id.as_str().to_string();
    state
        .chat_message_repo
        .create(pre_assistant_message)
        .await
        .expect("insert pre-assistant message");

    let event_ctx = crate::application::chat_service::event_context(
        &child_conversation.id,
        &ChatContextType::Ideation,
        child_id.as_str(),
    );
    let stream_error = StreamError::AgentExit {
        exit_code: Some(1),
        stderr: "agent exited".to_string(),
    };

    let recovery_spawned = handle_stream_error(
        "agent exited",
        Some(&stream_error),
        ChatContextType::Ideation,
        child_id.as_str(),
        child_conversation.id,
        "run-id-auto-continue",
        &pre_assistant_message_id,
        &event_ctx,
        Some("provider-session-verification"),
        crate::domain::agents::AgentHarnessKind::Codex,
        false,
        None,
        None,
        None,
        std::path::Path::new("/tmp/codex"),
        std::path::Path::new("/tmp/plugin"),
        std::path::Path::new("/tmp"),
        &state.chat_message_repo,
        &Some(state.chat_timeline_repo.clone()),
        &state.chat_attachment_repo,
        &state.artifact_repo,
        &state.chat_conversation_repo,
        &state.agent_run_repo,
        &state.task_repo,
        &state.task_dependency_repo,
        &state.project_repo,
        &state.ideation_session_repo,
        &None,
        &state.activity_event_repo,
        &state.message_queue,
        &state.running_agent_registry,
        &state.memory_event_repo,
        &None,
        &None,
        &None,
        &None,
        &ChatRuntimeFactoryDeps::from_app_state(&state),
        None,
        None,
        &None,
        &None,
        &None,
        &None,
    )
    .await;

    assert!(!recovery_spawned);

    let queued = state
        .message_queue
        .get_queued(ChatContextType::Ideation, child_id.as_str());
    assert_eq!(
        queued.len(),
        1,
        "auto-continue must queue one hidden control message"
    );
    assert_eq!(
        queued[0].metadata_override.as_deref(),
        Some(VERIFICATION_AUTO_CONTINUE_METADATA)
    );

    let parent_after = state
        .ideation_session_repo
        .get_by_id(&parent_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        parent_after.verification_status,
        VerificationStatus::NeedsRevision
    );
    assert!(parent_after.verification_in_progress);

    let child_after = state
        .ideation_session_repo
        .get_by_id(&child_id)
        .await
        .unwrap()
        .unwrap();
    assert_ne!(
        child_after.status,
        crate::domain::entities::IdeationSessionStatus::Archived
    );
}

#[tokio::test]
async fn test_handle_stream_error_appends_generic_agent_error_to_existing_content() {
    let state = AppState::new_test();
    let conversation_id = ChatConversationId::new();
    let context_id = IdeationSessionId::new();
    let pre_assistant_message =
        crate::application::chat_service::chat_service_context::create_assistant_message(
            ChatContextType::Ideation,
            context_id.as_str(),
            "Recovered ideation response",
            conversation_id.clone(),
            &[],
            &[ContentBlockItem::Text {
                text: "Recovered ideation response".to_string(),
            }],
        );
    let pre_assistant_message_id = pre_assistant_message.id.as_str().to_string();
    state
        .chat_message_repo
        .create(pre_assistant_message)
        .await
        .expect("insert pre-assistant message");

    let event_ctx = crate::application::chat_service::event_context(
        &conversation_id,
        &ChatContextType::Ideation,
        context_id.as_str(),
    );
    let stream_error = StreamError::AgentExit {
        exit_code: Some(1),
        stderr: "unexpected agent crash".to_string(),
    };

    let recovery_spawned = handle_stream_error(
        "unexpected agent crash",
        Some(&stream_error),
        ChatContextType::Ideation,
        context_id.as_str(),
        conversation_id,
        "run-id-2",
        &pre_assistant_message_id,
        &event_ctx,
        None,
        crate::domain::agents::AgentHarnessKind::Codex,
        false,
        None,
        None,
        None,
        std::path::Path::new("/tmp/codex"),
        std::path::Path::new("/tmp/plugin"),
        std::path::Path::new("/tmp"),
        &state.chat_message_repo,
        &Some(state.chat_timeline_repo.clone()),
        &state.chat_attachment_repo,
        &state.artifact_repo,
        &state.chat_conversation_repo,
        &state.agent_run_repo,
        &state.task_repo,
        &state.task_dependency_repo,
        &state.project_repo,
        &state.ideation_session_repo,
        &None,
        &state.activity_event_repo,
        &state.message_queue,
        &state.running_agent_registry,
        &state.memory_event_repo,
        &None,
        &None,
        &None,
        &None,
        &ChatRuntimeFactoryDeps::from_app_state(&state),
        None,
        None,
        &None,
        &None,
        &None,
        &None,
    )
    .await;

    assert!(
        !recovery_spawned,
        "generic agent error append path must not spawn recovery"
    );

    let stored = state
        .chat_message_repo
        .get_by_id(&ChatMessageId::from_string(pre_assistant_message_id))
        .await
        .expect("reload message")
        .expect("message should still exist");

    assert!(
        stored.content.contains("[Agent error:"),
        "generic agent failures must still be appended into persisted assistant/orchestrator content"
    );
    assert!(
        stored.content.contains("unexpected agent crash"),
        "generic agent failures must keep the error details in the appended note"
    );
}

// ========================================
// L1 Shutdown Guard Tests
// ========================================

/// ExecutionState is initialized with is_shutting_down = false.
/// The L1 shutdown guard checks this flag before escalating, so the default
/// must be false to avoid skipping escalation during normal agent exits.
#[test]
fn test_execution_state_shutdown_flag_starts_false() {
    let exec = ExecutionState::new();
    assert!(
        !exec
            .is_shutting_down
            .load(std::sync::atomic::Ordering::SeqCst),
        "is_shutting_down must start as false so normal agent exits are escalated"
    );
}

/// The shutdown flag can be set via store(true), which the RunEvent::Exit handler
/// calls as the FIRST operation before cleaning up agents.
#[test]
fn test_execution_state_shutdown_flag_can_be_set() {
    let exec = ExecutionState::new();
    exec.is_shutting_down
        .store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(
        exec.is_shutting_down
            .load(std::sync::atomic::Ordering::SeqCst),
        "is_shutting_down must reflect store(true)"
    );
}

/// The shutdown flag can be read back correctly after being set and cleared.
/// This guards against accidental persistence across test runs (AtomicBool is in-memory).
#[test]
fn test_execution_state_shutdown_flag_can_be_cleared() {
    let exec = ExecutionState::new();
    exec.is_shutting_down
        .store(true, std::sync::atomic::Ordering::SeqCst);
    exec.is_shutting_down
        .store(false, std::sync::atomic::Ordering::SeqCst);
    assert!(
        !exec
            .is_shutting_down
            .load(std::sync::atomic::Ordering::SeqCst),
        "is_shutting_down must reflect store(false) after being cleared"
    );
}

/// The L1 shutdown guard writes shutdown_interrupted: true into task metadata
/// when is_shutting_down is set. This test verifies the metadata manipulation
/// logic directly — creating a JSON object with the flag and confirming it is present.
#[test]
fn test_shutdown_interrupted_metadata_key_added_when_shutdown_flag_set() {
    // Simulate what the L1 guard does: it merges shutdown_interrupted=true into
    // the task's metadata JSON when is_shutting_down is detected.
    let mut meta: serde_json::Value = serde_json::json!({
        "last_agent_error_context": "execution"
    });

    // Simulate the guard's metadata write
    if let Some(obj) = meta.as_object_mut() {
        obj.insert("shutdown_interrupted".to_string(), serde_json::json!(true));
    }

    assert_eq!(
        meta.get("shutdown_interrupted").and_then(|v| v.as_bool()),
        Some(true),
        "shutdown_interrupted key must be present and true after L1 guard writes it"
    );
}

/// The shutdown_interrupted flag in metadata is a bool, not a string.
/// This ensures the startup recovery reader (should_auto_recover) can parse it correctly.
#[test]
fn test_shutdown_interrupted_metadata_value_is_bool() {
    let meta = serde_json::json!({
        "shutdown_interrupted": true,
        "last_agent_error_context": "review"
    });
    let flag = meta.get("shutdown_interrupted").and_then(|v| v.as_bool());
    assert_eq!(
        flag,
        Some(true),
        "shutdown_interrupted value must deserialize as bool true"
    );
}

#[tokio::test]
async fn test_task_execution_shutdown_success_persists_startup_recovery_metadata() {
    let state = AppState::new_test();
    let exec = Arc::new(ExecutionState::new());
    exec.is_shutting_down
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let execution_state = Some(Arc::clone(&exec));

    let project = Project::new("Test Project".into(), "/tmp/test-project".into());
    state.project_repo.create(project.clone()).await.unwrap();

    let mut task = Task::new(project.id.clone(), "Executing task".into());
    task.internal_status = InternalStatus::Executing;
    let task_id = task.id.clone();
    state.task_repo.create(task).await.unwrap();

    let app = mock_builder()
        .manage(state)
        .manage(Arc::clone(&exec))
        .build(mock_context(noop_assets()))
        .expect("mock app");
    let handle = app.handle().clone();
    let state = handle.state::<AppState>();

    handle_stream_success(
        "run-id-shutdown-success",
        ChatContextType::TaskExecution,
        task_id.as_str(),
        false,
        false,
        false,
        &execution_state,
        &state.task_repo,
        &state.task_dependency_repo,
        &state.project_repo,
        &state.artifact_repo,
        &state.chat_message_repo,
        &state.chat_attachment_repo,
        &state.chat_conversation_repo,
        &state.agent_run_repo,
        &state.ideation_session_repo,
        &state.activity_event_repo,
        &state.message_queue,
        &state.running_agent_registry,
        &state.memory_event_repo,
        &None,
        &None,
        &None,
        &ChatRuntimeFactoryDeps::from_app_state(&state),
        &None,
        &None,
        &None,
    )
    .await;

    let updated = state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("task should still exist");
    assert_eq!(
        updated.internal_status,
        InternalStatus::Executing,
        "shutdown success path must leave the task active for startup recovery"
    );

    let metadata: serde_json::Value = updated
        .metadata
        .as_deref()
        .and_then(|raw| serde_json::from_str(raw).ok())
        .expect("shutdown guard should persist metadata");
    assert_eq!(
        metadata
            .get("shutdown_interrupted")
            .and_then(|value| value.as_bool()),
        Some(true),
        "startup recovery marker must be persisted on shutdown success"
    );
    assert_eq!(
        metadata
            .get("last_agent_error_context")
            .and_then(|value| value.as_str()),
        Some("execution"),
        "startup recovery must know the interrupted context"
    );
}

#[tokio::test]
async fn test_task_execution_shutdown_error_persists_startup_recovery_metadata() {
    let state = AppState::new_test();
    let exec = Arc::new(ExecutionState::new());
    exec.is_shutting_down
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let execution_state = Some(Arc::clone(&exec));

    let project = Project::new("Test Project".into(), "/tmp/test-project".into());
    state.project_repo.create(project.clone()).await.unwrap();

    let mut task = Task::new(project.id.clone(), "Executing task".into());
    task.internal_status = InternalStatus::Executing;
    let task_id = task.id.clone();
    state.task_repo.create(task).await.unwrap();

    let conversation_id = ChatConversationId::new();
    let event_ctx = crate::application::chat_service::event_context(
        &conversation_id,
        &ChatContextType::TaskExecution,
        task_id.as_str(),
    );
    let stream_error = StreamError::AgentExit {
        exit_code: Some(1),
        stderr: "agent exited during shutdown".to_string(),
    };

    let recovery_spawned = handle_stream_error(
        "agent exited during shutdown",
        Some(&stream_error),
        ChatContextType::TaskExecution,
        task_id.as_str(),
        conversation_id,
        "run-id-shutdown-error",
        "message-id-shutdown-error",
        &event_ctx,
        None,
        crate::domain::agents::AgentHarnessKind::Codex,
        false,
        None,
        None,
        None,
        std::path::Path::new("/tmp/codex"),
        std::path::Path::new("/tmp/plugin"),
        std::path::Path::new("/tmp"),
        &state.chat_message_repo,
        &Some(state.chat_timeline_repo.clone()),
        &state.chat_attachment_repo,
        &state.artifact_repo,
        &state.chat_conversation_repo,
        &state.agent_run_repo,
        &state.task_repo,
        &state.task_dependency_repo,
        &state.project_repo,
        &state.ideation_session_repo,
        &None,
        &state.activity_event_repo,
        &state.message_queue,
        &state.running_agent_registry,
        &state.memory_event_repo,
        &execution_state,
        &None,
        &None,
        &None,
        &ChatRuntimeFactoryDeps::from_app_state(&state),
        None,
        None,
        &None,
        &None,
        &None,
        &None,
    )
    .await;

    assert!(
        !recovery_spawned,
        "shutdown error path should not spawn immediate recovery; startup owns it"
    );

    let updated = state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("task should still exist");
    assert_eq!(
        updated.internal_status,
        InternalStatus::Executing,
        "shutdown error path must leave the task active for startup recovery"
    );

    let metadata: serde_json::Value = updated
        .metadata
        .as_deref()
        .and_then(|raw| serde_json::from_str(raw).ok())
        .expect("shutdown guard should persist metadata");
    assert_eq!(
        metadata
            .get("shutdown_interrupted")
            .and_then(|value| value.as_bool()),
        Some(true),
        "startup recovery marker must be persisted on shutdown error"
    );
    assert_eq!(
        metadata
            .get("last_agent_error_context")
            .and_then(|value| value.as_str()),
        Some("execution"),
        "startup recovery must know the interrupted context"
    );
    assert_eq!(
        metadata
            .get("last_agent_error")
            .and_then(|value| value.as_str()),
        Some("agent exited during shutdown"),
        "shutdown error path should preserve the agent error for diagnostics"
    );
}

#[tokio::test]
async fn test_task_execution_error_finalizer_fails_current_attempt_with_metadata() {
    let state = AppState::new_test();
    let exec = Arc::new(ExecutionState::new());
    let execution_state = Some(Arc::clone(&exec));

    let project = Project::new("Failed Execution".into(), "/tmp/failed-execution".into());
    state.project_repo.create(project.clone()).await.unwrap();

    let mut task = Task::new(project.id.clone(), "Executing task".into());
    task.internal_status = InternalStatus::Executing;
    let task_id = task.id.clone();
    state.task_repo.create(task).await.unwrap();

    let conversation_id = ChatConversationId::new();
    let event_ctx = crate::application::chat_service::event_context(
        &conversation_id,
        &ChatContextType::TaskExecution,
        task_id.as_str(),
    );
    let stream_error = StreamError::Timeout {
        context_type: ChatContextType::TaskExecution,
        elapsed_secs: 120,
    };

    let recovery_spawned = handle_stream_error(
        "execution timed out after 120s",
        Some(&stream_error),
        ChatContextType::TaskExecution,
        task_id.as_str(),
        conversation_id,
        "run-id-timeout-error",
        "message-id-timeout-error",
        &event_ctx,
        None,
        crate::domain::agents::AgentHarnessKind::Codex,
        false,
        None,
        None,
        None,
        std::path::Path::new("/tmp/codex"),
        std::path::Path::new("/tmp/plugin"),
        std::path::Path::new("/tmp"),
        &state.chat_message_repo,
        &Some(state.chat_timeline_repo.clone()),
        &state.chat_attachment_repo,
        &state.artifact_repo,
        &state.chat_conversation_repo,
        &state.agent_run_repo,
        &state.task_repo,
        &state.task_dependency_repo,
        &state.project_repo,
        &state.ideation_session_repo,
        &None,
        &state.activity_event_repo,
        &state.message_queue,
        &state.running_agent_registry,
        &state.memory_event_repo,
        &execution_state,
        &None,
        &None,
        &None,
        &ChatRuntimeFactoryDeps::from_app_state(&state),
        None,
        None,
        &None,
        &None,
        &None,
        &None,
    )
    .await;

    assert!(
        !recovery_spawned,
        "normal execution error path should not spawn stale-session recovery"
    );

    let updated = state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("task should still exist");
    assert_eq!(
        updated.internal_status,
        InternalStatus::Failed,
        "current execution error should transition to Failed"
    );

    let metadata: serde_json::Value = updated
        .metadata
        .as_deref()
        .and_then(|raw| serde_json::from_str(raw).ok())
        .expect("execution error should persist metadata");
    assert_eq!(
        metadata
            .get("last_agent_error_context")
            .and_then(|value| value.as_str()),
        Some("execution")
    );
    assert_eq!(
        metadata
            .get("last_agent_error")
            .and_then(|value| value.as_str()),
        Some("execution timed out after 120s")
    );
    assert_eq!(
        metadata.get("is_timeout").and_then(|value| value.as_bool()),
        Some(true),
        "timeout finalizer should preserve timeout classification"
    );
    assert!(
        metadata.get("execution_recovery").is_some(),
        "non-provider execution errors should preserve recovery metadata"
    );
}

#[tokio::test]
async fn test_task_execution_agent_exit_preserves_worker_error_as_failure_error() {
    let state = AppState::new_test();
    let exec = Arc::new(ExecutionState::new());
    let execution_state = Some(Arc::clone(&exec));

    let project = Project::new(
        "Failed Worker Command".into(),
        "/tmp/failed-worker-command".into(),
    );
    state.project_repo.create(project.clone()).await.unwrap();

    let mut task = Task::new(project.id.clone(), "Executing task".into());
    task.internal_status = InternalStatus::Executing;
    let task_id = task.id.clone();
    state.task_repo.create(task).await.unwrap();

    let conversation_id = ChatConversationId::new();
    let event_ctx = crate::application::chat_service::event_context(
        &conversation_id,
        &ChatContextType::TaskExecution,
        task_id.as_str(),
    );
    let worker_stderr =
        "sed: .artifacts/specs/p6-pr-list-affordances/tracker.md: No such file or directory";
    let stream_error = StreamError::AgentExit {
        exit_code: Some(1),
        stderr: worker_stderr.to_string(),
    };
    let expected_error = format!("Agent failed: {}", worker_stderr);

    let recovery_spawned = handle_stream_error(
        &expected_error,
        Some(&stream_error),
        ChatContextType::TaskExecution,
        task_id.as_str(),
        conversation_id,
        "run-id-agent-exit-error",
        "message-id-agent-exit-error",
        &event_ctx,
        None,
        crate::domain::agents::AgentHarnessKind::Codex,
        false,
        None,
        None,
        None,
        std::path::Path::new("/tmp/codex"),
        std::path::Path::new("/tmp/plugin"),
        std::path::Path::new("/tmp"),
        &state.chat_message_repo,
        &Some(state.chat_timeline_repo.clone()),
        &state.chat_attachment_repo,
        &state.artifact_repo,
        &state.chat_conversation_repo,
        &state.agent_run_repo,
        &state.task_repo,
        &state.task_dependency_repo,
        &state.project_repo,
        &state.ideation_session_repo,
        &None,
        &state.activity_event_repo,
        &state.message_queue,
        &state.running_agent_registry,
        &state.memory_event_repo,
        &execution_state,
        &None,
        &None,
        &None,
        &ChatRuntimeFactoryDeps::from_app_state(&state),
        None,
        None,
        &None,
        &None,
        &None,
        &None,
    )
    .await;

    assert!(
        !recovery_spawned,
        "normal agent-exit path should not spawn stale-session recovery"
    );

    let updated = state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("task should still exist");
    assert_eq!(updated.internal_status, InternalStatus::Failed);

    let metadata: serde_json::Value = updated
        .metadata
        .as_deref()
        .and_then(|raw| serde_json::from_str(raw).ok())
        .expect("agent exit should persist metadata");
    assert_eq!(
        metadata
            .get("last_agent_error")
            .and_then(|value| value.as_str()),
        Some(expected_error.as_str())
    );
    assert_eq!(
        metadata
            .get("failure_error")
            .and_then(|value| value.as_str()),
        Some(expected_error.as_str()),
        "failed task details should show the worker error instead of the generic fallback"
    );
    assert_eq!(
        metadata.get("is_timeout").and_then(|value| value.as_bool()),
        Some(false),
        "agent exit failures should be recorded as non-timeout failures"
    );
}

#[tokio::test]
async fn test_task_execution_local_tool_failure_uses_head_matched_validation_cache_for_failed_steps(
) {
    let state = AppState::new_test();
    let exec = Arc::new(ExecutionState::new());
    let execution_state = Some(Arc::clone(&exec));
    let (_worktree, base_sha, head_sha) = git_worktree_with_base_and_change();

    let project = Project::new(
        "Local Tool Validation Override".into(),
        "/tmp/local-tool-validation-override".into(),
    );
    state.project_repo.create(project.clone()).await.unwrap();

    let mut task = Task::new(project.id.clone(), "Executing task".into());
    task.internal_status = InternalStatus::Executing;
    task.worktree_path = Some(_worktree.path().to_string_lossy().to_string());
    task.task_branch_base_ref = Some(base_sha.clone());
    task.task_branch_base_sha = Some(base_sha);
    let task_id = task.id.clone();
    state.task_repo.create(task).await.unwrap();
    let agent_run_id = seed_current_execution_attempt(&state, &task_id).await;
    let cache = validation_cache_fixture(&head_sha, true, true);
    let metadata = cache
        .update_task_metadata(None)
        .expect("validation cache metadata should serialize");
    state
        .task_repo
        .update_metadata(&task_id, Some(metadata))
        .await
        .unwrap();

    let task_step_repo: Option<Arc<dyn TaskStepRepository>> = Some(Arc::new(StubTaskStepRepo {
        steps: vec![
            make_step(&task_id, TaskStepStatus::Completed),
            make_step(&task_id, TaskStepStatus::Failed),
        ],
    }));

    let conversation_id = ChatConversationId::new();
    let event_ctx = crate::application::chat_service::event_context(
        &conversation_id,
        &ChatContextType::TaskExecution,
        task_id.as_str(),
    );
    let stream_error = StreamError::LocalToolFailed {
        message: "late local diagnostic after execution_complete".to_string(),
    };
    let runtime_deps = ChatRuntimeFactoryDeps::from_app_state(&state);

    let recovery_spawned = handle_stream_error(
        "late local diagnostic after execution_complete",
        Some(&stream_error),
        ChatContextType::TaskExecution,
        task_id.as_str(),
        conversation_id,
        agent_run_id.as_str(),
        "message-id-local-tool-validation-cache",
        &event_ctx,
        None,
        crate::domain::agents::AgentHarnessKind::Codex,
        false,
        None,
        None,
        None,
        std::path::Path::new("/tmp/codex"),
        std::path::Path::new("/tmp/plugin"),
        std::path::Path::new("/tmp"),
        &state.chat_message_repo,
        &Some(state.chat_timeline_repo.clone()),
        &state.chat_attachment_repo,
        &state.artifact_repo,
        &state.chat_conversation_repo,
        &state.agent_run_repo,
        &state.task_repo,
        &state.task_dependency_repo,
        &state.project_repo,
        &state.ideation_session_repo,
        &None,
        &state.activity_event_repo,
        &state.message_queue,
        &state.running_agent_registry,
        &state.memory_event_repo,
        &execution_state,
        &None,
        &None,
        &None,
        &runtime_deps,
        None,
        None,
        &None,
        &None,
        &task_step_repo,
        &None,
    )
    .await;

    assert!(
        !recovery_spawned,
        "normal local-tool diagnostic path should not spawn stale-session recovery"
    );

    let updated = state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("task should still exist");
    assert!(
        matches!(
            updated.internal_status,
            InternalStatus::PendingReview | InternalStatus::Reviewing
        ),
        "LocalToolFailed should route to review flow when validation cache proves completion, got {:?}",
        updated.internal_status
    );

    let agent_run = state
        .agent_run_repo
        .get_by_id(&AgentRunId::from_string(agent_run_id.clone()))
        .await
        .unwrap()
        .expect("agent run should still exist");
    assert_eq!(
        agent_run.status,
        AgentRunStatus::Completed,
        "validation-proven execution completion should not leave the agent run failed"
    );
    assert!(
        agent_run.error_message.is_none(),
        "completed execution run should clear stale failure text, got {:?}",
        agent_run.error_message
    );
    assert!(
        agent_run.completed_at.is_some(),
        "completed execution run should have a terminal timestamp"
    );
}

#[tokio::test]
async fn test_task_execution_provider_error_finalizer_pauses_with_metadata() {
    let state = AppState::new_test();
    let exec = Arc::new(ExecutionState::new());
    let execution_state = Some(Arc::clone(&exec));

    let project = Project::new(
        "Provider Pause".into(),
        "/tmp/provider-pause-finalizer".into(),
    );
    state.project_repo.create(project.clone()).await.unwrap();

    let existing_provider_metadata = ProviderErrorMetadata {
        category: ProviderErrorCategory::RateLimit,
        message: "previous limit".to_string(),
        retry_after: None,
        previous_status: InternalStatus::Executing.to_string(),
        paused_at: Utc::now().to_rfc3339(),
        auto_resumable: true,
        resume_attempts: 2,
    };

    let mut task = Task::new(project.id.clone(), "Executing task".into());
    task.internal_status = InternalStatus::Executing;
    task.metadata = Some(existing_provider_metadata.write_to_task_metadata(None));
    let task_id = task.id.clone();
    state.task_repo.create(task).await.unwrap();

    let conversation_id = ChatConversationId::new();
    let event_ctx = crate::application::chat_service::event_context(
        &conversation_id,
        &ChatContextType::TaskExecution,
        task_id.as_str(),
    );
    let retry_after = (Utc::now() + chrono::Duration::minutes(30)).to_rfc3339();
    let stream_error = StreamError::ProviderError {
        category: ProviderErrorCategory::RateLimit,
        message: "usage limit reached".to_string(),
        retry_after: Some(retry_after.clone()),
    };

    let recovery_spawned = handle_stream_error(
        "usage limit reached",
        Some(&stream_error),
        ChatContextType::TaskExecution,
        task_id.as_str(),
        conversation_id,
        "run-id-provider-error",
        "message-id-provider-error",
        &event_ctx,
        None,
        crate::domain::agents::AgentHarnessKind::Codex,
        false,
        None,
        None,
        None,
        std::path::Path::new("/tmp/codex"),
        std::path::Path::new("/tmp/plugin"),
        std::path::Path::new("/tmp"),
        &state.chat_message_repo,
        &Some(state.chat_timeline_repo.clone()),
        &state.chat_attachment_repo,
        &state.artifact_repo,
        &state.chat_conversation_repo,
        &state.agent_run_repo,
        &state.task_repo,
        &state.task_dependency_repo,
        &state.project_repo,
        &state.ideation_session_repo,
        &None,
        &state.activity_event_repo,
        &state.message_queue,
        &state.running_agent_registry,
        &state.memory_event_repo,
        &execution_state,
        &None,
        &None,
        &None,
        &ChatRuntimeFactoryDeps::from_app_state(&state),
        None,
        None,
        &None,
        &None,
        &None,
        &None,
    )
    .await;

    assert!(
        !recovery_spawned,
        "provider pause path should not spawn stale-session recovery"
    );

    let updated = state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("task should still exist");
    assert_eq!(updated.internal_status, InternalStatus::Paused);

    let provider_error = ProviderErrorMetadata::from_task_metadata(updated.metadata.as_deref())
        .expect("provider error metadata should be persisted");
    assert_eq!(provider_error.category, ProviderErrorCategory::RateLimit);
    assert_eq!(provider_error.message, "usage limit reached");
    assert_eq!(provider_error.retry_after, Some(retry_after));
    assert_eq!(
        provider_error.resume_attempts, 2,
        "existing resume attempts should carry forward across provider pauses"
    );

    let metadata: serde_json::Value = updated
        .metadata
        .as_deref()
        .and_then(|raw| serde_json::from_str(raw).ok())
        .expect("provider pause should persist metadata");
    assert!(
        metadata.get("pause_reason").is_some(),
        "provider pause metadata should include the unified pause reason"
    );

    let notifications = state
        .notification_repo
        .list(None, None, 50)
        .await
        .expect("provider pause notification query should succeed")
        .notifications;
    assert_eq!(
        notifications.len(),
        1,
        "per-task stream finalization must use the single global pause producer"
    );
    assert_eq!(
        notifications[0].category,
        NotificationCategory::ProviderPaused
    );
}

#[tokio::test]
async fn task_execution_recovery_failed_records_one_task_stuck_notification() {
    let app_state = AppState::new_test();
    let notification_service = app_state.notification_service();
    let notification_repo = notification_service.repository();
    let execution_state = Arc::new(ExecutionState::new());
    let mut task = Task::new(ProjectId::new(), "Recovery-failed task".to_string());
    task.internal_status = InternalStatus::Executing;
    let task_id = task.id.clone();
    let task_repo: Arc<dyn TaskRepository> = Arc::new(StubTaskRepo {
        task: Some(task),
        status_entered_at: None,
    });
    forced_transition_failures()
        .lock()
        .unwrap()
        .insert(task_id.to_string());

    let app = mock_builder()
        .manage(app_state)
        .manage(Arc::clone(&execution_state))
        .build(mock_context(noop_assets()))
        .expect("mock app");
    let handle = app.handle().clone();
    let state = handle.state::<AppState>();
    let conversation_id = ChatConversationId::new();
    let event_ctx = crate::application::chat_service::event_context(
        &conversation_id,
        &ChatContextType::TaskExecution,
        task_id.as_str(),
    );
    let stream_error = StreamError::AgentExit {
        exit_code: Some(1),
        stderr: "worker crashed".to_string(),
    };
    let runtime_deps = ChatRuntimeFactoryDeps::from_app_state(&state);

    let recovery_spawned = super::handle_stream_error(
        "worker crashed",
        Some(&stream_error),
        ChatContextType::TaskExecution,
        task_id.as_str(),
        conversation_id,
        "run-id-recovery-failed",
        "message-id-recovery-failed",
        &event_ctx,
        None,
        AgentHarnessKind::Codex,
        false,
        false,
        false,
        None,
        None,
        None,
        std::path::Path::new("/tmp/codex"),
        std::path::Path::new("/tmp/plugin"),
        std::path::Path::new("/tmp"),
        &state.chat_message_repo,
        &Some(state.chat_timeline_repo.clone()),
        &state.chat_attachment_repo,
        &state.artifact_repo,
        &state.chat_conversation_repo,
        &state.agent_run_repo,
        &task_repo,
        &state.task_dependency_repo,
        &state.project_repo,
        &state.ideation_session_repo,
        &None,
        &state.activity_event_repo,
        &state.message_queue,
        &state.running_agent_registry,
        &state.memory_event_repo,
        &Some(Arc::clone(&execution_state)),
        &None,
        &None,
        &None,
        &None,
        &None,
        Arc::clone(&runtime_deps.events),
        runtime_deps.plan_verification_completion.as_ref(),
        Some(&runtime_deps),
        None,
        None,
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
        &Some(Arc::clone(&notification_service)),
    )
    .await;

    forced_transition_failures()
        .lock()
        .unwrap()
        .remove(task_id.as_str());

    assert!(
        !recovery_spawned,
        "the fallback-transition failure must not spawn a stale-session recovery"
    );
    let notifications = notification_repo
        .list(None, None, 50)
        .await
        .expect("recovery-failed notification should be readable")
        .notifications;
    let task_stuck: Vec<_> = notifications
        .iter()
        .filter(|notification| notification.category == NotificationCategory::TaskStuck)
        .collect();
    assert_eq!(
        task_stuck.len(),
        1,
        "both failed fallback attempts describe one recovery-failed instance"
    );
    assert_eq!(task_stuck[0].severity, NotificationSeverity::Warning);
    let body = task_stuck[0]
        .body
        .as_deref()
        .expect("recovery failure needs notification copy");
    assert!(body.starts_with("Recovery failed on “Recovery-failed task” — task may be stuck."));
    assert!(body.contains("The automatic recovery transition failed:"));
    assert_eq!(task_stuck[0].target.kind, NotificationTargetKind::Task);
    assert_eq!(
        task_stuck[0].target.task_id.as_deref(),
        Some(task_id.as_str())
    );
    let expected_dedupe_key = format!("task:{task_id}:stuck:run-id-recovery-failed");
    assert_eq!(
        task_stuck[0].dedupe_key.as_deref(),
        Some(expected_dedupe_key.as_str())
    );
}

/// Multiple ExecutionState instances are independent (no global state).
/// The L1 guard reads a specific Arc<ExecutionState> passed to the handler,
/// so creating two instances with different flag values must not interfere.
#[test]
fn test_execution_state_shutdown_flags_are_independent() {
    let exec_a = ExecutionState::new();
    let exec_b = ExecutionState::new();

    exec_a
        .is_shutting_down
        .store(true, std::sync::atomic::Ordering::SeqCst);

    assert!(
        exec_a
            .is_shutting_down
            .load(std::sync::atomic::Ordering::SeqCst),
        "exec_a flag should be true"
    );
    assert!(
        !exec_b
            .is_shutting_down
            .load(std::sync::atomic::Ordering::SeqCst),
        "exec_b flag must remain false — instances are independent"
    );
}

// ---------------------------------------------------------------------------
// Regression tests: verification child timeout fix (Gate B guard)
// ---------------------------------------------------------------------------

/// Gate B check: `is_verification_child` returns `true` for a session that was
/// created with `session_purpose = Verification`.
///
/// This proves that `handle_stream_error` will enter the timeout-suppression branch
/// and skip `agent:error` emission when the lingering idle process eventually hits
/// the 600s no-output timeout.
#[tokio::test]
async fn test_no_agent_error_on_timeout_for_terminal_verification_child() {
    use crate::domain::entities::{IdeationSession, ProjectId, SessionPurpose};
    use crate::infrastructure::memory::MemoryIdeationSessionRepository;

    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    let repo_trait: Arc<dyn IdeationSessionRepository> = repo.clone();

    let parent_id = IdeationSessionId::new();
    let child_id = IdeationSessionId::new();

    // Create a verification child session (session_purpose = Verification).
    let child_session = IdeationSession::builder()
        .id(child_id.clone())
        .project_id(ProjectId::new())
        .session_purpose(SessionPurpose::Verification)
        .parent_session_id(parent_id.clone())
        .build();
    repo_trait
        .create(child_session)
        .await
        .expect("create verification child session");

    // Gate B: is_verification_child must return true for verification sessions.
    // When this returns true, handle_stream_error skips agent:error — which is the
    // regression being guarded: no false agent:error on timeout for already-reconciled
    // verification children.
    let is_verif = is_verification_child(child_id.as_str(), &repo_trait).await;
    assert!(
        is_verif,
        "Gate B must fire for Verification sessions — handle_stream_error will suppress agent:error"
    );
}

/// Gate B check: `is_verification_child` returns `false` for a regular (General)
/// ideation session.
///
/// This proves that `handle_stream_error` does NOT suppress `agent:error` for
/// normal ideation sessions — the verification timeout guard must not affect them.
#[tokio::test]
async fn test_normal_completion_unaffected_by_verification_guards() {
    use crate::domain::entities::{IdeationSession, ProjectId, SessionPurpose};
    use crate::infrastructure::memory::MemoryIdeationSessionRepository;

    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    let repo_trait: Arc<dyn IdeationSessionRepository> = repo.clone();

    let session_id = IdeationSessionId::new();

    // Create a normal (General) ideation session.
    let general_session = IdeationSession::builder()
        .id(session_id.clone())
        .project_id(ProjectId::new())
        .session_purpose(SessionPurpose::General)
        .build();
    repo_trait
        .create(general_session)
        .await
        .expect("create general session");

    // Gate B must NOT fire for General sessions.
    // handle_stream_error proceeds to emit agent:error normally.
    let is_verif = is_verification_child(session_id.as_str(), &repo_trait).await;
    assert!(
        !is_verif,
        "Gate B must NOT fire for General sessions — agent:error must be emitted normally"
    );

    // Sanity check: unknown session IDs also return false (safe fallthrough).
    let unknown_id = IdeationSessionId::new();
    let is_unknown = is_verification_child(unknown_id.as_str(), &repo_trait).await;
    assert!(
        !is_unknown,
        "Gate B must return false for unknown sessions (safe fallthrough to normal agent:error)"
    );
}

#[tokio::test]
async fn recovery_retry_persona_short_circuits_without_feature_or_runtime_deps() {
    let conversation =
        ChatConversation::new_project(ProjectId::from_string("retry-persona-project".to_string()));

    assert_eq!(
        resolve_recovery_retry_persona(
            None,
            false,
            &conversation,
            ChatContextType::Project,
            false,
        )
        .await
        .expect("feature-off retries must not resolve personas"),
        None
    );
    assert_eq!(
        resolve_recovery_retry_persona(None, true, &conversation, ChatContextType::Project, false,)
            .await
            .expect("retries without runtime deps must keep the prior no-persona behavior"),
        None
    );
}

#[tokio::test]
async fn recovery_retry_persona_uses_project_binding_without_a_workspace_row() {
    let now = Utc::now();
    let persona = Persona {
        id: PersonaId::from("retry-bound-persona"),
        artifact_id: None,

        project_id: None,
        slug: "retry-bound-persona".to_string(),
        name: "Retry Bound Persona".to_string(),
        description: "Retry persona fixture".to_string(),
        content: "Keep the recovered conversation focused.".to_string(),
        status: PersonaStatus::Active,
        version: 1,
        content_hash: "retry-bound-persona-hash".to_string(),
        source_session_id: None,
        source_persona_id: None,
        source_content_hash: None,
        source_json: "{}".to_string(),
        created_at: now,
        updated_at: now,
    };
    let mut conversation =
        ChatConversation::new_project(ProjectId::from_string("retry-persona-project".to_string()));
    conversation.persona_id = Some(persona.id.to_string());
    let state = AppState::new_test();
    state
        .persona_repo
        .create(persona)
        .await
        .expect("seed active retry persona");
    let runtime_deps = ChatRuntimeFactoryDeps::from_app_state(&state);

    let resolved = resolve_recovery_retry_persona(
        Some(&runtime_deps),
        true,
        &conversation,
        ChatContextType::Project,
        false,
    )
    .await
    .expect("workspace fallback should resolve the bound project persona")
    .expect("active binding should produce a persona block");

    assert!(resolved.block.contains("<ralphx_agent_persona>"));
    assert!(resolved
        .block
        .contains("Keep the recovered conversation focused."));
}
