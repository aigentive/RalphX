use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use tokio::sync::RwLock;

use crate::domain::agents::{ManualRoleDefault, RoutingRole, StoredManualRoleDefault};
use crate::domain::repositories::{
    manual_role_default_repository::ManualRoleDefaultRepositoryResult, ManualRoleDefaultRepository,
};

pub struct MemoryManualRoleDefaultRepository {
    next_id: Arc<RwLock<i64>>,
    global_rows: Arc<RwLock<HashMap<RoutingRole, StoredManualRoleDefault>>>,
    project_rows: Arc<RwLock<HashMap<(String, RoutingRole), StoredManualRoleDefault>>>,
}

impl Default for MemoryManualRoleDefaultRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryManualRoleDefaultRepository {
    pub fn new() -> Self {
        Self {
            next_id: Arc::new(RwLock::new(1)),
            global_rows: Arc::new(RwLock::new(HashMap::new())),
            project_rows: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    async fn allocate_id(&self) -> i64 {
        let mut next_id = self.next_id.write().await;
        let id = *next_id;
        *next_id += 1;
        id
    }
}

#[async_trait]
impl ManualRoleDefaultRepository for MemoryManualRoleDefaultRepository {
    async fn get_global(
        &self,
        role: RoutingRole,
    ) -> ManualRoleDefaultRepositoryResult<Option<StoredManualRoleDefault>> {
        Ok(self.global_rows.read().await.get(&role).cloned())
    }

    async fn get_for_project(
        &self,
        project_id: &str,
        role: RoutingRole,
    ) -> ManualRoleDefaultRepositoryResult<Option<StoredManualRoleDefault>> {
        Ok(self
            .project_rows
            .read()
            .await
            .get(&(project_id.to_string(), role))
            .cloned())
    }

    async fn list_global(&self) -> ManualRoleDefaultRepositoryResult<Vec<StoredManualRoleDefault>> {
        let mut rows = self
            .global_rows
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by_key(|row| row.role.to_string());
        Ok(rows)
    }

    async fn list_for_project(
        &self,
        project_id: &str,
    ) -> ManualRoleDefaultRepositoryResult<Vec<StoredManualRoleDefault>> {
        let mut rows = self
            .project_rows
            .read()
            .await
            .iter()
            .filter(|((stored_project_id, _), _)| stored_project_id == project_id)
            .map(|(_, row)| row.clone())
            .collect::<Vec<_>>();
        rows.sort_by_key(|row| row.role.to_string());
        Ok(rows)
    }

    async fn upsert_global(
        &self,
        role: RoutingRole,
        value: &ManualRoleDefault,
    ) -> ManualRoleDefaultRepositoryResult<StoredManualRoleDefault> {
        let existing_id = self.global_rows.read().await.get(&role).map(|row| row.id);
        let id = match existing_id {
            Some(id) => id,
            None => self.allocate_id().await,
        };
        let row = StoredManualRoleDefault {
            id,
            project_id: None,
            role,
            value: value.clone(),
            updated_at: Utc::now(),
        };
        self.global_rows.write().await.insert(role, row.clone());
        Ok(row)
    }

    async fn upsert_for_project(
        &self,
        project_id: &str,
        role: RoutingRole,
        value: &ManualRoleDefault,
    ) -> ManualRoleDefaultRepositoryResult<StoredManualRoleDefault> {
        let key = (project_id.to_string(), role);
        let existing_id = self.project_rows.read().await.get(&key).map(|row| row.id);
        let id = match existing_id {
            Some(id) => id,
            None => self.allocate_id().await,
        };
        let row = StoredManualRoleDefault {
            id,
            project_id: Some(project_id.to_string()),
            role,
            value: value.clone(),
            updated_at: Utc::now(),
        };
        self.project_rows.write().await.insert(key, row.clone());
        Ok(row)
    }

    async fn clear_global(&self, role: RoutingRole) -> ManualRoleDefaultRepositoryResult<bool> {
        Ok(self.global_rows.write().await.remove(&role).is_some())
    }

    async fn clear_for_project(
        &self,
        project_id: &str,
        role: RoutingRole,
    ) -> ManualRoleDefaultRepositoryResult<bool> {
        Ok(self
            .project_rows
            .write()
            .await
            .remove(&(project_id.to_string(), role))
            .is_some())
    }
}

#[cfg(test)]
#[path = "memory_manual_role_default_repo_tests.rs"]
mod tests;
