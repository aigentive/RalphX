use chrono::Utc;

use super::*;

fn make_info(request_id: &str, tool_name: &str) -> PendingPermissionInfo {
    PendingPermissionInfo {
        request_id: request_id.to_string(),
        tool_name: tool_name.to_string(),
        tool_input: serde_json::json!({}),
        context: None,
        agent_type: None,
        task_id: None,
        context_type: None,
        context_id: None,
        created_at: Utc::now().to_rfc3339(),
    }
}

#[tokio::test]
async fn test_permission_state_new() {
    let state = PermissionState::new();
    let pending = state.pending.lock().await;
    assert!(pending.is_empty());
}

#[tokio::test]
async fn test_permission_state_default() {
    let state = PermissionState::default();
    let pending = state.pending.lock().await;
    assert!(pending.is_empty());
}

#[tokio::test]
async fn test_permission_decision_clone() {
    let decision = PermissionDecision {
        decision: "allow".to_string(),
        message: Some("User approved".to_string()),
    };
    let cloned = decision.clone();
    assert_eq!(cloned.decision, "allow");
    assert_eq!(cloned.message, Some("User approved".to_string()));
}

#[tokio::test]
async fn test_permission_decision_serialization() {
    let decision = PermissionDecision {
        decision: "deny".to_string(),
        message: None,
    };
    let json = serde_json::to_string(&decision).unwrap();
    assert!(json.contains("\"decision\":\"deny\""));

    let deserialized: PermissionDecision = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.decision, "deny");
    assert!(deserialized.message.is_none());
}

#[tokio::test]
async fn test_pending_permission_info_serialization() {
    let info = PendingPermissionInfo {
        request_id: "req-123".to_string(),
        tool_name: "Bash".to_string(),
        tool_input: serde_json::json!({"command": "ls -la"}),
        context: Some("User wants to list files".to_string()),
        agent_type: None,
        task_id: None,
        context_type: None,
        context_id: None,
        created_at: "2026-07-10T00:00:00+00:00".to_string(),
    };
    let json = serde_json::to_string(&info).unwrap();
    assert!(json.contains("\"request_id\":\"req-123\""));
    assert!(json.contains("\"tool_name\":\"Bash\""));
    assert!(json.contains("\"command\":\"ls -la\""));
}

#[tokio::test]
async fn test_pending_permission_info_with_identity() {
    let info = PendingPermissionInfo {
        request_id: "req-456".to_string(),
        tool_name: "Edit".to_string(),
        tool_input: serde_json::json!({"path": "/foo/bar.rs"}),
        context: Some("Editing a file".to_string()),
        agent_type: Some("ralphx-execution-worker".to_string()),
        task_id: Some("task-abc".to_string()),
        context_type: Some("task_execution".to_string()),
        context_id: Some("task-abc".to_string()),
        created_at: "2026-07-10T00:00:00+00:00".to_string(),
    };
    let json = serde_json::to_string(&info).unwrap();
    assert!(json.contains("\"agent_type\":\"ralphx-execution-worker\""));
    assert!(json.contains("\"task_id\":\"task-abc\""));
    assert!(json.contains("\"context_type\":\"task_execution\""));
    assert!(json.contains("\"context_id\":\"task-abc\""));

    let deserialized: PendingPermissionInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(
        deserialized.agent_type,
        Some("ralphx-execution-worker".to_string())
    );
    assert_eq!(deserialized.task_id, Some("task-abc".to_string()));
    assert_eq!(
        deserialized.context_type,
        Some("task_execution".to_string())
    );
    assert_eq!(deserialized.context_id, Some("task-abc".to_string()));
}

#[tokio::test]
async fn test_pending_permission_info_partial_identity() {
    // Only some identity fields set
    let info = PendingPermissionInfo {
        request_id: "req-789".to_string(),
        tool_name: "Bash".to_string(),
        tool_input: serde_json::json!({}),
        context: None,
        agent_type: Some("ralphx-execution-reviewer".to_string()),
        task_id: None,
        context_type: None,
        context_id: None,
        created_at: "2026-07-10T00:00:00+00:00".to_string(),
    };
    let json = serde_json::to_string(&info).unwrap();
    let deserialized: PendingPermissionInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(
        deserialized.agent_type,
        Some("ralphx-execution-reviewer".to_string())
    );
    assert!(deserialized.task_id.is_none());
    assert!(deserialized.context_type.is_none());
    assert!(deserialized.context_id.is_none());
}

