// Interactive Mode Integration Tests
//
// Tests for the interactive process lifecycle: running count management,
// idle slot tracking, burst prevention, and reconciler-compatible patterns.
//
// These tests verify the ExecutionState + InteractiveProcessRegistry contracts
// that the reconciler relies on for correct prune/skip decisions.
// Also includes tests for provider-session persistence in both interactive
// and non-interactive modes, while preserving the legacy Claude alias.

use std::sync::Arc;

use ralphx_lib::application::interactive_process_registry::{
    InteractiveProcessKey, InteractiveProcessRegistry,
};
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::agents::{AgentHarnessKind, ProviderSessionRef};
use ralphx_lib::domain::entities::{ChatConversation, TaskId};
use ralphx_lib::domain::repositories::ChatConversationRepository;
use ralphx_lib::domain::services::running_agent_registry::{
    MemoryRunningAgentRegistry, RunningAgentKey, RunningAgentRegistry,
};
use ralphx_lib::infrastructure::memory::MemoryChatConversationRepository;

// ========================================
// Test 1: Reconciler skips idle interactive processes
// ========================================

#[tokio::test]
async fn test_reconciler_skips_idle_interactive_process() {
    // Setup: ExecutionState with running=1, registered in running_agent_registry,
    // and marked interactive idle. InteractiveProcessRegistry has the process.
    let execution_state = ExecutionState::new();
    let registry = MemoryRunningAgentRegistry::new();
    let ipr = InteractiveProcessRegistry::new();

    let context_type = "task_execution";
    let context_id = "task-1";
    let idle_key = format!("{}/{}", context_type, context_id);

    // Register agent in running registry
    let agent_key = RunningAgentKey {
        context_type: context_type.to_string(),
        context_id: context_id.to_string(),
    };
    registry
        .register(
            agent_key.clone(),
            12345, // pid
            "conv-1".to_string(),
            "run-1".to_string(),
            None,
            None,
        )
        .await;

    // Process is running (count=1)
    execution_state.increment_running();
    assert_eq!(execution_state.running_count(), 1);

    // TurnComplete: decrement + mark idle
    execution_state.decrement_and_mark_idle(&idle_key);
    assert_eq!(execution_state.running_count(), 0);
    assert!(execution_state.is_interactive_idle(&idle_key));

    // Register in InteractiveProcessRegistry (process is alive between turns)
    let (stdin, _child) = create_test_stdin().await;
    let ipr_key = InteractiveProcessKey::new(context_type, context_id);
    ipr.register(ipr_key.clone(), stdin).await;

    // Reconciler logic: check IPR has_process → should skip pruning
    assert!(
        ipr.has_process(&ipr_key).await,
        "IPR should report process exists"
    );

    // Entry should still be in running registry (NOT pruned)
    assert!(
        registry.is_running(&agent_key).await,
        "Running registry entry should be preserved when interactive process exists"
    );

    // Running count should still be 0 (idle between turns)
    assert_eq!(execution_state.running_count(), 0);
}

// ========================================
// Test 2: Reconciler prunes when interactive process is gone
// ========================================

#[tokio::test]
async fn test_reconciler_prunes_when_interactive_process_gone() {
    // Same setup but InteractiveProcessRegistry has NO process for this context
    let execution_state = ExecutionState::new();
    let registry = MemoryRunningAgentRegistry::new();
    let ipr = InteractiveProcessRegistry::new();

    let context_type = "task_execution";
    let context_id = "task-2";
    let idle_key = format!("{}/{}", context_type, context_id);

    // Register agent in running registry
    let agent_key = RunningAgentKey {
        context_type: context_type.to_string(),
        context_id: context_id.to_string(),
    };
    registry
        .register(
            agent_key.clone(),
            99999, // pid (dead)
            "conv-2".to_string(),
            "run-2".to_string(),
            None,
            None,
        )
        .await;

    execution_state.increment_running();
    execution_state.decrement_and_mark_idle(&idle_key);

    // InteractiveProcessRegistry does NOT have this process (it exited/crashed)
    let ipr_key = InteractiveProcessKey::new(context_type, context_id);
    assert!(
        !ipr.has_process(&ipr_key).await,
        "IPR should NOT have this process"
    );

    // Reconciler logic: IPR.has_process=false → process is dead, should prune
    // Simulate prune: unregister from running registry
    let unregistered = registry.unregister(&agent_key, "run-2").await;
    assert!(
        unregistered.is_some(),
        "Should successfully unregister dead agent"
    );

    // Verify pruned
    assert!(
        !registry.is_running(&agent_key).await,
        "Agent should no longer be in running registry after prune"
    );

    // Clean up idle slot since process is gone
    execution_state.remove_interactive_slot(&idle_key);
    assert!(!execution_state.is_interactive_idle(&idle_key));
}

