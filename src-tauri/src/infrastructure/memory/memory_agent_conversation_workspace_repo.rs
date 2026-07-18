use std::collections::{HashMap, HashSet};
#[cfg(test)]
use std::sync::Mutex;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tokio::sync::RwLock;

use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode,
    AgentConversationWorkspacePublicationEvent, AgentConversationWorkspaceStatus,
    AgentWorkspaceFollowupProvenance, AgentWorkspacePrCommentEvidence,
    AgentWorkspacePrCommentEvidenceUpsert, AgentWorkspacePrDescription,
    AgentWorkspacePrReviewAction, AgentWorkspacePrReviewActionStatus,
    AgentWorkspacePrReviewMonitor, AgentWorkspacePrReviewMonitorStatus,
    AgentWorkspaceReviewApprovalSnapshot, AgentWorkspaceReviewAutoMergeGuard,
    AgentWorkspaceReviewGateStatus, AgentWorkspaceReviewHunkAnnotation,
    AgentWorkspaceReviewMonitor, AgentWorkspaceReviewMonitorStatus, AgentWorkspaceReviewOutcome,
    AgentWorkspaceReviewTargetScope, ArtifactId, ChatConversationId, IdeationSessionId,
    PlanBranchId, ProjectId, DEFAULT_AGENT_WORKSPACE_PR_AUTO_MERGE_METHOD,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, AgentWorkspaceLocalCleanupClaim,
};
use crate::error::{AppError, AppResult};

#[cfg(test)]
#[path = "memory_agent_conversation_workspace_repo_tests.rs"]
mod memory_agent_conversation_workspace_repo_tests;

pub struct MemoryAgentConversationWorkspaceRepository {
    workspaces: RwLock<HashMap<ChatConversationId, AgentConversationWorkspace>>,
    followup_provenance: RwLock<HashMap<ChatConversationId, AgentWorkspaceFollowupProvenance>>,
    pr_descriptions: RwLock<HashMap<ChatConversationId, AgentWorkspacePrDescription>>,
    publication_events:
        RwLock<HashMap<ChatConversationId, Vec<AgentConversationWorkspacePublicationEvent>>>,
    pr_comment_evidence: RwLock<HashMap<(String, i64, String), AgentWorkspacePrCommentEvidence>>,
    pr_review_monitors: RwLock<HashMap<ChatConversationId, AgentWorkspacePrReviewMonitor>>,
    workspace_review_monitors: RwLock<HashMap<ChatConversationId, AgentWorkspaceReviewMonitor>>,
    workspace_review_hunk_annotations:
        RwLock<HashMap<(ChatConversationId, ArtifactId), Vec<AgentWorkspaceReviewHunkAnnotation>>>,
    pr_review_actions: RwLock<HashMap<String, AgentWorkspacePrReviewAction>>,
    local_cleanup_markers: RwLock<HashMap<ChatConversationId, (String, DateTime<Utc>)>>,
    #[cfg(test)]
    next_pr_supervision_preference_error: Mutex<Option<String>>,
    #[cfg(test)]
    next_publication_event_error: Mutex<Option<String>>,
    #[cfg(test)]
    next_publication_update_error: Mutex<Option<String>>,
    #[cfg(test)]
    next_worktree_path_list_error: Mutex<Option<String>>,
    #[cfg(test)]
    next_auto_merge_restore_completion_error: Mutex<Option<String>>,
}

impl MemoryAgentConversationWorkspaceRepository {
    pub fn new() -> Self {
        Self {
            workspaces: RwLock::new(HashMap::new()),
            followup_provenance: RwLock::new(HashMap::new()),
            pr_descriptions: RwLock::new(HashMap::new()),
            publication_events: RwLock::new(HashMap::new()),
            pr_comment_evidence: RwLock::new(HashMap::new()),
            pr_review_monitors: RwLock::new(HashMap::new()),
            workspace_review_monitors: RwLock::new(HashMap::new()),
            workspace_review_hunk_annotations: RwLock::new(HashMap::new()),
            pr_review_actions: RwLock::new(HashMap::new()),
            local_cleanup_markers: RwLock::new(HashMap::new()),
            #[cfg(test)]
            next_pr_supervision_preference_error: Mutex::new(None),
            #[cfg(test)]
            next_publication_event_error: Mutex::new(None),
            #[cfg(test)]
            next_publication_update_error: Mutex::new(None),
            #[cfg(test)]
            next_worktree_path_list_error: Mutex::new(None),
            #[cfg(test)]
            next_auto_merge_restore_completion_error: Mutex::new(None),
        }
    }

    #[cfg(test)]
    pub fn fail_next_pr_supervision_preference_update(&self, message: impl Into<String>) {
        *self.next_pr_supervision_preference_error.lock().unwrap() = Some(message.into());
    }

    #[cfg(test)]
    pub fn fail_next_publication_event(&self, message: impl Into<String>) {
        *self.next_publication_event_error.lock().unwrap() = Some(message.into());
    }

    #[cfg(test)]
    pub fn fail_next_publication_update(&self, message: impl Into<String>) {
        *self.next_publication_update_error.lock().unwrap() = Some(message.into());
    }

    #[cfg(test)]
    pub fn fail_next_worktree_path_list(&self, message: impl Into<String>) {
        *self.next_worktree_path_list_error.lock().unwrap() = Some(message.into());
    }

    #[cfg(test)]
    pub fn fail_next_auto_merge_restore_completion(&self, message: impl Into<String>) {
        *self
            .next_auto_merge_restore_completion_error
            .lock()
            .unwrap() = Some(message.into());
    }

    pub async fn local_cleanup_status_for_test(
        &self,
        conversation_id: &ChatConversationId,
    ) -> Option<String> {
        self.local_cleanup_markers
            .read()
            .await
            .get(conversation_id)
            .map(|(status, _)| status.clone())
    }
}

impl Default for MemoryAgentConversationWorkspaceRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AgentConversationWorkspaceRepository for MemoryAgentConversationWorkspaceRepository {
    async fn create_or_update(
        &self,
        mut workspace: AgentConversationWorkspace,
    ) -> AppResult<AgentConversationWorkspace> {
        let mut workspaces = self.workspaces.write().await;
        if let Some(existing) = workspaces.get(&workspace.conversation_id) {
            workspace.created_at = existing.created_at;
        }
        workspace.updated_at = Utc::now();
        workspaces.insert(workspace.conversation_id, workspace.clone());
        Ok(workspace)
    }

