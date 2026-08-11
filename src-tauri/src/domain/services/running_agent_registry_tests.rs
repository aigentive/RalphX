use super::*;
use std::collections::HashSet;
use std::sync::{Arc, Mutex as StdMutex};

#[derive(Clone, Default)]
struct FakeProcessState {
    alive: Arc<StdMutex<HashSet<u32>>>,
    kills: Arc<StdMutex<Vec<u32>>>,
    immediate_kills: Arc<StdMutex<Vec<u32>>>,
    remove_on_kill: bool,
}

impl FakeProcessState {
    fn with_alive(pids: &[u32]) -> Self {
        Self {
            alive: Arc::new(StdMutex::new(pids.iter().copied().collect())),
            kills: Arc::new(StdMutex::new(Vec::new())),
            immediate_kills: Arc::new(StdMutex::new(Vec::new())),
            remove_on_kill: false,
        }
    }

    fn removing_on_kill(mut self) -> Self {
        self.remove_on_kill = true;
        self
    }

    fn process_ops(&self) -> ProcessOps {
        let alive_for_check = Arc::clone(&self.alive);
        let alive_for_kill = Arc::clone(&self.alive);
        let alive_for_immediate = Arc::clone(&self.alive);
        let kills = Arc::clone(&self.kills);
        let immediate_kills = Arc::clone(&self.immediate_kills);
        let remove_on_kill = self.remove_on_kill;

        ProcessOps {
            is_alive: Arc::new(move |pid| alive_for_check.lock().unwrap().contains(&pid)),
            kill: Arc::new(move |pid| {
                kills.lock().unwrap().push(pid);
                if remove_on_kill {
                    alive_for_kill.lock().unwrap().remove(&pid);
                }
            }),
            kill_immediate: Arc::new(move |pid| {
                immediate_kills.lock().unwrap().push(pid);
                alive_for_immediate.lock().unwrap().remove(&pid);
            }),
        }
    }

    fn kills(&self) -> Vec<u32> {
        self.kills.lock().unwrap().clone()
    }

    fn immediate_kills(&self) -> Vec<u32> {
        self.immediate_kills.lock().unwrap().clone()
    }
}

#[test]
fn coverage_regression_storage_failure_is_not_reported_as_an_occupied_slot() {
    let error = TryRegisterError::Storage("database unavailable".to_string());

    assert!(error.occupied().is_none());
}

#[tokio::test]
async fn test_register_and_get() {
    let registry = MemoryRunningAgentRegistry::new();
    let key = RunningAgentKey::new("ideation", "session-123");

    registry
        .register(
            key.clone(),
            12345,
            "conv-abc".to_string(),
            "run-xyz".to_string(),
            None,
            None,
        )
        .await;

    let info = registry.get(&key).await;
    assert!(info.is_some());
    let info = info.unwrap();
    assert_eq!(info.pid, 12345);
    assert_eq!(info.conversation_id, "conv-abc");
    assert_eq!(info.agent_run_id, "run-xyz");
}

#[tokio::test]
async fn test_register_with_cancellation_token() {
    let registry =
        MemoryRunningAgentRegistry::with_process_ops(FakeProcessState::default().process_ops());
    let key = RunningAgentKey::new("task", "task-cancel");
    let token = CancellationToken::new();

    registry
        .register(
            key.clone(),
            99999,
            "conv-ct".to_string(),
            "run-ct".to_string(),
            None,
            Some(token.clone()),
        )
        .await;

    let info = registry.get(&key).await.unwrap();
    assert!(info.cancellation_token.is_some());
    assert!(!token.is_cancelled());

    // Stop should cancel token
    let _ = registry.stop(&key).await;
    assert!(token.is_cancelled());
}

#[tokio::test]
async fn test_stop_uses_process_ops_and_cancels_token() {
    let state = FakeProcessState::with_alive(&[7777]);
    let registry = MemoryRunningAgentRegistry::with_process_ops(state.process_ops());
    let key = RunningAgentKey::new("task", "task-stop");
    let token = CancellationToken::new();

    registry
        .register(
            key.clone(),
            7777,
            "conv-stop".to_string(),
            "run-stop".to_string(),
            None,
            Some(token.clone()),
        )
        .await;

    let stopped = registry.stop(&key).await.unwrap().unwrap();
    assert_eq!(stopped.pid, 7777);
    assert!(token.is_cancelled());
    assert_eq!(state.kills(), vec![7777]);
    assert!(!registry.is_running(&key).await);
}

