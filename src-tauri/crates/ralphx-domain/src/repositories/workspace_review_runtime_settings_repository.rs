use async_trait::async_trait;

use crate::agents::{
    AgentHarnessKind, StoredWorkspaceReviewRuntimeSettings, WorkspaceReviewRuntimeSettings,
};

#[async_trait]
pub trait WorkspaceReviewRuntimeSettingsRepository: Send + Sync {
    async fn get_global(
        &self,
        provider: AgentHarnessKind,
    ) -> Result<Option<StoredWorkspaceReviewRuntimeSettings>, Box<dyn std::error::Error>>;

    async fn get_for_project(
        &self,
        project_id: &str,
        provider: AgentHarnessKind,
    ) -> Result<Option<StoredWorkspaceReviewRuntimeSettings>, Box<dyn std::error::Error>>;

    async fn list_global(
        &self,
    ) -> Result<Vec<StoredWorkspaceReviewRuntimeSettings>, Box<dyn std::error::Error>>;

    async fn list_for_project(
        &self,
        project_id: &str,
    ) -> Result<Vec<StoredWorkspaceReviewRuntimeSettings>, Box<dyn std::error::Error>>;

    async fn upsert_global(
        &self,
        provider: AgentHarnessKind,
        settings: &WorkspaceReviewRuntimeSettings,
    ) -> Result<StoredWorkspaceReviewRuntimeSettings, Box<dyn std::error::Error>>;

    async fn upsert_for_project(
        &self,
        project_id: &str,
        provider: AgentHarnessKind,
        settings: &WorkspaceReviewRuntimeSettings,
    ) -> Result<StoredWorkspaceReviewRuntimeSettings, Box<dyn std::error::Error>>;
}
