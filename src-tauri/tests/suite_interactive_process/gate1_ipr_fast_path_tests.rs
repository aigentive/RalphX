// Gate 1 IPR Fast-Path Integration Tests
//
// Tests the Gate 1 interactive process fast-path logic in send_message (mod.rs).
// When an interactive process is registered in IPR for a context, send_message
// should write to the existing process's stdin, reuse the existing conversation,
// and return the same conversation_id — NOT spawn a new process.
//
// These tests verify the component contracts that Gate 1 relies on:
// - InteractiveProcessRegistry: has_process, write_message, remove
// - ChatConversationRepository: get_active_for_context (reuse, not create)
// - ExecutionState: claim_interactive_slot + increment_running (burst prevention)
// - RunningAgentRegistry: try_register (Gate 2 dedup)
//
// The actual app chat runtime send_message path requires a Tauri Runtime, so these
// tests simulate the Gate 1 logic step-by-step using the real components, matching
// the pattern used by interactive_mode_integration.rs and team_nudge_running_count_tests.rs.

use std::sync::Arc;

use ralphx_lib::application::interactive_process_registry::{
    InteractiveProcessKey, InteractiveProcessRegistry,
};
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::entities::{ChatContextType, ChatConversation, TaskId};
use ralphx_lib::domain::repositories::ChatConversationRepository;
use ralphx_lib::domain::services::running_agent_registry::{
    MemoryRunningAgentRegistry, RunningAgentKey, RunningAgentRegistry,
};
use ralphx_lib::infrastructure::memory::MemoryChatConversationRepository;

// ============================================================================
// Test 1: Gate 1 HIT — IPR has entry, writes to stdin, reuses existing conversation
// ============================================================================

/// When IPR has a registered interactive process for a TaskExecution context,
/// the Gate 1 fast-path should:
/// 1. Detect the process via has_process
/// 2. Write the message to stdin via write_message
/// 3. Retrieve the EXISTING conversation via get_active_for_context
/// 4. Return the same conversation_id (not create a new one)
/// 5. NOT attempt Gate 2 (try_register) or Gate 3 (spawn)
#[tokio::test]
async fn test_gate1_hit_writes_stdin_and_reuses_existing_conversation() {
    // --- Setup: simulate a running interactive process ---
    let ipr = Arc::new(InteractiveProcessRegistry::new());
    let conversation_repo = Arc::new(MemoryChatConversationRepository::new());
    let execution_state = Arc::new(ExecutionState::new());
    let running_agent_registry = MemoryRunningAgentRegistry::new();

    let context_type_str = "task_execution";
    let context_id = "task-gate1-test-1";
    let task_id = TaskId::from_string(context_id.to_string());

    // 1. Create and persist the existing conversation (as if Gate 3 spawned earlier)
    let existing_conv = ChatConversation::new_task_execution(task_id);
    let existing_conv_id = existing_conv.id;
    conversation_repo.create(existing_conv).await.unwrap();

    // 2. Register the process in IPR (simulating a prior spawn)
    let (stdin, _child) = create_test_stdin().await;
    let ipr_key = InteractiveProcessKey::new(context_type_str, context_id);
    ipr.register(ipr_key.clone(), stdin).await;

    // 3. Register in running agent registry (normal state after spawn)
    let agent_key = RunningAgentKey {
        context_type: context_type_str.to_string(),
        context_id: context_id.to_string(),
    };
    running_agent_registry
        .register(
            agent_key.clone(),
            12345,
            existing_conv_id.as_str().to_string(),
            "run-1".to_string(),
            None,
            None,
        )
        .await;

    // 4. Simulate the process having completed a turn (idle between turns)
    let slot_key = format!("{}/{}", context_type_str, context_id);
    execution_state.increment_running();
    execution_state.decrement_and_mark_idle(&slot_key);
    assert_eq!(execution_state.running_count(), 0, "Precondition: lead is idle");

    // --- Simulate Gate 1 logic (mirrors mod.rs lines 574-695) ---

    // Step A: Check IPR for existing interactive process
    let has_ipr_entry = ipr.has_process(&ipr_key).await;
    assert!(
        has_ipr_entry,
        "Gate 1: IPR must report process exists for registered context"
    );

    // Step B: Build and write message to stdin
    let test_message = "Execute the task implementation";
    // In production: build_initial_prompt + format_stream_json_input
    // Here we just verify the write succeeds
    let write_result = ipr.write_message(&ipr_key, test_message).await;
    assert!(
        write_result.is_ok(),
        "Gate 1: write_message must succeed for registered process"
    );

    // Step C: Claim interactive slot + increment running (burst prevention)
    if execution_state.claim_interactive_slot(&slot_key) {
        execution_state.increment_running();
    }
    assert_eq!(
        execution_state.running_count(),
        1,
        "Gate 1: running_count must be 1 after successful claim+increment"
    );

    // Step D: Use EXISTING conversation (get_active_for_context, NOT get_or_create)
    let retrieved_conv = conversation_repo
        .get_active_for_context(ChatContextType::TaskExecution, context_id)
        .await
        .unwrap();

    assert!(
        retrieved_conv.is_some(),
        "Gate 1: get_active_for_context must find the existing conversation"
    );
    let retrieved_conv = retrieved_conv.unwrap();
    assert_eq!(
        retrieved_conv.id, existing_conv_id,
        "Gate 1: must return the SAME conversation_id (not create a new one)"
    );
    assert_eq!(
        retrieved_conv.context_type,
        ChatContextType::TaskExecution,
        "Gate 1: conversation context_type must match"
    );
    assert_eq!(
        retrieved_conv.context_id, context_id,
        "Gate 1: conversation context_id must match"
    );

    // Step E: Verify Gate 2 was NOT reached (running agent registry not touched)
    // The process was already registered, so try_register would fail if called.
    // Since Gate 1 succeeded, Gate 2 should never be reached.
    assert!(
        running_agent_registry.is_running(&agent_key).await,
        "Gate 1 hit: running agent registry entry should be untouched"
    );
}

