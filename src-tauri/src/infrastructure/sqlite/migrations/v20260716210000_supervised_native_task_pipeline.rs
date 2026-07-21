// Migration v20260716210000: supervised native task pipeline modes and attachment

use rusqlite::Connection;

use crate::error::{AppError, AppResult};

use super::helpers::{add_column_if_not_exists, table_exists};
use super::v20260521222911_agent_plan_mode::{
    foreign_keys_enabled, legacy_alter_table_enabled, rewrite_table_check_constraint,
};

pub fn migrate(conn: &Connection) -> AppResult<()> {
    if table_exists(conn, "agent_conversation_workspaces") {
        add_column_if_not_exists(
            conn,
            "agent_conversation_workspaces",
            "task_pipeline_session_id",
            "TEXT NULL",
        )?;
    }
    if table_exists(conn, "ui_feature_flag_overrides") {
        add_column_if_not_exists(
            conn,
            "ui_feature_flag_overrides",
            "agent_conversation_autopilot",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
    }
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS agent_task_pipeline_append_replays (
            session_id TEXT NOT NULL,
            source_conversation_id TEXT NOT NULL,
            source_message_id TEXT NOT NULL,
            task_id TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            PRIMARY KEY(session_id, source_conversation_id, source_message_id)
         );",
    )?;

    widen_mode_constraints(conn)?;
    backfill_legacy_modes(conn)?;
    Ok(())
}

fn widen_mode_constraints(conn: &Connection) -> AppResult<()> {
    let foreign_keys_was_enabled = foreign_keys_enabled(conn)?;
    let legacy_alter_table_was_enabled = legacy_alter_table_enabled(conn)?;
    conn.execute("PRAGMA foreign_keys = OFF", [])
        .map_err(|error| AppError::Database(error.to_string()))?;
    conn.execute("PRAGMA legacy_alter_table = ON", [])
        .map_err(|error| AppError::Database(error.to_string()))?;

    let result: AppResult<()> = (|| {
        if table_exists(conn, "chat_conversations") {
            rewrite_table_check_constraint(
                conn,
                "chat_conversations",
                "'tasks'",
                &[(
                    "CHECK(agent_mode IN ('chat', 'edit', 'plan', 'ideation', 'review_pr', 'automation', 'persona_builder'))",
                    "CHECK(agent_mode IN ('chat', 'edit', 'plan', 'tasks', 'autopilot', 'ideation', 'review_pr', 'automation', 'persona_builder'))",
                )],
                "supervised native task pipeline modes",
            )?;
        }
        if table_exists(conn, "agent_conversation_workspaces") {
            rewrite_table_check_constraint(
                conn,
                "agent_conversation_workspaces",
                "'tasks'",
                &[(
                    "CHECK (mode IN ('chat', 'edit', 'plan', 'ideation', 'review_pr', 'automation', 'persona_builder'))",
                    "CHECK (mode IN ('chat', 'edit', 'plan', 'tasks', 'autopilot', 'ideation', 'review_pr', 'automation', 'persona_builder'))",
                )],
                "supervised native task pipeline modes",
            )?;
        }
        Ok(())
    })();

    let restore_legacy = conn
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
    let restore_foreign_keys = conn
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

    result?;
    restore_legacy?;
    restore_foreign_keys?;
    Ok(())
}

fn backfill_legacy_modes(conn: &Connection) -> AppResult<()> {
    if !table_exists(conn, "agent_conversation_workspaces") {
        return Ok(());
    }
    conn.execute(
        "UPDATE agent_conversation_workspaces
         SET task_pipeline_session_id = linked_ideation_session_id,
             mode = CASE
                 WHEN linked_ideation_session_id IS NOT NULL THEN 'tasks'
                 ELSE 'autopilot'
             END
         WHERE mode = 'ideation'",
        [],
    )?;
    if table_exists(conn, "chat_conversations") {
        conn.execute(
            "UPDATE chat_conversations
             SET agent_mode = (
                 SELECT workspace.mode
                 FROM agent_conversation_workspaces workspace
                 WHERE workspace.conversation_id = chat_conversations.id
             )
             WHERE id IN (
                 SELECT conversation_id FROM agent_conversation_workspaces
                 WHERE mode IN ('tasks', 'autopilot')
             )
             AND agent_mode = 'ideation'",
            [],
        )?;
    }
    Ok(())
}
