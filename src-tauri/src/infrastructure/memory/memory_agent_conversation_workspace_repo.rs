use std::collections::HashMap;

use async_trait::async_trait;
use chrono::Utc;
use tokio::sync::RwLock;

use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode,
    AgentConversationWorkspacePublicationEvent, AgentConversationWorkspaceStatus,
    AgentWorkspacePrCommentEvidence, AgentWorkspacePrCommentEvidenceUpsert,
    AgentWorkspacePrDescription, ChatConversationId, IdeationSessionId, PlanBranchId, ProjectId,
    DEFAULT_AGENT_WORKSPACE_PR_AUTO_MERGE_METHOD,
};
use crate::domain::repositories::AgentConversationWorkspaceRepository;
use crate::error::AppResult;

pub struct MemoryAgentConversationWorkspaceRepository {
    workspaces: RwLock<HashMap<ChatConversationId, AgentConversationWorkspace>>,
    pr_descriptions: RwLock<HashMap<ChatConversationId, AgentWorkspacePrDescription>>,
    publication_events:
        RwLock<HashMap<ChatConversationId, Vec<AgentConversationWorkspacePublicationEvent>>>,
    pr_comment_evidence: RwLock<HashMap<(String, i64, String), AgentWorkspacePrCommentEvidence>>,
}

impl MemoryAgentConversationWorkspaceRepository {
    pub fn new() -> Self {
        Self {
            workspaces: RwLock::new(HashMap::new()),
            pr_descriptions: RwLock::new(HashMap::new()),
            publication_events: RwLock::new(HashMap::new()),
            pr_comment_evidence: RwLock::new(HashMap::new()),
        }
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

    async fn update_publication(
        &self,
        conversation_id: &ChatConversationId,
        pr_number: Option<i64>,
        pr_url: Option<&str>,
        pr_status: Option<&str>,
        push_status: Option<&str>,
    ) -> AppResult<()> {
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

    async fn update_pr_supervision_preferences(
        &self,
        conversation_id: &ChatConversationId,
        autofix_enabled: bool,
        auto_merge_desired: bool,
        auto_merge_method: &str,
    ) -> AppResult<()> {
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

    async fn delete(&self, conversation_id: &ChatConversationId) -> AppResult<()> {
        self.workspaces.write().await.remove(conversation_id);
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
        Ok(())
    }
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

fn is_active_direct_external_pr_reconciliation_candidate(
    workspace: &AgentConversationWorkspace,
) -> bool {
    if workspace.mode != AgentConversationWorkspaceMode::Edit
        || workspace.linked_plan_branch_id.is_some()
        || matches!(
            workspace.publication_pr_status.as_deref(),
            Some("closed") | Some("merged")
        )
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

fn is_active_needs_agent_workspace(workspace: &AgentConversationWorkspace) -> bool {
    workspace.status == AgentConversationWorkspaceStatus::Active
        && workspace.publication_push_status.as_deref() == Some("needs_agent")
        && !matches!(
            workspace.publication_pr_status.as_deref(),
            Some("closed") | Some("merged")
        )
}

#[cfg(test)]
mod tests {
    use crate::domain::entities::{
        AgentConversationWorkspace, AgentConversationWorkspaceMode,
        AgentConversationWorkspacePublicationEvent, AgentConversationWorkspaceStatus,
        AgentWorkspacePrCommentEvidenceUpsert, AgentWorkspacePrDescription, ChatConversationId,
        IdeationAnalysisBaseRefKind, IdeationSessionId, PlanBranchId, ProjectId,
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
        repo.create_or_update(second.clone()).await.unwrap();

        let loaded = repo
            .get_by_linked_ideation_session_id(&session_id)
            .await
            .unwrap()
            .expect("latest linked workspace should load");
        assert_eq!(loaded.conversation_id, second.conversation_id);

        let missing = repo
            .get_by_linked_ideation_session_id(&IdeationSessionId::from_string("missing-session"))
            .await
            .unwrap();
        assert!(missing.is_none());
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
        assert_eq!(limited[0].conversation_id, linked_missing.conversation_id);

        let all = repo
            .list_active_direct_external_pr_reconciliation_candidates(10)
            .await
            .unwrap();
        assert_eq!(
            all.into_iter()
                .map(|workspace| workspace.conversation_id)
                .collect::<Vec<_>>(),
            vec![
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
}
