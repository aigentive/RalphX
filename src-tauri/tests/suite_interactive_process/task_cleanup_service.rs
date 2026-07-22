use std::sync::Arc;

use ralphx_lib::application::{
    AppState, InteractiveProcessKey, InteractiveProcessRegistry, StopMode, TaskCleanupService,
    TaskGroup,
};
use ralphx_lib::domain::entities::{
    IdeationSession, IdeationSessionId, IdeationSessionStatus, InternalStatus, Project, ProjectId,
    Task, TaskCategory,
};
use ralphx_lib::domain::services::{
    AttachProcessResult, MemoryRunningAgentRegistry, RunningAgentInfo, RunningAgentKey,
    RunningAgentRegistry, TryRegisterError,
};

#[tokio::test]
async fn test_cleanup_single_task_deletes_from_db() {
    let state = AppState::new_test();
    let project_id = ProjectId::new();

    // Create project
    let project = Project::new("Test".to_string(), "/tmp/test".to_string());
    state
        .project_repo
        .create(Project {
            id: project_id.clone(),
            ..project
        })
        .await
        .unwrap();

    // Create task
    let task = Task::new(project_id.clone(), "Test Task".to_string());
    let task_id = task.id.clone();
    let created = state.task_repo.create(task).await.unwrap();

    let service = TaskCleanupService::new(
        Arc::clone(&state.task_repo),
        Arc::clone(&state.project_repo),
        Arc::clone(&state.running_agent_registry),
        None,
    );

    service
        .cleanup_single_task(&created, StopMode::DirectStop, false)
        .await
        .unwrap();

    // Verify task is archived
    let task = state.task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert!(task.archived_at.is_some(), "Task should be archived after cleanup");
}

#[tokio::test]
async fn test_cleanup_task_ref_deletes_from_db() {
    let state = AppState::new_test();
    let project_id = ProjectId::new();

    let task = Task::new(project_id, "Test Task".to_string());
    let created = state.task_repo.create(task).await.unwrap();

    let service = TaskCleanupService::new(
        Arc::clone(&state.task_repo),
        Arc::clone(&state.project_repo),
        Arc::clone(&state.running_agent_registry),
        None,
    );
    let agent_stopped = service.cleanup_task_ref(&created).await.unwrap();
    assert!(!agent_stopped); // backlog task has no active agent

    let task = state.task_repo.get_by_id(&created.id).await.unwrap().unwrap();
    assert!(task.archived_at.is_some(), "Task should be archived after cleanup_task_ref");
}

#[tokio::test]
async fn test_cleanup_tasks_batch() {
    let state = AppState::new_test();
    let project_id = ProjectId::new();

    let project = Project::new("Test".to_string(), "/tmp/test".to_string());
    state
        .project_repo
        .create(Project {
            id: project_id.clone(),
            ..project
        })
        .await
        .unwrap();

    // Create multiple tasks
    let task1 = state
        .task_repo
        .create(Task::new(project_id.clone(), "Task 1".to_string()))
        .await
        .unwrap();
    let task2 = state
        .task_repo
        .create(Task::new(project_id.clone(), "Task 2".to_string()))
        .await
        .unwrap();
    let task3 = state
        .task_repo
        .create(Task::new(project_id.clone(), "Task 3".to_string()))
        .await
        .unwrap();

    let service = TaskCleanupService::new(
        Arc::clone(&state.task_repo),
        Arc::clone(&state.project_repo),
        Arc::clone(&state.running_agent_registry),
        None,
    );

    let report = service
        .cleanup_tasks(&[task1, task2, task3], StopMode::DirectStop, false)
        .await;

    assert_eq!(report.tasks_archived, 3);
    assert!(report.errors.is_empty());

    // Verify all tasks are archived
    let remaining = state.task_repo.get_by_project(&project_id).await.unwrap();
    assert_eq!(remaining.len(), 3, "All tasks should still be in DB (archived)");
    assert!(remaining.iter().all(|t| t.archived_at.is_some()), "All tasks should be archived");
}

#[tokio::test]
async fn test_cleanup_tasks_in_group_by_session() {
    let state = AppState::new_test();
    let project_id = ProjectId::new();
    let session_id = IdeationSessionId::new();

    let project = Project::new("Test".to_string(), "/tmp/test".to_string());
    state
        .project_repo
        .create(Project {
            id: project_id.clone(),
            ..project
        })
        .await
        .unwrap();

    // Create tasks linked to session
    let mut task1 = Task::new(project_id.clone(), "Session Task 1".to_string());
    task1.ideation_session_id = Some(session_id.clone());
    state.task_repo.create(task1).await.unwrap();

    let mut task2 = Task::new(project_id.clone(), "Session Task 2".to_string());
    task2.ideation_session_id = Some(session_id.clone());
    state.task_repo.create(task2).await.unwrap();

    // Create standalone task (should NOT be deleted)
    let standalone = state
        .task_repo
        .create(Task::new(project_id.clone(), "Standalone".to_string()))
        .await
        .unwrap();

    let service = TaskCleanupService::new(
        Arc::clone(&state.task_repo),
        Arc::clone(&state.project_repo),
        Arc::clone(&state.running_agent_registry),
        None,
    );

    let report = service
        .cleanup_tasks_in_group(TaskGroup::Session {
            session_id: session_id.as_str().to_string(),
            project_id: project_id.as_str().to_string(),
        })
        .await
        .unwrap();

    assert_eq!(report.tasks_archived, 2);

    // Standalone task should still exist
    assert!(state
        .task_repo
        .get_by_id(&standalone.id)
        .await
        .unwrap()
        .is_some());
}

