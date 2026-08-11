use crate::domain::entities::app_state::{AppSettings, ExecutionHaltMode, UpdateChannel};
use crate::domain::entities::ProjectId;
use async_trait::async_trait;

#[async_trait]
pub trait AppStateRepository: Send + Sync {
    async fn get(&self) -> Result<AppSettings, Box<dyn std::error::Error>>;
    async fn set_active_project(
        &self,
        project_id: Option<&ProjectId>,
    ) -> Result<(), Box<dyn std::error::Error>>;
    async fn set_execution_halt_mode(
        &self,
        halt_mode: ExecutionHaltMode,
    ) -> Result<(), Box<dyn std::error::Error>>;
    async fn set_update_channel(
        &self,
        update_channel: UpdateChannel,
    ) -> Result<(), Box<dyn std::error::Error>>;
    async fn set_last_seen_release_notes_version(
        &self,
        version: Option<&str>,
    ) -> Result<(), Box<dyn std::error::Error>>;
    async fn set_remove_inherited_github_cli_tokens(
        &self,
        enabled: bool,
    ) -> Result<(), Box<dyn std::error::Error>>;
}
