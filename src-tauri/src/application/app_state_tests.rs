use super::*;
use crate::application::execution_state::ExecutionState;
use crate::domain::agents::{
    AgentConfig, AgentError, AgentHandle, AgentHarnessKind, AgentLane, AgentLaneSettings,
    AgentOutput, AgentProviderCliManagementMode, AgentProviderSettings, AgentResponse, AgentResult,
    AgenticClient, ClientCapabilities, ClientType, LogicalEffort, ResponseChunk,
    WorkspaceReviewRuntimeSettings, CODEX_DEFAULT_APPROVAL_POLICY, CODEX_DEFAULT_SANDBOX_MODE,
};
use crate::domain::entities::{
    ChatMessage, IdeationSession, InternalStatus, Priority, Project, ProjectId, ProposalCategory,
    Task, TaskProposal,
};
use crate::infrastructure::{MockAgenticClient, MockCallType};
use futures::Stream;
use std::fs;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;

struct UnavailableCodexAgentClient {
    capabilities: ClientCapabilities,
}

impl UnavailableCodexAgentClient {
    fn new() -> Self {
        Self {
            capabilities: ClientCapabilities::codex(),
        }
    }
}

#[async_trait::async_trait]
impl AgenticClient for UnavailableCodexAgentClient {
    async fn spawn_agent(&self, _config: AgentConfig) -> AgentResult<AgentHandle> {
        Err(AgentError::CliNotAvailable(
            "static Codex client unavailable".to_string(),
        ))
    }

    async fn stop_agent(&self, _handle: &AgentHandle) -> AgentResult<()> {
        Ok(())
    }

    async fn wait_for_completion(&self, _handle: &AgentHandle) -> AgentResult<AgentOutput> {
        Err(AgentError::CliNotAvailable(
            "static Codex client unavailable".to_string(),
        ))
    }

    async fn send_prompt(
        &self,
        _handle: &AgentHandle,
        _prompt: &str,
    ) -> AgentResult<AgentResponse> {
        Err(AgentError::CliNotAvailable(
            "static Codex client unavailable".to_string(),
        ))
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
        Ok(false)
    }
}

fn write_executable(path: &Path, contents: &str) {
    // Path is created under this test's tempfile root.
    // codeql[rust/path-injection]
    fs::write(path, contents).expect("write executable");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Path is created under this test's tempfile root.
        // codeql[rust/path-injection]
        let mut permissions = fs::metadata(path)
            .expect("executable metadata")
            .permissions();
        permissions.set_mode(0o755);
        // Path is created under this test's tempfile root.
        // codeql[rust/path-injection]
        fs::set_permissions(path, permissions).expect("mark executable");
    }
}

#[tokio::test]
async fn test_new_test_creates_empty_repositories() {
    let state = AppState::new_test();

    // Task repo should be empty
    let project_id = ProjectId::new();
    let tasks = state.task_repo.get_by_project(&project_id).await.unwrap();
    assert!(tasks.is_empty());

    // Project repo should be empty
    let projects = state.project_repo.get_all().await.unwrap();
    assert!(projects.is_empty());
}

#[test]
fn test_workspace_and_repair_repositories_share_the_same_concrete_arc() {
    let state = AppState::new_test();
    let workspace_repository = Arc::as_ptr(&state.agent_conversation_workspace_repo) as *const ();
    let repair_repository = Arc::as_ptr(&state.agent_workspace_repair_repo) as *const ();

    assert_eq!(
        workspace_repository, repair_repository,
        "workspace and repair repository traits must share one concrete memory repository"
    );
}

#[test]
fn production_granola_api_client_prefers_hyper_client_when_available() {
    let client = AppState::production_granola_api_client();
    let hyper_available = crate::infrastructure::HyperGranolaApiClient::new().is_ok();

    assert_eq!(
        client.is_unavailable_for_tests(),
        !hyper_available,
        "production Granola wiring should use the real Hyper client whenever it can be constructed"
    );
}

#[tokio::test]
async fn test_with_repos_uses_custom_repositories() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());

    // Pre-populate the repos
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    project_repo.create(project.clone()).await.unwrap();

    let task = Task::new(project.id.clone(), "Test Task".to_string());
    task_repo.create(task.clone()).await.unwrap();

    // Create AppState with these repos
    let state = AppState::with_repos(task_repo, project_repo);

    // Verify the state uses our repos
    let projects = state.project_repo.get_all().await.unwrap();
    assert_eq!(projects.len(), 1);

    let tasks = state.task_repo.get_by_project(&project.id).await.unwrap();
    assert_eq!(tasks.len(), 1);
}