#[tokio::test]
async fn test_register_and_resolve_permission() {
    let state = PermissionState::new();

    // Register a pending permission using the new struct-based API
    let request_id = "test-request-123".to_string();
    let info = PendingPermissionInfo {
        request_id: request_id.clone(),
        tool_name: "Bash".to_string(),
        tool_input: serde_json::json!({"command": "rm -rf /tmp/test"}),
        context: Some("Cleanup temp files".to_string()),
        agent_type: None,
        task_id: None,
        context_type: None,
        context_id: None,
        created_at: "2026-07-10T12:00:00Z".to_string(),
    };
    state.register(info).await;

    // Verify it's in pending and subscribe to the channel
    let rx = {
        let pending = state.pending.lock().await;
        assert!(pending.contains_key(&request_id));
        let request = pending.get(&request_id).unwrap();
        assert_eq!(request.info.tool_name, "Bash");
        request.sender.subscribe()
    };

    // Resolve with a decision
    let resolved = state
        .resolve(
            &request_id,
            PermissionDecision {
                decision: "allow".to_string(),
                message: Some("Approved by user".to_string()),
            },
        )
        .await;
    assert!(resolved);

    // Check the decision was received
    let decision = rx.borrow().clone();
    assert!(decision.is_some());
    let decision = decision.unwrap();
    assert_eq!(decision.decision, "allow");
    assert_eq!(decision.message, Some("Approved by user".to_string()));
}

#[tokio::test]
async fn test_register_with_identity() {
    let state = PermissionState::new();

    let request_id = "identity-req-1".to_string();
    let info = PendingPermissionInfo {
        request_id: request_id.clone(),
        tool_name: "Write".to_string(),
        tool_input: serde_json::json!({"path": "/tmp/test.txt", "content": "hello"}),
        context: Some("Writing test file".to_string()),
        agent_type: Some("ralphx-execution-worker".to_string()),
        task_id: Some("task-xyz".to_string()),
        context_type: Some("task_execution".to_string()),
        context_id: Some("task-xyz".to_string()),
        created_at: "2026-07-10T12:00:00Z".to_string(),
    };

    let _rx = state.register(info).await;

    // Verify identity fields are stored in pending
    let pending = state.pending.lock().await;
    assert!(pending.contains_key(&request_id));
    let request = pending.get(&request_id).unwrap();
    assert_eq!(
        request.info.agent_type,
        Some("ralphx-execution-worker".to_string())
    );
    assert_eq!(request.info.task_id, Some("task-xyz".to_string()));
    assert_eq!(
        request.info.context_type,
        Some("task_execution".to_string())
    );
    assert_eq!(request.info.context_id, Some("task-xyz".to_string()));
}

#[tokio::test]
async fn test_get_pending_info() {
    let state = PermissionState::new();

    // Register multiple pending permissions
    for i in 0..3 {
        let info = PendingPermissionInfo {
            request_id: format!("request-{}", i),
            tool_name: format!("Tool{}", i),
            tool_input: serde_json::json!({"arg": i}),
            context: None,
            agent_type: None,
            task_id: None,
            context_type: None,
            context_id: None,
            created_at: "2026-07-10T12:00:00Z".to_string(),
        };
        state.register(info).await;
    }

    // Get pending info
    let pending_info = state.get_pending_info().await;
    assert_eq!(pending_info.len(), 3);

    // Verify all are present (order not guaranteed)
    let request_ids: Vec<_> = pending_info.iter().map(|p| p.request_id.as_str()).collect();
    assert!(request_ids.contains(&"request-0"));
    assert!(request_ids.contains(&"request-1"));
    assert!(request_ids.contains(&"request-2"));
}

#[tokio::test]
async fn test_multiple_pending_permissions() {
    let state = PermissionState::new();

    // Register multiple pending permissions
    for i in 0..5 {
        state
            .register(make_info(&format!("request-{}", i), "TestTool"))
            .await;
    }

    // Verify all are registered
    let pending = state.pending.lock().await;
    assert_eq!(pending.len(), 5);
    for i in 0..5 {
        assert!(pending.contains_key(&format!("request-{}", i)));
    }
}

#[tokio::test]
async fn test_remove_pending_permission() {
    let state = PermissionState::new();

    // Register a pending permission
    let request_id = "to-remove".to_string();
    state.register(make_info(&request_id, "Bash")).await;

    // Verify it exists
    {
        let pending = state.pending.lock().await;
        assert!(pending.contains_key(&request_id));
    }

    // Remove it
    let removed = state.remove(&request_id).await;
    assert!(removed);

    // Verify it's gone
    {
        let pending = state.pending.lock().await;
        assert!(!pending.contains_key(&request_id));
    }

    // Try to remove again - should return false
    let removed_again = state.remove(&request_id).await;
    assert!(!removed_again);
}

