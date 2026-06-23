// Migration v20260620075610: provider ticket operations

use rusqlite::Connection;

use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS provider_ticket_operations (
            id TEXT PRIMARY KEY,
            provider TEXT NOT NULL,
            external_kind TEXT NOT NULL,
            external_id TEXT NOT NULL,
            external_key TEXT,
            link_id TEXT,
            local_project_id TEXT,
            operation TEXT NOT NULL,
            client_operation_id TEXT NOT NULL UNIQUE,
            status TEXT NOT NULL,
            provider_operation_id TEXT,
            error_message TEXT,
            metadata_json TEXT,
            last_attempt_at TEXT,
            completed_at TEXT,
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')),
            updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')),
            FOREIGN KEY (link_id) REFERENCES external_issue_links(id) ON DELETE SET NULL
        );

        CREATE INDEX IF NOT EXISTS idx_provider_ticket_operations_ticket
            ON provider_ticket_operations (provider, external_kind, external_id, external_key);

        CREATE INDEX IF NOT EXISTS idx_provider_ticket_operations_link
            ON provider_ticket_operations (link_id);

        CREATE INDEX IF NOT EXISTS idx_provider_ticket_operations_project
            ON provider_ticket_operations (local_project_id);

        CREATE INDEX IF NOT EXISTS idx_provider_ticket_operations_status
            ON provider_ticket_operations (status);
        ",
    )?;
    Ok(())
}
