use std::collections::HashMap;

use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::domain::repositories::{McpCatalogSnapshot, McpCatalogSnapshotRepository};
use crate::error::AppResult;

#[derive(Default)]
pub struct MemoryMcpCatalogSnapshotRepository {
    rows: RwLock<HashMap<(Option<String>, String), McpCatalogSnapshot>>,
}

impl MemoryMcpCatalogSnapshotRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl McpCatalogSnapshotRepository for MemoryMcpCatalogSnapshotRepository {
    async fn get(
        &self,
        scope_project_id: Option<&str>,
        provider: &str,
    ) -> AppResult<Option<McpCatalogSnapshot>> {
        Ok(self
            .rows
            .read()
            .await
            .get(&(scope_project_id.map(str::to_string), provider.to_string()))
            .cloned())
    }

    async fn upsert(&self, snapshot: McpCatalogSnapshot) -> AppResult<McpCatalogSnapshot> {
        self.rows.write().await.insert(
            (snapshot.scope_project_id.clone(), snapshot.provider.clone()),
            snapshot.clone(),
        );
        Ok(snapshot)
    }
}
