// Migration v20260712162657: widen agent-mode CHECK constraints for 'persona_builder'
//
// The persona system introduced the PersonaBuilder conversation mode, but the
// CHECK constraints on chat_conversations.agent_mode and
// agent_conversation_workspaces.mode (last widened for 'automation') were not
// extended, so starting a PersonaBuilder conversation failed with
// "CHECK constraint failed".

use rusqlite::Connection;

use crate::error::{AppError, AppResult};
use crate::infrastructure::sqlite::migrations::v20260521222911_agent_plan_mode::{
    foreign_keys_enabled, legacy_alter_table_enabled, rewrite_table_check_constraint,
};

pub fn migrate(conn: &Connection) -> AppResult<()> {
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
            "'persona_builder'",
            &[
                (
                    "CHECK(agent_mode IN ('chat', 'edit', 'plan', 'ideation', 'review_pr', 'automation'))",
                    "CHECK(agent_mode IN ('chat', 'edit', 'plan', 'ideation', 'review_pr', 'automation', 'persona_builder'))",
                ),
                (
                    "CHECK(agent_mode IN ('chat', 'edit', 'ideation', 'plan', 'review_pr', 'automation'))",
                    "CHECK(agent_mode IN ('chat', 'edit', 'ideation', 'plan', 'review_pr', 'automation', 'persona_builder'))",
                ),
            ],
            "agent PersonaBuilder mode",
        )?;
        rewrite_table_check_constraint(
            conn,
            "agent_conversation_workspaces",
            "'persona_builder'",
            &[
                (
                    "CHECK (mode IN ('chat', 'edit', 'plan', 'ideation', 'review_pr', 'automation'))",
                    "CHECK (mode IN ('chat', 'edit', 'plan', 'ideation', 'review_pr', 'automation', 'persona_builder'))",
                ),
                (
                    "CHECK (mode IN ('edit', 'chat', 'plan', 'ideation', 'review_pr', 'automation'))",
                    "CHECK (mode IN ('edit', 'chat', 'plan', 'ideation', 'review_pr', 'automation', 'persona_builder'))",
                ),
                (
                    "CHECK (mode IN ('edit', 'ideation', 'chat', 'plan', 'review_pr', 'automation'))",
                    "CHECK (mode IN ('edit', 'ideation', 'chat', 'plan', 'review_pr', 'automation', 'persona_builder'))",
                ),
            ],
            "agent PersonaBuilder mode",
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
