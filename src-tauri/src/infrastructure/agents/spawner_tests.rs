use super::*;
use crate::commands::ExecutionState;
use crate::domain::agents::{
    AgentConfig, AgentHandle, AgentHarnessKind, AgentLane, AgentLaneSettings, AgentOutput,
    AgentProviderSettings, AgentResponse, AgentResult, AgenticClient, ClientCapabilities,
    ClientType, ResponseChunk, CODEX_DEFAULT_APPROVAL_POLICY, CODEX_DEFAULT_SANDBOX_MODE,
};
use crate::domain::entities::{GitMode, Project, ProjectId, Task, TaskId};
use crate::domain::execution::ExecutionSettings;
use crate::domain::repositories::{
    AgentProviderSettingsRepository, ProjectRepository, StateHistoryMetadata, StatusTransition,
    TaskRepository,
};
use crate::domain::services::{MemoryRunningAgentRegistry, RunningAgentKey, RunningAgentRegistry};
use crate::error::AppResult;
use crate::infrastructure::memory::{
    MemoryAgentLaneSettingsRepository, MemoryAgentProviderSettingsRepository,
    MemoryExecutionSettingsRepository, MemoryIdeationSessionRepository, MemoryProjectRepository,
    MemoryTaskRepository,
};
use crate::infrastructure::MockAgenticClient;
use async_trait::async_trait;
use futures::Stream;
use std::fs;
use std::pin::Pin;
use tokio::sync::RwLock;

// ==================== Mock Repos for CWD Tests ====================

fn make_local_plugin_task_workdir(task_name: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("create temp repo");
    let repo_root = dir.path().join("repo");
    fs::create_dir_all(repo_root.join("plugins/app")).expect("create plugin dir");
    let task_workdir = repo_root.join(task_name);
    fs::create_dir_all(&task_workdir).expect("create task workdir");
    (dir, task_workdir)
}

fn make_runtime_plugin_layout() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().expect("create runtime plugin tempdir");
    let plugin_dir = dir.path().join("plugins/app");
    let generated_dir = dir.path().join("generated/claude-plugin");

    fs::create_dir_all(plugin_dir.join("agents")).expect("create agents dir");
    fs::write(plugin_dir.join("agents/session-namer.md"), "# Session Namer\n")
        .expect("write agent prompt");

    (dir, plugin_dir, generated_dir)
}

/// Minimal mock TaskRepository that returns a configurable task
struct MockTaskRepoForSpawner {
    task: Option<Task>,
}

#[async_trait]
impl TaskRepository for MockTaskRepoForSpawner {
    async fn create(&self, task: Task) -> AppResult<Task> {
        Ok(task)
    }
    async fn get_by_id(&self, _id: &TaskId) -> AppResult<Option<Task>> {
        Ok(self.task.clone())
    }
    async fn get_by_project(&self, _: &ProjectId) -> AppResult<Vec<Task>> {
        Ok(vec![])
    }
    async fn update(&self, _: &Task) -> AppResult<()> {
        Ok(())
    }
    async fn update_with_expected_status(
        &self,
        _: &Task,
        _: crate::domain::entities::InternalStatus,
    ) -> AppResult<bool> {
        Ok(true)
    }
    async fn update_metadata(&self, _: &TaskId, _: Option<String>) -> AppResult<()> {
        Ok(())
    }
    async fn delete(&self, _: &TaskId) -> AppResult<()> {
        Ok(())
    }
    async fn get_by_status(
        &self,
        _: &ProjectId,
        _: crate::domain::entities::InternalStatus,
    ) -> AppResult<Vec<Task>> {
        Ok(vec![])
    }
    async fn persist_status_change(
        &self,
        _: &TaskId,
        _: crate::domain::entities::InternalStatus,
        _: crate::domain::entities::InternalStatus,
        _: &str,
    ) -> AppResult<()> {
        Ok(())
    }
    async fn get_status_history(&self, _: &TaskId) -> AppResult<Vec<StatusTransition>> {
        Ok(vec![])
    }
    async fn get_status_entered_at(
        &self,
        _: &TaskId,
        _: crate::domain::entities::InternalStatus,
    ) -> AppResult<Option<chrono::DateTime<chrono::Utc>>> {
        Ok(None)
    }
    async fn get_next_executable(&self, _: &ProjectId) -> AppResult<Option<Task>> {
        Ok(None)
    }
    async fn get_by_ideation_session(
        &self,
        _: &crate::domain::entities::IdeationSessionId,
    ) -> AppResult<Vec<Task>> {
        Ok(vec![])
    }
    async fn get_by_project_filtered(&self, _: &ProjectId, _: bool) -> AppResult<Vec<Task>> {
        Ok(vec![])
    }
    async fn archive(&self, _: &TaskId) -> AppResult<Task> {
        unimplemented!()
    }
    async fn restore(&self, _: &TaskId) -> AppResult<Task> {
        unimplemented!()
    }
    async fn get_archived_count(&self, _: &ProjectId, _: Option<&str>) -> AppResult<u32> {
        Ok(0)
    }
    async fn list_paginated(
        &self,
        _: &ProjectId,
        _: Option<Vec<crate::domain::entities::InternalStatus>>,
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
    async fn get_stale_ready_tasks(&self, _: u64) -> AppResult<Vec<Task>> {
        Ok(vec![])
    }
    async fn update_latest_state_history_metadata(
        &self,
        _: &TaskId,
        _: &StateHistoryMetadata,
    ) -> AppResult<()> {
        Ok(())
    }
    async fn has_task_in_states(
        &self,
        _: &ProjectId,
        _: &[crate::domain::entities::InternalStatus],
    ) -> AppResult<bool> {
        Ok(false)
    }
    async fn get_status_history_batch(
        &self,
        _task_ids: &[TaskId],
    ) -> AppResult<std::collections::HashMap<TaskId, Vec<StatusTransition>>> {
        Ok(std::collections::HashMap::new())
    }
}

/// Minimal mock ProjectRepository that returns a configurable project
struct MockProjectRepoForSpawner {
    project: Option<Project>,
}

#[async_trait]
impl ProjectRepository for MockProjectRepoForSpawner {
    async fn create(&self, project: Project) -> AppResult<Project> {
        Ok(project)
    }
    async fn get_by_id(&self, _: &ProjectId) -> AppResult<Option<Project>> {
        Ok(self.project.clone())
    }
    async fn get_all(&self) -> AppResult<Vec<Project>> {
        Ok(vec![])
    }
    async fn update(&self, _: &Project) -> AppResult<()> {
        Ok(())
    }
    async fn delete(&self, _: &ProjectId) -> AppResult<()> {
        Ok(())
    }
    async fn get_by_working_directory(&self, _: &str) -> AppResult<Option<Project>> {
        Ok(None)
    }

