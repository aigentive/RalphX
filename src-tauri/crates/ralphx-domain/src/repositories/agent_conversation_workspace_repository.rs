use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::entities::{
    AgentConversationWorkspace, AgentConversationWorkspacePublicationEvent,
    AgentConversationWorkspaceStatus, AgentWorkspacePrDescription, ChatConversationId,
    IdeationSessionId, PlanBranchId, ProjectId,
};
use crate::error::AppResult;

#[async_trait]
pub trait AgentConversationWorkspaceRepository: Send + Sync {
    async fn create_or_update(
        &self,
        workspace: AgentConversationWorkspace,
    ) -> AppResult<AgentConversationWorkspace>;

    async fn get_by_conversation_id(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentConversationWorkspace>>;

    async fn get_by_project_id(
        &self,
        project_id: &ProjectId,
    ) -> AppResult<Vec<AgentConversationWorkspace>>;

    async fn get_terminal_local_cleanup_candidates_by_project_id(
        &self,
        project_id: &ProjectId,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        let workspaces = self.get_by_project_id(project_id).await?;
        Ok(workspaces
            .into_iter()
            .filter(|workspace| {
                workspace
                    .publication_pr_status
                    .as_deref()
                    .is_some_and(|status| matches!(status, "merged" | "closed"))
            })
            .collect())
    }

    async fn mark_local_cleanup_status(
        &self,
        _conversation_id: &ChatConversationId,
        _status: &str,
        _checked_at: DateTime<Utc>,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn list_active_direct_published_workspaces(
        &self,
    ) -> AppResult<Vec<AgentConversationWorkspace>>;

    async fn list_active_direct_external_pr_reconciliation_candidates(
        &self,
        limit: usize,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        let _ = limit;
        Ok(Vec::new())
    }

    async fn list_active_needs_agent_workspaces(
        &self,
    ) -> AppResult<Vec<AgentConversationWorkspace>>;

    async fn update_links(
        &self,
        conversation_id: &ChatConversationId,
        ideation_session_id: Option<&IdeationSessionId>,
        plan_branch_id: Option<&PlanBranchId>,
    ) -> AppResult<()>;

    async fn update_publication(
        &self,
        conversation_id: &ChatConversationId,
        pr_number: Option<i64>,
        pr_url: Option<&str>,
        pr_status: Option<&str>,
        push_status: Option<&str>,
    ) -> AppResult<()>;

    async fn update_status(
        &self,
        conversation_id: &ChatConversationId,
        status: AgentConversationWorkspaceStatus,
    ) -> AppResult<()>;

    async fn save_pr_description(
        &self,
        conversation_id: &ChatConversationId,
        description: AgentWorkspacePrDescription,
    ) -> AppResult<()>;

    async fn get_pr_description(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentWorkspacePrDescription>>;

    async fn clear_pr_description(&self, conversation_id: &ChatConversationId) -> AppResult<()>;

    async fn append_publication_event(
        &self,
        event: AgentConversationWorkspacePublicationEvent,
    ) -> AppResult<()>;

    async fn list_publication_events(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Vec<AgentConversationWorkspacePublicationEvent>>;

    async fn delete(&self, conversation_id: &ChatConversationId) -> AppResult<()>;

    async fn list_worktree_paths_by_project_id(
        &self,
        project_id: &ProjectId,
    ) -> AppResult<std::collections::HashSet<String>> {
        let workspaces = self.get_by_project_id(project_id).await?;
        Ok(workspaces.into_iter().map(|w| w.worktree_path).collect())
    }
}
