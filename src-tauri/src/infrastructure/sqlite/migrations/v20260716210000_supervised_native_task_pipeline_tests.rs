use rusqlite::Connection;

use super::v20260716210000_supervised_native_task_pipeline;

fn legacy_db() -> Connection {
    let conn = Connection::open_in_memory().expect("open database");
    conn.execute_batch(
        "CREATE TABLE chat_conversations (
            id TEXT PRIMARY KEY,
            agent_mode TEXT CHECK(agent_mode IN ('chat', 'edit', 'plan', 'ideation', 'review_pr', 'automation', 'persona_builder'))
         );
         CREATE TABLE agent_conversation_workspaces (
            conversation_id TEXT PRIMARY KEY,
            mode TEXT NOT NULL CHECK (mode IN ('chat', 'edit', 'plan', 'ideation', 'review_pr', 'automation', 'persona_builder')),
            linked_ideation_session_id TEXT NULL
         );
         CREATE TABLE ui_feature_flag_overrides (
            id INTEGER PRIMARY KEY CHECK(id = 1),
            agent_personas INTEGER NULL,
            agent_conversation_team INTEGER NOT NULL DEFAULT 0,
            agent_conversation_workflows INTEGER NOT NULL DEFAULT 0
         );
         INSERT INTO ui_feature_flag_overrides VALUES (1, NULL, 1, 1);
         INSERT INTO chat_conversations VALUES ('linked', 'ideation');
         INSERT INTO chat_conversations VALUES ('unlinked', 'ideation');
         INSERT INTO agent_conversation_workspaces VALUES ('linked', 'ideation', 'session-1');
         INSERT INTO agent_conversation_workspaces VALUES ('unlinked', 'ideation', NULL);",
    )
    .expect("create legacy schema");
    conn
}

#[test]
fn migration_backfills_legacy_modes_attachment_and_default_off_capability() {
    let conn = legacy_db();
    v20260716210000_supervised_native_task_pipeline::migrate(&conn).expect("migration succeeds");

    let linked: (String, Option<String>) = conn
        .query_row(
            "SELECT mode, task_pipeline_session_id
             FROM agent_conversation_workspaces WHERE conversation_id = 'linked'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("linked workspace");
    let unlinked: String = conn
        .query_row(
            "SELECT mode FROM agent_conversation_workspaces
             WHERE conversation_id = 'unlinked'",
            [],
            |row| row.get(0),
        )
        .expect("unlinked workspace");
    let modes: (String, String) = conn
        .query_row(
            "SELECT
                (SELECT agent_mode FROM chat_conversations WHERE id = 'linked'),
                (SELECT agent_mode FROM chat_conversations WHERE id = 'unlinked')",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("conversation modes");
    let autopilot: i64 = conn
        .query_row(
            "SELECT agent_conversation_autopilot FROM ui_feature_flag_overrides WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .expect("autopilot capability");

    assert_eq!(linked, ("tasks".to_string(), Some("session-1".to_string())));
    assert_eq!(unlinked, "autopilot");
    assert_eq!(modes, ("tasks".to_string(), "autopilot".to_string()));
    assert_eq!(autopilot, 0);
}

#[test]
fn migration_is_idempotent_and_accepts_new_modes() {
    let conn = legacy_db();
    v20260716210000_supervised_native_task_pipeline::migrate(&conn).expect("first run");
    v20260716210000_supervised_native_task_pipeline::migrate(&conn).expect("second run");

    conn.execute(
        "INSERT INTO chat_conversations VALUES ('tasks-new', 'tasks')",
        [],
    )
    .expect("tasks mode accepted");
    conn.execute(
        "INSERT INTO agent_conversation_workspaces
         (conversation_id, mode, linked_ideation_session_id, task_pipeline_session_id)
         VALUES ('autopilot-new', 'autopilot', NULL, NULL)",
        [],
    )
    .expect("autopilot mode accepted");
}
