use async_trait::async_trait;
use std::path::Path;
use std::sync::Arc;

use crate::application::chat_service::{ChatService, SendMessageOptions};
use crate::domain::entities::{BranchUpdateOperation, ChatContextType, InternalStatus};
use crate::domain::repositories::{BranchUpdateRepository, TaskRepository};
use crate::domain::state_machine::services::{BranchUpdateWorkflow, BranchUpdateWorkflowOutcome};
use crate::error::{AppError, AppResult};
use crate::infrastructure::agents::claude::agent_names::AGENT_BRANCH_UPDATER;

pub struct ApplicationBranchUpdateWorkflow {
    chat_service: Arc<dyn ChatService>,
}

impl ApplicationBranchUpdateWorkflow {
    pub fn new(chat_service: Arc<dyn ChatService>) -> Self {
        Self { chat_service }
    }
}

#[async_trait]
impl BranchUpdateWorkflow for ApplicationBranchUpdateWorkflow {
    async fn execute_programmatic(
        &self,
        repository: Arc<dyn BranchUpdateRepository>,
        task_repository: Arc<dyn TaskRepository>,
        repo_path: &Path,
        operation: &BranchUpdateOperation,
        update_status: InternalStatus,
        fencing_epoch: u64,
    ) -> AppResult<BranchUpdateWorkflowOutcome> {
        let outcome = super::branch_update_executor::execute_programmatic_branch_update(
            repository,
            task_repository,
            repo_path,
            operation,
            update_status,
            fencing_epoch,
        )
        .await?;
        Ok(match outcome {
            super::branch_update_executor::BranchUpdateExecutionOutcome::Completed {
                destination,
            } => BranchUpdateWorkflowOutcome::Completed { destination },
            super::branch_update_executor::BranchUpdateExecutionOutcome::ContinuationPending => {
                BranchUpdateWorkflowOutcome::ContinuationPending
            }
            super::branch_update_executor::BranchUpdateExecutionOutcome::NeedsAgent => {
                BranchUpdateWorkflowOutcome::NeedsAgent
            }
            super::branch_update_executor::BranchUpdateExecutionOutcome::Blocked => {
                BranchUpdateWorkflowOutcome::Blocked
            }
        })
    }

    async fn publish_post_merge(
        &self,
        repository: Arc<dyn BranchUpdateRepository>,
        repo_path: &Path,
        operation: &BranchUpdateOperation,
        update_status: InternalStatus,
    ) -> AppResult<InternalStatus> {
        super::branch_update_executor::publish_post_merge_branch_update(
            repository,
            repo_path,
            operation,
            update_status,
        )
        .await
    }

    async fn start_resolver(&self, task_id: &str, prompt: &str, workspace: &Path) -> AppResult<()> {
        let result = self
            .chat_service
            .send_message(
                ChatContextType::BranchUpdate,
                task_id,
                prompt,
                SendMessageOptions {
                    agent_name_override: Some(AGENT_BRANCH_UPDATER.to_string()),
                    working_directory_override: Some(workspace.to_path_buf()),
                    ..Default::default()
                },
            )
            .await
            .map_err(|error| AppError::ExecutionBlocked(error.to_string()))?;
        tracing::info!(
            task_id,
            queued = result.was_queued,
            "Branch updater agent requested"
        );
        Ok(())
    }
}
