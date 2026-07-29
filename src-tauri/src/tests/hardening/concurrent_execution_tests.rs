// Concurrent execution hardening tests
//
// Tests for the race conditions fixed in the "agent already running" bug:
// 1. Concurrent try_register — atomic check-and-register on RunningAgentRegistry
// 2. Concurrent update_with_expected_status — optimistic locking on task transitions
// 3. Double-scheduler scenario — two schedulers both try to execute the same Ready task
// 4. Service-level concurrent transition_task — optimistic lock prevents double on_enter
// 5. ExecutionState execution-entry guard — direct entry actions dedupe per task
// 6. Service-level double-scheduler — two TaskSchedulerService instances, one agent registered
//
// Pattern: tokio::spawn + join! + XOR assertion ("exactly one wins")

use std::sync::Arc;

use crate::application::{AppState, TaskSchedulerService, TaskTransitionService};
use crate::commands::ExecutionState;
use crate::domain::entities::{InternalStatus, Project, ProjectId, Task};
use crate::domain::repositories::TaskRepository;
use crate::domain::services::{MemoryRunningAgentRegistry, RunningAgentKey, RunningAgentRegistry};
use crate::domain::state_machine::services::TaskScheduler;
use crate::infrastructure::memory::MemoryTaskRepository;

// ============================================================================
// Concurrent try_register — atomic slot claim
// ============================================================================

#[tokio::test]
async fn test_concurrent_try_register_only_one_wins() {
    // Two callers race to claim the same agent slot.
    // Exactly one should succeed; the other gets Err with existing info.
    let registry = Arc::new(MemoryRunningAgentRegistry::new());
    let key = RunningAgentKey::new("task_execution", "task-race-1");

    let reg1 = Arc::clone(&registry);
    let key1 = key.clone();
    let h1 = tokio::spawn(async move {
        reg1.try_register(key1, "conv-a".to_string(), "run-a".to_string())
            .await
    });

    let reg2 = Arc::clone(&registry);
    let key2 = key.clone();
    let h2 = tokio::spawn(async move {
        reg2.try_register(key2, "conv-b".to_string(), "run-b".to_string())
            .await
    });

    let (r1, r2) = tokio::join!(h1, h2);
    let r1 = r1.unwrap();
    let r2 = r2.unwrap();

    // XOR: exactly one succeeds
    assert!(
        r1.is_ok() ^ r2.is_ok(),
        "Exactly one try_register must succeed (XOR): r1={:?}, r2={:?}",
        r1.is_ok(),
        r2.is_ok()
    );

    // The winner's conversation_id should be in the registry
    let info = registry.get(&key).await.unwrap();
    let winner_conv = if r1.is_ok() { "conv-a" } else { "conv-b" };
    assert_eq!(info.conversation_id, winner_conv);
}

#[tokio::test]
async fn test_concurrent_try_register_different_keys_both_succeed() {
    // No contention: different keys should both succeed.
    let registry = Arc::new(MemoryRunningAgentRegistry::new());

    let reg1 = Arc::clone(&registry);
    let h1 = tokio::spawn(async move {
        reg1.try_register(
            RunningAgentKey::new("task_execution", "task-a"),
            "conv-a".to_string(),
            "run-a".to_string(),
        )
        .await
    });

    let reg2 = Arc::clone(&registry);
    let h2 = tokio::spawn(async move {
        reg2.try_register(
            RunningAgentKey::new("task_execution", "task-b"),
            "conv-b".to_string(),
            "run-b".to_string(),
        )
        .await
    });

    let (r1, r2) = tokio::join!(h1, h2);
    assert!(r1.unwrap().is_ok(), "task-a should succeed");
    assert!(r2.unwrap().is_ok(), "task-b should succeed");
}