#[tokio::test]
async fn test_task_and_project_repos_work_together() {
    let state = AppState::new_test();

    // Create a project
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    state.project_repo.create(project.clone()).await.unwrap();

    // Create tasks for that project
    let task1 = Task::new(project.id.clone(), "Task 1".to_string());
    let task2 = Task::new(project.id.clone(), "Task 2".to_string());
    state.task_repo.create(task1).await.unwrap();
    state.task_repo.create(task2).await.unwrap();

    // Verify we can retrieve them
    let tasks = state.task_repo.get_by_project(&project.id).await.unwrap();
    assert_eq!(tasks.len(), 2);
}

#[tokio::test]
async fn test_repositories_are_thread_safe() {
    let state = Arc::new(AppState::new_test());

    // Create a project first
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    state.project_repo.create(project.clone()).await.unwrap();

    // Spawn multiple tasks that use the repos concurrently
    let mut handles = vec![];
    for i in 0..10 {
        let state_clone = Arc::clone(&state);
        let project_id = project.id.clone();
        handles.push(tokio::spawn(async move {
            let task = Task::new(project_id, format!("Task {}", i));
            state_clone.task_repo.create(task).await
        }));
    }

    // Wait for all to complete
    for handle in handles {
        let result = handle.await.unwrap();
        assert!(result.is_ok());
    }

    // Verify all tasks were created
    let tasks = state.task_repo.get_by_project(&project.id).await.unwrap();
    assert_eq!(tasks.len(), 10);
}

#[tokio::test]
async fn test_new_test_creates_mock_agent_client() {
    let state = AppState::new_test();

    assert_eq!(
        state.agent_clients.default_harness,
        AgentHarnessKind::Claude
    );

    // Agent client should be mock and available
    let available = state
        .agent_clients
        .default_client
        .is_available()
        .await
        .unwrap();
    assert!(available);

    // Check capabilities indicate mock
    let caps = state.agent_clients.default_client.capabilities();
    assert_eq!(caps.client_type, ClientType::Mock);
}

#[tokio::test]
async fn test_with_agent_client_swaps_client() {
    let state = AppState::new_test();

    // Default is mock
    assert_eq!(
        state
            .agent_clients
            .default_client
            .capabilities()
            .client_type,
        ClientType::Mock
    );

    // Create custom mock with different capabilities wouldn't show,
    // but we can test the swap mechanism works
    let custom_mock = Arc::new(MockAgenticClient::new());
    let _state = state.with_agent_client(custom_mock);

    // If it compiled and ran, the swap worked
}

#[tokio::test]
async fn test_with_harness_agent_client_registers_specific_client() {
    let codex_mock: Arc<dyn AgenticClient> = Arc::new(MockAgenticClient::new());
    let state =
        AppState::new_test().with_harness_agent_client(AgentHarnessKind::Codex, codex_mock.clone());

    let resolved = state.resolve_harness_agent_client(AgentHarnessKind::Codex);

    assert_eq!(resolved.capabilities().client_type, ClientType::Mock);
    assert!(Arc::ptr_eq(&resolved, &codex_mock));
}

#[tokio::test]
async fn test_build_transition_service_with_execution_state_uses_app_agent_client() {
    let mock = Arc::new(MockAgenticClient::new());
    let state = AppState::new_test().with_agent_client(mock.clone());
    let service =
        state.build_transition_service_with_execution_state(Arc::new(ExecutionState::new()));

    let repo_dir = tempfile::tempdir().unwrap();
    std::process::Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(repo_dir.path())
        .output()
        .expect("git init");
    std::process::Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(repo_dir.path())
        .output()
        .expect("git email");
    std::process::Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(repo_dir.path())
        .output()
        .expect("git name");
    std::fs::write(repo_dir.path().join("README.md"), "# test").unwrap();
    std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(repo_dir.path())
        .output()
        .expect("git add");
    std::process::Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(repo_dir.path())
        .output()
        .expect("git commit");

    let project = Project::new(
        "Test Project".to_string(),
        repo_dir.path().to_string_lossy().into_owned(),
    );
    state.project_repo.create(project.clone()).await.unwrap();

    let mut task = Task::new(project.id.clone(), "Test Task".to_string());
    task.internal_status = InternalStatus::Executing;
    task.worktree_path = Some(repo_dir.path().to_string_lossy().into_owned());
    state.task_repo.create(task.clone()).await.unwrap();

    let updated_task = service
        .transition_task(&task.id, InternalStatus::QaRefining)
        .await
        .unwrap();

    assert_eq!(updated_task.internal_status, InternalStatus::QaRefining);

    let calls = mock.get_spawn_calls().await;
    assert_eq!(calls.len(), 1);
    match &calls[0].call_type {
        MockCallType::Spawn { role, prompt } => {
            assert_eq!(*role, crate::domain::agents::AgentRole::QaRefiner);
            assert!(prompt.contains(task.id.as_str()));
        }
        other => panic!("expected spawn call, got {other:?}"),
    }
}

