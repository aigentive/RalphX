// Migration v20260627104500: agent conversation Granola note links

use rusqlite::Connection;

use crate::error::{AppError, AppResult};

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS agent_conversation_granola_note_links (
            conversation_id TEXT PRIMARY KEY REFERENCES chat_conversations(id) ON DELETE CASCADE,
            project_id TEXT NOT NULL,
            provider TEXT NOT NULL DEFAULT 'granola',
            note_id TEXT NOT NULL,
            note_url TEXT,
            title TEXT,
            summary_markdown TEXT,
            transcript_json TEXT NOT NULL DEFAULT '[]',
            include_transcript INTEGER NOT NULL DEFAULT 1,
            last_refreshed_at TEXT,
            refresh_status TEXT NOT NULL DEFAULT 'not_loaded'
                CHECK(refresh_status IN ('not_loaded', 'loaded', 'error')),
            refresh_error TEXT,
            assigned_at TEXT NOT NULL,
            assigned_from_message_id TEXT REFERENCES chat_messages(id) ON DELETE SET NULL,
            manually_assigned INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_agent_conversation_granola_note_links_project_id
            ON agent_conversation_granola_note_links(project_id);
        CREATE INDEX IF NOT EXISTS idx_agent_conversation_granola_note_links_project_note
            ON agent_conversation_granola_note_links(project_id, note_id);
        CREATE INDEX IF NOT EXISTS idx_agent_conversation_granola_note_links_conversation
            ON agent_conversation_granola_note_links(conversation_id);",
    )
    .map_err(|e| AppError::Database(e.to_string()))
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::migrate;

    fn table_exists(conn: &Connection, table: &str) -> bool {
        conn.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1
            )",
            [table],
            |row| row.get::<_, i64>(0),
        )
        .unwrap()
            == 1
    }

    fn index_exists(conn: &Connection, index: &str) -> bool {
        conn.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM sqlite_master WHERE type = 'index' AND name = ?1
            )",
            [index],
            |row| row.get::<_, i64>(0),
        )
        .unwrap()
            == 1
    }

    #[test]
    fn creates_granola_note_link_table_and_indexes() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        migrate(&conn).unwrap();

        assert!(table_exists(&conn, "agent_conversation_granola_note_links"));
        assert!(index_exists(
            &conn,
            "idx_agent_conversation_granola_note_links_project_id"
        ));
        assert!(index_exists(
            &conn,
            "idx_agent_conversation_granola_note_links_project_note"
        ));
    }
}