#[tokio::test]
async fn test_concurrent_try_register_many_callers_exactly_one_wins() {
    // Stress test: 10 callers race for the same slot.
    let registry = Arc::new(MemoryRunningAgentRegistry::new());
    let key = RunningAgentKey::new("task_execution", "task-stress");

    let mut handles = Vec::new();
    for i in 0..10 {
        let reg = Arc::clone(&registry);
        let k = key.clone();
        handles.push(tokio::spawn(async move {
            reg.try_register(k, format!("conv-{}", i), format!("run-{}", i))
                .await
        }));
    }

    let mut ok_count = 0;
    let mut err_count = 0;
    for h in handles {
        match h.await.unwrap() {
            Ok(()) => ok_count += 1,
            Err(_) => err_count += 1,
        }
    }

    assert_eq!(ok_count, 1, "Exactly one caller should win the slot");
    assert_eq!(err_count, 9, "All other callers should fail");
}

// ============================================================================
// Concurrent update_with_expected_status — optimistic locking
// ============================================================================

#[tokio::test]
async fn test_concurrent_execution_entry_guard_only_one_wins() {
    let execution_state = Arc::new(ExecutionState::new());

    let mut handles = Vec::new();
    for _ in 0..10 {
        let state = Arc::clone(&execution_state);
        handles.push(tokio::spawn(async move {
            state.try_start_execution_entry("task-entry-race")
        }));
    }

    let mut admitted = 0;
    let mut skipped = 0;
    for handle in handles {
        if handle.await.unwrap() {
            admitted += 1;
        } else {
            skipped += 1;
        }
    }

    assert_eq!(admitted, 1, "Exactly one entry action should be admitted");
    assert_eq!(skipped, 9, "Duplicate entry actions should be skipped");
    assert!(
        execution_state.is_execution_entry_in_flight("task-entry-race"),
        "The winning entry action should keep the task marked in flight"
    );

    execution_state.finish_execution_entry("task-entry-race");

    assert!(
        !execution_state.is_execution_entry_in_flight("task-entry-race"),
        "Finishing should release the task entry guard"
    );
    assert!(
        execution_state.try_start_execution_entry("task-entry-race"),
        "A released task should be claimable again"
    );
    execution_state.finish_execution_entry("task-entry-race");
}

#[test]
fn test_execution_entry_guard_is_per_task() {
    let execution_state = ExecutionState::new();

    assert!(execution_state.try_start_execution_entry("task-entry-a"));
    assert!(execution_state.try_start_execution_entry("task-entry-b"));
    assert!(!execution_state.try_start_execution_entry("task-entry-a"));

    execution_state.finish_execution_entry("task-entry-a");
    execution_state.finish_execution_entry("task-entry-b");
}

#[tokio::test]
async fn test_execute_entry_actions_skips_when_execution_entry_guard_held() {
    let execution_state = Arc::new(ExecutionState::new());
    let app_state = AppState::new_test();

    let project = Project::new(
        "Entry Guard Project".to_string(),
        "/tmp/ralphx-entry-guard-missing-repo".to_string(),
    );
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Guarded entry task".to_string());
    task.internal_status = InternalStatus::Executing;
    let task_id = task.id.clone();
    app_state.task_repo.create(task.clone()).await.unwrap();

    assert!(execution_state.try_start_execution_entry(task_id.as_str()));

    let transition_service = build_transition_service(&app_state, &execution_state);
    transition_service
        .execute_entry_actions(&task_id, &task, InternalStatus::Executing)
        .await;

    let final_task = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        final_task.internal_status,
        InternalStatus::Executing,
        "A duplicate direct entry action should be a no-op, not a task failure"
    );
    assert!(
        execution_state.is_execution_entry_in_flight(task_id.as_str()),
        "The duplicate caller must not release another caller's entry guard"
    );

    execution_state.finish_execution_entry(task_id.as_str());
}

fn create_ready_task(id_suffix: &str) -> Task {
    let project_id = ProjectId::from_string("proj-concurrent".to_string());
    let mut task = Task::new(project_id, format!("Concurrent task {}", id_suffix));
    task.internal_status = InternalStatus::Ready;
    task
}