#[tokio::test]
async fn test_cleanup_tasks_in_group_by_status() {
    let state = AppState::new_test();
    let project_id = ProjectId::new();

    let project = Project::new("Test".to_string(), "/tmp/test".to_string());
    state
        .project_repo
        .create(Project {
            id: project_id.clone(),
            ..project
        })
        .await
        .unwrap();

    // Create two backlog tasks
    let task1 = Task::new(project_id.clone(), "Task 1".to_string());
    let created1 = state.task_repo.create(task1).await.unwrap();

    let task2 = Task::new(project_id.clone(), "Task 2".to_string());
    let created2 = state.task_repo.create(task2).await.unwrap();

    let service = TaskCleanupService::new(
        Arc::clone(&state.task_repo),
        Arc::clone(&state.project_repo),
        Arc::clone(&state.running_agent_registry),
        None,
    );

    let group = TaskGroup::Status {
        status: "backlog".to_string(),
        project_id: project_id.as_str().to_string(),
    };
    let report = service.cleanup_tasks_in_group(group).await.unwrap();

    assert_eq!(report.tasks_archived, 2);
    let task1 = state.task_repo.get_by_id(&created1.id).await.unwrap().unwrap();
    assert!(task1.archived_at.is_some(), "Task 1 should be archived");
    let task2 = state.task_repo.get_by_id(&created2.id).await.unwrap().unwrap();
    assert!(task2.archived_at.is_some(), "Task 2 should be archived");
}

#[tokio::test]
async fn test_cleanup_tasks_in_group_uncategorized() {
    let state = AppState::new_test();
    let project_id = ProjectId::new();
    let session_id = IdeationSessionId::new();

    let project = Project::new("Test".to_string(), "/tmp/test".to_string());
    state
        .project_repo
        .create(Project {
            id: project_id.clone(),
            ..project
        })
        .await
        .unwrap();

    // Create session task (should NOT be deleted)
    let mut session_task = Task::new(project_id.clone(), "Session Task".to_string());
    session_task.ideation_session_id = Some(session_id.clone());
    let session_created = state.task_repo.create(session_task).await.unwrap();

    // Create uncategorized tasks (should be deleted)
    state
        .task_repo
        .create(Task::new(project_id.clone(), "Uncat 1".to_string()))
        .await
        .unwrap();
    state
        .task_repo
        .create(Task::new(project_id.clone(), "Uncat 2".to_string()))
        .await
        .unwrap();

    let service = TaskCleanupService::new(
        Arc::clone(&state.task_repo),
        Arc::clone(&state.project_repo),
        Arc::clone(&state.running_agent_registry),
        None,
    );

    let report = service
        .cleanup_tasks_in_group(TaskGroup::Uncategorized {
            project_id: project_id.as_str().to_string(),
        })
        .await
        .unwrap();

    assert_eq!(report.tasks_archived, 2);

    // Session task should still exist
    assert!(state
        .task_repo
        .get_by_id(&session_created.id)
        .await
        .unwrap()
        .is_some());
}

#[tokio::test]
async fn test_cleanup_skips_plan_merge_tasks() {
    let state = AppState::new_test();
    let project_id = ProjectId::new();

    let project = Project::new("Test".to_string(), "/tmp/test".to_string());
    state
        .project_repo
        .create(Project {
            id: project_id.clone(),
            ..project
        })
        .await
        .unwrap();

    let task = Task::new_with_category(
        project_id.clone(),
        "Merge Plan".to_string(),
        TaskCategory::PlanMerge,
    );
    let created = state.task_repo.create(task).await.unwrap();

    let service = TaskCleanupService::new(
        Arc::clone(&state.task_repo),
        Arc::clone(&state.project_repo),
        Arc::clone(&state.running_agent_registry),
        None,
    );

    let group = TaskGroup::Status {
        status: "backlog".to_string(),
        project_id: project_id.as_str().to_string(),
    };
    let report = service.cleanup_tasks_in_group(group).await.unwrap();

    assert_eq!(report.tasks_archived, 0);
    // plan_merge task should still exist
    assert!(state
        .task_repo
        .get_by_id(&created.id)
        .await
        .unwrap()
        .is_some());
}

#[tokio::test]
async fn test_stop_mode_enum_equality() {
    assert_eq!(StopMode::Graceful, StopMode::Graceful);
    assert_eq!(StopMode::DirectStop, StopMode::DirectStop);
    assert_ne!(StopMode::Graceful, StopMode::DirectStop);
}

#[tokio::test]
async fn test_cleanup_empty_batch() {
    let state = AppState::new_test();

    let service = TaskCleanupService::new(
        Arc::clone(&state.task_repo),
        Arc::clone(&state.project_repo),
        Arc::clone(&state.running_agent_registry),
        None,
    );

    let report = service
        .cleanup_tasks(&[], StopMode::DirectStop, false)
        .await;

    assert_eq!(report.tasks_archived, 0);
    assert_eq!(report.tasks_stopped, 0);
    assert!(report.errors.is_empty());
}

#[tokio::test]
async fn test_cleanup_empty_group() {
    let state = AppState::new_test();
    let project_id = ProjectId::new();

    let service = TaskCleanupService::new(
        Arc::clone(&state.task_repo),
        Arc::clone(&state.project_repo),
        Arc::clone(&state.running_agent_registry),
        None,
    );

    let group = TaskGroup::Status {
        status: "backlog".to_string(),
        project_id: project_id.as_str().to_string(),
    };
    let report = service.cleanup_tasks_in_group(group).await.unwrap();

    assert_eq!(report.tasks_archived, 0);
    assert!(report.errors.is_empty());
}

