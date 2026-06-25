use std::str::FromStr;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use rusqlite::{Connection, OptionalExtension};
use tokio::sync::Mutex;

use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode,
    AgentConversationWorkspacePublicationEvent, AgentConversationWorkspaceStatus,
    AgentWorkspacePrCommentEvidence, AgentWorkspacePrCommentEvidenceUpsert,
    AgentWorkspacePrDescription, AgentWorkspacePrReviewAction, AgentWorkspacePrReviewActionKind,
    AgentWorkspacePrReviewActionStatus, AgentWorkspacePrReviewMonitor,
    AgentWorkspacePrReviewMonitorStatus, AgentWorkspaceReviewMonitor,
    AgentWorkspaceReviewMonitorStatus, AgentWorkspaceReviewTargetScope,
    AgentWorkspaceSourcePullRequest, ArtifactId, ChatConversationId, IdeationAnalysisBaseRefKind,
    IdeationSessionId, PlanBranchId, ProjectId, DEFAULT_AGENT_WORKSPACE_PR_AUTO_MERGE_METHOD,
};
use crate::domain::repositories::AgentConversationWorkspaceRepository;
use crate::error::{AppError, AppResult};
use crate::infrastructure::sqlite::DbConnection;

fn parse_datetime(value: &str) -> DateTime<Utc> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(value) {
        return dt.with_timezone(&Utc);
    }
    if let Ok(dt) = NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S") {
        return Utc.from_utc_datetime(&dt);
    }
    Utc::now()
}

#[cfg(test)]
#[path = "sqlite_agent_conversation_workspace_repo_tests.rs"]
mod tests;

fn row_to_workspace(row: &rusqlite::Row<'_>) -> rusqlite::Result<AgentConversationWorkspace> {
    let mode: String = row.get("mode")?;
    let base_ref_kind: String = row.get("base_ref_kind")?;
    let status: String = row.get("status")?;
    let created_at: String = row.get("created_at")?;
    let updated_at: String = row.get("updated_at")?;
    let source_pr_number: Option<i64> = row.get("source_pr_number")?;
    let source_pr_head_ref: Option<String> = row.get("source_pr_head_ref")?;
    let source_pull_request = source_pr_number
        .zip(source_pr_head_ref)
        .map(|(number, head_ref_name)| -> rusqlite::Result<_> {
            Ok(AgentWorkspaceSourcePullRequest {
                number,
                url: row.get("source_pr_url")?,
                title: row.get("source_pr_title")?,
                head_ref_name,
                base_ref_name: row.get("source_pr_base_ref")?,
                head_ref_oid: row.get("source_pr_head_sha")?,
            })
        })
        .transpose()?;

    Ok(AgentConversationWorkspace {
        conversation_id: ChatConversationId::from_string(row.get::<_, String>("conversation_id")?),
        project_id: ProjectId::from_string(row.get::<_, String>("project_id")?),
        mode: AgentConversationWorkspaceMode::from_str(&mode)
            .unwrap_or(AgentConversationWorkspaceMode::Edit),
        base_ref_kind: IdeationAnalysisBaseRefKind::from_str(&base_ref_kind)
            .unwrap_or(IdeationAnalysisBaseRefKind::ProjectDefault),
        base_ref: row.get("base_ref")?,
        base_display_name: row.get("base_display_name")?,
        base_commit: row.get("base_commit")?,
        branch_name: row.get("branch_name")?,
        worktree_path: row.get("worktree_path")?,
        linked_ideation_session_id: row
            .get::<_, Option<String>>("linked_ideation_session_id")?
            .map(IdeationSessionId::from_string),
        linked_plan_branch_id: row
            .get::<_, Option<String>>("linked_plan_branch_id")?
            .map(PlanBranchId::from_string),
        source_pull_request,
        publication_pr_number: row.get("publication_pr_number")?,
        publication_pr_url: row.get("publication_pr_url")?,
        publication_pr_status: row.get("publication_pr_status")?,
        publication_push_status: row.get("publication_push_status")?,
        auto_publish_enabled: row.get("auto_publish_enabled")?,
        auto_publish_initial_pr_enabled: row.get("auto_publish_initial_pr_enabled")?,
        auto_publish_paused_pr_autofix_enabled: row
            .get("auto_publish_paused_pr_autofix_enabled")?,
        auto_publish_paused_pr_auto_merge_desired: row
            .get("auto_publish_paused_pr_auto_merge_desired")?,
        pr_autofix_enabled: row.get("pr_autofix_enabled")?,
        pr_auto_merge_desired: row.get("pr_auto_merge_desired")?,
        pr_auto_merge_method: row
            .get::<_, Option<String>>("pr_auto_merge_method")?
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_AGENT_WORKSPACE_PR_AUTO_MERGE_METHOD.to_string()),
        pr_auto_merge_current: row.get("pr_auto_merge_current")?,
        pr_supervision_status: row.get("pr_supervision_status")?,
        pr_supervision_summary: row.get("pr_supervision_summary")?,
        pr_supervision_updated_at: row
            .get::<_, Option<String>>("pr_supervision_updated_at")?
            .map(|value| parse_datetime(&value)),
        status: AgentConversationWorkspaceStatus::from_str(&status)
            .unwrap_or(AgentConversationWorkspaceStatus::Active),
        created_at: parse_datetime(&created_at),
        updated_at: parse_datetime(&updated_at),
    })
}