#[tokio::test]
async fn test_concurrent_optimistic_lock_only_one_wins() {
    // Two callers both read a Ready task, both try to transition to Executing.
    // The optimistic lock (WHERE status = Ready) ensures only one succeeds.
    let repo = Arc::new(MemoryTaskRepository::new());

    let task = create_ready_task("opt-1");
    let task_id = task.id.clone();
    repo.create(task).await.unwrap();

    // Both callers read the task (simulating the race window)
    let task_a = repo.get_by_id(&task_id).await.unwrap().unwrap();
    let task_b = repo.get_by_id(&task_id).await.unwrap().unwrap();

    // Both try to transition Ready → Executing
    let mut task_a_updated = task_a;
    task_a_updated.internal_status = InternalStatus::Executing;

    let mut task_b_updated = task_b;
    task_b_updated.internal_status = InternalStatus::Executing;

    let repo1 = Arc::clone(&repo);
    let h1 = tokio::spawn(async move {
        repo1
            .update_with_expected_status(&task_a_updated, InternalStatus::Ready)
            .await
            .unwrap()
    });

    let repo2 = Arc::clone(&repo);
    let h2 = tokio::spawn(async move {
        repo2
            .update_with_expected_status(&task_b_updated, InternalStatus::Ready)
            .await
            .unwrap()
    });

    let (r1, r2) = tokio::join!(h1, h2);
    let r1 = r1.unwrap();
    let r2 = r2.unwrap();

    // XOR: exactly one succeeds (returns true)
    assert!(
        r1 ^ r2,
        "Exactly one optimistic update must succeed (XOR): r1={}, r2={}",
        r1,
        r2
    );

    // Final state should be Executing (from the winner)
    let final_task = repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(
        final_task.internal_status,
        InternalStatus::Executing,
        "Task should be Executing after the winning transition"
    );
}

#[tokio::test]
async fn test_optimistic_lock_rejects_stale_status() {
    // Direct test: if the task has already been transitioned, the optimistic
    // lock returns false (no rows affected).
    let repo = Arc::new(MemoryTaskRepository::new());

    let task = create_ready_task("stale-1");
    let task_id = task.id.clone();
    repo.create(task).await.unwrap();

    // Caller A reads and transitions
    let mut task_a = repo.get_by_id(&task_id).await.unwrap().unwrap();
    task_a.internal_status = InternalStatus::Executing;
    let result_a = repo
        .update_with_expected_status(&task_a, InternalStatus::Ready)
        .await
        .unwrap();
    assert!(result_a, "First update should succeed");

    // Caller B reads stale state and tries to transition
    let mut stale_task = repo.get_by_id(&task_id).await.unwrap().unwrap();
    // Task is now Executing, but caller B thinks it's Ready
    stale_task.internal_status = InternalStatus::Executing;
    let result_b = repo
        .update_with_expected_status(&stale_task, InternalStatus::Ready)
        .await
        .unwrap();
    assert!(
        !result_b,
        "Second update should fail — task is no longer Ready"
    );
}

#[tokio::test]
async fn test_concurrent_optimistic_lock_many_callers() {
    // Stress test: 10 callers all try to transition Ready → Executing.
    let repo = Arc::new(MemoryTaskRepository::new());

    let task = create_ready_task("stress-1");
    let task_id = task.id.clone();
    repo.create(task).await.unwrap();

    let mut handles = Vec::new();
    for i in 0..10 {
        let r = Arc::clone(&repo);
        let tid = task_id.clone();
        handles.push(tokio::spawn(async move {
            let mut t = r.get_by_id(&tid).await.unwrap().unwrap();
            t.internal_status = InternalStatus::Executing;
            t.description = Some(format!("Caller {}", i));
            r.update_with_expected_status(&t, InternalStatus::Ready)
                .await
                .unwrap()
        }));
    }

    let mut ok_count = 0;
    for h in handles {
        if h.await.unwrap() {
            ok_count += 1;
        }
    }

    assert_eq!(
        ok_count, 1,
        "Exactly one caller should win the optimistic lock"
    );
}

// ============================================================================
// Double-scheduler scenario — two schedulers race to execute the same task
// ============================================================================