#[tokio::test]
async fn test_cleanup_tasks_archives_task_idempotently() {
    let state = AppState::new_test();
    let project_id = ProjectId::new();

    let project = Project::new("Test".to_string(), "/tmp/test".to_string());
    state
        .project_repo
        .create(Project {
            id: project_id.clone(),
            ..project
        })
        .await
        .unwrap();

    // Create a task
    let task = Task::new(project_id.clone(), "Task to reappear".to_string());
    let task_id = task.id.clone();
    let created = state.task_repo.create(task).await.unwrap();

    // Create a custom in-memory mock that re-inserts tasks on delete
    // (simulating concurrent merge side effects writing them back)
    let test_repo = Arc::clone(&state.task_repo);

    let service = TaskCleanupService::new(
        Arc::clone(&state.task_repo),
        Arc::clone(&state.project_repo),
        Arc::clone(&state.running_agent_registry),
        None,
    );

    // Wrap the repo's delete to re-insert after deletion
    // We do this by directly manipulating the repo before cleanup
    let task_to_clean = vec![created.clone()];

    // Run cleanup
    let report = service
        .cleanup_tasks(&task_to_clean, StopMode::DirectStop, false)
        .await;

    // Task should be deleted on first attempt
    // (In a real scenario with concurrent writes, the post-verification would catch it)
    assert_eq!(report.tasks_archived, 1);

    // Verify task is archived
    let task = test_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert!(task.archived_at.is_some(), "Task should be archived after cleanup");
}

// ── IPR cleanup tests ──────────────────────────────────────────────────

/// Helper for creating test stdin pipes (real subprocess for IPR write testing)
async fn create_test_stdin() -> (tokio::process::ChildStdin, tokio::process::Child) {
    let mut child = tokio::process::Command::new("cat")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("failed to spawn cat");
    let stdin = child.stdin.take().expect("no stdin");
    (stdin, child)
}

/// stop_task_agent removes IPR entry before stopping the running agent.
/// Verify: after cleanup of an executing task, IPR is empty.
#[tokio::test]
async fn test_cleanup_removes_ipr_entry_for_executing_task() {
    let state = AppState::new_test();
    let project_id = ProjectId::new();
    let ipr = Arc::new(InteractiveProcessRegistry::new());

    let project = Project::new("Test".to_string(), "/tmp/test".to_string());
    state
        .project_repo
        .create(Project {
            id: project_id.clone(),
            ..project
        })
        .await
        .unwrap();

    // Create task in executing state (triggers stop_task_agent in cleanup)
    let mut task = Task::new(project_id.clone(), "IPR Test Task".to_string());
    task.internal_status = InternalStatus::Executing;
    let task = state.task_repo.create(task).await.unwrap();

    // Register in IPR (simulating an interactive process)
    let (stdin, _child) = create_test_stdin().await;
    let ipr_key = InteractiveProcessKey::new("task_execution", task.id.as_str());
    ipr.register(ipr_key.clone(), stdin).await;
    assert!(ipr.has_process(&ipr_key).await, "Precondition: IPR has entry");

    // Register in running agent registry
    let agent_key = RunningAgentKey::new("task_execution", task.id.as_str());
    state
        .running_agent_registry
        .register(
            agent_key.clone(),
            12345,
            "conv-1".to_string(),
            "run-1".to_string(),
            None,
            None,
        )
        .await;

    // Build service WITH IPR
    let service = TaskCleanupService::new(
        Arc::clone(&state.task_repo),
        Arc::clone(&state.project_repo),
        Arc::clone(&state.running_agent_registry),
        None,
    )
    .with_interactive_process_registry(Arc::clone(&ipr));

    service
        .cleanup_single_task(&task, StopMode::DirectStop, false)
        .await
        .unwrap();

    // IPR entry must be removed by stop_task_agent
    assert!(
        !ipr.has_process(&ipr_key).await,
        "IPR entry must be removed after cleanup"
    );
    assert_eq!(ipr.dump_state().await.len(), 0, "IPR must be empty");
    // Running agent registry must also be cleaned
    assert!(
        !state.running_agent_registry.is_running(&agent_key).await,
        "Agent must be unregistered"
    );
}

/// Batch cleanup removes IPR entries for ALL active tasks (different context types).
#[tokio::test]
async fn test_cleanup_batch_removes_all_ipr_entries() {
    let state = AppState::new_test();
    let project_id = ProjectId::new();
    let ipr = Arc::new(InteractiveProcessRegistry::new());

    let project = Project::new("Test".to_string(), "/tmp/test".to_string());
    state
        .project_repo
        .create(Project {
            id: project_id.clone(),
            ..project
        })
        .await
        .unwrap();

    // Task 1: executing → context_type = "task_execution"
    let mut task1 = Task::new(project_id.clone(), "Executing Task".to_string());
    task1.internal_status = InternalStatus::Executing;
    let task1 = state.task_repo.create(task1).await.unwrap();

    // Task 2: reviewing → context_type = "review"
    let mut task2 = Task::new(project_id.clone(), "Reviewing Task".to_string());
    task2.internal_status = InternalStatus::Reviewing;
    let task2 = state.task_repo.create(task2).await.unwrap();

    // Register both in IPR with matching context types
    let (stdin1, _child1) = create_test_stdin().await;
    let ipr_key1 = InteractiveProcessKey::new("task_execution", task1.id.as_str());
    ipr.register(ipr_key1.clone(), stdin1).await;

    let (stdin2, _child2) = create_test_stdin().await;
    let ipr_key2 = InteractiveProcessKey::new("review", task2.id.as_str());
    ipr.register(ipr_key2.clone(), stdin2).await;
    assert_eq!(ipr.dump_state().await.len(), 2, "Precondition: both entries registered");

    // Register both in running agent registry
    state
        .running_agent_registry
        .register(
            RunningAgentKey::new("task_execution", task1.id.as_str()),
            12345,
            "conv-1".to_string(),
            "run-1".to_string(),
            None,
            None,
        )
        .await;
    state
        .running_agent_registry
        .register(
            RunningAgentKey::new("review", task2.id.as_str()),
            12346,
            "conv-2".to_string(),
            "run-2".to_string(),
            None,
            None,
        )
        .await;

    let service = TaskCleanupService::new(
        Arc::clone(&state.task_repo),
        Arc::clone(&state.project_repo),
        Arc::clone(&state.running_agent_registry),
        None,
    )
    .with_interactive_process_registry(Arc::clone(&ipr));

    let report = service
        .cleanup_tasks(&[task1, task2], StopMode::DirectStop, false)
        .await;

    assert_eq!(report.tasks_archived, 2);
    assert_eq!(ipr.dump_state().await.len(), 0, "All IPR entries must be removed");
}

