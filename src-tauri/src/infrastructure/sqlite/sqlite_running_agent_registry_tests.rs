use super::*;
use crate::testing::SqliteTestDb;

fn setup_conn() -> SqliteTestDb {
    SqliteTestDb::new("sqlite-running-agent-registry")
}

#[tokio::test]
async fn test_register_and_get() {
    let db = setup_conn();
    let registry = SqliteRunningAgentRegistry::new(db.shared_conn());
    let key = RunningAgentKey::new("ideation", "session-123");

    registry
        .register(
            key.clone(),
            12345,
            "conv-abc".to_string(),
            "run-xyz".to_string(),
            Some("/tmp/worktree".to_string()),
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
    let db = setup_conn();
    let registry = SqliteRunningAgentRegistry::new(db.shared_conn());
    let key = RunningAgentKey::new("task", "task-cancel");
    let token = CancellationToken::new();

    registry
        .register(
            key.clone(),
            99999,
            "conv-ct".to_string(),
            "run-ct".to_string(),
            Some("/tmp/ct".to_string()),
            Some(token.clone()),
        )
        .await;

    let info = registry.get(&key).await.unwrap();
    assert!(info.cancellation_token.is_some());
    assert!(!token.is_cancelled());

    // Unregister should return token
    let info = registry.unregister(&key, "run-ct").await.unwrap();
    assert!(info.cancellation_token.is_some());
}

#[tokio::test]
async fn test_unregister() {
    let db = setup_conn();
    let registry = SqliteRunningAgentRegistry::new(db.shared_conn());
    let key = RunningAgentKey::new("task", "task-456");

    registry
        .register(
            key.clone(),
            999,
            "conv-1".to_string(),
            "run-1".to_string(),
            Some("/tmp/worktree".to_string()),
            None,
        )
        .await;

    let info = registry.unregister(&key, "run-1").await;
    assert!(info.is_some());
    assert_eq!(info.unwrap().pid, 999);

    // Should be gone
    assert!(!registry.is_running(&key).await);

    // Double unregister returns None
    let info = registry.unregister(&key, "run-1").await;
    assert!(info.is_none());
}

#[tokio::test]
async fn test_is_running() {
    let db = setup_conn();
    let registry = SqliteRunningAgentRegistry::new(db.shared_conn());
    let key = RunningAgentKey::new("review", "review-789");

    assert!(!registry.is_running(&key).await);

    registry
        .register(
            key.clone(),
            111,
            "conv-x".to_string(),
            "run-x".to_string(),
            Some("/tmp/worktree".to_string()),
            None,
        )
        .await;

    assert!(registry.is_running(&key).await);
}

#[tokio::test]
async fn test_list_all() {
    let db = setup_conn();
    let registry = SqliteRunningAgentRegistry::new(db.shared_conn());

    registry
        .register(
            RunningAgentKey::new("ideation", "s1"),
            100,
            "c1".to_string(),
            "r1".to_string(),
            Some("/tmp/k1".to_string()),
            None,
        )
        .await;
    registry
        .register(
            RunningAgentKey::new("task", "t1"),
            200,
            "c2".to_string(),
            "r2".to_string(),
            Some("/tmp/k2".to_string()),
            None,
        )
        .await;

    let all = registry.list_all().await;
    assert_eq!(all.len(), 2);
}

#[tokio::test]
async fn test_stop_all_clears_table() {
    let db = setup_conn();
    let registry = SqliteRunningAgentRegistry::new(db.shared_conn());

    registry
        .register(
            RunningAgentKey::new("a", "1"),
            10001,
            "c".to_string(),
            "r".to_string(),
            Some("/tmp/a".to_string()),
            None,
        )
        .await;
    registry
        .register(
            RunningAgentKey::new("b", "2"),
            10002,
            "c".to_string(),
            "r".to_string(),
            Some("/tmp/b".to_string()),
            None,
        )
        .await;

    let stopped = registry.stop_all().await;
    assert_eq!(stopped.len(), 2);

    // Table should be empty
    let all = registry.list_all().await;
    assert!(all.is_empty());
}

#[tokio::test]
async fn test_stop_all_started_before_preserves_current_boot_entry() {
    let db = setup_conn();
    let registry = SqliteRunningAgentRegistry::new(db.shared_conn());
    let old_key = RunningAgentKey::new("project", "old-conversation");
    let current_key = RunningAgentKey::new("project", "current-conversation");
    let current_token = CancellationToken::new();

    registry
        .register(
            old_key.clone(),
            999_991,
            "conv-old".to_string(),
            "run-old".to_string(),
            None,
            None,
        )
        .await;
    tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    let boot_cutoff = chrono::Utc::now();
    tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    registry
        .register(
            current_key.clone(),
            999_992,
            "conv-current".to_string(),
            "run-current".to_string(),
            None,
            Some(current_token.clone()),
        )
        .await;

    let stopped = registry.stop_all_started_before(boot_cutoff).await;

    assert_eq!(stopped, vec![old_key.clone()]);
    assert!(!registry.is_running(&old_key).await);
    assert!(registry.is_running(&current_key).await);
    assert!(!current_token.is_cancelled());
}

#[tokio::test]
async fn test_register_replaces_existing() {
    let db = setup_conn();
    let registry = SqliteRunningAgentRegistry::new(db.shared_conn());
    let key = RunningAgentKey::new("task", "task-1");

    registry
        .register(
            key.clone(),
            100,
            "conv-old".to_string(),
            "run-old".to_string(),
            Some("/tmp/old".to_string()),
            None,
        )
        .await;
    registry
        .register(
            key.clone(),
            200,
            "conv-new".to_string(),
            "run-new".to_string(),
            Some("/tmp/new".to_string()),
            None,
        )
        .await;

    let info = registry.get(&key).await.unwrap();
    assert_eq!(info.pid, 200);
    assert_eq!(info.conversation_id, "conv-new");

    // Only one entry
    let all = registry.list_all().await;
    assert_eq!(all.len(), 1);
}

#[tokio::test]
async fn test_register_stops_orphaned_process() {
    let db = setup_conn();
    let registry = SqliteRunningAgentRegistry::new(db.shared_conn());
    let key = RunningAgentKey::new("task", "task-orphan");
    let old_token = CancellationToken::new();

    // Spawn a real process so is_process_alive returns true
    let mut child = std::process::Command::new("sleep")
        .arg("60")
        .spawn()
        .expect("spawn sleep");
    let old_pid = child.id();

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
    assert!(is_process_alive(old_pid));

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

    // Reap the zombie (SIGTERM was sent, wait collects exit status)
    let _ = child.wait();
    assert!(!is_process_alive(old_pid));

    // New registration should be active
    let info = registry.get(&key).await.unwrap();
    assert_eq!(info.pid, 99999);
    assert_eq!(info.conversation_id, "conv-new");

    // Only one entry
    let all = registry.list_all().await;
    assert_eq!(all.len(), 1);
}

#[tokio::test]
async fn test_try_register_succeeds_when_empty() {
    let db = setup_conn();
    let registry = SqliteRunningAgentRegistry::new(db.shared_conn());
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
    let db = setup_conn();
    let registry = SqliteRunningAgentRegistry::new(db.shared_conn());
    let key = RunningAgentKey::new("task_execution", "task-occupied");

    // First registration via register()
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
    let db = setup_conn();
    let registry = SqliteRunningAgentRegistry::new(db.shared_conn());
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

/// A deleted reservation is a lost claim. Attachment must not reinsert it.
#[tokio::test]
async fn test_attach_process_does_not_revive_deleted_reservation() {
    let db = setup_conn();
    let shared_conn = db.shared_conn();
    let registry = SqliteRunningAgentRegistry::new(Arc::clone(&shared_conn));
    let key = RunningAgentKey::new("task_execution", "task-toctou");

    // Step 1: Claim the slot (placeholder pid=0)
    let result = registry
        .try_register(
            key.clone(),
            "conv-toctou".to_string(),
            "run-toctou".to_string(),
        )
        .await;
    assert!(result.is_ok());
    assert!(registry.is_running(&key).await);

    // Step 2: Simulate pruner deleting the placeholder row
    {
        let conn = shared_conn.lock().await;
        conn.execute(
            "DELETE FROM running_agents WHERE context_type = ?1 AND context_id = ?2",
            rusqlite::params!["task_execution", "task-toctou"],
        )
        .unwrap();
    }
    assert!(!registry.is_running(&key).await);

    // Step 3: attachment reports claim loss and does not install the token.
    let token = CancellationToken::new();
    let result = registry
        .attach_process(
            &key,
            "run-toctou",
            12345,
            Some("/tmp/worktree-toctou".to_string()),
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
async fn test_wrong_owner_cannot_remove_replacement_token_or_row() {
    let db = setup_conn();
    let registry = SqliteRunningAgentRegistry::new(db.shared_conn());
    let key = RunningAgentKey::new("task_execution", "task-owner-cas");
    let replacement_token = CancellationToken::new();

    registry
        .register(
            key.clone(),
            2_000_001,
            "conv-new".to_string(),
            "run-new".to_string(),
            None,
            Some(replacement_token.clone()),
        )
        .await;

    assert!(registry.unregister(&key, "run-old").await.is_none());
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
    assert!(current.cancellation_token.is_some());
    assert!(!replacement_token.is_cancelled());
}

#[tokio::test]
async fn test_try_register_cleanup_on_spawn_failure() {
    let db = setup_conn();
    let registry = SqliteRunningAgentRegistry::new(db.shared_conn());
    let key = RunningAgentKey::new("task_execution", "task-fail");

    // Claim the slot
    let result = registry
        .try_register(key.clone(), "conv-1".to_string(), "run-1".to_string())
        .await;
    assert!(result.is_ok());
    assert!(registry.is_running(&key).await);

    // Simulate spawn failure: unregister to release the slot
    registry.unregister(&key, "run-1").await;
    assert!(!registry.is_running(&key).await);

    // Another try_register should succeed now
    let result = registry
        .try_register(key.clone(), "conv-2".to_string(), "run-2".to_string())
        .await;
    assert!(result.is_ok());
}

/// RC-A regression: stop() must NOT call kill_worktree_processes (blocking lsof +D).
///
/// Pre-fix: SqliteRunningAgentRegistry::stop() called kill_worktree_processes(&worktree)
/// synchronously — blocking the Tokio thread via std::process::Command::output().
/// When pointed at a large directory tree, lsof +D could block for minutes, rendering
/// the agent_stop_timeout_secs guard in pre_merge_cleanup ineffective.
///
/// Post-fix: stop() only cancels the token + sends SIGTERM. Worktree lsof scanning
/// is handled exclusively by kill_worktree_processes_async in pre_merge_cleanup step 0b.
#[tokio::test]
async fn test_stop_completes_without_blocking_lsof_scan() {
    let db = setup_conn();
    let registry = SqliteRunningAgentRegistry::new(db.shared_conn());
    let key = RunningAgentKey::new("review", "task-rc-a-stop");

    // Register with worktree_path pointing at /tmp (exists).
    // Old code: triggered lsof +D /tmp — could take 10+ seconds.
    // New code: skips lsof entirely — must complete in well under 1s.
    registry
        .register(
            key.clone(),
            2_000_000, // non-existent PID — kill_process handles "No such process" gracefully
            "conv-rca".to_string(),
            "run-rca".to_string(),
            Some("/tmp".to_string()),
            None,
        )
        .await;

    let result = tokio::time::timeout(std::time::Duration::from_secs(1), registry.stop(&key)).await;

    assert!(
        result.is_ok(),
        "stop() timed out after 1s — blocking lsof scan may still be present"
    );
    assert!(result.unwrap().is_ok());
    assert!(!registry.is_running(&key).await);
}

// --- list_by_context_type tests ---

#[tokio::test]
async fn test_list_by_context_type_returns_only_matching() {
    let db = setup_conn();
    let registry = SqliteRunningAgentRegistry::new(db.shared_conn());

    registry
        .register(
            RunningAgentKey::new("ideation", "s1"),
            100,
            "c1".to_string(),
            "r1".to_string(),
            None,
            None,
        )
        .await;
    registry
        .register(
            RunningAgentKey::new("ideation", "s2"),
            200,
            "c2".to_string(),
            "r2".to_string(),
            None,
            None,
        )
        .await;
    registry
        .register(
            RunningAgentKey::new("task_execution", "t1"),
            300,
            "c3".to_string(),
            "r3".to_string(),
            None,
            None,
        )
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
async fn test_list_by_context_type_returns_empty_when_no_match() {
    let db = setup_conn();
    let registry = SqliteRunningAgentRegistry::new(db.shared_conn());

    registry
        .register(
            RunningAgentKey::new("task_execution", "t1"),
            100,
            "c1".to_string(),
            "r1".to_string(),
            None,
            None,
        )
        .await;

    let result = registry.list_by_context_type("ideation").await.unwrap();
    assert!(result.is_empty());
}

#[tokio::test]
async fn test_list_by_context_type_returns_full_info() {
    let db = setup_conn();
    let registry = SqliteRunningAgentRegistry::new(db.shared_conn());

    registry
        .register(
            RunningAgentKey::new("ideation", "session-abc"),
            54321,
            "conv-xyz".to_string(),
            "run-abc".to_string(),
            Some("/tmp/worktree".to_string()),
            None,
        )
        .await;

    let result = registry.list_by_context_type("ideation").await.unwrap();
    assert_eq!(result.len(), 1);
    let (key, info) = &result[0];
    assert_eq!(key.context_type, "ideation");
    assert_eq!(key.context_id, "session-abc");
    assert_eq!(info.pid, 54321);
    assert_eq!(info.conversation_id, "conv-xyz");
    assert_eq!(info.agent_run_id, "run-abc");
    assert_eq!(info.worktree_path.as_deref(), Some("/tmp/worktree"));
}

#[tokio::test]
async fn test_try_register_blocks_concurrent_claim() {
    let db = setup_conn();
    let registry = SqliteRunningAgentRegistry::new(db.shared_conn());
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
async fn sqlite_quiesce_retains_exact_owner_until_cleanup_finishes() {
    let db = setup_conn();
    let registry = SqliteRunningAgentRegistry::new(db.shared_conn());
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

#[tokio::test]
async fn coverage_regression_lease_updates_require_the_exact_reservation_owner() {
    let db = setup_conn();
    let registry = SqliteRunningAgentRegistry::new(db.shared_conn());
    let key = RunningAgentKey::new("project", "lease-owner");
    registry
        .try_register(key.clone(), "conversation".into(), "run-owned".into())
        .await
        .unwrap();
    let original = registry.get(&key).await.unwrap().last_active_at.unwrap();

    let heartbeat = original + chrono::Duration::seconds(5);
    assert!(!registry
        .update_heartbeat(&key, "run-stale", heartbeat)
        .await
        .unwrap());
    assert_eq!(
        registry.get(&key).await.unwrap().last_active_at,
        Some(original)
    );
    assert!(registry
        .update_heartbeat(&key, "run-owned", heartbeat)
        .await
        .unwrap());
    assert_eq!(
        registry.get(&key).await.unwrap().last_active_at,
        Some(heartbeat)
    );

    let renewal = heartbeat + chrono::Duration::seconds(5);
    assert!(!registry
        .renew_reservation(&key, "run-stale", renewal)
        .await
        .unwrap());
    assert!(registry
        .renew_reservation(&key, "run-owned", renewal)
        .await
        .unwrap());
    assert_eq!(
        registry.get(&key).await.unwrap().last_active_at,
        Some(renewal)
    );
}

#[tokio::test]
async fn coverage_regression_owned_tokens_survive_reads_and_cancel_on_quiesce() {
    let db = setup_conn();
    let registry = SqliteRunningAgentRegistry::new(db.shared_conn());
    let key = RunningAgentKey::new("merge", "token-owner");
    let token = CancellationToken::new();
    registry
        .register(
            key.clone(),
            2_000_001,
            "conversation".into(),
            "run-owned".into(),
            None,
            Some(token.clone()),
        )
        .await;

    let listed = registry.list_all().await;
    assert_eq!(listed.len(), 1);
    assert!(listed[0].1.cancellation_token.is_some());

    let occupied = registry
        .try_register(key.clone(), "replacement".into(), "run-new".into())
        .await
        .unwrap_err()
        .occupied()
        .cloned()
        .unwrap();
    assert_eq!(occupied.agent_run_id, "run-owned");
    assert!(occupied.cancellation_token.is_some());

    let quiesced = registry
        .quiesce_if_owned(&key, "run-owned")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(quiesced.agent_run_id, "run-owned");
    assert!(token.is_cancelled());
    assert_eq!(registry.get(&key).await.unwrap().pid, 0);
}

#[tokio::test]
async fn coverage_regression_stale_cleanup_deletes_and_returns_only_the_dead_owner() {
    let db = setup_conn();
    let registry = SqliteRunningAgentRegistry::new(db.shared_conn());
    let key = RunningAgentKey::new("task_execution", "dead-owner");
    let token = CancellationToken::new();
    registry
        .register(
            key.clone(),
            2_000_002,
            "conversation".into(),
            "run-owned".into(),
            None,
            Some(token),
        )
        .await;

    let removed = registry
        .cleanup_stale_entry(&key, "run-owned")
        .await
        .unwrap()
        .expect("dead exact owner should be removed");

    assert_eq!(removed.agent_run_id, "run-owned");
    assert!(removed.cancellation_token.is_some());
    assert!(registry.get(&key).await.is_none());
}
