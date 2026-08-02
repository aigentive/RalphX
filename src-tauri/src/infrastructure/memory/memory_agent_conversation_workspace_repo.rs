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
    AgentWorkspacePrMetadataDecision, AgentWorkspacePrReviewAction,
    AgentWorkspacePrReviewActionStatus, AgentWorkspacePrReviewMonitor,
    AgentWorkspacePrReviewMonitorStatus, AgentWorkspacePublicationMetadataPhase,
    AgentWorkspacePublicationMetadataReceipt, AgentWorkspacePublicationMetadataState,
    AgentWorkspaceRepairAttempt, AgentWorkspaceRepairAttemptId, AgentWorkspaceRepairEffect,
    AgentWorkspaceRepairEffectId, AgentWorkspaceReviewApprovalSnapshot,
    AgentWorkspaceReviewAutoMergeGuard, AgentWorkspaceReviewFixerSnapshot,
    AgentWorkspaceReviewGateStatus, AgentWorkspaceReviewHunkAnnotation,
    AgentWorkspaceReviewMonitor, AgentWorkspaceReviewMonitorStatus, AgentWorkspaceReviewOutcome,
    AgentWorkspaceReviewTargetScope, ArtifactId, ChatConversationId, IdeationSessionId,
    PlanBranchId, ProjectId, DEFAULT_AGENT_WORKSPACE_PR_AUTO_MERGE_METHOD,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, AgentWorkspaceLocalCleanupClaim,
    AgentWorkspacePrReviewActionMutation, AgentWorkspacePrReviewStateTransition,
    AgentWorkspacePrTerminalSettlement, AgentWorkspacePublicationGuard,
    AgentWorkspacePublicationMetadataReceiptClaim, AgentWorkspacePublicationMetadataReceiptRefresh,
    AgentWorkspacePublicationUpdate, ImportLegacyAgentWorkspaceRepairAttemptOutcome,
};
use crate::error::{AppError, AppResult};

const WORKSPACE_REVIEW_FIXER_STATUS_CYCLE_CAPPED: &str = "cycle_capped";

#[cfg(test)]
#[path = "memory_agent_conversation_workspace_repo_tests.rs"]
mod memory_agent_conversation_workspace_repo_tests;

mod repair_attempts;

#[cfg(test)]
#[path = "memory_agent_conversation_workspace_repo/repair_attempts_tests.rs"]
mod repair_attempts_tests;

#[cfg(test)]
#[path = "memory_agent_conversation_workspace_repo/repair_attempt_fencing_tests.rs"]
mod repair_attempt_fencing_tests;

#[cfg(test)]
#[path = "memory_agent_conversation_workspace_repo/repair_attempt_effect_fencing_tests.rs"]
mod repair_attempt_effect_fencing_tests;

#[cfg(test)]
#[derive(Debug, Clone, Copy)]
pub enum ForcedCreateAgentWorkspaceRepairEffectOutcome {
    Stale,
    Missing,
}

fn is_lowercase_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn validate_publication_metadata_receipt(
    receipt: &AgentWorkspacePublicationMetadataReceipt,
) -> AppResult<()> {
    if receipt.attempt_id.trim().is_empty() {
        return Err(AppError::Validation(
            "publication metadata receipt attempt id must not be empty".to_string(),
        ));
    }
    if receipt.target_pr_number <= 0 {
        return Err(AppError::Validation(
            "publication metadata receipt target PR number must be positive".to_string(),
        ));
    }
    for (label, value) in [
        ("before authority", receipt.before_authority_sha256.as_str()),
        ("before title", receipt.before_title_sha256.as_str()),
        (
            "before editable body",
            receipt.before_editable_body_sha256.as_str(),
        ),
    ] {
        if !is_lowercase_sha256(value) {
            return Err(AppError::Validation(format!(
                "publication metadata receipt {label} fingerprint must be lowercase SHA-256"
            )));
        }
    }
    for (label, value) in [
        (
            "before managed suffix",
            receipt.before_managed_suffix_sha256.as_deref(),
        ),
        ("intended title", receipt.intended_title_sha256.as_deref()),
        (
            "intended editable body",
            receipt.intended_editable_body_sha256.as_deref(),
        ),
    ] {
        if value.is_some_and(|value| !is_lowercase_sha256(value)) {
            return Err(AppError::Validation(format!(
                "publication metadata receipt {label} fingerprint must be lowercase SHA-256"
            )));
        }
    }
    Ok(())
}

fn validate_publication_metadata_refresh(
    refresh: &AgentWorkspacePublicationMetadataReceiptRefresh,
) -> AppResult<()> {
    validate_publication_metadata_receipt(&AgentWorkspacePublicationMetadataReceipt {
        attempt_id: "refresh".to_string(),
        phase: AgentWorkspacePublicationMetadataPhase::Prepared,
        state: AgentWorkspacePublicationMetadataState::NotAttempted,
        target_pr_number: refresh.target_pr_number,
        before_authority_sha256: refresh.before_authority_sha256.clone(),
        before_title_sha256: refresh.before_title_sha256.clone(),
        before_editable_body_sha256: refresh.before_editable_body_sha256.clone(),
        before_managed_suffix_sha256: refresh.before_managed_suffix_sha256.clone(),
        intended_title_sha256: refresh.intended_title_sha256.clone(),
        intended_editable_body_sha256: refresh.intended_editable_body_sha256.clone(),
        updated_at: refresh.updated_at,
    })
}

fn validate_publication_metadata_decision(
    receipt: &AgentWorkspacePublicationMetadataReceipt,
    decision: &AgentWorkspacePrMetadataDecision,
) -> AppResult<()> {
    let valid = match decision {
        AgentWorkspacePrMetadataDecision::Preserve => {
            receipt.intended_title_sha256.is_none()
                && receipt.intended_editable_body_sha256.is_none()
        }
        AgentWorkspacePrMetadataDecision::Patch {
            title,
            body_markdown,
        } => {
            (title.is_some() || body_markdown.is_some())
                && title.is_some() == receipt.intended_title_sha256.is_some()
                && body_markdown.is_some() == receipt.intended_editable_body_sha256.is_some()
        }
    };
    if valid {
        Ok(())
    } else {
        Err(AppError::Validation(
            "publication metadata decision does not match intended receipt fields".to_string(),
        ))
    }
}

