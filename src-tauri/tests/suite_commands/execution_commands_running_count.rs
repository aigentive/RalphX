// Regression tests for running count vs process list mismatch bug (a4f4fa6d).
//
// Bug: `get_execution_status` counted registry entries without checking task
// status, while `get_running_processes` filtered via
// `context_matches_running_status_for_gc`. A Failed task with a stale registry
// entry inflated the "Running: 2/10" count while only 1 task was visible.

use std::sync::Arc;

use ralphx_lib::application::{chat_service::uses_execution_slot, AppState};
use ralphx_lib::commands::execution_commands::context_matches_running_status_for_gc;
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::entities::{
    types::IdeationSessionId, ChatContextType, IdeationSession, IdeationSessionStatus,
    InternalStatus, Project, ProjectId, Task, TaskId,
};
use ralphx_lib::domain::repositories::{ProjectRepository, TaskRepository};
use ralphx_lib::domain::services::{RunningAgentKey, RunningAgentRegistry};
use ralphx_lib::infrastructure::memory::{MemoryProjectRepository, MemoryTaskRepository};

/// Shared helper: create an AppState with memory repos and a fresh project+task.
/// Returns (app_state, task) with the task already persisted in the given status.
async fn setup_with_task(status: InternalStatus) -> (AppState, Task) {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    let pid = project.id.clone();
    project_repo.create(project).await.unwrap();

    let mut task = Task::new(pid, "Test Task".to_string());
    task.internal_status = status;
    let task = task_repo.create(task).await.unwrap();

    let state = AppState::with_repos(task_repo, project_repo);
    (state, task)
}

/// Register a task_execution entry in the registry for the given task.
async fn register_task_execution(registry: &dyn RunningAgentRegistry, task_id: &TaskId) {
    let key = RunningAgentKey::new("task_execution", task_id.as_str());
    registry
        .register(
            key,
            9999, // fake pid
            "conv-test".to_string(),
            "run-test".to_string(),
            None,
            None,
        )
        .await;
}

/// Register an ideation entry in the registry with a session ID.
async fn register_ideation(registry: &dyn RunningAgentRegistry, session_id: &str) {
    let key = RunningAgentKey::new("ideation", session_id);
    registry
        .register(
            key,
            9999, // fake pid
            "conv-ideation".to_string(),
            "run-ideation".to_string(),
            None,
            None,
        )
        .await;
}

/// Replicate the counting logic from `get_execution_status` so we can
/// verify it without needing Tauri `State<'_>` wrappers.
async fn count_running(app_state: &AppState) -> u32 {
    let registry_entries = app_state.running_agent_registry.list_all().await;
    let mut running_count = 0u32;

    for (key, _) in registry_entries {
        let context_type = match key.context_type.parse::<ChatContextType>() {
            Ok(value) => value,
            Err(_) => continue,
        };

        if !uses_execution_slot(context_type) {
            continue;
        }

        // Ideation uses execution slots but is excluded from running_count
        // (running_count is execution-only: TaskExecution, Review, Merge).
        if matches!(context_type, ChatContextType::Ideation) {
            continue;
        }

        let task_id = TaskId::from_string(key.context_id);
        let task = match app_state.task_repo.get_by_id(&task_id).await {
            Ok(Some(task)) => task,
            _ => continue,
        };

        if task.archived_at.is_some() {
            continue;
        }

        if !context_matches_running_status_for_gc(context_type, task.internal_status) {
            continue;
        }

        running_count += 1;
    }

    running_count
}

/// Same filter used by `get_running_processes` — returns how many entries
/// survive the guard (task-based contexts only, excludes ideation).
async fn count_visible_processes(app_state: &AppState) -> usize {
    let registry_entries = app_state.running_agent_registry.list_all().await;
    let mut count = 0usize;

    for (key, _) in registry_entries {
        let context_type = match key.context_type.parse::<ChatContextType>() {
            Ok(value) => value,
            Err(_) => continue,
        };

        // Only task-based execution contexts appear in the process list
        if !matches!(
            context_type,
            ChatContextType::TaskExecution | ChatContextType::Review | ChatContextType::Merge
        ) {
            continue;
        }

        let task_id = TaskId::from_string(key.context_id);
        let task = match app_state.task_repo.get_by_id(&task_id).await {
            Ok(Some(task)) => task,
            _ => continue,
        };

        if task.archived_at.is_some() {
            continue;
        }

        if !context_matches_running_status_for_gc(context_type, task.internal_status) {
            continue;
        }

        count += 1;
    }

    count
}

