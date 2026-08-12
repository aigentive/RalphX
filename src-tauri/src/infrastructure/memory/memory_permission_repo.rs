use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::RwLock;

use crate::domain::entities::permission_request::{PendingPermissionInfo, PermissionDecision};
use crate::domain::repositories::PermissionRepository;
use crate::error::AppResult;

pub struct MemoryPermissionRepository {
    permissions: RwLock<HashMap<String, (PendingPermissionInfo, Option<PermissionDecision>)>>,
}

impl MemoryPermissionRepository {
    pub fn new() -> Self {
        Self {
            permissions: RwLock::new(HashMap::new()),
        }
    }
}

impl Default for MemoryPermissionRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PermissionRepository for MemoryPermissionRepository {
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

#[cfg(test)]
#[path = "memory_permission_repo_tests.rs"]
mod tests;