#[tokio::test]
async fn test_double_scheduler_only_one_executes() {
    // Simulates two independent scheduler instances both discovering a Ready task
    // and trying to execute it. The combination of try_register + optimistic lock
    // ensures only one succeeds at each layer.
    let repo = Arc::new(MemoryTaskRepository::new());
    let registry = Arc::new(MemoryRunningAgentRegistry::new());

    let task = create_ready_task("sched-1");
    let task_id = task.id.clone();
    repo.create(task).await.unwrap();

    // Both schedulers run the full sequence: read → try_register → update_with_expected_status
    let repo1 = Arc::clone(&repo);
    let reg1 = Arc::clone(&registry);
    let tid1 = task_id.clone();
    let h1 = tokio::spawn(async move {
        // Step 1: Read the task
        let mut t = repo1.get_by_id(&tid1).await.unwrap().unwrap();
        if t.internal_status != InternalStatus::Ready {
            return (false, false);
        }

        // Step 2: Try to claim the agent slot
        let key = RunningAgentKey::new("task_execution", tid1.as_str());
        let reg_ok = reg1
            .try_register(key, "conv-sched-1".to_string(), "run-sched-1".to_string())
            .await
            .is_ok();

        // Step 3: Try optimistic transition
        t.internal_status = InternalStatus::Executing;
        let update_ok = repo1
            .update_with_expected_status(&t, InternalStatus::Ready)
            .await
            .unwrap();

        (reg_ok, update_ok)
    });

    let repo2 = Arc::clone(&repo);
    let reg2 = Arc::clone(&registry);
    let tid2 = task_id.clone();
    let h2 = tokio::spawn(async move {
        let mut t = repo2.get_by_id(&tid2).await.unwrap().unwrap();
        if t.internal_status != InternalStatus::Ready {
            return (false, false);
        }

        let key = RunningAgentKey::new("task_execution", tid2.as_str());
        let reg_ok = reg2
            .try_register(key, "conv-sched-2".to_string(), "run-sched-2".to_string())
            .await
            .is_ok();

        t.internal_status = InternalStatus::Executing;
        let update_ok = repo2
            .update_with_expected_status(&t, InternalStatus::Ready)
            .await
            .unwrap();

        (reg_ok, update_ok)
    });

    let (r1, r2) = tokio::join!(h1, h2);
    let (reg1_ok, upd1_ok) = r1.unwrap();
    let (reg2_ok, upd2_ok) = r2.unwrap();

    // At least ONE of the two guards must have blocked the loser:
    // - try_register: exactly one wins the agent slot (XOR)
    // - optimistic lock: exactly one wins the DB update (XOR)
    //
    // The combined effect: it's impossible for both callers to fully succeed.
    let caller1_fully_succeeded = reg1_ok && upd1_ok;
    let caller2_fully_succeeded = reg2_ok && upd2_ok;

    assert!(
        !(caller1_fully_succeeded && caller2_fully_succeeded),
        "Both callers must NOT fully succeed — at least one guard must block: \
         caller1=(reg={}, upd={}), caller2=(reg={}, upd={})",
        reg1_ok,
        upd1_ok,
        reg2_ok,
        upd2_ok
    );

    // Task should end up in Executing
    let final_task = repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(final_task.internal_status, InternalStatus::Executing);
}

