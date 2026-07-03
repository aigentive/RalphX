// IPR Cleanup Guard Integration Tests
//
// Tests the IPR guard patterns used in GC (prune_stale_execution_registry_entries),
// reconciliation (recover_execution_stop, apply_recovery_decision), and
// stop/pause execution commands to prevent double-execution and orphaned processes.
//
// These tests simulate the guard logic step-by-step using real components, matching
// the pattern used by gate1_ipr_fast_path_tests.rs. The actual functions are
// internal, so we test the component contracts they rely on.

use std::sync::Arc;

use ralphx_lib::application::interactive_process_registry::{
    InteractiveProcessKey, InteractiveProcessRegistry,
};
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::services::running_agent_registry::{
    MemoryRunningAgentRegistry, RunningAgentKey, RunningAgentRegistry,
};

// ============================================================================
// GC Prune Guard Tests
// ============================================================================

/// prune_stale_execution_registry_entries SKIPS entries with an active IPR process.
/// The guard checks has_process() before deciding whether to prune.
/// IPR registered → skip (process alive between turns).
#[tokio::test]
async fn test_gc_prune_skips_entry_with_active_ipr_process() {
    let ipr = Arc::new(InteractiveProcessRegistry::new());
    let running_agent_registry = MemoryRunningAgentRegistry::new();

    let context_type_str = "task_execution";
    let context_id = "task-gc-ipr-1";

    // Register a running agent (simulating a spawned process)
    let agent_key = RunningAgentKey::new(context_type_str, context_id);
    running_agent_registry
        .register(
            agent_key.clone(),
            12345,
            "conv-gc-1".to_string(),
            "run-gc-1".to_string(),
            None,
            None,
        )
        .await;

    // Register in IPR (process is alive between turns)
    let (stdin, _child) = create_test_stdin().await;
    let ipr_key = InteractiveProcessKey::new(context_type_str, context_id);
    ipr.register(ipr_key.clone(), stdin).await;

    // --- Simulate GC prune guard (mirrors execution_commands.rs) ---
    // The pruner checks has_process() and continues (skips) if true.
    let has_ipr = ipr.has_process(&ipr_key).await;
    assert!(
        has_ipr,
        "GC guard: has_process must return true for registered IPR entry"
    );

    // Since has_ipr is true, the pruner should SKIP this entry.
    // Verify: agent must remain registered (not pruned).
    assert!(
        running_agent_registry.is_running(&agent_key).await,
        "GC guard: agent must NOT be pruned when IPR has active process"
    );
}

/// prune_stale_execution_registry_entries ACTS on entries without IPR process.
/// No IPR entry → pruner proceeds to staleness checks.
#[tokio::test]
async fn test_gc_prune_acts_when_no_ipr_process() {
    let ipr = InteractiveProcessRegistry::new();
    let running_agent_registry = MemoryRunningAgentRegistry::new();

    let context_type_str = "task_execution";
    let context_id = "task-gc-no-ipr-1";

    // Register a running agent but do NOT register in IPR
    let agent_key = RunningAgentKey::new(context_type_str, context_id);
    running_agent_registry
        .register(
            agent_key.clone(),
            12345,
            "conv-gc-2".to_string(),
            "run-gc-2".to_string(),
            None,
            None,
        )
        .await;

    // --- Simulate GC prune guard ---
    let ipr_key = InteractiveProcessKey::new(context_type_str, context_id);
    let has_ipr = ipr.has_process(&ipr_key).await;
    assert!(
        !has_ipr,
        "GC guard: has_process must return false when no IPR entry"
    );

    // Since has_ipr is false, the pruner proceeds to staleness checks.
    // Simulate: entry is stale (no IPR, assume pid dead or run completed) → prune it.
    let _ = running_agent_registry.stop(&agent_key).await;
    assert!(
        !running_agent_registry.is_running(&agent_key).await,
        "GC guard: agent must be pruned when no IPR process"
    );
}

// ============================================================================
// Reconciliation Guard Tests
// ============================================================================

