use std::sync::Arc;

use async_trait::async_trait;

use crate::application::agent_conversation_archive::archive_agent_conversation_for_state;
use crate::application::agent_workspace_terminal_cleanup::{
    terminalize_agent_workspace_after_pr, TerminalAgentWorkspaceCause,
};
use crate::application::chat_service::ChatService;
use crate::application::AppState;
use crate::domain::entities::ChatConversationId;
use crate::error::{AppError, AppResult};

#[async_trait]
pub trait AutomationMergedRunFinalizer: Send + Sync {
    async fn finalize_merged_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<()>;
}

pub struct AppStateAutomationMergedRunFinalizer {
    state: AppState,
}

impl AppStateAutomationMergedRunFinalizer {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl AutomationMergedRunFinalizer for AppStateAutomationMergedRunFinalizer {
    async fn finalize_merged_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<()> {
        self.state
            .chat_conversation_repo
            .get_by_id(conversation_id)
            .await?
            .ok_or_else(|| {
                AppError::NotFound(format!(
                    "Automation merged conversation {} not found",
                    conversation_id.as_str()
                ))
            })?;

        if let Some(workspace) = self
            .state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(conversation_id)
            .await?
        {
            let project = self
                .state
                .project_repo
                .get_by_id(&workspace.project_id)
                .await?
                .ok_or_else(|| {
                    AppError::NotFound(format!(
                        "Automation merged workspace project {} not found",
                        workspace.project_id.as_str()
                    ))
                })?;
            let chat_service: Arc<dyn ChatService> = Arc::new(self.state.build_chat_service());
            let terminalized = terminalize_agent_workspace_after_pr(
                Arc::clone(&self.state.agent_conversation_workspace_repo),
                Arc::clone(&self.state.agent_workspace_repair_repo),
                Arc::clone(&self.state.agent_run_repo),
                Some(Arc::clone(&self.state.plan_branch_repo)),
                Some(chat_service),
                conversation_id,
                &project,
                TerminalAgentWorkspaceCause::MergedPr,
            )
            .await;
            terminalized
                .require_runtime_shutdown()
                .map_err(AppError::Infrastructure)?;
        }

        archive_agent_conversation_for_state(conversation_id, &self.state, false)
            .await
            .map(|_| ())
            .map_err(AppError::Infrastructure)
    }
}

pub struct NoopAutomationMergedRunFinalizer;

#[async_trait]
impl AutomationMergedRunFinalizer for NoopAutomationMergedRunFinalizer {
    async fn finalize_merged_conversation(
        &self,
        _conversation_id: &ChatConversationId,
    ) -> AppResult<()> {
        Ok(())
    }
}