#[tokio::test]
async fn test_is_running() {
    let registry = MemoryRunningAgentRegistry::new();
    let key = RunningAgentKey::new("task", "task-123");

    assert!(!registry.is_running(&key).await);

    registry
        .register(
            key.clone(),
            12345,
            "conv-abc".to_string(),
            "run-xyz".to_string(),
            None,
            None,
        )
        .await;

    assert!(registry.is_running(&key).await);
}

#[tokio::test]
async fn test_unregister() {
    let registry = MemoryRunningAgentRegistry::new();
    let key = RunningAgentKey::new("project", "proj-123");

    registry
        .register(
            key.clone(),
            12345,
            "conv-abc".to_string(),
            "run-xyz".to_string(),
            None,
            None,
        )
        .await;

    let info = registry.unregister(&key, "run-xyz").await;
    assert!(info.is_some());

    assert!(!registry.is_running(&key).await);

    // Double unregister should return None
    let info = registry.unregister(&key, "run-xyz").await;
    assert!(info.is_none());
}

#[tokio::test]
async fn test_register_stops_orphaned_process() {
    let state = FakeProcessState::with_alive(&[4242]);
    let registry = MemoryRunningAgentRegistry::with_process_ops(state.process_ops());
    let key = RunningAgentKey::new("task", "task-orphan");
    let old_token = CancellationToken::new();
    let old_pid = 4242;

    registry
        .register(
            key.clone(),
            old_pid,
            "conv-old".to_string(),
            "run-old".to_string(),
            None,
            Some(old_token.clone()),
        )
        .await;

    assert!(!old_token.is_cancelled());

    // Re-register with a new PID — should stop the old process
    registry
        .register(
            key.clone(),
            99999,
            "conv-new".to_string(),
            "run-new".to_string(),
            None,
            None,
        )
        .await;

    // Old token should be cancelled
    assert!(old_token.is_cancelled());
    assert_eq!(state.kills(), vec![old_pid]);

    // New registration should be active
    let info = registry.get(&key).await.unwrap();
    assert_eq!(info.pid, 99999);
    assert_eq!(info.conversation_id, "conv-new");
}

#[tokio::test]
async fn test_list_all() {
    let registry = MemoryRunningAgentRegistry::new();

    registry
        .register(
            RunningAgentKey::new("ideation", "session-1"),
            111,
            "conv-1".to_string(),
            "run-1".to_string(),
            None,
            None,
        )
        .await;

    registry
        .register(
            RunningAgentKey::new("task", "task-2"),
            222,
            "conv-2".to_string(),
            "run-2".to_string(),
            None,
            None,
        )
        .await;

    let all = registry.list_all().await;
    assert_eq!(all.len(), 2);
}

#[tokio::test]
async fn test_try_register_succeeds_when_empty() {
    let registry = MemoryRunningAgentRegistry::new();
    let key = RunningAgentKey::new("task_execution", "task-fresh");

    let result = registry
        .try_register(key.clone(), "conv-1".to_string(), "run-1".to_string())
        .await;

    assert!(result.is_ok());
    assert!(registry.is_running(&key).await);

    // Placeholder should have pid=0
    let info = registry.get(&key).await.unwrap();
    assert_eq!(info.pid, 0);
    assert_eq!(info.conversation_id, "conv-1");
    assert_eq!(info.agent_run_id, "run-1");
}

#[tokio::test]
async fn test_try_register_fails_when_occupied() {
    let registry = MemoryRunningAgentRegistry::new();
    let key = RunningAgentKey::new("task_execution", "task-occupied");

    // First registration succeeds
    registry
        .register(
            key.clone(),
            12345,
            "conv-existing".to_string(),
            "run-existing".to_string(),
            None,
            None,
        )
        .await;

    // try_register should fail
    let result = registry
        .try_register(key.clone(), "conv-new".to_string(), "run-new".to_string())
        .await;

    assert!(result.is_err());
    let existing = result.unwrap_err().occupied().cloned().unwrap();
    assert_eq!(existing.pid, 12345);
    assert_eq!(existing.conversation_id, "conv-existing");

    // Original registration should be unchanged
    let info = registry.get(&key).await.unwrap();
    assert_eq!(info.pid, 12345);
}