// ========================================
// Test 3: Running count force-sync subtracts idle slots
// ========================================

#[tokio::test]
async fn test_force_sync_running_count_subtracts_idle_slots() {
    // Setup: 3 registry entries, 1 is interactive idle
    let execution_state = ExecutionState::new();
    let registry = MemoryRunningAgentRegistry::new();

    let contexts = vec![
        ("task_execution", "task-1"),
        ("task_execution", "task-2"),
        ("task_execution", "task-3"),
    ];

    // Register all 3 agents
    for (ct, ci) in &contexts {
        let key = RunningAgentKey {
            context_type: ct.to_string(),
            context_id: ci.to_string(),
        };
        registry
            .register(
                key,
                10000,
                format!("conv-{}", ci),
                format!("run-{}", ci),
                None,
                None,
            )
            .await;
        execution_state.increment_running();
    }
    assert_eq!(execution_state.running_count(), 3);

    // Mark task-2 as idle (TurnComplete)
    let idle_key = "task_execution/task-2";
    execution_state.decrement_and_mark_idle(idle_key);
    assert_eq!(execution_state.running_count(), 2);
    assert!(execution_state.is_interactive_idle(idle_key));

    // Force-sync: registry has 3 entries, 1 is idle → running = 3 - 1 = 2
    let all_entries = registry.list_all().await;
    let registry_count = all_entries.len() as u32;
    assert_eq!(registry_count, 3);

    // Count idle slots among registry entries
    let mut idle_count: u32 = 0;
    for (key, _) in &all_entries {
        let slot_key = format!("{}/{}", key.context_type, key.context_id);
        if execution_state.is_interactive_idle(&slot_key) {
            idle_count += 1;
        }
    }
    assert_eq!(idle_count, 1, "Exactly 1 entry should be idle");

    // Apply force-sync (what the reconciler does)
    execution_state.set_running_count(registry_count.saturating_sub(idle_count));
    assert_eq!(
        execution_state.running_count(),
        2,
        "Force-sync should set running_count = registry_entries - idle_slots"
    );
}

// ========================================
// Test 4: Turn lifecycle: increment → TurnComplete → resume → TurnComplete
// ========================================

#[tokio::test]
async fn test_turn_lifecycle_increment_decrement_resume() {
    let state = ExecutionState::new();
    let key = "task_execution/task-1";

    // Step 1: Spawn — increment
    let count = state.increment_running();
    assert_eq!(count, 1, "After spawn: running_count=1");
    assert_eq!(state.running_count(), 1);

    // Step 2: TurnComplete — atomic decrement + mark idle
    let count = state.decrement_and_mark_idle(key);
    assert_eq!(count, 0, "After TurnComplete: running_count=0");
    assert_eq!(state.running_count(), 0);
    assert!(
        state.is_interactive_idle(key),
        "Slot should be marked idle after TurnComplete"
    );

    // Step 3: Resume — claim slot + increment
    let claimed = state.claim_interactive_slot(key);
    assert!(claimed, "Claim should succeed for idle slot");
    assert!(
        !state.is_interactive_idle(key),
        "Slot should no longer be idle after claim"
    );
    let count = state.increment_running();
    assert_eq!(count, 1, "After resume: running_count=1");

    // Step 4: Second TurnComplete — decrement + mark idle again
    let count = state.decrement_and_mark_idle(key);
    assert_eq!(count, 0, "After second TurnComplete: running_count=0");
    assert_eq!(state.running_count(), 0);
    assert!(
        state.is_interactive_idle(key),
        "Slot should be idle again after second TurnComplete"
    );
}

