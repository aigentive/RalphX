use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use tokio::sync::Mutex;

use crate::domain::entities::{
    AgentConversationIssue, AgentConversationIssueOccurrence, ChatConversationId, ProjectId,
    AGENT_CONVERSATION_ISSUE_STATUS_DISMISSED, AGENT_CONVERSATION_ISSUE_STATUS_OPEN,
    AGENT_CONVERSATION_ISSUE_STATUS_RESOLVED,
};
use crate::domain::repositories::AgentConversationIssueRepository;
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

fn row_to_issue(row: &rusqlite::Row<'_>) -> rusqlite::Result<AgentConversationIssue> {
    let created_at: String = row.get("created_at")?;
    let updated_at: String = row.get("updated_at")?;
    let resolved_at = row
        .get::<_, Option<String>>("resolved_at")?
        .map(|value| parse_datetime(&value));
    Ok(AgentConversationIssue {
        id: row.get("id")?,
        project_id: ProjectId::from_string(row.get::<_, String>("project_id")?),
        conversation_id: ChatConversationId::from_string(row.get::<_, String>("conversation_id")?),
        source_task_id: row.get("source_task_id")?,
        source_context_type: row.get("source_context_type")?,
        source_context_id: row.get("source_context_id")?,
        source_agent_name: row.get("source_agent_name")?,
        issue_kind: row.get("issue_kind")?,
        severity: row.get("severity")?,
        status: row.get("status")?,
        blocking_scope: row.get("blocking_scope")?,
        title: row.get("title")?,
        summary: row.get("summary")?,
        evidence: row.get("evidence")?,
        recommendation: row.get("recommendation")?,
        blocker_fingerprint: row.get("blocker_fingerprint")?,
        canonical_fingerprint: row.get("canonical_fingerprint")?,
        canonical_scope_kind: row.get("canonical_scope_kind")?,
        canonical_scope_subject: row.get("canonical_scope_subject")?,
        canonical_family: row.get("canonical_family")?,
        superseded_by_issue_id: row.get("superseded_by_issue_id")?,
        followup_title: row.get("followup_title")?,
        followup_prompt: row.get("followup_prompt")?,
        auto_followup_eligible: row.get::<_, i64>("auto_followup_eligible")? != 0,
        linked_followup_conversation_id: row
            .get::<_, Option<String>>("linked_followup_conversation_id")?
            .map(ChatConversationId::from_string),
        created_at: parse_datetime(&created_at),
        updated_at: parse_datetime(&updated_at),
        resolved_at,
    })
}

