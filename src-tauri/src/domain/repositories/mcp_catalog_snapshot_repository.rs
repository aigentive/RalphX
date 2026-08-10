use async_trait::async_trait;

use crate::error::AppResult;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpCatalogSnapshot {
    pub scope_project_id: Option<String>,
    pub provider: String,
    pub response_json: String,
    pub captured_at: String,
}

#[async_trait]
pub trait McpCatalogSnapshotRepository: Send + Sync {
    async fn get(
        &self,
        scope_project_id: Option<&str>,
        provider: &str,
    ) -> AppResult<Option<McpCatalogSnapshot>>;

    async fn upsert(&self, snapshot: McpCatalogSnapshot) -> AppResult<McpCatalogSnapshot>;
}