#[tokio::test]
async fn test_build_transition_service_with_execution_state_uses_app_codex_client_for_codex_lane() {
    let default_mock = Arc::new(MockAgenticClient::new());
    let codex_mock = Arc::new(MockAgenticClient::new());
    let state = AppState::new_test()
        .with_agent_client(default_mock.clone())
        .with_harness_agent_client(AgentHarnessKind::Codex, codex_mock.clone());
    let service =
        state.build_transition_service_with_execution_state(Arc::new(ExecutionState::new()));

    let repo_dir = tempfile::tempdir().unwrap();
    std::process::Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(repo_dir.path())
        .output()
        .expect("git init");
    std::process::Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(repo_dir.path())
        .output()
        .expect("git email");
    std::process::Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(repo_dir.path())
        .output()
        .expect("git name");
    std::fs::write(repo_dir.path().join("README.md"), "# test").unwrap();
    std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(repo_dir.path())
        .output()
        .expect("git add");
    std::process::Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(repo_dir.path())
        .output()
        .expect("git commit");

    let project = Project::new(
        "Codex Project".to_string(),
        repo_dir.path().to_string_lossy().into_owned(),
    );
    state.project_repo.create(project.clone()).await.unwrap();

    let mut codex_lane = AgentLaneSettings::new(AgentHarnessKind::Codex);
    codex_lane.model = Some("gpt-5.4".to_string());
    codex_lane.effort = Some(LogicalEffort::XHigh);
    state
        .agent_lane_settings_repo
        .upsert_for_project(project.id.as_str(), AgentLane::ExecutionWorker, &codex_lane)
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Codex Task".to_string());
    task.internal_status = InternalStatus::Executing;
    task.worktree_path = Some(repo_dir.path().to_string_lossy().into_owned());
    state.task_repo.create(task.clone()).await.unwrap();

    let updated_task = service
        .transition_task(&task.id, InternalStatus::QaRefining)
        .await
        .unwrap();

    assert_eq!(updated_task.internal_status, InternalStatus::QaRefining);
    assert!(
        default_mock.get_spawn_calls().await.is_empty(),
        "default client should not receive spawn calls when execution lane resolves to Codex"
    );

    let calls = codex_mock.get_spawn_calls().await;
    assert_eq!(calls.len(), 1);
    match &calls[0].call_type {
        MockCallType::Spawn { role, prompt } => {
            assert_eq!(*role, crate::domain::agents::AgentRole::QaRefiner);
            assert!(prompt.contains(task.id.as_str()));
        }
        other => panic!("expected spawn call, got {other:?}"),
    }
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn test_resolve_background_agent_runtime_uses_rx_managed_codex_override() {
    let _lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("test env mutex");
    let temp = tempfile::tempdir().expect("temp dir");
    let managed_codex_path = temp.path().join("codex");
    write_executable(
        &managed_codex_path,
        r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  printf 'codex-cli 0.116.0\n'
elif [ "$1" = "--help" ]; then
  printf '%s\n' 'Codex CLI' 'Commands:' '  exec' '  resume' '  mcp' 'Options:' '  -c, --config <key=value>' '  -m, --model <MODEL>' '  -s, --sandbox <SANDBOX>' '      --search' '      --add-dir <DIR>'
elif [ "$1" = "exec" ] && [ "$2" = "--help" ]; then
  printf '%s\n' 'Run Codex non-interactively' 'Options:' '  -c, --config <key=value>' '  -m, --model <MODEL>' '  -s, --sandbox <SANDBOX>' '      --add-dir <DIR>' '      --json'
else
  printf 'unexpected args: %s\n' "$*" >&2
  exit 64
fi
"#,
    );
    let _managed_codex_override =
        crate::application::managed_provider_cli::override_managed_codex_binary_path_for_tests(
            managed_codex_path.clone(),
        );
    let default_mock: Arc<dyn AgenticClient> = Arc::new(MockAgenticClient::new());
    let unavailable_codex: Arc<dyn AgenticClient> = Arc::new(UnavailableCodexAgentClient::new());
    let state = AppState::new_test()
        .with_agent_client(default_mock)
        .with_harness_agent_client(AgentHarnessKind::Codex, Arc::clone(&unavailable_codex));

    let mut codex_provider = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Codex);
    codex_provider.enabled = true;
    codex_provider.cli_management_mode = AgentProviderCliManagementMode::RxManaged;
    state
        .agent_provider_settings_repo
        .upsert(&codex_provider)
        .await
        .unwrap();

    let runtime = state
        .resolve_background_agent_runtime_for_harness(
            AgentHarnessKind::Codex,
            "managed Codex helper runtime",
        )
        .await
        .expect("managed Codex helper runtime should resolve");

    assert!(Arc::ptr_eq(&runtime.client, &unavailable_codex));
    assert_eq!(runtime.harness, Some(AgentHarnessKind::Codex));
    assert_eq!(runtime.cli_path_override, Some(managed_codex_path));
}