// =========================================================================
// Regression: running count must exclude Failed tasks with stale registry
// =========================================================================

#[tokio::test]
async fn test_running_count_excludes_failed_task_with_stale_registry() {
    let (state, task) = setup_with_task(InternalStatus::Failed).await;
    register_task_execution(&*state.running_agent_registry, &task.id).await;

    // Registry has 1 entry, but the task is Failed → count must be 0.
    let entries = state.running_agent_registry.list_all().await;
    assert_eq!(entries.len(), 1, "registry should have 1 entry");

    let count = count_running(&state).await;
    assert_eq!(count, 0, "Failed task must not be counted as running");
}

#[tokio::test]
async fn test_running_count_excludes_archived_executing_task_with_registry() {
    let (state, task) = setup_with_task(InternalStatus::Executing).await;
    state.task_repo.archive(&task.id).await.unwrap();
    register_task_execution(&*state.running_agent_registry, &task.id).await;

    let running_count = count_running(&state).await;
    let process_count = count_visible_processes(&state).await;

    assert_eq!(running_count, 0, "archived task must not count as running");
    assert_eq!(
        process_count, 0,
        "archived task must not appear in running processes"
    );
}

// =========================================================================
// Positive: Executing task with registry entry IS counted
// =========================================================================

#[tokio::test]
async fn test_running_count_includes_executing_task() {
    let (state, task) = setup_with_task(InternalStatus::Executing).await;
    register_task_execution(&*state.running_agent_registry, &task.id).await;

    let count = count_running(&state).await;
    assert_eq!(count, 1, "Executing task must be counted as running");
}

// =========================================================================
// Negative: terminal/completed statuses must not be counted
// =========================================================================

#[tokio::test]
async fn test_running_count_excludes_completed_task() {
    // Merged is a terminal "completed" status
    let (state, task) = setup_with_task(InternalStatus::Merged).await;
    register_task_execution(&*state.running_agent_registry, &task.id).await;

    let count = count_running(&state).await;
    assert_eq!(count, 0, "Merged task must not be counted as running");
}

#[tokio::test]
async fn test_running_count_excludes_approved_task() {
    let (state, task) = setup_with_task(InternalStatus::Approved).await;
    register_task_execution(&*state.running_agent_registry, &task.id).await;

    let count = count_running(&state).await;
    assert_eq!(count, 0, "Approved task must not be counted as running");
}

// =========================================================================
// Core regression: running count MUST equal process list count
// =========================================================================

#[tokio::test]
async fn test_running_count_matches_process_list_count() {
    // Mixed scenario: 1 Executing + 1 Failed (stale registry entry)
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());

    let project = Project::new("Mixed Project".to_string(), "/test/mixed".to_string());
    let pid = project.id.clone();
    project_repo.create(project).await.unwrap();

    // Task A: genuinely executing
    let mut task_a = Task::new(pid.clone(), "Executing Task".to_string());
    task_a.internal_status = InternalStatus::Executing;
    let task_a = task_repo.create(task_a).await.unwrap();

    // Task B: failed but has stale registry entry (the bug scenario)
    let mut task_b = Task::new(pid, "Failed Task".to_string());
    task_b.internal_status = InternalStatus::Failed;
    let task_b = task_repo.create(task_b).await.unwrap();

    let state = AppState::with_repos(task_repo, project_repo);

    register_task_execution(&*state.running_agent_registry, &task_a.id).await;
    register_task_execution(&*state.running_agent_registry, &task_b.id).await;

    // Registry has 2 entries
    let entries = state.running_agent_registry.list_all().await;
    assert_eq!(entries.len(), 2, "registry should have 2 entries");

    let running_count = count_running(&state).await;
    let process_count = count_visible_processes(&state).await;

    assert_eq!(
        running_count, 1,
        "only the Executing task should be counted"
    );
    assert_eq!(
        running_count as usize, process_count,
        "running count must match visible process count"
    );
}

