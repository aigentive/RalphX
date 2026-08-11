// SQLite-based TaskProposalRepository implementation for production use
// Uses DbConnection (spawn_blocking) for non-blocking rusqlite access

use std::sync::Arc;
use tokio::sync::Mutex;

use async_trait::async_trait;
use chrono::Utc;
use rusqlite::Connection;

use crate::domain::entities::{
    ArtifactId, IdeationSessionId, PriorityAssessment, TaskId, TaskProposal, TaskProposalId,
};
use crate::domain::repositories::TaskProposalRepository;
use crate::error::{AppError, AppResult};

use super::DbConnection;

#[cfg(test)]
mod tests;

/// SQLite implementation of TaskProposalRepository for production use
pub struct SqliteTaskProposalRepository {
    db: DbConnection,
}

impl SqliteTaskProposalRepository {
    /// Create a new SQLite task proposal repository with the given connection
    pub fn new(conn: Connection) -> Self {
        Self {
            db: DbConnection::new(conn),
        }
    }

    /// Create from an Arc-wrapped mutex connection (for sharing)
    pub fn from_shared(conn: Arc<Mutex<Connection>>) -> Self {
        Self {
            db: DbConnection::from_shared(conn),
        }
    }

    // ============================================================================
    // Sync helpers — pub(crate) methods containing SQL logic.
    // Part of the sync-helper pattern: batch callers (e.g., artifact HTTP handlers)
    // call these directly with &Connection inside a db.run_transaction() closure.
    // Async trait methods wrap these in db.run() for single-operation use.
    // ============================================================================