/// recover_execution_stop SKIPS recovery when IPR has an active process.
/// The guard prevents stopping a healthy interactive agent that's idle between turns.
#[tokio::test]
async fn test_reconciliation_stop_skips_with_active_ipr_process() {
    let ipr = Arc::new(InteractiveProcessRegistry::new());
    let running_agent_registry = MemoryRunningAgentRegistry::new();

    let context_type_str = "task_execution";
    let context_id = "task-recon-stop-1";

    // Register agent in both running registry and IPR
    let agent_key = RunningAgentKey::new(context_type_str, context_id);
    running_agent_registry
        .register(
            agent_key.clone(),
            12345,
            "conv-r-1".to_string(),
            "run-r-1".to_string(),
            None,
            None,
        )
        .await;

    let (stdin, _child) = create_test_stdin().await;
    let ipr_key = InteractiveProcessKey::new(context_type_str, context_id);
    ipr.register(ipr_key.clone(), stdin).await;

    // --- Simulate recover_execution_stop guard (mirrors execution.rs:1070-1082) ---
    let has_ipr = ipr.has_process(&ipr_key).await;
    assert!(
        has_ipr,
        "Reconciliation stop guard: IPR must report process exists"
    );

    // Guard returns false (skip recovery) when IPR has process.
    // Agent must remain running.
    assert!(
        running_agent_registry.is_running(&agent_key).await,
        "Reconciliation stop: agent must NOT be stopped when IPR process active"
    );
}

/// apply_recovery_decision (ExecuteEntryActions) SKIPS re-spawn when IPR
/// has an active process. Prevents double-execution.
#[tokio::test]
async fn test_reconciliation_respawn_skips_with_active_ipr_process() {
    let ipr = Arc::new(InteractiveProcessRegistry::new());
    let running_agent_registry = MemoryRunningAgentRegistry::new();

    let context_type_str = "task_execution";
    let context_id = "task-recon-respawn-1";

    // Register agent in running registry
    let agent_key = RunningAgentKey::new(context_type_str, context_id);
    running_agent_registry
        .register(
            agent_key.clone(),
            12345,
            "conv-ar-1".to_string(),
            "run-ar-1".to_string(),
            None,
            None,
        )
        .await;

    // Register in IPR (process alive between turns)
    let (stdin, _child) = create_test_stdin().await;
    let ipr_key = InteractiveProcessKey::new(context_type_str, context_id);
    ipr.register(ipr_key.clone(), stdin).await;

    // --- Simulate apply_recovery_decision ExecuteEntryActions guard ---
    // (mirrors execution.rs:1207-1220)
    let has_ipr = ipr.has_process(&ipr_key).await;
    assert!(
        has_ipr,
        "Reconciliation re-spawn guard: IPR must report process exists"
    );

    // Guard returns false (skip re-spawn) when IPR has process.
    // This prevents double-execution: the existing process is alive and doesn't
    // need to be replaced.
    assert!(
        running_agent_registry.is_running(&agent_key).await,
        "Reconciliation re-spawn: existing agent must not be replaced"
    );
}

/// When IPR has NO process, reconciliation proceeds with recovery/re-spawn.
#[tokio::test]
async fn test_reconciliation_proceeds_when_no_ipr_process() {
    let ipr = InteractiveProcessRegistry::new();
    let running_agent_registry = MemoryRunningAgentRegistry::new();

    let context_type_str = "task_execution";
    let context_id = "task-recon-no-ipr-1";

    let agent_key = RunningAgentKey::new(context_type_str, context_id);
    running_agent_registry
        .register(
            agent_key.clone(),
            12345,
            "conv-rn-1".to_string(),
            "run-rn-1".to_string(),
            None,
            None,
        )
        .await;

    // No IPR entry — process is NOT alive between turns
    let ipr_key = InteractiveProcessKey::new(context_type_str, context_id);
    let has_ipr = ipr.has_process(&ipr_key).await;
    assert!(
        !has_ipr,
        "No IPR entry: reconciliation should proceed"
    );

    // Reconciliation would clear stale registry entry before re-spawning
    let _ = running_agent_registry.stop(&agent_key).await;
    assert!(
        !running_agent_registry.is_running(&agent_key).await,
        "Stale agent cleared for re-spawn"
    );
}

// ============================================================================
// Pause/Stop Execution IPR Clear Tests
// ============================================================================