// =========================================================================
// Guard function unit tests: context_matches_running_status_for_gc
// =========================================================================

#[test]
fn test_guard_task_execution_only_matches_executing_or_reexecuting() {
    let ctx = ChatContextType::TaskExecution;
    assert!(context_matches_running_status_for_gc(
        ctx,
        InternalStatus::Executing
    ));
    assert!(context_matches_running_status_for_gc(
        ctx,
        InternalStatus::ReExecuting
    ));

    // Every other status must NOT match
    for status in [
        InternalStatus::Backlog,
        InternalStatus::Ready,
        InternalStatus::Failed,
        InternalStatus::Cancelled,
        InternalStatus::Merged,
        InternalStatus::Approved,
        InternalStatus::PendingReview,
        InternalStatus::Reviewing,
        InternalStatus::Merging,
        InternalStatus::Stopped,
        InternalStatus::Paused,
    ] {
        assert!(
            !context_matches_running_status_for_gc(ctx, status),
            "TaskExecution context should NOT match {status:?}"
        );
    }
}

#[test]
fn test_guard_review_only_matches_reviewing() {
    let ctx = ChatContextType::Review;
    assert!(context_matches_running_status_for_gc(
        ctx,
        InternalStatus::Reviewing
    ));
    assert!(!context_matches_running_status_for_gc(
        ctx,
        InternalStatus::PendingReview
    ));
    assert!(!context_matches_running_status_for_gc(
        ctx,
        InternalStatus::Executing
    ));
    assert!(!context_matches_running_status_for_gc(
        ctx,
        InternalStatus::Failed
    ));
}

#[test]
fn test_guard_merge_only_matches_merging() {
    let ctx = ChatContextType::Merge;
    assert!(context_matches_running_status_for_gc(
        ctx,
        InternalStatus::Merging
    ));
    assert!(!context_matches_running_status_for_gc(
        ctx,
        InternalStatus::PendingMerge
    ));
    assert!(!context_matches_running_status_for_gc(
        ctx,
        InternalStatus::Merged
    ));
    assert!(!context_matches_running_status_for_gc(
        ctx,
        InternalStatus::Executing
    ));
}

#[test]
fn test_guard_non_execution_contexts_always_false() {
    for ctx in [
        ChatContextType::Task,
        ChatContextType::Ideation,
        ChatContextType::Project,
    ] {
        assert!(
            !context_matches_running_status_for_gc(ctx, InternalStatus::Executing),
            "{ctx:?} should never match any status"
        );
    }
}

// =========================================================================
// Ideation: excluded from running_count but excluded from process list
// =========================================================================

#[tokio::test]
async fn test_ideation_entry_counted_in_running_count() {
    let state = AppState::with_repos(
        Arc::new(MemoryTaskRepository::new()),
        Arc::new(MemoryProjectRepository::new()),
    );
    register_ideation(&*state.running_agent_registry, "session-abc").await;

    let count = count_running(&state).await;
    assert_eq!(
        count, 0,
        "Ideation entry must NOT be counted in running_count (execution-only)"
    );
}

#[tokio::test]
async fn test_ideation_entry_excluded_from_process_list() {
    let state = AppState::with_repos(
        Arc::new(MemoryTaskRepository::new()),
        Arc::new(MemoryProjectRepository::new()),
    );
    register_ideation(&*state.running_agent_registry, "session-abc").await;

    let process_count = count_visible_processes(&state).await;
    assert_eq!(
        process_count, 0,
        "Ideation entry must NOT appear in process list"
    );
}

#[tokio::test]
async fn test_mixed_executing_task_and_ideation() {
    // 1 executing task + 1 ideation session → count=2, process list=1
    let (state, task) = setup_with_task(InternalStatus::Executing).await;
    register_task_execution(&*state.running_agent_registry, &task.id).await;
    register_ideation(&*state.running_agent_registry, "session-xyz").await;

    let entries = state.running_agent_registry.list_all().await;
    assert_eq!(entries.len(), 2, "registry should have 2 entries");

    let running_count = count_running(&state).await;
    let process_count = count_visible_processes(&state).await;

    assert_eq!(
        running_count, 1,
        "Only the executing task must be counted in running_count (ideation excluded)"
    );
    assert_eq!(
        process_count, 1,
        "Only the executing task should appear in process list"
    );
}

