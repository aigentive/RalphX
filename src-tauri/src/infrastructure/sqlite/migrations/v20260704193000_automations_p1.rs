use rusqlite::Connection;

use crate::error::{AppError, AppResult};
use crate::infrastructure::sqlite::migrations::v20260521222911_agent_plan_mode::{
    foreign_keys_enabled, legacy_alter_table_enabled, rewrite_table_check_constraint,
};

use super::helpers;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    create_automation_tables(conn)?;
    add_conversation_ownership_columns(conn)?;
    widen_agent_modes(conn)?;
    Ok(())
}

fn create_automation_tables(conn: &Connection) -> AppResult<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS automations (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            name TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'draft' CHECK (status IN ('draft','active','paused','completed','stopped')),
            paused_reason_code TEXT,
            paused_reason_detail TEXT,
            goal_prompt TEXT NOT NULL DEFAULT '',
            setup_conversation_id TEXT,
            provider_harness TEXT NOT NULL,
            model_id TEXT NOT NULL,
            logical_effort TEXT,
            run_mode TEXT NOT NULL CHECK (run_mode IN ('edit','plan','ideation')),
            base_ref_kind TEXT NOT NULL,
            base_ref TEXT NOT NULL DEFAULT '',
            base_display_name TEXT,
            base_source_pull_request_json TEXT,
            goal_items_json TEXT,
            chain_mode TEXT NOT NULL DEFAULT 'merged_base' CHECK (chain_mode IN ('merged_base','pr_head_stacked')),
            completion_signal TEXT NOT NULL DEFAULT 'pr_merged' CHECK (completion_signal IN ('pr_merged')),
            max_runs INTEGER NOT NULL DEFAULT 25,
            max_consecutive_failures INTEGER NOT NULL DEFAULT 3,
            first_run_prompt TEXT,
            setup_analysis_summary TEXT,
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')),
            updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')),
            FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE,
            FOREIGN KEY (setup_conversation_id) REFERENCES chat_conversations(id) ON DELETE SET NULL
        )",
        [],
    )
    .map_err(|error| AppError::Database(error.to_string()))?;
    helpers::create_index_if_not_exists(
        conn,
        "idx_automations_project",
        "automations",
        "project_id",
    )?;
    helpers::create_index_if_not_exists(conn, "idx_automations_status", "automations", "status")?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS automation_runs (
            id TEXT PRIMARY KEY,
            automation_id TEXT NOT NULL,
            run_index INTEGER NOT NULL,
            status TEXT NOT NULL CHECK (status IN ('pending','provisioning','running','published','merged','pr_closed','agent_failed','cancelled')),
            judge_state TEXT NOT NULL DEFAULT 'none' CHECK (judge_state IN ('none','in_progress','done','failed','skipped')),
            judge_lease_expires_at TEXT,
            conversation_id TEXT,
            run_prompt TEXT NOT NULL,
            prompt_author TEXT NOT NULL CHECK (prompt_author IN ('setup_agent','judge','skip_judge_template')),
            base_ref_kind TEXT NOT NULL,
            base_ref_used TEXT NOT NULL,
            base_from_run_id TEXT,
            branch_name TEXT,
            pr_number INTEGER,
            pr_url TEXT,
            pr_title TEXT,
            pr_head_ref_name TEXT,
            pr_base_ref_name TEXT,
            pr_merged_at TEXT,
            merge_commit_sha TEXT,
            diff_stats_json TEXT,
            agent_summary TEXT,
            judge_verdict_json TEXT,
            judge_model_id TEXT,
            error_code TEXT,
            error_detail TEXT,
            signal_check_failures INTEGER NOT NULL DEFAULT 0,
            started_at TEXT,
            finished_at TEXT,
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')),
            updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')),
            FOREIGN KEY (automation_id) REFERENCES automations(id) ON DELETE CASCADE,
            FOREIGN KEY (conversation_id) REFERENCES chat_conversations(id) ON DELETE SET NULL,
            FOREIGN KEY (base_from_run_id) REFERENCES automation_runs(id) ON DELETE SET NULL,
            UNIQUE (automation_id, run_index)
        )",
        [],
    )
    .map_err(|error| AppError::Database(error.to_string()))?;
    helpers::create_index_if_not_exists(
        conn,
        "idx_automation_runs_automation",
        "automation_runs",
        "automation_id",
    )?;
    helpers::create_index_if_not_exists(
        conn,
        "idx_automation_runs_status",
        "automation_runs",
        "status",
    )?;
    conn.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_automation_runs_single_open
         ON automation_runs(automation_id)
         WHERE status IN ('pending','provisioning','running','published')
            OR (status IN ('merged','pr_closed','agent_failed')
                AND judge_state IN ('none','in_progress','failed'))",
        [],
    )
    .map_err(|error| AppError::Database(error.to_string()))?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS automation_attachments (
            id TEXT PRIMARY KEY,
            automation_id TEXT NOT NULL,
            file_name TEXT NOT NULL,
            file_path TEXT NOT NULL,
            mime_type TEXT,
            file_size INTEGER,
            created_at TEXT NOT NULL,
            FOREIGN KEY (automation_id) REFERENCES automations(id) ON DELETE CASCADE
        )",
        [],
    )
    .map_err(|error| AppError::Database(error.to_string()))?;
    helpers::create_index_if_not_exists(
        conn,
        "idx_automation_attachments_automation",
        "automation_attachments",
        "automation_id",
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS automation_context_refs (
            id TEXT PRIMARY KEY,
            automation_id TEXT NOT NULL,
            ref_kind TEXT NOT NULL CHECK (ref_kind IN ('project','integration','artifact')),
            payload_json TEXT NOT NULL,
            position INTEGER NOT NULL,
            FOREIGN KEY (automation_id) REFERENCES automations(id) ON DELETE CASCADE
        )",
        [],
    )
    .map_err(|error| AppError::Database(error.to_string()))?;
    helpers::create_index_if_not_exists(
        conn,
        "idx_automation_context_refs_automation",
        "automation_context_refs",
        "automation_id, position",
    )?;

    Ok(())
}

