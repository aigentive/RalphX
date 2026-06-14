use async_trait::async_trait;

use crate::domain::entities::types::ProjectId;
use crate::domain::entities::{TaskOutcome, TaskOutcomeId, TaskOutcomeStatus};
use crate::error::AppResult;

#[derive(Debug, Clone)]
pub struct UpsertTaskOutcomeInput {
    pub outcome: TaskOutcome,
}

#[derive(Debug, Clone, Default)]
pub struct TaskOutcomeListOptions {
    pub source: Option<String>,
    pub status: Option<TaskOutcomeStatus>,
}

#[async_trait]
pub trait TaskOutcomeRepository: Send + Sync {
    async fn upsert(&self, input: UpsertTaskOutcomeInput) -> AppResult<TaskOutcome>;

    async fn get_by_id(&self, id: &TaskOutcomeId) -> AppResult<Option<TaskOutcome>>;

    async fn list_by_project(
        &self,
        project_id: &ProjectId,
        options: TaskOutcomeListOptions,
    ) -> AppResult<Vec<TaskOutcome>>;
}