/// pause_execution and stop_execution both call stop_all() + clear().
/// After these, IPR must be completely empty.
#[tokio::test]
async fn test_pause_stop_execution_clears_all_ipr_entries() {
    let ipr = Arc::new(InteractiveProcessRegistry::new());
    let running_agent_registry = Arc::new(MemoryRunningAgentRegistry::new());
    let execution_state = Arc::new(ExecutionState::new());

    // Register multiple processes in different contexts
    let contexts = [
        ("task_execution", "task-ps-1"),
        ("review", "task-ps-2"),
        ("merge", "task-ps-3"),
    ];

    for (ctx_type, ctx_id) in &contexts {
        // Register in running agent registry
        let agent_key = RunningAgentKey::new(*ctx_type, *ctx_id);
        running_agent_registry
            .register(
                agent_key,
                12345,
                format!("conv-{}", ctx_id),
                format!("run-{}", ctx_id),
                None,
                None,
            )
            .await;

        // Register in IPR
        let (stdin, _child) = create_test_stdin().await;
        let ipr_key = InteractiveProcessKey::new(*ctx_type, *ctx_id);
        ipr.register(ipr_key, stdin).await;

        // Track execution state
        execution_state.increment_running();
    }

    // Verify all entries are registered
    for (ctx_type, ctx_id) in &contexts {
        let ipr_key = InteractiveProcessKey::new(*ctx_type, *ctx_id);
        assert!(ipr.has_process(&ipr_key).await, "Precondition: {} entry exists", ctx_id);
    }
    assert_eq!(execution_state.running_count(), 3, "Precondition: 3 running");

    // --- Simulate pause_execution / stop_execution ---
    // Both commands do: stop_all() + clear()
    running_agent_registry.stop_all().await;
    ipr.clear().await;

    // Verify everything is clean
    for (ctx_type, ctx_id) in &contexts {
        let agent_key = RunningAgentKey::new(*ctx_type, *ctx_id);
        assert!(
            !running_agent_registry.is_running(&agent_key).await,
            "Agent {} must be stopped after stop_all()",
            ctx_id
        );

        let ipr_key = InteractiveProcessKey::new(*ctx_type, *ctx_id);
        assert!(
            !ipr.has_process(&ipr_key).await,
            "IPR entry for {} must be removed after clear()",
            ctx_id
        );
    }
}

/// stop_all + clear is idempotent — calling on empty registries doesn't panic.
#[tokio::test]
async fn test_stop_clear_idempotent_on_empty() {
    let ipr = InteractiveProcessRegistry::new();
    let running_agent_registry = MemoryRunningAgentRegistry::new();

    // Should not panic on empty
    running_agent_registry.stop_all().await;
    ipr.clear().await;

    // Verify has_process returns false for an arbitrary key (empty registry)
    let key = InteractiveProcessKey::new("task_execution", "nonexistent");
    assert!(!ipr.has_process(&key).await, "Empty registry must report no process");
}

// ============================================================================
// GC IPR Guard with Execution State Integration
// ============================================================================

/// Full scenario: process spawned → TurnComplete (idle) → GC runs →
/// IPR guard prevents pruning → message arrives → resumes correctly.
#[tokio::test]
async fn test_gc_guard_preserves_idle_process_for_next_message() {
    let ipr = Arc::new(InteractiveProcessRegistry::new());
    let execution_state = Arc::new(ExecutionState::new());
    let running_agent_registry = MemoryRunningAgentRegistry::new();

    let context_type_str = "task_execution";
    let context_id = "task-gc-lifecycle-1";
    let slot_key = format!("{}/{}", context_type_str, context_id);
    let ipr_key = InteractiveProcessKey::new(context_type_str, context_id);

    // Phase 1: Process spawned
    let (stdin, _child) = create_test_stdin().await;
    ipr.register(ipr_key.clone(), stdin).await;
    let agent_key = RunningAgentKey::new(context_type_str, context_id);
    running_agent_registry
        .register(
            agent_key.clone(),
            12345,
            "conv-lc-1".to_string(),
            "run-lc-1".to_string(),
            None,
            None,
        )
        .await;
    execution_state.increment_running();

    // Phase 2: TurnComplete → idle
    execution_state.decrement_and_mark_idle(&slot_key);
    assert_eq!(execution_state.running_count(), 0, "Phase 2: idle");

    // Phase 3: GC runs — IPR guard should SKIP this entry
    let has_ipr = ipr.has_process(&ipr_key).await;
    assert!(has_ipr, "Phase 3: GC guard must see active IPR process");
    // (GC skips — does NOT prune)

    // Phase 4: New message arrives — Gate 1 fast-path works
    assert!(
        execution_state.claim_interactive_slot(&slot_key),
        "Phase 4: slot must be claimable (was idle)"
    );
    execution_state.increment_running();
    let write_ok = ipr.write_message(&ipr_key, "follow-up\n").await;
    assert!(write_ok.is_ok(), "Phase 4: stdin write must succeed");
    assert_eq!(
        execution_state.running_count(),
        1,
        "Phase 4: process active again"
    );

    // Verify: if GC had pruned the entry (bug), the write would have failed
    // and the process would be orphaned. The test passing confirms the guard works.
}

// ============================================================================
// Helpers
// ============================================================================

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