fn add_conversation_ownership_columns(conn: &Connection) -> AppResult<()> {
    helpers::add_column_if_not_exists(conn, "chat_conversations", "automation_id", "TEXT NULL")?;
    helpers::add_column_if_not_exists(
        conn,
        "chat_conversations",
        "automation_run_id",
        "TEXT NULL",
    )?;
    helpers::create_index_if_not_exists(
        conn,
        "idx_chat_conversations_automation",
        "chat_conversations",
        "automation_id",
    )?;
    helpers::create_index_if_not_exists(
        conn,
        "idx_chat_conversations_automation_run",
        "chat_conversations",
        "automation_run_id",
    )?;
    Ok(())
}

fn widen_agent_modes(conn: &Connection) -> AppResult<()> {
    let foreign_keys_was_enabled = foreign_keys_enabled(conn)?;
    let legacy_alter_table_was_enabled = legacy_alter_table_enabled(conn)?;
    conn.execute("PRAGMA foreign_keys = OFF", [])
        .map_err(|error| AppError::Database(error.to_string()))?;
    conn.execute("PRAGMA legacy_alter_table = ON", [])
        .map_err(|error| AppError::Database(error.to_string()))?;

    let migrate_result = (|| {
        rewrite_table_check_constraint(
            conn,
            "chat_conversations",
            "'automation'",
            &[
                (
                    "CHECK(agent_mode IN ('chat', 'edit', 'plan', 'ideation', 'review_pr'))",
                    "CHECK(agent_mode IN ('chat', 'edit', 'plan', 'ideation', 'review_pr', 'automation'))",
                ),
                (
                    "CHECK(agent_mode IN ('chat', 'edit', 'ideation', 'plan', 'review_pr'))",
                    "CHECK(agent_mode IN ('chat', 'edit', 'ideation', 'plan', 'review_pr', 'automation'))",
                ),
            ],
            "agent Automation mode",
        )?;
        rewrite_table_check_constraint(
            conn,
            "agent_conversation_workspaces",
            "'automation'",
            &[
                (
                    "CHECK (mode IN ('chat', 'edit', 'plan', 'ideation', 'review_pr'))",
                    "CHECK (mode IN ('chat', 'edit', 'plan', 'ideation', 'review_pr', 'automation'))",
                ),
                (
                    "CHECK (mode IN ('edit', 'chat', 'plan', 'ideation', 'review_pr'))",
                    "CHECK (mode IN ('edit', 'chat', 'plan', 'ideation', 'review_pr', 'automation'))",
                ),
                (
                    "CHECK (mode IN ('edit', 'ideation', 'chat', 'plan', 'review_pr'))",
                    "CHECK (mode IN ('edit', 'ideation', 'chat', 'plan', 'review_pr', 'automation'))",
                ),
            ],
            "agent Automation mode",
        )
    })();

    let restore_legacy_result = conn
        .execute(
            if legacy_alter_table_was_enabled {
                "PRAGMA legacy_alter_table = ON"
            } else {
                "PRAGMA legacy_alter_table = OFF"
            },
            [],
        )
        .map(|_| ())
        .map_err(|error| AppError::Database(error.to_string()));
    let restore_foreign_keys_result = conn
        .execute(
            if foreign_keys_was_enabled {
                "PRAGMA foreign_keys = ON"
            } else {
                "PRAGMA foreign_keys = OFF"
            },
            [],
        )
        .map(|_| ())
        .map_err(|error| AppError::Database(error.to_string()));

    migrate_result?;
    restore_legacy_result?;
    restore_foreign_keys_result?;
    Ok(())
}
