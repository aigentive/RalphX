// SQLite-based ProjectRepository implementation for production use
// Uses rusqlite with connection pooling for thread-safe access

use std::sync::Arc;
use tokio::sync::Mutex;

use async_trait::async_trait;
use chrono::Utc;
use rusqlite::{Connection, OptionalExtension};

use super::DbConnection;
use crate::domain::entities::{Project, ProjectId};
use crate::domain::repositories::ProjectRepository;
use crate::error::AppResult;

pub(crate) fn insert_project_row(conn: &Connection, project: &Project) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO projects (id, name, working_directory, git_mode, base_branch, worktree_parent_directory, use_feature_branches, merge_validation_mode, merge_strategy, detected_analysis, custom_analysis, analyzed_at, created_at, updated_at, github_pr_enabled)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        rusqlite::params![
            project.id.as_str(),
            project.name.as_str(),
            project.working_directory.as_str(),
            project.git_mode.to_string(),
            project.base_branch.as_deref(),
            project.worktree_parent_directory.as_deref(),
            project.use_feature_branches as i64,
            project.merge_validation_mode.to_string(),
            project.merge_strategy.to_string(),
            project.detected_analysis.as_deref(),
            project.custom_analysis.as_deref(),
            project.analyzed_at.as_deref(),
            project.created_at.to_rfc3339(),
            project.updated_at.to_rfc3339(),
            project.github_pr_enabled as i64,
        ],
    )?;
    Ok(())
}

/// SQLite implementation of ProjectRepository for production use
/// Uses a mutex-protected connection for thread-safe access
pub struct SqliteProjectRepository {
    db: DbConnection,
}

impl SqliteProjectRepository {
    /// Create a new SQLite project repository with the given connection
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
}

#[async_trait]
impl ProjectRepository for SqliteProjectRepository {
    async fn create(&self, project: Project) -> AppResult<Project> {
        self.db
            .run(move |conn| {
                if let Some(mut existing_project) = conn
                    .query_row(
                        "SELECT id, name, working_directory, git_mode, base_branch, worktree_parent_directory, use_feature_branches, merge_validation_mode, merge_strategy, detected_analysis, custom_analysis, analyzed_at, created_at, updated_at, github_pr_enabled, archived_at
                         FROM projects WHERE working_directory = ?1",
                        [project.working_directory.as_str()],
                        |row| Project::from_row(row),
                    )
                    .optional()?
                {
                    if existing_project.archived_at.is_some() {
                        let now = Utc::now();
                        existing_project.name = project.name.clone();
                        existing_project.working_directory = project.working_directory.clone();
                        existing_project.git_mode = project.git_mode;
                        existing_project.base_branch = project.base_branch.clone();
                        existing_project.worktree_parent_directory =
                            project.worktree_parent_directory.clone();
                        existing_project.updated_at = now;
                        existing_project.archived_at = None;

                        conn.execute(
                            "UPDATE projects
                             SET name = ?2,
                                 working_directory = ?3,
                                 git_mode = ?4,
                                 base_branch = ?5,
                                 worktree_parent_directory = ?6,
                                 updated_at = ?7,
                                 archived_at = NULL
                             WHERE id = ?1",
                            rusqlite::params![
                                existing_project.id.as_str(),
                                existing_project.name.as_str(),
                                existing_project.working_directory.as_str(),
                                existing_project.git_mode.to_string(),
                                existing_project.base_branch.as_deref(),
                                existing_project.worktree_parent_directory.as_deref(),
                                existing_project.updated_at.to_rfc3339(),
                            ],
                        )?;

                        return Ok(existing_project);
                    }
                }

                insert_project_row(conn, &project)?;
                Ok(project)
            })
            .await
    }

    async fn get_by_id(&self, id: &ProjectId) -> AppResult<Option<Project>> {
        let id = id.as_str().to_string();
        self.db
            .query_optional(move |conn| {
                conn.query_row(
                    "SELECT id, name, working_directory, git_mode, base_branch, worktree_parent_directory, use_feature_branches, merge_validation_mode, merge_strategy, detected_analysis, custom_analysis, analyzed_at, created_at, updated_at, github_pr_enabled, archived_at
                     FROM projects WHERE id = ?1",
                    [id.as_str()],
                    |row| Project::from_row(row),
                )
            })
            .await
    }