// ============================================================================
// Test 2: Gate 1 MISS — IPR has NO entry, falls through to Gate 2/3
// ============================================================================

/// When IPR has no registered process for the context, Gate 1 should miss
/// and the logic should fall through to Gate 2 (try_register) and eventually
/// Gate 3 (spawn a new process).
#[tokio::test]
async fn test_gate1_miss_no_ipr_entry_falls_through() {
    let ipr = InteractiveProcessRegistry::new();
    let running_agent_registry = MemoryRunningAgentRegistry::new();

    let context_type_str = "task_execution";
    let context_id = "task-gate1-miss-1";

    let ipr_key = InteractiveProcessKey::new(context_type_str, context_id);

    // --- Gate 1: Check IPR ---
    let has_ipr_entry = ipr.has_process(&ipr_key).await;
    assert!(
        !has_ipr_entry,
        "Gate 1 miss: has_process must return false when no process registered"
    );

    // --- Gate 2: try_register should succeed (no existing agent) ---
    let agent_key = RunningAgentKey {
        context_type: context_type_str.to_string(),
        context_id: context_id.to_string(),
    };
    let register_result = running_agent_registry
        .try_register(
            agent_key.clone(),
            "new-conv-id".to_string(),
            "new-run-id".to_string(),
        )
        .await;
    assert!(
        register_result.is_ok(),
        "Gate 2: try_register must succeed when no agent is running"
    );

    // Verify we're now registered (Gate 3 would spawn the process next)
    assert!(
        running_agent_registry.is_running(&agent_key).await,
        "After Gate 2: agent must be registered in running registry"
    );
}

// ============================================================================
// Test 3: Gate 1 conversation reuse vs force_fresh divergence
// ============================================================================