#[tokio::test]
async fn test_try_register_then_attach_process() {
    let registry = MemoryRunningAgentRegistry::new();
    let key = RunningAgentKey::new("task_execution", "task-update");
    let token = CancellationToken::new();

    // Claim the slot
    let result = registry
        .try_register(key.clone(), "conv-1".to_string(), "run-1".to_string())
        .await;
    assert!(result.is_ok());

    // Placeholder has pid=0
    let info = registry.get(&key).await.unwrap();
    assert_eq!(info.pid, 0);
    assert!(info.worktree_path.is_none());
    assert!(info.cancellation_token.is_none());

    // Update with real process details
    registry
        .attach_process(
            &key,
            "run-1",
            54321,
            Some("/tmp/worktree".to_string()),
            Some(token.clone()),
            None,
        )
        .await
        .unwrap();

    // Should now have real PID, agent_run_id, and worktree
    let info = registry.get(&key).await.unwrap();
    assert_eq!(info.pid, 54321);
    assert_eq!(info.agent_run_id, "run-1");
    assert_eq!(info.worktree_path.as_deref(), Some("/tmp/worktree"));
    assert!(info.cancellation_token.is_some());
}

/// A pruner that removes the reservation also removes launch authority. Attachment
/// must fail closed instead of reviving the deleted claim.
#[tokio::test]
async fn test_attach_process_does_not_revive_deleted_reservation() {
    let registry = MemoryRunningAgentRegistry::new();
    let key = RunningAgentKey::new("task_execution", "task-toctou");

    // Step 1: Claim the slot
    let result = registry
        .try_register(
            key.clone(),
            "conv-toctou".to_string(),
            "run-toctou".to_string(),
        )
        .await;
    assert!(result.is_ok());
    assert!(registry.is_running(&key).await);

    // Step 2: Simulate pruner removing the entry
    registry.unregister(&key, "run-toctou").await;
    assert!(!registry.is_running(&key).await);

    // Step 3: attachment must report the lost claim and leave the slot empty.
    let token = CancellationToken::new();
    let result = registry
        .attach_process(
            &key,
            "run-toctou",
            12345,
            Some("/tmp/worktree".to_string()),
            Some(token.clone()),
            None,
        )
        .await
        .unwrap();

    assert_eq!(result, AttachProcessResult::ClaimLost);
    assert!(!registry.is_running(&key).await);
    assert!(!token.is_cancelled());
}

#[tokio::test]
async fn test_stale_owner_cannot_attach_refresh_stop_or_cleanup_replacement() {
    let state = FakeProcessState::with_alive(&[54321]);
    let registry = MemoryRunningAgentRegistry::with_process_ops(state.process_ops());
    let key = RunningAgentKey::new("task_execution", "task-owner-cas");
    let replacement_token = CancellationToken::new();

    registry
        .register(
            key.clone(),
            54321,
            "conv-new".to_string(),
            "run-new".to_string(),
            None,
            Some(replacement_token.clone()),
        )
        .await;

    let attached = registry
        .attach_process(&key, "run-old", 12345, None, None, None)
        .await
        .unwrap();
    assert_eq!(attached, AttachProcessResult::ClaimLost);
    assert!(!registry
        .renew_reservation(&key, "run-old", chrono::Utc::now())
        .await
        .unwrap());
    assert!(registry
        .stop_if_owned(&key, "run-old")
        .await
        .unwrap()
        .is_none());
    assert!(registry
        .cleanup_stale_entry(&key, "run-old")
        .await
        .unwrap()
        .is_none());

    let current = registry.get(&key).await.unwrap();
    assert_eq!(current.agent_run_id, "run-new");
    assert_eq!(current.pid, 54321);
    assert!(!replacement_token.is_cancelled());
    assert!(state.kills().is_empty());
}

