use std::{
    error::Error,
    io,
    sync::{Arc, Mutex, OnceLock},
};

use async_trait::async_trait;
use tauri::Manager;

use crate::application::{
    chat_service::{AppChatService, ChatService, SendMessageOptions},
    AppState,
};
use crate::shell::setup_settings::initialize_settings_defaults;
use crate::domain::agents::{AgentHarnessKind, AgentProviderSettings, DEFAULT_AGENT_HARNESS};
use crate::domain::entities::{
    app_state::ExecutionHaltMode, ChatContextType, ChatConversation, Project,
};
use crate::domain::repositories::AgentProviderSettingsRepository;
use crate::infrastructure::memory::MemoryAgentProviderSettingsRepository;

struct FailingAgentProviderSettingsRepository;

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

fn claude_spawn_permission_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn provider_repo_error() -> Box<dyn Error> {
    Box::new(io::Error::other("provider repo failed"))
}

#[async_trait]
impl AgentProviderSettingsRepository for FailingAgentProviderSettingsRepository {
    async fn get(
        &self,
        _provider: AgentHarnessKind,
    ) -> Result<Option<AgentProviderSettings>, Box<dyn Error>> {
        Err(provider_repo_error())
    }

    async fn list(&self) -> Result<Vec<AgentProviderSettings>, Box<dyn Error>> {
        Err(provider_repo_error())
    }

    async fn get_default(&self) -> Result<Option<AgentProviderSettings>, Box<dyn Error>> {
        Err(provider_repo_error())
    }

    async fn upsert(
        &self,
        _settings: &AgentProviderSettings,
    ) -> Result<AgentProviderSettings, Box<dyn Error>> {
        Err(provider_repo_error())
    }
}

fn assert_initialization_leaves_global_execution_running(
    provider_repo: Arc<dyn AgentProviderSettingsRepository>,
) {
    let mut app_state = AppState::new_test();
    app_state.agent_provider_settings_repo = provider_repo;
    let execution_state = Arc::new(Default::default());

    tauri::async_runtime::block_on(initialize_settings_defaults(
        &app_state,
        Arc::clone(&execution_state),
    ));

    assert!(
        !execution_state.is_paused(),
        "provider onboarding readiness must not mutate the global pause barrier"
    );
    let app_settings = tauri::async_runtime::block_on(app_state.app_state_repo.get())
        .expect("app settings should remain readable");
    assert_eq!(
        app_settings.execution_halt_mode,
        ExecutionHaltMode::Running,
        "provider onboarding must not persist a global halt mode"
    );
}

#[test]
fn initialize_settings_defaults_keeps_execution_running_without_provider_rows() {
    assert_initialization_leaves_global_execution_running(Arc::new(
        MemoryAgentProviderSettingsRepository::new(),
    ));
}

#[test]
fn initialize_settings_defaults_keeps_execution_running_with_disabled_default_provider() {
    let repo = Arc::new(MemoryAgentProviderSettingsRepository::new());
    let mut settings = AgentProviderSettings::disabled_defaults(DEFAULT_AGENT_HARNESS);
    settings.is_default = true;
    tauri::async_runtime::block_on(repo.upsert(&settings)).expect("seed disabled default");

    assert_initialization_leaves_global_execution_running(repo);
}

#[test]
fn initialize_settings_defaults_keeps_execution_running_without_marked_default_provider() {
    let repo = Arc::new(MemoryAgentProviderSettingsRepository::new());
    let mut settings = AgentProviderSettings::disabled_defaults(DEFAULT_AGENT_HARNESS);
    settings.enabled = true;
    tauri::async_runtime::block_on(repo.upsert(&settings)).expect("seed enabled provider");

    assert_initialization_leaves_global_execution_running(repo);
}

#[test]
fn initialize_settings_defaults_keeps_execution_running_when_provider_read_fails() {
    assert_initialization_leaves_global_execution_running(Arc::new(
        FailingAgentProviderSettingsRepository,
    ));
}

#[test]
fn initialize_settings_defaults_keeps_execution_running_with_default_provider() {
    let mut app_state = AppState::new_test();
    app_state.agent_provider_settings_repo = Arc::new(
        MemoryAgentProviderSettingsRepository::with_all_providers_enabled(DEFAULT_AGENT_HARNESS),
    );
    let execution_state = Arc::new(Default::default());

    tauri::async_runtime::block_on(initialize_settings_defaults(
        &app_state,
        Arc::clone(&execution_state),
    ));

    assert!(!execution_state.is_paused());
}

#[test]
fn initialize_settings_defaults_does_not_resume_an_existing_global_pause() {
    let mut app_state = AppState::new_test();
    app_state.agent_provider_settings_repo = Arc::new(
        MemoryAgentProviderSettingsRepository::with_all_providers_enabled(DEFAULT_AGENT_HARNESS),
    );
    let execution_state = Arc::new(Default::default());
    tauri::async_runtime::block_on(initialize_settings_defaults(
        &app_state,
        Arc::clone(&execution_state),
    ));
    execution_state.pause();

    tauri::async_runtime::block_on(initialize_settings_defaults(
        &app_state,
        Arc::clone(&execution_state),
    ));

    assert!(
        execution_state.is_paused(),
        "settings initialization must preserve an authoritative global pause"
    );
}