    async fn get_all(&self) -> AppResult<Vec<Project>> {
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, name, working_directory, git_mode, base_branch, worktree_parent_directory, use_feature_branches, merge_validation_mode, merge_strategy, detected_analysis, custom_analysis, analyzed_at, created_at, updated_at, github_pr_enabled, archived_at
                     FROM projects WHERE archived_at IS NULL ORDER BY name ASC",
                )?;
                let projects = stmt
                    .query_map([], Project::from_row)?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(projects)
            })
            .await
    }

    async fn update(&self, project: &Project) -> AppResult<()> {
        let id = project.id.as_str().to_string();
        let name = project.name.clone();
        let working_directory = project.working_directory.clone();
        let git_mode = project.git_mode.to_string();
        let base_branch = project.base_branch.clone();
        let worktree_parent_directory = project.worktree_parent_directory.clone();
        let use_feature_branches = project.use_feature_branches as i64;
        let merge_validation_mode = project.merge_validation_mode.to_string();
        let merge_strategy = project.merge_strategy.to_string();
        let detected_analysis = project.detected_analysis.clone();
        let custom_analysis = project.custom_analysis.clone();
        let analyzed_at = project.analyzed_at.clone();
        let updated_at = project.updated_at.to_rfc3339();
        let github_pr_enabled = project.github_pr_enabled as i64;

        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE projects SET name = ?2, working_directory = ?3, git_mode = ?4, base_branch = ?5, worktree_parent_directory = ?6, use_feature_branches = ?7, merge_validation_mode = ?8, merge_strategy = ?9, detected_analysis = ?10, custom_analysis = ?11, analyzed_at = ?12, updated_at = ?13, github_pr_enabled = ?14
                     WHERE id = ?1",
                    rusqlite::params![
                        id,
                        name,
                        working_directory,
                        git_mode,
                        base_branch,
                        worktree_parent_directory,
                        use_feature_branches,
                        merge_validation_mode,
                        merge_strategy,
                        detected_analysis,
                        custom_analysis,
                        analyzed_at,
                        updated_at,
                        github_pr_enabled,
                    ],
                )?;
                Ok(())
            })
            .await
    }

    async fn delete(&self, id: &ProjectId) -> AppResult<()> {
        let id = id.as_str().to_string();
        self.db
            .run(move |conn| {
                conn.execute("DELETE FROM projects WHERE id = ?1", [id.as_str()])?;
                Ok(())
            })
            .await
    }

    async fn delete_with_dependent_sweep(&self, id: &ProjectId) -> AppResult<()> {
        let project_id = id.as_str().to_string();
        self.db
            .run_transaction(move |conn| {
                conn.execute(
                    "UPDATE chat_conversations
                     SET persona_id = NULL, builder_draft_id = NULL,
                         builder_result_persona_id = NULL
                     WHERE (context_type = 'project' AND context_id = ?1)
                        OR persona_id IN (SELECT id FROM personas WHERE project_id = ?1)
                        OR builder_draft_id IN (SELECT id FROM personas WHERE project_id = ?1)
                        OR builder_result_persona_id IN (
                            SELECT id FROM personas WHERE project_id = ?1
                        )",
                    [&project_id],
                )?;
                conn.execute(
                    "WITH RECURSIVE draft_chains(id) AS (
                         SELECT artifact_id FROM personas
                         WHERE project_id = ?1 AND status = 'draft' AND artifact_id IS NOT NULL
                         UNION
                         SELECT artifacts.previous_version_id
                         FROM artifacts JOIN draft_chains ON artifacts.id = draft_chains.id
                         WHERE artifacts.previous_version_id IS NOT NULL
                     )
                     DELETE FROM artifacts WHERE id IN (SELECT id FROM draft_chains)",
                    [&project_id],
                )?;
                conn.execute(
                    "DELETE FROM personas WHERE project_id = ?1 AND status = 'draft'",
                    [&project_id],
                )?;
                conn.execute(
                    "UPDATE personas SET status = 'archived', updated_at = ?2
                     WHERE project_id = ?1 AND status = 'active'",
                    rusqlite::params![project_id, Utc::now().to_rfc3339()],
                )?;
                conn.execute("DELETE FROM projects WHERE id = ?1", [&project_id])?;
                Ok(())
            })
            .await
    }

    async fn get_by_working_directory(&self, path: &str) -> AppResult<Option<Project>> {
        let path = path.to_string();
        self.db
            .query_optional(move |conn| {
                conn.query_row(
                    "SELECT id, name, working_directory, git_mode, base_branch, worktree_parent_directory, use_feature_branches, merge_validation_mode, merge_strategy, detected_analysis, custom_analysis, analyzed_at, created_at, updated_at, github_pr_enabled, archived_at
                     FROM projects WHERE working_directory = ?1",
                    [path.as_str()],
                    |row| Project::from_row(row),
                )
            })
            .await
    }

    async fn archive(&self, id: &ProjectId) -> AppResult<Project> {
        let id = id.as_str().to_string();
        self.db
            .run(move |conn| {
                let now = Utc::now();
                conn.execute(
                    "UPDATE projects SET archived_at = ?2, updated_at = ?3 WHERE id = ?1 AND archived_at IS NULL",
                    rusqlite::params![id.as_str(), now.to_rfc3339(), now.to_rfc3339()],
                )?;
                let project = conn.query_row(
                    "SELECT id, name, working_directory, git_mode, base_branch, worktree_parent_directory, use_feature_branches, merge_validation_mode, merge_strategy, detected_analysis, custom_analysis, analyzed_at, created_at, updated_at, github_pr_enabled, archived_at
                     FROM projects WHERE id = ?1",
                    [id.as_str()],
                    |row| Project::from_row(row),
                )?;
                Ok(project)
            })
            .await
    }
}

#[cfg(test)]
#[path = "sqlite_project_repo_tests.rs"]
mod tests;