/// Gate 1 MUST use get_active_for_context (returns existing conversation).
/// Gate 3 (spawn path) uses get_or_create_conversation which for TaskExecution
/// creates a FRESH conversation (force_fresh=true).
///
/// This test verifies that get_active_for_context returns the pre-existing
/// conversation, while a new conversation created afterward has a DIFFERENT id.
#[tokio::test]
async fn test_gate1_reuses_existing_conversation_vs_fresh_on_miss() {
    let conversation_repo = MemoryChatConversationRepository::new();
    let context_id = "task-gate1-reuse-1";
    let task_id = TaskId::from_string(context_id.to_string());

    // Create the "original" conversation (from initial spawn)
    let original_conv = ChatConversation::new_task_execution(task_id.clone());
    let original_conv_id = original_conv.id;
    conversation_repo.create(original_conv).await.unwrap();

    // Gate 1 path: get_active_for_context returns the existing one
    let gate1_conv = conversation_repo
        .get_active_for_context(ChatContextType::TaskExecution, context_id)
        .await
        .unwrap()
        .expect("Gate 1 must find existing conversation");
    assert_eq!(
        gate1_conv.id, original_conv_id,
        "Gate 1 must return the original conversation_id"
    );

    // Gate 3 path (simulated): creating a new conversation yields a DIFFERENT id
    let fresh_conv = ChatConversation::new_task_execution(task_id);
    let fresh_conv_id = fresh_conv.id;
    conversation_repo.create(fresh_conv).await.unwrap();

    assert_ne!(
        original_conv_id, fresh_conv_id,
        "Force-fresh conversation must have a different id than the original"
    );

    // After creating the fresh one, get_active_for_context returns the MOST RECENT
    // (max by created_at), which would be the fresh one — demonstrating why
    // Gate 1 must be checked BEFORE get_or_create_conversation
    let latest_conv = conversation_repo
        .get_active_for_context(ChatContextType::TaskExecution, context_id)
        .await
        .unwrap()
        .expect("Must find a conversation");
    assert_eq!(
        latest_conv.id, fresh_conv_id,
        "After force_fresh, get_active_for_context returns the newest — \
         Gate 1 must run BEFORE get_or_create to avoid creating an unwanted fresh conv"
    );
}

// ============================================================================
// Test 4: Gate 1 stdin write failure → remove IPR entry and fall through
// ============================================================================

/// When IPR has an entry but the stdin write fails (no process registered),
/// Gate 1 should:
/// 1. Remove the broken IPR entry
/// 2. Fall through to Gate 2/3 (normal spawn path)
///
/// Note: OS-level broken pipe detection is unreliable for small writes (kernel
/// buffers may absorb them even after the reader dies). Instead, we test the
/// write_message error path for a non-existent key, and the remove-on-failure
/// cleanup pattern that Gate 1 uses.
#[tokio::test]
async fn test_gate1_write_failure_removes_ipr_entry_and_falls_through() {
    let ipr = InteractiveProcessRegistry::new();
    let context_type_str = "task_execution";
    let context_id = "task-gate1-broken-1";

    let ipr_key = InteractiveProcessKey::new(context_type_str, context_id);

    // Register a real process, then immediately remove it to simulate stale state
    // This leaves the IPR empty for this key, so write_message will fail
    let (stdin, _child) = create_test_stdin().await;
    ipr.register(ipr_key.clone(), stdin).await;
    assert!(ipr.has_process(&ipr_key).await, "Precondition: entry exists");

    // Remove the entry (simulating what happens when IPR discovers a dead process)
    ipr.remove(&ipr_key).await;

    // Now verify the Gate 1 fallback pattern:
    // write_message fails for non-existent key
    let write_result = ipr.write_message(&ipr_key, "test message").await;
    assert!(
        write_result.is_err(),
        "Gate 1: write_message must fail when no process registered"
    );

    // After failure, ensure has_process returns false (fall through to Gate 2/3)
    assert!(
        !ipr.has_process(&ipr_key).await,
        "After removal: IPR must not report the broken process"
    );

    // Additionally test the full Gate 1 error-path pattern:
    // has_process → true, write_message → Err, remove (mirrors mod.rs lines 697-709)
    let (stdin2, _child2) = create_test_stdin().await;
    let broken_key = InteractiveProcessKey::new(context_type_str, "task-gate1-broken-2");
    ipr.register(broken_key.clone(), stdin2).await;

    // Simulate: has_process = true (stale check)
    assert!(ipr.has_process(&broken_key).await);

    // Simulate write failure by removing entry right before write (race-like scenario)
    // In production, this is the broken pipe case
    ipr.remove(&broken_key).await;
    let write_result = ipr.write_message(&broken_key, "message after removal").await;
    assert!(write_result.is_err(), "Write must fail after entry removed");

    // Post-failure cleanup: remove (idempotent — already removed)
    let removed = ipr.remove(&broken_key).await;
    assert!(
        removed.is_none(),
        "Remove after removal should return None (idempotent cleanup)"
    );
    assert!(
        !ipr.has_process(&broken_key).await,
        "Gate 1 fallback complete: no stale entries remain"
    );
}