#[test]
fn missing_provider_blocks_first_project_send_without_queueing_or_pausing() {
    let mut app_state = AppState::new_test();
    app_state.agent_provider_settings_repo = Arc::new(MemoryAgentProviderSettingsRepository::new());
    let execution_state = Arc::new(Default::default());
    tauri::async_runtime::block_on(initialize_settings_defaults(
        &app_state,
        Arc::clone(&execution_state),
    ));

    tauri::async_runtime::block_on(async {
        let project = app_state
            .project_repo
            .create(Project::new(
                "fresh install project".to_string(),
                "/tmp/fresh-install-project".to_string(),
            ))
            .await
            .expect("create project");
        let conversation = app_state
            .chat_conversation_repo
            .create(ChatConversation::new_project(project.id.clone()))
            .await
            .expect("create project conversation");
        let service =
            app_state.build_chat_service_with_execution_state(Arc::clone(&execution_state));

        let error = service
            .send_message(
                ChatContextType::Project,
                project.id.as_str(),
                "first prompt",
                SendMessageOptions {
                    conversation_id_override: Some(conversation.id.clone()),
                    ..Default::default()
                },
            )
            .await
            .expect_err("missing provider should fail closed before queueing");

        assert!(error.to_string().contains("Settings > Harness > Providers"));
        assert!(
            app_state
                .message_queue
                .get_queued(ChatContextType::Project, &conversation.id.as_str())
                .is_empty(),
            "provider setup must not create a paused project-message queue item"
        );
        assert!(
            app_state
                .chat_message_repo
                .get_by_conversation(&conversation.id)
                .await
                .expect("read first-send messages")
                .is_empty(),
            "provider setup failure must happen before the first prompt is persisted"
        );
    });

    assert!(
        !execution_state.is_paused(),
        "provider setup failure must leave global execution running"
    );
}

#[cfg(unix)]
#[test]
#[allow(clippy::await_holding_lock)]
fn onboarding_then_first_project_send_starts_without_manual_resume() {
    use std::os::unix::fs::PermissionsExt;

    let _spawn_guard = claude_spawn_permission_lock()
        .lock()
        .expect("spawn permission lock");
    let _spawn_permission = EnvVarGuard::set("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");
    let mut app_state = AppState::new_test();
    app_state.agent_provider_settings_repo = Arc::new(MemoryAgentProviderSettingsRepository::new());
    let execution_state = Arc::new(Default::default());
    tauri::async_runtime::block_on(initialize_settings_defaults(
        &app_state,
        Arc::clone(&execution_state),
    ));

    let project_dir = tempfile::tempdir().expect("project directory");
    let cli_path = project_dir.path().join("fake-claude");
    std::fs::write(
        &cli_path,
        r#"#!/bin/sh
cat >/dev/null &
stdin_drain_pid=$!
printf '%s\n' '{"type":"assistant","message":{"content":[{"type":"text","text":"ready"}]},"session_id":"fresh-install-session"}'
printf '%s\n' '{"type":"result","session_id":"fresh-install-session","is_error":false,"result":"ready","cost_usd":0.0}'
sleep 1
kill "$stdin_drain_pid" 2>/dev/null || true
wait "$stdin_drain_pid" 2>/dev/null || true
"#,
    )
    .expect("write fake Claude CLI");
    std::fs::set_permissions(&cli_path, std::fs::Permissions::from_mode(0o755))
        .expect("make fake Claude CLI executable");

    tauri::async_runtime::block_on(async {
        let mut provider = AgentProviderSettings::disabled_defaults(DEFAULT_AGENT_HARNESS);
        provider.enabled = true;
        provider.is_default = true;
        app_state
            .agent_provider_settings_repo
            .upsert(&provider)
            .await
            .expect("complete provider onboarding");
        let project = app_state
            .project_repo
            .create(Project::new(
                "onboarded project".to_string(),
                project_dir.path().to_string_lossy().into_owned(),
            ))
            .await
            .expect("create project");
        let conversation = app_state
            .chat_conversation_repo
            .create(ChatConversation::new_project(project.id.clone()))
            .await
            .expect("create project conversation");
        let conversation_id = conversation.id.clone();
        let app = tauri::test::mock_builder()
            .manage(app_state)
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        let service: AppChatService = app
            .state::<AppState>()
            .build_chat_service_with_execution_state(Arc::clone(&execution_state))
            .with_cli_path(cli_path)
            .with_working_directory(project_dir.path());

        let result = service
            .send_message(
                ChatContextType::Project,
                project.id.as_str(),
                "first prompt after onboarding",
                SendMessageOptions {
                    conversation_id_override: Some(conversation_id.clone()),
                    ..Default::default()
                },
            )
            .await
            .expect("first prompt should start after onboarding");

        assert!(!result.was_queued);
        assert!(!result.queued_as_pending);
        assert!(
            app.state::<AppState>()
                .message_queue
                .get_queued(ChatContextType::Project, &conversation_id.as_str())
                .is_empty(),
            "first prompt must not be parked behind a stale startup pause"
        );
    });

    assert!(
        !execution_state.is_paused(),
        "onboarding must not require an explicit global resume"
    );
}