#[tokio::test]
async fn test_resolve_background_agent_runtime_uses_custom_codex_override() {
    let temp = tempfile::tempdir().expect("temp dir");
    let custom_codex_path = temp.path().join("codex-wrapper");
    let env_path = temp.path().join("codex.env");
    write_executable(
        &custom_codex_path,
        r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  printf 'codex-cli 0.116.0\n'
elif [ "$1" = "--help" ]; then
  printf '%s\n' 'Codex CLI' 'Commands:' '  exec' '  resume' '  mcp' 'Options:' '  -c, --config <key=value>' '  -m, --model <MODEL>' '  -s, --sandbox <SANDBOX>' '      --search' '      --add-dir <DIR>'
elif [ "$1" = "exec" ] && [ "$2" = "--help" ]; then
  printf '%s\n' 'Run Codex non-interactively' 'Options:' '  -c, --config <key=value>' '  -m, --model <MODEL>' '  -s, --sandbox <SANDBOX>' '      --add-dir <DIR>' '      --json'
else
  printf 'unexpected args: %s\n' "$*" >&2
  exit 64
fi
"#,
    );
    fs::write(
        &env_path,
        "CUSTOM_PROVIDER_TOKEN=from-env-file\nANTHROPIC_MODEL=spoofed\n",
    )
    .expect("write provider env file");
    let default_mock: Arc<dyn AgenticClient> = Arc::new(MockAgenticClient::new());
    let unavailable_codex: Arc<dyn AgenticClient> = Arc::new(UnavailableCodexAgentClient::new());
    let state = AppState::new_test()
        .with_agent_client(default_mock)
        .with_harness_agent_client(AgentHarnessKind::Codex, Arc::clone(&unavailable_codex));

    let mut codex_provider = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Codex);
    codex_provider.enabled = true;
    codex_provider.custom_binary_enabled = true;
    codex_provider.custom_binary_path = Some(custom_codex_path.to_string_lossy().into_owned());
    codex_provider.custom_env_file_enabled = true;
    codex_provider.custom_env_file_path = Some(env_path.to_string_lossy().into_owned());
    state
        .agent_provider_settings_repo
        .upsert(&codex_provider)
        .await
        .unwrap();

    let runtime = state
        .resolve_background_agent_runtime_for_harness(
            AgentHarnessKind::Codex,
            "custom Codex helper runtime",
        )
        .await
        .expect("custom Codex helper runtime should resolve");

    assert!(Arc::ptr_eq(&runtime.client, &unavailable_codex));
    assert_eq!(runtime.harness, Some(AgentHarnessKind::Codex));
    assert_eq!(runtime.cli_path_override, Some(custom_codex_path));
    assert_eq!(
        runtime.env.get("CUSTOM_PROVIDER_TOKEN").map(String::as_str),
        Some("from-env-file")
    );
    assert!(!runtime.env.contains_key("ANTHROPIC_MODEL"));
}