// ========================================
// Test 5: Burst prevention — rapid messages only increment once
// ========================================

#[tokio::test]
async fn test_burst_prevention_rapid_messages_only_increment_once() {
    let state = Arc::new(ExecutionState::new());
    let key = "task_execution/task-1";

    // Initial state: process was running, now completed a turn
    state.increment_running();
    state.decrement_and_mark_idle(key);
    assert_eq!(state.running_count(), 0);
    assert!(state.is_interactive_idle(key));

    // Simulate 3 concurrent claim_interactive_slot calls (3 messages arrive at once)
    let mut claim_results = Vec::new();
    for _ in 0..3 {
        claim_results.push(state.claim_interactive_slot(key));
    }

    // Exactly 1 should return true (won the claim)
    let successful_claims: usize = claim_results.iter().filter(|&&r| r).count();
    assert_eq!(
        successful_claims, 1,
        "Exactly one concurrent claim should succeed"
    );

    // Only the winner increments
    state.increment_running();
    assert_eq!(
        state.running_count(),
        1,
        "Running count should be 1 (not 3) after burst"
    );
}

#[tokio::test]
async fn test_burst_prevention_concurrent_threads() {
    // More realistic: actual thread concurrency
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::thread;

    let state = Arc::new(ExecutionState::new());
    let key = "task_execution/task-1";

    // Setup: process idle between turns
    state.increment_running();
    state.decrement_and_mark_idle(key);
    assert_eq!(state.running_count(), 0);

    let claim_count = Arc::new(AtomicU32::new(0));
    let barrier = Arc::new(std::sync::Barrier::new(10));
    let mut handles = vec![];

    // Spawn 10 threads all trying to claim the same slot
    for _ in 0..10 {
        let s = Arc::clone(&state);
        let cc = Arc::clone(&claim_count);
        let b = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            b.wait(); // Synchronize all threads to start simultaneously
            if s.claim_interactive_slot(key) {
                s.increment_running();
                cc.fetch_add(1, Ordering::SeqCst);
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    assert_eq!(
        claim_count.load(Ordering::SeqCst),
        1,
        "Exactly one thread should win the claim"
    );
    assert_eq!(
        state.running_count(),
        1,
        "Running count should be 1 after burst"
    );
}

// ========================================
// Test 6: Process death while idle — no double-decrement
// ========================================

#[tokio::test]
async fn test_process_death_while_idle_no_double_decrement() {
    let state = ExecutionState::new();
    let key = "task_execution/task-1";

    // Process starts running
    state.increment_running();
    assert_eq!(state.running_count(), 1);

    // TurnComplete: decrement + mark idle → count=0
    state.decrement_and_mark_idle(key);
    assert_eq!(state.running_count(), 0);
    assert!(state.is_interactive_idle(key));

    // Process dies while idle — cleanup removes slot
    state.remove_interactive_slot(key);
    assert!(!state.is_interactive_idle(key));

    // On-exit handler tries to decrement (should NOT underflow)
    let count = state.decrement_running();
    assert_eq!(
        count, 0,
        "saturating_sub should prevent underflow: count stays at 0"
    );
    assert_eq!(
        state.running_count(),
        0,
        "Running count must be 0, not u32::MAX"
    );

    // Double-check: another decrement attempt also stays at 0
    let count = state.decrement_running();
    assert_eq!(count, 0);
    assert_eq!(state.running_count(), 0);
}

// ========================================
// Test 7: Process death mid-turn — normal decrement
// ========================================

#[tokio::test]
async fn test_process_death_mid_turn_normal_decrement() {
    let state = ExecutionState::new();
    let key = "task_execution/task-1";

    // Process starts running
    state.increment_running();
    assert_eq!(state.running_count(), 1);

    // Process dies mid-turn (no TurnComplete was received)
    // On-exit handler decrements
    let count = state.decrement_running();
    assert_eq!(count, 0, "Normal decrement from 1 → 0");
    assert_eq!(state.running_count(), 0);

    // remove_interactive_slot is a no-op since it was never marked idle
    assert!(!state.is_interactive_idle(key));
    state.remove_interactive_slot(key);
    assert!(!state.is_interactive_idle(key));

    // Running count unchanged
    assert_eq!(state.running_count(), 0);
}

// ========================================
// Additional edge case tests
// ========================================

#[tokio::test]
async fn test_multiple_interactive_processes_independent_slots() {
    // Verify that multiple interactive processes track independently
    let state = ExecutionState::new();
    let key1 = "task_execution/task-1";
    let key2 = "ideation/session-1";
    let key3 = "task_execution/task-2";

    // Start 3 processes
    state.increment_running(); // task-1
    state.increment_running(); // session-1
    state.increment_running(); // task-2
    assert_eq!(state.running_count(), 3);

    // task-1 goes idle
    state.decrement_and_mark_idle(key1);
    assert_eq!(state.running_count(), 2);
    assert!(state.is_interactive_idle(key1));
    assert!(!state.is_interactive_idle(key2));
    assert!(!state.is_interactive_idle(key3));

    // session-1 goes idle
    state.decrement_and_mark_idle(key2);
    assert_eq!(state.running_count(), 1);
    assert!(state.is_interactive_idle(key1));
    assert!(state.is_interactive_idle(key2));

    // task-1 resumes (new message)
    assert!(state.claim_interactive_slot(key1));
    state.increment_running();
    assert_eq!(state.running_count(), 2);
    assert!(!state.is_interactive_idle(key1));
    assert!(state.is_interactive_idle(key2));

    // task-2 completes its turn
    state.decrement_and_mark_idle(key3);
    assert_eq!(state.running_count(), 1);
    assert!(state.is_interactive_idle(key3));

    // Only task-1 is actively running
    assert!(!state.is_interactive_idle(key1));
    assert!(state.is_interactive_idle(key2));
    assert!(state.is_interactive_idle(key3));
}

#[tokio::test]
async fn test_decrement_and_mark_idle_from_zero_saturates() {
    // Edge case: if decrement_and_mark_idle is called when count is already 0
    let state = ExecutionState::new();
    let key = "task_execution/task-1";

    assert_eq!(state.running_count(), 0);

    // Should not panic or underflow
    let count = state.decrement_and_mark_idle(key);
    assert_eq!(count, 0, "saturating_sub from 0 should stay at 0");
    assert_eq!(state.running_count(), 0);
    assert!(state.is_interactive_idle(key));
}

#[tokio::test]
async fn test_reconciler_pattern_with_mixed_active_and_idle() {
    // Full reconciler pattern: some processes active, some idle, force-sync
    let state = ExecutionState::new();
    let registry = MemoryRunningAgentRegistry::new();
    let ipr = InteractiveProcessRegistry::new();

    // 5 registered agents
    let contexts = vec![
        ("task_execution", "task-1", true),  // active (has IPR, actively processing)
        ("task_execution", "task-2", true),  // idle (has IPR, between turns)
        ("ideation", "session-1", true),     // idle (has IPR, between turns)
        ("task_execution", "task-3", false), // dead (no IPR, process crashed)
        ("review", "task-4", false),         // normal task agent (no IPR)
    ];

    for (ct, ci, has_ipr) in &contexts {
        let key = RunningAgentKey {
            context_type: ct.to_string(),
            context_id: ci.to_string(),
        };
        registry
            .register(
                key,
                10000,
                format!("conv-{}", ci),
                format!("run-{}", ci),
                None,
                None,
            )
            .await;
        state.increment_running();

        if *has_ipr {
            let (stdin, _child) = create_test_stdin().await;
            let ipr_key = InteractiveProcessKey::new(*ct, *ci);
            ipr.register(ipr_key, stdin).await;
        }
    }
    assert_eq!(state.running_count(), 5);

    // Mark task-2 and session-1 as idle (TurnComplete)
    state.decrement_and_mark_idle("task_execution/task-2");
    state.decrement_and_mark_idle("ideation/session-1");
    assert_eq!(state.running_count(), 3);

    // Reconciler runs:
    // - task-1: IPR=true, NOT idle → active, keep
    // - task-2: IPR=true, IS idle → keep (interactive, between turns)
    // - session-1: IPR=true, IS idle → keep (interactive, between turns)
    // - task-3: IPR=false → dead, prune
    // - task-4: IPR=false → normal agent (check pid alive, etc.)

    let entries = registry.list_all().await;
    let mut kept_count: u32 = 0;
    let mut idle_count: u32 = 0;

    for (key, _info) in &entries {
        let ipr_key = InteractiveProcessKey::new(&key.context_type, &key.context_id);
        let slot_key = format!("{}/{}", key.context_type, key.context_id);

        if ipr.has_process(&ipr_key).await {
            // Skip prune for interactive processes
            kept_count += 1;
            if state.is_interactive_idle(&slot_key) {
                idle_count += 1;
            }
        } else {
            // Would be pruned by reconciler (check pid, etc.)
            // For this test, we just count
        }
    }

    assert_eq!(kept_count, 3, "3 interactive processes should be kept");
    assert_eq!(idle_count, 2, "2 of the kept processes are idle");

    // Force-sync: total entries(5) - pruned(assume task-3 pruned) = 4 kept
    // But idle=2, so effective running = 4 - 2 = 2 (task-1 active + task-4 normal)
    // Plus the idle ones don't count toward running
    let effective_running = (entries.len() as u32) - 1 - idle_count; // -1 for task-3 pruned
    state.set_running_count(effective_running);
    assert_eq!(state.running_count(), 2);
}

// ========================================
// claude_session_id persistence tests
// ========================================

/// Simulates what the TurnComplete arm in process_stream_background does:
/// when a TurnComplete event arrives with session_id=Some(...), it persists a
/// provider session ref and keeps the legacy Claude alias in sync.
#[tokio::test]
async fn test_interactive_turn_complete_persists_session_id() {
    let repo = MemoryChatConversationRepository::new();
    let task_id = TaskId::from_string("task-interactive-session-1".to_string());
    let conv = ChatConversation::new_task(task_id);
    let conv_id = conv.id;

    repo.create(conv).await.unwrap();

    // Verify no session_id initially
    let before = repo.get_by_id(&conv_id).await.unwrap().unwrap();
    assert!(
        before.claude_session_id.is_none(),
        "claude_session_id should be None before TurnComplete"
    );

    // Simulate TurnComplete: session_id is Some → persist it
    let session_id = "test-session-123";
    repo.update_provider_session_ref(
        &conv_id,
        &ProviderSessionRef {
            harness: AgentHarnessKind::Claude,
            provider_session_id: session_id.to_string(),
        },
    )
        .await
        .unwrap();

    // Verify persisted
    let after = repo.get_by_id(&conv_id).await.unwrap().unwrap();
    assert_eq!(after.provider_session_id, Some(session_id.to_string()));
    assert_eq!(after.provider_harness, Some(AgentHarnessKind::Claude));
    assert_eq!(
        after.claude_session_id,
        Some(session_id.to_string()),
        "claude_session_id should be persisted after TurnComplete"
    );
}

/// The session_id_persisted flag in process_stream_background ensures only the
/// first TurnComplete with a session_id persists a provider session ref.
/// This test verifies that simulating the guard logic produces first-wins semantics.
#[tokio::test]
async fn test_session_id_first_capture_wins() {
    let repo = MemoryChatConversationRepository::new();
    let task_id = TaskId::from_string("task-interactive-session-2".to_string());
    let conv = ChatConversation::new_task(task_id);
    let conv_id = conv.id;

    repo.create(conv).await.unwrap();

    // Simulate first TurnComplete: session_id_persisted=false → persist and set flag
    let first_session_id = "first-session-abc";
    let mut session_id_persisted = false;
    if !session_id_persisted {
        repo.update_provider_session_ref(
            &conv_id,
            &ProviderSessionRef {
                harness: AgentHarnessKind::Claude,
                provider_session_id: first_session_id.to_string(),
            },
        )
            .await
            .unwrap();
        session_id_persisted = true;
    }

    // Simulate second TurnComplete: session_id_persisted=true → skip persistence
    let second_session_id = "second-session-xyz";
    if !session_id_persisted {
        repo.update_provider_session_ref(
            &conv_id,
            &ProviderSessionRef {
                harness: AgentHarnessKind::Claude,
                provider_session_id: second_session_id.to_string(),
            },
        )
            .await
            .unwrap();
    }

    // First session_id must win
    let result = repo.get_by_id(&conv_id).await.unwrap().unwrap();
    assert_eq!(
        result.provider_session_id,
        Some(first_session_id.to_string())
    );
    assert_eq!(result.provider_harness, Some(AgentHarnessKind::Claude));
    assert_eq!(
        result.claude_session_id,
        Some(first_session_id.to_string()),
        "First session_id should win — the session_id_persisted guard prevents subsequent overwrite"
    );
}

/// The TurnComplete arm uses `if let (Some(ref sess_id), ...) = (&session_id, ...)`,
/// so it only persists a provider session ref when session_id is Some.
/// An existing session_id must not be cleared when a later TurnComplete has None.
#[tokio::test]
async fn test_turn_complete_with_none_session_id_does_not_clear_existing() {
    let repo = MemoryChatConversationRepository::new();
    let task_id = TaskId::from_string("task-interactive-session-3".to_string());
    let conv = ChatConversation::new_task(task_id);
    let conv_id = conv.id;

    repo.create(conv).await.unwrap();

    // First TurnComplete: set session_id
    let existing_session_id = "existing-session-456";
    repo.update_provider_session_ref(
        &conv_id,
        &ProviderSessionRef {
            harness: AgentHarnessKind::Claude,
            provider_session_id: existing_session_id.to_string(),
        },
    )
        .await
        .unwrap();

    // Second TurnComplete with session_id=None — simulate the if-let guard
    let session_id_from_event: Option<String> = None;
    if let Some(ref sess_id) = session_id_from_event {
        // This branch is only entered when Some — will not run for None
        repo.update_provider_session_ref(
            &conv_id,
            &ProviderSessionRef {
                harness: AgentHarnessKind::Claude,
                provider_session_id: sess_id.clone(),
            },
        )
            .await
            .unwrap();
    }
    // clear_provider_session_ref is never called in TurnComplete path

    // Existing session_id must be preserved
    let result = repo.get_by_id(&conv_id).await.unwrap().unwrap();
    assert_eq!(
        result.provider_session_id,
        Some(existing_session_id.to_string())
    );
    assert_eq!(result.provider_harness, Some(AgentHarnessKind::Claude));
    assert_eq!(
        result.claude_session_id,
        Some(existing_session_id.to_string()),
        "Existing claude_session_id must not be cleared when TurnComplete carries no session_id"
    );
}

/// Non-interactive (one-shot) agents persist session_id via the post-loop code in
/// chat_service_send_background.rs: `if let Some(ref sess_id) = provider_session_id { ... }`
/// This is the same underlying provider-session update call — verify it persists.
#[tokio::test]
async fn test_non_interactive_post_loop_persists_session_id() {
    let repo = MemoryChatConversationRepository::new();
    let task_id = TaskId::from_string("task-noninteractive-session-1".to_string());
    let conv = ChatConversation::new_task_execution(task_id);
    let conv_id = conv.id;

    repo.create(conv).await.unwrap();

    // Verify no session_id initially
    let before = repo.get_by_id(&conv_id).await.unwrap().unwrap();
    assert!(
        before.claude_session_id.is_none(),
        "claude_session_id should be None before post-loop persistence"
    );

    // Simulate post-loop persistence (chat_service_send_background.rs Ok(outcome) branch)
    let provider_session_id: Option<String> = Some("noninteractive-session-789".to_string());
    if let Some(ref sess_id) = provider_session_id {
        repo.update_provider_session_ref(
            &conv_id,
            &ProviderSessionRef {
                harness: AgentHarnessKind::Claude,
                provider_session_id: sess_id.clone(),
            },
        )
            .await
            .unwrap();
    }

    // Verify persisted
    let after = repo.get_by_id(&conv_id).await.unwrap().unwrap();
    assert_eq!(
        after.provider_session_id,
        Some("noninteractive-session-789".to_string())
    );
    assert_eq!(after.provider_harness, Some(AgentHarnessKind::Claude));
    assert_eq!(
        after.claude_session_id,
        Some("noninteractive-session-789".to_string()),
        "Non-interactive post-loop must persist the legacy Claude alias from StreamOutcome"
    );
}

// ========================================
// Helpers
// ========================================

/// Create a real stdin pipe via `cat` subprocess for testing InteractiveProcessRegistry.
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