pub struct MemoryAgentConversationWorkspaceRepository {
    workspaces: RwLock<HashMap<ChatConversationId, AgentConversationWorkspace>>,
    repair_attempts: RwLock<HashMap<AgentWorkspaceRepairAttemptId, AgentWorkspaceRepairAttempt>>,
    repair_effects: RwLock<HashMap<AgentWorkspaceRepairEffectId, AgentWorkspaceRepairEffect>>,
    followup_provenance: RwLock<HashMap<ChatConversationId, AgentWorkspaceFollowupProvenance>>,
    pr_descriptions: RwLock<HashMap<ChatConversationId, AgentWorkspacePrDescription>>,
    pr_metadata_decisions: RwLock<HashMap<ChatConversationId, AgentWorkspacePrMetadataDecision>>,
    publication_metadata_receipts:
        RwLock<HashMap<ChatConversationId, AgentWorkspacePublicationMetadataReceipt>>,
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
    matching_publication_event_error: Mutex<Option<(String, String, String)>>,
    #[cfg(test)]
    next_publication_update_error: Mutex<Option<String>>,
    #[cfg(test)]
    next_worktree_path_list_error: Mutex<Option<String>>,
    #[cfg(test)]
    next_auto_merge_restore_completion_error: Mutex<Option<String>>,
    #[cfg(test)]
    next_create_repair_effect_outcome: Mutex<Option<ForcedCreateAgentWorkspaceRepairEffectOutcome>>,
}