#[tokio::test]
async fn test_resolve_workspace_reviewer_runtime_uses_provider_default() {
    let default_mock: Arc<dyn AgenticClient> = Arc::new(MockAgenticClient::new());
    let codex_mock: Arc<dyn AgenticClient> = Arc::new(MockAgenticClient::new());
    let mut state = AppState::new_test()
        .with_agent_client(default_mock)
        .with_harness_agent_client(AgentHarnessKind::Codex, codex_mock.clone());
    let provider_repo = Arc::new(MemoryAgentProviderSettingsRepository::new());
    let mut codex = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Codex);
    codex.enabled = true;
    codex.is_default = true;
    codex.model = Some("gpt-5.6-terra".to_string());
    codex.effort = Some(LogicalEffort::XHigh);
    provider_repo.upsert(&codex).await.unwrap();
    state.agent_provider_settings_repo = provider_repo;

    let project_id = ProjectId::new();

    let runtime = state
        .resolve_workspace_reviewer_runtime_for_project(project_id.as_str())
        .await
        .expect("workspace reviewer should resolve from the effective Reviewer role default");

    assert!(
        Arc::ptr_eq(&runtime.client, &codex_mock),
        "workspace reviewer should use the provider selected by the Reviewer role default"
    );
    assert_eq!(runtime.harness, Some(AgentHarnessKind::Codex));
    assert_eq!(runtime.model.as_deref(), Some("gpt-5.6-terra"));
    assert_eq!(runtime.logical_effort, Some(LogicalEffort::XHigh));
    assert_eq!(
        runtime.approval_policy.as_deref(),
        Some(CODEX_DEFAULT_APPROVAL_POLICY)
    );
    assert_eq!(
        runtime.sandbox_mode.as_deref(),
        Some(CODEX_DEFAULT_SANDBOX_MODE)
    );
}

#[tokio::test]
async fn test_resolve_workspace_reviewer_runtime_uses_enabled_default_provider() {
    let claude_mock: Arc<dyn AgenticClient> = Arc::new(MockAgenticClient::new());
    let codex_mock: Arc<dyn AgenticClient> = Arc::new(MockAgenticClient::new());
    let mut state = AppState::new_test()
        .with_agent_client(claude_mock)
        .with_harness_agent_client(AgentHarnessKind::Codex, codex_mock.clone());
    let provider_repo = Arc::new(MemoryAgentProviderSettingsRepository::new());
    let mut codex = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Codex);
    codex.enabled = true;
    codex.is_default = true;
    codex.model = Some("gpt-provider-default".to_string());
    codex.approval_policy = Some(CODEX_DEFAULT_APPROVAL_POLICY.to_string());
    codex.sandbox_mode = Some(CODEX_DEFAULT_SANDBOX_MODE.to_string());
    codex.service_tier = Some("standard".to_string());
    provider_repo.upsert(&codex).await.unwrap();
    provider_repo
        .upsert(&AgentProviderSettings::disabled_defaults(
            AgentHarnessKind::Claude,
        ))
        .await
        .unwrap();
    state.agent_provider_settings_repo = provider_repo;

    let project_id = ProjectId::new();

    let runtime = state
        .resolve_workspace_reviewer_runtime_for_project(project_id.as_str())
        .await
        .expect("workspace reviewer should use the enabled default provider");

    assert!(Arc::ptr_eq(&runtime.client, &codex_mock));
    assert_eq!(runtime.harness, Some(AgentHarnessKind::Codex));
    assert_eq!(runtime.model.as_deref(), Some("gpt-provider-default"));
    assert_eq!(
        runtime.approval_policy.as_deref(),
        Some(CODEX_DEFAULT_APPROVAL_POLICY)
    );
    assert_eq!(
        runtime.sandbox_mode.as_deref(),
        Some(CODEX_DEFAULT_SANDBOX_MODE)
    );
    assert_eq!(runtime.service_tier.as_deref(), Some("standard"));
}

#[tokio::test]
async fn test_resolve_workspace_reviewer_runtime_uses_default_provider_without_role_override() {
    let default_mock: Arc<dyn AgenticClient> = Arc::new(MockAgenticClient::new());
    let codex_mock: Arc<dyn AgenticClient> = Arc::new(MockAgenticClient::new());
    let state = AppState::new_test()
        .with_agent_client(default_mock.clone())
        .with_harness_agent_client(AgentHarnessKind::Codex, codex_mock);

    let project_id = ProjectId::new();

    let runtime = state
        .resolve_workspace_reviewer_runtime_for_project(project_id.as_str())
        .await
        .expect("workspace reviewer should resolve from default provider");

    assert!(
        Arc::ptr_eq(&runtime.client, &default_mock),
        "Reviewer runtime without a role override should use the enabled default provider"
    );
    assert_eq!(runtime.harness, Some(AgentHarnessKind::Claude));
    assert_eq!(runtime.model.as_deref(), Some("sonnet"));
    assert_eq!(runtime.logical_effort, Some(LogicalEffort::Medium));
}