#[tokio::test]
async fn test_resolve_nonexistent_request() {
    let state = PermissionState::new();

    // Try to resolve a request that doesn't exist
    let resolved = state
        .resolve(
            "nonexistent",
            PermissionDecision {
                decision: "deny".to_string(),
                message: None,
            },
        )
        .await;
    assert!(!resolved);
}

// --- Tests with repo persistence ---

mod with_repo {
    use super::*;
    use crate::domain::repositories::PermissionRepository;
    use crate::infrastructure::memory::MemoryPermissionRepository;
    use chrono::Duration;
    use std::sync::Arc;

    fn make_state_with_repo() -> (PermissionState, Arc<MemoryPermissionRepository>) {
        let repo = Arc::new(MemoryPermissionRepository::new());
        let state = PermissionState::with_repo(repo.clone());
        (state, repo)
    }

    #[tokio::test]
    async fn test_with_repo_constructor() {
        let (state, _repo) = make_state_with_repo();
        assert!(state.repo.is_some());
        let pending = state.pending.lock().await;
        assert!(pending.is_empty());
    }

    #[tokio::test]
    async fn test_register_persists_to_repo() {
        let (state, repo) = make_state_with_repo();

        let info = PendingPermissionInfo {
            request_id: "perm-1".to_string(),
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({"command": "ls"}),
            context: Some("List files".to_string()),
            agent_type: None,
            task_id: None,
            context_type: None,
            context_id: None,
            created_at: "2026-07-10T12:00:00Z".to_string(),
        };
        state.register(info).await;

        let repo_pending = repo.get_pending().await.unwrap();
        assert_eq!(repo_pending.len(), 1);
        assert_eq!(repo_pending[0].request_id, "perm-1");
        assert_eq!(repo_pending[0].tool_name, "Bash");
    }

    #[tokio::test]
    async fn test_resolve_persists_to_repo() {
        let (state, repo) = make_state_with_repo();

        let info = PendingPermissionInfo {
            request_id: "perm-1".to_string(),
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({}),
            context: None,
            agent_type: None,
            task_id: None,
            context_type: None,
            context_id: None,
            created_at: "2026-07-10T12:00:00Z".to_string(),
        };
        state.register(info).await;

        let decision = PermissionDecision {
            decision: "allow".to_string(),
            message: Some("Approved".to_string()),
        };
        let resolved = state.resolve("perm-1", decision).await;
        assert!(resolved);

        // After resolve, repo should have no pending
        let repo_pending = repo.get_pending().await.unwrap();
        assert!(repo_pending.is_empty());

        // But the record still exists
        let found = repo.get_by_request_id("perm-1").await.unwrap();
        assert!(found.is_some());
    }