    async fn get_by_conversation_id(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentConversationWorkspace>> {
        Ok(self.workspaces.read().await.get(conversation_id).cloned())
    }

    async fn get_by_project_id(
        &self,
        project_id: &ProjectId,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        Ok(self
            .workspaces
            .read()
            .await
            .values()
            .filter(|workspace| workspace.project_id == *project_id)
            .cloned()
            .collect())
    }

    async fn get_terminal_local_cleanup_candidates_by_project_id(
        &self,
        project_id: &ProjectId,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        let retry_secs = crate::infrastructure::agents::claude::git_runtime_config()
            .terminal_pr_local_cleanup_retry_secs;
        let retry_secs = i64::try_from(retry_secs).unwrap_or(i64::MAX);
        let retry_cutoff = Utc::now() - chrono::Duration::seconds(retry_secs);
        let markers = self.local_cleanup_markers.read().await;
        Ok(self
            .workspaces
            .read()
            .await
            .values()
            .filter(|workspace| workspace.project_id == *project_id)
            .filter(|workspace| {
                workspace.status == AgentConversationWorkspaceStatus::Archived
                    || workspace
                        .publication_pr_status
                        .as_deref()
                        .is_some_and(|status| matches!(status, "merged" | "closed"))
            })
            .filter(|workspace| match markers.get(&workspace.conversation_id) {
                None => true,
                Some((status, checked_at)) => {
                    matches!(
                        status.as_str(),
                        "pending"
                            | "failed"
                            | "failed_unsafe"
                            | "failed_operational"
                            | "unsafe"
                            | "target_ref_missing"
                            | "workspace_dirty"
                            | "branch_missing"
                            | "cleaning"
                    ) && *checked_at < retry_cutoff
                }
            })
            .cloned()
            .collect())
    }

    async fn mark_local_cleanup_status(
        &self,
        conversation_id: &ChatConversationId,
        status: &str,
        checked_at: DateTime<Utc>,
    ) -> AppResult<()> {
        self.local_cleanup_markers
            .write()
            .await
            .insert(conversation_id.clone(), (status.to_string(), checked_at));
        Ok(())
    }

    async fn claim_local_cleanup(
        &self,
        conversation_id: &ChatConversationId,
        claimed_at: DateTime<Utc>,
        stale_before: DateTime<Utc>,
    ) -> AppResult<AgentWorkspaceLocalCleanupClaim> {
        if !self.workspaces.read().await.contains_key(conversation_id) {
            return Err(AppError::NotFound(format!(
                "Agent conversation workspace not found while claiming local cleanup: {conversation_id}"
            )));
        }
        let mut markers = self.local_cleanup_markers.write().await;
        let claim = match markers.get(conversation_id) {
            Some((status, _)) if status == "cleaned" => {
                AgentWorkspaceLocalCleanupClaim::AlreadyCleaned
            }
            Some((status, checked_at)) if status == "cleaning" && *checked_at >= stale_before => {
                AgentWorkspaceLocalCleanupClaim::AlreadyInProgress
            }
            Some((status, _))
                if !matches!(
                    status.as_str(),
                    "cleaning"
                        | "pending"
                        | "failed"
                        | "failed_unsafe"
                        | "failed_operational"
                        | "unsafe"
                        | "target_ref_missing"
                        | "workspace_dirty"
                        | "branch_missing"
                ) =>
            {
                AgentWorkspaceLocalCleanupClaim::AlreadyInProgress
            }
            _ => {
                markers.insert(
                    conversation_id.clone(),
                    ("cleaning".to_string(), claimed_at),
                );
                AgentWorkspaceLocalCleanupClaim::Claimed
            }
        };
        Ok(claim)
    }

    async fn finalize_local_cleanup(
        &self,
        conversation_id: &ChatConversationId,
        claimed_at: DateTime<Utc>,
        status: &str,
        checked_at: DateTime<Utc>,
    ) -> AppResult<bool> {
        let mut markers = self.local_cleanup_markers.write().await;
        if markers
            .get(conversation_id)
            .is_none_or(|(current, current_claimed_at)| {
                current != "cleaning" || *current_claimed_at != claimed_at
            })
        {
            return Ok(false);
        }
        markers.insert(conversation_id.clone(), (status.to_string(), checked_at));
        Ok(true)
    }

    async fn get_local_cleanup_status(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<String>> {
        Ok(self
            .local_cleanup_markers
            .read()
            .await
            .get(conversation_id)
            .map(|(status, _)| status.clone()))
    }

    async fn clear_local_cleanup_status(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<()> {
        self.local_cleanup_markers
            .write()
            .await
            .remove(conversation_id);
        Ok(())
    }

    async fn get_by_linked_ideation_session_id(
        &self,
        ideation_session_id: &IdeationSessionId,
    ) -> AppResult<Option<AgentConversationWorkspace>> {
        Ok(self
            .workspaces
            .read()
            .await
            .values()
            .filter(|workspace| {
                workspace.linked_ideation_session_id.as_ref() == Some(ideation_session_id)
            })
            .max_by(|left, right| left.updated_at.cmp(&right.updated_at))
            .cloned())
    }

    async fn get_by_task_pipeline_session_id(
        &self,
        ideation_session_id: &IdeationSessionId,
    ) -> AppResult<Option<AgentConversationWorkspace>> {
        Ok(self
            .workspaces
            .read()
            .await
            .values()
            .filter(|workspace| {
                workspace.task_pipeline_session_id.as_ref() == Some(ideation_session_id)
            })
            .max_by(|left, right| left.updated_at.cmp(&right.updated_at))
            .cloned())
    }

    async fn save_followup_provenance(
        &self,
        conversation_id: &ChatConversationId,
        provenance: AgentWorkspaceFollowupProvenance,
    ) -> AppResult<()> {
        self.followup_provenance
            .write()
            .await
            .insert(conversation_id.clone(), provenance);
        Ok(())
    }

    async fn find_active_followup_by_blocker(
        &self,
        origin_conversation_id: &ChatConversationId,
        source_task_id: &str,
        blocker_fingerprint: &str,
    ) -> AppResult<Option<AgentConversationWorkspace>> {
        let provenance = self.followup_provenance.read().await;
        let workspaces = self.workspaces.read().await;
        Ok(provenance
            .iter()
            .filter_map(|(conversation_id, stored)| {
                if stored.origin_conversation_id != *origin_conversation_id
                    || stored.source_task_id.as_deref() != Some(source_task_id)
                    || stored.blocker_fingerprint.as_deref() != Some(blocker_fingerprint)
                {
                    return None;
                }
                workspaces.get(conversation_id).filter(|workspace| {
                    workspace.status == AgentConversationWorkspaceStatus::Active
                })
            })
            .max_by(|left, right| left.updated_at.cmp(&right.updated_at))
            .cloned())
    }

    async fn list_active_direct_published_workspaces(
        &self,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        Ok(self
            .workspaces
            .read()
            .await
            .values()
            .filter(|workspace| is_active_direct_published_workspace(workspace))
            .cloned()
            .collect())
    }

    async fn list_active_pr_poller_recovery_workspaces(
        &self,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        Ok(self
            .workspaces
            .read()
            .await
            .values()
            .filter(|workspace| is_active_pr_poller_recovery_workspace(workspace))
            .cloned()
            .collect())
    }

    async fn list_active_needs_agent_workspaces(
        &self,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        Ok(self
            .workspaces
            .read()
            .await
            .values()
            .filter(|workspace| is_active_needs_agent_workspace(workspace))
            .cloned()
            .collect())
    }

    async fn list_active_transient_publish_status_workspaces(
        &self,
        stale_older_than_secs: u64,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        let cutoff = Utc::now() - chrono::Duration::seconds(stale_older_than_secs as i64);
        Ok(self
            .workspaces
            .read()
            .await
            .values()
            .filter(|w| is_stale_transient_publish_status_workspace(w, cutoff))
            .cloned()
            .collect())
    }

    async fn list_active_direct_external_pr_reconciliation_candidates(
        &self,
        limit: usize,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        let mut workspaces = self
            .workspaces
            .read()
            .await
            .values()
            .filter(|workspace| is_active_direct_external_pr_reconciliation_candidate(workspace))
            .cloned()
            .collect::<Vec<_>>();
        workspaces.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
        workspaces.truncate(limit);
        Ok(workspaces)
    }

    async fn list_active_direct_pr_supervision_recovery_candidates(
        &self,
        limit: usize,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        let mut workspaces = self
            .workspaces
            .read()
            .await
            .values()
            .filter(|workspace| is_active_direct_pr_supervision_recovery_candidate(workspace))
            .cloned()
            .collect::<Vec<_>>();
        workspaces.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
        workspaces.truncate(limit);
        Ok(workspaces)
    }

    async fn list_active_linked_plan_pr_supervision_recovery_candidates(
        &self,
        limit: usize,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        let mut workspaces = self
            .workspaces
            .read()
            .await
            .values()
            .filter(|workspace| is_active_linked_plan_pr_supervision_recovery_candidate(workspace))
            .cloned()
            .collect::<Vec<_>>();
        workspaces.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
        workspaces.truncate(limit);
        Ok(workspaces)
    }

    async fn update_links(
        &self,
        conversation_id: &ChatConversationId,
        ideation_session_id: Option<&IdeationSessionId>,
        plan_branch_id: Option<&PlanBranchId>,
    ) -> AppResult<()> {
        if let Some(workspace) = self.workspaces.write().await.get_mut(conversation_id) {
            workspace.linked_ideation_session_id = ideation_session_id.cloned();
            workspace.linked_plan_branch_id = plan_branch_id.cloned();
            workspace.updated_at = Utc::now();
        }
        Ok(())
    }

    async fn restore_after_restart(
        &self,
        conversation_id: &ChatConversationId,
        ideation_session_id: &IdeationSessionId,
        plan_branch_id: &PlanBranchId,
    ) -> AppResult<()> {
        {
            let mut workspaces = self.workspaces.write().await;
            let workspace = workspaces.get_mut(conversation_id).ok_or_else(|| {
                AppError::NotFound(format!("Workspace not found: {conversation_id}"))
            })?;
            workspace.linked_ideation_session_id = Some(ideation_session_id.clone());
            workspace.linked_plan_branch_id = Some(plan_branch_id.clone());
            workspace.status = AgentConversationWorkspaceStatus::Active;
            workspace.updated_at = Utc::now();
        }
        self.local_cleanup_markers
            .write()
            .await
            .remove(conversation_id);
        Ok(())
    }

    async fn update_publication(
        &self,
        conversation_id: &ChatConversationId,
        pr_number: Option<i64>,
        pr_url: Option<&str>,
        pr_status: Option<&str>,
        push_status: Option<&str>,
    ) -> AppResult<()> {
        #[cfg(test)]
        if let Some(message) = self.next_publication_update_error.lock().unwrap().take() {
            return Err(AppError::Infrastructure(message));
        }

        if let Some(workspace) = self.workspaces.write().await.get_mut(conversation_id) {
            workspace.publication_pr_number = pr_number;
            workspace.publication_pr_url = pr_url.map(str::to_string);
            workspace.publication_pr_status = pr_status.map(str::to_string);
            workspace.publication_push_status = push_status.map(str::to_string);
            let now = Utc::now();
            if matches!(pr_status, Some("merged" | "closed")) {
                workspace.pr_supervision_status = None;
                workspace.pr_supervision_summary = None;
                workspace.pr_supervision_updated_at = Some(now);
            }
            workspace.updated_at = now;
        }
        Ok(())
    }

    async fn list_worktree_paths_by_project_id(
        &self,
        project_id: &ProjectId,
    ) -> AppResult<HashSet<String>> {
        #[cfg(test)]
        if let Some(message) = self.next_worktree_path_list_error.lock().unwrap().take() {
            return Err(AppError::Infrastructure(message));
        }

        Ok(self
            .workspaces
            .read()
            .await
            .values()
            .filter(|workspace| workspace.project_id == *project_id)
            .map(|workspace| workspace.worktree_path.clone())
            .collect())
    }

    async fn update_pr_supervision_preferences(
        &self,
        conversation_id: &ChatConversationId,
        autofix_enabled: bool,
        auto_merge_desired: bool,
        auto_merge_method: &str,
    ) -> AppResult<()> {
        #[cfg(test)]
        {
            let error = self
                .next_pr_supervision_preference_error
                .lock()
                .unwrap()
                .take();
            if let Some(message) = error {
                return Err(crate::error::AppError::Infrastructure(message));
            }
        }

        if let Some(workspace) = self.workspaces.write().await.get_mut(conversation_id) {
            workspace.pr_autofix_enabled = autofix_enabled;
            workspace.pr_auto_merge_desired = auto_merge_desired;
            let method = auto_merge_method.trim();
            workspace.pr_auto_merge_method = if method.is_empty() {
                DEFAULT_AGENT_WORKSPACE_PR_AUTO_MERGE_METHOD.to_string()
            } else {
                method.to_string()
            };
            workspace.pr_supervision_status = Some(
                if autofix_enabled || auto_merge_desired {
                    "monitoring"
                } else {
                    "disabled"
                }
                .to_string(),
            );
            workspace.pr_supervision_summary = (autofix_enabled || auto_merge_desired)
                .then(|| "RalphX PR supervision is enabled.".to_string());
            let now = Utc::now();
            workspace.pr_supervision_updated_at = Some(now);
            workspace.updated_at = now;
        }
        Ok(())
    }

    async fn update_pr_auto_merge_state(
        &self,
        conversation_id: &ChatConversationId,
        auto_merge_current: Option<bool>,
        status: Option<&str>,
        summary: Option<&str>,
    ) -> AppResult<()> {
        if let Some(workspace) = self.workspaces.write().await.get_mut(conversation_id) {
            workspace.pr_auto_merge_current = auto_merge_current;
            if let Some(status) = status {
                workspace.pr_supervision_status = Some(status.to_string());
            }
            if let Some(summary) = summary {
                workspace.pr_supervision_summary = Some(summary.to_string());
            }
            let now = Utc::now();
            workspace.pr_supervision_updated_at = Some(now);
            workspace.updated_at = now;
        }
        Ok(())
    }

    async fn update_auto_publish_preferences(
        &self,
        conversation_id: &ChatConversationId,
        auto_publish_enabled: bool,
        paused_pr_autofix_enabled: Option<bool>,
        paused_pr_auto_merge_desired: Option<bool>,
        pr_autofix_enabled: bool,
        pr_auto_merge_desired: bool,
        pr_supervision_status: Option<&str>,
        pr_supervision_summary: Option<&str>,
    ) -> AppResult<()> {
        if let Some(workspace) = self.workspaces.write().await.get_mut(conversation_id) {
            workspace.auto_publish_enabled = auto_publish_enabled;
            workspace.auto_publish_paused_pr_autofix_enabled = paused_pr_autofix_enabled;
            workspace.auto_publish_paused_pr_auto_merge_desired = paused_pr_auto_merge_desired;
            workspace.pr_autofix_enabled = pr_autofix_enabled;
            workspace.pr_auto_merge_desired = pr_auto_merge_desired;
            workspace.pr_supervision_status = pr_supervision_status.map(str::to_string);
            workspace.pr_supervision_summary = pr_supervision_summary.map(str::to_string);
            let now = Utc::now();
            workspace.pr_supervision_updated_at = Some(now);
            workspace.updated_at = now;
        }
        Ok(())
    }

    async fn update_auto_publish_initial_pr_preference(
        &self,
        conversation_id: &ChatConversationId,
        enabled: bool,
    ) -> AppResult<()> {
        if let Some(workspace) = self.workspaces.write().await.get_mut(conversation_id) {
            workspace.auto_publish_initial_pr_enabled = enabled;
            workspace.updated_at = Utc::now();
        }
        Ok(())
    }

    async fn update_status(
        &self,
        conversation_id: &ChatConversationId,
        status: AgentConversationWorkspaceStatus,
    ) -> AppResult<()> {
        if let Some(workspace) = self.workspaces.write().await.get_mut(conversation_id) {
            workspace.status = status;
            workspace.updated_at = Utc::now();
        }
        Ok(())
    }

    async fn save_pr_description(
        &self,
        conversation_id: &ChatConversationId,
        description: AgentWorkspacePrDescription,
    ) -> AppResult<()> {
        self.pr_descriptions
            .write()
            .await
            .insert(conversation_id.clone(), description);
        Ok(())
    }

    async fn get_pr_description(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentWorkspacePrDescription>> {
        Ok(self
            .pr_descriptions
            .read()
            .await
            .get(conversation_id)
            .cloned())
    }

    async fn clear_pr_description(&self, conversation_id: &ChatConversationId) -> AppResult<()> {
        self.pr_descriptions.write().await.remove(conversation_id);
        Ok(())
    }

    async fn append_publication_event(
        &self,
        event: AgentConversationWorkspacePublicationEvent,
    ) -> AppResult<()> {
        #[cfg(test)]
        if let Some(message) = self.next_publication_event_error.lock().unwrap().take() {
            return Err(AppError::Infrastructure(message));
        }
        self.publication_events
            .write()
            .await
            .entry(event.conversation_id)
            .or_default()
            .push(event);
        Ok(())
    }

    async fn list_publication_events(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Vec<AgentConversationWorkspacePublicationEvent>> {
        Ok(self
            .publication_events
            .read()
            .await
            .get(conversation_id)
            .cloned()
            .unwrap_or_default())
    }

    async fn upsert_pr_comment_evidence(
        &self,
        conversation_id: &ChatConversationId,
        comments: Vec<AgentWorkspacePrCommentEvidenceUpsert>,
    ) -> AppResult<()> {
        if comments.is_empty() {
            return Ok(());
        }
        let now = Utc::now();
        let conversation_key = conversation_id.as_str().to_string();
        let mut evidence = self.pr_comment_evidence.write().await;
        for comment in comments {
            let key = (
                conversation_key.clone(),
                comment.pr_number,
                comment.comment_id.clone(),
            );
            if let Some(existing) = evidence.get_mut(&key) {
                if existing.body_sha256 != comment.body_sha256 {
                    existing.edit_count += 1;
                }
                existing.author = comment.author;
                existing.body = comment.body;
                existing.body_excerpt = comment.body_excerpt;
                existing.body_sha256 = comment.body_sha256;
                existing.url = comment.url;
                existing.github_created_at = comment.github_created_at;
                existing.github_updated_at = comment.github_updated_at;
                existing.is_codecov = comment.is_codecov;
                existing.is_bot = comment.is_bot;
                existing.last_seen_at = now;
            } else {
                evidence.insert(
                    key,
                    AgentWorkspacePrCommentEvidence {
                        conversation_id: conversation_id.clone(),
                        pr_number: comment.pr_number,
                        comment_id: comment.comment_id,
                        author: comment.author,
                        body: comment.body,
                        body_excerpt: comment.body_excerpt,
                        body_sha256: comment.body_sha256,
                        url: comment.url,
                        github_created_at: comment.github_created_at,
                        github_updated_at: comment.github_updated_at,
                        is_codecov: comment.is_codecov,
                        is_bot: comment.is_bot,
                        first_seen_at: now,
                        last_seen_at: now,
                        last_included_at: None,
                        last_read_at: None,
                        edit_count: 0,
                    },
                );
            }
        }
        Ok(())
    }

    async fn list_pr_comment_evidence(
        &self,
        conversation_id: &ChatConversationId,
        pr_number: i64,
        limit: usize,
    ) -> AppResult<Vec<AgentWorkspacePrCommentEvidence>> {
        let conversation_key = conversation_id.as_str();
        let mut comments = self
            .pr_comment_evidence
            .read()
            .await
            .values()
            .filter(|comment| {
                comment.conversation_id.as_str() == conversation_key
                    && comment.pr_number == pr_number
            })
            .cloned()
            .collect::<Vec<_>>();
        comments.sort_by(|left, right| {
            right
                .github_updated_at
                .cmp(&left.github_updated_at)
                .then(right.last_seen_at.cmp(&left.last_seen_at))
                .then(right.comment_id.cmp(&left.comment_id))
        });
        comments.truncate(limit);
        Ok(comments)
    }

    async fn get_pr_comment_evidence(
        &self,
        conversation_id: &ChatConversationId,
        pr_number: i64,
        comment_id: &str,
    ) -> AppResult<Option<AgentWorkspacePrCommentEvidence>> {
        Ok(self
            .pr_comment_evidence
            .read()
            .await
            .get(&(
                conversation_id.as_str().to_string(),
                pr_number,
                comment_id.to_string(),
            ))
            .cloned())
    }

    async fn mark_pr_comments_included(
        &self,
        conversation_id: &ChatConversationId,
        pr_number: i64,
        comment_ids: &[String],
    ) -> AppResult<()> {
        let now = Utc::now();
        let conversation_key = conversation_id.as_str().to_string();
        let mut evidence = self.pr_comment_evidence.write().await;
        for comment_id in comment_ids {
            if let Some(comment) =
                evidence.get_mut(&(conversation_key.clone(), pr_number, comment_id.clone()))
            {
                comment.last_included_at = Some(now);
            }
        }
        Ok(())
    }

    async fn mark_pr_comment_read(
        &self,
        conversation_id: &ChatConversationId,
        pr_number: i64,
        comment_id: &str,
    ) -> AppResult<()> {
        let key = (
            conversation_id.as_str().to_string(),
            pr_number,
            comment_id.to_string(),
        );
        if let Some(comment) = self.pr_comment_evidence.write().await.get_mut(&key) {
            comment.last_read_at = Some(Utc::now());
        }
        Ok(())
    }

    async fn upsert_pr_review_monitor(
        &self,
        mut monitor: AgentWorkspacePrReviewMonitor,
    ) -> AppResult<AgentWorkspacePrReviewMonitor> {
        let mut monitors = self.pr_review_monitors.write().await;
        if let Some(existing) = monitors.get(&monitor.conversation_id) {
            if monitor.updated_at < existing.updated_at {
                return Ok(existing.clone());
            }
            monitor.created_at = existing.created_at;
            monitor.auto_approve_enabled = existing.auto_approve_enabled;
            monitor.first_action_resolved = existing.first_action_resolved;
            if !existing.monitor_enabled
                && monitor.monitor_enabled
                && matches!(
                    existing.status,
                    AgentWorkspacePrReviewMonitorStatus::Paused
                        | AgentWorkspacePrReviewMonitorStatus::Terminal
                )
            {
                monitor.monitor_enabled = false;
                monitor.status = existing.status;
            }
            if monitor.review_artifact_id.is_none() {
                monitor.review_artifact_id = existing.review_artifact_id.clone();
                monitor.review_artifact_head_sha = existing.review_artifact_head_sha.clone();
                monitor.review_artifact_version = existing.review_artifact_version;
                monitor.review_artifact_updated_at = existing.review_artifact_updated_at;
            }
        }
        monitor.updated_at = Utc::now();
        monitors.insert(monitor.conversation_id, monitor.clone());
        Ok(monitor)
    }

    async fn get_pr_review_monitor(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentWorkspacePrReviewMonitor>> {
        Ok(self
            .pr_review_monitors
            .read()
            .await
            .get(conversation_id)
            .cloned())
    }

    async fn set_pr_review_auto_approve_enabled(
        &self,
        conversation_id: &ChatConversationId,
        enabled: bool,
    ) -> AppResult<AgentWorkspacePrReviewMonitor> {
        let mut monitors = self.pr_review_monitors.write().await;
        let monitor = monitors
            .get_mut(conversation_id)
            .expect("PR review monitor must exist before updating Auto Approve");
        monitor.auto_approve_enabled = enabled;
        monitor.updated_at = Utc::now();
        Ok(monitor.clone())
    }

    async fn set_pr_review_monitor_enabled(
        &self,
        conversation_id: &ChatConversationId,
        enabled: bool,
    ) -> AppResult<AgentWorkspacePrReviewMonitor> {
        let mut monitors = self.pr_review_monitors.write().await;
        let monitor = monitors
            .get_mut(conversation_id)
            .expect("PR review monitor must exist before updating monitoring");
        monitor.monitor_enabled = enabled;
        monitor.status = if enabled {
            AgentWorkspacePrReviewMonitorStatus::Watching
        } else {
            AgentWorkspacePrReviewMonitorStatus::Paused
        };
        monitor.updated_at = Utc::now();
        Ok(monitor.clone())
    }

    async fn supersede_pending_pr_review_actions_except_head(
        &self,
        conversation_id: &ChatConversationId,
        pr_number: i64,
        head_sha: &str,
    ) -> AppResult<Vec<String>> {
        let mut actions = self.pr_review_actions.write().await;
        let mut superseded_ids = Vec::new();
        for action in actions.values_mut() {
            if action.conversation_id == *conversation_id
                && action.pr_number == pr_number
                && action.head_sha != head_sha
                && action.status == AgentWorkspacePrReviewActionStatus::Pending
            {
                superseded_ids.push(action.id.clone());
                action.status = AgentWorkspacePrReviewActionStatus::Superseded;
                action.resolved_at = Some(Utc::now());
                action.updated_at = Utc::now();
            }
        }
        Ok(superseded_ids)
    }

    async fn mark_pr_review_first_action_resolved(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<AgentWorkspacePrReviewMonitor> {
        let mut monitors = self.pr_review_monitors.write().await;
        let monitor = monitors
            .get_mut(conversation_id)
            .expect("PR review monitor must exist before resolving the first action");
        monitor.first_action_resolved = true;
        monitor.updated_at = Utc::now();
        Ok(monitor.clone())
    }

    async fn list_active_pr_review_monitors(
        &self,
    ) -> AppResult<Vec<AgentWorkspacePrReviewMonitor>> {
        let mut monitors = self
            .pr_review_monitors
            .read()
            .await
            .values()
            .filter(|monitor| {
                monitor.monitor_enabled
                    && !matches!(
                        monitor.status,
                        AgentWorkspacePrReviewMonitorStatus::Terminal
                    )
            })
            .cloned()
            .collect::<Vec<_>>();
        monitors.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
        Ok(monitors)
    }

    async fn upsert_workspace_review_monitor(
        &self,
        mut monitor: AgentWorkspaceReviewMonitor,
    ) -> AppResult<AgentWorkspaceReviewMonitor> {
        let mut monitors = self.workspace_review_monitors.write().await;
        if let Some(existing) = monitors.get(&monitor.conversation_id) {
            monitor.created_at = existing.created_at;
            if monitor.review_conversation_id.is_none() {
                monitor.review_conversation_id = existing.review_conversation_id.clone();
            }
            if monitor.review_artifact_id.is_none() {
                monitor.review_artifact_id = existing.review_artifact_id.clone();
                monitor.review_artifact_version = existing.review_artifact_version;
                monitor.review_artifact_updated_at = existing.review_artifact_updated_at;
            }
            if monitor.previous_version_id.is_none() {
                monitor.previous_version_id = existing.previous_version_id.clone();
            }
            // Guard transitions are exclusively compare-and-set operations. A normal Review
            // monitor upsert must not erase the durable GitHub auto-merge ownership record.
            monitor.auto_merge_guard = existing.auto_merge_guard.clone();
        }
        monitor.updated_at = Utc::now();
        monitors.insert(monitor.conversation_id, monitor.clone());
        Ok(monitor)
    }

    async fn get_workspace_review_monitor(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentWorkspaceReviewMonitor>> {
        Ok(self
            .workspace_review_monitors
            .read()
            .await
            .get(conversation_id)
            .cloned())
    }

    async fn approve_workspace_review_anyway(
        &self,
        conversation_id: &ChatConversationId,
        snapshot: &AgentWorkspaceReviewApprovalSnapshot,
        approved_at: DateTime<Utc>,
    ) -> AppResult<Option<AgentWorkspaceReviewMonitor>> {
        let workspaces = self.workspaces.read().await;
        let Some(workspace) = workspaces.get(conversation_id) else {
            return Ok(None);
        };
        if workspace_review_approval_publish_status_is_active(
            workspace.publication_push_status.as_deref(),
        ) {
            return Ok(None);
        }
        let mut monitors = self.workspace_review_monitors.write().await;
        let Some(monitor) = monitors.get_mut(conversation_id) else {
            return Ok(None);
        };
        let fixer_active = matches!(
            monitor.review_fixer_status.as_deref(),
            Some("routing" | "queued" | "running")
        );
        if monitor.status != AgentWorkspaceReviewMonitorStatus::Ready
            || monitor.review_outcome != AgentWorkspaceReviewOutcome::Blocking
            || monitor.review_gate_status != AgentWorkspaceReviewGateStatus::Blocking
            || monitor.current_target_scope != Some(snapshot.target_scope)
            || monitor.reviewed_target_scope != Some(snapshot.target_scope)
            || monitor.current_diff_fingerprint.as_deref()
                != Some(snapshot.diff_fingerprint.as_str())
            || monitor.reviewed_diff_fingerprint.as_deref()
                != Some(snapshot.diff_fingerprint.as_str())
            || monitor.review_artifact_id.as_ref() != Some(&snapshot.artifact_id)
            || monitor.review_artifact_version != Some(snapshot.artifact_version)
            || fixer_active
        {
            return Ok(None);
        }
        monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Passed;
        monitor.review_gate_bypassed_at = Some(approved_at);
        monitor.review_gate_bypassed_target_scope = Some(snapshot.target_scope);
        monitor.review_gate_bypassed_diff_fingerprint = Some(snapshot.diff_fingerprint.clone());
        monitor.review_gate_bypassed_artifact_id = Some(snapshot.artifact_id.clone());
        monitor.review_gate_bypassed_artifact_version = Some(snapshot.artifact_version);
        monitor.updated_at = approved_at;
        let updated = monitor.clone();
        self.publication_events
            .write()
            .await
            .entry(*conversation_id)
            .or_default()
            .push(snapshot.audit_event(*conversation_id, approved_at));
        Ok(Some(updated))
    }

    async fn list_reviewing_workspace_review_monitors(
        &self,
    ) -> AppResult<Vec<AgentWorkspaceReviewMonitor>> {
        let mut monitors = self
            .workspace_review_monitors
            .read()
            .await
            .values()
            .filter(|monitor| monitor.status == AgentWorkspaceReviewMonitorStatus::Reviewing)
            .cloned()
            .collect::<Vec<_>>();
        monitors.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
        Ok(monitors)
    }

    async fn compare_and_set_workspace_review_auto_merge_guard(
        &self,
        conversation_id: &ChatConversationId,
        expected: Option<AgentWorkspaceReviewAutoMergeGuard>,
        next: Option<AgentWorkspaceReviewAutoMergeGuard>,
    ) -> AppResult<bool> {
        let mut monitors = self.workspace_review_monitors.write().await;
        let Some(monitor) = monitors.get_mut(conversation_id) else {
            return Ok(false);
        };
        if monitor.auto_merge_guard != expected {
            return Ok(false);
        }
        monitor.auto_merge_guard = next;
        monitor.updated_at = Utc::now();
        Ok(true)
    }

    async fn complete_workspace_review_auto_merge_restore(
        &self,
        conversation_id: &ChatConversationId,
        expected: AgentWorkspaceReviewAutoMergeGuard,
    ) -> AppResult<bool> {
        #[cfg(test)]
        if let Some(message) = self
            .next_auto_merge_restore_completion_error
            .lock()
            .unwrap()
            .take()
        {
            return Err(AppError::Infrastructure(message));
        }
        let now = Utc::now();
        let mut workspaces = self.workspaces.write().await;
        let Some(workspace) = workspaces.get_mut(conversation_id) else {
            return Ok(false);
        };
        if !workspace.pr_auto_merge_desired
            || (expected.target_scope == AgentWorkspaceReviewTargetScope::WorkspaceDelta
                && (workspace.publication_pr_number != Some(expected.pr_number)
                    || workspace.has_terminal_publication_pr_status()))
        {
            return Ok(false);
        }
        let mut monitors = self.workspace_review_monitors.write().await;
        let Some(monitor) = monitors.get_mut(conversation_id) else {
            return Ok(false);
        };
        if monitor.auto_merge_guard != Some(expected.clone()) {
            return Ok(false);
        }
        if expected.target_scope == AgentWorkspaceReviewTargetScope::SelectedSource
            && (monitor.current_target_scope != Some(expected.target_scope)
                || monitor.current_diff_fingerprint.as_deref()
                    != Some(expected.diff_fingerprint.as_str())
                || monitor.selected_source_pull_request_number != Some(expected.pr_number)
                || monitor.selected_source_head_sha != expected.head_sha)
        {
            return Ok(false);
        }
        workspace.pr_auto_merge_current = Some(true);
        workspace.pr_supervision_status = Some("monitoring".to_string());
        workspace.pr_supervision_summary =
            Some("GitHub auto-merge was restored after the workspace Review passed.".to_string());
        workspace.pr_supervision_updated_at = Some(now);
        workspace.updated_at = now;
        monitor.auto_merge_guard = None;
        monitor.updated_at = now;
        Ok(true)
    }

    async fn list_active_workspace_review_auto_merge_guards(
        &self,
    ) -> AppResult<Vec<AgentWorkspaceReviewMonitor>> {
        let mut monitors = self
            .workspace_review_monitors
            .read()
            .await
            .values()
            .filter(|monitor| monitor.auto_merge_guard.is_some())
            .cloned()
            .collect::<Vec<_>>();
        monitors.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
        Ok(monitors)
    }

    async fn replace_workspace_review_hunk_annotations(
        &self,
        conversation_id: &ChatConversationId,
        artifact_id: &ArtifactId,
        annotations: Vec<AgentWorkspaceReviewHunkAnnotation>,
    ) -> AppResult<()> {
        self.workspace_review_hunk_annotations
            .write()
            .await
            .insert((conversation_id.clone(), artifact_id.clone()), annotations);
        Ok(())
    }

    async fn list_workspace_review_hunk_annotations(
        &self,
        conversation_id: &ChatConversationId,
        artifact_id: &ArtifactId,
    ) -> AppResult<Vec<AgentWorkspaceReviewHunkAnnotation>> {
        Ok(self
            .workspace_review_hunk_annotations
            .read()
            .await
            .get(&(conversation_id.clone(), artifact_id.clone()))
            .cloned()
            .unwrap_or_default())
    }

    async fn create_or_update_pr_review_action(
        &self,
        mut action: AgentWorkspacePrReviewAction,
    ) -> AppResult<AgentWorkspacePrReviewAction> {
        let mut actions = self.pr_review_actions.write().await;
        if let Some(existing) = actions.values_mut().find(|existing| {
            existing.conversation_id == action.conversation_id
                && existing.pr_number == action.pr_number
                && existing.head_sha == action.head_sha
                && existing.status == AgentWorkspacePrReviewActionStatus::Pending
        }) {
            existing.proposed_action = action.proposed_action;
            existing.summary = action.summary;
            existing.review_body = action.review_body;
            existing.findings_json = action.findings_json;
            existing.created_by_run_id = action.created_by_run_id;
            existing.updated_at = Utc::now();
            return Ok(existing.clone());
        }

        action.updated_at = Utc::now();
        actions.insert(action.id.clone(), action.clone());
        Ok(action)
    }

    async fn get_pr_review_action(
        &self,
        action_id: &str,
    ) -> AppResult<Option<AgentWorkspacePrReviewAction>> {
        Ok(self.pr_review_actions.read().await.get(action_id).cloned())
    }

    async fn get_pending_pr_review_action_for_head(
        &self,
        conversation_id: &ChatConversationId,
        pr_number: i64,
        head_sha: &str,
    ) -> AppResult<Option<AgentWorkspacePrReviewAction>> {
        Ok(self
            .pr_review_actions
            .read()
            .await
            .values()
            .find(|action| {
                action.conversation_id == *conversation_id
                    && action.pr_number == pr_number
                    && action.head_sha == head_sha
                    && action.status == AgentWorkspacePrReviewActionStatus::Pending
            })
            .cloned())
    }

    async fn list_pr_review_actions(
        &self,
        conversation_id: &ChatConversationId,
        limit: usize,
    ) -> AppResult<Vec<AgentWorkspacePrReviewAction>> {
        let mut actions = self
            .pr_review_actions
            .read()
            .await
            .values()
            .filter(|action| action.conversation_id == *conversation_id)
            .cloned()
            .collect::<Vec<_>>();
        actions.sort_by(|left, right| right.created_at.cmp(&left.created_at));
        actions.truncate(limit);
        Ok(actions)
    }

    async fn update_pr_review_action_status(
        &self,
        action_id: &str,
        status: AgentWorkspacePrReviewActionStatus,
        submitted_review_id: Option<&str>,
    ) -> AppResult<()> {
        if let Some(action) = self.pr_review_actions.write().await.get_mut(action_id) {
            action.status = status;
            action.submitted_review_id = submitted_review_id.map(str::to_string);
            action.updated_at = Utc::now();
            action.resolved_at = pr_review_action_terminal_status(status).then(Utc::now);
        }
        Ok(())
    }

    async fn claim_pending_pr_review_action(&self, action_id: &str) -> AppResult<bool> {
        let mut actions = self.pr_review_actions.write().await;
        let Some(action) = actions.get_mut(action_id) else {
            return Ok(false);
        };
        if action.status != AgentWorkspacePrReviewActionStatus::Pending {
            return Ok(false);
        }
        action.status = AgentWorkspacePrReviewActionStatus::Submitting;
        action.updated_at = Utc::now();
        Ok(true)
    }

    async fn delete(&self, conversation_id: &ChatConversationId) -> AppResult<()> {
        self.workspaces.write().await.remove(conversation_id);
        self.followup_provenance
            .write()
            .await
            .remove(conversation_id);
        self.publication_events
            .write()
            .await
            .remove(conversation_id);
        self.pr_descriptions.write().await.remove(conversation_id);
        let conversation_key = conversation_id.as_str().to_string();
        self.pr_comment_evidence
            .write()
            .await
            .retain(|(id, _, _), _| id != &conversation_key);
        self.pr_review_monitors
            .write()
            .await
            .remove(conversation_id);
        self.workspace_review_monitors
            .write()
            .await
            .remove(conversation_id);
        self.pr_review_actions
            .write()
            .await
            .retain(|_, action| action.conversation_id != *conversation_id);
        Ok(())
    }
}

fn workspace_review_approval_publish_status_is_active(status: Option<&str>) -> bool {
    matches!(
        status,
        Some("checking" | "committing" | "refreshing" | "describing" | "pushing")
    )
}

fn pr_review_action_terminal_status(status: AgentWorkspacePrReviewActionStatus) -> bool {
    matches!(
        status,
        AgentWorkspacePrReviewActionStatus::Skipped
            | AgentWorkspacePrReviewActionStatus::Submitted
            | AgentWorkspacePrReviewActionStatus::Failed
            | AgentWorkspacePrReviewActionStatus::Superseded
    )
}

fn is_active_direct_published_workspace(workspace: &AgentConversationWorkspace) -> bool {
    workspace.status == AgentConversationWorkspaceStatus::Active
        && workspace.mode == AgentConversationWorkspaceMode::Edit
        && workspace.linked_plan_branch_id.is_none()
        && workspace.publication_pr_number.is_some()
        && workspace.auto_publish_enabled
        && workspace.has_pr_status_pollable_push_status()
        && !workspace.has_terminal_publication_pr_status()
}

fn is_active_pr_poller_recovery_workspace(workspace: &AgentConversationWorkspace) -> bool {
    if is_active_direct_published_workspace(workspace) {
        return true;
    }

    if workspace.status == AgentConversationWorkspaceStatus::Active
        && workspace.mode == AgentConversationWorkspaceMode::ReviewPr
        && workspace.source_pull_request.is_some()
        && workspace.auto_publish_enabled
        && workspace.has_pr_status_pollable_push_status()
        && !workspace.has_terminal_publication_pr_status()
    {
        return true;
    }

    workspace.status == AgentConversationWorkspaceStatus::Active
        && workspace.mode == AgentConversationWorkspaceMode::Ideation
        && workspace.linked_plan_branch_id.is_some()
        && workspace.publication_pr_number.is_some()
        && workspace.auto_publish_enabled
        && workspace.has_pr_status_pollable_push_status()
        && (workspace.pr_autofix_enabled || workspace.pr_auto_merge_desired)
        && !workspace.has_terminal_publication_pr_status()
}

fn is_active_direct_external_pr_reconciliation_candidate(
    workspace: &AgentConversationWorkspace,
) -> bool {
    if workspace.mode != AgentConversationWorkspaceMode::Edit
        || workspace.linked_plan_branch_id.is_some()
        || (matches!(
            workspace.publication_pr_status.as_deref(),
            Some("closed") | Some("merged")
        ) && workspace.publication_pr_number.is_none())
    {
        return false;
    }

    if workspace.publication_pr_number.is_some() {
        return matches!(
            workspace.status,
            AgentConversationWorkspaceStatus::Active | AgentConversationWorkspaceStatus::Missing
        );
    }

    workspace.status == AgentConversationWorkspaceStatus::Active
        && !matches!(
            workspace.publication_push_status.as_deref(),
            Some("needs_agent" | "pending" | "failed" | "description_failed")
        )
}

fn is_active_direct_pr_supervision_recovery_candidate(
    workspace: &AgentConversationWorkspace,
) -> bool {
    workspace.status == AgentConversationWorkspaceStatus::Active
        && workspace.mode == AgentConversationWorkspaceMode::Edit
        && workspace.linked_plan_branch_id.is_none()
        && workspace.publication_pr_number.is_some()
        && workspace.publication_push_status.as_deref() == Some("failed")
        && workspace.pr_supervision_status.as_deref() == Some("blocked")
        && workspace.auto_publish_enabled
        && (workspace.pr_autofix_enabled || workspace.pr_auto_merge_desired)
        && !matches!(
            workspace.publication_pr_status.as_deref(),
            Some("closed") | Some("merged")
        )
}

fn is_active_linked_plan_pr_supervision_recovery_candidate(
    workspace: &AgentConversationWorkspace,
) -> bool {
    workspace.status == AgentConversationWorkspaceStatus::Active
        && workspace.mode == AgentConversationWorkspaceMode::Ideation
        && workspace.linked_plan_branch_id.is_some()
        && workspace.auto_publish_enabled
        && (workspace.pr_autofix_enabled || workspace.pr_auto_merge_desired)
        && matches!(
            workspace.pr_supervision_status.as_deref(),
            Some("blocked" | "fixing")
        )
}

fn is_active_needs_agent_workspace(workspace: &AgentConversationWorkspace) -> bool {
    workspace.status == AgentConversationWorkspaceStatus::Active
        && workspace.publication_push_status.as_deref() == Some("needs_agent")
        && !matches!(
            workspace.publication_pr_status.as_deref(),
            Some("closed") | Some("merged")
        )
}

fn is_stale_transient_publish_status_workspace(
    workspace: &AgentConversationWorkspace,
    cutoff: chrono::DateTime<Utc>,
) -> bool {
    workspace.status == AgentConversationWorkspaceStatus::Active
        && matches!(
            workspace.publication_push_status.as_deref(),
            Some("refreshing") | Some("checking") | Some("committing") | Some("describing")
        )
        && !matches!(
            workspace.publication_pr_status.as_deref(),
            Some("closed") | Some("merged")
        )
        && workspace.updated_at <= cutoff
}

#[cfg(test)]
mod tests {
    use crate::domain::entities::{
        AgentConversationWorkspace, AgentConversationWorkspaceMode,
        AgentConversationWorkspacePublicationEvent, AgentConversationWorkspaceStatus,
        AgentWorkspacePrCommentEvidenceUpsert, AgentWorkspacePrDescription,
        AgentWorkspacePrReviewAction, AgentWorkspacePrReviewActionKind,
        AgentWorkspacePrReviewActionStatus, AgentWorkspacePrReviewMonitor,
        AgentWorkspacePrReviewMonitorStatus, ChatConversationId, IdeationAnalysisBaseRefKind,
        IdeationSessionId, PlanBranchId, ProjectId,
    };
    use crate::domain::repositories::AgentConversationWorkspaceRepository;

    use super::MemoryAgentConversationWorkspaceRepository;

    #[tokio::test]
    async fn pr_description_round_trips_and_clears() {
        let repo = MemoryAgentConversationWorkspaceRepository::new();
        let conversation_id = ChatConversationId::from_string("conversation-1");

        repo.save_pr_description(
            &conversation_id,
            AgentWorkspacePrDescription::new(
                Some("Describe agent workspace publish".to_string()),
                "## Summary\n\n- Added publish descriptions".to_string(),
            ),
        )
        .await
        .unwrap();

        let saved = repo
            .get_pr_description(&conversation_id)
            .await
            .unwrap()
            .expect("description should be saved");
        assert_eq!(
            saved.title.as_deref(),
            Some("Describe agent workspace publish")
        );
        assert!(saved.body_markdown.contains("## Summary"));

        repo.clear_pr_description(&conversation_id).await.unwrap();
        assert!(repo
            .get_pr_description(&conversation_id)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn publication_events_are_listed_in_append_order() {
        let repo = MemoryAgentConversationWorkspaceRepository::new();
        let conversation_id = ChatConversationId::from_string("conversation-1");

        repo.append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id,
            "checking",
            "started",
            "Checking workspace",
            None,
        ))
        .await
        .unwrap();
        repo.append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id,
            "failed",
            "failed",
            "Pre-commit hook failed",
            Some("agent_fixable".to_string()),
        ))
        .await
        .unwrap();

        let events = repo
            .list_publication_events(&conversation_id)
            .await
            .unwrap();

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].step, "checking");
        assert_eq!(events[1].classification.as_deref(), Some("agent_fixable"));
    }

    #[tokio::test]
    async fn pr_comment_evidence_tracks_edits_inclusion_and_reads() {
        let repo = MemoryAgentConversationWorkspaceRepository::new();
        let conversation_id = ChatConversationId::from_string("conversation-1");

        repo.upsert_pr_comment_evidence(
            &conversation_id,
            vec![AgentWorkspacePrCommentEvidenceUpsert::new(
                267,
                "comment-1".to_string(),
                Some("codecov".to_string()),
                "Patch coverage is below target.".to_string(),
                Some("https://github.com/owner/repo/pull/267#issuecomment-1".to_string()),
                Some("2026-05-18T22:00:00Z".to_string()),
                Some("2026-05-18T22:00:00Z".to_string()),
                true,
                true,
            )],
        )
        .await
        .unwrap();

        let first = repo
            .list_pr_comment_evidence(&conversation_id, 267, 10)
            .await
            .unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].edit_count, 0);

        repo.mark_pr_comments_included(&conversation_id, 267, &["comment-1".to_string()])
            .await
            .unwrap();
        repo.mark_pr_comment_read(&conversation_id, 267, "comment-1")
            .await
            .unwrap();
        repo.upsert_pr_comment_evidence(
            &conversation_id,
            vec![AgentWorkspacePrCommentEvidenceUpsert::new(
                267,
                "comment-1".to_string(),
                Some("codecov".to_string()),
                "Patch coverage recovered after rerun.".to_string(),
                Some("https://github.com/owner/repo/pull/267#issuecomment-1".to_string()),
                Some("2026-05-18T22:00:00Z".to_string()),
                Some("2026-05-18T22:05:00Z".to_string()),
                true,
                true,
            )],
        )
        .await
        .unwrap();

        let updated = repo
            .get_pr_comment_evidence(&conversation_id, 267, "comment-1")
            .await
            .unwrap()
            .expect("comment should exist");
        assert_eq!(updated.edit_count, 1);
        assert_eq!(updated.body, "Patch coverage recovered after rerun.");
        assert!(updated.last_included_at.is_some());
        assert!(updated.last_read_at.is_some());
    }

    #[tokio::test]
    async fn linked_ideation_session_lookup_returns_latest_workspace_and_none_for_missing() {
        let repo = MemoryAgentConversationWorkspaceRepository::new();
        let session_id = IdeationSessionId::from_string("ideation-session-1");
        let mut first = candidate_workspace("linked-first");
        first.linked_ideation_session_id = Some(session_id.clone());
        repo.create_or_update(first.clone()).await.unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(1)).await;

        let mut second = candidate_workspace("linked-second");
        second.linked_ideation_session_id = Some(session_id.clone());
        second.task_pipeline_session_id = Some(session_id.clone());
        repo.create_or_update(second.clone()).await.unwrap();

        let loaded = repo
            .get_by_linked_ideation_session_id(&session_id)
            .await
            .unwrap()
            .expect("latest linked workspace should load");
        assert_eq!(loaded.conversation_id, second.conversation_id);

        let task_pipeline = repo
            .get_by_task_pipeline_session_id(&session_id)
            .await
            .unwrap()
            .expect("durably attached Tasks workspace should load");
        assert_eq!(task_pipeline.conversation_id, second.conversation_id);

        let missing = repo
            .get_by_linked_ideation_session_id(&IdeationSessionId::from_string("missing-session"))
            .await
            .unwrap();
        assert!(missing.is_none());
        assert!(repo
            .get_by_task_pipeline_session_id(&IdeationSessionId::from_string("missing-session"))
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn delete_removes_publication_events_for_conversation() {
        let repo = MemoryAgentConversationWorkspaceRepository::new();
        let conversation_id = ChatConversationId::from_string("conversation-1");
        repo.append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id,
            "checking",
            "started",
            "Checking workspace",
            None,
        ))
        .await
        .unwrap();
        repo.upsert_pr_comment_evidence(
            &conversation_id,
            vec![AgentWorkspacePrCommentEvidenceUpsert::new(
                267,
                "comment-1".to_string(),
                Some("codecov".to_string()),
                "Patch coverage is below target.".to_string(),
                Some("https://github.com/owner/repo/pull/267#issuecomment-1".to_string()),
                Some("2026-05-18T22:00:00Z".to_string()),
                Some("2026-05-18T22:00:00Z".to_string()),
                true,
                true,
            )],
        )
        .await
        .unwrap();

        repo.delete(&conversation_id).await.unwrap();

        let events = repo
            .list_publication_events(&conversation_id)
            .await
            .unwrap();
        assert!(events.is_empty());
        let comments = repo
            .list_pr_comment_evidence(&conversation_id, 267, 10)
            .await
            .unwrap();
        assert!(comments.is_empty());
    }

    #[tokio::test]
    async fn pr_review_monitor_and_actions_round_trip_and_clear_on_delete() {
        let repo = MemoryAgentConversationWorkspaceRepository::new();
        let workspace = candidate_workspace("review");
        let conversation_id = workspace.conversation_id.clone();
        repo.create_or_update(workspace).await.unwrap();

        let mut monitor = AgentWorkspacePrReviewMonitor::new(
            conversation_id.clone(),
            ProjectId::from_string("project-1".to_string()),
            411,
            Some("head-sha-1".to_string()),
        );
        monitor.status = AgentWorkspacePrReviewMonitorStatus::Watching;
        monitor.monitor_enabled = true;
        monitor.first_review_completed = true;
        monitor.last_reviewed_head_sha = Some("head-sha-1".to_string());
        monitor.last_review_outcome = Some("request_changes".to_string());
        let saved_monitor = repo.upsert_pr_review_monitor(monitor).await.unwrap();
        assert_eq!(
            saved_monitor.status,
            AgentWorkspacePrReviewMonitorStatus::Watching
        );

        let loaded_monitor = repo
            .get_pr_review_monitor(&conversation_id)
            .await
            .unwrap()
            .expect("monitor should exist");
        assert!(loaded_monitor.monitor_enabled);
        assert_eq!(
            loaded_monitor.last_reviewed_head_sha.as_deref(),
            Some("head-sha-1")
        );
        assert_eq!(
            repo.list_active_pr_review_monitors().await.unwrap().len(),
            1
        );

        let action = AgentWorkspacePrReviewAction::new(
            conversation_id.clone(),
            411,
            "head-sha-1".to_string(),
            AgentWorkspacePrReviewActionKind::RequestChanges,
            "Found blocking issues".to_string(),
            "Please address the blocking issues.".to_string(),
            Some(r#"[{"path":"src/lib.rs"}]"#.to_string()),
            Some("run-1".to_string()),
        );
        let saved_action = repo
            .create_or_update_pr_review_action(action)
            .await
            .unwrap();

        let replacement = AgentWorkspacePrReviewAction::new(
            conversation_id.clone(),
            411,
            "head-sha-1".to_string(),
            AgentWorkspacePrReviewActionKind::Approve,
            "Looks good now".to_string(),
            "The follow-up commit fixed the review findings.".to_string(),
            None,
            Some("run-2".to_string()),
        );
        let updated_action = repo
            .create_or_update_pr_review_action(replacement)
            .await
            .unwrap();
        assert_eq!(updated_action.id, saved_action.id);
        assert_eq!(
            updated_action.proposed_action,
            AgentWorkspacePrReviewActionKind::Approve
        );
        assert_eq!(updated_action.created_by_run_id.as_deref(), Some("run-2"));

        let pending = repo
            .get_pending_pr_review_action_for_head(&conversation_id, 411, "head-sha-1")
            .await
            .unwrap()
            .expect("pending action should exist");
        assert_eq!(pending.id, saved_action.id);
        assert_eq!(
            repo.list_pr_review_actions(&conversation_id, 10)
                .await
                .unwrap()
                .len(),
            1
        );

        repo.update_pr_review_action_status(
            &saved_action.id,
            AgentWorkspacePrReviewActionStatus::Submitted,
            Some("review-1"),
        )
        .await
        .unwrap();
        let submitted = repo
            .get_pr_review_action(&saved_action.id)
            .await
            .unwrap()
            .expect("submitted action should remain queryable");
        assert_eq!(
            submitted.status,
            AgentWorkspacePrReviewActionStatus::Submitted
        );
        assert_eq!(submitted.submitted_review_id.as_deref(), Some("review-1"));
        assert!(submitted.resolved_at.is_some());
        assert!(repo
            .get_pending_pr_review_action_for_head(&conversation_id, 411, "head-sha-1")
            .await
            .unwrap()
            .is_none());

        let mut terminal_monitor = loaded_monitor;
        terminal_monitor.status = AgentWorkspacePrReviewMonitorStatus::Terminal;
        repo.upsert_pr_review_monitor(terminal_monitor)
            .await
            .unwrap();
        assert!(repo
            .list_active_pr_review_monitors()
            .await
            .unwrap()
            .is_empty());

        repo.delete(&conversation_id).await.unwrap();
        assert!(repo
            .get_pr_review_monitor(&conversation_id)
            .await
            .unwrap()
            .is_none());
        assert!(repo
            .list_pr_review_actions(&conversation_id, 10)
            .await
            .unwrap()
            .is_empty());
    }

    fn candidate_workspace(id: &str) -> AgentConversationWorkspace {
        AgentConversationWorkspace::new(
            ChatConversationId::new(),
            ProjectId::from_string("project-1".to_string()),
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("Project default (main)".to_string()),
            Some("base-sha".to_string()),
            format!("ralphx/demo/agent-{id}"),
            format!("/tmp/ralphx-demo-{id}"),
        )
    }

    #[tokio::test]
    async fn active_direct_published_workspaces_include_refreshed_prs_for_status_polling() {
        let repo = MemoryAgentConversationWorkspaceRepository::new();
        let mut pushed = candidate_workspace("pushed");
        pushed.publication_pr_number = Some(12);
        pushed.publication_pr_status = Some("open".to_string());
        pushed.publication_push_status = Some("pushed".to_string());
        let mut refreshed = candidate_workspace("refreshed");
        refreshed.publication_pr_number = Some(13);
        refreshed.publication_pr_status = Some("open".to_string());
        refreshed.publication_push_status = Some("refreshed".to_string());

        repo.create_or_update(pushed.clone()).await.unwrap();
        repo.create_or_update(refreshed.clone()).await.unwrap();

        let workspaces = repo
            .list_active_direct_published_workspaces()
            .await
            .unwrap();

        assert_eq!(workspaces.len(), 2);
        assert!(workspaces
            .iter()
            .any(|workspace| workspace.conversation_id == pushed.conversation_id));
        assert!(workspaces
            .iter()
            .any(|workspace| workspace.conversation_id == refreshed.conversation_id));
    }

    #[tokio::test]
    async fn transient_publish_status_workspaces_filter_stale_active_open_rows() {
        let repo = MemoryAgentConversationWorkspaceRepository::new();
        let stale = chrono::Utc::now() - chrono::Duration::minutes(10);

        let mut refreshing = candidate_workspace("refreshing");
        refreshing.publication_pr_number = Some(21);
        refreshing.publication_pr_status = Some("open".to_string());
        refreshing.publication_push_status = Some("refreshing".to_string());
        refreshing.updated_at = stale;

        let mut closed = candidate_workspace("closed");
        closed.publication_pr_number = Some(23);
        closed.publication_pr_status = Some("closed".to_string());
        closed.publication_push_status = Some("committing".to_string());
        closed.updated_at = stale;

        let mut archived = candidate_workspace("archived-transient");
        archived.status = AgentConversationWorkspaceStatus::Archived;
        archived.publication_pr_number = Some(24);
        archived.publication_pr_status = Some("open".to_string());
        archived.publication_push_status = Some("describing".to_string());
        archived.updated_at = stale;

        for workspace in [refreshing.clone(), closed, archived] {
            repo.create_or_update(workspace).await.unwrap();
        }

        let workspaces = repo
            .list_active_transient_publish_status_workspaces(0)
            .await
            .unwrap();

        assert_eq!(workspaces.len(), 1);
        assert_eq!(workspaces[0].conversation_id, refreshing.conversation_id);
    }

    #[tokio::test]
    async fn pr_poller_recovery_workspaces_include_supervised_ideation_prs() {
        let repo = MemoryAgentConversationWorkspaceRepository::new();
        let mut direct = candidate_workspace("direct");
        direct.publication_pr_number = Some(12);
        direct.publication_pr_status = Some("open".to_string());
        direct.publication_push_status = Some("pushed".to_string());

        let mut ideation = candidate_workspace("ideation");
        ideation.mode = AgentConversationWorkspaceMode::Ideation;
        ideation.linked_plan_branch_id = Some(PlanBranchId::from_string("plan-1"));
        ideation.publication_pr_number = Some(13);
        ideation.publication_pr_status = Some("open".to_string());
        ideation.publication_push_status = Some("pushed".to_string());
        ideation.pr_autofix_enabled = true;

        let mut unsupervised_ideation = candidate_workspace("unsupervised-ideation");
        unsupervised_ideation.mode = AgentConversationWorkspaceMode::Ideation;
        unsupervised_ideation.linked_plan_branch_id = Some(PlanBranchId::from_string("plan-2"));
        unsupervised_ideation.publication_pr_number = Some(14);
        unsupervised_ideation.publication_pr_status = Some("open".to_string());
        unsupervised_ideation.publication_push_status = Some("pushed".to_string());

        for workspace in [
            direct.clone(),
            ideation.clone(),
            unsupervised_ideation.clone(),
        ] {
            repo.create_or_update(workspace).await.unwrap();
        }

        let workspaces = repo
            .list_active_pr_poller_recovery_workspaces()
            .await
            .unwrap();

        assert_eq!(workspaces.len(), 2);
        assert!(workspaces
            .iter()
            .any(|workspace| workspace.conversation_id == direct.conversation_id));
        assert!(workspaces
            .iter()
            .any(|workspace| workspace.conversation_id == ideation.conversation_id));
    }

    #[tokio::test]
    async fn external_pr_reconciliation_candidates_filter_and_limit_recent_direct_workspaces() {
        let repo = MemoryAgentConversationWorkspaceRepository::new();

        let first = candidate_workspace("candidate-1");
        let second = candidate_workspace("candidate-2");
        let mut linked_failed = candidate_workspace("linked-failed");
        linked_failed.publication_pr_number = Some(12);
        linked_failed.publication_pr_status = Some("open".to_string());
        linked_failed.publication_push_status = Some("failed".to_string());
        let mut linked_missing = candidate_workspace("linked-missing");
        linked_missing.status = AgentConversationWorkspaceStatus::Missing;
        linked_missing.publication_pr_number = Some(13);
        linked_missing.publication_pr_status = Some("open".to_string());
        linked_missing.publication_push_status = Some("needs_agent".to_string());
        let mut terminal_linked = candidate_workspace("terminal-linked");
        terminal_linked.publication_pr_number = Some(14);
        terminal_linked.publication_pr_status = Some("merged".to_string());
        terminal_linked.publication_push_status = Some("pushed".to_string());
        let mut linked_plan = candidate_workspace("linked-plan");
        linked_plan.linked_plan_branch_id = Some(PlanBranchId::from_string("plan-1"));
        let mut blocked_push = candidate_workspace("blocked-push");
        blocked_push.publication_push_status = Some("needs_agent".to_string());
        let mut terminal = candidate_workspace("terminal");
        terminal.publication_pr_status = Some("merged".to_string());
        let mut chat = candidate_workspace("chat");
        chat.mode = AgentConversationWorkspaceMode::Chat;
        let mut archived = candidate_workspace("archived");
        archived.status = AgentConversationWorkspaceStatus::Archived;

        for workspace in [
            first.clone(),
            second.clone(),
            linked_failed.clone(),
            linked_missing.clone(),
            terminal_linked.clone(),
            linked_plan,
            blocked_push,
            terminal,
            chat,
            archived,
        ] {
            repo.create_or_update(workspace).await.unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }

        let limited = repo
            .list_active_direct_external_pr_reconciliation_candidates(1)
            .await
            .unwrap();
        assert_eq!(limited.len(), 1);
        assert_eq!(limited[0].conversation_id, terminal_linked.conversation_id);

        let all = repo
            .list_active_direct_external_pr_reconciliation_candidates(10)
            .await
            .unwrap();
        assert_eq!(
            all.into_iter()
                .map(|workspace| workspace.conversation_id)
                .collect::<Vec<_>>(),
            vec![
                terminal_linked.conversation_id,
                linked_missing.conversation_id,
                linked_failed.conversation_id,
                second.conversation_id,
                first.conversation_id
            ]
        );
    }

    #[tokio::test]
    async fn pr_supervision_recovery_candidates_filter_blocked_failed_supervised_prs() {
        let repo = MemoryAgentConversationWorkspaceRepository::new();

        let mut first = candidate_workspace("candidate-1");
        first.publication_pr_number = Some(41);
        first.publication_pr_status = Some("open".to_string());
        first.publication_push_status = Some("failed".to_string());
        first.pr_supervision_status = Some("blocked".to_string());
        first.pr_autofix_enabled = true;
        let mut second = candidate_workspace("candidate-2");
        second.publication_pr_number = Some(42);
        second.publication_pr_status = Some("open".to_string());
        second.publication_push_status = Some("failed".to_string());
        second.pr_supervision_status = Some("blocked".to_string());
        second.pr_auto_merge_desired = true;
        let mut disabled = candidate_workspace("disabled");
        disabled.publication_pr_number = Some(43);
        disabled.publication_push_status = Some("failed".to_string());
        disabled.pr_supervision_status = Some("blocked".to_string());
        let mut needs_agent = candidate_workspace("needs-agent");
        needs_agent.publication_pr_number = Some(44);
        needs_agent.publication_push_status = Some("needs_agent".to_string());
        needs_agent.pr_supervision_status = Some("blocked".to_string());
        needs_agent.pr_autofix_enabled = true;
        let mut terminal = candidate_workspace("terminal");
        terminal.publication_pr_number = Some(45);
        terminal.publication_pr_status = Some("merged".to_string());
        terminal.publication_push_status = Some("failed".to_string());
        terminal.pr_supervision_status = Some("blocked".to_string());
        terminal.pr_autofix_enabled = true;

        for workspace in [
            first.clone(),
            second.clone(),
            disabled,
            needs_agent,
            terminal,
        ] {
            repo.create_or_update(workspace).await.unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }

        let limited = repo
            .list_active_direct_pr_supervision_recovery_candidates(1)
            .await
            .unwrap();
        assert_eq!(limited.len(), 1);
        assert_eq!(limited[0].conversation_id, second.conversation_id);

        let all = repo
            .list_active_direct_pr_supervision_recovery_candidates(10)
            .await
            .unwrap();
        assert_eq!(
            all.into_iter()
                .map(|workspace| workspace.conversation_id)
                .collect::<Vec<_>>(),
            vec![second.conversation_id, first.conversation_id]
        );
    }

    #[tokio::test]
    async fn linked_plan_pr_supervision_recovery_candidates_filter_ideation_rows() {
        let repo = MemoryAgentConversationWorkspaceRepository::new();

        let mut blocked = candidate_workspace("linked-blocked");
        blocked.mode = AgentConversationWorkspaceMode::Ideation;
        blocked.linked_plan_branch_id = Some(PlanBranchId::from_string("plan-linked-1"));
        blocked.pr_supervision_status = Some("blocked".to_string());
        blocked.pr_autofix_enabled = true;

        let mut fixing = candidate_workspace("linked-fixing");
        fixing.mode = AgentConversationWorkspaceMode::Ideation;
        fixing.linked_plan_branch_id = Some(PlanBranchId::from_string("plan-linked-2"));
        fixing.pr_supervision_status = Some("fixing".to_string());
        fixing.pr_auto_merge_desired = true;

        let mut direct = candidate_workspace("direct");
        direct.linked_plan_branch_id = Some(PlanBranchId::from_string("plan-direct"));
        direct.pr_supervision_status = Some("blocked".to_string());
        direct.pr_autofix_enabled = true;

        let mut unlinked = candidate_workspace("unlinked");
        unlinked.mode = AgentConversationWorkspaceMode::Ideation;
        unlinked.pr_supervision_status = Some("blocked".to_string());
        unlinked.pr_autofix_enabled = true;

        let mut disabled = candidate_workspace("disabled");
        disabled.mode = AgentConversationWorkspaceMode::Ideation;
        disabled.linked_plan_branch_id = Some(PlanBranchId::from_string("plan-disabled"));
        disabled.pr_supervision_status = Some("blocked".to_string());

        let mut monitoring = candidate_workspace("monitoring");
        monitoring.mode = AgentConversationWorkspaceMode::Ideation;
        monitoring.linked_plan_branch_id = Some(PlanBranchId::from_string("plan-monitoring"));
        monitoring.pr_supervision_status = Some("monitoring".to_string());
        monitoring.pr_autofix_enabled = true;

        let mut paused = candidate_workspace("paused");
        paused.mode = AgentConversationWorkspaceMode::Ideation;
        paused.linked_plan_branch_id = Some(PlanBranchId::from_string("plan-paused"));
        paused.pr_supervision_status = Some("blocked".to_string());
        paused.pr_autofix_enabled = true;
        paused.auto_publish_enabled = false;

        for workspace in [
            blocked.clone(),
            fixing.clone(),
            direct,
            unlinked,
            disabled,
            monitoring,
            paused,
        ] {
            repo.create_or_update(workspace).await.unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }

        let limited = repo
            .list_active_linked_plan_pr_supervision_recovery_candidates(1)
            .await
            .unwrap();
        assert_eq!(limited.len(), 1);
        assert_eq!(limited[0].conversation_id, fixing.conversation_id);

        let all = repo
            .list_active_linked_plan_pr_supervision_recovery_candidates(10)
            .await
            .unwrap();
        assert_eq!(
            all.into_iter()
                .map(|workspace| workspace.conversation_id)
                .collect::<Vec<_>>(),
            vec![fixing.conversation_id, blocked.conversation_id]
        );
    }
}