#[tokio::test]
async fn test_try_register_cleanup_on_spawn_failure() {
    let registry = MemoryRunningAgentRegistry::new();
    let key = RunningAgentKey::new("task_execution", "task-fail");

    // Claim the slot
    let result = registry
        .try_register(key.clone(), "conv-1".to_string(), "run-1".to_string())
        .await;
    assert!(result.is_ok());
    assert!(registry.is_running(&key).await);

    // Simulate spawn failure: unregister to release the slot
    registry.unregister(&key, "run-1").await;

    // Slot should be free again
    assert!(!registry.is_running(&key).await);

    // Another try_register should succeed now
    let result = registry
        .try_register(key.clone(), "conv-2".to_string(), "run-2".to_string())
        .await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_try_register_blocks_concurrent_claim() {
    let registry = MemoryRunningAgentRegistry::new();
    let key = RunningAgentKey::new("task_execution", "task-race");

    // First try_register claims the slot
    let r1 = registry
        .try_register(key.clone(), "conv-1".to_string(), "run-1".to_string())
        .await;
    assert!(r1.is_ok());

    // Second try_register should fail (slot is claimed even with pid=0)
    let r2 = registry
        .try_register(key.clone(), "conv-2".to_string(), "run-2".to_string())
        .await;
    assert!(r2.is_err());
    let existing = r2.unwrap_err().occupied().cloned().unwrap();
    assert_eq!(existing.pid, 0); // Still placeholder
    assert_eq!(existing.conversation_id, "conv-1");
}

#[tokio::test]
async fn test_stop_all_started_before_preserves_current_boot_entries() {
    let state = FakeProcessState::with_alive(&[4101, 4102]);
    let registry = MemoryRunningAgentRegistry::with_process_ops(state.process_ops());
    let old_key = RunningAgentKey::new("project", "old-conversation");
    let current_key = RunningAgentKey::new("project", "current-conversation");
    let old_token = CancellationToken::new();
    let current_token = CancellationToken::new();

    registry
        .register(
            old_key.clone(),
            4101,
            "conv-old".to_string(),
            "run-old".to_string(),
            None,
            Some(old_token.clone()),
        )
        .await;
    tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    let boot_cutoff = chrono::Utc::now();
    tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    registry
        .register(
            current_key.clone(),
            4102,
            "conv-current".to_string(),
            "run-current".to_string(),
            None,
            Some(current_token.clone()),
        )
        .await;

    let stopped = registry.stop_all_started_before(boot_cutoff).await;

    assert_eq!(stopped, vec![old_key.clone()]);
    assert_eq!(state.kills(), vec![4101]);
    assert!(old_token.is_cancelled());
    assert!(!current_token.is_cancelled());
    assert!(!registry.is_running(&old_key).await);
    assert!(registry.is_running(&current_key).await);
}

#[tokio::test]
#[ignore = "requires lsof process-enumeration capability"]
async fn test_kill_worktree_processes_async_completes_within_timeout() {
    // Use a temp dir that exists but has no processes — lsof should return quickly
    let tmp_dir = tempfile::TempDir::new().expect("create temp dir");
    let tmp = tmp_dir.path().join("ralphx_test_lsof_async");
    // codeql[rust/path-injection]
    let _ = std::fs::create_dir_all(&tmp);

    let start = std::time::Instant::now();
    kill_worktree_processes_async(&tmp, 5, false).await;
    let elapsed = start.elapsed();

    // Should complete well within 5s since no heavy scanning needed
    assert!(
        elapsed.as_secs() < 5,
        "Expected completion within timeout, took {:?}",
        elapsed
    );

    // codeql[rust/path-injection]
    let _ = std::fs::remove_dir_all(&tmp);
}

#[tokio::test]
#[ignore = "requires lsof process-enumeration capability"]
async fn test_kill_worktree_processes_async_nonexistent_path() {
    // Non-existent path — should not panic, just log debug
    let bogus = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../.artifacts/tests/ralphx_test_nonexistent_worktree_path_12345");
    kill_worktree_processes_async(&bogus, 2, false).await;
    // If we get here without panic, the test passes
}

#[tokio::test]
#[ignore = "requires lsof process-enumeration capability"]
async fn test_kill_worktree_processes_async_timeout_returns_quickly() {
    // Test that the timeout mechanism works by using a very short timeout (1s).
    // Even if lsof somehow takes longer, we should return within ~1s.
    let tmp_dir = tempfile::TempDir::new().expect("create temp dir");
    let tmp = tmp_dir.path().join("ralphx_test_lsof_timeout");
    // codeql[rust/path-injection]
    let _ = std::fs::create_dir_all(&tmp);

    let start = std::time::Instant::now();
    kill_worktree_processes_async(&tmp, 1, false).await;
    let elapsed = start.elapsed();

    // Must return within timeout + small overhead (async dispatch overhead)
    assert!(
        elapsed.as_secs() < 3,
        "Expected return within ~1s timeout, took {:?}",
        elapsed
    );

    // codeql[rust/path-injection]
    let _ = std::fs::remove_dir_all(&tmp);
}

/// Verify that kill_worktree_processes_async WAITS for processes to die
/// (via await_process_death) instead of fire-and-forget SIGTERM.
#[tokio::test]
#[ignore = "requires real process spawn/kill capability"]
async fn test_kill_worktree_processes_async_waits_for_process_exit() {
    let dir = tempfile::TempDir::new().unwrap();
    let dir_path = dir.path().to_path_buf();

    // Spawn a long-running process with cwd = dir so lsof can discover it
    let mut child = std::process::Command::new("sleep")
        .arg("60")
        .current_dir(&dir_path)
        .spawn()
        .unwrap();
    let pid = child.id();

    // Give lsof time to see the process
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert!(is_process_alive(pid));

    kill_worktree_processes_async(&dir_path, 10, false).await;

    // Reap the zombie so is_process_alive correctly reports dead
    let _ = child.wait();
    assert!(
        !is_process_alive(pid),
        "Process {} should be dead after kill_worktree_processes_async returns",
        pid
    );
}

// ===== Non-blocking process check tests (nix-based) =====

#[test]
fn test_is_process_alive_current_process() {
    // Current process is always alive
    let pid = std::process::id();
    assert!(is_process_alive(pid), "Current process should be alive");
}

#[test]
fn test_is_process_alive_nonexistent_pid() {
    // PID 999999 is almost certainly not running
    assert!(
        !is_process_alive(999999),
        "Non-existent PID should report dead"
    );
}

#[test]
fn test_is_process_alive_pid_zero() {
    // PID 0 is special (process group) and should always return false
    assert!(!is_process_alive(0), "PID 0 should always return false");
}

#[test]
#[ignore = "requires real child process capability"]
fn test_is_process_alive_spawned_child() {
    let mut child = std::process::Command::new("sleep")
        .arg("60")
        .spawn()
        .expect("spawn sleep");
    let pid = child.id();

    assert!(is_process_alive(pid), "Spawned child should be alive");

    // Kill and reap
    let _ = child.kill();
    let _ = child.wait();

    assert!(
        !is_process_alive(pid),
        "Killed child should be dead after wait()"
    );
}

#[test]
#[ignore = "requires real child process capability"]
fn test_kill_process_immediate_kills_child() {
    let mut child = std::process::Command::new("sleep")
        .arg("60")
        .spawn()
        .expect("spawn sleep");
    let pid = child.id();

    assert!(is_process_alive(pid));

    kill_process_immediate(pid);

    // Reap the zombie
    let _ = child.wait();

    assert!(
        !is_process_alive(pid),
        "Process should be dead after kill_process_immediate"
    );
}

#[test]
#[ignore = "requires real child process capability"]
fn test_kill_process_immediate_sigterm_resistant() {
    // Spawn a process that ignores SIGTERM
    let mut child = std::process::Command::new("bash")
        .args(["-c", "trap '' TERM; sleep 60"])
        .spawn()
        .expect("spawn bash");
    let pid = child.id();

    // Give bash time to set up the trap
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert!(is_process_alive(pid));

    // kill_process_immediate sends SIGKILL which cannot be trapped
    kill_process_immediate(pid);

    // Reap the zombie
    let _ = child.wait();

    assert!(
        !is_process_alive(pid),
        "SIGTERM-resistant process should be dead after SIGKILL"
    );
}

#[tokio::test]
#[ignore = "requires real child process capability"]
async fn test_await_process_death_immediate_kill_fast() {
    // Spawn a SIGTERM-resistant process
    let mut child = std::process::Command::new("bash")
        .args(["-c", "trap '' TERM; sleep 60"])
        .spawn()
        .expect("spawn bash");
    let pid = child.id();

    std::thread::sleep(std::time::Duration::from_millis(200));
    assert!(is_process_alive(pid));

    let start = std::time::Instant::now();
    let _survivors = await_process_death(
        &[pid],
        std::time::Duration::from_secs(5),
        true, // immediate_kill = true → SIGKILL right away
    )
    .await;
    let elapsed = start.elapsed();

    // Should complete very quickly (< 2s) since SIGKILL is immediate
    assert!(
        elapsed.as_secs() < 2,
        "immediate_kill should not wait for SIGTERM timeout, took {:?}",
        elapsed
    );

    // Note: _survivors may contain pid because our test process is the parent,
    // so the killed child becomes a zombie until we call wait(). In real usage,
    // the caller doesn't own the child handle so zombies don't occur.
    // Reap the zombie and verify the process is truly dead.
    let _ = child.wait();
    assert!(
        !is_process_alive(pid),
        "Process should be dead after SIGKILL + reap"
    );
}

#[tokio::test]
#[ignore = "requires real child process capability"]
async fn test_await_process_death_graceful_exit() {
    // Spawn a short-lived process
    let mut child = std::process::Command::new("sleep")
        .arg("0.1")
        .spawn()
        .expect("spawn sleep");
    let pid = child.id();

    // Wait for it to finish naturally
    let _ = child.wait();

    let survivors = await_process_death(&[pid], std::time::Duration::from_secs(2), false).await;

    assert!(
        survivors.is_empty(),
        "Already-dead process should not be a survivor"
    );
}

#[tokio::test]
async fn test_await_process_death_immediate_mode_uses_process_ops() {
    let state = FakeProcessState::with_alive(&[8001]).removing_on_kill();
    let survivors = await_process_death_with_process_ops(
        &[8001],
        std::time::Duration::from_millis(50),
        true,
        &state.process_ops(),
    )
    .await;

    assert!(survivors.is_empty());
    assert_eq!(state.immediate_kills(), vec![8001]);
}

#[tokio::test]
async fn test_await_process_death_escalates_after_timeout_with_process_ops() {
    let state = FakeProcessState::with_alive(&[8002]);
    let survivors = await_process_death_with_process_ops(
        &[8002],
        std::time::Duration::from_millis(50),
        false,
        &state.process_ops(),
    )
    .await;

    assert!(survivors.is_empty());
    assert!(
        state.kills().is_empty(),
        "await_process_death only observes prior SIGTERM handling"
    );
    assert_eq!(state.immediate_kills(), vec![8002]);
}

// ===== Regression tests for pkill .spawn() fix (7dbc2f32) =====
// Before the fix, kill_process() and kill_process_immediate() used
// std::process::Command::new("pkill").output() which blocks the tokio
// worker thread, starving tokio::time::timeout. The fix changed to .spawn().

/// Key regression test: kill_process() must not block the tokio runtime.
/// Before the fix, .output() would block and tokio::time::timeout could
/// never fire. With .spawn(), the call returns nearly instantly.
#[tokio::test]
#[ignore = "requires real child process capability"]
async fn test_kill_process_does_not_block_tokio_timeout() {
    let mut child = std::process::Command::new("sleep")
        .arg("60")
        .spawn()
        .expect("spawn sleep");
    let pid = child.id();

    assert!(is_process_alive(pid));

    let start = std::time::Instant::now();
    let result = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        kill_process(pid);
    })
    .await;
    let elapsed = start.elapsed();

    // The timeout must NOT fire — kill_process should return almost instantly
    assert!(
        result.is_ok(),
        "kill_process() blocked the tokio runtime — timeout fired after {:?}",
        elapsed
    );
    assert!(
        elapsed.as_millis() < 1000,
        "kill_process() took too long ({:?}), should return nearly instantly",
        elapsed
    );

    // Reap the zombie
    let _ = child.wait();
}