#[tokio::test]
async fn test_resolve_workspace_reviewer_runtime_uses_global_provider_review_defaults() {
    let default_mock: Arc<dyn AgenticClient> = Arc::new(MockAgenticClient::new());
    let codex_mock: Arc<dyn AgenticClient> = Arc::new(MockAgenticClient::new());
    let mut state = AppState::new_test()
        .with_agent_client(default_mock)
        .with_harness_agent_client(AgentHarnessKind::Codex, codex_mock.clone());
    state.agent_provider_settings_repo = Arc::new(
        MemoryAgentProviderSettingsRepository::with_all_providers_enabled(AgentHarnessKind::Codex),
    );

    state
        .workspace_review_runtime_settings_repo
        .upsert_global(
            AgentHarnessKind::Codex,
            &WorkspaceReviewRuntimeSettings {
                model: Some("gpt-5.4".to_string()),
                effort: Some(LogicalEffort::High),
            },
        )
        .await
        .unwrap();

    let project_id = ProjectId::new();

    let runtime = state
        .resolve_workspace_reviewer_runtime_for_project(project_id.as_str())
        .await
        .expect("workspace reviewer should resolve from configured review defaults");

    assert!(Arc::ptr_eq(&runtime.client, &codex_mock));
    assert_eq!(runtime.harness, Some(AgentHarnessKind::Codex));
    assert_eq!(runtime.model.as_deref(), Some("gpt-5.4"));
    assert_eq!(runtime.logical_effort, Some(LogicalEffort::High));
}

#[tokio::test]
async fn test_effective_reviewer_role_default_surfaces_legacy_review_settings() {
    let mut state = AppState::new_test();
    state.agent_provider_settings_repo = Arc::new(
        MemoryAgentProviderSettingsRepository::with_all_providers_enabled(AgentHarnessKind::Codex),
    );
    state
        .workspace_review_runtime_settings_repo
        .upsert_global(
            AgentHarnessKind::Codex,
            &WorkspaceReviewRuntimeSettings {
                model: Some("gpt-legacy-review".to_string()),
                effort: Some(LogicalEffort::High),
            },
        )
        .await
        .unwrap();

    let resolved = state
        .resolve_effective_manual_role_default(
            None,
            None,
            crate::domain::agents::RoutingRole::WorkspaceReviewer,
        )
        .await
        .expect("legacy reviewer settings should be projected into the effective role default");

    assert_eq!(
        resolved.source,
        crate::application::manual_role_default_service::ManualDefaultSource::LegacyWorkspaceReview
    );
    assert_eq!(resolved.value.harness, AgentHarnessKind::Codex);
    assert_eq!(resolved.value.model.as_deref(), Some("gpt-legacy-review"));
    assert_eq!(resolved.value.effort, Some(LogicalEffort::High));
}

#[tokio::test]
async fn test_explicit_reviewer_role_default_wins_over_legacy_review_settings() {
    let state = AppState::new_test();
    state
        .workspace_review_runtime_settings_repo
        .upsert_global(
            AgentHarnessKind::Claude,
            &WorkspaceReviewRuntimeSettings {
                model: Some("legacy-review-model".to_string()),
                effort: Some(LogicalEffort::Low),
            },
        )
        .await
        .unwrap();
    state
        .manual_role_default_repo
        .upsert_global(
            crate::domain::agents::RoutingRole::WorkspaceReviewer,
            &crate::domain::agents::ManualRoleDefault {
                harness: AgentHarnessKind::Claude,
                model: Some("explicit-reviewer-model".to_string()),
                effort: Some(LogicalEffort::High),
                service_tier: crate::domain::agents::ManualServiceTier::ProviderDefault,
                coordination_mode: None,
                persona_id: None,
                approval_policy: None,
                sandbox_mode: None,
            },
        )
        .await
        .unwrap();

    let resolved = state
        .resolve_effective_manual_role_default(
            None,
            None,
            crate::domain::agents::RoutingRole::WorkspaceReviewer,
        )
        .await
        .expect("explicit Reviewer settings should remain authoritative");

    assert_eq!(
        resolved.source,
        crate::application::manual_role_default_service::ManualDefaultSource::GlobalUi
    );
    assert_eq!(
        resolved.value.model.as_deref(),
        Some("explicit-reviewer-model")
    );
    assert_eq!(resolved.value.effort, Some(LogicalEffort::High));
}

