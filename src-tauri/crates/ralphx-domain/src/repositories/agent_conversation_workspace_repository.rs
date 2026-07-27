use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::entities::{
    AgentConversationWorkspace, AgentConversationWorkspacePublicationEvent,
    AgentConversationWorkspaceStatus, AgentWorkspaceFollowupProvenance,
    AgentWorkspacePrCommentEvidence, AgentWorkspacePrCommentEvidenceUpsert,
    AgentWorkspacePrDescription, AgentWorkspacePrReviewAction, AgentWorkspacePrReviewActionStatus,
    AgentWorkspacePrReviewMonitor, AgentWorkspaceReviewApprovalSnapshot,
    AgentWorkspaceReviewAutoMergeGuard, AgentWorkspaceReviewFixerSnapshot,
    AgentWorkspaceReviewHunkAnnotation, AgentWorkspaceReviewMonitor, ChatConversationId,
    IdeationSessionId, PlanBranchId, ProjectId,
};
use crate::error::AppResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentWorkspaceLocalCleanupClaim {
    Claimed,
    AlreadyInProgress,
    AlreadyCleaned,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentWorkspacePrTerminalSettlement {
    pub superseded_action_ids: Vec<String>,
    pub event_inserted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentWorkspacePrReviewActionMutation {
    UpsertPending(AgentWorkspacePrReviewAction),
    CompareAndSet {
        action_id: String,
        expected: AgentWorkspacePrReviewActionStatus,
        status: AgentWorkspacePrReviewActionStatus,
        submitted_review_id: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentWorkspacePrReviewStateTransition {
    pub monitor: AgentWorkspacePrReviewMonitor,
    pub action: Option<AgentWorkspacePrReviewAction>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentWorkspaceRepairStateGuard {
    pub publication_push_status: Option<String>,
    pub pr_supervision_status: Option<String>,
    pub pr_supervision_updated_at: Option<DateTime<Utc>>,
}

impl AgentWorkspaceRepairStateGuard {
    pub fn from_workspace(workspace: &AgentConversationWorkspace) -> Self {
        Self {
            publication_push_status: workspace.publication_push_status.clone(),
            pr_supervision_status: workspace.pr_supervision_status.clone(),
            pr_supervision_updated_at: workspace.pr_supervision_updated_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentWorkspaceRepairStateTransition {
    pub publication_push_status: Option<String>,
    pub pr_supervision_status: Option<String>,
    pub pr_supervision_summary: Option<String>,
    pub pr_supervision_updated_at: DateTime<Utc>,
    pub pr_auto_merge_current: Option<bool>,
    pub base_commit: Option<String>,
}

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

    async fn get_by_task_pipeline_session_id(
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

    async fn claim_local_cleanup(
        &self,
        conversation_id: &ChatConversationId,
        claimed_at: DateTime<Utc>,
        _stale_before: DateTime<Utc>,
    ) -> AppResult<AgentWorkspaceLocalCleanupClaim> {
        self.mark_local_cleanup_status(conversation_id, "cleaning", claimed_at)
            .await?;
        Ok(AgentWorkspaceLocalCleanupClaim::Claimed)
    }

    async fn finalize_local_cleanup(
        &self,
        conversation_id: &ChatConversationId,
        claimed_at: DateTime<Utc>,
        status: &str,
        checked_at: DateTime<Utc>,
    ) -> AppResult<bool> {
        let _ = claimed_at;
        self.mark_local_cleanup_status(conversation_id, status, checked_at)
            .await?;
        Ok(true)
    }

    async fn get_local_cleanup_status(
        &self,
        _conversation_id: &ChatConversationId,
    ) -> AppResult<Option<String>> {
        Ok(None)
    }

    async fn clear_local_cleanup_status(
        &self,
        _conversation_id: &ChatConversationId,
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

    async fn restore_after_restart(
        &self,
        conversation_id: &ChatConversationId,
        ideation_session_id: &IdeationSessionId,
        plan_branch_id: &PlanBranchId,
    ) -> AppResult<()> {
        self.update_links(
            conversation_id,
            Some(ideation_session_id),
            Some(plan_branch_id),
        )
        .await?;
        self.update_status(conversation_id, AgentConversationWorkspaceStatus::Active)
            .await?;
        self.clear_local_cleanup_status(conversation_id).await
    }

    async fn update_publication(
        &self,
        conversation_id: &ChatConversationId,
        pr_number: Option<i64>,
        pr_url: Option<&str>,
        pr_status: Option<&str>,
        push_status: Option<&str>,
    ) -> AppResult<()>;

    /// Persist authoritative pushed-SHA evidence only while the caller still
    /// owns the exact active workspace branch it pushed.
    async fn set_publication_pushed_sha(
        &self,
        _conversation_id: &ChatConversationId,
        _expected_branch_name: &str,
        _pushed_sha: &str,
    ) -> AppResult<bool> {
        Ok(false)
    }

    async fn clear_publication_pushed_sha(
        &self,
        _conversation_id: &ChatConversationId,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn compare_and_set_repair_state(
        &self,
        _conversation_id: &ChatConversationId,
        _expected: &AgentWorkspaceRepairStateGuard,
        _transition: &AgentWorkspaceRepairStateTransition,
    ) -> AppResult<bool> {
        Ok(false)
    }

    async fn compare_and_set_repair_state_with_events(
        &self,
        _conversation_id: &ChatConversationId,
        _expected: &AgentWorkspaceRepairStateGuard,
        _transition: &AgentWorkspaceRepairStateTransition,
        _events: Vec<AgentConversationWorkspacePublicationEvent>,
    ) -> AppResult<bool> {
        Ok(false)
    }

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

    async fn save_pr_metadata_decision(
        &self,
        conversation_id: &ChatConversationId,
        decision: crate::entities::AgentWorkspacePrMetadataDecision,
    ) -> AppResult<()> {
        let crate::entities::AgentWorkspacePrMetadataDecision::Patch {
            title,
            body_markdown,
        } = decision
        else {
            return Err(crate::error::AppError::Validation(
                "preserve metadata decisions require a repository implementation".to_string(),
            ));
        };
        let Some(body_markdown) = body_markdown else {
            return Err(crate::error::AppError::Validation(
                "legacy PR description storage requires a body".to_string(),
            ));
        };
        self.save_pr_description(
            conversation_id,
            AgentWorkspacePrDescription::new(title, body_markdown),
        )
        .await
    }

    async fn get_pr_metadata_decision(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<crate::entities::AgentWorkspacePrMetadataDecision>> {
        Ok(self
            .get_pr_description(conversation_id)
            .await?
            .map(
                |description| crate::entities::AgentWorkspacePrMetadataDecision::Patch {
                    title: description.title,
                    body_markdown: Some(description.body_markdown),
                },
            ))
    }

    async fn clear_pr_metadata_decision(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<()> {
        self.clear_pr_description(conversation_id).await
    }

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

    async fn set_pr_review_monitor_enabled(
        &self,
        _conversation_id: &ChatConversationId,
        _enabled: bool,
    ) -> AppResult<AgentWorkspacePrReviewMonitor> {
        Err(crate::error::AppError::Infrastructure(
            "PR review monitor settings are unsupported by this repository".to_string(),
        ))
    }

    async fn supersede_pending_pr_review_actions_except_head(
        &self,
        _conversation_id: &ChatConversationId,
        _pr_number: i64,
        _head_sha: &str,
    ) -> AppResult<Vec<String>> {
        Ok(Vec::new())
    }

    async fn mark_pr_review_first_action_resolved(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<AgentWorkspacePrReviewMonitor>;

    async fn list_active_pr_review_monitors(
        &self,
    ) -> AppResult<Vec<AgentWorkspacePrReviewMonitor>> {
        Ok(Vec::new())
    }

    /// Review PR lifecycle candidates, including monitors whose new-head dispatch is paused.
    async fn list_pr_review_lifecycle_monitors(
        &self,
    ) -> AppResult<Vec<AgentWorkspacePrReviewMonitor>> {
        self.list_active_pr_review_monitors().await
    }

    /// Startup repair candidates include both nonterminal lifecycle monitors and legacy rows whose
    /// workspace or monitor already carries terminal authority.
    async fn list_pr_review_lifecycle_recovery_workspaces(
        &self,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        Ok(Vec::new())
    }

    /// Rearms only a legacy terminal-monitor/nonterminal-workspace split after a live GitHub Open
    /// observation. Normal paused monitors never use this repair transition.
    async fn rearm_terminal_pr_review_monitor_after_live_open(
        &self,
        _conversation_id: &ChatConversationId,
        _pr_number: i64,
    ) -> AppResult<Option<AgentWorkspacePrReviewMonitor>> {
        Ok(None)
    }

    /// Atomically settles every durable Review PR projection after GitHub reports a terminal PR.
    async fn settle_pr_review_terminal(
        &self,
        _conversation_id: &ChatConversationId,
        _pr_number: i64,
        _status: &str,
        _summary: &str,
    ) -> AppResult<AgentWorkspacePrTerminalSettlement> {
        Err(crate::error::AppError::Infrastructure(
            "Review PR terminal settlement is unsupported by this repository".to_string(),
        ))
    }

    /// Atomically settles one Review PR monitor transition and optional action mutation while the
    /// exact workspace/PR and monitor remain nonterminal. `None` means stale or terminal authority
    /// rejected the transition without changing either row.
    async fn transition_pr_review_state_if_nonterminal(
        &self,
        _monitor: AgentWorkspacePrReviewMonitor,
        _action: Option<AgentWorkspacePrReviewActionMutation>,
    ) -> AppResult<Option<AgentWorkspacePrReviewStateTransition>> {
        Err(crate::error::AppError::Infrastructure(
            "Guarded Review PR state transitions are unsupported by this repository".to_string(),
        ))
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

    /// Atomically reserves the exact current blocking Review for one repair attempt.
    async fn claim_workspace_review_fixer(
        &self,
        _conversation_id: &ChatConversationId,
        _snapshot: &AgentWorkspaceReviewFixerSnapshot,
        _attempt_id: &str,
        _claimed_at: DateTime<Utc>,
    ) -> AppResult<Option<AgentWorkspaceReviewMonitor>> {
        Ok(None)
    }

    /// Writes a repair routing result only while the same backend attempt owns the claim.
    async fn settle_workspace_review_fixer_attempt(
        &self,
        _monitor: AgentWorkspaceReviewMonitor,
        _expected_attempt_id: &str,
        _expected_snapshot: &AgentWorkspaceReviewFixerSnapshot,
    ) -> AppResult<Option<AgentWorkspaceReviewMonitor>> {
        Ok(None)
    }

    /// Fails a malformed active repair attempt only while the same attempt still owns the row.
    async fn fail_invalid_workspace_review_fixer_attempt(
        &self,
        _conversation_id: &ChatConversationId,
        _expected_attempt_id: Option<&str>,
        _error: &str,
    ) -> AppResult<Option<AgentWorkspaceReviewMonitor>> {
        Ok(None)
    }

    /// Atomically records a reviewer launch failure only while the exact reserved launch still
    /// owns the monitor. Returns `false` when a newer launch or target has superseded it.
    async fn fail_reserved_workspace_review_start(
        &self,
        _conversation_id: &ChatConversationId,
        _expected_target_scope: crate::entities::AgentWorkspaceReviewTargetScope,
        _expected_diff_fingerprint: &str,
        _expected_review_conversation_id: &ChatConversationId,
        _expected_run_id: &str,
        _error: &str,
    ) -> AppResult<bool> {
        Ok(false)
    }

    async fn approve_workspace_review_anyway(
        &self,
        _conversation_id: &ChatConversationId,
        _snapshot: &AgentWorkspaceReviewApprovalSnapshot,
        _approved_at: DateTime<Utc>,
    ) -> AppResult<Option<AgentWorkspaceReviewMonitor>> {
        Err(crate::error::AppError::Infrastructure(
            "Workspace Review approval is unsupported by this repository".to_string(),
        ))
    }

    async fn list_reviewing_workspace_review_monitors(
        &self,
    ) -> AppResult<Vec<AgentWorkspaceReviewMonitor>> {
        Ok(Vec::new())
    }

    /// Lists active fixer attempts that require startup reconciliation.
    async fn list_active_workspace_review_fixers(
        &self,
    ) -> AppResult<Vec<AgentWorkspaceReviewMonitor>> {
        Ok(Vec::new())
    }

    /// Atomically changes the guard only when the expected guard still owns the monitor.
    async fn compare_and_set_workspace_review_auto_merge_guard(
        &self,
        _conversation_id: &ChatConversationId,
        _expected: Option<AgentWorkspaceReviewAutoMergeGuard>,
        _next: Option<AgentWorkspaceReviewAutoMergeGuard>,
    ) -> AppResult<bool> {
        Ok(false)
    }

    /// Atomically records a confirmed restoration only while the captured guard still owns the
    /// monitor and the user still wants GitHub auto-merge enabled.
    async fn complete_workspace_review_auto_merge_restore(
        &self,
        _conversation_id: &ChatConversationId,
        _expected: AgentWorkspaceReviewAutoMergeGuard,
    ) -> AppResult<bool> {
        Ok(false)
    }

    async fn list_active_workspace_review_auto_merge_guards(
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

    async fn create_or_update_pr_review_action_if_nonterminal(
        &self,
        action: AgentWorkspacePrReviewAction,
    ) -> AppResult<AgentWorkspacePrReviewAction> {
        self.create_or_update_pr_review_action(action).await
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

    async fn get_latest_pending_pr_review_action(
        &self,
        _conversation_id: &ChatConversationId,
        _pr_number: i64,
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

    async fn claim_pending_pr_review_action_if_nonterminal(
        &self,
        action_id: &str,
        _conversation_id: &ChatConversationId,
        _pr_number: i64,
    ) -> AppResult<bool> {
        self.claim_pending_pr_review_action(action_id).await
    }

    async fn compare_and_set_pr_review_action_status(
        &self,
        action_id: &str,
        expected: AgentWorkspacePrReviewActionStatus,
        status: AgentWorkspacePrReviewActionStatus,
        submitted_review_id: Option<&str>,
    ) -> AppResult<bool> {
        let Some(action) = self.get_pr_review_action(action_id).await? else {
            return Ok(false);
        };
        if action.status != expected {
            return Ok(false);
        }
        self.update_pr_review_action_status(action_id, status, submitted_review_id)
            .await?;
        Ok(true)
    }

    async fn delete(&self, conversation_id: &ChatConversationId) -> AppResult<()>;

    async fn list_worktree_paths_by_project_id(
        &self,
        project_id: &ProjectId,
    ) -> AppResult<std::collections::HashSet<String>> {
        let workspaces = self.get_by_project_id(project_id).await?;
        Ok(workspaces.into_iter().map(|w| w.worktree_path).collect())
    }
}