    #[tokio::test]
    async fn test_remove_persists_to_repo() {
        let (state, repo) = make_state_with_repo();

        let info = PendingPermissionInfo {
            request_id: "perm-rm".to_string(),
            tool_name: "Edit".to_string(),
            tool_input: serde_json::json!({}),
            context: None,
            agent_type: None,
            task_id: None,
            context_type: None,
            context_id: None,
            created_at: "2026-07-10T12:00:00Z".to_string(),
        };
        state.register(info).await;

        let removed = state.remove("perm-rm").await;
        assert!(removed);

        // Repo record should be gone
        let found = repo.get_by_request_id("perm-rm").await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_expire_stale_on_startup() {
        let repo = Arc::new(MemoryPermissionRepository::new());

        // Seed repo with pending permissions (simulating leftover from previous run)
        for i in 0..3 {
            let info = PendingPermissionInfo {
                request_id: format!("stale-{}", i),
                tool_name: "Bash".to_string(),
                tool_input: serde_json::json!({}),
                context: None,
                agent_type: None,
                task_id: None,
                context_type: None,
                context_id: None,
                created_at: "2026-07-10T12:00:00Z".to_string(),
            };
            repo.create_pending(&info).await.unwrap();
        }

        assert_eq!(repo.get_pending().await.unwrap().len(), 3);

        let state = PermissionState::with_repo(repo.clone());
        state.expire_stale_on_startup().await;

        // All stale permissions should be expired
        assert!(repo.get_pending().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_expire_stale_noop_without_repo() {
        let state = PermissionState::new();
        // Should not panic when no repo
        state.expire_stale_on_startup().await;
    }

    #[tokio::test]
    async fn strict_pending_read_merges_durable_and_live_permissions_without_duplicates() {
        let repo = Arc::new(MemoryPermissionRepository::new());
        let mut durable = make_info("durable-permission", "Bash");
        durable.created_at = Utc::now().to_rfc3339();
        repo.create_pending(&durable).await.unwrap();
        let state = PermissionState::with_repo(repo);
        state.register(make_info("live-permission", "Edit")).await;
        state
            .register(make_info("durable-permission", "Duplicate"))
            .await;

        let pending = state.get_pending_info_strict().await.unwrap();
        let request_ids: std::collections::HashSet<_> = pending
            .iter()
            .map(|permission| permission.request_id.as_str())
            .collect();

        assert_eq!(
            request_ids,
            std::collections::HashSet::from(["durable-permission", "live-permission"])
        );
        assert_eq!(pending.len(), 2);
    }

    #[tokio::test]
    async fn strict_pending_read_excludes_expired_permissions_and_keeps_fresh_entries() {
        let repo = Arc::new(MemoryPermissionRepository::new());
        let mut expired_durable = make_info("expired-durable", "Bash");
        expired_durable.created_at = (Utc::now() - Duration::minutes(6)).to_rfc3339();
        repo.create_pending(&expired_durable).await.unwrap();

        let mut fresh_durable = make_info("fresh-durable", "Edit");
        fresh_durable.created_at = Utc::now().to_rfc3339();
        repo.create_pending(&fresh_durable).await.unwrap();

        let state = PermissionState::with_repo(repo);
        let mut expired_live = make_info("expired-live", "Read");
        expired_live.created_at = (Utc::now() - Duration::minutes(6)).to_rfc3339();
        state.register(expired_live).await;

        let mut fresh_live = make_info("fresh-live", "Write");
        fresh_live.created_at = Utc::now().to_rfc3339();
        state.register(fresh_live).await;

        let request_ids: std::collections::HashSet<_> = state
            .get_pending_info_strict()
            .await
            .unwrap()
            .into_iter()
            .map(|permission| permission.request_id)
            .collect();

        assert_eq!(
            request_ids,
            std::collections::HashSet::from([
                "fresh-durable".to_string(),
                "fresh-live".to_string(),
            ])
        );
    }

    #[tokio::test]
    async fn strict_pending_read_excludes_resolved_but_undrained_live_permissions() {
        let state = PermissionState::new();
        state.register(make_info("resolved-live", "Bash")).await;
        let _receiver = state
            .pending
            .lock()
            .await
            .get("resolved-live")
            .expect("registered permission remains live until long-poll drain")
            .sender
            .subscribe();
        assert!(
            state
                .resolve(
                    "resolved-live",
                    PermissionDecision {
                        decision: "allow".to_string(),
                        message: None,
                    },
                )
                .await
        );
        assert!(state.pending.lock().await.contains_key("resolved-live"));

        assert!(state.get_pending_info_strict().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn strict_pending_read_returns_durable_repository_failure() {
        use crate::error::AppError;

        struct FailingReadRepo(MemoryPermissionRepository);

        #[async_trait::async_trait]
        impl PermissionRepository for FailingReadRepo {
            async fn create_pending(
                &self,
                info: &PendingPermissionInfo,
            ) -> crate::error::AppResult<()> {
                self.0.create_pending(info).await
            }
            async fn resolve(
                &self,
                request_id: &str,
                decision: &PermissionDecision,
            ) -> crate::error::AppResult<bool> {
                self.0.resolve(request_id, decision).await
            }
            async fn get_pending(&self) -> crate::error::AppResult<Vec<PendingPermissionInfo>> {
                Err(AppError::Database("durable read failed".to_string()))
            }
            async fn get_by_request_id(
                &self,
                request_id: &str,
            ) -> crate::error::AppResult<Option<PendingPermissionInfo>> {
                self.0.get_by_request_id(request_id).await
            }
            async fn expire_all_pending(&self) -> crate::error::AppResult<u64> {
                self.0.expire_all_pending().await
            }
            async fn remove(&self, request_id: &str) -> crate::error::AppResult<bool> {
                self.0.remove(request_id).await
            }
        }

        let repo = Arc::new(FailingReadRepo(MemoryPermissionRepository::new()));
        let state = PermissionState::with_repo(repo);
        state.register(make_info("live-permission", "Bash")).await;

        let error = state.get_pending_info_strict().await.unwrap_err();
        assert!(matches!(error, AppError::Database(message) if message == "durable read failed"));
    }
}