/// Same regression test for kill_process_immediate() — must not block tokio.
#[tokio::test]
#[ignore = "requires real child process capability"]
async fn test_kill_process_immediate_does_not_block_tokio_timeout() {
    let mut child = std::process::Command::new("sleep")
        .arg("60")
        .spawn()
        .expect("spawn sleep");
    let pid = child.id();

    assert!(is_process_alive(pid));

    let start = std::time::Instant::now();
    let result = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        kill_process_immediate(pid);
    })
    .await;
    let elapsed = start.elapsed();

    assert!(
        result.is_ok(),
        "kill_process_immediate() blocked the tokio runtime — timeout fired after {:?}",
        elapsed
    );
    assert!(
        elapsed.as_millis() < 1000,
        "kill_process_immediate() took too long ({:?}), should return nearly instantly",
        elapsed
    );

    // Reap the zombie
    let _ = child.wait();
}

/// Verify that kill_process_immediate (which uses .spawn() for pkill +
/// process group SIGKILL) still kills child processes despite fire-and-forget.
/// The process group kill `kill(-(pid), SIGKILL)` is the reliable mechanism
/// for child killing; pkill -P is supplementary and platform-dependent.
#[tokio::test]
#[ignore = "requires real process-group capability"]
async fn test_kill_process_immediate_kills_process_group_children() {
    use std::os::unix::process::CommandExt;

    // Spawn a parent as its own process group leader (setpgid(0,0))
    // so that kill(-(pid), SIGKILL) targets this group.
    // In real usage, agent processes are session leaders with PID=PGID.
    let mut parent = unsafe {
        std::process::Command::new("bash")
            .args(["-c", "sleep 60 & sleep 60 & wait"])
            .pre_exec(|| {
                nix::unistd::setpgid(nix::unistd::Pid::from_raw(0), nix::unistd::Pid::from_raw(0))
                    .map_err(|e| std::io::Error::other(e))
            })
            .spawn()
            .expect("spawn bash parent")
    };
    let parent_pid = parent.id();

    // Give bash time to spawn children
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    // Find child PIDs via pgrep -P
    let pgrep_output = std::process::Command::new("pgrep")
        .args(["-P", &parent_pid.to_string()])
        .output()
        .expect("pgrep");
    let child_pids: Vec<u32> = String::from_utf8_lossy(&pgrep_output.stdout)
        .lines()
        .filter_map(|l| l.trim().parse::<u32>().ok())
        .collect();

    assert!(
        !child_pids.is_empty(),
        "Parent bash should have spawned at least one child process"
    );
    for &cpid in &child_pids {
        assert!(
            is_process_alive(cpid),
            "Child {cpid} should be alive before kill"
        );
    }

    // kill_process_immediate uses .spawn() for pkill + process group SIGKILL
    kill_process_immediate(parent_pid);

    // Give fire-and-forget pkill + process group kill time to propagate
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Reap the parent
    let _ = parent.wait();

    // Parent should be dead
    assert!(
        !is_process_alive(parent_pid),
        "Parent process should be dead after kill_process_immediate"
    );

    // Children should also be dead (via process group SIGKILL)
    for &cpid in &child_pids {
        assert!(
            !is_process_alive(cpid),
            "Child process {cpid} should be dead — process group SIGKILL should have killed it"
        );
    }
}