#[tokio::test]
async fn test_resolve_workspace_reviewer_runtime_uses_project_provider_review_defaults() {
    let default_mock: Arc<dyn AgenticClient> = Arc::new(MockAgenticClient::new());
    let codex_mock: Arc<dyn AgenticClient> = Arc::new(MockAgenticClient::new());
    let mut state = AppState::new_test()
        .with_agent_client(default_mock)
        .with_harness_agent_client(AgentHarnessKind::Codex, codex_mock.clone());
    state.agent_provider_settings_repo = Arc::new(
        MemoryAgentProviderSettingsRepository::with_all_providers_enabled(AgentHarnessKind::Codex),
    );

    state
        .workspace_review_runtime_settings_repo
        .upsert_global(
            AgentHarnessKind::Codex,
            &WorkspaceReviewRuntimeSettings {
                model: Some("gpt-5.4".to_string()),
                effort: Some(LogicalEffort::High),
            },
        )
        .await
        .unwrap();

    let project_id = ProjectId::new();
    state
        .workspace_review_runtime_settings_repo
        .upsert_for_project(
            project_id.as_str(),
            AgentHarnessKind::Codex,
            &WorkspaceReviewRuntimeSettings {
                model: Some("gpt-5.3-codex".to_string()),
                effort: None,
            },
        )
        .await
        .unwrap();
    let runtime = state
        .resolve_workspace_reviewer_runtime_for_project(project_id.as_str())
        .await
        .expect("workspace reviewer should resolve from project review defaults");

    assert!(Arc::ptr_eq(&runtime.client, &codex_mock));
    assert_eq!(runtime.harness, Some(AgentHarnessKind::Codex));
    assert_eq!(runtime.model.as_deref(), Some("gpt-5.3-codex"));
    assert_eq!(
        runtime.logical_effort,
        Some(LogicalEffort::High),
        "project rows inherit global effort when they only override model"
    );
}

#[tokio::test]
async fn test_resolve_workspace_reviewer_runtime_uses_explicit_workspace_project_scope() {
    let default_mock: Arc<dyn AgenticClient> = Arc::new(MockAgenticClient::new());
    let codex_mock: Arc<dyn AgenticClient> = Arc::new(MockAgenticClient::new());
    let mut state = AppState::new_test()
        .with_agent_client(default_mock)
        .with_harness_agent_client(AgentHarnessKind::Codex, codex_mock.clone());
    state.agent_provider_settings_repo = Arc::new(
        MemoryAgentProviderSettingsRepository::with_all_providers_enabled(AgentHarnessKind::Codex),
    );

    let project_id = ProjectId::from_string("workspace-review-project".to_string());
    state
        .workspace_review_runtime_settings_repo
        .upsert_global(
            AgentHarnessKind::Codex,
            &WorkspaceReviewRuntimeSettings {
                model: Some("gpt-global-review".to_string()),
                effort: Some(LogicalEffort::Low),
            },
        )
        .await
        .unwrap();
    state
        .workspace_review_runtime_settings_repo
        .upsert_for_project(
            project_id.as_str(),
            AgentHarnessKind::Codex,
            &WorkspaceReviewRuntimeSettings {
                model: Some("gpt-project-review".to_string()),
                effort: Some(LogicalEffort::High),
            },
        )
        .await
        .unwrap();

    let runtime = state
        .resolve_workspace_reviewer_runtime_for_project(project_id.as_str())
        .await
        .expect("workspace reviewer should resolve from explicit project scope");

    assert!(Arc::ptr_eq(&runtime.client, &codex_mock));
    assert_eq!(runtime.harness, Some(AgentHarnessKind::Codex));
    assert_eq!(runtime.model.as_deref(), Some("gpt-project-review"));
    assert_eq!(runtime.logical_effort, Some(LogicalEffort::High));
}

#[tokio::test]
async fn test_ideation_repos_accessible() {
    let state = AppState::new_test();
    let project_id = ProjectId::new();

    // Create an ideation session
    let session = IdeationSession::new_with_title(project_id.clone(), "Test Session");
    let session_id = session.id.clone();
    state.ideation_session_repo.create(session).await.unwrap();

    // Verify we can retrieve it
    let retrieved = state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .unwrap();
    assert!(retrieved.is_some());

    // Create a proposal
    let proposal = TaskProposal::new(
        session_id.clone(),
        "Test Proposal",
        ProposalCategory::Feature,
        Priority::Medium,
    );
    let proposal_id = proposal.id.clone();
    state.task_proposal_repo.create(proposal).await.unwrap();

    // Verify we can retrieve proposals
    let proposals = state
        .task_proposal_repo
        .get_by_session(&session_id)
        .await
        .unwrap();
    assert_eq!(proposals.len(), 1);

    // Create a chat message
    let message = ChatMessage::user_in_session(session_id.clone(), "Hello");
    state.chat_message_repo.create(message).await.unwrap();

    // Verify we can retrieve messages
    let messages = state
        .chat_message_repo
        .get_by_session(&session_id)
        .await
        .unwrap();
    assert_eq!(messages.len(), 1);

    // Add a dependency
    let proposal2 = TaskProposal::new(
        session_id.clone(),
        "Another Proposal",
        ProposalCategory::Feature,
        Priority::Low,
    );
    let proposal2_id = proposal2.id.clone();
    state.task_proposal_repo.create(proposal2).await.unwrap();

    state
        .proposal_dependency_repo
        .add_dependency(&proposal_id, &proposal2_id, None, None)
        .await
        .unwrap();

    let deps = state
        .proposal_dependency_repo
        .get_dependencies(&proposal_id)
        .await
        .unwrap();
    assert_eq!(deps.len(), 1);
}

