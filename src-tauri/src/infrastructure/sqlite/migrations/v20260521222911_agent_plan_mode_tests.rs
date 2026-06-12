//! Tests for migration v20260521222911: agent plan mode

use rusqlite::Connection;

use super::v20260521222911_agent_plan_mode;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute_batch(
        "CREATE TABLE chat_conversations (
            id TEXT PRIMARY KEY,
            context_type TEXT NOT NULL,
            context_id TEXT NOT NULL,
            agent_mode TEXT CHECK(agent_mode IN ('chat', 'edit', 'ideation'))
         );

         CREATE INDEX idx_chat_conversations_context
            ON chat_conversations(context_type, context_id);

         CREATE TABLE agent_conversation_workspaces (
            conversation_id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            mode TEXT NOT NULL CHECK (mode IN ('chat', 'edit', 'ideation')),
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
            '2026-05-21T22:29:11Z',
            '2026-05-21T22:29:11Z'
         );",
    )
    .expect("create legacy schema");
    conn
}

#[test]
fn migration_allows_plan_agent_and_workspace_modes() {
    let conn = setup_test_db();
    v20260521222911_agent_plan_mode::migrate(&conn).unwrap();

    let preserved_mode: String = conn
        .query_row(
            "SELECT mode FROM agent_conversation_workspaces WHERE conversation_id = 'conversation-edit'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(preserved_mode, "edit");

    conn.execute(
        "INSERT INTO chat_conversations (id, context_type, context_id, agent_mode)
         VALUES ('conversation-plan', 'project', 'project-1', 'plan')",
        [],
    )
    .unwrap();
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
        VALUES (
            'conversation-plan',
            'project-1',
            'plan',
            'current_branch',
            'feature/plan-mode',
            'ralphx/project/agent-conversation-plan',
            '/tmp/agent-conversation-plan',
            '2026-05-21T22:30:00Z',
            '2026-05-21T22:30:00Z'
        )",
        [],
    )
    .unwrap();

    assert!(conn
        .execute(
            "INSERT INTO chat_conversations (id, context_type, context_id, agent_mode)
             VALUES ('conversation-review', 'project', 'project-1', 'review')",
            [],
        )
        .is_err());
    assert!(conn
        .execute(
            "UPDATE agent_conversation_workspaces
             SET mode = 'review'
             WHERE conversation_id = 'conversation-plan'",
            [],
        )
        .is_err());

    let index_sql: String = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'index' AND name = 'idx_agent_conversation_workspaces_project'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(index_sql.contains("agent_conversation_workspaces"));
}

#[test]
fn migration_is_idempotent_after_plan_checks_exist() {
    let conn = setup_test_db();
    v20260521222911_agent_plan_mode::migrate(&conn).unwrap();
    v20260521222911_agent_plan_mode::migrate(&conn).unwrap();

    conn.execute(
        "INSERT INTO chat_conversations (id, context_type, context_id, agent_mode)
         VALUES ('conversation-plan-again', 'project', 'project-1', 'plan')",
        [],
    )
    .unwrap();
}