/// Verify SIGKILL escalation for processes that ignore SIGTERM.
/// This test takes ~5-6s due to the SIGTERM wait window.
#[tokio::test]
#[ignore = "requires real process spawn/kill capability"]
async fn test_kill_worktree_processes_async_escalates_to_sigkill() {
    let dir = tempfile::TempDir::new().unwrap();
    let dir_path = dir.path().to_path_buf();

    // Spawn a bash process that IGNORES SIGTERM and keeps cwd = dir.
    // The while loop restarts sleep after pkill kills its child, keeping bash alive.
    let mut child = std::process::Command::new("bash")
        .args([
            "-c",
            &format!(
                "trap '' TERM; cd '{}'; while true; do sleep 60 2>/dev/null; done",
                dir_path.display()
            ),
        ])
        .spawn()
        .unwrap();
    let pid = child.id();

    // Give bash time to set up the trap and cd
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert!(is_process_alive(pid));

    let start = std::time::Instant::now();
    kill_worktree_processes_async(&dir_path, 15, false).await;
    let elapsed = start.elapsed();

    // Should take ≥4s (SIGTERM wait window) but <15s (outer lsof timeout)
    assert!(
        elapsed.as_secs() >= 4,
        "Should wait for SIGTERM window before escalating to SIGKILL, took {:?}",
        elapsed
    );
    assert!(
        elapsed.as_secs() < 15,
        "Should not hit outer timeout, took {:?}",
        elapsed
    );

    // Reap the zombie
    let _ = child.wait();
    assert!(
        !is_process_alive(pid),
        "SIGTERM-resistant process {} should be dead after SIGKILL escalation",
        pid
    );
}

