use async_trait::async_trait;

use crate::entities::{
    AgentConversationWorkspace, AgentConversationWorkspacePublicationEvent,
    AgentConversationWorkspaceStatus, AgentWorkspacePrDescription, AgentWorkspacePrReviewMonitor,
    ChatConversationId, IdeationSessionId, PlanBranchId, ProjectId,
};
use crate::error::{AppError, AppResult};
use crate::repositories::agent_conversation_workspace_repository::AgentConversationWorkspaceRepository;

struct FallbackOnlyWorkspaceRepository;

#[async_trait]
impl AgentConversationWorkspaceRepository for FallbackOnlyWorkspaceRepository {
    async fn create_or_update(
        &self,
        workspace: AgentConversationWorkspace,
    ) -> AppResult<AgentConversationWorkspace> {
        Ok(workspace)
    }

    async fn get_by_conversation_id(
        &self,
        _conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentConversationWorkspace>> {
        Ok(None)
    }

    async fn get_by_project_id(
        &self,
        _project_id: &ProjectId,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        Ok(Vec::new())
    }

    async fn list_active_direct_published_workspaces(
        &self,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        Ok(Vec::new())
    }

    async fn list_active_needs_agent_workspaces(
        &self,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        Ok(Vec::new())
    }

    async fn update_links(
        &self,
        _conversation_id: &ChatConversationId,
        _ideation_session_id: Option<&IdeationSessionId>,
        _plan_branch_id: Option<&PlanBranchId>,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn update_publication(
        &self,
        _conversation_id: &ChatConversationId,
        _pr_number: Option<i64>,
        _pr_url: Option<&str>,
        _pr_status: Option<&str>,
        _push_status: Option<&str>,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn update_pr_supervision_preferences(
        &self,
        _conversation_id: &ChatConversationId,
        _autofix_enabled: bool,
        _auto_merge_desired: bool,
        _auto_merge_method: &str,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn update_status(
        &self,
        _conversation_id: &ChatConversationId,
        _status: AgentConversationWorkspaceStatus,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn save_pr_description(
        &self,
        _conversation_id: &ChatConversationId,
        _description: AgentWorkspacePrDescription,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn get_pr_description(
        &self,
        _conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentWorkspacePrDescription>> {
        Ok(None)
    }

    async fn clear_pr_description(&self, _conversation_id: &ChatConversationId) -> AppResult<()> {
        Ok(())
    }

    async fn append_publication_event(
        &self,
        _event: AgentConversationWorkspacePublicationEvent,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn list_publication_events(
        &self,
        _conversation_id: &ChatConversationId,
    ) -> AppResult<Vec<AgentConversationWorkspacePublicationEvent>> {
        Ok(Vec::new())
    }

    async fn set_pr_review_auto_approve_enabled(
        &self,
        conversation_id: &ChatConversationId,
        _enabled: bool,
    ) -> AppResult<AgentWorkspacePrReviewMonitor> {
        Ok(AgentWorkspacePrReviewMonitor::new(
            *conversation_id,
            ProjectId::from_string("project-fallback".to_string()),
            1,
            None,
        ))
    }

    async fn mark_pr_review_first_action_resolved(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<AgentWorkspacePrReviewMonitor> {
        Ok(AgentWorkspacePrReviewMonitor::new(
            *conversation_id,
            ProjectId::from_string("project-fallback".to_string()),
            1,
            None,
        ))
    }

    async fn claim_pending_pr_review_action(&self, _action_id: &str) -> AppResult<bool> {
        Ok(false)
    }

    async fn set_last_blocked_pr_health_fingerprint(
        &self,
        _conversation_id: &ChatConversationId,
        _fingerprint: Option<&str>,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn delete(&self, _conversation_id: &ChatConversationId) -> AppResult<()> {
        Ok(())
    }
}

#[tokio::test]
async fn pr_review_monitor_enabled_default_fails_closed_for_unsupported_repo() {
    let repo = FallbackOnlyWorkspaceRepository;
    let conversation_id = ChatConversationId::from_string("fallback-monitor".to_string());

    let error = repo
        .set_pr_review_monitor_enabled(&conversation_id, true)
        .await
        .expect_err("unsupported repository must fail closed when toggling monitoring");

    assert!(matches!(
        error,
        AppError::Infrastructure(message)
            if message.contains("PR review monitor settings are unsupported")
    ));
}

#[tokio::test]
async fn supersede_pending_pr_review_actions_default_is_noop() {
    let repo = FallbackOnlyWorkspaceRepository;
    let conversation_id = ChatConversationId::from_string("fallback-actions".to_string());

    repo.supersede_pending_pr_review_actions_except_head(&conversation_id, 12, "head-sha")
        .await
        .expect("default action supersession should be a compatibility no-op");
}
