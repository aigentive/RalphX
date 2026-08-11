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

    rewrite_table_check_constraint(
        conn,
        "automations",
        "'ideation_finalized'",
        &[(
            "CHECK (completion_signal IN ('pr_merged','agent_completed'))",
            "CHECK (completion_signal IN ('pr_merged','agent_completed','ideation_finalized'))",
        )],
        "automation ideation completion signal",
    )?;

    conn.execute(
        if legacy_alter_table_was_enabled {
            "PRAGMA legacy_alter_table = ON"
        } else {
            "PRAGMA legacy_alter_table = OFF"
        },
        [],
    )
    .map_err(|error| AppError::Database(error.to_string()))?;
    conn.execute(
        if foreign_keys_was_enabled {
            "PRAGMA foreign_keys = ON"
        } else {
            "PRAGMA foreign_keys = OFF"
        },
        [],
    )
    .map_err(|error| AppError::Database(error.to_string()))?;
    Ok(())
}