    async fn archive(&self, _id: &ProjectId) -> AppResult<Project> {
        unimplemented!()
    }
}

struct TestAgentClient {
    client_type: ClientType,
    available: bool,
    spawns: Arc<RwLock<Vec<AgentConfig>>>,
    capabilities: ClientCapabilities,
}

impl TestAgentClient {
    fn new(client_type: ClientType, available: bool) -> Self {
        let capabilities = match client_type {
            ClientType::Codex => ClientCapabilities::codex(),
            ClientType::ClaudeCode => ClientCapabilities::claude_code(),
            ClientType::Mock => ClientCapabilities::mock(),
            _ => ClientCapabilities::mock(),
        };
        Self {
            client_type,
            available,
            spawns: Arc::new(RwLock::new(Vec::new())),
            capabilities,
        }
    }

    async fn spawn_count(&self) -> usize {
        self.spawns.read().await.len()
    }

    async fn last_spawn(&self) -> Option<AgentConfig> {
        self.spawns.read().await.last().cloned()
    }
}

#[async_trait]
impl AgenticClient for TestAgentClient {
    async fn spawn_agent(&self, config: AgentConfig) -> AgentResult<AgentHandle> {
        self.spawns.write().await.push(config.clone());
        Ok(AgentHandle::new(self.client_type.clone(), config.role))
    }

    async fn stop_agent(&self, _handle: &AgentHandle) -> AgentResult<()> {
        Ok(())
    }

