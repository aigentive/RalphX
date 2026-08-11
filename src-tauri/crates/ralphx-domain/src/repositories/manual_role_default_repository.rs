use async_trait::async_trait;

use crate::agents::{ManualRoleDefault, RoutingRole, StoredManualRoleDefault};

pub type ManualRoleDefaultRepositoryResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[async_trait]
pub trait ManualRoleDefaultRepository: Send + Sync {
    async fn get_global(
        &self,
        role: RoutingRole,
    ) -> ManualRoleDefaultRepositoryResult<Option<StoredManualRoleDefault>>;

    async fn get_for_project(
        &self,
        project_id: &str,
        role: RoutingRole,
    ) -> ManualRoleDefaultRepositoryResult<Option<StoredManualRoleDefault>>;

    async fn list_global(&self) -> ManualRoleDefaultRepositoryResult<Vec<StoredManualRoleDefault>>;

    async fn list_for_project(
        &self,
        project_id: &str,
    ) -> ManualRoleDefaultRepositoryResult<Vec<StoredManualRoleDefault>>;

    async fn upsert_global(
        &self,
        role: RoutingRole,
        value: &ManualRoleDefault,
    ) -> ManualRoleDefaultRepositoryResult<StoredManualRoleDefault>;

    async fn upsert_for_project(
        &self,
        project_id: &str,
        role: RoutingRole,
        value: &ManualRoleDefault,
    ) -> ManualRoleDefaultRepositoryResult<StoredManualRoleDefault>;

    async fn clear_global(&self, role: RoutingRole) -> ManualRoleDefaultRepositoryResult<bool>;

    async fn clear_for_project(
        &self,
        project_id: &str,
        role: RoutingRole,
    ) -> ManualRoleDefaultRepositoryResult<bool>;
}