// --- MemoryRunningAgentRegistry: list_by_context_type tests ---

#[tokio::test]
async fn test_memory_list_by_context_type_returns_only_matching() {
    let registry = MemoryRunningAgentRegistry::new();

    registry
        .set_running(RunningAgentKey::new("ideation", "s1"))
        .await;
    registry
        .set_running(RunningAgentKey::new("ideation", "s2"))
        .await;
    registry
        .set_running(RunningAgentKey::new("task_execution", "t1"))
        .await;

    let ideation = registry.list_by_context_type("ideation").await.unwrap();
    assert_eq!(ideation.len(), 2);
    for (key, _) in &ideation {
        assert_eq!(key.context_type, "ideation");
    }

    let task_exec = registry
        .list_by_context_type("task_execution")
        .await
        .unwrap();
    assert_eq!(task_exec.len(), 1);
    assert_eq!(task_exec[0].0.context_id, "t1");
}

#[tokio::test]
async fn test_memory_list_by_context_type_returns_empty_when_no_match() {
    let registry = MemoryRunningAgentRegistry::new();
    registry
        .set_running(RunningAgentKey::new("task_execution", "t1"))
        .await;

    let result = registry.list_by_context_type("ideation").await.unwrap();
    assert!(result.is_empty());
}

#[tokio::test]
async fn test_memory_list_by_context_type_empty_registry() {
    let registry = MemoryRunningAgentRegistry::new();
    let result = registry.list_by_context_type("ideation").await.unwrap();
    assert!(result.is_empty());
}