// ============================================================================
// Test 5: Gate 1 burst prevention — multiple messages, single increment
// ============================================================================

/// When multiple messages arrive for the same interactive context in quick
/// succession (burst), only the first should claim the interactive slot.
/// Subsequent messages should still write to stdin but NOT double-increment
/// the running count.
#[tokio::test]
async fn test_gate1_burst_prevention_multiple_messages_single_increment() {
    let ipr = Arc::new(InteractiveProcessRegistry::new());
    let execution_state = Arc::new(ExecutionState::new());

    let context_type_str = "task_execution";
    let context_id = "task-gate1-burst-1";
    let slot_key = format!("{}/{}", context_type_str, context_id);

    // Register interactive process
    let (stdin, _child) = create_test_stdin().await;
    let ipr_key = InteractiveProcessKey::new(context_type_str, context_id);
    ipr.register(ipr_key.clone(), stdin).await;

    // Process finished a turn → idle
    execution_state.increment_running();
    execution_state.decrement_and_mark_idle(&slot_key);
    assert_eq!(execution_state.running_count(), 0);

    // Simulate 5 rapid Gate 1 hits (5 messages arriving nearly simultaneously)
    let mut successful_writes = 0;
    let mut successful_claims = 0;

    for i in 0..5 {
        // Each message hits Gate 1: write to stdin
        let msg = format!("burst message {}\n", i);
        if ipr.write_message(&ipr_key, &msg).await.is_ok() {
            successful_writes += 1;

            // Gate 1 burst prevention: claim_interactive_slot is atomic
            if execution_state.claim_interactive_slot(&slot_key) {
                execution_state.increment_running();
                successful_claims += 1;
            }
        }
    }

    assert_eq!(successful_writes, 5, "All 5 writes should succeed");
    assert_eq!(
        successful_claims, 1,
        "Only the first message should claim the slot (burst prevention)"
    );
    assert_eq!(
        execution_state.running_count(),
        1,
        "Running count must be 1 (not 5) after burst"
    );
}

// ============================================================================
// Test 6: Gate 1 with different context types — IPR isolation
// ============================================================================

/// IPR entries are keyed by (context_type, context_id). A TaskExecution entry
/// must not match an Ideation query for the same context_id.
#[tokio::test]
async fn test_gate1_ipr_context_type_isolation() {
    let ipr = InteractiveProcessRegistry::new();
    let context_id = "shared-id-123";

    // Register a TaskExecution process
    let (stdin, _child) = create_test_stdin().await;
    let task_exec_key = InteractiveProcessKey::new("task_execution", context_id);
    ipr.register(task_exec_key.clone(), stdin).await;

    // Verify TaskExecution key matches
    assert!(
        ipr.has_process(&task_exec_key).await,
        "TaskExecution key should match"
    );

    // Verify Ideation key does NOT match (different context_type, same context_id)
    let ideation_key = InteractiveProcessKey::new("ideation", context_id);
    assert!(
        !ipr.has_process(&ideation_key).await,
        "Ideation key must NOT match TaskExecution entry (context_type isolation)"
    );

    // Verify Merge key does NOT match
    let merge_key = InteractiveProcessKey::new("merge", context_id);
    assert!(
        !ipr.has_process(&merge_key).await,
        "Merge key must NOT match TaskExecution entry"
    );
}

// ============================================================================
// Test 7: Gate 1 full lifecycle — spawn → idle → Gate 1 hit → TurnComplete
// ============================================================================

