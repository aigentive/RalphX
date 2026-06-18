use std::sync::Arc;

use serde_json::Value;

use crate::domain::entities::{
    AgentTaskCreate, AgentTaskDetail, AgentTaskListId, AgentTaskListSummary,
    AgentTaskMutationResult, AgentTaskPatch, AgentTaskScope, AgentTaskState, AgentTaskSummary,
};
use crate::domain::repositories::{AgentTaskListOptions, AgentTaskRepository};
use crate::error::{AppError, AppResult};

const SINGLE_TASK_LEDGER_REPAIR_HINT: &str = concat!(
    "single-task agent task ledger cannot be claimed, activated, or completed. ",
    "If the work is simple, mark it dropped using update_agent_task state=dropped and continue ",
    "without the ledger. If the work is non-trivial, decompose it into multiple concrete tasks ",
    "before claiming work."
);

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

    pub async fn list_task_lists(
        &self,
        scope: &AgentTaskScope,
    ) -> AppResult<Vec<AgentTaskListSummary>> {
        self.repo.list_task_lists(scope).await
    }

    pub async fn list_tasks_for_list(
        &self,
        scope: &AgentTaskScope,
        list_id: &AgentTaskListId,
        options: AgentTaskListOptions,
    ) -> AppResult<Vec<AgentTaskSummary>> {
        self.repo.list_tasks_for_list(scope, list_id, options).await
    }

    pub async fn update_task(
        &self,
        scope: &AgentTaskScope,
        task_ref: &str,
        patch: AgentTaskPatch,
    ) -> AppResult<Option<AgentTaskMutationResult>> {
        if matches!(
            patch.state,
            Some(AgentTaskState::Active | AgentTaskState::Done)
        ) {
            let Some(task) = self.repo.get_task(scope, task_ref).await? else {
                return Ok(None);
            };
            self.ensure_not_single_task_ledger(scope, &task).await?;
        }
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
        self.ensure_not_single_task_ledger(scope, &task).await?;

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
        self.update_task(
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

    async fn ensure_not_single_task_ledger(
        &self,
        scope: &AgentTaskScope,
        task: &AgentTaskDetail,
    ) -> AppResult<()> {
        let meaningful_tasks = self
            .repo
            .list_tasks(scope, AgentTaskListOptions { include_done: true })
            .await?
            .into_iter()
            .filter(|summary| summary.state != AgentTaskState::Dropped)
            .collect::<Vec<_>>();
        if meaningful_tasks.len() == 1 && meaningful_tasks[0].task_id == task.task_id {
            return Err(AppError::Validation(
                SINGLE_TASK_LEDGER_REPAIR_HINT.to_string(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "agent_task_service_tests.rs"]
mod tests;