fn row_to_occurrence(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<AgentConversationIssueOccurrence> {
    let created_at: String = row.get("created_at")?;
    Ok(AgentConversationIssueOccurrence {
        id: row.get("id")?,
        issue_id: row.get("issue_id")?,
        project_id: ProjectId::from_string(row.get::<_, String>("project_id")?),
        conversation_id: ChatConversationId::from_string(row.get::<_, String>("conversation_id")?),
        source_task_id: row.get("source_task_id")?,
        source_context_type: row.get("source_context_type")?,
        source_context_id: row.get("source_context_id")?,
        source_agent_name: row.get("source_agent_name")?,
        issue_kind: row.get("issue_kind")?,
        severity: row.get("severity")?,
        blocking_scope: row.get("blocking_scope")?,
        title: row.get("title")?,
        summary: row.get("summary")?,
        evidence: row.get("evidence")?,
        recommendation: row.get("recommendation")?,
        raw_blocker_fingerprint: row.get("raw_blocker_fingerprint")?,
        canonical_fingerprint: row.get("canonical_fingerprint")?,
        dedupe_decision: row.get("dedupe_decision")?,
        created_at: parse_datetime(&created_at),
    })
}

pub struct SqliteAgentConversationIssueRepository {
    db: DbConnection,
}

impl SqliteAgentConversationIssueRepository {
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
impl AgentConversationIssueRepository for SqliteAgentConversationIssueRepository {
    async fn save(&self, issue: &AgentConversationIssue) -> AppResult<AgentConversationIssue> {
        let issue = issue.clone();
        let fetch_id = issue.id.clone();
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO agent_conversation_issues (
                        id, project_id, conversation_id, source_task_id,
                        source_context_type, source_context_id, source_agent_name,
                        issue_kind, severity, status, blocking_scope, title, summary,
                        evidence, recommendation, blocker_fingerprint, canonical_fingerprint,
                        canonical_scope_kind, canonical_scope_subject, canonical_family,
                        superseded_by_issue_id, followup_title, followup_prompt, auto_followup_eligible,
                        linked_followup_conversation_id, created_at, updated_at, resolved_at
                    ) VALUES (
                        ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
                        ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23,
                        ?24, ?25, ?26, ?27, ?28
                    )
                    ON CONFLICT(id) DO UPDATE SET
                        project_id=excluded.project_id,
                        conversation_id=excluded.conversation_id,
                        source_task_id=excluded.source_task_id,
                        source_context_type=excluded.source_context_type,
                        source_context_id=excluded.source_context_id,
                        source_agent_name=excluded.source_agent_name,
                        issue_kind=excluded.issue_kind,
                        severity=excluded.severity,
                        status=excluded.status,
                        blocking_scope=excluded.blocking_scope,
                        title=excluded.title,
                        summary=excluded.summary,
                        evidence=excluded.evidence,
                        recommendation=excluded.recommendation,
                        blocker_fingerprint=excluded.blocker_fingerprint,
                        canonical_fingerprint=excluded.canonical_fingerprint,
                        canonical_scope_kind=excluded.canonical_scope_kind,
                        canonical_scope_subject=excluded.canonical_scope_subject,
                        canonical_family=excluded.canonical_family,
                        superseded_by_issue_id=excluded.superseded_by_issue_id,
                        followup_title=excluded.followup_title,
                        followup_prompt=excluded.followup_prompt,
                        auto_followup_eligible=excluded.auto_followup_eligible,
                        linked_followup_conversation_id=excluded.linked_followup_conversation_id,
                        updated_at=excluded.updated_at,
                        resolved_at=excluded.resolved_at",
                    params![
                        issue.id.as_str(),
                        issue.project_id.as_str(),
                        issue.conversation_id.as_str(),
                        issue.source_task_id.as_deref(),
                        issue.source_context_type.as_deref(),
                        issue.source_context_id.as_deref(),
                        issue.source_agent_name.as_deref(),
                        issue.issue_kind.as_str(),
                        issue.severity.as_str(),
                        issue.status.as_str(),
                        issue.blocking_scope.as_str(),
                        issue.title.as_str(),
                        issue.summary.as_str(),
                        issue.evidence.as_deref(),
                        issue.recommendation.as_deref(),
                        issue.blocker_fingerprint.as_deref(),
                        issue.canonical_fingerprint.as_deref(),
                        issue.canonical_scope_kind.as_deref(),
                        issue.canonical_scope_subject.as_deref(),
                        issue.canonical_family.as_deref(),
                        issue.superseded_by_issue_id.as_deref(),
                        issue.followup_title.as_deref(),
                        issue.followup_prompt.as_deref(),
                        issue.auto_followup_eligible as i64,
                        issue
                            .linked_followup_conversation_id
                            .as_ref()
                            .map(|id| id.as_str()),
                        issue.created_at.to_rfc3339(),
                        issue.updated_at.to_rfc3339(),
                        issue.resolved_at.as_ref().map(DateTime::to_rfc3339),
                    ],
                )?;
                conn.query_row(
                    "SELECT * FROM agent_conversation_issues WHERE id = ?1",
                    params![fetch_id],
                    row_to_issue,
                )
                .map_err(AppError::from)
            })
            .await
    }

    async fn get_by_id(&self, issue_id: &str) -> AppResult<Option<AgentConversationIssue>> {
        let issue_id = issue_id.to_string();
        self.db
            .run(move |conn| {
                conn.query_row(
                    "SELECT * FROM agent_conversation_issues WHERE id = ?1",
                    params![issue_id],
                    row_to_issue,
                )
                .optional()
                .map_err(AppError::from)
            })
            .await
    }

    async fn list_by_conversation(
        &self,
        conversation_id: &ChatConversationId,
        include_resolved: bool,
    ) -> AppResult<Vec<AgentConversationIssue>> {
        let conversation_id = conversation_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = if include_resolved {
                    conn.prepare(
                        "SELECT * FROM agent_conversation_issues
                         WHERE conversation_id = ?1
                         ORDER BY updated_at DESC",
                    )?
                } else {
                    conn.prepare(
                        "SELECT * FROM agent_conversation_issues
                         WHERE conversation_id = ?1
                           AND status NOT IN (?2, ?3)
                         ORDER BY updated_at DESC",
                    )?
                };
                let rows = if include_resolved {
                    stmt.query_map(params![conversation_id.as_str()], row_to_issue)?
                        .collect::<Result<Vec<_>, _>>()?
                } else {
                    stmt.query_map(
                        params![
                            conversation_id.as_str(),
                            AGENT_CONVERSATION_ISSUE_STATUS_RESOLVED,
                            AGENT_CONVERSATION_ISSUE_STATUS_DISMISSED
                        ],
                        row_to_issue,
                    )?
                    .collect::<Result<Vec<_>, _>>()?
                };
                Ok(rows)
            })
            .await
    }

    async fn find_open_by_fingerprint(
        &self,
        conversation_id: &ChatConversationId,
        source_task_id: Option<&str>,
        issue_kind: &str,
        blocker_fingerprint: &str,
    ) -> AppResult<Option<AgentConversationIssue>> {
        let conversation_id = conversation_id.as_str().to_string();
        let source_task_id = source_task_id.map(str::to_string);
        let issue_kind = issue_kind.to_string();
        let blocker_fingerprint = blocker_fingerprint.to_string();
        self.db
            .run(move |conn| {
                conn.query_row(
                    "SELECT * FROM agent_conversation_issues
                     WHERE conversation_id = ?1
                       AND ((source_task_id IS NULL AND ?2 IS NULL) OR source_task_id = ?2)
                       AND issue_kind = ?3
                       AND blocker_fingerprint = ?4
                       AND status = ?5
                     ORDER BY updated_at DESC
                     LIMIT 1",
                    params![
                        conversation_id.as_str(),
                        source_task_id.as_deref(),
                        issue_kind,
                        blocker_fingerprint,
                        AGENT_CONVERSATION_ISSUE_STATUS_OPEN,
                    ],
                    row_to_issue,
                )
                .optional()
                .map_err(AppError::from)
            })
            .await
    }

    async fn find_open_by_canonical_fingerprint(
        &self,
        conversation_id: &ChatConversationId,
        canonical_fingerprint: &str,
    ) -> AppResult<Option<AgentConversationIssue>> {
        let conversation_id = conversation_id.as_str().to_string();
        let canonical_fingerprint = canonical_fingerprint.to_string();
        self.db
            .run(move |conn| {
                conn.query_row(
                    "SELECT * FROM agent_conversation_issues
                     WHERE conversation_id = ?1
                       AND canonical_fingerprint = ?2
                       AND status = ?3
                     ORDER BY updated_at DESC
                     LIMIT 1",
                    params![
                        conversation_id.as_str(),
                        canonical_fingerprint,
                        AGENT_CONVERSATION_ISSUE_STATUS_OPEN,
                    ],
                    row_to_issue,
                )
                .optional()
                .map_err(AppError::from)
            })
            .await
    }

    async fn list_open_candidates_by_identity(
        &self,
        conversation_id: &ChatConversationId,
        canonical_scope_kind: &str,
        canonical_scope_subject: &str,
        canonical_family: &str,
        exclude_canonical_fingerprint: &str,
        limit: usize,
    ) -> AppResult<Vec<AgentConversationIssue>> {
        let conversation_id = conversation_id.as_str().to_string();
        let canonical_scope_kind = canonical_scope_kind.to_string();
        let canonical_scope_subject = canonical_scope_subject.to_string();
        let canonical_family = canonical_family.to_string();
        let exclude_canonical_fingerprint = exclude_canonical_fingerprint.to_string();
        let limit = limit as i64;
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT * FROM agent_conversation_issues
                     WHERE conversation_id = ?1
                       AND canonical_scope_kind = ?2
                       AND canonical_scope_subject = ?3
                       AND canonical_family = ?4
                       AND (canonical_fingerprint IS NULL OR canonical_fingerprint <> ?5)
                       AND status = ?6
                     ORDER BY updated_at DESC
                     LIMIT ?7",
                )?;
                let rows = stmt.query_map(
                    params![
                        conversation_id.as_str(),
                        canonical_scope_kind,
                        canonical_scope_subject,
                        canonical_family,
                        exclude_canonical_fingerprint,
                        AGENT_CONVERSATION_ISSUE_STATUS_OPEN,
                        limit,
                    ],
                    row_to_issue,
                )?;
                rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
            })
            .await
    }

    async fn append_occurrence(
        &self,
        occurrence: &AgentConversationIssueOccurrence,
    ) -> AppResult<AgentConversationIssueOccurrence> {
        let occurrence = occurrence.clone();
        let fetch_id = occurrence.id.clone();
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO agent_conversation_issue_occurrences (
                        id, issue_id, project_id, conversation_id, source_task_id,
                        source_context_type, source_context_id, source_agent_name,
                        issue_kind, severity, blocking_scope, title, summary,
                        evidence, recommendation, raw_blocker_fingerprint,
                        canonical_fingerprint, dedupe_decision, created_at
                    ) VALUES (
                        ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                        ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19
                    )
                    ON CONFLICT(id) DO UPDATE SET
                        issue_id=excluded.issue_id,
                        project_id=excluded.project_id,
                        conversation_id=excluded.conversation_id,
                        source_task_id=excluded.source_task_id,
                        source_context_type=excluded.source_context_type,
                        source_context_id=excluded.source_context_id,
                        source_agent_name=excluded.source_agent_name,
                        issue_kind=excluded.issue_kind,
                        severity=excluded.severity,
                        blocking_scope=excluded.blocking_scope,
                        title=excluded.title,
                        summary=excluded.summary,
                        evidence=excluded.evidence,
                        recommendation=excluded.recommendation,
                        raw_blocker_fingerprint=excluded.raw_blocker_fingerprint,
                        canonical_fingerprint=excluded.canonical_fingerprint,
                        dedupe_decision=excluded.dedupe_decision,
                        created_at=excluded.created_at",
                    params![
                        occurrence.id.as_str(),
                        occurrence.issue_id.as_str(),
                        occurrence.project_id.as_str(),
                        occurrence.conversation_id.as_str(),
                        occurrence.source_task_id.as_deref(),
                        occurrence.source_context_type.as_deref(),
                        occurrence.source_context_id.as_deref(),
                        occurrence.source_agent_name.as_deref(),
                        occurrence.issue_kind.as_str(),
                        occurrence.severity.as_str(),
                        occurrence.blocking_scope.as_str(),
                        occurrence.title.as_str(),
                        occurrence.summary.as_str(),
                        occurrence.evidence.as_deref(),
                        occurrence.recommendation.as_deref(),
                        occurrence.raw_blocker_fingerprint.as_deref(),
                        occurrence.canonical_fingerprint.as_deref(),
                        occurrence.dedupe_decision.as_deref(),
                        occurrence.created_at.to_rfc3339(),
                    ],
                )?;
                conn.query_row(
                    "SELECT * FROM agent_conversation_issue_occurrences WHERE id = ?1",
                    params![fetch_id],
                    row_to_occurrence,
                )
                .map_err(AppError::from)
            })
            .await
    }

    async fn list_occurrences_by_issue(
        &self,
        issue_id: &str,
    ) -> AppResult<Vec<AgentConversationIssueOccurrence>> {
        let issue_id = issue_id.to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT * FROM agent_conversation_issue_occurrences
                     WHERE issue_id = ?1
                     ORDER BY created_at DESC",
                )?;
                let rows = stmt.query_map(params![issue_id], row_to_occurrence)?;
                rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
            })
            .await
    }

    async fn update_status(
        &self,
        issue_id: &str,
        status: &str,
    ) -> AppResult<Option<AgentConversationIssue>> {
        let issue_id = issue_id.to_string();
        let status = status.to_string();
        self.db
            .run(move |conn| {
                let now = Utc::now().to_rfc3339();
                conn.execute(
                    "UPDATE agent_conversation_issues
                     SET status = ?2,
                         updated_at = ?3,
                         resolved_at = CASE WHEN ?2 = ?4 THEN NULL ELSE ?3 END
                     WHERE id = ?1",
                    params![issue_id, status, now, AGENT_CONVERSATION_ISSUE_STATUS_OPEN],
                )?;
                conn.query_row(
                    "SELECT * FROM agent_conversation_issues WHERE id = ?1",
                    params![issue_id],
                    row_to_issue,
                )
                .optional()
                .map_err(AppError::from)
            })
            .await
    }

    async fn link_followup_conversation(
        &self,
        issue_id: &str,
        followup_conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentConversationIssue>> {
        let issue_id = issue_id.to_string();
        let followup_conversation_id = followup_conversation_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let now = Utc::now().to_rfc3339();
                conn.execute(
                    "UPDATE agent_conversation_issues
                     SET linked_followup_conversation_id = ?2,
                         updated_at = ?3
                     WHERE id = ?1",
                    params![issue_id, followup_conversation_id.as_str(), now],
                )?;
                conn.query_row(
                    "SELECT * FROM agent_conversation_issues WHERE id = ?1",
                    params![issue_id],
                    row_to_issue,
                )
                .optional()
                .map_err(AppError::from)
            })
            .await
    }
}

#[cfg(test)]
#[path = "sqlite_agent_conversation_issue_repo_tests.rs"]
mod tests;