    /// Fetch proposals linked to a specific plan artifact ID.
    pub(crate) fn get_by_plan_artifact_id_sync(
        conn: &Connection,
        artifact_id: &str,
    ) -> AppResult<Vec<TaskProposal>> {
        let mut stmt = conn.prepare(
            "SELECT id, session_id, title, description, category, steps, acceptance_criteria,
                    affected_paths,
                    suggested_priority, priority_score, priority_reason, priority_factors,
                    estimated_complexity, user_priority, user_modified, status, selected,
                    created_task_id, plan_artifact_id, plan_version_at_creation, blueprint_artifact_id, blueprint_version_at_creation, sort_order, created_at, updated_at, archived_at,
                    target_project, migrated_from_session_id, migrated_from_proposal_id
             FROM task_proposals
             WHERE plan_artifact_id = ?1
             ORDER BY sort_order ASC",
        )?;
        let proposals = stmt
            .query_map([artifact_id], TaskProposal::from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(proposals)
    }

    /// Batch-update all proposals from old_artifact_id to new_artifact_id in a single UPDATE.
    /// Handles the zero-row case without error (returns Ok(()) when no rows match).
    pub(crate) fn batch_update_artifact_id_sync(
        conn: &Connection,
        old_artifact_id: &str,
        new_artifact_id: &str,
    ) -> AppResult<()> {
        let now = Utc::now();
        conn.execute(
            "UPDATE task_proposals SET plan_artifact_id = ?2, updated_at = ?3
             WHERE plan_artifact_id = ?1",
            rusqlite::params![old_artifact_id, new_artifact_id, now.to_rfc3339()],
        )?;
        Ok(())
    }

    pub(crate) fn get_by_blueprint_artifact_id_sync(
        conn: &Connection,
        artifact_id: &str,
    ) -> AppResult<Vec<TaskProposal>> {
        let mut stmt = conn.prepare(
            "SELECT id, session_id, title, description, category, steps, acceptance_criteria,
                    affected_paths, suggested_priority, priority_score, priority_reason,
                    priority_factors, estimated_complexity, user_priority, user_modified,
                    status, selected, created_task_id, plan_artifact_id,
                    plan_version_at_creation, blueprint_artifact_id,
                    blueprint_version_at_creation, sort_order, created_at, updated_at,
                    archived_at, target_project, migrated_from_session_id,
                    migrated_from_proposal_id
             FROM task_proposals
             WHERE blueprint_artifact_id = ?1
             ORDER BY sort_order ASC",
        )?;
        let proposals = stmt
            .query_map([artifact_id], TaskProposal::from_row)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(AppError::from)?;
        Ok(proposals)
    }

    pub(crate) fn batch_update_blueprint_artifact_id_sync(
        conn: &Connection,
        old_artifact_id: &str,
        new_artifact_id: &str,
    ) -> AppResult<()> {
        conn.execute(
            "UPDATE task_proposals
             SET blueprint_artifact_id = ?2, updated_at = ?3
             WHERE blueprint_artifact_id = ?1",
            rusqlite::params![old_artifact_id, new_artifact_id, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    /// Update a batch of proposals to the same exact plan Overview and Blueprint pair.
    /// Legacy v1 plans clear any stale Blueprint lineage.
    pub(crate) fn batch_link_proposals_sync(
        conn: &Connection,
        proposal_ids: &[String],
        artifact_id: &str,
        version: u32,
        blueprint_artifact_id: Option<&str>,
        blueprint_version: Option<u32>,
    ) -> AppResult<()> {
        if proposal_ids.is_empty() {
            return Ok(());
        }
        let now = Utc::now().to_rfc3339();
        let placeholders: Vec<String> = (0..proposal_ids.len())
            .map(|i| format!("?{}", i + 6))
            .collect();
        let sql = format!(
            "UPDATE task_proposals SET plan_artifact_id = ?1, plan_version_at_creation = ?2, \
             blueprint_artifact_id = ?3, blueprint_version_at_creation = ?4, updated_at = ?5 \
             WHERE id IN ({})",
            placeholders.join(", ")
        );
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> =
            Vec::with_capacity(5 + proposal_ids.len());
        params.push(Box::new(artifact_id.to_string()));
        params.push(Box::new(version));
        params.push(Box::new(blueprint_artifact_id.map(str::to_string)));
        params.push(Box::new(blueprint_version));
        params.push(Box::new(now));
        for id in proposal_ids {
            params.push(Box::new(id.clone()));
        }
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|p| p.as_ref()).collect();
        conn.execute(&sql, param_refs.as_slice())?;
        Ok(())
    }

    /// Insert a proposal into the database within a transaction closure.
    /// Takes &Connection directly for use inside db.run_transaction().
    pub(crate) fn create_sync(
        conn: &Connection,
        proposal: TaskProposal,
    ) -> AppResult<TaskProposal> {
        let priority_factors_json = proposal
            .priority_factors
            .as_ref()
            .and_then(|f| serde_json::to_string(f).ok());
        conn.execute(
            "INSERT INTO task_proposals (
                id, session_id, title, description, category, steps, acceptance_criteria,
                affected_paths,
                suggested_priority, priority_score, priority_reason, priority_factors,
                estimated_complexity, user_priority, user_modified, status, selected,
                created_task_id, plan_artifact_id, plan_version_at_creation, blueprint_artifact_id, blueprint_version_at_creation, sort_order, created_at, updated_at,
                target_project, migrated_from_session_id, migrated_from_proposal_id
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23,
                ?24, ?25, ?26, ?27, ?28
            )",
            rusqlite::params![
                proposal.id.as_str(),
                proposal.session_id.as_str(),
                proposal.title,
                proposal.description,
                proposal.category.to_string(),
                proposal.steps,
                proposal.acceptance_criteria,
                proposal.affected_paths,
                proposal.suggested_priority.to_string(),
                proposal.priority_score,
                proposal.priority_reason,
                priority_factors_json,
                proposal.estimated_complexity.to_string(),
                proposal.user_priority.map(|p| p.to_string()),
                proposal.user_modified as i32,
                proposal.status.to_string(),
                proposal.selected as i32,
                proposal.created_task_id.as_ref().map(|id| id.as_str()),
                proposal.plan_artifact_id.as_ref().map(|id| id.as_str()),
                proposal.plan_version_at_creation,
                proposal.blueprint_artifact_id.as_ref().map(|id| id.as_str()),
                proposal.blueprint_version_at_creation,
                proposal.sort_order,
                proposal.created_at.to_rfc3339(),
                proposal.updated_at.to_rfc3339(),
                proposal.target_project,
                proposal.migrated_from_session_id,
                proposal.migrated_from_proposal_id,
            ],
        )?;
        Ok(proposal)
    }

    /// Count proposals for a session within a transaction closure.
    /// Returns usize for direct use in sort_order calculations.
    pub(crate) fn count_by_session_sync(conn: &Connection, session_id: &str) -> AppResult<usize> {
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM task_proposals WHERE session_id = ?1",
            [session_id],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    /// Update a proposal within a transaction closure.
    /// Takes &Connection directly for use inside db.run_transaction().
    /// Returns the proposal with updated `updated_at` timestamp.
    pub(crate) fn update_sync(
        conn: &Connection,
        proposal: &TaskProposal,
    ) -> AppResult<TaskProposal> {
        let now = Utc::now();
        let priority_factors_json = proposal
            .priority_factors
            .as_ref()
            .and_then(|f| serde_json::to_string(f).ok());
        conn.execute(
            "UPDATE task_proposals SET
                title = ?2, description = ?3, category = ?4, steps = ?5, acceptance_criteria = ?6,
                affected_paths = ?7, suggested_priority = ?8, priority_score = ?9, priority_reason = ?10, priority_factors = ?11,
                estimated_complexity = ?12, user_priority = ?13, user_modified = ?14, status = ?15,
                selected = ?16, created_task_id = ?17, plan_artifact_id = ?18, plan_version_at_creation = ?19,
                blueprint_artifact_id = ?20, blueprint_version_at_creation = ?21,
                target_project = ?22, sort_order = ?23, updated_at = ?24,
                migrated_from_session_id = ?25, migrated_from_proposal_id = ?26
             WHERE id = ?1",
            rusqlite::params![
                proposal.id.as_str(),
                proposal.title,
                proposal.description,
                proposal.category.to_string(),
                proposal.steps,
                proposal.acceptance_criteria,
                proposal.affected_paths,
                proposal.suggested_priority.to_string(),
                proposal.priority_score,
                proposal.priority_reason,
                priority_factors_json,
                proposal.estimated_complexity.to_string(),
                proposal.user_priority.map(|p| p.to_string()),
                proposal.user_modified as i32,
                proposal.status.to_string(),
                proposal.selected as i32,
                proposal.created_task_id.as_ref().map(|id| id.as_str()),
                proposal.plan_artifact_id.as_ref().map(|id| id.as_str()),
                proposal.plan_version_at_creation,
                proposal.blueprint_artifact_id.as_ref().map(|id| id.as_str()),
                proposal.blueprint_version_at_creation,
                proposal.target_project,
                proposal.sort_order,
                now.to_rfc3339(),
                proposal.migrated_from_session_id,
                proposal.migrated_from_proposal_id,
            ],
        )?;
        let mut updated = proposal.clone();
        updated.updated_at = now;
        Ok(updated)
    }

    /// Delete a proposal within a transaction closure, scoped to a session.
    /// Takes both proposal_id and session_id to prevent cross-session deletions.
    /// Callers: downstream transaction closures in proposal impl functions (Phase 1 refactor).
    #[allow(dead_code)]
    pub(crate) fn delete_sync(
        conn: &Connection,
        proposal_id: &str,
        session_id: &str,
    ) -> AppResult<()> {
        // CASCADE is defined in the schema, so deleting the proposal
        // will automatically delete related dependencies
        conn.execute(
            "DELETE FROM task_proposals WHERE id = ?1 AND session_id = ?2",
            rusqlite::params![proposal_id, session_id],
        )?;
        Ok(())
    }

    /// Archive a proposal within a transaction closure.
    pub(crate) fn archive_sync(conn: &Connection, id: &TaskProposalId) -> AppResult<TaskProposal> {
        let now = Utc::now();
        conn.execute(
            "UPDATE task_proposals SET archived_at = ?2, updated_at = ?3 WHERE id = ?1 AND archived_at IS NULL",
            rusqlite::params![id.as_str(), now.to_rfc3339(), now.to_rfc3339()],
        )?;
        let proposal = conn.query_row(
            "SELECT id, session_id, title, description, category, steps, acceptance_criteria,
                    affected_paths,
                    suggested_priority, priority_score, priority_reason, priority_factors,
                    estimated_complexity, user_priority, user_modified, status, selected,
                    created_task_id, plan_artifact_id, plan_version_at_creation, blueprint_artifact_id, blueprint_version_at_creation, sort_order, created_at, updated_at, archived_at,
                    target_project, migrated_from_session_id, migrated_from_proposal_id
             FROM task_proposals WHERE id = ?1",
            [id.as_str()],
            |row| TaskProposal::from_row(row),
        )?;
        Ok(proposal)
    }
}

#[async_trait]
impl TaskProposalRepository for SqliteTaskProposalRepository {
    async fn create(&self, proposal: TaskProposal) -> AppResult<TaskProposal> {
        self.db
            .run(move |conn| SqliteTaskProposalRepository::create_sync(conn, proposal))
            .await
    }

    async fn get_by_id(&self, id: &TaskProposalId) -> AppResult<Option<TaskProposal>> {
        let id = id.as_str().to_string();
        self.db
            .query_optional(move |conn| {
                conn.query_row(
                    "SELECT id, session_id, title, description, category, steps, acceptance_criteria,
                            affected_paths,
                            suggested_priority, priority_score, priority_reason, priority_factors,
                            estimated_complexity, user_priority, user_modified, status, selected,
                            created_task_id, plan_artifact_id, plan_version_at_creation, blueprint_artifact_id, blueprint_version_at_creation, sort_order, created_at, updated_at, archived_at,
                            target_project, migrated_from_session_id, migrated_from_proposal_id
                     FROM task_proposals WHERE id = ?1",
                    [&id],
                    |row| TaskProposal::from_row(row),
                )
            })
            .await
    }

    async fn get_by_session(&self, session_id: &IdeationSessionId) -> AppResult<Vec<TaskProposal>> {
        let session_id = session_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, session_id, title, description, category, steps, acceptance_criteria,
                            affected_paths,
                            suggested_priority, priority_score, priority_reason, priority_factors,
                            estimated_complexity, user_priority, user_modified, status, selected,
                            created_task_id, plan_artifact_id, plan_version_at_creation, blueprint_artifact_id, blueprint_version_at_creation, sort_order, created_at, updated_at, archived_at,
                            target_project, migrated_from_session_id, migrated_from_proposal_id
                     FROM task_proposals WHERE session_id = ?1 AND archived_at IS NULL ORDER BY sort_order ASC",
                )?;
                let proposals = stmt
                    .query_map([&session_id], TaskProposal::from_row)?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(proposals)
            })
            .await
    }