#[tokio::test]
async fn test_ideation_not_affected_by_failed_task_regression() {
    // 1 ideation + 1 failed task (stale) → count=1, process list=0
    let (state, task) = setup_with_task(InternalStatus::Failed).await;
    register_task_execution(&*state.running_agent_registry, &task.id).await;
    register_ideation(&*state.running_agent_registry, "session-live").await;

    let running_count = count_running(&state).await;
    let process_count = count_visible_processes(&state).await;

    assert_eq!(
        running_count, 0,
        "Neither ideation nor failed task must be counted in running_count"
    );
    assert_eq!(
        process_count, 0,
        "Failed task must not appear in process list"
    );
}

// =========================================================================
// uses_execution_slot: ideation IS an execution slot context
// =========================================================================

#[test]
fn test_uses_execution_slot_includes_ideation() {
    assert!(
        uses_execution_slot(ChatContextType::Ideation),
        "Ideation must use an execution slot"
    );
    assert!(uses_execution_slot(ChatContextType::TaskExecution));
    assert!(uses_execution_slot(ChatContextType::Review));
    assert!(uses_execution_slot(ChatContextType::Merge));
    assert!(
        !uses_execution_slot(ChatContextType::Task),
        "Task chat must NOT use an execution slot"
    );
    assert!(
        !uses_execution_slot(ChatContextType::Project),
        "Project chat must NOT use an execution slot"
    );
}

// =========================================================================
// Ideation admission policy
// =========================================================================

#[test]
fn test_can_start_ideation_blocks_at_global_ideation_cap() {
    let state = ExecutionState::with_max_concurrent(5);
    state.set_global_max_concurrent(5);
    state.set_global_ideation_max(2);

    state.increment_running();
    state.increment_running();

    assert!(
        !state.can_start_ideation(2, 0, 2, 5, 2, false, false),
        "ideation must stop at the ideation cap even when total capacity remains"
    );
}

#[test]
fn test_execution_capacity_remains_when_ideation_cap_is_full() {
    let state = ExecutionState::with_max_concurrent(5);
    state.set_global_max_concurrent(5);
    state.set_global_ideation_max(2);

    state.increment_running();
    state.increment_running();

    assert!(
        state.can_start_task(),
        "execution capacity should still remain when only the ideation cap is full"
    );
    assert!(
        !state.can_start_ideation(2, 0, 2, 5, 2, false, false),
        "ideation must not consume the remaining execution-reserved capacity"
    );
}

#[test]
fn test_can_start_execution_context_blocks_when_project_total_cap_is_full() {
    let state = ExecutionState::with_max_concurrent(5);
    state.set_global_max_concurrent(8);

    state.increment_running();
    state.increment_running();

    assert!(
        !state.can_start_execution_context(2, 2),
        "execution-side contexts must stop at the project total cap"
    );
}

#[test]
fn test_can_start_execution_context_allows_when_only_ideation_cap_is_full() {
    let state = ExecutionState::with_max_concurrent(5);
    state.set_global_max_concurrent(5);
    state.set_global_ideation_max(1);

    state.increment_running();

    assert!(
        state.can_start_execution_context(1, 5),
        "execution-side contexts must still use reserved capacity when ideation is capped"
    );
}

#[test]
fn test_can_start_ideation_borrowing_requires_no_waiting_execution() {
    let state = ExecutionState::with_max_concurrent(5);
    state.set_global_max_concurrent(5);
    state.set_global_ideation_max(2);
    state.set_allow_ideation_borrow_idle_execution(true);

    state.increment_running();
    state.increment_running();

    assert!(
        !state.can_start_ideation(2, 0, 2, 5, 2, true, false),
        "borrowing must stay blocked while runnable execution work is waiting"
    );
    assert!(
        state.can_start_ideation(2, 0, 2, 5, 2, false, false),
        "ideation may borrow idle execution capacity only when execution is not waiting"
    );
}

