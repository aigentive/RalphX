// Migration v20260616182441: external issue links

use rusqlite::Connection;

use crate::error::AppResult;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS external_issue_links (
            id TEXT PRIMARY KEY,
            provider TEXT NOT NULL,
            external_kind TEXT NOT NULL,
            external_id TEXT NOT NULL,
            external_key TEXT,
            external_url TEXT,
            local_object_kind TEXT NOT NULL,
            local_object_id TEXT NOT NULL,
            local_project_id TEXT,
            local_sha TEXT,
            local_state TEXT,
            idempotency_key TEXT NOT NULL UNIQUE,
            metadata_json TEXT,
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')),
            updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')),
            UNIQUE (
                provider,
                external_kind,
                external_id,
                local_object_kind,
                local_object_id
            )
        );

        CREATE INDEX IF NOT EXISTS idx_external_issue_links_local
            ON external_issue_links (local_object_kind, local_object_id);

        CREATE INDEX IF NOT EXISTS idx_external_issue_links_external
            ON external_issue_links (provider, external_kind, external_id);

        CREATE TABLE IF NOT EXISTS external_issue_sync_records (
            id TEXT PRIMARY KEY,
            link_id TEXT NOT NULL,
            sync_kind TEXT NOT NULL,
            idempotency_key TEXT NOT NULL UNIQUE,
            local_sha TEXT,
            local_state TEXT,
            external_version TEXT,
            status TEXT NOT NULL,
            last_attempt_at TEXT,
            completed_at TEXT,
            error_message TEXT,
            metadata_json TEXT,
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')),
            updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')),
            FOREIGN KEY (link_id) REFERENCES external_issue_links(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_external_issue_sync_records_link
            ON external_issue_sync_records (link_id, sync_kind);

        CREATE INDEX IF NOT EXISTS idx_external_issue_sync_records_status
            ON external_issue_sync_records (status);
        ",
    )?;
    Ok(())
}
