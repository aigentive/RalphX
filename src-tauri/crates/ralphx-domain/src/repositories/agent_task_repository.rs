use async_trait::async_trait;

use crate::domain::entities::{
    AgentTaskCreate, AgentTaskDetail, AgentTaskListId, AgentTaskListSummary,
    AgentTaskMutationResult, AgentTaskPatch, AgentTaskScope, AgentTaskSummary,
};
use crate::error::AppResult;

#[derive(Debug, Clone, Copy, Default)]
pub struct AgentTaskListOptions {
    pub include_done: bool,
}

#[async_trait]
pub trait AgentTaskRepository: Send + Sync {
    async fn create_task(
        &self,
        scope: &AgentTaskScope,
        input: AgentTaskCreate,
    ) -> AppResult<AgentTaskMutationResult>;

    async fn get_task(
        &self,
        scope: &AgentTaskScope,
        task_ref: &str,
    ) -> AppResult<Option<AgentTaskDetail>>;

    async fn list_tasks(
        &self,
        scope: &AgentTaskScope,
        options: AgentTaskListOptions,
    ) -> AppResult<Vec<AgentTaskSummary>>;

    async fn list_task_lists(&self, scope: &AgentTaskScope)
        -> AppResult<Vec<AgentTaskListSummary>>;

    async fn list_tasks_for_list(
        &self,
        scope: &AgentTaskScope,
        list_id: &AgentTaskListId,
        options: AgentTaskListOptions,
    ) -> AppResult<Vec<AgentTaskSummary>>;

    async fn update_task(
        &self,
        scope: &AgentTaskScope,
        task_ref: &str,
        patch: AgentTaskPatch,
    ) -> AppResult<Option<AgentTaskMutationResult>>;
}