#[tokio::test]
async fn test_double_scheduler_with_guard_check() {
    // A stricter version: scheduler checks try_register result before proceeding.
    // This mirrors the production code pattern where try_register failure skips execution.
    let repo = Arc::new(MemoryTaskRepository::new());
    let registry = Arc::new(MemoryRunningAgentRegistry::new());

    let task = create_ready_task("sched-guard-1");
    let task_id = task.id.clone();
    repo.create(task).await.unwrap();

    let repo1 = Arc::clone(&repo);
    let reg1 = Arc::clone(&registry);
    let tid1 = task_id.clone();
    let h1 = tokio::spawn(async move {
        let mut t = repo1.get_by_id(&tid1).await.unwrap().unwrap();
        if t.internal_status != InternalStatus::Ready {
            return false;
        }

        let key = RunningAgentKey::new("task_execution", tid1.as_str());
        if reg1
            .try_register(key, "conv-1".to_string(), "run-1".to_string())
            .await
            .is_err()
        {
            return false; // Agent slot taken — bail out
        }

        t.internal_status = InternalStatus::Executing;
        repo1
            .update_with_expected_status(&t, InternalStatus::Ready)
            .await
            .unwrap()
    });

    let repo2 = Arc::clone(&repo);
    let reg2 = Arc::clone(&registry);
    let tid2 = task_id.clone();
    let h2 = tokio::spawn(async move {
        let mut t = repo2.get_by_id(&tid2).await.unwrap().unwrap();
        if t.internal_status != InternalStatus::Ready {
            return false;
        }

        let key = RunningAgentKey::new("task_execution", tid2.as_str());
        if reg2
            .try_register(key, "conv-2".to_string(), "run-2".to_string())
            .await
            .is_err()
        {
            return false;
        }

        t.internal_status = InternalStatus::Executing;
        repo2
            .update_with_expected_status(&t, InternalStatus::Ready)
            .await
            .unwrap()
    });

    let (r1, r2) = tokio::join!(h1, h2);
    let succeeded1 = r1.unwrap();
    let succeeded2 = r2.unwrap();

    // XOR: exactly one scheduler succeeds end-to-end
    assert!(
        succeeded1 ^ succeeded2,
        "Exactly one scheduler must fully succeed (XOR): s1={}, s2={}",
        succeeded1,
        succeeded2
    );
}

// ============================================================================
// Service-level: concurrent transition_task — optimistic lock prevents double on_enter
// ============================================================================

/// Build a TaskTransitionService from test AppState (same pattern as scheduler tests).
fn build_transition_service(
    app_state: &AppState,
    execution_state: &Arc<ExecutionState>,
) -> TaskTransitionService {
    TaskTransitionService::new(
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.task_dependency_repo),
        Arc::clone(&app_state.project_repo),
        Arc::clone(&app_state.chat_message_repo),
        Arc::clone(&app_state.chat_attachment_repo),
        Arc::clone(&app_state.chat_conversation_repo),
        Arc::clone(&app_state.agent_run_repo),
        Arc::clone(&app_state.ideation_session_repo),
        Arc::clone(&app_state.activity_event_repo),
        Arc::clone(&app_state.message_queue),
        Arc::clone(&app_state.running_agent_registry),
        Arc::clone(execution_state),
        None,
        Arc::clone(&app_state.memory_event_repo),
    )
}

/// Build a TaskSchedulerService from test AppState.
fn build_scheduler(
    app_state: &AppState,
    execution_state: &Arc<ExecutionState>,
) -> TaskSchedulerService {
    TaskSchedulerService::new(
        Arc::clone(execution_state),
        Arc::clone(&app_state.project_repo),
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.task_dependency_repo),
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
        None,
    )
}

