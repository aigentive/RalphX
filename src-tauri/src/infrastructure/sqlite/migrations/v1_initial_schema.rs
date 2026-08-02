// Migration v1: Initial schema
//
// This migration creates the complete database schema.
// Generated from production database dump on 2026-01-30.
//
// For existing databases (schema version < 1), this creates all tables.
// For fresh databases, this sets up the complete schema in one go.

use rusqlite::Connection;

use crate::error::{AppError, AppResult};

/// Migration v1: Create complete initial schema
pub fn migrate(conn: &Connection) -> AppResult<()> {
    // ==========================================================================
    // Core tables: Projects and Tasks
    // ==========================================================================

    conn.execute(
        "CREATE TABLE IF NOT EXISTS projects (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            working_directory TEXT NOT NULL,
            git_mode TEXT NOT NULL DEFAULT 'local',
            worktree_path TEXT,
            worktree_branch TEXT,
            base_branch TEXT,
            created_at TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')),
            updated_at TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))
        )",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS tasks (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            category TEXT NOT NULL DEFAULT 'feature',
            title TEXT NOT NULL,
            description TEXT,
            priority INTEGER DEFAULT 0,
            internal_status TEXT NOT NULL DEFAULT 'backlog',
            created_at TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')),
            updated_at TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')),
            started_at DATETIME,
            completed_at DATETIME,
            needs_qa BOOLEAN DEFAULT NULL,
            qa_prep_status TEXT DEFAULT 'pending',
            qa_test_status TEXT DEFAULT 'pending',
            needs_review_point INTEGER DEFAULT 0,
            external_status TEXT,
            wave INTEGER,
            checkpoint_type TEXT,
            phase_id TEXT,
            plan_id TEXT,
            must_haves_json TEXT,
            source_proposal_id TEXT,
            plan_artifact_id TEXT,
            archived_at TEXT
        )",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_tasks_project_id ON tasks(project_id)",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_tasks_internal_status ON tasks(internal_status)",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_tasks_wave ON tasks(wave)",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_tasks_external_status ON tasks(external_status)",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_tasks_archived ON tasks(project_id, archived_at)",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    // ==========================================================================
    // Task relationships: Dependencies, Blockers, Steps
    // ==========================================================================

    conn.execute(
        "CREATE TABLE IF NOT EXISTS task_dependencies (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
            depends_on_task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
            created_at TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')),
            UNIQUE(task_id, depends_on_task_id),
            CHECK(task_id != depends_on_task_id)
        )",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_task_dependencies_task_id ON task_dependencies(task_id)",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_task_dependencies_depends_on ON task_dependencies(depends_on_task_id)",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS task_blockers (
            task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
            blocker_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
            created_at TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')),
            PRIMARY KEY (task_id, blocker_id)
        )",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_task_blockers_task_id ON task_blockers(task_id)",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_task_blockers_blocker_id ON task_blockers(blocker_id)",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS task_steps (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
            title TEXT NOT NULL,
            description TEXT,
            status TEXT NOT NULL DEFAULT 'pending',
            sort_order INTEGER NOT NULL DEFAULT 0,
            depends_on TEXT REFERENCES task_steps(id) ON DELETE SET NULL,
            created_by TEXT NOT NULL DEFAULT 'user',
            completion_note TEXT,
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')),
            updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')),
            started_at TEXT,
            completed_at TEXT
        )",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_task_steps_task_id ON task_steps(task_id)",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_task_steps_task_order ON task_steps(task_id, sort_order)",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    // ==========================================================================
    // Task state tracking: History and State Data
    // ==========================================================================

    conn.execute(
        "CREATE TABLE IF NOT EXISTS task_state_history (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
            from_status TEXT,
            to_status TEXT NOT NULL,
            changed_by TEXT NOT NULL,
            reason TEXT,
            metadata JSON,
            created_at TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))
        )",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_task_state_history_task_id ON task_state_history(task_id)",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS task_state_data (
            task_id TEXT PRIMARY KEY REFERENCES tasks(id) ON DELETE CASCADE,
            state_type TEXT NOT NULL,
            data TEXT NOT NULL,
            updated_at TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))
        )",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_task_state_data_state_type ON task_state_data(state_type)",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    // ==========================================================================
    // QA System: Task QA tracking
    // ==========================================================================

    conn.execute(
        "CREATE TABLE IF NOT EXISTS task_qa (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
            acceptance_criteria TEXT,
            qa_test_steps TEXT,
            prep_agent_id TEXT,
            prep_started_at DATETIME,
            prep_completed_at DATETIME,
            actual_implementation TEXT,
            refined_test_steps TEXT,
            refinement_agent_id TEXT,
            refinement_completed_at DATETIME,
            test_results TEXT,
            screenshots TEXT,
            test_agent_id TEXT,
            test_completed_at DATETIME,
            created_at TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))
        )",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_task_qa_task_id ON task_qa(task_id)",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    // ==========================================================================
    // Review System: Reviews, Actions, Notes, Settings
    // ==========================================================================

    conn.execute(
        "CREATE TABLE IF NOT EXISTS reviews (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL REFERENCES projects(id),
            task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
            reviewer_type TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'pending',
            notes TEXT,
            created_at TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')),
            completed_at DATETIME
        )",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_reviews_task_id ON reviews(task_id)",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_reviews_project_id ON reviews(project_id)",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_reviews_status ON reviews(status)",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS review_actions (
            id TEXT PRIMARY KEY,
            review_id TEXT NOT NULL REFERENCES reviews(id) ON DELETE CASCADE,
            action_type TEXT NOT NULL,
            target_task_id TEXT,
            created_at TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))
        )",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_review_actions_review_id ON review_actions(review_id)",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_review_actions_target_task_id ON review_actions(target_task_id)",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS review_notes (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
            reviewer TEXT NOT NULL,
            outcome TEXT NOT NULL,
            notes TEXT,
            created_at TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))
        )",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_review_notes_task_id ON review_notes(task_id)",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS review_settings (
            id INTEGER PRIMARY KEY DEFAULT 1 CHECK (id = 1),
            ai_review_enabled INTEGER NOT NULL DEFAULT 1,
            ai_review_auto_fix INTEGER NOT NULL DEFAULT 1,
            require_fix_approval INTEGER NOT NULL DEFAULT 0,
            require_human_review INTEGER NOT NULL DEFAULT 0,
            require_workspace_review INTEGER NOT NULL DEFAULT 1,
            autofix_workspace_review_blocking_findings INTEGER NOT NULL DEFAULT 1,
            workspace_review_fixer_cycle_cap INTEGER NOT NULL DEFAULT 3,
            run_task_validations INTEGER NOT NULL DEFAULT 1,
            max_fix_attempts INTEGER NOT NULL DEFAULT 3,
            max_revision_cycles INTEGER NOT NULL DEFAULT 5,
            updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))
        )",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    // Seed default review settings
    conn.execute(
        "INSERT OR IGNORE INTO review_settings (id, updated_at) VALUES (1, strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    // ==========================================================================
    // Ideation System: Sessions, Proposals, Dependencies, Settings
    // ==========================================================================

    conn.execute(
        "CREATE TABLE IF NOT EXISTS ideation_sessions (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            title TEXT,
            status TEXT NOT NULL DEFAULT 'active',
            created_at TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')),
            updated_at TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')),
            archived_at DATETIME,
            converted_at DATETIME,
            plan_artifact_id TEXT,
            seed_task_id TEXT
        )",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_ideation_sessions_project_id ON ideation_sessions(project_id)",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_ideation_sessions_status ON ideation_sessions(status)",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS task_proposals (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL REFERENCES ideation_sessions(id) ON DELETE CASCADE,
            title TEXT NOT NULL,
            description TEXT,
            category TEXT NOT NULL,
            steps TEXT,
            acceptance_criteria TEXT,
            suggested_priority TEXT NOT NULL,
            priority_score INTEGER NOT NULL DEFAULT 50,
            priority_reason TEXT,
            priority_factors TEXT,
            estimated_complexity TEXT DEFAULT 'moderate',
            user_priority TEXT,
            user_modified INTEGER DEFAULT 0,
            status TEXT NOT NULL DEFAULT 'pending',
            selected INTEGER DEFAULT 0,
            created_task_id TEXT REFERENCES tasks(id),
            sort_order INTEGER NOT NULL DEFAULT 0,
            created_at TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')),
            updated_at TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')),
            plan_artifact_id TEXT,
            plan_version_at_creation INTEGER
        )",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_task_proposals_session_id ON task_proposals(session_id)",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_task_proposals_sort_order ON task_proposals(session_id, sort_order)",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS proposal_dependencies (
            id TEXT PRIMARY KEY,
            proposal_id TEXT NOT NULL REFERENCES task_proposals(id) ON DELETE CASCADE,
            depends_on_proposal_id TEXT NOT NULL REFERENCES task_proposals(id) ON DELETE CASCADE,
            created_at TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')),
            UNIQUE(proposal_id, depends_on_proposal_id),
            CHECK(proposal_id != depends_on_proposal_id)
        )",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_proposal_dependencies_proposal_id ON proposal_dependencies(proposal_id)",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_proposal_dependencies_depends_on ON proposal_dependencies(depends_on_proposal_id)",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS ideation_settings (
            id INTEGER PRIMARY KEY DEFAULT 1 CHECK (id = 1),
            plan_mode TEXT NOT NULL DEFAULT 'optional',
            require_plan_approval INTEGER NOT NULL DEFAULT 0,
            suggest_plans_for_complex INTEGER NOT NULL DEFAULT 1,
            auto_link_proposals INTEGER NOT NULL DEFAULT 1,
            updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))
        )",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    // Seed default ideation settings
    conn.execute(
        "INSERT OR IGNORE INTO ideation_settings (id, updated_at) VALUES (1, strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    // ==========================================================================
    // Chat System: Conversations, Messages, Agent Runs
    // ==========================================================================

    conn.execute(
        "CREATE TABLE IF NOT EXISTS chat_conversations (
            id TEXT PRIMARY KEY,
            context_type TEXT NOT NULL,
            context_id TEXT NOT NULL,
            claude_session_id TEXT,
            title TEXT,
            message_count INTEGER NOT NULL DEFAULT 0,
            last_message_at TEXT,
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')),
            updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))
        )",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_chat_conversations_context ON chat_conversations(context_type, context_id)",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_chat_conversations_claude_session ON chat_conversations(claude_session_id)",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS chat_messages (
            id TEXT PRIMARY KEY,
            session_id TEXT REFERENCES ideation_sessions(id) ON DELETE CASCADE,
            project_id TEXT REFERENCES projects(id) ON DELETE CASCADE,
            task_id TEXT REFERENCES tasks(id) ON DELETE CASCADE,
            conversation_id TEXT REFERENCES chat_conversations(id) ON DELETE CASCADE,
            role TEXT NOT NULL,
            content TEXT NOT NULL,
            metadata TEXT,
            tool_calls TEXT,
            content_blocks TEXT,
            parent_message_id TEXT REFERENCES chat_messages(id),
            created_at TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))
        )",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_chat_messages_session_id ON chat_messages(session_id)",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_chat_messages_project_id ON chat_messages(project_id)",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_chat_messages_task_id ON chat_messages(task_id)",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_chat_messages_conversation ON chat_messages(conversation_id)",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS agent_runs (
            id TEXT PRIMARY KEY,
            conversation_id TEXT NOT NULL REFERENCES chat_conversations(id) ON DELETE CASCADE,
            status TEXT NOT NULL DEFAULT 'running',
            started_at TEXT NOT NULL,
            completed_at TEXT,
            error_message TEXT
        )",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_agent_runs_conversation ON agent_runs(conversation_id)",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_agent_runs_status ON agent_runs(status)",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    // Trigger to update conversation message count
    conn.execute(
        "CREATE TRIGGER IF NOT EXISTS update_conversation_message_count
        AFTER INSERT ON chat_messages
        FOR EACH ROW
        WHEN NEW.conversation_id IS NOT NULL
        BEGIN
            UPDATE chat_conversations
            SET message_count = message_count + 1,
                last_message_at = NEW.created_at,
                updated_at = strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')
            WHERE id = NEW.conversation_id;
        END",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    // ==========================================================================
    // Artifact System: Buckets, Artifacts, Relations, Flows
    // ==========================================================================

    conn.execute(
        "CREATE TABLE IF NOT EXISTS artifact_buckets (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            config_json TEXT NOT NULL,
            is_system INTEGER DEFAULT 0,
            created_at TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))
        )",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_artifact_buckets_is_system ON artifact_buckets(is_system)",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS artifacts (
            id TEXT PRIMARY KEY,
            type TEXT NOT NULL,
            name TEXT NOT NULL,
            content_type TEXT NOT NULL,
            content_text TEXT,
            content_path TEXT,
            bucket_id TEXT REFERENCES artifact_buckets(id),
            task_id TEXT REFERENCES tasks(id),
            process_id TEXT,
            created_by TEXT NOT NULL,
            version INTEGER DEFAULT 1,
            previous_version_id TEXT REFERENCES artifacts(id),
            metadata_json TEXT,
            created_at TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))
        )",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_artifacts_bucket ON artifacts(bucket_id)",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_artifacts_type ON artifacts(type)",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_artifacts_task ON artifacts(task_id)",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS artifact_relations (
            id TEXT PRIMARY KEY,
            from_artifact_id TEXT NOT NULL REFERENCES artifacts(id) ON DELETE CASCADE,
            to_artifact_id TEXT NOT NULL REFERENCES artifacts(id) ON DELETE CASCADE,
            relation_type TEXT NOT NULL,
            created_at TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')),
            UNIQUE(from_artifact_id, to_artifact_id, relation_type)
        )",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_artifact_relations_from ON artifact_relations(from_artifact_id)",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_artifact_relations_to ON artifact_relations(to_artifact_id)",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS artifact_flows (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            trigger_json TEXT NOT NULL,
            steps_json TEXT NOT NULL,
            is_active INTEGER DEFAULT 1,
            created_at TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))
        )",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_artifact_flows_active ON artifact_flows(is_active)",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    // ==========================================================================
    // Supporting tables: Workflows, Processes, Methodology, Agent Profiles
    // ==========================================================================

    conn.execute(
        "CREATE TABLE IF NOT EXISTS workflows (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            description TEXT,
            schema_json TEXT NOT NULL,
            is_default INTEGER DEFAULT 0,
            created_at TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))
        )",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_workflows_is_default ON workflows(is_default)",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS processes (
            id TEXT PRIMARY KEY,
            type TEXT NOT NULL,
            name TEXT NOT NULL,
            config_json TEXT NOT NULL,
            status TEXT NOT NULL,
            current_iteration INTEGER DEFAULT 0,
            created_at TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')),
            started_at DATETIME,
            completed_at DATETIME
        )",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_processes_status ON processes(status)",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_processes_type ON processes(type)",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS methodology_extensions (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            description TEXT,
            config_json TEXT NOT NULL,
            is_active INTEGER DEFAULT 0,
            created_at TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))
        )",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_methodology_extensions_active ON methodology_extensions(is_active)",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS agent_profiles (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL UNIQUE,
            role TEXT NOT NULL,
            profile_json TEXT NOT NULL,
            is_builtin INTEGER NOT NULL DEFAULT 0,
            created_at TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')),
            updated_at TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))
        )",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_agent_profiles_role ON agent_profiles(role)",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_agent_profiles_is_builtin ON agent_profiles(is_builtin)",
        [],
    )
    .map_err(|e| AppError::Database(e.to_string()))?;

    Ok(())
}