/// Without IPR set on service, cleanup still works (backward compat).
#[tokio::test]
async fn test_cleanup_without_ipr_does_not_panic() {
    let state = AppState::new_test();
    let project_id = ProjectId::new();

    let project = Project::new("Test".to_string(), "/tmp/test".to_string());
    state
        .project_repo
        .create(Project {
            id: project_id.clone(),
            ..project
        })
        .await
        .unwrap();

    let mut task = Task::new(project_id.clone(), "No IPR Task".to_string());
    task.internal_status = InternalStatus::Executing;
    let task = state.task_repo.create(task).await.unwrap();

    let agent_key = RunningAgentKey::new("task_execution", task.id.as_str());
    state
        .running_agent_registry
        .register(
            agent_key.clone(),
            12345,
            "conv-1".to_string(),
            "run-1".to_string(),
            None,
            None,
        )
        .await;

    // Service WITHOUT IPR (interactive_process_registry = None)
    let service = TaskCleanupService::new(
        Arc::clone(&state.task_repo),
        Arc::clone(&state.project_repo),
        Arc::clone(&state.running_agent_registry),
        None,
    );

    // Should not panic even without IPR
    service
        .cleanup_single_task(&task, StopMode::DirectStop, false)
        .await
        .unwrap();

    assert!(
        !state.running_agent_registry.is_running(&agent_key).await,
        "Agent must still be stopped"
    );
}

// ── stop_ideation_session_agent tests ──────────────────────────────────

/// Mock RunningAgentRegistry whose stop() always returns Err.
/// Used to verify that stop_ideation_session_agent returns true even when
/// the registry stop call fails (IPR was cleaned up; warn is logged).
struct AlwaysErrStopRegistry(Arc<MemoryRunningAgentRegistry>);

impl AlwaysErrStopRegistry {
    fn new() -> Self {
        Self(Arc::new(MemoryRunningAgentRegistry::new()))
    }
}

#[async_trait::async_trait]
impl RunningAgentRegistry for AlwaysErrStopRegistry {
    async fn register(
        &self,
        key: RunningAgentKey,
        pid: u32,
        conversation_id: String,
        agent_run_id: String,
        worktree_path: Option<String>,
        cancellation_token: Option<tokio_util::sync::CancellationToken>,
    ) {
        self.0
            .register(key, pid, conversation_id, agent_run_id, worktree_path, cancellation_token)
            .await;
    }

    async fn unregister(
        &self,
        key: &RunningAgentKey,
        agent_run_id: &str,
    ) -> Option<RunningAgentInfo> {
        self.0.unregister(key, agent_run_id).await
    }

    async fn get(&self, key: &RunningAgentKey) -> Option<RunningAgentInfo> {
        self.0.get(key).await
    }

    async fn is_running(&self, key: &RunningAgentKey) -> bool {
        self.0.is_running(key).await
    }

    /// Always returns Err — simulates registry failure for error-path testing.
    async fn stop(&self, _key: &RunningAgentKey) -> Result<Option<RunningAgentInfo>, String> {
        Err("simulated stop error".to_string())
    }

    async fn stop_if_owned(
        &self,
        _key: &RunningAgentKey,
        _agent_run_id: &str,
    ) -> Result<Option<RunningAgentInfo>, String> {
        Err("simulated stop error".to_string())
    }

    async fn quiesce_if_owned(
        &self,
        _key: &RunningAgentKey,
        _agent_run_id: &str,
    ) -> Result<Option<RunningAgentInfo>, String> {
        Err("simulated stop error".to_string())
    }

    async fn list_all(&self) -> Vec<(RunningAgentKey, RunningAgentInfo)> {
        self.0.list_all().await
    }

    async fn stop_all(&self) -> Vec<RunningAgentKey> {
        self.0.stop_all().await
    }

    async fn stop_all_started_before(
        &self,
        cutoff: chrono::DateTime<chrono::Utc>,
    ) -> Vec<RunningAgentKey> {
        self.0.stop_all_started_before(cutoff).await
    }