/// End-to-end Gate 1 lifecycle:
/// 1. Initial spawn (Gate 3) creates conversation + registers in IPR
/// 2. TurnComplete → process goes idle
/// 3. New message → Gate 1 hits, writes to stdin, reuses conversation
/// 4. TurnComplete → process goes idle again
/// 5. Process exits → IPR entry removed
#[tokio::test]
async fn test_gate1_full_lifecycle_spawn_idle_hit_complete_exit() {
    let ipr = Arc::new(InteractiveProcessRegistry::new());
    let conversation_repo = Arc::new(MemoryChatConversationRepository::new());
    let execution_state = Arc::new(ExecutionState::new());

    let context_type_str = "task_execution";
    let context_id = "task-lifecycle-1";
    let task_id = TaskId::from_string(context_id.to_string());
    let slot_key = format!("{}/{}", context_type_str, context_id);
    let ipr_key = InteractiveProcessKey::new(context_type_str, context_id);

    // === Phase 1: Initial spawn (Gate 3 would do this) ===
    let original_conv = ChatConversation::new_task_execution(task_id);
    let original_conv_id = original_conv.id;
    conversation_repo.create(original_conv).await.unwrap();

    let (stdin, _child) = create_test_stdin().await;
    ipr.register(ipr_key.clone(), stdin).await;
    execution_state.increment_running();
    assert_eq!(execution_state.running_count(), 1, "Phase 1: process running");

    // === Phase 2: TurnComplete → idle ===
    execution_state.decrement_and_mark_idle(&slot_key);
    assert_eq!(execution_state.running_count(), 0, "Phase 2: process idle");
    assert!(execution_state.is_interactive_idle(&slot_key));

    // === Phase 3: New message → Gate 1 hit ===
    assert!(ipr.has_process(&ipr_key).await, "Phase 3: IPR has entry");

    let write_result = ipr.write_message(&ipr_key, "follow-up message").await;
    assert!(write_result.is_ok(), "Phase 3: stdin write succeeds");

    // Claim slot + increment
    assert!(execution_state.claim_interactive_slot(&slot_key));
    execution_state.increment_running();
    assert_eq!(execution_state.running_count(), 1, "Phase 3: process active again");

    // Reuse existing conversation
    let reused_conv = conversation_repo
        .get_active_for_context(ChatContextType::TaskExecution, context_id)
        .await
        .unwrap()
        .expect("Phase 3: must find existing conversation");
    assert_eq!(
        reused_conv.id, original_conv_id,
        "Phase 3: must reuse original conversation_id"
    );

    // === Phase 4: Second TurnComplete → idle again ===
    execution_state.decrement_and_mark_idle(&slot_key);
    assert_eq!(execution_state.running_count(), 0, "Phase 4: idle again");
    assert!(execution_state.is_interactive_idle(&slot_key));

    // === Phase 5: Process exits → cleanup ===
    ipr.remove(&ipr_key).await;
    assert!(!ipr.has_process(&ipr_key).await, "Phase 5: IPR entry removed");
    execution_state.remove_interactive_slot(&slot_key);
    assert!(
        !execution_state.is_interactive_idle(&slot_key),
        "Phase 5: slot cleaned up"
    );
}

// ============================================================================
// Test 8: Gate 1 with shared IPR — verify same Arc sees same entries
// ============================================================================

/// The shared IPR pattern (CRITICAL from MEMORY.md): all services must use
/// the same Arc<InteractiveProcessRegistry>. This test verifies that two
/// references to the same Arc see the same entries.
#[tokio::test]
async fn test_gate1_shared_ipr_arc_sees_same_entries() {
    let shared_ipr = Arc::new(InteractiveProcessRegistry::new());
    let ipr_ref1 = Arc::clone(&shared_ipr);
    let ipr_ref2 = Arc::clone(&shared_ipr);

    let context_type_str = "task_execution";
    let context_id = "task-shared-ipr-1";
    let ipr_key = InteractiveProcessKey::new(context_type_str, context_id);

    // Reference 1 registers the process
    let (stdin, _child) = create_test_stdin().await;
    ipr_ref1.register(ipr_key.clone(), stdin).await;

    // Reference 2 should see it (same underlying HashMap)
    assert!(
        ipr_ref2.has_process(&ipr_key).await,
        "Shared IPR: Arc clone must see entries registered by sibling reference"
    );

    // Reference 2 can write to it
    let write_result = ipr_ref2.write_message(&ipr_key, "hello from ref2").await;
    assert!(
        write_result.is_ok(),
        "Shared IPR: Arc clone must be able to write to process registered by sibling"
    );

    // Reference 1 removes it
    ipr_ref1.remove(&ipr_key).await;

    // Reference 2 should no longer see it
    assert!(
        !ipr_ref2.has_process(&ipr_key).await,
        "Shared IPR: removal via one Arc clone must be visible to the other"
    );
}

// ============================================================================
// Helpers
// ============================================================================

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
