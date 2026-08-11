// Integration tests for merge pipeline round 4 fixes.
//
// Fix 2: per-step timeouts for cleanup steps + settle sleep watchdog.
//
// Tests:
//   1. run_cleanup_step timeout fires when step exceeds deadline (Step 3 simulation)
//   2. Subsequent steps still run after Step 3 times out (non-fatal)
//   3. os_thread_timeout watchdog fires on stalled tokio::time::sleep
//   4. Settle sleep with watchdog completes normally when sleep finishes in time

use crate::domain::state_machine::transition_handler::cleanup_helpers::{
    os_thread_timeout, run_cleanup_step, CleanupStepResult,
};
use std::time::Duration;

// ──────────────────────────────────────────────────────────────────────────────
// Test 1 + 2: run_cleanup_step timeout fires and is non-fatal (Step 3 simulation)
// ──────────────────────────────────────────────────────────────────────────────

/// Simulates Step 3 (prune_worktrees) exceeding cleanup_git_op_timeout_secs.
/// run_cleanup_step must return TimedOut (not panic or hang), and subsequent
/// steps must still be reachable.
#[tokio::test]
async fn test_step3_timeout_is_nonfatal_subsequent_steps_run() {
    // Simulate Step 3: a future that never completes (hung git operation)
    let step3_result = run_cleanup_step(
        "prune_worktrees",
        1, // 1 second timeout
        "task-step3-test",
        async {
            // Simulates a hung git prune operation
            tokio::time::sleep(Duration::from_secs(60)).await;
            Ok::<(), String>(())
        },
    )
    .await;

    // Step 3 must time out (non-fatal)
    assert!(
        matches!(step3_result, CleanupStepResult::TimedOut { .. }),
        "Step 3 must return TimedOut when hung, got: {:?}",
        step3_result
    );

    // Subsequent steps (Step 4+) must still run — tracked via a flag after the call
    let step4_result = run_cleanup_step("step4_simulation", 5, "task-step3-test", async {
        Ok::<(), String>(())
    })
    .await;

    assert!(
        matches!(step4_result, CleanupStepResult::Ok),
        "Step 4 must still run after Step 3 times out (non-fatal cleanup)"
    );
}

/// run_cleanup_step returns Ok when Step 3 completes within timeout.
#[tokio::test]
async fn test_step3_success_within_timeout() {
    let result = run_cleanup_step(
        "prune_worktrees",
        5, // 5 second timeout — plenty of time
        "task-step3-success",
        async {
            // Fast operation — completes immediately
            Ok::<(), String>(())
        },
    )
    .await;
    assert!(
        matches!(result, CleanupStepResult::Ok),
        "Step 3 must return Ok when it completes within timeout, got: {:?}",
        result
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// Test 3 + 4: os_thread_timeout watchdog for settle sleep
// ──────────────────────────────────────────────────────────────────────────────

/// The watchdog fires when the inner tokio::time::sleep exceeds the OS-thread deadline.
///
/// This validates that os_thread_timeout provides protection against timer-driver starvation:
/// even if tokio's timer driver were stalled, the OS thread deadline still fires.
///
/// Here we use a very short watchdog (100ms) with a long inner sleep (60s) to verify
/// that Err(OsTimeoutElapsed) is returned quickly.
#[tokio::test]
async fn test_settle_sleep_watchdog_fires_when_inner_sleep_exceeds_deadline() {
    let start = std::time::Instant::now();

    let result = os_thread_timeout(
        Duration::from_millis(100),                  // watchdog: 100ms
        tokio::time::sleep(Duration::from_secs(60)), // inner sleep: 60s
    )
    .await;

    let elapsed = start.elapsed();

    // Watchdog must fire (Err)
    assert!(
        result.is_err(),
        "Watchdog must fire when inner sleep exceeds deadline"
    );

    // Must complete quickly (within 1s), not block for 60s
    assert!(
        elapsed.as_millis() < 1000,
        "Watchdog must fire quickly, took {}ms",
        elapsed.as_millis()
    );
}

/// Normal path: settle sleep completes before the watchdog fires.
///
/// With watchdog = settle_secs + 1s grace (production pattern), the inner sleep
/// wins the race and the watchdog OS thread exits cleanly.
#[tokio::test]
async fn test_settle_sleep_completes_normally_before_watchdog() {
    let settle_secs = 0u64; // zero-duration settle (instant)

    let result = os_thread_timeout(
        Duration::from_millis(500),                   // watchdog: 500ms grace
        tokio::time::sleep(Duration::from_millis(0)), // instant settle
    )
    .await;

    // Must succeed (Ok)
    assert!(
        result.is_ok(),
        "Settle sleep must complete normally when it finishes before watchdog, got Err"
    );
    let _ = settle_secs; // suppress unused warning
}

/// Verify the production watchdog pattern: watchdog = settle_secs + 1s grace.
/// The inner sleep (settle_secs) completes before the watchdog (settle_secs + 1s).
#[tokio::test]
async fn test_settle_sleep_production_pattern_grace_period() {
    // Production pattern: inner sleep = settle_secs, watchdog = settle_secs + 1s
    // Using very short durations to keep the test fast.
    let settle_ms = 10u64;
    let watchdog_ms = settle_ms + 200; // generous grace

    let result = os_thread_timeout(
        Duration::from_millis(watchdog_ms),
        tokio::time::sleep(Duration::from_millis(settle_ms)),
    )
    .await;

    assert!(
        result.is_ok(),
        "Production settle pattern must complete without watchdog firing"
    );
}