#[test]
fn test_can_start_ideation_blocks_at_project_ideation_cap_without_borrow() {
    let state = ExecutionState::with_max_concurrent(5);
    state.set_global_max_concurrent(5);
    state.set_global_ideation_max(4);

    state.increment_running();

    assert!(
        !state.can_start_ideation(1, 1, 1, 5, 1, false, false),
        "project ideation cap must block additional ideation in the same project"
    );
}

#[test]
fn test_can_start_ideation_blocks_when_project_total_cap_is_full() {
    let state = ExecutionState::with_max_concurrent(5);
    state.set_global_max_concurrent(8);
    state.set_global_ideation_max(4);

    state.increment_running();
    state.increment_running();
    state.increment_running();

    assert!(
        !state.can_start_ideation(1, 1, 3, 3, 2, false, false),
        "project total cap must block ideation even when global capacity remains"
    );
}

#[test]
fn test_can_start_ideation_borrowing_requires_no_waiting_execution_in_project() {
    let state = ExecutionState::with_max_concurrent(5);
    state.set_global_max_concurrent(5);
    state.set_global_ideation_max(4);
    state.set_allow_ideation_borrow_idle_execution(true);

    state.increment_running();

    assert!(
        !state.can_start_ideation(1, 1, 1, 5, 1, false, true),
        "project borrowing must stay blocked while that project's execution work is waiting"
    );
    assert!(
        state.can_start_ideation(1, 1, 1, 5, 1, false, false),
        "project ideation may borrow only when project execution is not waiting"
    );
}

// =========================================================================
// Phase: idle ideation sessions must not inflate global_running_count
// =========================================================================

/// Helper: replicate the FIXED `get_execution_status` global count logic.
/// Uses ExecutionState.interactive_idle_count() to subtract idle slots.
async fn count_global_running_with_state(
    app_state: &AppState,
    execution_state: &ExecutionState,
) -> u32 {
    let registry_entries = app_state.running_agent_registry.list_all().await;
    let total_with_slot = registry_entries
        .iter()
        .filter(|(key, _)| {
            key.context_type
                .parse::<ChatContextType>()
                .map(uses_execution_slot)
                .unwrap_or(false)
        })
        .count();
    (total_with_slot.saturating_sub(execution_state.interactive_idle_count())) as u32
}

/// Helper: replicate the per-project running count from `get_execution_status`,
/// excluding idle ideation sessions.
async fn count_project_running_with_state(
    app_state: &AppState,
    execution_state: &ExecutionState,
) -> u32 {
    let registry_entries = app_state.running_agent_registry.list_all().await;
    let mut running_count = 0u32;

    for (key, _) in registry_entries {
        let context_type = match key.context_type.parse::<ChatContextType>() {
            Ok(value) => value,
            Err(_) => continue,
        };

        if !uses_execution_slot(context_type) {
            continue;
        }

        if matches!(context_type, ChatContextType::Ideation) {
            let slot_key = format!("{}/{}", key.context_type, key.context_id);
            if !execution_state.is_interactive_idle(&slot_key) {
                running_count += 1;
            }
            continue;
        }

        let task_id = TaskId::from_string(key.context_id);
        let task = match app_state.task_repo.get_by_id(&task_id).await {
            Ok(Some(task)) => task,
            _ => continue,
        };

        if !context_matches_running_status_for_gc(context_type, task.internal_status) {
            continue;
        }

        running_count += 1;
    }

    running_count
}

/// Test 1: 2 ideation sessions registered → mark 1 idle → global count == 1
#[tokio::test]
async fn test_idle_ideation_excluded_from_global_running_count() {
    let state = AppState::with_repos(
        Arc::new(MemoryTaskRepository::new()),
        Arc::new(MemoryProjectRepository::new()),
    );
    let exec_state = ExecutionState::new();

    register_ideation(&*state.running_agent_registry, "session-active").await;
    register_ideation(&*state.running_agent_registry, "session-idle").await;

    // Mark second session as idle between turns
    exec_state.mark_interactive_idle("ideation/session-idle");

    let global_count = count_global_running_with_state(&state, &exec_state).await;
    assert_eq!(
        global_count, 1,
        "Idle ideation session must not inflate global running count"
    );
}