impl MemoryAgentConversationWorkspaceRepository {
    pub fn new() -> Self {
        Self {
            workspaces: RwLock::new(HashMap::new()),
            repair_attempts: RwLock::new(HashMap::new()),
            repair_effects: RwLock::new(HashMap::new()),
            followup_provenance: RwLock::new(HashMap::new()),
            pr_descriptions: RwLock::new(HashMap::new()),
            pr_metadata_decisions: RwLock::new(HashMap::new()),
            publication_metadata_receipts: RwLock::new(HashMap::new()),
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
            matching_publication_event_error: Mutex::new(None),
            #[cfg(test)]
            next_publication_update_error: Mutex::new(None),
            #[cfg(test)]
            next_worktree_path_list_error: Mutex::new(None),
            #[cfg(test)]
            next_auto_merge_restore_completion_error: Mutex::new(None),
            #[cfg(test)]
            next_create_repair_effect_outcome: Mutex::new(None),
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
    pub fn fail_next_matching_publication_event(
        &self,
        step: impl Into<String>,
        status: impl Into<String>,
        message: impl Into<String>,
    ) {
        *self.matching_publication_event_error.lock().unwrap() =
            Some((step.into(), status.into(), message.into()));
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

    #[cfg(test)]
    pub fn force_next_create_repair_effect_outcome(
        &self,
        outcome: ForcedCreateAgentWorkspaceRepairEffectOutcome,
    ) {
        *self.next_create_repair_effect_outcome.lock().unwrap() = Some(outcome);
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
            // Receipt authority is changed only by its attempt-scoped CAS methods.
            // Normal workspace upserts must not erase an in-flight receipt.
            if workspace.publication_metadata_attempt_id.is_none() {
                workspace.publication_metadata_phase = existing.publication_metadata_phase;
                workspace.publication_metadata_state = existing.publication_metadata_state;
                workspace.publication_metadata_attempt_id =
                    existing.publication_metadata_attempt_id.clone();
            }
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

    async fn list_active_pending_publication_metadata_receipt_workspaces(
        &self,
        stale_older_than_secs: u64,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        let cutoff = Utc::now() - chrono::Duration::seconds(stale_older_than_secs as i64);
        let workspaces = self.workspaces.read().await;
        let receipts = self.publication_metadata_receipts.read().await;
        Ok(workspaces
            .values()
            .filter(|workspace| {
                is_active_pending_publication_metadata_receipt_workspace(workspace)
                    && receipts
                        .get(&workspace.conversation_id)
                        .is_some_and(|receipt| receipt.updated_at <= cutoff)
            })
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

    async fn claim_publication_metadata_receipt(
        &self,
        conversation_id: &ChatConversationId,
        claim: AgentWorkspacePublicationMetadataReceiptClaim,
    ) -> AppResult<bool> {
        validate_publication_metadata_receipt(&claim.receipt)?;
        validate_publication_metadata_decision(&claim.receipt, &claim.decision)?;
        if claim.event.conversation_id != *conversation_id
            || claim.event.attempt_id.as_deref() != Some(claim.receipt.attempt_id.as_str())
        {
            return Err(AppError::Validation(
                "publication metadata receipt claim event must belong to the claimed attempt"
                    .to_string(),
            ));
        }
        if claim.receipt.phase != AgentWorkspacePublicationMetadataPhase::Prepared
            || claim.receipt.state != AgentWorkspacePublicationMetadataState::NotAttempted
        {
            return Err(AppError::Validation(
                "publication metadata receipt claim must start prepared and not_attempted"
                    .to_string(),
            ));
        }
        #[cfg(test)]
        if let Some(message) = self.next_publication_event_error.lock().unwrap().take() {
            return Err(AppError::Infrastructure(message));
        }

        let mut workspaces = self.workspaces.write().await;
        let mut receipts = self.publication_metadata_receipts.write().await;
        let mut decisions = self.pr_metadata_decisions.write().await;
        let mut publication_events = self.publication_events.write().await;
        let Some(workspace) = workspaces.get_mut(conversation_id) else {
            return Ok(false);
        };
        if workspace.publication_pr_number != Some(claim.receipt.target_pr_number) {
            return Err(AppError::Validation(
                "publication metadata receipt target does not match the workspace PR".to_string(),
            ));
        }
        match (
            workspace.publication_metadata_attempt_id.as_deref(),
            workspace.publication_metadata_phase,
            workspace.publication_metadata_state,
        ) {
            (None, None, None) => {
                if receipts.contains_key(conversation_id) {
                    return Err(AppError::Validation(
                        "publication metadata receipt authority is inconsistent".to_string(),
                    ));
                }
            }
            (
                Some(attempt_id),
                Some(AgentWorkspacePublicationMetadataPhase::Settled),
                Some(state),
            ) if state != AgentWorkspacePublicationMetadataState::Unknown => {
                let receipt = receipts.get(conversation_id).ok_or_else(|| {
                    AppError::Validation(
                        "publication metadata receipt authority is incomplete".to_string(),
                    )
                })?;
                if receipt.attempt_id != attempt_id
                    || receipt.phase != AgentWorkspacePublicationMetadataPhase::Settled
                    || receipt.state != state
                {
                    return Err(AppError::Validation(
                        "publication metadata receipt authority is inconsistent".to_string(),
                    ));
                }
                validate_publication_metadata_receipt(receipt)?;
            }
            (Some(_), Some(phase), Some(_))
                if phase != AgentWorkspacePublicationMetadataPhase::Settled =>
            {
                return Ok(false);
            }
            _ => {
                return Err(AppError::Validation(
                    "publication metadata receipt authority is incomplete".to_string(),
                ));
            }
        }

        workspace.publication_metadata_phase = Some(claim.receipt.phase);
        workspace.publication_metadata_state = Some(claim.receipt.state);
        workspace.publication_metadata_attempt_id = Some(claim.receipt.attempt_id.clone());
        workspace.publication_push_status = Some("pushing".to_string());
        workspace.updated_at = claim.receipt.updated_at;
        receipts.insert(conversation_id.clone(), claim.receipt);
        decisions.insert(conversation_id.clone(), claim.decision);
        publication_events
            .entry(conversation_id.clone())
            .or_default()
            .push(claim.event);
        Ok(true)
    }

    async fn compare_and_set_publication_metadata_receipt_with_events(
        &self,
        conversation_id: &ChatConversationId,
        expected_attempt_id: &str,
        expected_phase: AgentWorkspacePublicationMetadataPhase,
        expected_state: AgentWorkspacePublicationMetadataState,
        next_phase: AgentWorkspacePublicationMetadataPhase,
        next_state: AgentWorkspacePublicationMetadataState,
        refresh: Option<AgentWorkspacePublicationMetadataReceiptRefresh>,
        events: Vec<AgentConversationWorkspacePublicationEvent>,
    ) -> AppResult<bool> {
        if let Some(refresh) = refresh.as_ref() {
            validate_publication_metadata_refresh(refresh)?;
            validate_publication_metadata_decision(
                &AgentWorkspacePublicationMetadataReceipt {
                    attempt_id: expected_attempt_id.to_string(),
                    phase: next_phase,
                    state: next_state,
                    target_pr_number: refresh.target_pr_number,
                    before_authority_sha256: refresh.before_authority_sha256.clone(),
                    before_title_sha256: refresh.before_title_sha256.clone(),
                    before_editable_body_sha256: refresh.before_editable_body_sha256.clone(),
                    before_managed_suffix_sha256: refresh.before_managed_suffix_sha256.clone(),
                    intended_title_sha256: refresh.intended_title_sha256.clone(),
                    intended_editable_body_sha256: refresh.intended_editable_body_sha256.clone(),
                    updated_at: refresh.updated_at,
                },
                &refresh.decision,
            )?;
        }
        if events.iter().any(|event| {
            event.conversation_id != *conversation_id
                || event.attempt_id.as_deref() != Some(expected_attempt_id)
        }) {
            return Err(AppError::Validation(
                "publication metadata receipt events must belong to the guarded attempt"
                    .to_string(),
            ));
        }
        #[cfg(test)]
        if let Some(message) = self.next_publication_event_error.lock().unwrap().take() {
            return Err(AppError::Infrastructure(message));
        }
        #[cfg(test)]
        {
            let mut matching_error = self.matching_publication_event_error.lock().unwrap();
            if let Some((_, _, message)) = matching_error.as_ref().filter(|(step, status, _)| {
                events
                    .iter()
                    .any(|event| event.step == *step && event.status == *status)
            }) {
                let message = message.clone();
                matching_error.take();
                return Err(AppError::Infrastructure(message));
            }
        }

        let mut workspaces = self.workspaces.write().await;
        let mut receipts = self.publication_metadata_receipts.write().await;
        let mut decisions = self.pr_metadata_decisions.write().await;
        let mut publication_events = self.publication_events.write().await;
        let Some(workspace) = workspaces.get_mut(conversation_id) else {
            return Ok(false);
        };
        if workspace.publication_metadata_attempt_id.as_deref() != Some(expected_attempt_id)
            || workspace.publication_metadata_phase != Some(expected_phase)
            || workspace.publication_metadata_state != Some(expected_state)
        {
            return Ok(false);
        }

        let updated_at = refresh
            .as_ref()
            .map(|refresh| refresh.updated_at)
            .unwrap_or_else(Utc::now);
        let receipt = receipts.get_mut(conversation_id).ok_or_else(|| {
            AppError::Validation("publication metadata receipt authority is missing".to_string())
        })?;
        receipt.phase = next_phase;
        receipt.state = next_state;
        receipt.updated_at = updated_at;
        if let Some(refresh) = refresh {
            receipt.target_pr_number = refresh.target_pr_number;
            receipt.before_authority_sha256 = refresh.before_authority_sha256;
            receipt.before_title_sha256 = refresh.before_title_sha256;
            receipt.before_editable_body_sha256 = refresh.before_editable_body_sha256;
            receipt.before_managed_suffix_sha256 = refresh.before_managed_suffix_sha256;
            receipt.intended_title_sha256 = refresh.intended_title_sha256;
            receipt.intended_editable_body_sha256 = refresh.intended_editable_body_sha256;
            decisions.insert(conversation_id.clone(), refresh.decision);
        }
        workspace.publication_metadata_phase = Some(next_phase);
        workspace.publication_metadata_state = Some(next_state);
        workspace.updated_at = updated_at;
        publication_events
            .entry(conversation_id.clone())
            .or_default()
            .extend(events);
        Ok(true)
    }

    async fn settle_publication_metadata_receipt_with_events(
        &self,
        conversation_id: &ChatConversationId,
        expected_attempt_id: &str,
        expected_phase: AgentWorkspacePublicationMetadataPhase,
        expected_state: AgentWorkspacePublicationMetadataState,
        next_phase: AgentWorkspacePublicationMetadataPhase,
        next_state: AgentWorkspacePublicationMetadataState,
        publication: AgentWorkspacePublicationUpdate,
        events: Vec<AgentConversationWorkspacePublicationEvent>,
    ) -> AppResult<bool> {
        if events.iter().any(|event| {
            event.conversation_id != *conversation_id
                || event.attempt_id.as_deref() != Some(expected_attempt_id)
        }) {
            return Err(AppError::Validation(
                "publication metadata receipt events must belong to the guarded attempt"
                    .to_string(),
            ));
        }
        #[cfg(test)]
        if let Some(message) = self.next_publication_event_error.lock().unwrap().take() {
            return Err(AppError::Infrastructure(message));
        }
        #[cfg(test)]
        {
            let mut matching_error = self.matching_publication_event_error.lock().unwrap();
            if let Some((_, _, message)) = matching_error.as_ref().filter(|(step, status, _)| {
                events
                    .iter()
                    .any(|event| event.step == *step && event.status == *status)
            }) {
                let message = message.clone();
                matching_error.take();
                return Err(AppError::Infrastructure(message));
            }
        }

        let mut workspaces = self.workspaces.write().await;
        let mut receipts = self.publication_metadata_receipts.write().await;
        let mut publication_events = self.publication_events.write().await;
        let Some(workspace) = workspaces.get_mut(conversation_id) else {
            return Ok(false);
        };
        if workspace.publication_metadata_attempt_id.as_deref() != Some(expected_attempt_id)
            || workspace.publication_metadata_phase != Some(expected_phase)
            || workspace.publication_metadata_state != Some(expected_state)
        {
            return Ok(false);
        }

        let now = Utc::now();
        workspace.publication_pr_number = publication.pr_number;
        workspace.publication_pr_url = publication.pr_url;
        workspace.publication_pr_status = publication.pr_status;
        workspace.publication_push_status = publication.push_status;
        workspace.publication_metadata_phase = Some(next_phase);
        workspace.publication_metadata_state = Some(next_state);
        let receipt = receipts.get_mut(conversation_id).ok_or_else(|| {
            AppError::Validation("publication metadata receipt authority is missing".to_string())
        })?;
        receipt.phase = next_phase;
        receipt.state = next_state;
        receipt.updated_at = now;
        if matches!(
            workspace.publication_pr_status.as_deref(),
            Some("merged" | "closed")
        ) {
            workspace.pr_supervision_status = None;
            workspace.pr_supervision_summary = None;
            workspace.pr_supervision_updated_at = Some(now);
        }
        workspace.updated_at = now;
        publication_events
            .entry(conversation_id.clone())
            .or_default()
            .extend(events);
        Ok(true)
    }

    async fn update_publication_with_events(
        &self,
        conversation_id: &ChatConversationId,
        expected: &AgentWorkspacePublicationGuard,
        publication: AgentWorkspacePublicationUpdate,
        events: Vec<AgentConversationWorkspacePublicationEvent>,
    ) -> AppResult<bool> {
        if events
            .iter()
            .any(|event| event.conversation_id != *conversation_id)
        {
            return Err(AppError::Validation(
                "publication events must belong to the workspace".to_string(),
            ));
        }
        #[cfg(test)]
        if let Some(message) = self.next_publication_event_error.lock().unwrap().take() {
            return Err(AppError::Infrastructure(message));
        }
        let mut workspaces = self.workspaces.write().await;
        let mut publication_events = self.publication_events.write().await;
        let Some(workspace) = workspaces.get_mut(conversation_id) else {
            return Ok(false);
        };
        if AgentWorkspacePublicationGuard::from_workspace(workspace) != *expected {
            return Ok(false);
        }
        let now = Utc::now();
        workspace.publication_pr_number = publication.pr_number;
        workspace.publication_pr_url = publication.pr_url;
        workspace.publication_pr_status = publication.pr_status;
        workspace.publication_push_status = publication.push_status;
        if matches!(
            workspace.publication_pr_status.as_deref(),
            Some("merged" | "closed")
        ) {
            workspace.pr_supervision_status = None;
            workspace.pr_supervision_summary = None;
            workspace.pr_supervision_updated_at = Some(now);
        }
        workspace.updated_at = now;
        publication_events
            .entry(conversation_id.clone())
            .or_default()
            .extend(events);
        Ok(true)
    }

    async fn get_publication_metadata_receipt(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentWorkspacePublicationMetadataReceipt>> {
        let workspace = self.workspaces.read().await.get(conversation_id).cloned();
        let receipt = self
            .publication_metadata_receipts
            .read()
            .await
            .get(conversation_id)
            .cloned();
        match (workspace, receipt) {
            (None, _) => Ok(None),
            (Some(workspace), None)
                if workspace.publication_metadata_phase.is_none()
                    && workspace.publication_metadata_state.is_none()
                    && workspace.publication_metadata_attempt_id.is_none() =>
            {
                Ok(None)
            }
            (Some(_), None) => Err(AppError::Validation(
                "publication metadata receipt authority is incomplete".to_string(),
            )),
            (Some(workspace), Some(receipt))
                if workspace.publication_metadata_attempt_id.as_deref()
                    == Some(&receipt.attempt_id)
                    && workspace.publication_metadata_phase == Some(receipt.phase)
                    && workspace.publication_metadata_state == Some(receipt.state) =>
            {
                validate_publication_metadata_receipt(&receipt)?;
                Ok(Some(receipt))
            }
            (Some(_), Some(_)) => Err(AppError::Validation(
                "publication metadata receipt authority is inconsistent".to_string(),
            )),
        }
    }

    async fn compare_and_set_repair_state(
        &self,
        conversation_id: &ChatConversationId,
        expected: &crate::domain::repositories::AgentWorkspaceRepairStateGuard,
        transition: &crate::domain::repositories::AgentWorkspaceRepairStateTransition,
    ) -> AppResult<bool> {
        // The coarse workspace tuple is migration-era state only. Keep its test seam fenced so
        // an old caller cannot overwrite a durable generation's projection.
        let attempts = self.repair_attempts.write().await;
        if attempts
            .values()
            .any(|attempt| attempt.conversation_id == *conversation_id)
        {
            return Ok(false);
        }
        let mut workspaces = self.workspaces.write().await;
        let Some(workspace) = workspaces.get_mut(conversation_id) else {
            return Ok(false);
        };
        if workspace.publication_push_status != expected.publication_push_status
            || workspace.pr_supervision_status != expected.pr_supervision_status
            || workspace.pr_supervision_updated_at != expected.pr_supervision_updated_at
        {
            return Ok(false);
        }

        workspace.publication_push_status = transition.publication_push_status.clone();
        workspace.pr_supervision_status = transition.pr_supervision_status.clone();
        workspace.pr_supervision_summary = transition.pr_supervision_summary.clone();
        workspace.pr_supervision_updated_at = Some(transition.pr_supervision_updated_at);
        if let Some(auto_merge_current) = transition.pr_auto_merge_current {
            workspace.pr_auto_merge_current = Some(auto_merge_current);
        }
        if let Some(base_commit) = transition.base_commit.as_ref() {
            workspace.base_commit = Some(base_commit.clone());
        }
        workspace.updated_at = transition.pr_supervision_updated_at;
        Ok(true)
    }

    async fn compare_and_set_repair_state_with_events(
        &self,
        conversation_id: &ChatConversationId,
        expected: &crate::domain::repositories::AgentWorkspaceRepairStateGuard,
        transition: &crate::domain::repositories::AgentWorkspaceRepairStateTransition,
        events: Vec<AgentConversationWorkspacePublicationEvent>,
    ) -> AppResult<bool> {
        if events
            .iter()
            .any(|event| event.conversation_id != *conversation_id)
        {
            return Err(AppError::Validation(
                "repair transition events must belong to the guarded workspace".to_string(),
            ));
        }
        #[cfg(test)]
        if let Some(message) = self.next_publication_event_error.lock().unwrap().take() {
            return Err(AppError::Infrastructure(message));
        }
        #[cfg(test)]
        {
            let mut matching_error = self.matching_publication_event_error.lock().unwrap();
            if let Some((_, _, message)) = matching_error.as_ref().filter(|(step, status, _)| {
                events
                    .iter()
                    .any(|event| event.step == *step && event.status == *status)
            }) {
                let message = message.clone();
                matching_error.take();
                return Err(AppError::Infrastructure(message));
            }
        }

        let attempts = self.repair_attempts.write().await;
        if attempts
            .values()
            .any(|attempt| attempt.conversation_id == *conversation_id)
        {
            return Ok(false);
        }
        let mut workspaces = self.workspaces.write().await;
        let mut publication_events = self.publication_events.write().await;
        let Some(workspace) = workspaces.get_mut(conversation_id) else {
            return Ok(false);
        };
        if workspace.publication_push_status != expected.publication_push_status
            || workspace.pr_supervision_status != expected.pr_supervision_status
            || workspace.pr_supervision_updated_at != expected.pr_supervision_updated_at
        {
            return Ok(false);
        }

        workspace.publication_push_status = transition.publication_push_status.clone();
        workspace.pr_supervision_status = transition.pr_supervision_status.clone();
        workspace.pr_supervision_summary = transition.pr_supervision_summary.clone();
        workspace.pr_supervision_updated_at = Some(transition.pr_supervision_updated_at);
        if let Some(auto_merge_current) = transition.pr_auto_merge_current {
            workspace.pr_auto_merge_current = Some(auto_merge_current);
        }
        if let Some(base_commit) = transition.base_commit.as_ref() {
            workspace.base_commit = Some(base_commit.clone());
        }
        workspace.updated_at = transition.pr_supervision_updated_at;
        publication_events
            .entry(conversation_id.clone())
            .or_default()
            .extend(events);
        Ok(true)
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

    async fn set_last_blocked_pr_health_fingerprint(
        &self,
        conversation_id: &ChatConversationId,
        fingerprint: Option<&str>,
    ) -> AppResult<()> {
        if let Some(workspace) = self.workspaces.write().await.get_mut(conversation_id) {
            workspace.last_blocked_pr_health_fingerprint = fingerprint.map(str::to_string);
            workspace.last_blocked_pr_health_at = fingerprint.map(|_| Utc::now());
            workspace.updated_at = Utc::now();
        }
        Ok(())
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

    async fn set_review_automation_override(
        &self,
        conversation_id: &ChatConversationId,
        value: Option<bool>,
    ) -> AppResult<()> {
        let now = Utc::now();
        if let Some(workspace) = self.workspaces.write().await.get_mut(conversation_id) {
            workspace.review_automation_override = value;
            workspace.updated_at = now;
        }
        if value == Some(true) {
            if let Some(monitor) = self
                .workspace_review_monitors
                .write()
                .await
                .get_mut(conversation_id)
            {
                if monitor.review_fixer_status.as_deref()
                    == Some(WORKSPACE_REVIEW_FIXER_STATUS_CYCLE_CAPPED)
                {
                    monitor.review_fixer_cycle_count = 0;
                    monitor.review_fixer_status = None;
                    monitor.review_fixer_attempt_id = None;
                    monitor.updated_at = now;
                }
            }
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

    async fn save_pr_metadata_decision(
        &self,
        conversation_id: &ChatConversationId,
        decision: AgentWorkspacePrMetadataDecision,
    ) -> AppResult<()> {
        self.pr_metadata_decisions
            .write()
            .await
            .insert(conversation_id.clone(), decision);
        Ok(())
    }

    async fn get_pr_metadata_decision(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentWorkspacePrMetadataDecision>> {
        if let Some(decision) = self
            .pr_metadata_decisions
            .read()
            .await
            .get(conversation_id)
            .cloned()
        {
            return Ok(Some(decision));
        }

        // Legacy rows were stored separately from the explicit decision map. Await
        // this read so a contended lock cannot be mistaken for no submission.
        Ok(self
            .pr_descriptions
            .read()
            .await
            .get(conversation_id)
            .cloned()
            .map(|description| AgentWorkspacePrMetadataDecision::Patch {
                title: description.title,
                body_markdown: Some(description.body_markdown),
            }))
    }

    async fn clear_pr_metadata_decision(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<()> {
        self.pr_metadata_decisions
            .write()
            .await
            .remove(conversation_id);
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
        #[cfg(test)]
        {
            let mut matching_error = self.matching_publication_event_error.lock().unwrap();
            if matching_error
                .as_ref()
                .is_some_and(|(step, status, _)| step == &event.step && status == &event.status)
            {
                let (_, _, message) = matching_error.take().expect("matching event error");
                return Err(AppError::Infrastructure(message));
            }
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
            if existing.status == AgentWorkspacePrReviewMonitorStatus::Terminal {
                return Ok(existing.clone());
            }
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
        if monitor.status == AgentWorkspacePrReviewMonitorStatus::Terminal {
            return Err(AppError::Conflict(
                "Review PR settings cannot change after terminal authority".to_string(),
            ));
        }
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
        if monitor.status == AgentWorkspacePrReviewMonitorStatus::Terminal {
            return Err(AppError::Conflict(
                "Review PR settings cannot change after terminal authority".to_string(),
            ));
        }
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

    async fn list_pr_review_lifecycle_monitors(
        &self,
    ) -> AppResult<Vec<AgentWorkspacePrReviewMonitor>> {
        let workspaces = self.workspaces.read().await;
        let mut monitors = self
            .pr_review_monitors
            .read()
            .await
            .values()
            .filter(|monitor| {
                monitor.status != AgentWorkspacePrReviewMonitorStatus::Terminal
                    && workspaces
                        .get(&monitor.conversation_id)
                        .is_some_and(|workspace| {
                            workspace.mode == AgentConversationWorkspaceMode::ReviewPr
                                && workspace.status == AgentConversationWorkspaceStatus::Active
                                && !workspace.has_terminal_publication_pr_status()
                        })
            })
            .cloned()
            .collect::<Vec<_>>();
        monitors.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
        Ok(monitors)
    }

    async fn list_pr_review_lifecycle_recovery_workspaces(
        &self,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        let mut workspaces = self
            .workspaces
            .read()
            .await
            .values()
            .filter(|workspace| {
                workspace.mode == AgentConversationWorkspaceMode::ReviewPr
                    && workspace.status == AgentConversationWorkspaceStatus::Active
                    && workspace
                        .source_pull_request
                        .as_ref()
                        .map(|pull_request| pull_request.number)
                        .or(workspace.publication_pr_number)
                        .is_some()
            })
            .cloned()
            .collect::<Vec<_>>();
        workspaces.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
        Ok(workspaces)
    }

    async fn rearm_terminal_pr_review_monitor_after_live_open(
        &self,
        conversation_id: &ChatConversationId,
        pr_number: i64,
    ) -> AppResult<Option<AgentWorkspacePrReviewMonitor>> {
        let workspaces = self.workspaces.read().await;
        let authorized = workspaces.get(conversation_id).is_some_and(|workspace| {
            workspace.mode == AgentConversationWorkspaceMode::ReviewPr
                && workspace.status == AgentConversationWorkspaceStatus::Active
                && !workspace.has_terminal_publication_pr_status()
                && workspace
                    .source_pull_request
                    .as_ref()
                    .map(|pull_request| pull_request.number)
                    .or(workspace.publication_pr_number)
                    == Some(pr_number)
        });
        if !authorized {
            return Ok(None);
        }
        let mut monitors = self.pr_review_monitors.write().await;
        let Some(monitor) = monitors.get_mut(conversation_id) else {
            return Ok(None);
        };
        if monitor.pr_number != pr_number
            || monitor.status != AgentWorkspacePrReviewMonitorStatus::Terminal
        {
            return Ok(None);
        }
        monitor.monitor_enabled = true;
        monitor.status = AgentWorkspacePrReviewMonitorStatus::Watching;
        monitor.last_error = None;
        monitor.updated_at = Utc::now();
        Ok(Some(monitor.clone()))
    }

    async fn settle_pr_review_terminal(
        &self,
        conversation_id: &ChatConversationId,
        pr_number: i64,
        status: &str,
        summary: &str,
    ) -> AppResult<AgentWorkspacePrTerminalSettlement> {
        if !matches!(status, "merged" | "closed") {
            return Err(AppError::Validation(format!(
                "Review PR terminal status must be merged or closed, got '{status}'"
            )));
        }
        #[cfg(test)]
        if let Some(message) = self.next_publication_update_error.lock().unwrap().take() {
            return Err(AppError::Infrastructure(message));
        }

        let mut workspaces = self.workspaces.write().await;
        let workspace = workspaces
            .get_mut(conversation_id)
            .ok_or_else(|| AppError::NotFound(format!("Workspace not found: {conversation_id}")))?;
        let workspace_pr_number = workspace
            .source_pull_request
            .as_ref()
            .map(|pull_request| pull_request.number)
            .or(workspace.publication_pr_number);
        if workspace.mode != AgentConversationWorkspaceMode::ReviewPr
            || workspace_pr_number != Some(pr_number)
        {
            return Err(AppError::Conflict(
                "Review PR terminal authority does not match this workspace".to_string(),
            ));
        }

        let project_id = workspace.project_id.clone();
        let head_sha = workspace
            .source_pull_request
            .as_ref()
            .and_then(|pull_request| pull_request.head_ref_oid.clone());
        let now = Utc::now();
        let mut monitors = self.pr_review_monitors.write().await;
        let monitor = monitors.entry(conversation_id.clone()).or_insert_with(|| {
            AgentWorkspacePrReviewMonitor::new(
                conversation_id.clone(),
                project_id,
                pr_number,
                head_sha,
            )
        });
        if monitor.pr_number != pr_number {
            return Err(AppError::Conflict(
                "Review PR terminal monitor does not match this workspace".to_string(),
            ));
        }

        workspace.publication_pr_number = workspace.publication_pr_number.or(Some(pr_number));
        workspace.publication_pr_status = Some(status.to_string());
        workspace.pr_supervision_status = None;
        workspace.pr_supervision_summary = None;
        workspace.pr_supervision_updated_at = Some(now);
        workspace.updated_at = now;
        monitor.status = AgentWorkspacePrReviewMonitorStatus::Terminal;
        monitor.monitor_enabled = false;
        monitor.last_review_outcome = Some(status.to_string());
        monitor.last_error = (status == "closed").then(|| summary.to_string());
        monitor.updated_at = now;

        let mut actions = self.pr_review_actions.write().await;
        let mut superseded_action_ids = Vec::new();
        for action in actions.values_mut() {
            if action.conversation_id == *conversation_id
                && action.pr_number == pr_number
                && matches!(
                    action.status,
                    AgentWorkspacePrReviewActionStatus::Pending
                        | AgentWorkspacePrReviewActionStatus::Submitting
                )
            {
                superseded_action_ids.push(action.id.clone());
                action.status = AgentWorkspacePrReviewActionStatus::Superseded;
                action.resolved_at = Some(now);
                action.updated_at = now;
            }
        }
        superseded_action_ids = actions
            .values()
            .filter(|action| {
                action.conversation_id == *conversation_id
                    && action.pr_number == pr_number
                    && action.status == AgentWorkspacePrReviewActionStatus::Superseded
            })
            .map(|action| action.id.clone())
            .collect();
        superseded_action_ids.sort();

        let step = format!("pr_{status}");
        let mut events = self.publication_events.write().await;
        let conversation_events = events.entry(conversation_id.clone()).or_default();
        let event_inserted = !conversation_events
            .iter()
            .any(|event| event.step == step && event.status == "succeeded");
        if event_inserted {
            conversation_events.push(AgentConversationWorkspacePublicationEvent::new(
                conversation_id.clone(),
                step,
                "succeeded",
                summary,
                None,
            ));
        }

        Ok(AgentWorkspacePrTerminalSettlement {
            superseded_action_ids,
            event_inserted,
        })
    }

    async fn transition_pr_review_state_if_nonterminal(
        &self,
        mut monitor: AgentWorkspacePrReviewMonitor,
        action_mutation: Option<AgentWorkspacePrReviewActionMutation>,
    ) -> AppResult<Option<AgentWorkspacePrReviewStateTransition>> {
        let workspaces = self.workspaces.read().await;
        let authorized = workspaces
            .get(&monitor.conversation_id)
            .is_some_and(|workspace| {
                workspace.mode == AgentConversationWorkspaceMode::ReviewPr
                    && workspace.status == AgentConversationWorkspaceStatus::Active
                    && !workspace.has_terminal_publication_pr_status()
                    && workspace
                        .source_pull_request
                        .as_ref()
                        .map(|pull_request| pull_request.number)
                        .or(workspace.publication_pr_number)
                        == Some(monitor.pr_number)
            });
        if !authorized {
            return Ok(None);
        }

        let mut monitors = self.pr_review_monitors.write().await;
        let existing_monitor = match monitors.get(&monitor.conversation_id).cloned() {
            Some(existing_monitor) => existing_monitor,
            None if action_mutation.is_none() => {
                monitors.insert(monitor.conversation_id.clone(), monitor.clone());
                monitor.clone()
            }
            None => return Ok(None),
        };
        if existing_monitor.pr_number != monitor.pr_number
            || existing_monitor.status == AgentWorkspacePrReviewMonitorStatus::Terminal
            || monitor.updated_at < existing_monitor.updated_at
        {
            return Ok(None);
        }

        let mut actions = self.pr_review_actions.write().await;
        let action = match action_mutation {
            Some(AgentWorkspacePrReviewActionMutation::UpsertPending(mut action)) => {
                if action.conversation_id != monitor.conversation_id
                    || action.pr_number != monitor.pr_number
                    || action.status != AgentWorkspacePrReviewActionStatus::Pending
                {
                    return Ok(None);
                }
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
                    Some(existing.clone())
                } else {
                    action.updated_at = Utc::now();
                    actions.insert(action.id.clone(), action.clone());
                    Some(action)
                }
            }
            Some(AgentWorkspacePrReviewActionMutation::CompareAndSet {
                action_id,
                expected,
                status,
                submitted_review_id,
            }) => {
                let Some(action) = actions.get_mut(&action_id) else {
                    return Ok(None);
                };
                if action.conversation_id != monitor.conversation_id
                    || action.pr_number != monitor.pr_number
                    || action.status != expected
                {
                    return Ok(None);
                }
                action.status = status;
                action.submitted_review_id = submitted_review_id;
                action.updated_at = Utc::now();
                action.resolved_at = pr_review_action_terminal_status(status).then(Utc::now);
                Some(action.clone())
            }
            None => None,
        };

        monitor.created_at = existing_monitor.created_at;
        monitor.updated_at = Utc::now();
        monitors.insert(monitor.conversation_id.clone(), monitor.clone());
        Ok(Some(AgentWorkspacePrReviewStateTransition {
            monitor,
            action,
        }))
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
            if monitor.review_requested_changes_artifact_id.is_none() {
                monitor.review_requested_changes_artifact_id =
                    existing.review_requested_changes_artifact_id.clone();
                monitor.review_requested_changes_artifact_version =
                    existing.review_requested_changes_artifact_version;
                monitor.review_requested_changes_artifact_updated_at =
                    existing.review_requested_changes_artifact_updated_at;
            }
            if monitor.previous_version_id.is_none() {
                monitor.previous_version_id = existing.previous_version_id.clone();
            }
            if monitor
                .review_requested_changes_previous_version_id
                .is_none()
            {
                monitor.review_requested_changes_previous_version_id = existing
                    .review_requested_changes_previous_version_id
                    .clone();
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

    async fn claim_workspace_review_fixer(
        &self,
        conversation_id: &ChatConversationId,
        snapshot: &AgentWorkspaceReviewFixerSnapshot,
        attempt_id: &str,
        claimed_at: DateTime<Utc>,
    ) -> AppResult<Option<AgentWorkspaceReviewMonitor>> {
        let mut monitors = self.workspace_review_monitors.write().await;
        let Some(monitor) = monitors.get_mut(conversation_id) else {
            return Ok(None);
        };
        if attempt_id.trim().is_empty()
            || monitor.status != AgentWorkspaceReviewMonitorStatus::Ready
            || monitor.review_outcome != AgentWorkspaceReviewOutcome::Blocking
            || monitor.review_gate_status != AgentWorkspaceReviewGateStatus::Blocking
            || monitor.current_target_scope != Some(snapshot.target_scope)
            || monitor.reviewed_target_scope != Some(snapshot.target_scope)
            || monitor.current_diff_fingerprint.as_deref()
                != Some(snapshot.diff_fingerprint.as_str())
            || monitor.reviewed_diff_fingerprint.as_deref()
                != Some(snapshot.diff_fingerprint.as_str())
            || monitor.current_plan_context_fingerprint != snapshot.plan_context_fingerprint
            || monitor.reviewed_plan_context_fingerprint != snapshot.plan_context_fingerprint
            || monitor.review_artifact_id.as_ref() != Some(&snapshot.artifact_id)
            || monitor.review_artifact_version != Some(snapshot.artifact_version)
            || monitor.review_requested_changes_artifact_id.as_ref()
                != Some(&snapshot.requested_changes_artifact_id)
            || monitor.review_requested_changes_artifact_version
                != Some(snapshot.requested_changes_artifact_version)
            || monitor.review_blocking_fingerprint.as_deref()
                != Some(snapshot.blocking_fingerprint.as_str())
            || matches!(
                monitor.review_fixer_status.as_deref(),
                Some("routing" | "queued" | "running")
            )
        {
            return Ok(None);
        }
        monitor.review_fixer_status = Some("routing".to_string());
        monitor.review_fixer_attempt_id = Some(attempt_id.to_string());
        monitor.review_fixer_cycle_count = monitor.review_fixer_cycle_count.saturating_add(1);
        monitor.review_fixer_run_id = None;
        monitor.review_fixer_conversation_id = None;
        monitor.last_error = None;
        monitor.updated_at = claimed_at;
        Ok(Some(monitor.clone()))
    }

    async fn settle_workspace_review_fixer_attempt(
        &self,
        next: AgentWorkspaceReviewMonitor,
        expected_attempt_id: &str,
        expected_snapshot: &AgentWorkspaceReviewFixerSnapshot,
    ) -> AppResult<Option<AgentWorkspaceReviewMonitor>> {
        let mut monitors = self.workspace_review_monitors.write().await;
        let Some(current) = monitors.get_mut(&next.conversation_id) else {
            return Ok(None);
        };
        if current.review_fixer_attempt_id.as_deref() != Some(expected_attempt_id)
            || current.current_target_scope != Some(expected_snapshot.target_scope)
            || current.reviewed_target_scope != Some(expected_snapshot.target_scope)
            || current.current_diff_fingerprint.as_deref()
                != Some(expected_snapshot.diff_fingerprint.as_str())
            || current.reviewed_diff_fingerprint.as_deref()
                != Some(expected_snapshot.diff_fingerprint.as_str())
            || current.current_plan_context_fingerprint
                != expected_snapshot.plan_context_fingerprint
            || current.reviewed_plan_context_fingerprint
                != expected_snapshot.plan_context_fingerprint
            || current.review_artifact_id.as_ref() != Some(&expected_snapshot.artifact_id)
            || current.review_artifact_version != Some(expected_snapshot.artifact_version)
            || current.review_requested_changes_artifact_id.as_ref()
                != Some(&expected_snapshot.requested_changes_artifact_id)
            || current.review_requested_changes_artifact_version
                != Some(expected_snapshot.requested_changes_artifact_version)
            || current.review_blocking_fingerprint.as_deref()
                != Some(expected_snapshot.blocking_fingerprint.as_str())
        {
            return Ok(None);
        }
        current.review_fixer_status = next.review_fixer_status;
        current.review_fixer_run_id = next.review_fixer_run_id;
        current.review_fixer_conversation_id = next.review_fixer_conversation_id;
        current.last_error = next.last_error;
        current.updated_at = Utc::now();
        Ok(Some(current.clone()))
    }

    async fn fail_invalid_workspace_review_fixer_attempt(
        &self,
        conversation_id: &ChatConversationId,
        expected_attempt_id: Option<&str>,
        error: &str,
    ) -> AppResult<Option<AgentWorkspaceReviewMonitor>> {
        let mut monitors = self.workspace_review_monitors.write().await;
        let Some(current) = monitors.get_mut(conversation_id) else {
            return Ok(None);
        };
        if current.review_fixer_attempt_id.as_deref() != expected_attempt_id
            || !matches!(
                current.review_fixer_status.as_deref(),
                Some("routing" | "queued" | "running")
            )
            || AgentWorkspaceReviewFixerSnapshot::from_monitor(current).is_some()
        {
            return Ok(None);
        }
        current.review_fixer_status = Some("failed".to_string());
        current.review_fixer_run_id = None;
        current.review_fixer_conversation_id = None;
        current.last_error = Some(error.to_string());
        current.updated_at = Utc::now();
        Ok(Some(current.clone()))
    }

    async fn fail_reserved_workspace_review_start(
        &self,
        conversation_id: &ChatConversationId,
        expected_target_scope: AgentWorkspaceReviewTargetScope,
        expected_diff_fingerprint: &str,
        expected_review_conversation_id: &ChatConversationId,
        expected_run_id: &str,
        error: &str,
    ) -> AppResult<bool> {
        let mut monitors = self.workspace_review_monitors.write().await;
        let Some(monitor) = monitors.get_mut(conversation_id) else {
            return Ok(false);
        };
        if monitor.status != AgentWorkspaceReviewMonitorStatus::Reviewing
            || monitor.current_target_scope != Some(expected_target_scope)
            || monitor.current_diff_fingerprint.as_deref() != Some(expected_diff_fingerprint)
            || monitor.review_conversation_id.as_ref() != Some(expected_review_conversation_id)
            || monitor.last_run_id.as_deref() != Some(expected_run_id)
        {
            return Ok(false);
        }

        monitor.status = AgentWorkspaceReviewMonitorStatus::Blocked;
        monitor.review_outcome = AgentWorkspaceReviewOutcome::RunFailed;
        monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Failed;
        monitor.review_blocking_summary = None;
        monitor.review_blocking_fingerprint = None;
        monitor.review_fixer_run_id = None;
        monitor.review_fixer_conversation_id = None;
        monitor.review_fixer_status = None;
        monitor.review_fixer_attempt_id = None;
        monitor.last_error = Some(error.to_string());
        monitor.updated_at = Utc::now();
        Ok(true)
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
            || !monitor.has_review_artifact_pair()
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

    async fn list_active_workspace_review_fixers(
        &self,
    ) -> AppResult<Vec<AgentWorkspaceReviewMonitor>> {
        let mut monitors = self
            .workspace_review_monitors
            .read()
            .await
            .values()
            .filter(|monitor| {
                matches!(
                    monitor.review_fixer_status.as_deref(),
                    Some("routing" | "queued" | "running")
                )
            })
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

    async fn create_or_update_pr_review_action_if_nonterminal(
        &self,
        mut action: AgentWorkspacePrReviewAction,
    ) -> AppResult<AgentWorkspacePrReviewAction> {
        let workspaces = self.workspaces.read().await;
        let authorized = workspaces
            .get(&action.conversation_id)
            .is_some_and(|workspace| {
                workspace.mode == AgentConversationWorkspaceMode::ReviewPr
                    && !workspace.has_terminal_publication_pr_status()
                    && workspace
                        .source_pull_request
                        .as_ref()
                        .map(|pull_request| pull_request.number)
                        .or(workspace.publication_pr_number)
                        == Some(action.pr_number)
            });
        if !authorized {
            return Err(AppError::Conflict(
                "Review PR action cannot be proposed after terminal authority".to_string(),
            ));
        }
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

    async fn get_latest_pending_pr_review_action(
        &self,
        conversation_id: &ChatConversationId,
        pr_number: i64,
    ) -> AppResult<Option<AgentWorkspacePrReviewAction>> {
        Ok(self
            .pr_review_actions
            .read()
            .await
            .values()
            .filter(|action| {
                action.conversation_id == *conversation_id
                    && action.pr_number == pr_number
                    && action.status == AgentWorkspacePrReviewActionStatus::Pending
            })
            .max_by(|left, right| {
                left.updated_at
                    .cmp(&right.updated_at)
                    .then_with(|| left.created_at.cmp(&right.created_at))
                    .then_with(|| left.id.cmp(&right.id))
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

    async fn claim_pending_pr_review_action_if_nonterminal(
        &self,
        action_id: &str,
        conversation_id: &ChatConversationId,
        pr_number: i64,
    ) -> AppResult<bool> {
        let workspaces = self.workspaces.read().await;
        let authorized = workspaces.get(conversation_id).is_some_and(|workspace| {
            workspace.mode == AgentConversationWorkspaceMode::ReviewPr
                && !workspace.has_terminal_publication_pr_status()
                && workspace
                    .source_pull_request
                    .as_ref()
                    .map(|pull_request| pull_request.number)
                    .or(workspace.publication_pr_number)
                    == Some(pr_number)
        });
        if !authorized {
            return Ok(false);
        }
        let mut actions = self.pr_review_actions.write().await;
        let Some(action) = actions.get_mut(action_id) else {
            return Ok(false);
        };
        if action.conversation_id != *conversation_id
            || action.pr_number != pr_number
            || action.status != AgentWorkspacePrReviewActionStatus::Pending
        {
            return Ok(false);
        }
        action.status = AgentWorkspacePrReviewActionStatus::Submitting;
        action.updated_at = Utc::now();
        Ok(true)
    }

    async fn compare_and_set_pr_review_action_status(
        &self,
        action_id: &str,
        expected: AgentWorkspacePrReviewActionStatus,
        status: AgentWorkspacePrReviewActionStatus,
        submitted_review_id: Option<&str>,
    ) -> AppResult<bool> {
        let mut actions = self.pr_review_actions.write().await;
        let Some(action) = actions.get_mut(action_id) else {
            return Ok(false);
        };
        if action.status != expected {
            return Ok(false);
        }
        action.status = status;
        action.submitted_review_id = submitted_review_id.map(str::to_string);
        action.updated_at = Utc::now();
        action.resolved_at = pr_review_action_terminal_status(status).then(Utc::now);
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
        && matches!(
            (
                workspace.publication_push_status.as_deref(),
                workspace.pr_supervision_status.as_deref(),
            ),
            (Some("failed"), Some("blocked")) | (Some("refreshed"), Some("fixing" | "reviewing"))
        )
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

fn is_active_pending_publication_metadata_receipt_workspace(
    workspace: &AgentConversationWorkspace,
) -> bool {
    workspace.status == AgentConversationWorkspaceStatus::Active
        && matches!(
            workspace.publication_metadata_phase,
            Some(
                AgentWorkspacePublicationMetadataPhase::Prepared
                    | AgentWorkspacePublicationMetadataPhase::Mutating
                    | AgentWorkspacePublicationMetadataPhase::Reconciling
            )
        )
        && !matches!(
            workspace.publication_pr_status.as_deref(),
            Some("closed") | Some("merged")
        )
}