    async fn update(&self, proposal: &TaskProposal) -> AppResult<()> {
        let proposal = proposal.clone();
        self.db
            .run(move |conn| SqliteTaskProposalRepository::update_sync(conn, &proposal).map(|_| ()))
            .await
    }

    async fn update_priority(
        &self,
        id: &TaskProposalId,
        assessment: &PriorityAssessment,
    ) -> AppResult<()> {
        let id = id.as_str().to_string();
        let now = Utc::now();
        let suggested_priority = assessment.suggested_priority.to_string();
        let priority_score = assessment.priority_score;
        let priority_reason = assessment.priority_reason.clone();
        let factors_json = serde_json::to_string(&assessment.factors)
            .map_err(|e| AppError::Database(e.to_string()))?;

        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE task_proposals SET
                        suggested_priority = ?2, priority_score = ?3, priority_reason = ?4,
                        priority_factors = ?5, updated_at = ?6
                     WHERE id = ?1",
                    rusqlite::params![
                        id,
                        suggested_priority,
                        priority_score,
                        priority_reason,
                        factors_json,
                        now.to_rfc3339(),
                    ],
                )?;
                Ok(())
            })
            .await
    }

    async fn update_selection(&self, id: &TaskProposalId, selected: bool) -> AppResult<()> {
        let id = id.as_str().to_string();
        let now = Utc::now();

        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE task_proposals SET selected = ?2, updated_at = ?3 WHERE id = ?1",
                    rusqlite::params![id, selected as i32, now.to_rfc3339()],
                )?;
                Ok(())
            })
            .await
    }

    async fn set_created_task_id(&self, id: &TaskProposalId, task_id: &TaskId) -> AppResult<()> {
        let id = id.as_str().to_string();
        let task_id = task_id.as_str().to_string();
        let now = Utc::now();

        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE task_proposals SET created_task_id = ?2, updated_at = ?3 WHERE id = ?1",
                    rusqlite::params![id, task_id, now.to_rfc3339()],
                )?;
                Ok(())
            })
            .await
    }

    async fn delete(&self, id: &TaskProposalId) -> AppResult<()> {
        let id = id.as_str().to_string();
        self.db
            .run(move |conn| {
                // CASCADE is defined in the schema, so deleting the proposal
                // will automatically delete related dependencies
                conn.execute("DELETE FROM task_proposals WHERE id = ?1", [id])?;
                Ok(())
            })
            .await
    }

    async fn reorder(
        &self,
        session_id: &IdeationSessionId,
        proposal_ids: Vec<TaskProposalId>,
    ) -> AppResult<()> {
        let session_id = session_id.as_str().to_string();
        let now = Utc::now();

        self.db
            .run(move |conn| {
                if proposal_ids.is_empty() {
                    return Ok(());
                }
                // Single UPDATE with CASE expression for per-row sort_order values
                let mut case_parts = Vec::with_capacity(proposal_ids.len());
                let mut id_placeholders = Vec::with_capacity(proposal_ids.len());
                let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
                // ?1 = updated_at, ?2 = session_id, then ?3..?N for CASE WHEN pairs
                params.push(Box::new(now.to_rfc3339()));
                params.push(Box::new(session_id));
                let mut idx = 3;
                for (order, proposal_id) in proposal_ids.iter().enumerate() {
                    case_parts.push(format!("WHEN id = ?{} THEN ?{}", idx, idx + 1));
                    id_placeholders.push(format!("?{}", idx));
                    params.push(Box::new(proposal_id.as_str().to_string()));
                    params.push(Box::new(order as i32));
                    idx += 2;
                }
                let sql = format!(
                    "UPDATE task_proposals SET sort_order = CASE {} END, updated_at = ?1 \
                     WHERE session_id = ?2 AND id IN ({})",
                    case_parts.join(" "),
                    id_placeholders.join(", ")
                );
                let param_refs: Vec<&dyn rusqlite::types::ToSql> =
                    params.iter().map(|p| p.as_ref()).collect();
                conn.execute(&sql, param_refs.as_slice())?;
                Ok(())
            })
            .await
    }

    async fn get_selected_by_session(
        &self,
        session_id: &IdeationSessionId,
    ) -> AppResult<Vec<TaskProposal>> {
        let session_id = session_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, session_id, title, description, category, steps, acceptance_criteria,
                            affected_paths,
                            suggested_priority, priority_score, priority_reason, priority_factors,
                            estimated_complexity, user_priority, user_modified, status, selected,
                            created_task_id, plan_artifact_id, plan_version_at_creation, blueprint_artifact_id, blueprint_version_at_creation, sort_order, created_at, updated_at, archived_at,
                            target_project, migrated_from_session_id, migrated_from_proposal_id
                     FROM task_proposals
                     WHERE session_id = ?1 AND selected = 1 AND archived_at IS NULL
                     ORDER BY sort_order ASC",
                )?;
                let proposals = stmt
                    .query_map([&session_id], TaskProposal::from_row)?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(proposals)
            })
            .await
    }

    async fn count_by_session(&self, session_id: &IdeationSessionId) -> AppResult<u32> {
        let session_id = session_id.as_str().to_string();
        self.db
            .run(move |conn| {
                SqliteTaskProposalRepository::count_by_session_sync(conn, &session_id)
                    .map(|c| c as u32)
            })
            .await
    }

    async fn count_selected_by_session(&self, session_id: &IdeationSessionId) -> AppResult<u32> {
        let session_id = session_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let count: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM task_proposals WHERE session_id = ?1 AND selected = 1",
                    [&session_id],
                    |row| row.get(0),
                )?;
                Ok(count as u32)
            })
            .await
    }

    async fn get_by_plan_artifact_id(
        &self,
        artifact_id: &ArtifactId,
    ) -> AppResult<Vec<TaskProposal>> {
        let artifact_id = artifact_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, session_id, title, description, category, steps, acceptance_criteria,
                            affected_paths,
                            suggested_priority, priority_score, priority_reason, priority_factors,
                            estimated_complexity, user_priority, user_modified, status, selected,
                            created_task_id, plan_artifact_id, plan_version_at_creation, blueprint_artifact_id, blueprint_version_at_creation, sort_order, created_at, updated_at, archived_at,
                            target_project, migrated_from_session_id, migrated_from_proposal_id
                     FROM task_proposals
                     WHERE plan_artifact_id = ?1 AND archived_at IS NULL
                     ORDER BY sort_order ASC",
                )?;
                let proposals = stmt
                    .query_map([&artifact_id], TaskProposal::from_row)?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(proposals)
            })
            .await
    }

    async fn clear_created_task_ids_by_session(
        &self,
        session_id: &IdeationSessionId,
    ) -> AppResult<()> {
        let session_id = session_id.as_str().to_string();
        let now = Utc::now();

        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE task_proposals SET created_task_id = NULL, updated_at = ?2 WHERE session_id = ?1",
                    rusqlite::params![session_id, now.to_rfc3339()],
                )?;
                Ok(())
            })
            .await
    }

    async fn archive(&self, id: &TaskProposalId) -> AppResult<TaskProposal> {
        let id = id.clone();
        self.db
            .run(move |conn| SqliteTaskProposalRepository::archive_sync(conn, &id))
            .await
    }
}