/// Test 2: GC prune cleans idle slots — after pruning a registry entry,
/// interactive_idle_count() must return 0 (no ghost entries).
#[tokio::test]
async fn test_gc_prune_clears_interactive_idle_slot() {
    let state = AppState::with_repos(
        Arc::new(MemoryTaskRepository::new()),
        Arc::new(MemoryProjectRepository::new()),
    );
    let exec_state = ExecutionState::new();

    register_ideation(&*state.running_agent_registry, "session-gone").await;
    exec_state.mark_interactive_idle("ideation/session-gone");

    assert_eq!(
        exec_state.interactive_idle_count(),
        1,
        "Should have 1 idle slot before pruning"
    );

    // Simulate what prune does: remove from registry AND remove from idle slots
    let entries = state.running_agent_registry.list_all().await;
    for (key, info) in entries {
        let _ = state
            .running_agent_registry
            .unregister(&key, &info.agent_run_id)
            .await;
        let slot_key = format!("{}/{}", key.context_type, key.context_id);
        exec_state.remove_interactive_slot(&slot_key);
    }

    assert_eq!(
        exec_state.interactive_idle_count(),
        0,
        "Ghost idle slot must be removed after GC prune"
    );
}

/// Test 3: Per-project count excludes idle ideation sessions.
#[tokio::test]
async fn test_per_project_count_excludes_idle_ideation() {
    let state = AppState::with_repos(
        Arc::new(MemoryTaskRepository::new()),
        Arc::new(MemoryProjectRepository::new()),
    );
    let exec_state = ExecutionState::new();

    register_ideation(&*state.running_agent_registry, "session-idle").await;
    exec_state.mark_interactive_idle("ideation/session-idle");

    let project_count = count_project_running_with_state(&state, &exec_state).await;
    assert_eq!(
        project_count, 0,
        "Idle ideation session must not appear in per-project running count"
    );
}

/// Test 4: Slot key format consistency — key used by streaming code matches
/// the key used by count logic. Format: "{context_type}/{context_id}".
#[test]
fn test_slot_key_format_consistency() {
    let context_type = "ideation";
    let context_id = "session-abc-123";
    let slot_key = format!("{}/{}", context_type, context_id);

    // The key format used when marking idle must match the key used when checking.
    let exec_state = ExecutionState::new();
    exec_state.mark_interactive_idle(&slot_key);

    // Check using the same format that count_global_running_with_state uses
    assert!(
        exec_state.is_interactive_idle(&slot_key),
        "Slot key format must be consistent: '{}'",
        slot_key
    );
    assert_eq!(slot_key, "ideation/session-abc-123");
}

/// Test 5: Generating (non-idle) ideation session IS counted in both global
/// and per-project running counts.
#[tokio::test]
async fn test_generating_ideation_counted_in_running_counts() {
    let state = AppState::with_repos(
        Arc::new(MemoryTaskRepository::new()),
        Arc::new(MemoryProjectRepository::new()),
    );
    let exec_state = ExecutionState::new();

    // Register ideation session — NOT marked idle, so it's actively generating
    register_ideation(&*state.running_agent_registry, "session-generating").await;

    let global_count = count_global_running_with_state(&state, &exec_state).await;
    let project_count = count_project_running_with_state(&state, &exec_state).await;

    assert_eq!(
        global_count, 1,
        "Generating ideation must count toward global running count"
    );
    assert_eq!(
        project_count, 1,
        "Generating ideation must count toward per-project running count"
    );
}

// =========================================================================
// Multi-project ideation scoping: counts must be project-scoped
// =========================================================================