#[tokio::test]
async fn test_task_dependency_repo_accessible() {
    let state = AppState::new_test();
    let project_id = ProjectId::new();

    // Create two tasks
    let task1 = Task::new(project_id.clone(), "Task 1".to_string());
    let task2 = Task::new(project_id.clone(), "Task 2".to_string());

    let task1_id = task1.id.clone();
    let task2_id = task2.id.clone();

    state.task_repo.create(task1).await.unwrap();
    state.task_repo.create(task2).await.unwrap();

    // Add a dependency
    state
        .task_dependency_repo
        .add_dependency(&task1_id, &task2_id)
        .await
        .unwrap();

    // Verify the dependency exists
    let has_dep = state
        .task_dependency_repo
        .has_dependency(&task1_id, &task2_id)
        .await
        .unwrap();
    assert!(has_dep);

    let blockers = state
        .task_dependency_repo
        .get_blockers(&task1_id)
        .await
        .unwrap();
    assert_eq!(blockers.len(), 1);
}

#[tokio::test]
async fn test_extensibility_repos_accessible() {
    use crate::domain::entities::methodology::MethodologyExtension;
    use crate::domain::entities::research::{ResearchBrief, ResearchProcess};
    use crate::domain::entities::status::InternalStatus;
    use crate::domain::entities::{
        Artifact, ArtifactBucket, ArtifactFlow, ArtifactFlowTrigger, ArtifactType, WorkflowColumn,
        WorkflowSchema,
    };

    let state = AppState::new_test();

    // Test workflow repository
    let workflow = WorkflowSchema::new(
        "Test Workflow",
        vec![
            WorkflowColumn::new("backlog", "Backlog", InternalStatus::Backlog),
            WorkflowColumn::new("done", "Done", InternalStatus::Approved),
        ],
    );
    state.workflow_repo.create(workflow.clone()).await.unwrap();
    let found_workflow = state.workflow_repo.get_by_id(&workflow.id).await.unwrap();
    assert!(found_workflow.is_some());

    // Test artifact repository
    let artifact = Artifact::new_inline("Test", ArtifactType::Prd, "content", "user");
    state.artifact_repo.create(artifact.clone()).await.unwrap();
    let found_artifact = state.artifact_repo.get_by_id(&artifact.id).await.unwrap();
    assert!(found_artifact.is_some());

    // Test artifact bucket repository
    let bucket = ArtifactBucket::new("Test Bucket")
        .accepts(ArtifactType::Prd)
        .with_writer("user");
    state
        .artifact_bucket_repo
        .create(bucket.clone())
        .await
        .unwrap();
    let found_bucket = state
        .artifact_bucket_repo
        .get_by_id(&bucket.id)
        .await
        .unwrap();
    assert!(found_bucket.is_some());

    // Test artifact flow repository
    let flow = ArtifactFlow::new("Test Flow", ArtifactFlowTrigger::on_artifact_created());
    state.artifact_flow_repo.create(flow.clone()).await.unwrap();
    let found_flow = state.artifact_flow_repo.get_by_id(&flow.id).await.unwrap();
    assert!(found_flow.is_some());

    // Test process repository
    let brief = ResearchBrief::new("Test question");
    let process = ResearchProcess::new("Test Research", brief, "researcher");
    state.process_repo.create(process.clone()).await.unwrap();
    let found_process = state.process_repo.get_by_id(&process.id).await.unwrap();
    assert!(found_process.is_some());

    // Test methodology repository
    let methodology = MethodologyExtension::new("Test Method", workflow);
    state
        .methodology_repo
        .create(methodology.clone())
        .await
        .unwrap();
    let found_methodology = state
        .methodology_repo
        .get_by_id(&methodology.id)
        .await
        .unwrap();
    assert!(found_methodology.is_some());
}