fn row_to_publication_event(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<AgentConversationWorkspacePublicationEvent> {
    let created_at: String = row.get("created_at")?;
    Ok(AgentConversationWorkspacePublicationEvent {
        id: row.get("id")?,
        conversation_id: ChatConversationId::from_string(row.get::<_, String>("conversation_id")?),
        step: row.get("step")?,
        status: row.get("status")?,
        summary: row.get("summary")?,
        classification: row.get("classification")?,
        created_at: parse_datetime(&created_at),
    })
}

fn row_to_pr_comment_evidence(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<AgentWorkspacePrCommentEvidence> {
    let first_seen_at: String = row.get("first_seen_at")?;
    let last_seen_at: String = row.get("last_seen_at")?;
    Ok(AgentWorkspacePrCommentEvidence {
        conversation_id: ChatConversationId::from_string(row.get::<_, String>("conversation_id")?),
        pr_number: row.get("pr_number")?,
        comment_id: row.get("comment_id")?,
        author: row.get("author")?,
        body: row.get("body")?,
        body_excerpt: row.get("body_excerpt")?,
        body_sha256: row.get("body_sha256")?,
        url: row.get("url")?,
        github_created_at: row.get("github_created_at")?,
        github_updated_at: row.get("github_updated_at")?,
        is_codecov: row.get("is_codecov")?,
        is_bot: row.get("is_bot")?,
        first_seen_at: parse_datetime(&first_seen_at),
        last_seen_at: parse_datetime(&last_seen_at),
        last_included_at: row
            .get::<_, Option<String>>("last_included_at")?
            .map(|value| parse_datetime(&value)),
        last_read_at: row
            .get::<_, Option<String>>("last_read_at")?
            .map(|value| parse_datetime(&value)),
        edit_count: row.get("edit_count")?,
    })
}

fn row_to_pr_review_monitor(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<AgentWorkspacePrReviewMonitor> {
    let status: String = row.get("status")?;
    let created_at: String = row.get("created_at")?;
    let updated_at: String = row.get("updated_at")?;
    Ok(AgentWorkspacePrReviewMonitor {
        conversation_id: ChatConversationId::from_string(row.get::<_, String>("conversation_id")?),
        project_id: ProjectId::from_string(row.get::<_, String>("project_id")?),
        pr_number: row.get("pr_number")?,
        status: AgentWorkspacePrReviewMonitorStatus::from_str(&status)
            .unwrap_or(AgentWorkspacePrReviewMonitorStatus::Idle),
        monitor_enabled: row.get("monitor_enabled")?,
        first_review_completed: row.get("first_review_completed")?,
        last_seen_head_sha: row.get("last_seen_head_sha")?,
        last_reviewed_head_sha: row.get("last_reviewed_head_sha")?,
        last_review_run_id: row.get("last_review_run_id")?,
        last_review_outcome: row.get("last_review_outcome")?,
        last_submitted_review_id: row.get("last_submitted_review_id")?,
        review_artifact_id: row
            .get::<_, Option<String>>("review_artifact_id")?
            .map(ArtifactId::from_string),
        review_artifact_head_sha: row.get("review_artifact_head_sha")?,
        review_artifact_version: row
            .get::<_, Option<i64>>("review_artifact_version")?
            .and_then(|value| u32::try_from(value).ok()),
        review_artifact_updated_at: row
            .get::<_, Option<String>>("review_artifact_updated_at")?
            .map(|value| parse_datetime(&value)),
        last_error: row.get("last_error")?,
        created_at: parse_datetime(&created_at),
        updated_at: parse_datetime(&updated_at),
    })
}

fn row_to_workspace_review_monitor(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<AgentWorkspaceReviewMonitor> {
    let status: String = row.get("status")?;
    let current_target_scope = row
        .get::<_, Option<String>>("current_target_scope")?
        .and_then(|value| AgentWorkspaceReviewTargetScope::from_str(&value).ok());
    let reviewed_target_scope = row
        .get::<_, Option<String>>("reviewed_target_scope")?
        .and_then(|value| AgentWorkspaceReviewTargetScope::from_str(&value).ok());
    let created_at: String = row.get("created_at")?;
    let updated_at: String = row.get("updated_at")?;
    Ok(AgentWorkspaceReviewMonitor {
        conversation_id: ChatConversationId::from_string(row.get::<_, String>("conversation_id")?),
        project_id: ProjectId::from_string(row.get::<_, String>("project_id")?),
        status: AgentWorkspaceReviewMonitorStatus::from_str(&status)
            .unwrap_or(AgentWorkspaceReviewMonitorStatus::Idle),
        current_target_scope,
        reviewed_target_scope,
        review_artifact_id: row
            .get::<_, Option<String>>("review_artifact_id")?
            .map(ArtifactId::from_string),
        review_artifact_version: row
            .get::<_, Option<i64>>("review_artifact_version")?
            .and_then(|value| u32::try_from(value).ok()),
        review_artifact_updated_at: row
            .get::<_, Option<String>>("review_artifact_updated_at")?
            .map(|value| parse_datetime(&value)),
        reviewed_head_sha: row.get("reviewed_head_sha")?,
        reviewed_diff_fingerprint: row.get("reviewed_diff_fingerprint")?,
        selected_source_base_ref: row.get("selected_source_base_ref")?,
        selected_source_base_sha: row.get("selected_source_base_sha")?,
        selected_source_head_ref: row.get("selected_source_head_ref")?,
        selected_source_head_sha: row.get("selected_source_head_sha")?,
        selected_source_pull_request_number: row.get("selected_source_pull_request_number")?,
        workspace_base_ref: row.get("workspace_base_ref")?,
        workspace_base_sha: row.get("workspace_base_sha")?,
        workspace_head_ref: row.get("workspace_head_ref")?,
        workspace_head_sha: row.get("workspace_head_sha")?,
        current_diff_fingerprint: row.get("current_diff_fingerprint")?,
        previous_version_id: row
            .get::<_, Option<String>>("previous_version_id")?
            .map(ArtifactId::from_string),
        last_run_id: row.get("last_run_id")?,
        last_error: row.get("last_error")?,
        created_at: parse_datetime(&created_at),
        updated_at: parse_datetime(&updated_at),
    })
}

fn row_to_pr_review_action(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<AgentWorkspacePrReviewAction> {
    let proposed_action: String = row.get("proposed_action")?;
    let status: String = row.get("status")?;
    let created_at: String = row.get("created_at")?;
    let updated_at: String = row.get("updated_at")?;
    Ok(AgentWorkspacePrReviewAction {
        id: row.get("id")?,
        conversation_id: ChatConversationId::from_string(row.get::<_, String>("conversation_id")?),
        pr_number: row.get("pr_number")?,
        head_sha: row.get("head_sha")?,
        proposed_action: AgentWorkspacePrReviewActionKind::from_str(&proposed_action)
            .unwrap_or(AgentWorkspacePrReviewActionKind::Comment),
        summary: row.get("summary")?,
        review_body: row.get("review_body")?,
        findings_json: row.get("findings_json")?,
        status: AgentWorkspacePrReviewActionStatus::from_str(&status)
            .unwrap_or(AgentWorkspacePrReviewActionStatus::Pending),
        submitted_review_id: row.get("submitted_review_id")?,
        created_by_run_id: row.get("created_by_run_id")?,
        created_at: parse_datetime(&created_at),
        updated_at: parse_datetime(&updated_at),
        resolved_at: row
            .get::<_, Option<String>>("resolved_at")?
            .map(|value| parse_datetime(&value)),
    })
}

pub struct SqliteAgentConversationWorkspaceRepository {
    db: DbConnection,
}

impl SqliteAgentConversationWorkspaceRepository {
    pub fn new(conn: Connection) -> Self {
        Self {
            db: DbConnection::new(conn),
        }
    }

    pub fn from_shared(conn: Arc<Mutex<Connection>>) -> Self {
        Self {
            db: DbConnection::from_shared(conn),
        }
    }
}

#[async_trait]
impl AgentConversationWorkspaceRepository for SqliteAgentConversationWorkspaceRepository {
    async fn create_or_update(
        &self,
        workspace: AgentConversationWorkspace,
    ) -> AppResult<AgentConversationWorkspace> {
        let conversation_id = workspace.conversation_id.as_str().to_string();
        let project_id = workspace.project_id.as_str().to_string();
        let mode = workspace.mode.to_string();
        let base_ref_kind = workspace.base_ref_kind.to_string();
        let base_ref = workspace.base_ref.clone();
        let base_display_name = workspace.base_display_name.clone();
        let base_commit = workspace.base_commit.clone();
        let branch_name = workspace.branch_name.clone();
        let worktree_path = workspace.worktree_path.clone();
        let linked_ideation_session_id = workspace
            .linked_ideation_session_id
            .as_ref()
            .map(|id| id.as_str().to_string());
        let linked_plan_branch_id = workspace
            .linked_plan_branch_id
            .as_ref()
            .map(|id| id.as_str().to_string());
        let source_pr_number = workspace
            .source_pull_request
            .as_ref()
            .map(|pull_request| pull_request.number);
        let source_pr_url = workspace
            .source_pull_request
            .as_ref()
            .and_then(|pull_request| pull_request.url.clone());
        let source_pr_title = workspace
            .source_pull_request
            .as_ref()
            .and_then(|pull_request| pull_request.title.clone());
        let source_pr_head_ref = workspace
            .source_pull_request
            .as_ref()
            .map(|pull_request| pull_request.head_ref_name.clone());
        let source_pr_base_ref = workspace
            .source_pull_request
            .as_ref()
            .and_then(|pull_request| pull_request.base_ref_name.clone());
        let source_pr_head_sha = workspace
            .source_pull_request
            .as_ref()
            .and_then(|pull_request| pull_request.head_ref_oid.clone());
        let publication_pr_number = workspace.publication_pr_number;
        let publication_pr_url = workspace.publication_pr_url.clone();
        let publication_pr_status = workspace.publication_pr_status.clone();
        let publication_push_status = workspace.publication_push_status.clone();
        let auto_publish_enabled = workspace.auto_publish_enabled;
        let auto_publish_initial_pr_enabled = workspace.auto_publish_initial_pr_enabled;
        let auto_publish_paused_pr_autofix_enabled =
            workspace.auto_publish_paused_pr_autofix_enabled;
        let auto_publish_paused_pr_auto_merge_desired =
            workspace.auto_publish_paused_pr_auto_merge_desired;
        let pr_autofix_enabled = workspace.pr_autofix_enabled;
        let pr_auto_merge_desired = workspace.pr_auto_merge_desired;
        let pr_auto_merge_method = workspace.pr_auto_merge_method.clone();
        let pr_auto_merge_current = workspace.pr_auto_merge_current;
        let pr_supervision_status = workspace.pr_supervision_status.clone();
        let pr_supervision_summary = workspace.pr_supervision_summary.clone();
        let pr_supervision_updated_at = workspace
            .pr_supervision_updated_at
            .map(|value| value.to_rfc3339());
        let status = workspace.status.to_string();
        let created_at = workspace.created_at.to_rfc3339();
        let updated_at = Utc::now().to_rfc3339();
        let fetch_id = workspace.conversation_id;

        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO agent_conversation_workspaces (
                        conversation_id, project_id, mode, base_ref_kind, base_ref,
                        base_display_name, base_commit, branch_name, worktree_path,
                        linked_ideation_session_id, linked_plan_branch_id,
                        source_pr_number, source_pr_url, source_pr_title,
                        source_pr_head_ref, source_pr_base_ref, source_pr_head_sha,
                        publication_pr_number, publication_pr_url, publication_pr_status,
                        publication_push_status, auto_publish_enabled,
                        auto_publish_initial_pr_enabled, auto_publish_paused_pr_autofix_enabled,
                        auto_publish_paused_pr_auto_merge_desired, pr_autofix_enabled,
                        pr_auto_merge_desired, pr_auto_merge_method,
                        pr_auto_merge_current, pr_supervision_status,
                        pr_supervision_summary, pr_supervision_updated_at, status,
                        created_at, updated_at
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30, ?31, ?32, ?33, ?34, ?35)
                    ON CONFLICT(conversation_id) DO UPDATE SET
                        project_id=excluded.project_id,
                        mode=excluded.mode,
                        base_ref_kind=excluded.base_ref_kind,
                        base_ref=excluded.base_ref,
                        base_display_name=excluded.base_display_name,
                        base_commit=excluded.base_commit,
                        branch_name=excluded.branch_name,
                        worktree_path=excluded.worktree_path,
                        linked_ideation_session_id=excluded.linked_ideation_session_id,
                        linked_plan_branch_id=excluded.linked_plan_branch_id,
                        source_pr_number=excluded.source_pr_number,
                        source_pr_url=excluded.source_pr_url,
                        source_pr_title=excluded.source_pr_title,
                        source_pr_head_ref=excluded.source_pr_head_ref,
                        source_pr_base_ref=excluded.source_pr_base_ref,
                        source_pr_head_sha=excluded.source_pr_head_sha,
                        publication_pr_number=excluded.publication_pr_number,
                        publication_pr_url=excluded.publication_pr_url,
                        publication_pr_status=excluded.publication_pr_status,
                        publication_push_status=excluded.publication_push_status,
                        auto_publish_enabled=excluded.auto_publish_enabled,
                        auto_publish_initial_pr_enabled=excluded.auto_publish_initial_pr_enabled,
                        auto_publish_paused_pr_autofix_enabled=excluded.auto_publish_paused_pr_autofix_enabled,
                        auto_publish_paused_pr_auto_merge_desired=excluded.auto_publish_paused_pr_auto_merge_desired,
                        pr_autofix_enabled=excluded.pr_autofix_enabled,
                        pr_auto_merge_desired=excluded.pr_auto_merge_desired,
                        pr_auto_merge_method=excluded.pr_auto_merge_method,
                        pr_auto_merge_current=excluded.pr_auto_merge_current,
                        pr_supervision_status=excluded.pr_supervision_status,
                        pr_supervision_summary=excluded.pr_supervision_summary,
                        pr_supervision_updated_at=excluded.pr_supervision_updated_at,
                        status=excluded.status,
                        updated_at=excluded.updated_at",
                    rusqlite::params![
                        conversation_id,
                        project_id,
                        mode,
                        base_ref_kind,
                        base_ref,
                        base_display_name,
                        base_commit,
                        branch_name,
                        worktree_path,
                        linked_ideation_session_id,
                        linked_plan_branch_id,
                        source_pr_number,
                        source_pr_url,
                        source_pr_title,
                        source_pr_head_ref,
                        source_pr_base_ref,
                        source_pr_head_sha,
                        publication_pr_number,
                        publication_pr_url,
                        publication_pr_status,
                        publication_push_status,
                        auto_publish_enabled,
                        auto_publish_initial_pr_enabled,
                        auto_publish_paused_pr_autofix_enabled,
                        auto_publish_paused_pr_auto_merge_desired,
                        pr_autofix_enabled,
                        pr_auto_merge_desired,
                        pr_auto_merge_method,
                        pr_auto_merge_current,
                        pr_supervision_status,
                        pr_supervision_summary,
                        pr_supervision_updated_at,
                        status,
                        created_at,
                        updated_at,
                    ],
                )?;
                Ok(())
            })
            .await?;

        self.get_by_conversation_id(&fetch_id)
            .await?
            .ok_or_else(|| {
                AppError::Database("Failed to load saved agent conversation workspace".to_string())
            })
    }

    async fn get_by_conversation_id(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentConversationWorkspace>> {
        let conversation_id = conversation_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT * FROM agent_conversation_workspaces WHERE conversation_id = ?1",
                )?;
                let mut rows = stmt.query(rusqlite::params![conversation_id])?;
                if let Some(row) = rows.next()? {
                    Ok(Some(row_to_workspace(row)?))
                } else {
                    Ok(None)
                }
            })
            .await
    }

    async fn get_by_project_id(
        &self,
        project_id: &ProjectId,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        let project_id = project_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT * FROM agent_conversation_workspaces
                     WHERE project_id = ?1
                     ORDER BY created_at DESC",
                )?;
                let rows = stmt.query_map(rusqlite::params![project_id], row_to_workspace)?;
                let mut workspaces = Vec::new();
                for row in rows {
                    workspaces.push(row?);
                }
                Ok(workspaces)
            })
            .await
    }

    async fn get_by_linked_ideation_session_id(
        &self,
        ideation_session_id: &IdeationSessionId,
    ) -> AppResult<Option<AgentConversationWorkspace>> {
        let ideation_session_id = ideation_session_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT * FROM agent_conversation_workspaces
                     WHERE linked_ideation_session_id = ?1
                     ORDER BY updated_at DESC
                     LIMIT 1",
                )?;
                let mut rows = stmt.query(rusqlite::params![ideation_session_id])?;
                if let Some(row) = rows.next()? {
                    Ok(Some(row_to_workspace(row)?))
                } else {
                    Ok(None)
                }
            })
            .await
    }

    async fn get_terminal_local_cleanup_candidates_by_project_id(
        &self,
        project_id: &ProjectId,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        let project_id = project_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT * FROM agent_conversation_workspaces
                     WHERE project_id = ?1
                       AND (
                         publication_pr_status IN ('closed', 'merged')
                         OR status = 'archived'
                       )
                       AND (
                         local_cleanup_status IS NULL
                         OR (
                           local_cleanup_status IN ('unsafe', 'target_ref_missing')
                           AND local_cleanup_checked_at IS NOT NULL
                           AND local_cleanup_checked_at < strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now', '-24 hours')
                         )
                       )
                     ORDER BY created_at DESC",
                )?;
                let rows = stmt.query_map(rusqlite::params![project_id], row_to_workspace)?;
                let mut workspaces = Vec::new();
                for row in rows {
                    workspaces.push(row?);
                }
                Ok(workspaces)
            })
            .await
    }

    async fn mark_local_cleanup_status(
        &self,
        conversation_id: &ChatConversationId,
        status: &str,
        checked_at: DateTime<Utc>,
    ) -> AppResult<()> {
        let conversation_id = conversation_id.as_str().to_string();
        let status = status.to_string();
        let checked_at = checked_at.to_rfc3339();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE agent_conversation_workspaces
                     SET local_cleanup_status = ?1, local_cleanup_checked_at = ?2,
                         updated_at = ?2
                     WHERE conversation_id = ?3",
                    rusqlite::params![status, checked_at, conversation_id],
                )?;
                Ok(())
            })
            .await
    }

    async fn list_worktree_paths_by_project_id(
        &self,
        project_id: &ProjectId,
    ) -> AppResult<std::collections::HashSet<String>> {
        let project_id = project_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT worktree_path FROM agent_conversation_workspaces
                     WHERE project_id = ?1",
                )?;
                let rows =
                    stmt.query_map(rusqlite::params![project_id], |row| row.get::<_, String>(0))?;
                let mut paths = std::collections::HashSet::new();
                for row in rows {
                    paths.insert(row?);
                }
                Ok(paths)
            })
            .await
    }

    async fn list_active_direct_published_workspaces(
        &self,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT * FROM agent_conversation_workspaces
                     WHERE status = 'active'
                       AND mode = 'edit'
                       AND linked_plan_branch_id IS NULL
                       AND publication_pr_number IS NOT NULL
                       AND auto_publish_enabled = 1
                       AND COALESCE(publication_push_status, 'pushed') IN ('pushed', 'refreshed')
                       AND COALESCE(publication_pr_status, '') NOT IN ('closed', 'merged')
                     ORDER BY updated_at DESC",
                )?;
                let rows = stmt.query_map([], row_to_workspace)?;
                let mut workspaces = Vec::new();
                for row in rows {
                    workspaces.push(row?);
                }
                Ok(workspaces)
            })
            .await
    }

    async fn list_active_pr_poller_recovery_workspaces(
        &self,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT * FROM agent_conversation_workspaces
                     WHERE status = 'active'
                       AND publication_pr_number IS NOT NULL
                       AND auto_publish_enabled = 1
                       AND COALESCE(publication_push_status, 'pushed') IN ('pushed', 'refreshed')
                       AND COALESCE(publication_pr_status, '') NOT IN ('closed', 'merged')
                       AND (
                         (mode = 'edit' AND linked_plan_branch_id IS NULL)
                         OR (
                           mode = 'ideation'
                           AND linked_plan_branch_id IS NOT NULL
                           AND (pr_autofix_enabled = 1 OR pr_auto_merge_desired = 1)
                         )
                       )
                     ORDER BY updated_at DESC",
                )?;
                let rows = stmt.query_map([], row_to_workspace)?;
                let mut workspaces = Vec::new();
                for row in rows {
                    workspaces.push(row?);
                }
                Ok(workspaces)
            })
            .await
    }

    async fn list_active_needs_agent_workspaces(
        &self,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT * FROM agent_conversation_workspaces
                     WHERE status = 'active'
                       AND publication_push_status = 'needs_agent'
                       AND COALESCE(publication_pr_status, '') NOT IN ('closed', 'merged')
                     ORDER BY updated_at DESC",
                )?;
                let rows = stmt.query_map([], row_to_workspace)?;
                let mut workspaces = Vec::new();
                for row in rows {
                    workspaces.push(row?);
                }
                Ok(workspaces)
            })
            .await
    }

    async fn list_active_transient_publish_status_workspaces(
        &self,
        stale_older_than_secs: u64,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        let cutoff = (chrono::Utc::now()
            - chrono::Duration::seconds(stale_older_than_secs as i64))
            .format("%Y-%m-%dT%H:%M:%S+00:00")
            .to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT * FROM agent_conversation_workspaces
                     WHERE status = 'active'
                       AND publication_push_status IN ('refreshing', 'checking', 'committing', 'describing')
                       AND COALESCE(publication_pr_status, '') NOT IN ('closed', 'merged')
                       AND updated_at <= ?1
                     ORDER BY updated_at ASC",
                )?;
                let rows = stmt.query_map([cutoff], row_to_workspace)?;
                let mut workspaces = Vec::new();
                for row in rows {
                    workspaces.push(row?);
                }
                Ok(workspaces)
            })
            .await
    }

    async fn list_active_direct_external_pr_reconciliation_candidates(
        &self,
        limit: usize,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT * FROM agent_conversation_workspaces
                     WHERE mode = 'edit'
                       AND linked_plan_branch_id IS NULL
                       AND COALESCE(publication_pr_status, '') NOT IN ('closed', 'merged')
                       AND (
                         (
                           status = 'active'
                           AND publication_pr_number IS NULL
                           AND COALESCE(publication_push_status, 'pushed') NOT IN (
                               'needs_agent', 'pending', 'failed', 'description_failed'
                           )
                         )
                         OR (
                           status IN ('active', 'missing')
                           AND publication_pr_number IS NOT NULL
                         )
                       )
                     ORDER BY updated_at DESC
                     LIMIT ?1",
                )?;
                let rows = stmt.query_map(rusqlite::params![limit], row_to_workspace)?;
                let mut workspaces = Vec::new();
                for row in rows {
                    workspaces.push(row?);
                }
                Ok(workspaces)
            })
            .await
    }

    async fn list_active_direct_pr_supervision_recovery_candidates(
        &self,
        limit: usize,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT * FROM agent_conversation_workspaces
                     WHERE status = 'active'
                       AND mode = 'edit'
                       AND linked_plan_branch_id IS NULL
                       AND publication_pr_number IS NOT NULL
                       AND publication_push_status = 'failed'
                       AND pr_supervision_status = 'blocked'
                       AND auto_publish_enabled = 1
                       AND (pr_autofix_enabled = 1 OR pr_auto_merge_desired = 1)
                       AND COALESCE(publication_pr_status, '') NOT IN ('closed', 'merged')
                     ORDER BY updated_at DESC
                     LIMIT ?1",
                )?;
                let rows = stmt.query_map(rusqlite::params![limit], row_to_workspace)?;
                let mut workspaces = Vec::new();
                for row in rows {
                    workspaces.push(row?);
                }
                Ok(workspaces)
            })
            .await
    }

    async fn update_links(
        &self,
        conversation_id: &ChatConversationId,
        ideation_session_id: Option<&IdeationSessionId>,
        plan_branch_id: Option<&PlanBranchId>,
    ) -> AppResult<()> {
        let conversation_id = conversation_id.as_str().to_string();
        let ideation_session_id = ideation_session_id.map(|id| id.as_str().to_string());
        let plan_branch_id = plan_branch_id.map(|id| id.as_str().to_string());
        let updated_at = Utc::now().to_rfc3339();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE agent_conversation_workspaces
                     SET linked_ideation_session_id = ?2,
                         linked_plan_branch_id = ?3,
                         updated_at = ?4
                     WHERE conversation_id = ?1",
                    rusqlite::params![
                        conversation_id,
                        ideation_session_id,
                        plan_branch_id,
                        updated_at
                    ],
                )?;
                Ok(())
            })
            .await
    }

    async fn update_publication(
        &self,
        conversation_id: &ChatConversationId,
        pr_number: Option<i64>,
        pr_url: Option<&str>,
        pr_status: Option<&str>,
        push_status: Option<&str>,
    ) -> AppResult<()> {
        let conversation_id = conversation_id.as_str().to_string();
        let pr_url = pr_url.map(str::to_string);
        let pr_status = pr_status.map(str::to_string);
        let push_status = push_status.map(str::to_string);
        let terminal_pr_status = matches!(pr_status.as_deref(), Some("merged" | "closed"));
        let updated_at = Utc::now().to_rfc3339();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE agent_conversation_workspaces
                     SET publication_pr_number = ?2,
                         publication_pr_url = ?3,
                         publication_pr_status = ?4,
                         publication_push_status = ?5,
                         pr_supervision_status = CASE WHEN ?7 THEN NULL ELSE pr_supervision_status END,
                         pr_supervision_summary = CASE WHEN ?7 THEN NULL ELSE pr_supervision_summary END,
                         pr_supervision_updated_at = CASE WHEN ?7 THEN ?6 ELSE pr_supervision_updated_at END,
                         updated_at = ?6
                     WHERE conversation_id = ?1",
                    rusqlite::params![
                        conversation_id,
                        pr_number,
                        pr_url,
                        pr_status,
                        push_status,
                        updated_at,
                        terminal_pr_status
                    ],
                )?;
                Ok(())
            })
            .await
    }

    async fn update_pr_supervision_preferences(
        &self,
        conversation_id: &ChatConversationId,
        autofix_enabled: bool,
        auto_merge_desired: bool,
        auto_merge_method: &str,
    ) -> AppResult<()> {
        let conversation_id = conversation_id.as_str().to_string();
        let auto_merge_method = auto_merge_method.trim().to_string();
        let auto_merge_method = if auto_merge_method.is_empty() {
            DEFAULT_AGENT_WORKSPACE_PR_AUTO_MERGE_METHOD.to_string()
        } else {
            auto_merge_method
        };
        let supervision_status = if autofix_enabled || auto_merge_desired {
            Some("monitoring".to_string())
        } else {
            Some("disabled".to_string())
        };
        let supervision_summary = if autofix_enabled || auto_merge_desired {
            Some("RalphX PR supervision is enabled.".to_string())
        } else {
            None
        };
        let now = Utc::now().to_rfc3339();
        let supervision_updated_at = now.clone();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE agent_conversation_workspaces
                     SET pr_autofix_enabled = ?2,
                         pr_auto_merge_desired = ?3,
                         pr_auto_merge_method = ?4,
                         pr_supervision_status = ?5,
                         pr_supervision_summary = ?6,
                         pr_supervision_updated_at = ?7,
                         updated_at = ?8
                     WHERE conversation_id = ?1",
                    rusqlite::params![
                        conversation_id,
                        autofix_enabled,
                        auto_merge_desired,
                        auto_merge_method,
                        supervision_status,
                        supervision_summary,
                        supervision_updated_at,
                        now
                    ],
                )?;
                Ok(())
            })
            .await
    }

    async fn update_pr_auto_merge_state(
        &self,
        conversation_id: &ChatConversationId,
        auto_merge_current: Option<bool>,
        status: Option<&str>,
        summary: Option<&str>,
    ) -> AppResult<()> {
        let conversation_id = conversation_id.as_str().to_string();
        let status = status.map(str::to_string);
        let summary = summary.map(str::to_string);
        let now = Utc::now().to_rfc3339();
        let supervision_updated_at = now.clone();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE agent_conversation_workspaces
                     SET pr_auto_merge_current = ?2,
                         pr_supervision_status = COALESCE(?3, pr_supervision_status),
                         pr_supervision_summary = COALESCE(?4, pr_supervision_summary),
                         pr_supervision_updated_at = ?5,
                         updated_at = ?6
                     WHERE conversation_id = ?1",
                    rusqlite::params![
                        conversation_id,
                        auto_merge_current,
                        status,
                        summary,
                        supervision_updated_at,
                        now
                    ],
                )?;
                Ok(())
            })
            .await
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
        let conversation_id = conversation_id.as_str().to_string();
        let pr_supervision_status = pr_supervision_status.map(str::to_string);
        let pr_supervision_summary = pr_supervision_summary.map(str::to_string);
        let now = Utc::now().to_rfc3339();
        let supervision_updated_at = now.clone();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE agent_conversation_workspaces
                     SET auto_publish_enabled = ?2,
                         auto_publish_paused_pr_autofix_enabled = ?3,
                         auto_publish_paused_pr_auto_merge_desired = ?4,
                         pr_autofix_enabled = ?5,
                         pr_auto_merge_desired = ?6,
                         pr_supervision_status = ?7,
                         pr_supervision_summary = ?8,
                         pr_supervision_updated_at = ?9,
                         updated_at = ?10
                     WHERE conversation_id = ?1",
                    rusqlite::params![
                        conversation_id,
                        auto_publish_enabled,
                        paused_pr_autofix_enabled,
                        paused_pr_auto_merge_desired,
                        pr_autofix_enabled,
                        pr_auto_merge_desired,
                        pr_supervision_status,
                        pr_supervision_summary,
                        supervision_updated_at,
                        now
                    ],
                )?;
                Ok(())
            })
            .await
    }

    async fn update_auto_publish_initial_pr_preference(
        &self,
        conversation_id: &ChatConversationId,
        enabled: bool,
    ) -> AppResult<()> {
        let conversation_id = conversation_id.as_str().to_string();
        let now = Utc::now().to_rfc3339();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE agent_conversation_workspaces
                     SET auto_publish_initial_pr_enabled = ?2,
                         updated_at = ?3
                     WHERE conversation_id = ?1",
                    rusqlite::params![conversation_id, enabled, now],
                )?;
                Ok(())
            })
            .await
    }

    async fn update_status(
        &self,
        conversation_id: &ChatConversationId,
        status: AgentConversationWorkspaceStatus,
    ) -> AppResult<()> {
        let conversation_id = conversation_id.as_str().to_string();
        let status = status.to_string();
        let updated_at = Utc::now().to_rfc3339();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE agent_conversation_workspaces
                     SET status = ?2, updated_at = ?3
                     WHERE conversation_id = ?1",
                    rusqlite::params![conversation_id, status, updated_at],
                )?;
                Ok(())
            })
            .await
    }

    async fn save_pr_description(
        &self,
        conversation_id: &ChatConversationId,
        description: AgentWorkspacePrDescription,
    ) -> AppResult<()> {
        let conversation_id = conversation_id.as_str().to_string();
        let title = description.title;
        let body_markdown = description.body_markdown;
        let updated_at = Utc::now().to_rfc3339();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE agent_conversation_workspaces
                     SET publication_pr_title = ?2,
                         publication_pr_body = ?3,
                         updated_at = ?4
                     WHERE conversation_id = ?1",
                    rusqlite::params![conversation_id, title, body_markdown, updated_at],
                )?;
                Ok(())
            })
            .await
    }

    async fn get_pr_description(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentWorkspacePrDescription>> {
        let conversation_id = conversation_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT publication_pr_title, publication_pr_body
                     FROM agent_conversation_workspaces
                     WHERE conversation_id = ?1",
                )?;
                let mut rows = stmt.query(rusqlite::params![conversation_id])?;
                let Some(row) = rows.next()? else {
                    return Ok(None);
                };
                let body_markdown: Option<String> = row.get(1)?;
                let title: Option<String> = row.get(0)?;
                Ok(body_markdown.map(|body| AgentWorkspacePrDescription {
                    title,
                    body_markdown: body,
                }))
            })
            .await
    }

    async fn clear_pr_description(&self, conversation_id: &ChatConversationId) -> AppResult<()> {
        let conversation_id = conversation_id.as_str().to_string();
        let updated_at = Utc::now().to_rfc3339();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE agent_conversation_workspaces
                     SET publication_pr_title = NULL,
                         publication_pr_body = NULL,
                         updated_at = ?2
                     WHERE conversation_id = ?1",
                    rusqlite::params![conversation_id, updated_at],
                )?;
                Ok(())
            })
            .await
    }

    async fn append_publication_event(
        &self,
        event: AgentConversationWorkspacePublicationEvent,
    ) -> AppResult<()> {
        let id = event.id;
        let conversation_id = event.conversation_id.as_str().to_string();
        let step = event.step;
        let status = event.status;
        let summary = event.summary;
        let classification = event.classification;
        let created_at = event.created_at.to_rfc3339();
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO agent_conversation_workspace_publication_events (
                        id, conversation_id, step, status, summary, classification, created_at
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    rusqlite::params![
                        id,
                        conversation_id,
                        step,
                        status,
                        summary,
                        classification,
                        created_at
                    ],
                )?;
                Ok(())
            })
            .await
    }

    async fn list_publication_events(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Vec<AgentConversationWorkspacePublicationEvent>> {
        let conversation_id = conversation_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT * FROM agent_conversation_workspace_publication_events
                     WHERE conversation_id = ?1
                     ORDER BY created_at ASC, rowid ASC",
                )?;
                let rows =
                    stmt.query_map(rusqlite::params![conversation_id], row_to_publication_event)?;
                let mut events = Vec::new();
                for row in rows {
                    events.push(row?);
                }
                Ok(events)
            })
            .await
    }

    async fn upsert_pr_comment_evidence(
        &self,
        conversation_id: &ChatConversationId,
        comments: Vec<AgentWorkspacePrCommentEvidenceUpsert>,
    ) -> AppResult<()> {
        if comments.is_empty() {
            return Ok(());
        }
        let conversation_id = conversation_id.as_str().to_string();
        let now = Utc::now().to_rfc3339();
        self.db
            .run(move |conn| {
                for comment in comments {
                    conn.execute(
                        "INSERT INTO agent_workspace_pr_comment_evidence (
                            conversation_id, pr_number, comment_id, author, body,
                            body_excerpt, body_sha256, url, github_created_at,
                            github_updated_at, is_codecov, is_bot, first_seen_at,
                            last_seen_at, edit_count
                         ) VALUES (
                            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
                            ?13, ?13, 0
                         )
                         ON CONFLICT(conversation_id, pr_number, comment_id) DO UPDATE SET
                            author = excluded.author,
                            body = excluded.body,
                            body_excerpt = excluded.body_excerpt,
                            body_sha256 = excluded.body_sha256,
                            url = excluded.url,
                            github_created_at = excluded.github_created_at,
                            github_updated_at = excluded.github_updated_at,
                            is_codecov = excluded.is_codecov,
                            is_bot = excluded.is_bot,
                            last_seen_at = excluded.last_seen_at,
                            edit_count = CASE
                                WHEN agent_workspace_pr_comment_evidence.body_sha256 != excluded.body_sha256
                                THEN agent_workspace_pr_comment_evidence.edit_count + 1
                                ELSE agent_workspace_pr_comment_evidence.edit_count
                            END",
                        rusqlite::params![
                            conversation_id.as_str(),
                            comment.pr_number,
                            comment.comment_id,
                            comment.author,
                            comment.body,
                            comment.body_excerpt,
                            comment.body_sha256,
                            comment.url,
                            comment.github_created_at,
                            comment.github_updated_at,
                            comment.is_codecov,
                            comment.is_bot,
                            now.as_str(),
                        ],
                    )?;
                }
                Ok(())
            })
            .await
    }

    async fn list_pr_comment_evidence(
        &self,
        conversation_id: &ChatConversationId,
        pr_number: i64,
        limit: usize,
    ) -> AppResult<Vec<AgentWorkspacePrCommentEvidence>> {
        let conversation_id = conversation_id.as_str().to_string();
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT * FROM agent_workspace_pr_comment_evidence
                     WHERE conversation_id = ?1 AND pr_number = ?2
                     ORDER BY
                        COALESCE(github_updated_at, github_created_at, last_seen_at) DESC,
                        comment_id DESC
                     LIMIT ?3",
                )?;
                let rows = stmt.query_map(
                    rusqlite::params![conversation_id, pr_number, limit],
                    row_to_pr_comment_evidence,
                )?;
                let mut comments = Vec::new();
                for row in rows {
                    comments.push(row?);
                }
                Ok(comments)
            })
            .await
    }

    async fn get_pr_comment_evidence(
        &self,
        conversation_id: &ChatConversationId,
        pr_number: i64,
        comment_id: &str,
    ) -> AppResult<Option<AgentWorkspacePrCommentEvidence>> {
        let conversation_id = conversation_id.as_str().to_string();
        let comment_id = comment_id.to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT * FROM agent_workspace_pr_comment_evidence
                     WHERE conversation_id = ?1 AND pr_number = ?2 AND comment_id = ?3",
                )?;
                let mut rows =
                    stmt.query(rusqlite::params![conversation_id, pr_number, comment_id])?;
                if let Some(row) = rows.next()? {
                    Ok(Some(row_to_pr_comment_evidence(row)?))
                } else {
                    Ok(None)
                }
            })
            .await
    }

    async fn mark_pr_comments_included(
        &self,
        conversation_id: &ChatConversationId,
        pr_number: i64,
        comment_ids: &[String],
    ) -> AppResult<()> {
        if comment_ids.is_empty() {
            return Ok(());
        }
        let conversation_id = conversation_id.as_str().to_string();
        let comment_ids = comment_ids.to_vec();
        let now = Utc::now().to_rfc3339();
        self.db
            .run(move |conn| {
                for comment_id in comment_ids {
                    conn.execute(
                        "UPDATE agent_workspace_pr_comment_evidence
                         SET last_included_at = ?4
                         WHERE conversation_id = ?1 AND pr_number = ?2 AND comment_id = ?3",
                        rusqlite::params![
                            conversation_id.as_str(),
                            pr_number,
                            comment_id,
                            now.as_str()
                        ],
                    )?;
                }
                Ok(())
            })
            .await
    }

    async fn mark_pr_comment_read(
        &self,
        conversation_id: &ChatConversationId,
        pr_number: i64,
        comment_id: &str,
    ) -> AppResult<()> {
        let conversation_id = conversation_id.as_str().to_string();
        let comment_id = comment_id.to_string();
        let now = Utc::now().to_rfc3339();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE agent_workspace_pr_comment_evidence
                     SET last_read_at = ?4
                     WHERE conversation_id = ?1 AND pr_number = ?2 AND comment_id = ?3",
                    rusqlite::params![conversation_id, pr_number, comment_id, now],
                )?;
                Ok(())
            })
            .await
    }

    async fn upsert_pr_review_monitor(
        &self,
        monitor: AgentWorkspacePrReviewMonitor,
    ) -> AppResult<AgentWorkspacePrReviewMonitor> {
        let conversation_id = monitor.conversation_id.as_str().to_string();
        let project_id = monitor.project_id.as_str().to_string();
        let pr_number = monitor.pr_number;
        let status = monitor.status.to_string();
        let monitor_enabled = monitor.monitor_enabled;
        let first_review_completed = monitor.first_review_completed;
        let last_seen_head_sha = monitor.last_seen_head_sha.clone();
        let last_reviewed_head_sha = monitor.last_reviewed_head_sha.clone();
        let last_review_run_id = monitor.last_review_run_id.clone();
        let last_review_outcome = monitor.last_review_outcome.clone();
        let last_submitted_review_id = monitor.last_submitted_review_id.clone();
        let review_artifact_id = monitor
            .review_artifact_id
            .as_ref()
            .map(|id| id.as_str().to_string());
        let review_artifact_head_sha = monitor.review_artifact_head_sha.clone();
        let review_artifact_version = monitor.review_artifact_version.map(i64::from);
        let review_artifact_updated_at = monitor
            .review_artifact_updated_at
            .map(|value| value.to_rfc3339());
        let last_error = monitor.last_error.clone();
        let created_at = monitor.created_at.to_rfc3339();
        let updated_at = Utc::now().to_rfc3339();
        let fetch_id = monitor.conversation_id;

        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO agent_workspace_pr_review_monitors (
                        conversation_id, project_id, pr_number, status, monitor_enabled,
                        first_review_completed, last_seen_head_sha, last_reviewed_head_sha,
                        last_review_run_id, last_review_outcome, last_submitted_review_id,
                        review_artifact_id, review_artifact_head_sha, review_artifact_version,
                        review_artifact_updated_at, last_error, created_at, updated_at
                    ) VALUES (
                        ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18
                    )
                    ON CONFLICT(conversation_id) DO UPDATE SET
                        project_id = excluded.project_id,
                        pr_number = excluded.pr_number,
                        status = excluded.status,
                        monitor_enabled = excluded.monitor_enabled,
                        first_review_completed = excluded.first_review_completed,
                        last_seen_head_sha = excluded.last_seen_head_sha,
                        last_reviewed_head_sha = excluded.last_reviewed_head_sha,
                        last_review_run_id = excluded.last_review_run_id,
                        last_review_outcome = excluded.last_review_outcome,
                        last_submitted_review_id = excluded.last_submitted_review_id,
                        review_artifact_id = COALESCE(excluded.review_artifact_id, agent_workspace_pr_review_monitors.review_artifact_id),
                        review_artifact_head_sha = COALESCE(excluded.review_artifact_head_sha, agent_workspace_pr_review_monitors.review_artifact_head_sha),
                        review_artifact_version = COALESCE(excluded.review_artifact_version, agent_workspace_pr_review_monitors.review_artifact_version),
                        review_artifact_updated_at = COALESCE(excluded.review_artifact_updated_at, agent_workspace_pr_review_monitors.review_artifact_updated_at),
                        last_error = excluded.last_error,
                        updated_at = excluded.updated_at",
                    rusqlite::params![
                        conversation_id,
                        project_id,
                        pr_number,
                        status,
                        monitor_enabled,
                        first_review_completed,
                        last_seen_head_sha,
                        last_reviewed_head_sha,
                        last_review_run_id,
                        last_review_outcome,
                        last_submitted_review_id,
                        review_artifact_id,
                        review_artifact_head_sha,
                        review_artifact_version,
                        review_artifact_updated_at,
                        last_error,
                        created_at,
                        updated_at,
                    ],
                )?;
                Ok(())
            })
            .await?;

        self.get_pr_review_monitor(&fetch_id)
            .await?
            .ok_or_else(|| AppError::Database("Failed to load saved PR review monitor".to_string()))
    }

    async fn get_pr_review_monitor(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentWorkspacePrReviewMonitor>> {
        let conversation_id = conversation_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT * FROM agent_workspace_pr_review_monitors
                     WHERE conversation_id = ?1",
                )?;
                let mut rows = stmt.query(rusqlite::params![conversation_id])?;
                if let Some(row) = rows.next()? {
                    Ok(Some(row_to_pr_review_monitor(row)?))
                } else {
                    Ok(None)
                }
            })
            .await
    }

    async fn list_active_pr_review_monitors(
        &self,
    ) -> AppResult<Vec<AgentWorkspacePrReviewMonitor>> {
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT * FROM agent_workspace_pr_review_monitors
                     WHERE monitor_enabled = 1
                       AND status != 'terminal'
                     ORDER BY updated_at DESC",
                )?;
                let rows = stmt.query_map([], row_to_pr_review_monitor)?;
                let mut monitors = Vec::new();
                for row in rows {
                    monitors.push(row?);
                }
                Ok(monitors)
            })
            .await
    }

    async fn upsert_workspace_review_monitor(
        &self,
        monitor: AgentWorkspaceReviewMonitor,
    ) -> AppResult<AgentWorkspaceReviewMonitor> {
        let conversation_id = monitor.conversation_id.as_str().to_string();
        let project_id = monitor.project_id.as_str().to_string();
        let status = monitor.status.to_string();
        let current_target_scope = monitor.current_target_scope.map(|scope| scope.to_string());
        let reviewed_target_scope = monitor.reviewed_target_scope.map(|scope| scope.to_string());
        let review_artifact_id = monitor
            .review_artifact_id
            .as_ref()
            .map(|id| id.as_str().to_string());
        let review_artifact_version = monitor.review_artifact_version.map(i64::from);
        let review_artifact_updated_at = monitor
            .review_artifact_updated_at
            .map(|value| value.to_rfc3339());
        let reviewed_head_sha = monitor.reviewed_head_sha;
        let reviewed_diff_fingerprint = monitor.reviewed_diff_fingerprint;
        let selected_source_base_ref = monitor.selected_source_base_ref;
        let selected_source_base_sha = monitor.selected_source_base_sha;
        let selected_source_head_ref = monitor.selected_source_head_ref;
        let selected_source_head_sha = monitor.selected_source_head_sha;
        let selected_source_pull_request_number = monitor.selected_source_pull_request_number;
        let workspace_base_ref = monitor.workspace_base_ref;
        let workspace_base_sha = monitor.workspace_base_sha;
        let workspace_head_ref = monitor.workspace_head_ref;
        let workspace_head_sha = monitor.workspace_head_sha;
        let current_diff_fingerprint = monitor.current_diff_fingerprint;
        let previous_version_id = monitor
            .previous_version_id
            .as_ref()
            .map(|id| id.as_str().to_string());
        let last_run_id = monitor.last_run_id;
        let last_error = monitor.last_error;
        let created_at = monitor.created_at.to_rfc3339();
        let updated_at = Utc::now().to_rfc3339();
        let fetch_id = monitor.conversation_id;

        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO agent_workspace_review_monitors (
                        conversation_id, project_id, status, current_target_scope,
                        reviewed_target_scope, review_artifact_id,
                        review_artifact_version, review_artifact_updated_at,
                        reviewed_head_sha, reviewed_diff_fingerprint,
                        selected_source_base_ref, selected_source_base_sha,
                        selected_source_head_ref, selected_source_head_sha,
                        selected_source_pull_request_number, workspace_base_ref,
                        workspace_base_sha, workspace_head_ref, workspace_head_sha,
                        current_diff_fingerprint, previous_version_id, last_run_id,
                        last_error, created_at, updated_at
                    ) VALUES (
                        ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                        ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25
                    )
                    ON CONFLICT(conversation_id) DO UPDATE SET
                        project_id = excluded.project_id,
                        status = excluded.status,
                        current_target_scope = excluded.current_target_scope,
                        reviewed_target_scope = excluded.reviewed_target_scope,
                        review_artifact_id = COALESCE(excluded.review_artifact_id, agent_workspace_review_monitors.review_artifact_id),
                        review_artifact_version = COALESCE(excluded.review_artifact_version, agent_workspace_review_monitors.review_artifact_version),
                        review_artifact_updated_at = COALESCE(excluded.review_artifact_updated_at, agent_workspace_review_monitors.review_artifact_updated_at),
                        reviewed_head_sha = excluded.reviewed_head_sha,
                        reviewed_diff_fingerprint = excluded.reviewed_diff_fingerprint,
                        selected_source_base_ref = excluded.selected_source_base_ref,
                        selected_source_base_sha = excluded.selected_source_base_sha,
                        selected_source_head_ref = excluded.selected_source_head_ref,
                        selected_source_head_sha = excluded.selected_source_head_sha,
                        selected_source_pull_request_number = excluded.selected_source_pull_request_number,
                        workspace_base_ref = excluded.workspace_base_ref,
                        workspace_base_sha = excluded.workspace_base_sha,
                        workspace_head_ref = excluded.workspace_head_ref,
                        workspace_head_sha = excluded.workspace_head_sha,
                        current_diff_fingerprint = excluded.current_diff_fingerprint,
                        previous_version_id = COALESCE(excluded.previous_version_id, agent_workspace_review_monitors.previous_version_id),
                        last_run_id = excluded.last_run_id,
                        last_error = excluded.last_error,
                        updated_at = excluded.updated_at",
                    rusqlite::params![
                        conversation_id,
                        project_id,
                        status,
                        current_target_scope,
                        reviewed_target_scope,
                        review_artifact_id,
                        review_artifact_version,
                        review_artifact_updated_at,
                        reviewed_head_sha,
                        reviewed_diff_fingerprint,
                        selected_source_base_ref,
                        selected_source_base_sha,
                        selected_source_head_ref,
                        selected_source_head_sha,
                        selected_source_pull_request_number,
                        workspace_base_ref,
                        workspace_base_sha,
                        workspace_head_ref,
                        workspace_head_sha,
                        current_diff_fingerprint,
                        previous_version_id,
                        last_run_id,
                        last_error,
                        created_at,
                        updated_at,
                    ],
                )?;
                Ok(())
            })
            .await?;

        self.get_workspace_review_monitor(&fetch_id)
            .await?
            .ok_or_else(|| {
                AppError::Database("Failed to load saved workspace review monitor".to_string())
            })
    }

    async fn get_workspace_review_monitor(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentWorkspaceReviewMonitor>> {
        let conversation_id = conversation_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT * FROM agent_workspace_review_monitors
                     WHERE conversation_id = ?1",
                )?;
                let mut rows = stmt.query(rusqlite::params![conversation_id])?;
                if let Some(row) = rows.next()? {
                    Ok(Some(row_to_workspace_review_monitor(row)?))
                } else {
                    Ok(None)
                }
            })
            .await
    }

    async fn create_or_update_pr_review_action(
        &self,
        action: AgentWorkspacePrReviewAction,
    ) -> AppResult<AgentWorkspacePrReviewAction> {
        let id = action.id;
        let conversation_id = action.conversation_id.as_str().to_string();
        let pr_number = action.pr_number;
        let head_sha = action.head_sha;
        let proposed_action = action.proposed_action.to_string();
        let summary = action.summary;
        let review_body = action.review_body;
        let findings_json = action.findings_json;
        let status = action.status.to_string();
        let submitted_review_id = action.submitted_review_id;
        let created_by_run_id = action.created_by_run_id;
        let created_at = action.created_at.to_rfc3339();
        let updated_at = Utc::now().to_rfc3339();

        self.db
            .run_transaction(move |conn| {
                let existing_id = conn
                    .query_row(
                        "SELECT id FROM agent_workspace_pr_review_actions
                         WHERE conversation_id = ?1
                           AND pr_number = ?2
                           AND head_sha = ?3
                           AND status = 'pending'
                         LIMIT 1",
                        rusqlite::params![conversation_id, pr_number, head_sha],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()?;
                let fetch_id = existing_id.unwrap_or_else(|| id.clone());

                if fetch_id == id {
                    conn.execute(
                        "INSERT INTO agent_workspace_pr_review_actions (
                            id, conversation_id, pr_number, head_sha, proposed_action,
                            summary, review_body, findings_json, status, submitted_review_id,
                            created_by_run_id, created_at, updated_at
                        ) VALUES (
                            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13
                        )",
                        rusqlite::params![
                            id,
                            conversation_id,
                            pr_number,
                            head_sha,
                            proposed_action,
                            summary,
                            review_body,
                            findings_json,
                            status,
                            submitted_review_id,
                            created_by_run_id,
                            created_at,
                            updated_at,
                        ],
                    )?;
                } else {
                    conn.execute(
                        "UPDATE agent_workspace_pr_review_actions
                         SET proposed_action = ?2,
                             summary = ?3,
                             review_body = ?4,
                             findings_json = ?5,
                             submitted_review_id = ?6,
                             created_by_run_id = ?7,
                             updated_at = ?8
                         WHERE id = ?1",
                        rusqlite::params![
                            fetch_id,
                            proposed_action,
                            summary,
                            review_body,
                            findings_json,
                            submitted_review_id,
                            created_by_run_id,
                            updated_at,
                        ],
                    )?;
                }

                let mut stmt =
                    conn.prepare("SELECT * FROM agent_workspace_pr_review_actions WHERE id = ?1")?;
                let action =
                    stmt.query_row(rusqlite::params![fetch_id], row_to_pr_review_action)?;
                Ok(action)
            })
            .await
    }

    async fn get_pr_review_action(
        &self,
        action_id: &str,
    ) -> AppResult<Option<AgentWorkspacePrReviewAction>> {
        let action_id = action_id.to_string();
        self.db
            .run(move |conn| {
                let mut stmt =
                    conn.prepare("SELECT * FROM agent_workspace_pr_review_actions WHERE id = ?1")?;
                let mut rows = stmt.query(rusqlite::params![action_id])?;
                if let Some(row) = rows.next()? {
                    Ok(Some(row_to_pr_review_action(row)?))
                } else {
                    Ok(None)
                }
            })
            .await
    }

    async fn get_pending_pr_review_action_for_head(
        &self,
        conversation_id: &ChatConversationId,
        pr_number: i64,
        head_sha: &str,
    ) -> AppResult<Option<AgentWorkspacePrReviewAction>> {
        let conversation_id = conversation_id.as_str().to_string();
        let head_sha = head_sha.to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT * FROM agent_workspace_pr_review_actions
                     WHERE conversation_id = ?1
                       AND pr_number = ?2
                       AND head_sha = ?3
                       AND status = 'pending'
                     ORDER BY created_at DESC
                     LIMIT 1",
                )?;
                let mut rows =
                    stmt.query(rusqlite::params![conversation_id, pr_number, head_sha])?;
                if let Some(row) = rows.next()? {
                    Ok(Some(row_to_pr_review_action(row)?))
                } else {
                    Ok(None)
                }
            })
            .await
    }

    async fn list_pr_review_actions(
        &self,
        conversation_id: &ChatConversationId,
        limit: usize,
    ) -> AppResult<Vec<AgentWorkspacePrReviewAction>> {
        let conversation_id = conversation_id.as_str().to_string();
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT * FROM agent_workspace_pr_review_actions
                     WHERE conversation_id = ?1
                     ORDER BY created_at DESC
                     LIMIT ?2",
                )?;
                let rows = stmt.query_map(
                    rusqlite::params![conversation_id, limit],
                    row_to_pr_review_action,
                )?;
                let mut actions = Vec::new();
                for row in rows {
                    actions.push(row?);
                }
                Ok(actions)
            })
            .await
    }

    async fn update_pr_review_action_status(
        &self,
        action_id: &str,
        status: AgentWorkspacePrReviewActionStatus,
        submitted_review_id: Option<&str>,
    ) -> AppResult<()> {
        let action_id = action_id.to_string();
        let status_value = status.to_string();
        let submitted_review_id = submitted_review_id.map(str::to_string);
        let updated_at = Utc::now().to_rfc3339();
        let resolved_at = pr_review_action_terminal_status(status).then(|| updated_at.clone());
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE agent_workspace_pr_review_actions
                     SET status = ?2,
                         submitted_review_id = ?3,
                         updated_at = ?4,
                         resolved_at = ?5
                     WHERE id = ?1",
                    rusqlite::params![
                        action_id,
                        status_value,
                        submitted_review_id,
                        updated_at,
                        resolved_at
                    ],
                )?;
                Ok(())
            })
            .await
    }

    async fn delete(&self, conversation_id: &ChatConversationId) -> AppResult<()> {
        let conversation_id = conversation_id.as_str().to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "DELETE FROM agent_workspace_pr_comment_evidence WHERE conversation_id = ?1",
                    rusqlite::params![conversation_id.as_str()],
                )?;
                conn.execute(
                    "DELETE FROM agent_workspace_pr_review_actions WHERE conversation_id = ?1",
                    rusqlite::params![conversation_id.as_str()],
                )?;
                conn.execute(
                    "DELETE FROM agent_workspace_pr_review_monitors WHERE conversation_id = ?1",
                    rusqlite::params![conversation_id.as_str()],
                )?;
                conn.execute(
                    "DELETE FROM agent_workspace_review_monitors WHERE conversation_id = ?1",
                    rusqlite::params![conversation_id.as_str()],
                )?;
                conn.execute(
                    "DELETE FROM agent_conversation_workspaces WHERE conversation_id = ?1",
                    rusqlite::params![conversation_id],
                )?;
                Ok(())
            })
            .await
    }
}

fn pr_review_action_terminal_status(status: AgentWorkspacePrReviewActionStatus) -> bool {
    matches!(
        status,
        AgentWorkspacePrReviewActionStatus::Skipped
            | AgentWorkspacePrReviewActionStatus::Submitted
            | AgentWorkspacePrReviewActionStatus::Failed
    )
}
