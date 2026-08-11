//! Tests for migration v20260713131052: disable auto followup by default

use super::{
    get_schema_version, run_migrations, run_migrations_through,
    v20260713131052_disable_auto_followup_by_default, SCHEMA_VERSION,
};
use crate::infrastructure::sqlite::open_memory_connection;

const PREVIOUS_SCHEMA_VERSION: i64 = 20260712153932;

#[test]
fn migration_resets_existing_auto_followup_policy_without_changing_other_settings() {
    let conn = open_memory_connection().expect("create memory db");
    run_migrations_through(&conn, PREVIOUS_SCHEMA_VERSION).expect("run prior migrations");
    conn.execute(
        "UPDATE review_settings
         SET ai_review_enabled = 0,
             max_fix_attempts = 7,
             auto_create_followup_agent_conversation = 1
         WHERE id = 1",
        [],
    )
    .expect("seed prior autonomy policy");

    v20260713131052_disable_auto_followup_by_default::migrate(&conn)
        .expect("disable auto followup");

    let settings: (i64, i64, i64) = conn
        .query_row(
            "SELECT ai_review_enabled, max_fix_attempts, auto_create_followup_agent_conversation
             FROM review_settings WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("read migrated review settings");
    assert_eq!(settings, (0, 7, 0));
}

#[test]
fn migration_runner_resets_once_and_preserves_a_later_manual_opt_in() {
    let conn = open_memory_connection().expect("create memory db");
    run_migrations_through(&conn, PREVIOUS_SCHEMA_VERSION).expect("run prior migrations");
    conn.execute(
        "UPDATE review_settings SET auto_create_followup_agent_conversation = 1 WHERE id = 1",
        [],
    )
    .expect("seed enabled prior autonomy policy");

    run_migrations(&conn).expect("run upgrade migration");
    let after_upgrade: i64 = conn
        .query_row(
            "SELECT auto_create_followup_agent_conversation FROM review_settings WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .expect("read upgraded autonomy policy");
    assert_eq!(after_upgrade, 0);
    assert_eq!(get_schema_version(&conn).unwrap(), SCHEMA_VERSION);

    conn.execute(
        "UPDATE review_settings SET auto_create_followup_agent_conversation = 1 WHERE id = 1",
        [],
    )
    .expect("manually enable autonomy policy");

    run_migrations(&conn).expect("restart after manual opt-in");
    let after_restart: i64 = conn
        .query_row(
            "SELECT auto_create_followup_agent_conversation FROM review_settings WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .expect("read persisted manual opt-in");
    assert_eq!(after_restart, 1);
}