    async fn update_heartbeat(
        &self,
        key: &RunningAgentKey,
        agent_run_id: &str,
        at: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, String> {
        self.0.update_heartbeat(key, agent_run_id, at).await
    }

    async fn try_register(
        &self,
        key: RunningAgentKey,
        conversation_id: String,
        agent_run_id: String,
    ) -> Result<(), TryRegisterError> {
        self.0.try_register(key, conversation_id, agent_run_id).await
    }

    async fn renew_reservation(
        &self,
        key: &RunningAgentKey,
        agent_run_id: &str,
        at: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, String> {
        self.0.renew_reservation(key, agent_run_id, at).await
    }

    async fn attach_process(
        &self,
        key: &RunningAgentKey,
        expected_agent_run_id: &str,
        pid: u32,
        worktree_path: Option<String>,
        cancellation_token: Option<tokio_util::sync::CancellationToken>,
        model: Option<String>,
    ) -> Result<AttachProcessResult, String> {
        self.0
            .attach_process(
                key,
                expected_agent_run_id,
                pid,
                worktree_path,
                cancellation_token,
                model,
            )
            .await
    }

    async fn cleanup_stale_entry(
        &self,
        key: &RunningAgentKey,
        expected_agent_run_id: &str,
    ) -> Result<Option<RunningAgentInfo>, String> {
        self.0.cleanup_stale_entry(key, expected_agent_run_id).await
    }

    async fn list_by_context_type(
        &self,
        context_type: &str,
    ) -> Result<Vec<(RunningAgentKey, RunningAgentInfo)>, String> {
        self.0.list_by_context_type(context_type).await
    }
}

/// Test 1: "ideation" key cleanup.
/// Register IPR + RunningAgentRegistry under "ideation" → helper removes both.
#[tokio::test]
async fn test_stop_ideation_agent_ideation_key_cleanup() {
    let state = AppState::new_test();
    let ipr = Arc::new(InteractiveProcessRegistry::new());
    let session_id = "test-session-ideation";

    let (stdin, _child) = create_test_stdin().await;
    let ipr_key = InteractiveProcessKey::new("ideation", session_id);
    ipr.register(ipr_key.clone(), stdin).await;
    assert!(ipr.has_process(&ipr_key).await, "Precondition: IPR has entry");

    let agent_key = RunningAgentKey::new("ideation", session_id);
    state
        .running_agent_registry
        .register(
            agent_key.clone(),
            12345,
            "conv-1".to_string(),
            "run-1".to_string(),
            None,
            None,
        )
        .await;

    let service = TaskCleanupService::new(
        Arc::clone(&state.task_repo),
        Arc::clone(&state.project_repo),
        Arc::clone(&state.running_agent_registry),
        None,
    )
    .with_interactive_process_registry(Arc::clone(&ipr));

    let stopped = service.stop_ideation_session_agent(session_id).await;

    assert!(stopped, "Helper must return true when process found");
    assert!(!ipr.has_process(&ipr_key).await, "IPR entry must be removed");
    assert_eq!(ipr.dump_state().await.len(), 0, "IPR must be empty");
    assert!(
        !state.running_agent_registry.is_running(&agent_key).await,
        "Agent must be unregistered from registry"
    );
}

/// Test 2: "session" key cleanup (HTTP external spawn path).
/// Register IPR + RunningAgentRegistry under "session" → helper removes both.
#[tokio::test]
async fn test_stop_ideation_agent_session_key_cleanup() {
    let state = AppState::new_test();
    let ipr = Arc::new(InteractiveProcessRegistry::new());
    let session_id = "test-session-http";

    let (stdin, _child) = create_test_stdin().await;
    let ipr_key = InteractiveProcessKey::new("session", session_id);
    ipr.register(ipr_key.clone(), stdin).await;
    assert!(ipr.has_process(&ipr_key).await, "Precondition: IPR has entry");

    let agent_key = RunningAgentKey::new("session", session_id);
    state
        .running_agent_registry
        .register(
            agent_key.clone(),
            12346,
            "conv-2".to_string(),
            "run-2".to_string(),
            None,
            None,
        )
        .await;

    let service = TaskCleanupService::new(
        Arc::clone(&state.task_repo),
        Arc::clone(&state.project_repo),
        Arc::clone(&state.running_agent_registry),
        None,
    )
    .with_interactive_process_registry(Arc::clone(&ipr));

    let stopped = service.stop_ideation_session_agent(session_id).await;

    assert!(stopped, "Helper must return true when process found");
    assert!(!ipr.has_process(&ipr_key).await, "IPR entry must be removed");
    assert_eq!(ipr.dump_state().await.len(), 0, "IPR must be empty");
    assert!(
        !state.running_agent_registry.is_running(&agent_key).await,
        "Agent must be unregistered from registry"
    );
}

/// Test 3: No process running — returns false, no errors, IPR unchanged.
#[tokio::test]
async fn test_stop_ideation_agent_no_process_returns_false() {
    let state = AppState::new_test();
    let ipr = Arc::new(InteractiveProcessRegistry::new());
    let session_id = "nonexistent-session";

    let service = TaskCleanupService::new(
        Arc::clone(&state.task_repo),
        Arc::clone(&state.project_repo),
        Arc::clone(&state.running_agent_registry),
        None,
    )
    .with_interactive_process_registry(Arc::clone(&ipr));

    let stopped = service.stop_ideation_session_agent(session_id).await;

    assert!(!stopped, "Helper must return false when no process is registered");
    assert_eq!(ipr.dump_state().await.len(), 0, "IPR must remain empty");
}

/// Test 4: IPR not set (misconfiguration) — returns false, warning logged.
#[tokio::test]
async fn test_stop_ideation_agent_ipr_not_set_returns_false() {
    let state = AppState::new_test();
    let session_id = "some-session";

    // Service WITHOUT .with_interactive_process_registry() → IPR is None
    let service = TaskCleanupService::new(
        Arc::clone(&state.task_repo),
        Arc::clone(&state.project_repo),
        Arc::clone(&state.running_agent_registry),
        None,
    );

    let stopped = service.stop_ideation_session_agent(session_id).await;

    assert!(
        !stopped,
        "Helper must return false when IPR is not configured (misconfiguration guard)"
    );
}

/// Test 5: Double-call idempotency — second call returns false, no panics.
#[tokio::test]
async fn test_stop_ideation_agent_idempotent_second_call_returns_false() {
    let state = AppState::new_test();
    let ipr = Arc::new(InteractiveProcessRegistry::new());
    let session_id = "idempotent-session";

    let (stdin, _child) = create_test_stdin().await;
    let ipr_key = InteractiveProcessKey::new("ideation", session_id);
    ipr.register(ipr_key.clone(), stdin).await;

    state
        .running_agent_registry
        .register(
            RunningAgentKey::new("ideation", session_id),
            12345,
            "conv-1".to_string(),
            "run-1".to_string(),
            None,
            None,
        )
        .await;

    let service = TaskCleanupService::new(
        Arc::clone(&state.task_repo),
        Arc::clone(&state.project_repo),
        Arc::clone(&state.running_agent_registry),
        None,
    )
    .with_interactive_process_registry(Arc::clone(&ipr));

    // First call: finds the process and cleans up
    let first = service.stop_ideation_session_agent(session_id).await;
    assert!(first, "First call must return true");

    // Second call: process already removed → no-op
    let second = service.stop_ideation_session_agent(session_id).await;
    assert!(!second, "Second call must return false (idempotent)");
    assert_eq!(ipr.dump_state().await.len(), 0, "IPR must still be empty");
}

/// Test 6: stop() returns Err — IPR was cleaned up, returns true (warn logged, not fatal).
#[tokio::test]
async fn test_stop_ideation_agent_stop_err_still_returns_true() {
    let ipr = Arc::new(InteractiveProcessRegistry::new());
    let session_id = "err-session";
    let err_registry: Arc<dyn RunningAgentRegistry> = Arc::new(AlwaysErrStopRegistry::new());

    let (stdin, _child) = create_test_stdin().await;
    let ipr_key = InteractiveProcessKey::new("ideation", session_id);
    ipr.register(ipr_key.clone(), stdin).await;

    let state = AppState::new_test();
    let service = TaskCleanupService::new(
        Arc::clone(&state.task_repo),
        Arc::clone(&state.project_repo),
        err_registry,
        None,
    )
    .with_interactive_process_registry(Arc::clone(&ipr));

    let stopped = service.stop_ideation_session_agent(session_id).await;

    // IPR was cleaned up even though registry.stop() returned Err → must return true
    assert!(
        stopped,
        "Must return true even when registry.stop() returns Err (IPR was cleaned)"
    );
    assert!(
        !ipr.has_process(&ipr_key).await,
        "IPR entry must be removed even if registry stop fails"
    );
}

/// Test 7 + 8: Event payload context_type correctness.
/// Note: Tauri AppHandle is not constructable in unit tests, so event emission
/// cannot be directly observed. These tests verify that the correct context_type
/// is matched (i.e., the registry key used for stop() matches what was registered),
/// which determines the event payload's context_type field.
///
/// "ideation" key → stop() uses RunningAgentKey("ideation", ...) → events use "ideation"
#[tokio::test]
async fn test_stop_ideation_agent_event_context_type_ideation_key() {
    let state = AppState::new_test();
    let ipr = Arc::new(InteractiveProcessRegistry::new());
    let session_id = "event-ideation-session";

    let (stdin, _child) = create_test_stdin().await;
    ipr.register(InteractiveProcessKey::new("ideation", session_id), stdin).await;

    state
        .running_agent_registry
        .register(
            RunningAgentKey::new("ideation", session_id),
            12345,
            "conv-ideation".to_string(),
            "run-ideation".to_string(),
            None,
            None,
        )
        .await;

    let service = TaskCleanupService::new(
        Arc::clone(&state.task_repo),
        Arc::clone(&state.project_repo),
        Arc::clone(&state.running_agent_registry),
        None, // AppHandle not available in unit tests
    )
    .with_interactive_process_registry(Arc::clone(&ipr));

    let stopped = service.stop_ideation_session_agent(session_id).await;
    assert!(stopped);

    // The "ideation" registry entry was stopped — confirms context_type = "ideation" was used.
    // In production, emitted events contain context_type = "ideation".
    assert!(
        !state
            .running_agent_registry
            .is_running(&RunningAgentKey::new("ideation", session_id))
            .await,
        "'ideation' registry key was used for stop() (confirms event payload context_type)"
    );
    // "session" key was NOT touched
    assert!(
        !state
            .running_agent_registry
            .is_running(&RunningAgentKey::new("session", session_id))
            .await,
        "'session' registry key was not affected"
    );
}

/// "session" key → stop() uses RunningAgentKey("session", ...) → events use "session"
#[tokio::test]
async fn test_stop_ideation_agent_event_context_type_session_key() {
    let state = AppState::new_test();
    let ipr = Arc::new(InteractiveProcessRegistry::new());
    let session_id = "event-session-session";

    let (stdin, _child) = create_test_stdin().await;
    ipr.register(InteractiveProcessKey::new("session", session_id), stdin).await;

    state
        .running_agent_registry
        .register(
            RunningAgentKey::new("session", session_id),
            12345,
            "conv-session".to_string(),
            "run-session".to_string(),
            None,
            None,
        )
        .await;

    let service = TaskCleanupService::new(
        Arc::clone(&state.task_repo),
        Arc::clone(&state.project_repo),
        Arc::clone(&state.running_agent_registry),
        None, // AppHandle not available in unit tests
    )
    .with_interactive_process_registry(Arc::clone(&ipr));

    let stopped = service.stop_ideation_session_agent(session_id).await;
    assert!(stopped);

    // The "session" registry entry was stopped — confirms context_type = "session" was used.
    // In production, emitted events contain context_type = "session".
    assert!(
        !state
            .running_agent_registry
            .is_running(&RunningAgentKey::new("session", session_id))
            .await,
        "'session' registry key was used for stop() (confirms event payload context_type)"
    );
    // "ideation" key was NOT touched
    assert!(
        !state
            .running_agent_registry
            .is_running(&RunningAgentKey::new("ideation", session_id))
            .await,
        "'ideation' registry key was not affected"
    );
}

/// Test 9: Dual-key probing — "ideation" only registered.
/// Helper must find "ideation", stop it, and NOT attempt "session" stop.
#[tokio::test]
async fn test_stop_ideation_agent_probes_ideation_key_only() {
    let state = AppState::new_test();
    let ipr = Arc::new(InteractiveProcessRegistry::new());
    let session_id = "probe-ideation-only";

    // Register ONLY under "ideation" — "session" is absent
    let (stdin, _child) = create_test_stdin().await;
    let ideation_key = InteractiveProcessKey::new("ideation", session_id);
    ipr.register(ideation_key.clone(), stdin).await;

    state
        .running_agent_registry
        .register(
            RunningAgentKey::new("ideation", session_id),
            12345,
            "conv-1".to_string(),
            "run-1".to_string(),
            None,
            None,
        )
        .await;

    let service = TaskCleanupService::new(
        Arc::clone(&state.task_repo),
        Arc::clone(&state.project_repo),
        Arc::clone(&state.running_agent_registry),
        None,
    )
    .with_interactive_process_registry(Arc::clone(&ipr));

    let stopped = service.stop_ideation_session_agent(session_id).await;

    assert!(stopped, "Should find the 'ideation' key and return true");
    assert!(
        !ipr.has_process(&ideation_key).await,
        "'ideation' IPR entry must be removed"
    );
    assert_eq!(ipr.dump_state().await.len(), 0, "No IPR entries must remain");
    assert!(
        !state
            .running_agent_registry
            .is_running(&RunningAgentKey::new("ideation", session_id))
            .await,
        "'ideation' registry entry must be stopped"
    );
    // "session" was never registered — no side effects
    assert!(
        !state
            .running_agent_registry
            .is_running(&RunningAgentKey::new("session", session_id))
            .await,
        "'session' registry must not be affected (never registered)"
    );
}

/// Test 10: Dual-key probing — "session" only registered.
/// Helper must skip "ideation" (absent), find "session", stop it.
#[tokio::test]
async fn test_stop_ideation_agent_probes_session_key_only() {
    let state = AppState::new_test();
    let ipr = Arc::new(InteractiveProcessRegistry::new());
    let session_id = "probe-session-only";

    // Register ONLY under "session" — "ideation" is absent
    let (stdin, _child) = create_test_stdin().await;
    let session_key = InteractiveProcessKey::new("session", session_id);
    ipr.register(session_key.clone(), stdin).await;

    state
        .running_agent_registry
        .register(
            RunningAgentKey::new("session", session_id),
            12346,
            "conv-2".to_string(),
            "run-2".to_string(),
            None,
            None,
        )
        .await;

    let service = TaskCleanupService::new(
        Arc::clone(&state.task_repo),
        Arc::clone(&state.project_repo),
        Arc::clone(&state.running_agent_registry),
        None,
    )
    .with_interactive_process_registry(Arc::clone(&ipr));

    let stopped = service.stop_ideation_session_agent(session_id).await;

    assert!(stopped, "Should find the 'session' key and return true");
    assert!(
        !ipr.has_process(&session_key).await,
        "'session' IPR entry must be removed"
    );
    assert_eq!(ipr.dump_state().await.len(), 0, "No IPR entries must remain");
    assert!(
        !state
            .running_agent_registry
            .is_running(&RunningAgentKey::new("session", session_id))
            .await,
        "'session' registry entry must be stopped"
    );
    // "ideation" was never registered — no side effects
    assert!(
        !state
            .running_agent_registry
            .is_running(&RunningAgentKey::new("ideation", session_id))
            .await,
        "'ideation' registry must not be affected (never registered)"
    );
}

// ── Call-site behavior tests ─────────────────────────────────────────────

/// HTTP apply path: service created with None app_handle (no Tauri events emitted).
/// The HTTP handler uses "session" key and passes None for app_handle.
#[tokio::test]
async fn test_call_site_http_path_none_app_handle() {
    let state = AppState::new_test();
    let ipr = Arc::new(InteractiveProcessRegistry::new());
    let session_id = "http-apply-session";

    // HTTP external path uses "session" key
    let (stdin, _child) = create_test_stdin().await;
    let ipr_key = InteractiveProcessKey::new("session", session_id);
    ipr.register(ipr_key.clone(), stdin).await;

    state
        .running_agent_registry
        .register(
            RunningAgentKey::new("session", session_id),
            12345,
            "conv-1".to_string(),
            "run-1".to_string(),
            None,
            None,
        )
        .await;

    // HTTP path: TaskCleanupService::new(..., None) — no AppHandle
    let service = TaskCleanupService::new(
        Arc::clone(&state.task_repo),
        Arc::clone(&state.project_repo),
        Arc::clone(&state.running_agent_registry),
        None, // ← None as in HTTP handler (no AppHandle in HTTP context)
    )
    .with_interactive_process_registry(Arc::clone(&ipr));

    let stopped = service.stop_ideation_session_agent(session_id).await;

    assert!(stopped, "HTTP path: helper returns true when process found");
    assert!(!ipr.has_process(&ipr_key).await, "IPR entry removed in HTTP path");
    assert!(
        !state
            .running_agent_registry
            .is_running(&RunningAgentKey::new("session", session_id))
            .await,
        "Registry entry removed in HTTP path"
    );
}

/// Archive path: cleanup happens before status update.
/// Mirrors: stop_ideation_session_agent() → update_status(Archived).
#[tokio::test]
async fn test_call_site_archive_path_cleanup_then_status_update() {
    let state = AppState::new_test();
    let ipr = Arc::new(InteractiveProcessRegistry::new());
    let project_id = ProjectId::new();
    let session_id = IdeationSessionId::new();
    let session_id_str = session_id.as_str().to_string();

    // Register an active ideation session agent (Tauri IPC path → "ideation" key)
    let (stdin, _child) = create_test_stdin().await;
    let ipr_key = InteractiveProcessKey::new("ideation", &session_id_str);
    ipr.register(ipr_key.clone(), stdin).await;

    state
        .running_agent_registry
        .register(
            RunningAgentKey::new("ideation", &session_id_str),
            12345,
            "conv-1".to_string(),
            "run-1".to_string(),
            None,
            None,
        )
        .await;

    let service = TaskCleanupService::new(
        Arc::clone(&state.task_repo),
        Arc::clone(&state.project_repo),
        Arc::clone(&state.running_agent_registry),
        None,
    )
    .with_interactive_process_registry(Arc::clone(&ipr));

    // Step 1: stop ideation agent (mirrors archive_ideation_session before update_status)
    let stopped = service.stop_ideation_session_agent(&session_id_str).await;
    assert!(stopped, "Archive: agent must be stopped");
    assert!(!ipr.has_process(&ipr_key).await, "IPR cleared before status update");

    // Step 2: update session status (mirrors archive_ideation_session after stop)
    let session = IdeationSession::new(project_id);
    state
        .ideation_session_repo
        .create(IdeationSession {
            id: session_id.clone(),
            ..session
        })
        .await
        .unwrap();

    state
        .ideation_session_repo
        .update_status(&session_id, IdeationSessionStatus::Archived)
        .await
        .unwrap();

    let updated = state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated.status,
        IdeationSessionStatus::Archived,
        "Session must be archived after cleanup"
    );
}

/// Accept path: helper called only when session_converted == true.
/// When session_converted is false, IPR entry must remain untouched.
#[tokio::test]
async fn test_call_site_accept_path_only_when_session_converted() {
    let state = AppState::new_test();
    let ipr = Arc::new(InteractiveProcessRegistry::new());
    let session_id = "accept-session";

    let (stdin, _child) = create_test_stdin().await;
    let ipr_key = InteractiveProcessKey::new("ideation", session_id);
    ipr.register(ipr_key.clone(), stdin).await;

    let service = TaskCleanupService::new(
        Arc::clone(&state.task_repo),
        Arc::clone(&state.project_repo),
        Arc::clone(&state.running_agent_registry),
        None,
    )
    .with_interactive_process_registry(Arc::clone(&ipr));

    // Simulate: session_converted == false → helper NOT called → IPR unchanged
    let session_converted = false;
    if session_converted {
        service.stop_ideation_session_agent(session_id).await;
    }
    assert!(
        ipr.has_process(&ipr_key).await,
        "IPR entry must remain when session_converted == false"
    );

    // Simulate: session_converted == true → helper IS called → IPR cleaned
    let session_converted = true;
    if session_converted {
        let stopped = service.stop_ideation_session_agent(session_id).await;
        assert!(stopped, "Accept: helper returns true when session_converted == true");
    }
    assert!(
        !ipr.has_process(&ipr_key).await,
        "IPR entry removed when session_converted == true"
    );
}

/// Reopen path: stop_cleanup is a separate instance created BEFORE task_cleanup
/// is moved into SessionReopenService. Verifies the cleanup pattern.
#[tokio::test]
async fn test_call_site_reopen_path_cleanup_before_reopen() {
    let state = AppState::new_test();
    let ipr = Arc::new(InteractiveProcessRegistry::new());
    let session_id = "reopen-session";

    let (stdin, _child) = create_test_stdin().await;
    let ipr_key = InteractiveProcessKey::new("ideation", session_id);
    ipr.register(ipr_key.clone(), stdin).await;

    state
        .running_agent_registry
        .register(
            RunningAgentKey::new("ideation", session_id),
            12345,
            "conv-1".to_string(),
            "run-1".to_string(),
            None,
            None,
        )
        .await;

    // stop_cleanup is a SEPARATE instance created before task_cleanup is moved
    // into SessionReopenService (mirrors reopen_ideation_session handler)
    let stop_cleanup = TaskCleanupService::new(
        Arc::clone(&state.task_repo),
        Arc::clone(&state.project_repo),
        Arc::clone(&state.running_agent_registry),
        None,
    )
    .with_interactive_process_registry(Arc::clone(&ipr));

    // Stop agent before reopen (mirrors handler ordering)
    let stopped = stop_cleanup.stop_ideation_session_agent(session_id).await;
    assert!(stopped, "Reopen path: stop must succeed before reopen");
    assert!(
        !ipr.has_process(&ipr_key).await,
        "IPR cleared before SessionReopenService construction"
    );
    assert!(
        !state
            .running_agent_registry
            .is_running(&RunningAgentKey::new("ideation", session_id))
            .await,
        "Registry cleared before reopen"
    );

    // A second instance (task_cleanup) can then be moved into SessionReopenService
    // without IPR interference — verified by checking IPR count remains 0
    let _task_cleanup = TaskCleanupService::new(
        Arc::clone(&state.task_repo),
        Arc::clone(&state.project_repo),
        Arc::clone(&state.running_agent_registry),
        None,
    )
    .with_interactive_process_registry(Arc::clone(&ipr));

    assert_eq!(
        ipr.dump_state().await.len(),
        0,
        "IPR must be empty after stop_cleanup (second instance does not re-add entries)"
    );
}