/// Build a minimal IdeationSession for a given project, identified by session_id.
fn make_ideation_session(session_id: &str, project_id: &ProjectId) -> IdeationSession {
    IdeationSession {
        id: IdeationSessionId::from_string(session_id.to_string()),
        project_id: project_id.clone(),
        title: None,
        status: IdeationSessionStatus::default(),
        plan_artifact_id: None,
        plan_blueprint_artifact_id: None,
        verified_plan_artifact_id: None,
        verified_plan_blueprint_artifact_id: None,
        verified_plan_agent_run_id: None,
        inherited_plan_artifact_id: None,
        inherited_plan_blueprint_artifact_id: None,
        seed_task_id: None,
        parent_session_id: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        archived_at: None,
        converted_at: None,
        title_source: None,
        verification_status: Default::default(),
        verification_in_progress: false,
        verification_generation: 0,
        verification_current_round: None,
        verification_max_rounds: None,
        verification_gap_count: 0,
        verification_gap_score: None,
        verification_convergence_reason: None,
        source_project_id: None,
        source_session_id: None,
        source_task_id: None,
        source_context_type: None,
        source_context_id: None,
        spawn_reason: None,
        blocker_fingerprint: None,
        session_purpose: Default::default(),
        session_flow: Default::default(),
        cross_project_checked: true,
        plan_version_last_read: None,
        blueprint_version_last_read: None,
        plan_contract_version: 1,
        origin: Default::default(),
        expected_proposal_count: None,
        auto_accept_status: None,
        auto_accept_started_at: None,
        api_key_id: None,
        idempotency_key: None,
        external_activity_phase: None,
        external_last_read_message_id: None,
        dependencies_acknowledged: false,
        pending_initial_prompt: None,
        acceptance_status: None,
        verification_confirmation_status: None,
        analysis: Default::default(),
        last_effective_model: None,
    }
}

/// Replicate the UPDATED `get_execution_status` per-project loop logic:
/// session lookup + project filter for ideation, task lookup + project filter for tasks.
async fn count_scoped_running(
    app_state: &AppState,
    execution_state: &ExecutionState,
    project_id: Option<&ProjectId>,
) -> (u32, u32, u32) {
    // returns (running_count, ideation_active, ideation_idle)
    let registry_entries = app_state.running_agent_registry.list_all().await;
    let mut running_count = 0u32;
    let mut ideation_active = 0u32;
    let mut ideation_idle = 0u32;

    for (key, _) in registry_entries {
        let context_type = match key.context_type.parse::<ChatContextType>() {
            Ok(value) => value,
            Err(_) => continue,
        };

        if !uses_execution_slot(context_type) {
            continue;
        }

        if matches!(context_type, ChatContextType::Ideation) {
            let session_id = IdeationSessionId::from_string(key.context_id.clone());
            let session = match app_state.ideation_session_repo.get_by_id(&session_id).await {
                Ok(Some(s)) => s,
                _ => continue,
            };
            if let Some(pid) = project_id {
                if session.project_id != *pid {
                    continue;
                }
            }
            let slot_key = format!("{}/{}", key.context_type, key.context_id);
            if execution_state.is_interactive_idle(&slot_key) {
                ideation_idle += 1;
            } else {
                ideation_active += 1;
                // Ideation is excluded from running_count (execution-only counter).
            }
            continue;
        }

        let task_id = TaskId::from_string(key.context_id);
        let task = match app_state.task_repo.get_by_id(&task_id).await {
            Ok(Some(task)) => task,
            _ => continue,
        };

        if task.archived_at.is_some() {
            continue;
        }

        if let Some(pid) = project_id {
            if task.project_id != *pid {
                continue;
            }
        }

        if !context_matches_running_status_for_gc(context_type, task.internal_status) {
            continue;
        }

        running_count += 1;
    }

    (running_count, ideation_active, ideation_idle)
}

