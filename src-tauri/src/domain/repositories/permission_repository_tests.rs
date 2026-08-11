use super::*;
use std::collections::HashMap;
use std::sync::RwLock;

struct MockPermissionRepository {
    permissions: RwLock<HashMap<String, (PendingPermissionInfo, Option<PermissionDecision>)>>,
}

impl MockPermissionRepository {
    fn new() -> Self {
        Self {
            permissions: RwLock::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl PermissionRepository for MockPermissionRepository {
    async fn create_pending(&self, info: &PendingPermissionInfo) -> AppResult<()> {
        let mut permissions = self.permissions.write().unwrap();
        permissions.insert(info.request_id.clone(), (info.clone(), None));
        Ok(())
    }

    async fn resolve(&self, request_id: &str, decision: &PermissionDecision) -> AppResult<bool> {
        let mut permissions = self.permissions.write().unwrap();
        if let Some(entry) = permissions.get_mut(request_id) {
            entry.1 = Some(decision.clone());
            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn get_pending(&self) -> AppResult<Vec<PendingPermissionInfo>> {
        let permissions = self.permissions.read().unwrap();
        Ok(permissions
            .values()
            .filter(|(_, decision)| decision.is_none())
            .map(|(info, _)| info.clone())
            .collect())
    }

    async fn get_by_request_id(
        &self,
        request_id: &str,
    ) -> AppResult<Option<PendingPermissionInfo>> {
        let permissions = self.permissions.read().unwrap();
        Ok(permissions.get(request_id).map(|(info, _)| info.clone()))
    }

    async fn expire_all_pending(&self) -> AppResult<u64> {
        let mut permissions = self.permissions.write().unwrap();
        let pending_ids: Vec<String> = permissions
            .iter()
            .filter(|(_, (_, decision))| decision.is_none())
            .map(|(id, _)| id.clone())
            .collect();
        let count = pending_ids.len() as u64;
        for id in pending_ids {
            permissions.remove(&id);
        }
        Ok(count)
    }

    async fn remove(&self, request_id: &str) -> AppResult<bool> {
        let mut permissions = self.permissions.write().unwrap();
        Ok(permissions.remove(request_id).is_some())
    }
}

#[test]
fn test_permission_repository_trait_is_object_safe() {
    let repo: std::sync::Arc<dyn PermissionRepository> =
        std::sync::Arc::new(MockPermissionRepository::new());
    assert_eq!(std::sync::Arc::strong_count(&repo), 1);
}

#[tokio::test]
async fn test_create_and_get_pending() {
    let repo = MockPermissionRepository::new();
    let info = PendingPermissionInfo {
        request_id: "perm-1".to_string(),
        tool_name: "Bash".to_string(),
        tool_input: serde_json::json!({"command": "ls"}),
        context: Some("List files".to_string()),
        agent_type: None,
        task_id: None,
        context_type: None,
        context_id: None,
        created_at: "2026-07-10T00:00:00+00:00".to_string(),
    };

    repo.create_pending(&info).await.unwrap();

    let pending = repo.get_pending().await.unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].request_id, "perm-1");
    assert_eq!(pending[0].tool_name, "Bash");
}

#[tokio::test]
async fn test_get_by_request_id() {
    let repo = MockPermissionRepository::new();
    let info = PendingPermissionInfo {
        request_id: "perm-42".to_string(),
        tool_name: "Write".to_string(),
        tool_input: serde_json::json!({}),
        context: None,
        agent_type: None,
        task_id: None,
        context_type: None,
        context_id: None,
        created_at: "2026-07-10T00:00:00+00:00".to_string(),
    };

    repo.create_pending(&info).await.unwrap();

    let found = repo.get_by_request_id("perm-42").await.unwrap();
    assert!(found.is_some());
    assert_eq!(found.unwrap().tool_name, "Write");

    let not_found = repo.get_by_request_id("nonexistent").await.unwrap();
    assert!(not_found.is_none());
}

#[tokio::test]
async fn test_resolve() {
    let repo = MockPermissionRepository::new();
    let info = PendingPermissionInfo {
        request_id: "perm-1".to_string(),
        tool_name: "Bash".to_string(),
        tool_input: serde_json::json!({}),
        context: None,
        agent_type: None,
        task_id: None,
        context_type: None,
        context_id: None,
        created_at: "2026-07-10T00:00:00+00:00".to_string(),
    };

    repo.create_pending(&info).await.unwrap();

    let decision = PermissionDecision {
        decision: "allow".to_string(),
        message: Some("Approved".to_string()),
    };
    let resolved = repo.resolve("perm-1", &decision).await.unwrap();
    assert!(resolved);

    // After resolving, it should no longer appear in get_pending
    let pending = repo.get_pending().await.unwrap();
    assert!(pending.is_empty());

    // But get_by_request_id still returns it (record exists)
    let found = repo.get_by_request_id("perm-1").await.unwrap();
    assert!(found.is_some());
}

#[tokio::test]
async fn test_resolve_nonexistent() {
    let repo = MockPermissionRepository::new();
    let decision = PermissionDecision {
        decision: "deny".to_string(),
        message: None,
    };
    let resolved = repo.resolve("nope", &decision).await.unwrap();
    assert!(!resolved);
}

#[tokio::test]
async fn test_expire_all_pending() {
    let repo = MockPermissionRepository::new();

    for i in 0..3 {
        let info = PendingPermissionInfo {
            request_id: format!("perm-{}", i),
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({}),
            context: None,
            agent_type: None,
            task_id: None,
            context_type: None,
            context_id: None,
            created_at: "2026-07-10T00:00:00+00:00".to_string(),
        };
        repo.create_pending(&info).await.unwrap();
    }

    // Resolve one so it's not pending
    let decision = PermissionDecision {
        decision: "allow".to_string(),
        message: None,
    };
    repo.resolve("perm-0", &decision).await.unwrap();

    // Expire remaining pending
    let expired = repo.expire_all_pending().await.unwrap();
    assert_eq!(expired, 2);

    let pending = repo.get_pending().await.unwrap();
    assert!(pending.is_empty());
}

#[tokio::test]
async fn test_remove() {
    let repo = MockPermissionRepository::new();
    let info = PendingPermissionInfo {
        request_id: "perm-rm".to_string(),
        tool_name: "Edit".to_string(),
        tool_input: serde_json::json!({}),
        context: None,
        agent_type: None,
        task_id: None,
        context_type: None,
        context_id: None,
        created_at: "2026-07-10T00:00:00+00:00".to_string(),
    };

    repo.create_pending(&info).await.unwrap();
    let removed = repo.remove("perm-rm").await.unwrap();
    assert!(removed);

    let found = repo.get_by_request_id("perm-rm").await.unwrap();
    assert!(found.is_none());

    let removed_again = repo.remove("perm-rm").await.unwrap();
    assert!(!removed_again);
}
