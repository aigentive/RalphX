use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::entities::{
    AgentConversationWorkspace, AgentConversationWorkspacePublicationEvent,
    AgentConversationWorkspaceStatus, AgentWorkspaceFollowupProvenance,
    AgentWorkspacePrCommentEvidence, AgentWorkspacePrCommentEvidenceUpsert,
    AgentWorkspacePrDescription, AgentWorkspacePrReviewAction, AgentWorkspacePrReviewActionStatus,
    AgentWorkspacePrReviewMonitor, AgentWorkspaceReviewHunkAnnotation, AgentWorkspaceReviewMonitor,
    ChatConversationId, IdeationSessionId, PlanBranchId, ProjectId,
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

    async fn get_by_linked_ideation_session_id(
        &self,
        _ideation_session_id: &IdeationSessionId,
    ) -> AppResult<Option<AgentConversationWorkspace>> {
        Ok(None)
    }

    async fn get_by_project_id(
        &self,
        project_id: &ProjectId,
    ) -> AppResult<Vec<AgentConversationWorkspace>>;

    async fn find_active_by_project_and_branch_name(
        &self,
        project_id: &ProjectId,
        branch_name: &str,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        let branch_name = branch_name.trim();
        if branch_name.is_empty() {
            return Ok(Vec::new());
        }
        let workspaces = self.get_by_project_id(project_id).await?;
        Ok(workspaces
            .into_iter()
            .filter(|workspace| {
                workspace.status == AgentConversationWorkspaceStatus::Active
                    && workspace.branch_name == branch_name
            })
            .collect())
    }

    async fn save_followup_provenance(
        &self,
        _conversation_id: &ChatConversationId,
        _provenance: AgentWorkspaceFollowupProvenance,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn find_active_followup_by_blocker(
        &self,
        _origin_conversation_id: &ChatConversationId,
        _source_task_id: &str,
        _blocker_fingerprint: &str,
    ) -> AppResult<Option<AgentConversationWorkspace>> {
        Ok(None)
    }

    /// Project-scoped lookup of workspaces whose branch matches `head_ref`.
    ///
    /// Powers the PR-detail "attached RalphX conversations" rollup: workspaces
    /// where `project_id == project_id` AND `branch_name == head_ref` (each may
    /// also carry a `linked_ideation_session_id`). Project scope is MANDATORY —
    /// `branch_name` is not globally unique, so an unscoped lookup would
    /// cross-attach conversations from unrelated projects. The default filters
    /// the project's workspaces in memory; SQLite overrides with a scoped query.
    async fn find_by_head_ref(
        &self,
        project_id: &ProjectId,
        head_ref: &str,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        let workspaces = self.get_by_project_id(project_id).await?;
        Ok(workspaces
            .into_iter()
            .filter(|workspace| workspace.branch_name == head_ref)
            .collect())
    }

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

    async fn list_active_pr_poller_recovery_workspaces(
        &self,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        self.list_active_direct_published_workspaces().await
    }

    async fn list_active_direct_external_pr_reconciliation_candidates(
        &self,
        limit: usize,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        let _ = limit;
        Ok(Vec::new())
    }

    async fn list_active_direct_pr_supervision_recovery_candidates(
        &self,
        limit: usize,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        let _ = limit;
        Ok(Vec::new())
    }

    async fn list_active_linked_plan_pr_supervision_recovery_candidates(
        &self,
        limit: usize,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        let _ = limit;
        Ok(Vec::new())
    }

    async fn list_active_needs_agent_workspaces(
        &self,
    ) -> AppResult<Vec<AgentConversationWorkspace>>;

    async fn list_active_transient_publish_status_workspaces(
        &self,
        stale_older_than_secs: u64,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        let _ = stale_older_than_secs;
        Ok(Vec::new())
    }

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

    async fn update_pr_supervision_preferences(
        &self,
        conversation_id: &ChatConversationId,
        autofix_enabled: bool,
        auto_merge_desired: bool,
        auto_merge_method: &str,
    ) -> AppResult<()>;

    async fn update_auto_publish_preferences(
        &self,
        _conversation_id: &ChatConversationId,
        _auto_publish_enabled: bool,
        _paused_pr_autofix_enabled: Option<bool>,
        _paused_pr_auto_merge_desired: Option<bool>,
        _pr_autofix_enabled: bool,
        _pr_auto_merge_desired: bool,
        _pr_supervision_status: Option<&str>,
        _pr_supervision_summary: Option<&str>,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn update_auto_publish_initial_pr_preference(
        &self,
        _conversation_id: &ChatConversationId,
        _enabled: bool,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn update_pr_auto_merge_state(
        &self,
        _conversation_id: &ChatConversationId,
        _auto_merge_current: Option<bool>,
        _status: Option<&str>,
        _summary: Option<&str>,
    ) -> AppResult<()> {
        Ok(())
    }

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

    async fn upsert_pr_comment_evidence(
        &self,
        _conversation_id: &ChatConversationId,
        _comments: Vec<AgentWorkspacePrCommentEvidenceUpsert>,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn list_pr_comment_evidence(
        &self,
        _conversation_id: &ChatConversationId,
        _pr_number: i64,
        _limit: usize,
    ) -> AppResult<Vec<AgentWorkspacePrCommentEvidence>> {
        Ok(Vec::new())
    }

    async fn get_pr_comment_evidence(
        &self,
        _conversation_id: &ChatConversationId,
        _pr_number: i64,
        _comment_id: &str,
    ) -> AppResult<Option<AgentWorkspacePrCommentEvidence>> {
        Ok(None)
    }

    async fn mark_pr_comments_included(
        &self,
        _conversation_id: &ChatConversationId,
        _pr_number: i64,
        _comment_ids: &[String],
    ) -> AppResult<()> {
        Ok(())
    }

    async fn mark_pr_comment_read(
        &self,
        _conversation_id: &ChatConversationId,
        _pr_number: i64,
        _comment_id: &str,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn upsert_pr_review_monitor(
        &self,
        monitor: AgentWorkspacePrReviewMonitor,
    ) -> AppResult<AgentWorkspacePrReviewMonitor> {
        Ok(monitor)
    }

    async fn get_pr_review_monitor(
        &self,
        _conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentWorkspacePrReviewMonitor>> {
        Ok(None)
    }

    async fn set_pr_review_auto_approve_enabled(
        &self,
        conversation_id: &ChatConversationId,
        enabled: bool,
    ) -> AppResult<AgentWorkspacePrReviewMonitor>;

    async fn mark_pr_review_first_action_resolved(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<AgentWorkspacePrReviewMonitor>;

    async fn list_active_pr_review_monitors(
        &self,
    ) -> AppResult<Vec<AgentWorkspacePrReviewMonitor>> {
        Ok(Vec::new())
    }

    async fn upsert_workspace_review_monitor(
        &self,
        monitor: AgentWorkspaceReviewMonitor,
    ) -> AppResult<AgentWorkspaceReviewMonitor> {
        Ok(monitor)
    }

    async fn get_workspace_review_monitor(
        &self,
        _conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentWorkspaceReviewMonitor>> {
        Ok(None)
    }

    async fn list_reviewing_workspace_review_monitors(
        &self,
    ) -> AppResult<Vec<AgentWorkspaceReviewMonitor>> {
        Ok(Vec::new())
    }

    async fn replace_workspace_review_hunk_annotations(
        &self,
        _conversation_id: &ChatConversationId,
        _artifact_id: &crate::entities::ArtifactId,
        _annotations: Vec<AgentWorkspaceReviewHunkAnnotation>,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn list_workspace_review_hunk_annotations(
        &self,
        _conversation_id: &ChatConversationId,
        _artifact_id: &crate::entities::ArtifactId,
    ) -> AppResult<Vec<AgentWorkspaceReviewHunkAnnotation>> {
        Ok(Vec::new())
    }

    async fn create_or_update_pr_review_action(
        &self,
        action: AgentWorkspacePrReviewAction,
    ) -> AppResult<AgentWorkspacePrReviewAction> {
        Ok(action)
    }

    async fn get_pr_review_action(
        &self,
        _action_id: &str,
    ) -> AppResult<Option<AgentWorkspacePrReviewAction>> {
        Ok(None)
    }

    async fn get_pending_pr_review_action_for_head(
        &self,
        _conversation_id: &ChatConversationId,
        _pr_number: i64,
        _head_sha: &str,
    ) -> AppResult<Option<AgentWorkspacePrReviewAction>> {
        Ok(None)
    }

    async fn list_pr_review_actions(
        &self,
        _conversation_id: &ChatConversationId,
        _limit: usize,
    ) -> AppResult<Vec<AgentWorkspacePrReviewAction>> {
        Ok(Vec::new())
    }

    async fn update_pr_review_action_status(
        &self,
        _action_id: &str,
        _status: AgentWorkspacePrReviewActionStatus,
        _submitted_review_id: Option<&str>,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn claim_pending_pr_review_action(&self, action_id: &str) -> AppResult<bool>;

    async fn delete(&self, conversation_id: &ChatConversationId) -> AppResult<()>;

    async fn list_worktree_paths_by_project_id(
        &self,
        project_id: &ProjectId,
    ) -> AppResult<std::collections::HashSet<String>> {
        let workspaces = self.get_by_project_id(project_id).await?;
        Ok(workspaces.into_iter().map(|w| w.worktree_path).collect())
    }
}
