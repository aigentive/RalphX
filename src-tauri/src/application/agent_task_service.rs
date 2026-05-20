use std::sync::Arc;

use serde_json::Value;

use crate::domain::entities::{
    AgentTaskCreate, AgentTaskDetail, AgentTaskMutationResult, AgentTaskPatch, AgentTaskScope,
    AgentTaskState, AgentTaskSummary,
};
use crate::domain::repositories::{AgentTaskListOptions, AgentTaskRepository};
use crate::error::{AppError, AppResult};

pub struct AgentTaskService {
    repo: Arc<dyn AgentTaskRepository>,
}

impl AgentTaskService {
    pub fn new(repo: Arc<dyn AgentTaskRepository>) -> Self {
        Self { repo }
    }

    pub async fn create_task(
        &self,
        scope: &AgentTaskScope,
        input: AgentTaskCreate,
    ) -> AppResult<AgentTaskMutationResult> {
        self.repo.create_task(scope, input).await
    }

    pub async fn get_task(
        &self,
        scope: &AgentTaskScope,
        task_ref: &str,
    ) -> AppResult<Option<AgentTaskDetail>> {
        self.repo.get_task(scope, task_ref).await
    }

    pub async fn list_tasks(
        &self,
        scope: &AgentTaskScope,
        options: AgentTaskListOptions,
    ) -> AppResult<Vec<AgentTaskSummary>> {
        self.repo.list_tasks(scope, options).await
    }

    pub async fn update_task(
        &self,
        scope: &AgentTaskScope,
        task_ref: &str,
        patch: AgentTaskPatch,
    ) -> AppResult<Option<AgentTaskMutationResult>> {
        self.repo.update_task(scope, task_ref, patch).await
    }

    pub async fn claim_task(
        &self,
        scope: &AgentTaskScope,
        task_ref: &str,
        owner_agent: Option<String>,
    ) -> AppResult<Option<AgentTaskMutationResult>> {
        let Some(task) = self.repo.get_task(scope, task_ref).await? else {
            return Ok(None);
        };
        if !task.unresolved_blocked_by.is_empty() {
            return Err(AppError::Validation(format!(
                "agent task is blocked by unresolved tasks: {}",
                task.unresolved_blocked_by.join(", ")
            )));
        }

        let owner = owner_agent.or_else(|| scope.actor_agent.clone());
        self.repo
            .update_task(
                scope,
                task_ref,
                AgentTaskPatch {
                    owner_agent: Some(owner),
                    state: Some(AgentTaskState::Active),
                    ..Default::default()
                },
            )
            .await
    }

    pub async fn complete_task(
        &self,
        scope: &AgentTaskScope,
        task_ref: &str,
        metadata_patch: Option<Value>,
    ) -> AppResult<Option<AgentTaskMutationResult>> {
        self.repo
            .update_task(
                scope,
                task_ref,
                AgentTaskPatch {
                    state: Some(AgentTaskState::Done),
                    metadata_patch,
                    ..Default::default()
                },
            )
            .await
    }
}

#[cfg(test)]
#[path = "agent_task_service_tests.rs"]
mod tests;