    async fn wait_for_completion(&self, _handle: &AgentHandle) -> AgentResult<AgentOutput> {
        Ok(AgentOutput::success("ok"))
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
        Ok(self.available)
    }
}

#[test]
fn test_role_from_string() {
    // Standard roles
    assert_eq!(
        AgenticClientSpawner::role_from_string("worker"),
        AgentRole::Worker
    );
    assert_eq!(
        AgenticClientSpawner::role_from_string("qa-prep"),
        AgentRole::QaPrep
    );
    assert_eq!(
        AgenticClientSpawner::role_from_string("qa-refiner"),
        AgentRole::QaRefiner
    );
    assert_eq!(
        AgenticClientSpawner::role_from_string("qa-tester"),
        AgentRole::QaTester
    );
    assert_eq!(
        AgenticClientSpawner::role_from_string("reviewer"),
        AgentRole::Reviewer
    );
    // Custom role
    assert_eq!(
        AgenticClientSpawner::role_from_string("my-custom-agent"),
        AgentRole::Custom("my-custom-agent".to_string())
    );
}

#[tokio::test]
async fn test_spawn_calls_client() {
    let mock = Arc::new(MockAgenticClient::new());
    let spawner = AgenticClientSpawner::new(mock.clone());

    spawner.spawn("worker", "task-123").await;

    let calls = mock.get_spawn_calls().await;
    assert_eq!(calls.len(), 1);
}

#[tokio::test]
async fn test_spawn_uses_correct_role() {
    let mock = Arc::new(MockAgenticClient::new());
    let spawner = AgenticClientSpawner::new(mock.clone());

    spawner.spawn("reviewer", "task-456").await;

    let calls = mock.get_spawn_calls().await;
    assert_eq!(calls.len(), 1);
    if let crate::infrastructure::MockCallType::Spawn { role, prompt } = &calls[0].call_type {
        assert_eq!(*role, AgentRole::Reviewer);
        assert!(prompt.contains("task-456"));
    } else {
        panic!("Expected Spawn call");
    }
}

#[tokio::test]
async fn test_spawn_background_calls_client() {
    let mock = Arc::new(MockAgenticClient::new());
    let spawner = AgenticClientSpawner::new(mock.clone());

    spawner.spawn_background("qa-prep", "task-789").await;

    let calls = mock.get_spawn_calls().await;
    assert_eq!(calls.len(), 1);
}

#[tokio::test]
async fn test_with_working_dir() {
    let mock = Arc::new(MockAgenticClient::new());
    let spawner = AgenticClientSpawner::new(mock.clone()).with_working_dir("/custom/work/dir");

    assert_eq!(spawner.working_directory, PathBuf::from("/custom/work/dir"));
}

#[tokio::test]
async fn test_wait_for_is_noop() {
    let mock = Arc::new(MockAgenticClient::new());
    let spawner = AgenticClientSpawner::new(mock.clone());

    // Should not panic or error
    spawner.wait_for("worker", "task-123").await;
}

// ==================== Event Bus Tests ====================

#[tokio::test]
async fn test_with_event_bus() {
    let mock = Arc::new(MockAgenticClient::new());
    let event_bus = Arc::new(EventBus::new());
    let spawner = AgenticClientSpawner::new(mock.clone()).with_event_bus(event_bus.clone());

    assert!(spawner.event_bus().is_some());
}

#[tokio::test]
async fn test_spawn_emits_task_start_event() {
    let mock = Arc::new(MockAgenticClient::new());
    let event_bus = Arc::new(EventBus::new());
    let mut subscriber = event_bus.subscribe();

    let spawner = AgenticClientSpawner::new(mock.clone()).with_event_bus(event_bus);

    spawner.spawn("worker", "task-123").await;

    // Check that TaskStart event was emitted
    let event = subscriber.try_recv().unwrap();
    if let SupervisorEvent::TaskStart {
        task_id,
        agent_role,
        ..
    } = event
    {
        assert_eq!(task_id, "task-123");
        assert_eq!(agent_role, "worker");
    } else {
        panic!("Expected TaskStart event, got {:?}", event);
    }
}

#[tokio::test]
async fn test_emit_tool_call() {
    let mock = Arc::new(MockAgenticClient::new());
    let event_bus = Arc::new(EventBus::new());
    let mut subscriber = event_bus.subscribe();

    let spawner = AgenticClientSpawner::new(mock.clone()).with_event_bus(event_bus);

    let info = ToolCallInfo::new("Write", r#"{"path": "test.txt"}"#);
    spawner.emit_tool_call("task-123", info);

    let event = subscriber.try_recv().unwrap();
    if let SupervisorEvent::ToolCall { task_id, info } = event {
        assert_eq!(task_id, "task-123");
        assert_eq!(info.tool_name, "Write");
    } else {
        panic!("Expected ToolCall event, got {:?}", event);
    }
}

#[tokio::test]
async fn test_emit_error() {
    let mock = Arc::new(MockAgenticClient::new());
    let event_bus = Arc::new(EventBus::new());
    let mut subscriber = event_bus.subscribe();

    let spawner = AgenticClientSpawner::new(mock.clone()).with_event_bus(event_bus);

    let info = ErrorInfo::new("Something went wrong", "test_source");
    spawner.emit_error("task-123", info);

    let event = subscriber.try_recv().unwrap();
    if let SupervisorEvent::Error { task_id, info } = event {
        assert_eq!(task_id, "task-123");
        assert_eq!(info.message, "Something went wrong");
        assert_eq!(info.source, "test_source");
    } else {
        panic!("Expected Error event, got {:?}", event);
    }
}

#[tokio::test]
async fn test_spawn_without_event_bus_works() {
    let mock = Arc::new(MockAgenticClient::new());
    let spawner = AgenticClientSpawner::new(mock.clone());

    // Should not panic even without event bus
    spawner.spawn("worker", "task-123").await;

    let calls = mock.get_spawn_calls().await;
    assert_eq!(calls.len(), 1);
}

#[tokio::test]
async fn test_emit_without_event_bus_is_noop() {
    let mock = Arc::new(MockAgenticClient::new());
    let spawner = AgenticClientSpawner::new(mock.clone());

    // Should not panic
    let info = ToolCallInfo::new("Read", "{}");
    spawner.emit_tool_call("task-123", info);

    let error_info = ErrorInfo::new("Test error", "test");
    spawner.emit_error("task-123", error_info);
}

#[tokio::test]
async fn test_multiple_spawns_emit_multiple_events() {
    let mock = Arc::new(MockAgenticClient::new());
    let event_bus = Arc::new(EventBus::new());
    let mut subscriber = event_bus.subscribe();

    let spawner = AgenticClientSpawner::new(mock.clone()).with_event_bus(event_bus);

    spawner.spawn("worker", "task-1").await;
    spawner.spawn("reviewer", "task-2").await;
    spawner.spawn("custom-agent", "task-3").await;

    // Check all three events
    let event1 = subscriber.try_recv().unwrap();
    let event2 = subscriber.try_recv().unwrap();
    let event3 = subscriber.try_recv().unwrap();

    assert_eq!(event1.task_id(), "task-1");
    assert_eq!(event2.task_id(), "task-2");
    assert_eq!(event3.task_id(), "task-3");
}

// ==================== Execution State Tests ====================

#[tokio::test]
async fn test_with_execution_state() {
    let mock = Arc::new(MockAgenticClient::new());
    let exec_state = Arc::new(ExecutionState::new());
    let spawner = AgenticClientSpawner::new(mock.clone()).with_execution_state(exec_state.clone());

    assert!(spawner.execution_state.is_some());
}

#[tokio::test]
async fn test_spawn_blocked_when_paused() {
    let mock = Arc::new(MockAgenticClient::new());
    let exec_state = Arc::new(ExecutionState::new());

    // Pause execution
    exec_state.pause();

    let spawner = AgenticClientSpawner::new(mock.clone()).with_execution_state(exec_state.clone());

    // Try to spawn while paused
    spawner.spawn("worker", "task-123").await;

    // Verify no spawn occurred
    let calls = mock.get_spawn_calls().await;
    assert_eq!(calls.len(), 0, "Should not spawn when paused");

    // Running count should not have incremented
    assert_eq!(exec_state.running_count(), 0);
}

#[tokio::test]
async fn test_spawn_blocked_at_max_concurrent() {
    let mock = Arc::new(MockAgenticClient::new());
    let exec_state = Arc::new(ExecutionState::with_max_concurrent(2));
    exec_state.set_global_max_concurrent(2);

    // Fill up to max concurrent
    exec_state.increment_running();
    exec_state.increment_running();

    let spawner = AgenticClientSpawner::new(mock.clone()).with_execution_state(exec_state.clone());

    // Try to spawn at max concurrent
    spawner.spawn("worker", "task-123").await;

    // Verify no spawn occurred
    let calls = mock.get_spawn_calls().await;
    assert_eq!(calls.len(), 0, "Should not spawn when at max concurrent");

    // Running count should still be 2 (not incremented)
    assert_eq!(exec_state.running_count(), 2);
}

#[tokio::test]
async fn test_spawn_blocked_when_same_project_capacity_reached() {
    let mock = Arc::new(MockAgenticClient::new());
    let exec_state = Arc::new(ExecutionState::with_max_concurrent(5));
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let settings_repo = Arc::new(MemoryExecutionSettingsRepository::new());
    let ideation_session_repo = Arc::new(MemoryIdeationSessionRepository::new());
    let agent_lane_settings_repo = Arc::new(MemoryAgentLaneSettingsRepository::new());
    let running_agent_registry = Arc::new(MemoryRunningAgentRegistry::new());

    let project_id = ProjectId::from_string("project-a".to_string());
    let mut project = Project::new("Project A".to_string(), "/tmp/project-a".to_string());
    project.id = project_id.clone();
    project_repo.create(project).await.unwrap();

    settings_repo
        .update_settings(
            Some(&project_id),
            &ExecutionSettings {
                max_concurrent_tasks: 1,
                ..ExecutionSettings::default()
            },
        )
        .await
        .unwrap();

    let mut running_task = Task::new(project_id.clone(), "Running task".to_string());
    running_task.id = TaskId::from_string("task-running".to_string());
    running_task.internal_status = crate::domain::entities::InternalStatus::Executing;
    running_task.worktree_path = Some("/tmp/task-running".to_string());
    task_repo.create(running_task.clone()).await.unwrap();

    let mut candidate_task = Task::new(project_id.clone(), "Candidate task".to_string());
    candidate_task.id = TaskId::from_string("task-candidate".to_string());
    candidate_task.worktree_path = Some("/tmp/task-candidate".to_string());
    task_repo.create(candidate_task).await.unwrap();

    running_agent_registry
        .register(
            RunningAgentKey::new("task_execution", running_task.id.as_str()),
            4242,
            "conv-running".to_string(),
            "run-running".to_string(),
            Some("/tmp/task-running".to_string()),
            None,
        )
        .await;

    let spawner = AgenticClientSpawner::new(mock.clone())
        .with_repos(task_repo, project_repo)
        .with_execution_state(exec_state)
        .with_runtime_admission_context(
            settings_repo,
            agent_lane_settings_repo,
            ideation_session_repo,
            running_agent_registry,
        )
        .with_working_dir("/tmp");

    spawner.spawn("worker", "task-candidate").await;

    let calls = mock.get_spawn_calls().await;
    assert_eq!(
        calls.len(),
        0,
        "Should not spawn when same-project capacity is full"
    );
}

#[tokio::test]
async fn test_spawn_ignores_other_project_capacity_usage() {
    let mock = Arc::new(MockAgenticClient::new());
    let exec_state = Arc::new(ExecutionState::with_max_concurrent(5));
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let settings_repo = Arc::new(MemoryExecutionSettingsRepository::new());
    let ideation_session_repo = Arc::new(MemoryIdeationSessionRepository::new());
    let agent_lane_settings_repo = Arc::new(MemoryAgentLaneSettingsRepository::new());
    let running_agent_registry = Arc::new(MemoryRunningAgentRegistry::new());

    let project_a_id = ProjectId::from_string("project-a".to_string());
    let mut project_a = Project::new("Project A".to_string(), "/tmp/project-a".to_string());
    project_a.id = project_a_id.clone();
    project_repo.create(project_a).await.unwrap();

    let project_b_id = ProjectId::from_string("project-b".to_string());
    let mut project_b = Project::new("Project B".to_string(), "/tmp/project-b".to_string());
    project_b.id = project_b_id.clone();
    project_repo.create(project_b).await.unwrap();

    settings_repo
        .update_settings(
            Some(&project_a_id),
            &ExecutionSettings {
                max_concurrent_tasks: 1,
                ..ExecutionSettings::default()
            },
        )
        .await
        .unwrap();
    settings_repo
        .update_settings(
            Some(&project_b_id),
            &ExecutionSettings {
                max_concurrent_tasks: 1,
                ..ExecutionSettings::default()
            },
        )
        .await
        .unwrap();

    let mut other_project_running = Task::new(project_b_id.clone(), "Busy elsewhere".to_string());
    other_project_running.id = TaskId::from_string("task-other-project".to_string());
    other_project_running.internal_status = crate::domain::entities::InternalStatus::Executing;
    other_project_running.worktree_path = Some("/tmp/task-other-project".to_string());
    task_repo
        .create(other_project_running.clone())
        .await
        .unwrap();

    let mut candidate_task = Task::new(project_a_id.clone(), "Candidate task".to_string());
    candidate_task.id = TaskId::from_string("task-candidate".to_string());
    candidate_task.worktree_path = Some("/tmp/task-candidate".to_string());
    task_repo.create(candidate_task).await.unwrap();

    running_agent_registry
        .register(
            RunningAgentKey::new("task_execution", other_project_running.id.as_str()),
            5252,
            "conv-other".to_string(),
            "run-other".to_string(),
            Some("/tmp/task-other-project".to_string()),
            None,
        )
        .await;

    let spawner = AgenticClientSpawner::new(mock.clone())
        .with_repos(task_repo, project_repo)
        .with_execution_state(exec_state)
        .with_runtime_admission_context(
            settings_repo,
            agent_lane_settings_repo,
            ideation_session_repo,
            running_agent_registry,
        )
        .with_working_dir("/tmp");

    spawner.spawn("worker", "task-candidate").await;

    let calls = mock.get_spawn_calls().await;
    assert_eq!(
        calls.len(),
        1,
        "Should still spawn when only another project is busy"
    );
}

#[tokio::test]
async fn test_spawn_increments_running_count() {
    let mock = Arc::new(MockAgenticClient::new());
    let exec_state = Arc::new(ExecutionState::with_max_concurrent(5));

    let spawner = AgenticClientSpawner::new(mock.clone()).with_execution_state(exec_state.clone());

    // Verify initial state
    assert_eq!(exec_state.running_count(), 0);

    // Spawn a task
    spawner.spawn("worker", "task-1").await;

    // Verify running count incremented
    assert_eq!(exec_state.running_count(), 1);

    // Spawn another task
    spawner.spawn("reviewer", "task-2").await;

    // Verify running count incremented again
    assert_eq!(exec_state.running_count(), 2);

    // Verify spawns actually occurred
    let calls = mock.get_spawn_calls().await;
    assert_eq!(calls.len(), 2);
}

#[tokio::test]
async fn test_spawn_without_execution_state_still_works() {
    let mock = Arc::new(MockAgenticClient::new());
    // No execution state attached
    let spawner = AgenticClientSpawner::new(mock.clone());

    // Should still spawn normally
    spawner.spawn("worker", "task-123").await;

    let calls = mock.get_spawn_calls().await;
    assert_eq!(calls.len(), 1);
}

// ==================== App Handle Tests ====================

#[test]
fn test_app_handle_defaults_to_none() {
    let mock = Arc::new(MockAgenticClient::new());
    let spawner = AgenticClientSpawner::new(mock.clone());

    // By default, app_handle should be None
    assert!(spawner.app_handle.is_none());
}

#[test]
fn test_app_handle_field_accessible() {
    let mock = Arc::new(MockAgenticClient::new());
    let spawner = AgenticClientSpawner::new(mock.clone());

    // Verify app_handle can be accessed (compile-time check + runtime assertion)
    // Note: with_app_handle() requires a real AppHandle<Wry> which is not available in tests,
    // but we verify the field exists and defaults correctly.
    let _handle: &Option<AppHandle<Wry>> = &spawner.app_handle;
    assert!(spawner.app_handle.is_none());
}

#[tokio::test]
async fn test_spawn_with_execution_state_no_app_handle_does_not_panic() {
    // Verifies that spawn() handles the case where execution_state is Some
    // but app_handle is None (the emit_status_changed call is skipped gracefully).
    // Note: Actual event emission with app_handle requires a real Wry runtime,
    // which is tested via integration tests and execution_commands.rs emit tests.
    let mock = Arc::new(MockAgenticClient::new());
    let exec_state = Arc::new(ExecutionState::with_max_concurrent(5));

    // No app_handle attached, but execution_state is present
    let spawner = AgenticClientSpawner::new(mock.clone()).with_execution_state(exec_state.clone());

    // Should spawn without panicking (emit_status_changed is skipped when app_handle is None)
    spawner.spawn("worker", "task-123").await;

    // Verify spawn occurred and running count incremented
    let calls = mock.get_spawn_calls().await;
    assert_eq!(calls.len(), 1);
    assert_eq!(exec_state.running_count(), 1);
}

// ==================== Per-Task CWD Resolution Tests ====================

#[tokio::test]
async fn test_resolve_working_directory_worktree_mode() {
    let mock = Arc::new(MockAgenticClient::new());

    let project_id = ProjectId("proj-1".to_string());
    let mut task = Task::new(project_id.clone(), "Test task".to_string());
    task.id = TaskId("task-worktree".to_string());
    task.worktree_path = Some("/worktrees/task-worktree".to_string());

    let mut project = Project::new("Test Project".to_string(), "/project/root".to_string());
    project.id = project_id;
    project.git_mode = GitMode::Worktree;

    let task_repo: Arc<dyn TaskRepository> = Arc::new(MockTaskRepoForSpawner { task: Some(task) });
    let project_repo: Arc<dyn ProjectRepository> = Arc::new(MockProjectRepoForSpawner {
        project: Some(project),
    });

    let spawner = AgenticClientSpawner::new(mock)
        .with_working_dir("/fallback")
        .with_repos(task_repo, project_repo);

    let resolved = spawner.resolve_working_directory("task-worktree").await;
    assert_eq!(resolved, PathBuf::from("/worktrees/task-worktree"));
}

#[tokio::test]
async fn test_resolve_working_directory_worktree_mode_no_worktree_path() {
    let mock = Arc::new(MockAgenticClient::new());

    let project_id = ProjectId("proj-2".to_string());
    let mut task = Task::new(project_id.clone(), "Test task".to_string());
    task.id = TaskId("task-no-wt".to_string());
    // worktree_path is None

    let mut project = Project::new("Test Project".to_string(), "/project/root".to_string());
    project.id = project_id;
    project.git_mode = GitMode::Worktree;

    let task_repo: Arc<dyn TaskRepository> = Arc::new(MockTaskRepoForSpawner { task: Some(task) });
    let project_repo: Arc<dyn ProjectRepository> = Arc::new(MockProjectRepoForSpawner {
        project: Some(project),
    });

    let spawner = AgenticClientSpawner::new(mock)
        .with_working_dir("/fallback")
        .with_repos(task_repo, project_repo);

    // Safety net: falls back to spawner default (NOT project dir) when worktree_path is None
    let resolved = spawner.resolve_working_directory("task-no-wt").await;
    assert_eq!(resolved, PathBuf::from("/fallback"));
}

#[tokio::test]
async fn test_resolve_working_directory_fallback_no_repos() {
    let mock = Arc::new(MockAgenticClient::new());

    // No repos attached — should fall back to self.working_directory
    let spawner = AgenticClientSpawner::new(mock).with_working_dir("/fallback");

    let resolved = spawner.resolve_working_directory("any-task").await;
    assert_eq!(resolved, PathBuf::from("/fallback"));
}

#[tokio::test]
async fn test_resolve_working_directory_fallback_task_not_found() {
    let mock = Arc::new(MockAgenticClient::new());

    // Repos attached but task not found
    let task_repo: Arc<dyn TaskRepository> = Arc::new(MockTaskRepoForSpawner { task: None });
    let project_repo: Arc<dyn ProjectRepository> =
        Arc::new(MockProjectRepoForSpawner { project: None });

    let spawner = AgenticClientSpawner::new(mock)
        .with_working_dir("/fallback")
        .with_repos(task_repo, project_repo);

    let resolved = spawner.resolve_working_directory("nonexistent-task").await;
    assert_eq!(resolved, PathBuf::from("/fallback"));
}

#[test]
fn test_build_agent_config_for_mock_client_omits_claude_plugin_wiring() {
    let mock = Arc::new(MockAgenticClient::new());
    let spawner = AgenticClientSpawner::new(mock);

    let config = spawner.build_agent_config(
        AgentHarnessKind::Claude,
        ClientType::Mock,
        AgentRole::Worker,
        "worker",
        "task-123",
        PathBuf::from("/tmp/task-123"),
        Some("project-123".to_string()),
        None,
        None,
        None,
        None,
    );

    assert_eq!(config.role, AgentRole::Worker);
    assert_eq!(config.prompt, "Execute task task-123");
    assert_eq!(config.working_directory, PathBuf::from("/tmp/task-123"));
    assert!(config.plugin_dir.is_none());
    assert!(config.agent.is_none());
    assert_eq!(
        config.env.get("RALPHX_PROJECT_ID").map(String::as_str),
        Some("project-123")
    );
}

#[test]
fn test_build_agent_config_for_claude_client_sets_plugin_and_agent() {
    let (_runtime_dir, plugin_dir, generated_dir) = make_runtime_plugin_layout();
    let _runtime_plugin_dir_guard =
        crate::infrastructure::agents::claude::override_runtime_plugin_dirs_for_tests(
            plugin_dir,
            generated_dir.clone(),
        );
    let client = Arc::new(crate::infrastructure::ClaudeCodeClient::new());
    let spawner = AgenticClientSpawner::new(client);

    let config = spawner.build_agent_config(
        AgentHarnessKind::Claude,
        ClientType::ClaudeCode,
        AgentRole::QaRefiner,
        "qa-refiner",
        "task-456",
        PathBuf::from("/tmp/task-456"),
        None,
        None,
        None,
        None,
        None,
    );

    assert_eq!(config.role, AgentRole::QaRefiner);
    assert_eq!(config.prompt, "Execute task task-456");
    assert_eq!(
        config.agent.as_deref(),
        Some("ralphx:ralphx-qa-executor")
    );
    assert_eq!(config.plugin_dir, Some(generated_dir));
}

#[test]
fn test_build_agent_config_for_codex_client_uses_process_mapping() {
    let (_runtime_dir, plugin_dir, generated_dir) = make_runtime_plugin_layout();
    let _runtime_plugin_dir_guard =
        crate::infrastructure::agents::claude::override_runtime_plugin_dirs_for_tests(
            plugin_dir,
            generated_dir.clone(),
        );
    let default_client = Arc::new(MockAgenticClient::new());
    let spawner = AgenticClientSpawner::new(default_client);
    let (_dir, task_workdir) = make_local_plugin_task_workdir("task-789");

    let config = spawner.build_agent_config(
        AgentHarnessKind::Codex,
        ClientType::Codex,
        AgentRole::QaRefiner,
        "qa-refiner",
        "task-789",
        task_workdir.clone(),
        None,
        Some("gpt-5.4".to_string()),
        Some(crate::domain::agents::LogicalEffort::XHigh),
        Some("on-request".to_string()),
        Some("workspace-write".to_string()),
    );

    assert_eq!(config.harness, Some(AgentHarnessKind::Codex));
    assert_eq!(config.agent.as_deref(), Some("ralphx:ralphx-qa-executor"));
    assert_eq!(config.plugin_dir, Some(generated_dir));
    assert_eq!(config.model.as_deref(), Some("gpt-5.4"));
    assert_eq!(
        config.logical_effort,
        Some(crate::domain::agents::LogicalEffort::XHigh)
    );
}

#[tokio::test]
async fn test_spawn_uses_codex_client_when_execution_lane_resolves_to_codex() {
    let default_client = Arc::new(TestAgentClient::new(ClientType::ClaudeCode, true));
    let codex_client = Arc::new(TestAgentClient::new(ClientType::Codex, true));
    let exec_state = Arc::new(ExecutionState::with_max_concurrent(5));
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let settings_repo = Arc::new(MemoryExecutionSettingsRepository::new());
    let ideation_session_repo = Arc::new(MemoryIdeationSessionRepository::new());
    let agent_lane_settings_repo = Arc::new(MemoryAgentLaneSettingsRepository::new());
    let running_agent_registry = Arc::new(MemoryRunningAgentRegistry::new());

    let project_id = ProjectId::from_string("project-codex".to_string());
    let mut project = Project::new(
        "Project Codex".to_string(),
        "/tmp/project-codex".to_string(),
    );
    project.id = project_id.clone();
    project_repo.create(project).await.unwrap();

    let mut task = Task::new(project_id.clone(), "Codex lane task".to_string());
    task.id = TaskId::from_string("task-codex".to_string());
    task.worktree_path = Some("/tmp/task-codex".to_string());
    task_repo.create(task).await.unwrap();

    let mut lane_settings = AgentLaneSettings::new(AgentHarnessKind::Codex);
    lane_settings.model = Some("gpt-5.4".to_string());
    lane_settings.effort = Some(crate::domain::agents::LogicalEffort::XHigh);
    lane_settings.approval_policy = Some("on-request".to_string());
    lane_settings.sandbox_mode = Some("workspace-write".to_string());
    agent_lane_settings_repo
        .upsert_for_project(
            project_id.as_str(),
            AgentLane::ExecutionWorker,
            &lane_settings,
        )
        .await
        .unwrap();

    let spawner = AgenticClientSpawner::new(default_client.clone())
        .with_harness_client(AgentHarnessKind::Codex, codex_client.clone())
        .with_repos(task_repo, project_repo)
        .with_execution_state(exec_state)
        .with_runtime_admission_context(
            settings_repo,
            agent_lane_settings_repo,
            ideation_session_repo,
            running_agent_registry,
        )
        .with_working_dir("/tmp");

    spawner.spawn("worker", "task-codex").await;

    assert_eq!(default_client.spawn_count().await, 0);
    assert_eq!(codex_client.spawn_count().await, 1);
    let config = codex_client.last_spawn().await.expect("codex spawn config");
    assert_eq!(config.harness, Some(AgentHarnessKind::Codex));
    assert_eq!(
        config.agent.as_deref(),
        Some("ralphx:ralphx-execution-worker")
    );
    assert_eq!(config.model.as_deref(), Some("gpt-5.4"));
}

#[tokio::test]
async fn test_spawn_uses_reexecutor_lane_for_reexecuting_task() {
    let default_client = Arc::new(TestAgentClient::new(ClientType::ClaudeCode, true));
    let codex_client = Arc::new(TestAgentClient::new(ClientType::Codex, true));
    let exec_state = Arc::new(ExecutionState::with_max_concurrent(5));
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let settings_repo = Arc::new(MemoryExecutionSettingsRepository::new());
    let ideation_session_repo = Arc::new(MemoryIdeationSessionRepository::new());
    let agent_lane_settings_repo = Arc::new(MemoryAgentLaneSettingsRepository::new());
    let running_agent_registry = Arc::new(MemoryRunningAgentRegistry::new());

    let project_id = ProjectId::from_string("project-reexecute".to_string());
    let mut project = Project::new(
        "Project Reexecute".to_string(),
        "/tmp/project-reexecute".to_string(),
    );
    project.id = project_id.clone();
    project_repo.create(project).await.unwrap();

    let mut task = Task::new(project_id.clone(), "Reexecution task".to_string());
    task.id = TaskId::from_string("task-reexecute".to_string());
    task.internal_status = crate::domain::entities::InternalStatus::ReExecuting;
    task.worktree_path = Some("/tmp/task-reexecute".to_string());
    task_repo.create(task).await.unwrap();

    let mut lane_settings = AgentLaneSettings::new(AgentHarnessKind::Codex);
    lane_settings.model = Some("gpt-5.4-mini".to_string());
    lane_settings.effort = Some(crate::domain::agents::LogicalEffort::Medium);
    lane_settings.approval_policy = Some("on-request".to_string());
    lane_settings.sandbox_mode = Some("read-only".to_string());
    agent_lane_settings_repo
        .upsert_for_project(
            project_id.as_str(),
            AgentLane::ExecutionReexecutor,
            &lane_settings,
        )
        .await
        .unwrap();

    let spawner = AgenticClientSpawner::new(default_client.clone())
        .with_harness_client(AgentHarnessKind::Codex, codex_client.clone())
        .with_repos(task_repo, project_repo)
        .with_execution_state(exec_state)
        .with_runtime_admission_context(
            settings_repo,
            agent_lane_settings_repo,
            ideation_session_repo,
            running_agent_registry,
        )
        .with_working_dir("/tmp");

    spawner.spawn("worker", "task-reexecute").await;

    assert_eq!(default_client.spawn_count().await, 0);
    assert_eq!(codex_client.spawn_count().await, 1);
    let config = codex_client.last_spawn().await.expect("codex spawn config");
    assert_eq!(config.harness, Some(AgentHarnessKind::Codex));
    assert_eq!(
        config.agent.as_deref(),
        Some("ralphx:ralphx-execution-worker")
    );
    assert_eq!(config.model.as_deref(), Some("gpt-5.4-mini"));
    assert_eq!(
        config.logical_effort,
        Some(crate::domain::agents::LogicalEffort::Medium)
    );
    assert_eq!(
        config.approval_policy.as_deref(),
        Some(CODEX_DEFAULT_APPROVAL_POLICY)
    );
    assert_eq!(
        config.sandbox_mode.as_deref(),
        Some(CODEX_DEFAULT_SANDBOX_MODE)
    );
}

#[tokio::test]
async fn test_spawn_uses_reviewer_lane_when_review_task_resolves_to_codex() {
    let (_runtime_dir, plugin_dir, generated_dir) = make_runtime_plugin_layout();
    let _runtime_plugin_dir_guard =
        crate::infrastructure::agents::claude::override_runtime_plugin_dirs_for_tests(
            plugin_dir,
            generated_dir.clone(),
        );
    let default_client = Arc::new(TestAgentClient::new(ClientType::ClaudeCode, true));
    let codex_client = Arc::new(TestAgentClient::new(ClientType::Codex, true));
    let exec_state = Arc::new(ExecutionState::with_max_concurrent(5));
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let settings_repo = Arc::new(MemoryExecutionSettingsRepository::new());
    let ideation_session_repo = Arc::new(MemoryIdeationSessionRepository::new());
    let agent_lane_settings_repo = Arc::new(MemoryAgentLaneSettingsRepository::new());
    let running_agent_registry = Arc::new(MemoryRunningAgentRegistry::new());
    let (_dir, task_workdir) = make_local_plugin_task_workdir("task-review-codex");

    let project_id = ProjectId::from_string("project-review-codex".to_string());
    let mut project = Project::new(
        "Project Review Codex".to_string(),
        "/tmp/project-review-codex".to_string(),
    );
    project.id = project_id.clone();
    project_repo.create(project).await.unwrap();

    let mut task = Task::new(project_id.clone(), "Codex reviewer task".to_string());
    task.id = TaskId::from_string("task-review-codex".to_string());
    task.internal_status = crate::domain::entities::InternalStatus::Reviewing;
    task.worktree_path = Some(task_workdir.to_string_lossy().into_owned());
    task_repo.create(task).await.unwrap();

    let mut lane_settings = AgentLaneSettings::new(AgentHarnessKind::Codex);
    lane_settings.model = Some("gpt-5.4".to_string());
    lane_settings.effort = Some(crate::domain::agents::LogicalEffort::High);
    agent_lane_settings_repo
        .upsert_for_project(
            project_id.as_str(),
            AgentLane::ExecutionReviewer,
            &lane_settings,
        )
        .await
        .unwrap();

    let spawner = AgenticClientSpawner::new(default_client.clone())
        .with_harness_client(AgentHarnessKind::Codex, codex_client.clone())
        .with_repos(task_repo, project_repo)
        .with_execution_state(exec_state)
        .with_runtime_admission_context(
            settings_repo,
            agent_lane_settings_repo,
            ideation_session_repo,
            running_agent_registry,
        )
        .with_working_dir("/tmp");

    spawner.spawn("reviewer", "task-review-codex").await;

    assert_eq!(default_client.spawn_count().await, 0);
    assert_eq!(codex_client.spawn_count().await, 1);
    let config = codex_client
        .last_spawn()
        .await
        .expect("codex review spawn config");
    assert_eq!(config.harness, Some(AgentHarnessKind::Codex));
    assert_eq!(config.agent.as_deref(), Some("ralphx:ralphx-execution-reviewer"));
    assert_eq!(config.model.as_deref(), Some("gpt-5.4"));
    assert_eq!(config.plugin_dir, Some(generated_dir));
}

#[tokio::test]
async fn test_spawn_uses_merger_lane_when_merge_task_resolves_to_codex() {
    let (_runtime_dir, plugin_dir, generated_dir) = make_runtime_plugin_layout();
    let _runtime_plugin_dir_guard =
        crate::infrastructure::agents::claude::override_runtime_plugin_dirs_for_tests(
            plugin_dir,
            generated_dir.clone(),
        );
    let default_client = Arc::new(TestAgentClient::new(ClientType::ClaudeCode, true));
    let codex_client = Arc::new(TestAgentClient::new(ClientType::Codex, true));
    let exec_state = Arc::new(ExecutionState::with_max_concurrent(5));
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let settings_repo = Arc::new(MemoryExecutionSettingsRepository::new());
    let ideation_session_repo = Arc::new(MemoryIdeationSessionRepository::new());
    let agent_lane_settings_repo = Arc::new(MemoryAgentLaneSettingsRepository::new());
    let running_agent_registry = Arc::new(MemoryRunningAgentRegistry::new());
    let (_dir, task_workdir) = make_local_plugin_task_workdir("task-merge-codex");

    let project_id = ProjectId::from_string("project-merge-codex".to_string());
    let mut project = Project::new(
        "Project Merge Codex".to_string(),
        "/tmp/project-merge-codex".to_string(),
    );
    project.id = project_id.clone();
    project_repo.create(project).await.unwrap();

    let mut task = Task::new(project_id.clone(), "Codex merger task".to_string());
    task.id = TaskId::from_string("task-merge-codex".to_string());
    task.internal_status = crate::domain::entities::InternalStatus::Merging;
    task.worktree_path = Some(task_workdir.to_string_lossy().into_owned());
    task_repo.create(task).await.unwrap();

    let mut lane_settings = AgentLaneSettings::new(AgentHarnessKind::Codex);
    lane_settings.model = Some("gpt-5.4".to_string());
    lane_settings.effort = Some(crate::domain::agents::LogicalEffort::Medium);
    agent_lane_settings_repo
        .upsert_for_project(
            project_id.as_str(),
            AgentLane::ExecutionMerger,
            &lane_settings,
        )
        .await
        .unwrap();

    let spawner = AgenticClientSpawner::new(default_client.clone())
        .with_harness_client(AgentHarnessKind::Codex, codex_client.clone())
        .with_repos(task_repo, project_repo)
        .with_execution_state(exec_state)
        .with_runtime_admission_context(
            settings_repo,
            agent_lane_settings_repo,
            ideation_session_repo,
            running_agent_registry,
        )
        .with_working_dir("/tmp");

    spawner.spawn("merger", "task-merge-codex").await;

    assert_eq!(default_client.spawn_count().await, 0);
    assert_eq!(codex_client.spawn_count().await, 1);
    let config = codex_client
        .last_spawn()
        .await
        .expect("codex merge spawn config");
    assert_eq!(config.harness, Some(AgentHarnessKind::Codex));
    assert_eq!(config.agent.as_deref(), Some("ralphx:ralphx-execution-merger"));
    assert_eq!(config.model.as_deref(), Some("gpt-5.4"));
    assert_eq!(config.plugin_dir, Some(generated_dir));
}

#[tokio::test]
async fn test_spawn_does_not_fall_back_when_requested_harness_is_unavailable() {
    let default_client = Arc::new(TestAgentClient::new(ClientType::Codex, true));
    let unavailable_claude_client = Arc::new(TestAgentClient::new(ClientType::ClaudeCode, false));
    let exec_state = Arc::new(ExecutionState::with_max_concurrent(5));
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());

    let project_id = ProjectId::from_string("project-default-harness".to_string());
    let mut project = Project::new(
        "Project Default Harness".to_string(),
        "/tmp/project-default-harness".to_string(),
    );
    project.id = project_id.clone();
    project_repo.create(project).await.unwrap();

    let mut task = Task::new(project_id, "Default harness fallback task".to_string());
    task.id = TaskId::from_string("task-default-harness".to_string());
    task.worktree_path = Some("/tmp/task-default-harness".to_string());
    task_repo.create(task).await.unwrap();

    let spawner = AgenticClientSpawner::new(default_client.clone())
        .with_default_harness(AgentHarnessKind::Codex)
        .with_harness_client(AgentHarnessKind::Claude, unavailable_claude_client.clone())
        .with_repos(task_repo, project_repo)
        .with_execution_state(exec_state)
        .with_working_dir("/tmp");

    spawner.spawn("worker", "task-default-harness").await;

    assert_eq!(default_client.spawn_count().await, 0);
    assert_eq!(unavailable_claude_client.spawn_count().await, 0);
}

#[test]
fn test_with_agent_provider_settings_repo_attaches_repo() {
    let mock = Arc::new(MockAgenticClient::new());
    let provider_repo: Arc<dyn AgentProviderSettingsRepository> =
        Arc::new(MemoryAgentProviderSettingsRepository::new());

    let spawner = AgenticClientSpawner::new(mock).with_agent_provider_settings_repo(provider_repo);

    assert!(spawner.agent_provider_settings_repo.is_some());
}

#[tokio::test]
async fn test_custom_agent_spawn_requires_enabled_default_provider() {
    let mock = Arc::new(MockAgenticClient::new());
    let provider_repo: Arc<dyn AgentProviderSettingsRepository> =
        Arc::new(MemoryAgentProviderSettingsRepository::new());
    let spawner = AgenticClientSpawner::new(mock).with_agent_provider_settings_repo(provider_repo);

    let error = spawner
        .resolve_spawn_harness("custom-agent", "task-custom", None)
        .await
        .expect_err("custom agent should be blocked without provider setup");

    assert!(error.contains("Settings > Harness > Providers"));
}

#[tokio::test]
async fn test_resolved_lane_spawn_requires_requested_provider_enabled() {
    let default_client = Arc::new(TestAgentClient::new(ClientType::ClaudeCode, true));
    let codex_client = Arc::new(TestAgentClient::new(ClientType::Codex, true));
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let settings_repo = Arc::new(MemoryExecutionSettingsRepository::new());
    let ideation_session_repo = Arc::new(MemoryIdeationSessionRepository::new());
    let running_agent_registry = Arc::new(MemoryRunningAgentRegistry::new());
    let agent_lane_settings_repo = Arc::new(MemoryAgentLaneSettingsRepository::new());
    let provider_repo = Arc::new(MemoryAgentProviderSettingsRepository::new());

    let mut claude = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Claude);
    claude.enabled = true;
    claude.is_default = true;
    provider_repo.upsert(&claude).await.unwrap();