/// Rapid-restart dedup: two concurrent recovery attempts for the same ideation session_id.
///
/// Scenario: App restarts twice rapidly. Both recovery loops try to spawn an agent
/// for the same ideation session. `try_register()` is the atomic guard that prevents
/// the second spawn from proceeding — the first call succeeds and claims the slot,
/// the second call returns `Err` with the existing placeholder info (pid=0).
#[tokio::test]
async fn test_rapid_restart_dedup_try_register_ideation() {
    let registry = MemoryRunningAgentRegistry::new();
    let session_id = "session-rapid-restart-test";
    let key = RunningAgentKey::new("ideation", session_id);

    // First recovery attempt claims the slot.
    let r1 = registry
        .try_register(
            key.clone(),
            "conv-ideation-1".to_string(),
            "run-1".to_string(),
        )
        .await;
    assert!(r1.is_ok(), "First recovery attempt should claim the slot");

    // Placeholder is registered with pid=0 before the real agent process starts.
    let placeholder = registry.get(&key).await;
    assert!(placeholder.is_some());
    assert_eq!(placeholder.unwrap().pid, 0);

    // Second concurrent recovery attempt for the same session_id must be rejected.
    let r2 = registry
        .try_register(
            key.clone(),
            "conv-ideation-2".to_string(),
            "run-2".to_string(),
        )
        .await;
    assert!(
        r2.is_err(),
        "Second recovery attempt for the same ideation session_id must be rejected by try_register()"
    );

    // Confirm the existing placeholder's conversation_id is preserved (first wins).
    let existing = r2.unwrap_err().occupied().cloned().unwrap();
    assert_eq!(
        existing.conversation_id, "conv-ideation-1",
        "First registration must be preserved; second attempt must not overwrite it"
    );
    assert_eq!(
        existing.pid, 0,
        "Slot is still the placeholder (no real process spawned yet)"
    );
}

#[tokio::test]
async fn memory_quiesce_retains_exact_owner_until_cleanup_finishes() {
    let registry = MemoryRunningAgentRegistry::new();
    let key = RunningAgentKey::new("merge", "task-cleanup");
    registry
        .register(
            key.clone(),
            999_999,
            "conversation".to_string(),
            "run-owned".to_string(),
            Some("/tmp/worktree".to_string()),
            None,
        )
        .await;

    assert!(registry
        .quiesce_if_owned(&key, "run-stale")
        .await
        .unwrap()
        .is_none());
    assert_eq!(registry.get(&key).await.unwrap().pid, 999_999);

    let owned = registry
        .quiesce_if_owned(&key, "run-owned")
        .await
        .unwrap()
        .expect("owner should quiesce");
    assert_eq!(owned.agent_run_id, "run-owned");
    assert_eq!(registry.get(&key).await.unwrap().pid, 0);
    assert!(registry
        .try_register(key.clone(), "replacement".into(), "run-new".into())
        .await
        .is_err());

    registry.unregister(&key, "run-owned").await;
    assert!(registry
        .try_register(key, "replacement".into(), "run-new".into())
        .await
        .is_ok());
}
