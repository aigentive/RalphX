use std::str::FromStr;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use rusqlite::Connection;
use tokio::sync::Mutex;

use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode,
    AgentConversationWorkspacePublicationEvent, AgentConversationWorkspaceStatus,
    AgentWorkspacePrDescription, ChatConversationId, IdeationAnalysisBaseRefKind,
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
        publication_pr_number: row.get("publication_pr_number")?,
        publication_pr_url: row.get("publication_pr_url")?,
        publication_pr_status: row.get("publication_pr_status")?,
        publication_push_status: row.get("publication_push_status")?,
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
        let publication_pr_number = workspace.publication_pr_number;
        let publication_pr_url = workspace.publication_pr_url.clone();
        let publication_pr_status = workspace.publication_pr_status.clone();
        let publication_push_status = workspace.publication_push_status.clone();
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
                        publication_pr_number, publication_pr_url, publication_pr_status,
                        publication_push_status, pr_autofix_enabled, pr_auto_merge_desired,
                        pr_auto_merge_method, pr_auto_merge_current, pr_supervision_status,
                        pr_supervision_summary, pr_supervision_updated_at, status, created_at,
                        updated_at
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25)
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
                        publication_pr_number=excluded.publication_pr_number,
                        publication_pr_url=excluded.publication_pr_url,
                        publication_pr_status=excluded.publication_pr_status,
                        publication_push_status=excluded.publication_push_status,
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
                        publication_pr_number,
                        publication_pr_url,
                        publication_pr_status,
                        publication_push_status,
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

    async fn list_active_direct_external_pr_reconciliation_candidates(
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
                       AND publication_pr_number IS NULL
                       AND COALESCE(publication_push_status, 'pushed') NOT IN (
                           'needs_agent', 'pending', 'failed', 'description_failed'
                       )
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
        let updated_at = Utc::now().to_rfc3339();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE agent_conversation_workspaces
                     SET publication_pr_number = ?2,
                         publication_pr_url = ?3,
                         publication_pr_status = ?4,
                         publication_push_status = ?5,
                         updated_at = ?6
                     WHERE conversation_id = ?1",
                    rusqlite::params![
                        conversation_id,
                        pr_number,
                        pr_url,
                        pr_status,
                        push_status,
                        updated_at
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

    async fn delete(&self, conversation_id: &ChatConversationId) -> AppResult<()> {
        let conversation_id = conversation_id.as_str().to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "DELETE FROM agent_conversation_workspaces WHERE conversation_id = ?1",
                    rusqlite::params![conversation_id],
                )?;
                Ok(())
            })
            .await
    }
}
