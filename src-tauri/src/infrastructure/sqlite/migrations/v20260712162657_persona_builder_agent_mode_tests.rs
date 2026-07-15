//! Tests for migration v20260712162657: persona builder agent mode

use rusqlite::Connection;

use super::v20260712162657_persona_builder_agent_mode;

fn setup_post_automation_schema() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute_batch(
        "CREATE TABLE chat_conversations (
            id TEXT PRIMARY KEY,
            context_type TEXT NOT NULL,
            context_id TEXT NOT NULL,
            agent_mode TEXT CHECK(agent_mode IN ('chat', 'edit', 'plan', 'ideation', 'review_pr', 'automation'))
         );

         CREATE INDEX idx_chat_conversations_context
            ON chat_conversations(context_type, context_id);

         CREATE TABLE agent_conversation_workspaces (
            conversation_id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            mode TEXT NOT NULL CHECK (mode IN ('chat', 'edit', 'plan', 'ideation', 'review_pr', 'automation')),
            base_ref_kind TEXT NOT NULL,
            base_ref TEXT NOT NULL,
            branch_name TEXT NOT NULL,
            worktree_path TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'archived', 'missing')),
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            FOREIGN KEY(conversation_id) REFERENCES chat_conversations(id) ON DELETE CASCADE
         );

         CREATE INDEX idx_agent_conversation_workspaces_project
            ON agent_conversation_workspaces(project_id);

         INSERT INTO chat_conversations (id, context_type, context_id, agent_mode)
         VALUES ('conversation-edit', 'project', 'project-1', 'edit');

         INSERT INTO agent_conversation_workspaces (
            conversation_id,
            project_id,
            mode,
            base_ref_kind,
            base_ref,
            branch_name,
            worktree_path,
            created_at,
            updated_at
         )
         VALUES (
            'conversation-edit',
            'project-1',
            'edit',
            'project_default',
            'main',
            'ralphx/project/agent-conversation-edit',
            '/tmp/agent-conversation-edit',
            '2026-07-12T16:26:00Z',
            '2026-07-12T16:26:00Z'
         );",
    )
    .expect("create post-automation schema");
    conn
}

fn insert_persona_builder_conversation(conn: &Connection, id: &str) -> rusqlite::Result<usize> {
    conn.execute(
        "INSERT INTO chat_conversations (id, context_type, context_id, agent_mode)
         VALUES (?1, 'project', 'project-1', 'persona_builder')",
        [id],
    )
}

fn insert_persona_builder_workspace(
    conn: &Connection,
    conversation_id: &str,
) -> rusqlite::Result<usize> {
    conn.execute(
        "INSERT INTO agent_conversation_workspaces (
            conversation_id,
            project_id,
            mode,
            base_ref_kind,
            base_ref,
            branch_name,
            worktree_path,
            created_at,
            updated_at
        )
        VALUES (?1, 'project-1', 'persona_builder', 'project_default', 'main',
                'ralphx/project/agent-conversation-pb', '/tmp/agent-conversation-pb',
                '2026-07-12T16:26:00Z', '2026-07-12T16:26:00Z')",
        [conversation_id],
    )
}

#[test]
fn migration_allows_persona_builder_agent_and_workspace_modes() {
    let conn = setup_post_automation_schema();

    assert!(
        insert_persona_builder_conversation(&conn, "conversation-pb-before").is_err(),
        "persona_builder must be rejected before the migration runs"
    );

    v20260712162657_persona_builder_agent_mode::migrate(&conn).unwrap();

    insert_persona_builder_conversation(&conn, "conversation-pb").unwrap();
    insert_persona_builder_workspace(&conn, "conversation-pb").unwrap();

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM chat_conversations WHERE agent_mode = 'persona_builder'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn migration_preserves_existing_rows_and_still_rejects_unknown_modes() {
    let conn = setup_post_automation_schema();

    v20260712162657_persona_builder_agent_mode::migrate(&conn).unwrap();

    let existing: String = conn
        .query_row(
            "SELECT agent_mode FROM chat_conversations WHERE id = 'conversation-edit'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(existing, "edit");

    let workspace_mode: String = conn
        .query_row(
            "SELECT mode FROM agent_conversation_workspaces WHERE conversation_id = 'conversation-edit'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(workspace_mode, "edit");

    assert!(
        conn.execute(
            "INSERT INTO chat_conversations (id, context_type, context_id, agent_mode)
             VALUES ('conversation-bogus', 'project', 'project-1', 'bogus_mode')",
            [],
        )
        .is_err(),
        "unknown agent modes must remain rejected after widening"
    );
}

#[test]
fn migration_is_idempotent() {
    let conn = setup_post_automation_schema();

    v20260712162657_persona_builder_agent_mode::migrate(&conn).unwrap();
    v20260712162657_persona_builder_agent_mode::migrate(&conn).unwrap();

    insert_persona_builder_conversation(&conn, "conversation-pb-idempotent").unwrap();
    insert_persona_builder_workspace(&conn, "conversation-pb-idempotent").unwrap();
}
