// Migration v20260715181627: agent conversation capabilities

use rusqlite::Connection;

use crate::error::{AppError, AppResult};

use super::helpers::add_column_if_not_exists;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    widen_conversation_coordination_modes(conn)?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS ui_feature_flag_overrides (
            id INTEGER PRIMARY KEY CHECK(id = 1),
            agent_personas INTEGER NULL,
            agent_conversation_team INTEGER NOT NULL DEFAULT 0,
            agent_conversation_workflows INTEGER NOT NULL DEFAULT 0
         );",
    )?;
    add_column_if_not_exists(
        conn,
        "ui_feature_flag_overrides",
        "agent_conversation_team",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    add_column_if_not_exists(
        conn,
        "ui_feature_flag_overrides",
        "agent_conversation_workflows",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO ui_feature_flag_overrides (
            id, agent_personas, agent_conversation_team, agent_conversation_workflows
         ) VALUES (1, NULL, 0, 0)",
        [],
    )?;
    Ok(())
}

fn widen_conversation_coordination_modes(conn: &Connection) -> AppResult<()> {
    let table_exists = conn
        .query_row(
            "SELECT EXISTS(
                SELECT 1 FROM sqlite_master
                WHERE type = 'table' AND name = 'chat_conversations'
             )",
            [],
            |row| row.get::<_, bool>(0),
        )
        .map_err(|error| AppError::Database(error.to_string()))?;
    if !table_exists {
        return Ok(());
    }

    let foreign_keys_was_enabled =
        super::v20260521222911_agent_plan_mode::foreign_keys_enabled(conn)?;
    let legacy_alter_table_was_enabled =
        super::v20260521222911_agent_plan_mode::legacy_alter_table_enabled(conn)?;
    conn.execute("PRAGMA foreign_keys = OFF", [])
        .map_err(|error| AppError::Database(error.to_string()))?;
    conn.execute("PRAGMA legacy_alter_table = ON", [])
        .map_err(|error| AppError::Database(error.to_string()))?;

    let rewrite_result =
        super::v20260521222911_agent_plan_mode::rewrite_table_check_constraint(
            conn,
            "chat_conversations",
            "'rx_native_workflow'",
            &[
                (
                    "CHECK(coordination_mode IN ('solo', 'legacy_claude_team', 'rx_native_team'))",
                    "CHECK(coordination_mode IN ('solo', 'legacy_claude_team', 'rx_native_team', 'rx_native_workflow', 'codex_native_ultra'))",
                ),
                (
                    "CHECK (coordination_mode IN ('solo', 'legacy_claude_team', 'rx_native_team'))",
                    "CHECK (coordination_mode IN ('solo', 'legacy_claude_team', 'rx_native_team', 'rx_native_workflow', 'codex_native_ultra'))",
                ),
            ],
            "agent conversation capability",
        );

    let legacy_restore_result = conn
        .execute(
            if legacy_alter_table_was_enabled {
                "PRAGMA legacy_alter_table = ON"
            } else {
                "PRAGMA legacy_alter_table = OFF"
            },
            [],
        )
        .map_err(|error| AppError::Database(error.to_string()));
    let foreign_key_restore_result = conn
        .execute(
            if foreign_keys_was_enabled {
                "PRAGMA foreign_keys = ON"
            } else {
                "PRAGMA foreign_keys = OFF"
            },
            [],
        )
        .map_err(|error| AppError::Database(error.to_string()));

    rewrite_result?;
    legacy_restore_result?;
    foreign_key_restore_result?;
    Ok(())
}
