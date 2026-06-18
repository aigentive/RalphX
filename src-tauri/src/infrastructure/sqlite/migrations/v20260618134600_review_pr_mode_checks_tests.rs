//! Tests for migration v20260618134600: review PR mode checks

use rusqlite::Connection;

use super::v20260618134600_review_pr_mode_checks;

fn setup_plan_mode_schema() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute_batch(
        "CREATE TABLE chat_conversations (
            id TEXT PRIMARY KEY,
            context_type TEXT NOT NULL,
            context_id TEXT NOT NULL,
            agent_mode TEXT CHECK(agent_mode IN ('chat', 'edit', 'plan', 'ideation'))
         );

         CREATE INDEX idx_chat_conversations_context
            ON chat_conversations(context_type, context_id);

         CREATE TABLE agent_conversation_workspaces (
            conversation_id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            mode TEXT NOT NULL CHECK (mode IN ('chat', 'edit', 'plan', 'ideation')),
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
         VALUES ('conversation-draft', 'project', 'project-1', NULL);

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
            '2026-06-18T13:46:00Z',
            '2026-06-18T13:46:00Z'
         );",
    )
    .expect("create plan-mode schema");
    conn
}

#[test]
fn migration_allows_review_pr_agent_and_workspace_modes() {
    let conn = setup_plan_mode_schema();

    assert!(conn
        .execute(
            "UPDATE chat_conversations
             SET agent_mode = 'review_pr'
             WHERE id = 'conversation-draft'",
            [],
        )
        .is_err());

    v20260618134600_review_pr_mode_checks::migrate(&conn).unwrap();

    conn.execute(
        "UPDATE chat_conversations
         SET agent_mode = 'review_pr'
         WHERE id = 'conversation-draft'",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO chat_conversations (id, context_type, context_id, agent_mode)
         VALUES ('conversation-review-pr', 'project', 'project-1', 'review_pr')",
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
            'conversation-review-pr',
            'project-1',
            'review_pr',
            'local_branch',
            'feature/review-pr-mode',
            'ralphx/project/agent-conversation-review-pr',
            '/tmp/agent-conversation-review-pr',
            '2026-06-18T13:47:00Z',
            '2026-06-18T13:47:00Z'
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
             WHERE conversation_id = 'conversation-review-pr'",
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
fn migration_is_idempotent_after_review_pr_checks_exist() {
    let conn = setup_plan_mode_schema();
    v20260618134600_review_pr_mode_checks::migrate(&conn).unwrap();
    v20260618134600_review_pr_mode_checks::migrate(&conn).unwrap();

    conn.execute(
        "INSERT INTO chat_conversations (id, context_type, context_id, agent_mode)
         VALUES ('conversation-review-pr-again', 'project', 'project-1', 'review_pr')",
        [],
    )
    .unwrap();
}