#[tokio::test]
async fn test_concurrent_transition_task_only_one_triggers_on_enter() {
    // Two TaskTransitionService instances share the same repos (including registry).
    // Both call transition_task(task_id, Executing) concurrently for the same Ready task.
    // The optimistic lock (Task #6) ensures only ONE wins the DB update,
    // and therefore only ONE triggers on_enter(Executing).
    let execution_state = Arc::new(ExecutionState::new());
    execution_state.set_max_concurrent(10);
    let app_state = AppState::new_test();

    // Create project and Ready task
    let project = Project::new(
        "Concurrent Transition Test".to_string(),
        "/test/concurrent".to_string(),
    );
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Race Condition Task".to_string());
    task.internal_status = InternalStatus::Ready;
    app_state.task_repo.create(task.clone()).await.unwrap();
    let task_id = task.id.clone();

    // Build two transition services sharing the same repos (including agent registry)
    let ts1 = build_transition_service(&app_state, &execution_state);
    let ts2 = build_transition_service(&app_state, &execution_state);

    let tid1 = task_id.clone();
    let h1 =
        tokio::spawn(async move { ts1.transition_task(&tid1, InternalStatus::Executing).await });

    let tid2 = task_id.clone();
    let h2 =
        tokio::spawn(async move { ts2.transition_task(&tid2, InternalStatus::Executing).await });

    let (r1, r2) = tokio::join!(h1, h2);
    let r1 = r1.unwrap();
    let r2 = r2.unwrap();

    // Current transition validation allows the losing caller to observe the task after the
    // winning caller has already moved it, in which case InvalidTransition is expected.
    let at_least_one_succeeded = r1.is_ok() || r2.is_ok();
    assert!(
        at_least_one_succeeded,
        "At least one concurrent caller must win the transition: r1={:?}, r2={:?}",
        r1, r2
    );
    assert!(
        r1.is_ok() || matches!(&r1, Err(crate::error::AppError::InvalidTransition { .. })),
        "Caller 1 must either win or observe a validated transition loss: {:?}",
        r1
    );
    assert!(
        r2.is_ok() || matches!(&r2, Err(crate::error::AppError::InvalidTransition { .. })),
        "Caller 2 must either win or observe a validated transition loss: {:?}",
        r2
    );

    // Task should no longer be Ready (winner transitioned it)
    let final_task = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .unwrap();
    assert_ne!(
        final_task.internal_status,
        InternalStatus::Ready,
        "Task should have been transitioned from Ready by the winning caller"
    );

    // Key assertion: at most one agent registration in the shared registry.
    // This proves only ONE on_enter(Executing) fired (the winner's).
    // The loser was blocked by the optimistic lock before reaching on_enter.
    let all_agents = app_state.running_agent_registry.list_all().await;
    assert!(
        all_agents.len() <= 1,
        "At most one agent should be registered — only one on_enter should fire. Found: {}",
        all_agents.len()
    );
}

// ============================================================================
// Service-level: double-scheduler — two TaskSchedulerService instances
// ============================================================================

#[tokio::test]
async fn test_double_scheduler_service_only_one_agent_registered() {
    // Two independent TaskSchedulerService instances share the same repos.
    // Each has its own scheduling_lock (per-instance), so both can proceed
    // concurrently. The optimistic lock + try_register prevent double execution.
    let execution_state = Arc::new(ExecutionState::new());
    execution_state.set_max_concurrent(10);
    let app_state = AppState::new_test();

    // Create project and Ready task
    let project = Project::new(
        "Double Scheduler Test".to_string(),
        "/test/double-sched".to_string(),
    );
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Double-Scheduled Task".to_string());
    task.internal_status = InternalStatus::Ready;
    app_state.task_repo.create(task.clone()).await.unwrap();
    let task_id = task.id.clone();

    // Build two scheduler instances — separate scheduling_locks but shared repos
    let sched1 = Arc::new(build_scheduler(&app_state, &execution_state));
    let sched2 = Arc::new(build_scheduler(&app_state, &execution_state));

    let s1 = Arc::clone(&sched1);
    let h1 = tokio::spawn(async move {
        s1.try_schedule_ready_tasks().await;
    });

    let s2 = Arc::clone(&sched2);
    let h2 = tokio::spawn(async move {
        s2.try_schedule_ready_tasks().await;
    });

    let (r1, r2) = tokio::join!(h1, h2);
    r1.unwrap();
    r2.unwrap();

    // Task should have been transitioned from Ready
    let final_task = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .unwrap();
    assert_ne!(
        final_task.internal_status,
        InternalStatus::Ready,
        "Task should have been transitioned by one of the schedulers"
    );

    // Key assertion: at most one agent registered in the shared registry.
    // This proves the three-layer fix (optimistic lock + try_register + Ok(was_queued: true) guard)
    // prevents double-execution even when two schedulers race.
    let all_agents = app_state.running_agent_registry.list_all().await;
    assert!(
        all_agents.len() <= 1,
        "At most one agent should be registered after two schedulers race. Found: {}",
        all_agents.len()
    );
}