    let project_id = ProjectId::from_string("project-provider-gate".to_string());
    let mut project = Project::new(
        "Project Provider Gate".to_string(),
        "/tmp/project-provider-gate".to_string(),
    );
    project.id = project_id.clone();
    project_repo.create(project).await.unwrap();

    let mut task = Task::new(project_id.clone(), "Codex disabled lane task".to_string());
    task.id = TaskId::from_string("task-provider-gate".to_string());
    task.worktree_path = Some("/tmp/task-provider-gate".to_string());
    task_repo.create(task).await.unwrap();

    agent_lane_settings_repo
        .upsert_for_project(
            project_id.as_str(),
            AgentLane::ExecutionWorker,
            &AgentLaneSettings::new(AgentHarnessKind::Codex),
        )
        .await
        .unwrap();

    let provider_repo: Arc<dyn AgentProviderSettingsRepository> = provider_repo;
    let spawner = AgenticClientSpawner::new(default_client)
        .with_harness_client(AgentHarnessKind::Codex, codex_client)
        .with_repos(task_repo, project_repo)
        .with_runtime_admission_context(
            settings_repo,
            agent_lane_settings_repo,
            ideation_session_repo,
            running_agent_registry,
        )
        .with_agent_provider_settings_repo(provider_repo)
        .with_working_dir("/tmp");

    let error = spawner
        .resolve_spawn_harness("worker", "task-provider-gate", Some(project_id.as_str()))
        .await
        .expect_err("disabled codex provider should block execution lane spawn");

    assert!(error.contains("codex is not enabled"));
}