/// Test: ideation_active counts are scoped to the requested project.
/// Project A has 2 active sessions, project B has 1. Querying project A → 2; B → 1.
#[tokio::test]
async fn test_ideation_active_is_project_scoped() {
    let state = AppState::with_repos(
        Arc::new(MemoryTaskRepository::new()),
        Arc::new(MemoryProjectRepository::new()),
    );
    let exec_state = ExecutionState::new();

    let pid_a = ProjectId::from_string("proj-a".to_string());
    let pid_b = ProjectId::from_string("proj-b".to_string());

    // Seed sessions in the ideation_session_repo
    state
        .ideation_session_repo
        .create(make_ideation_session("session-a1", &pid_a))
        .await
        .unwrap();
    state
        .ideation_session_repo
        .create(make_ideation_session("session-a2", &pid_a))
        .await
        .unwrap();
    state
        .ideation_session_repo
        .create(make_ideation_session("session-b1", &pid_b))
        .await
        .unwrap();

    // Register all three as active (generating) in the registry
    register_ideation(&*state.running_agent_registry, "session-a1").await;
    register_ideation(&*state.running_agent_registry, "session-a2").await;
    register_ideation(&*state.running_agent_registry, "session-b1").await;

    let (rc_a, active_a, idle_a) = count_scoped_running(&state, &exec_state, Some(&pid_a)).await;
    let (rc_b, active_b, idle_b) = count_scoped_running(&state, &exec_state, Some(&pid_b)).await;

    assert_eq!(
        active_a, 2,
        "Project A must have 2 active ideation sessions"
    );
    assert_eq!(idle_a, 0, "Project A must have 0 idle ideation sessions");
    assert_eq!(
        rc_a, 0,
        "running_count for project A must be 0 (ideation excluded)"
    );

    assert_eq!(active_b, 1, "Project B must have 1 active ideation session");
    assert_eq!(idle_b, 0, "Project B must have 0 idle ideation sessions");
    assert_eq!(
        rc_b, 0,
        "running_count for project B must be 0 (ideation excluded)"
    );
}

/// Test: orphaned registry entries (no matching session row) are skipped.
#[tokio::test]
async fn test_orphaned_ideation_registry_entry_is_skipped() {
    let state = AppState::with_repos(
        Arc::new(MemoryTaskRepository::new()),
        Arc::new(MemoryProjectRepository::new()),
    );
    let exec_state = ExecutionState::new();

    // Register a session that has NO matching row in ideation_session_repo
    register_ideation(&*state.running_agent_registry, "session-orphan").await;

    let (rc, active, idle) = count_scoped_running(&state, &exec_state, None).await;

    assert_eq!(rc, 0, "Orphaned registry entry must not be counted");
    assert_eq!(active, 0);
    assert_eq!(idle, 0);
}

/// Test: running_count = (same-project active ideation) + (same-project active tasks).
#[tokio::test]
async fn test_running_count_equals_project_ideation_plus_project_tasks() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());

    let project_a = Project::new("Project A".to_string(), "/a".to_string());
    let pid_a = project_a.id.clone();
    let project_b = Project::new("Project B".to_string(), "/b".to_string());
    let pid_b = project_b.id.clone();
    project_repo.create(project_a).await.unwrap();
    project_repo.create(project_b).await.unwrap();

    // Project A: 1 executing task + 1 ideation session
    let mut task_a = Task::new(pid_a.clone(), "Task A".to_string());
    task_a.internal_status = InternalStatus::Executing;
    let task_a = task_repo.create(task_a).await.unwrap();

    // Project B: 1 executing task (must NOT appear in project A counts)
    let mut task_b = Task::new(pid_b.clone(), "Task B".to_string());
    task_b.internal_status = InternalStatus::Executing;
    let task_b = task_repo.create(task_b).await.unwrap();

    let state = AppState::with_repos(task_repo, project_repo);
    let exec_state = ExecutionState::new();

    state
        .ideation_session_repo
        .create(make_ideation_session("session-a1", &pid_a))
        .await
        .unwrap();

    register_task_execution(&*state.running_agent_registry, &task_a.id).await;
    register_task_execution(&*state.running_agent_registry, &task_b.id).await;
    register_ideation(&*state.running_agent_registry, "session-a1").await;

    let (rc_a, active_a, _idle_a) = count_scoped_running(&state, &exec_state, Some(&pid_a)).await;
    let (rc_b, active_b, _idle_b) = count_scoped_running(&state, &exec_state, Some(&pid_b)).await;

    assert_eq!(
        rc_a, 1,
        "Project A running_count must be 1 (task only, ideation excluded)"
    );
    assert_eq!(active_a, 1, "Project A must have 1 active ideation session");

    assert_eq!(
        rc_b, 1,
        "Project B running_count must be 1 task only (no ideation)"
    );
    assert_eq!(active_b, 0, "Project B has no ideation sessions");
}
