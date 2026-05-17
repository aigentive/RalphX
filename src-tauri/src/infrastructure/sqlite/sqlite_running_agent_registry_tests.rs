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
    let existing = result.unwrap_err();
    assert_eq!(existing.pid, 12345);
    assert_eq!(existing.conversation_id, "conv-existing");

    // Original registration should be unchanged
    let info = registry.get(&key).await.unwrap();
    assert_eq!(info.pid, 12345);
}

#[tokio::test]
async fn test_try_register_then_update_agent_process() {
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
        .update_agent_process(
            &key,
            54321,
            "conv-1",
            "run-real",
            Some("/tmp/worktree".to_string()),
            Some(token.clone()),
            None,
        )
        .await
        .unwrap();

    // Should now have real PID, agent_run_id, and worktree
    let info = registry.get(&key).await.unwrap();
    assert_eq!(info.pid, 54321);
    assert_eq!(info.agent_run_id, "run-real");
    assert_eq!(info.worktree_path.as_deref(), Some("/tmp/worktree"));
    assert!(info.cancellation_token.is_some());
}

/// TOCTOU race: try_register → pruner deletes row → update_agent_process re-inserts.
/// Ensures the agent is tracked even if the placeholder was pruned mid-spawn.
#[tokio::test]
async fn test_toctou_pruner_deletes_placeholder_then_update_reinserts() {
    let db = setup_conn();
    let shared_conn = db.shared_conn();
    let registry = SqliteRunningAgentRegistry::new(Arc::clone(&shared_conn));
    let key = RunningAgentKey::new("task_execution", "task-toctou");

    // Step 1: Claim the slot (placeholder pid=0)
    let result = registry
        .try_register(key.clone(), "conv-toctou".to_string(), "run-toctou".to_string())
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

    // Step 3: update_agent_process should re-insert the full registration
    let token = CancellationToken::new();
    registry
        .update_agent_process(
            &key,
            12345,
            "conv-toctou",
            "run-real",
            Some("/tmp/worktree-toctou".to_string()),
            Some(token.clone()),
            None,
        )
        .await
        .unwrap();

    // Step 4: Verify the row was re-inserted with correct data
    assert!(registry.is_running(&key).await);
    let info = registry.get(&key).await.unwrap();
    assert_eq!(info.pid, 12345);
    assert_eq!(info.conversation_id, "conv-toctou");
    assert_eq!(info.agent_run_id, "run-real");
    assert_eq!(
        info.worktree_path.as_deref(),
        Some("/tmp/worktree-toctou")
    );
    assert!(info.cancellation_token.is_some());

    // Only one entry in the table
    let all = registry.list_all().await;
    assert_eq!(all.len(), 1);
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

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        registry.stop(&key),
    )
    .await;

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

    let task_exec = registry.list_by_context_type("task_execution").await.unwrap();
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
    let existing = r2.unwrap_err();
    assert_eq!(existing.pid, 0); // Still placeholder
    assert_eq!(existing.conversation_id, "conv-1");
}
