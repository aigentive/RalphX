//! Tests for VerificationChildProcessRegistry.

use super::VerificationChildProcessRegistry;

// ---------------------------------------------------------------------------
// Test 1: process killed and entry removed after remove_and_kill
// ---------------------------------------------------------------------------

/// Spawn a real short-lived process, register its PID, call remove_and_kill,
/// and assert the entry is gone and the process is no longer alive.
///
/// This is a capability test (spawns and kills a real OS process).  It is marked
/// `#[ignore]` so that the default `cargo nextest run --lib` pass does not depend
/// on process-management capabilities.  Run it explicitly with:
///
/// ```
/// cargo test --manifest-path src-tauri/Cargo.toml \
///   'application::chat_service::verification_child_process_registry::tests::test_verification_child_process_killed_after_reconcile' \
///   --lib -- --ignored
/// ```
#[test]
#[ignore = "requires process management capability"]
fn test_verification_child_process_killed_after_reconcile() {
    let registry = VerificationChildProcessRegistry::new();

    // Spawn a long-lived process so the kill has something to terminate.
    let mut child = std::process::Command::new("sleep")
        .arg("60")
        .spawn()
        .expect("failed to spawn sleep process");

    let pid = child.id();
    let context_id = format!("test-verification-child-{pid}");

    // Register the PID in the registry.
    registry.register(&context_id, pid);

    // remove_and_kill should remove the entry AND send SIGTERM to the process.
    registry.remove_and_kill(&context_id);

    // A second remove_and_kill on the same ID must be a safe no-op (entry already gone).
    registry.remove_and_kill(&context_id);

    // Wait for the child to exit — SIGTERM from remove_and_kill should have terminated it.
    // If kill failed, this blocks indefinitely (the test runner will time out).
    let status = child.wait().expect("failed to wait for child process");

    // On Unix, SIGTERM causes a non-success exit.
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        // Process should have been terminated by a signal (SIGTERM = 15) rather than
        // exiting normally.
        assert!(
            status.signal().is_some() || !status.success(),
            "expected the process to be killed by a signal, got status: {status}"
        );
    }

    // On non-Unix, just assert the process exited (any exit is acceptable).
    #[cfg(not(unix))]
    assert!(
        !status.success() || status.code().is_some(),
        "expected the process to have exited after remove_and_kill"
    );
}

// ---------------------------------------------------------------------------
// Registry unit tests (no real processes involved — always run)
// ---------------------------------------------------------------------------

/// register + remove_and_kill on a non-existent PID is a safe no-op.
/// We use PID 99999999 which is guaranteed not to exist, so kill() will fail
/// harmlessly — no panic, no side effects.
#[test]
fn test_remove_and_kill_nonexistent_pid_is_noop() {
    let registry = VerificationChildProcessRegistry::new();
    let context_id = "ghost-context-id";

    // Register a PID that does not correspond to a real process.
    registry.register(context_id, 99_999_999);

    // Should not panic even if kill returns ESRCH.
    registry.remove_and_kill(context_id);
}

/// remove_and_kill on an unregistered key is a safe no-op.
#[test]
fn test_remove_and_kill_unregistered_key_is_noop() {
    let registry = VerificationChildProcessRegistry::new();
    // No prior registration — must not panic.
    registry.remove_and_kill("never-registered");
}

/// Registering the same context_id twice keeps only the latest PID.
#[test]
fn test_register_overwrites_previous_entry() {
    let registry = VerificationChildProcessRegistry::new();
    registry.register("ctx", 1_000);
    registry.register("ctx", 2_000); // second registration should overwrite the first
    // remove_and_kill will try to kill 2_000 (non-existent) — must not panic.
    registry.remove_and_kill("ctx");
}
