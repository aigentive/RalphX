use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rusqlite::Connection;
use serde_json::to_string;
use tempfile::{Builder, TempDir};
use tokio::sync::Mutex;

use crate::application::AppState;
use crate::domain::entities::{
    ChatConversation, IdeationSession, IdeationSessionId, Project, ProjectId, ReviewNote, Task,
    TaskId,
};
use crate::infrastructure::sqlite::sqlite_project_repo::insert_project_row;
use crate::infrastructure::sqlite::{open_connection, run_migrations};

pub struct SqliteTestDb {
    _temp_dir: TempDir,
    path: PathBuf,
    shared_conn: Arc<Mutex<Connection>>,
}

impl SqliteTestDb {
    pub fn new(name: &str) -> Self {
        let sanitized_name = sanitize_name(name);
        let temp_dir = Builder::new()
            .prefix(&format!("ralphx-{sanitized_name}-"))
            .tempdir()
            .expect("Failed to create temp dir for SQLite test DB");
        let path = temp_dir.path().join("test.db");
        let conn = open_connection(&path).expect("Failed to open SQLite test DB");
        run_migrations(&conn).expect("Failed to run migrations for SQLite test DB");

        Self {
            _temp_dir: temp_dir,
            path,
            shared_conn: Arc::new(Mutex::new(conn)),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn shared_conn(&self) -> Arc<Mutex<Connection>> {
        Arc::clone(&self.shared_conn)
    }

    pub fn new_connection(&self) -> Connection {
        open_connection(&self.path).expect("Failed to open additional SQLite test DB connection")
    }

    pub fn with_connection<T>(&self, f: impl FnOnce(&Connection) -> T) -> T {
        let guard = self
            .shared_conn
            .try_lock()
            .expect("SQLite test DB unexpectedly contended during setup");
        f(&guard)
    }

    pub fn insert_project(&self, project: Project) -> Project {
        self.with_connection(|conn| {
            insert_project_row(conn, &project).expect("Failed to insert test project");
        });
        project
    }

    pub fn insert_project_using_schema_defaults(&self, project: Project) -> Project {
        self.with_connection(|conn| {
            conn.execute(
                "INSERT INTO projects (id, name, working_directory, git_mode, base_branch, worktree_parent_directory, use_feature_branches, merge_strategy, detected_analysis, custom_analysis, analyzed_at, created_at, updated_at, github_pr_enabled)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                rusqlite::params![
                    project.id.as_str(),
                    project.name.as_str(),
                    project.working_directory.as_str(),
                    project.git_mode.to_string(),
                    project.base_branch.as_deref(),
                    project.worktree_parent_directory.as_deref(),
                    project.use_feature_branches as i64,
                    project.merge_strategy.to_string(),
                    project.detected_analysis.as_deref(),
                    project.custom_analysis.as_deref(),
                    project.analyzed_at.as_deref(),
                    project.created_at.to_rfc3339(),
                    project.updated_at.to_rfc3339(),
                    project.github_pr_enabled as i64,
                ],
            )
            .expect("Failed to insert test project using schema defaults");
        });
        project
    }

    pub fn seed_project(&self, name: &str) -> Project {
        let mut project = Project::new(name.to_string(), String::new());
        project.working_directory = format!("/tmp/ralphx-tests/{}", project.id.as_str());
        self.insert_project(project)
    }

    pub fn insert_task(&self, task: Task) -> Task {
        self.with_connection(|conn| {
            conn.execute(
                "INSERT INTO tasks (id, project_id, category, title, description, priority, internal_status, needs_review_point, source_proposal_id, plan_artifact_id, ideation_session_id, execution_plan_id, created_at, updated_at, started_at, completed_at, archived_at, blocked_reason, task_branch, task_branch_base_ref, task_branch_base_sha, worktree_path, merge_commit_sha, metadata, merge_pipeline_active)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25)",
                rusqlite::params![
                    task.id.as_str(),
                    task.project_id.as_str(),
                    task.category.to_string(),
                    task.title.as_str(),
                    task.description.as_deref(),
                    task.priority,
                    task.internal_status.as_str(),
                    task.needs_review_point,
                    task.source_proposal_id.as_ref().map(|id| id.as_str()),
                    task.plan_artifact_id.as_ref().map(|id| id.as_str()),
                    task.ideation_session_id.as_ref().map(|id| id.as_str()),
                    task.execution_plan_id.as_ref().map(|id| id.as_str()),
                    task.created_at.to_rfc3339(),
                    task.updated_at.to_rfc3339(),
                    task.started_at.as_ref().map(|dt| dt.to_rfc3339()),
                    task.completed_at.as_ref().map(|dt| dt.to_rfc3339()),
                    task.archived_at.as_ref().map(|dt| dt.to_rfc3339()),
                    task.blocked_reason.as_deref(),
                    task.task_branch.as_deref(),
                    task.task_branch_base_ref.as_deref(),
                    task.task_branch_base_sha.as_deref(),
                    task.worktree_path.as_deref(),
                    task.merge_commit_sha.as_deref(),
                    task.metadata.as_deref(),
                    task.merge_pipeline_active.as_deref(),
                ],
            )
            .expect("Failed to insert test task");
        });
        task
    }

    pub fn seed_task(&self, project_id: ProjectId, title: &str) -> Task {
        self.insert_task(Task::new(project_id, title.to_string()))
    }

    pub fn insert_ideation_session(&self, session: IdeationSession) -> IdeationSession {
        self.with_connection(|conn| {
            conn.execute(
                "INSERT INTO ideation_sessions (id, project_id, title, title_source, status, plan_artifact_id, inherited_plan_artifact_id, seed_task_id, parent_session_id, created_at, updated_at, archived_at, converted_at, verification_status, source_project_id, source_session_id, session_purpose)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
                rusqlite::params![
                    session.id.as_str(),
                    session.project_id.as_str(),
                    session.title.as_deref(),
                    session.title_source.as_deref(),
                    session.status.to_string(),
                    session.plan_artifact_id.as_ref().map(|id| id.as_str()),
                    session.inherited_plan_artifact_id.as_ref().map(|id| id.as_str()),
                    session.seed_task_id.as_ref().map(|id| id.as_str()),
                    session.parent_session_id.as_ref().map(|id| id.as_str()),
                    session.created_at.to_rfc3339(),
                    session.updated_at.to_rfc3339(),
                    session.archived_at.as_ref().map(|dt| dt.to_rfc3339()),
                    session.converted_at.as_ref().map(|dt| dt.to_rfc3339()),
                    session.verification_status.to_string(),
                    session.source_project_id.as_deref(),
                    session.source_session_id.as_deref(),
                    session.session_purpose.to_string(),
                ],
            )
            .expect("Failed to insert test ideation session");
        });
        session
    }

    pub fn seed_ideation_session(&self, project_id: ProjectId) -> IdeationSession {
        self.insert_ideation_session(IdeationSession::new(project_id))
    }

    pub fn insert_conversation(&self, conversation: ChatConversation) -> ChatConversation {
        self.with_connection(|conn| {
            conn.execute(
                "INSERT INTO chat_conversations (
                    id, context_type, context_id, claude_session_id, provider_session_id,
                    provider_harness, upstream_provider, provider_profile, agent_mode, coordination_mode,
                    automation_id, automation_run_id, title, message_count, last_message_at, created_at,
                    updated_at, archived_at, parent_conversation_id, attribution_backfill_status,
                    attribution_backfill_source, attribution_backfill_source_path,
                    attribution_backfill_last_attempted_at, attribution_backfill_completed_at,
                    attribution_backfill_error_summary
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25)",
                rusqlite::params![
                    conversation.id.as_str(),
                    conversation.context_type.to_string(),
                    conversation.context_id.as_str(),
                    conversation.claude_session_id.as_deref(),
                    conversation.provider_session_id.as_deref(),
                    conversation
                        .provider_harness
                        .map(|value| value.to_string()),
                    conversation.upstream_provider.as_deref(),
                    conversation.provider_profile.as_deref(),
                    conversation.agent_mode.map(|value| value.to_string()),
                    conversation.coordination_mode.to_string(),
                    conversation
                        .automation_id
                        .as_ref()
                        .map(|value| value.as_str()),
                    conversation
                        .automation_run_id
                        .as_ref()
                        .map(|value| value.as_str()),
                    conversation.title.as_deref(),
                    conversation.message_count,
                    conversation.last_message_at.as_ref().map(|dt| dt.to_rfc3339()),
                    conversation.created_at.to_rfc3339(),
                    conversation.updated_at.to_rfc3339(),
                    conversation.archived_at.as_ref().map(|dt| dt.to_rfc3339()),
                    conversation.parent_conversation_id.as_deref(),
                    conversation
                        .attribution_backfill_status
                        .map(|value| value.to_string()),
                    conversation.attribution_backfill_source.as_deref(),
                    conversation.attribution_backfill_source_path.as_deref(),
                    conversation
                        .attribution_backfill_last_attempted_at
                        .as_ref()
                        .map(|dt| dt.to_rfc3339()),
                    conversation
                        .attribution_backfill_completed_at
                        .as_ref()
                        .map(|dt| dt.to_rfc3339()),
                    conversation.attribution_backfill_error_summary.as_deref(),
                ],
            )
            .expect("Failed to insert test conversation");
        });
        conversation
    }

    pub fn seed_ideation_conversation(&self) -> ChatConversation {
        self.insert_conversation(ChatConversation::new_ideation(IdeationSessionId::new()))
    }

    pub fn seed_task_conversation(&self, task_id: TaskId) -> ChatConversation {
        self.insert_conversation(ChatConversation::new_task(task_id))
    }

    pub fn insert_review_note(&self, note: ReviewNote) -> ReviewNote {
        self.with_connection(|conn| {
            let issues_json = note
                .issues
                .as_ref()
                .map(|issues| to_string(issues).expect("Failed to serialize review issues"));

            conn.execute(
                "INSERT INTO review_notes (id, task_id, reviewer, outcome, summary, notes, issues, followup_session_id, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                rusqlite::params![
                    note.id.as_str(),
                    note.task_id.as_str(),
                    note.reviewer.to_string(),
                    note.outcome.to_string(),
                    note.summary.as_deref(),
                    note.notes.as_deref(),
                    issues_json.as_deref(),
                    note.followup_session_id.as_deref(),
                    note.created_at.to_rfc3339(),
                ],
            )
            .expect("Failed to insert test review note");
        });
        note
    }
}

pub struct SqliteStateFixture {
    _db: SqliteTestDb,
    state: AppState,
}

impl SqliteStateFixture {
    pub fn new(name: &str, configure: impl FnOnce(&SqliteTestDb, &mut AppState)) -> Self {
        let db = SqliteTestDb::new(name);
        let mut state = AppState::new_test();
        configure(&db, &mut state);
        Self { _db: db, state }
    }

    pub fn db(&self) -> &SqliteTestDb {
        &self._db
    }
}

impl Deref for SqliteStateFixture {
    type Target = AppState;

    fn deref(&self) -> &Self::Target {
        &self.state
    }
}

fn sanitize_name(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect();
    sanitized.trim_matches('-').to_string()
}
