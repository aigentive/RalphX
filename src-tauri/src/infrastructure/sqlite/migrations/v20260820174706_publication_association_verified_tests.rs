//! Tests for migration v20260820174706: publication association verified

use rusqlite::Connection;

use super::v20260820174706_publication_association_verified::migrate;

fn seeded_connection() -> Connection {
    let conn = Connection::open_in_memory().expect("open migration fixture");
    conn.execute_batch(
        "CREATE TABLE agent_conversation_workspaces (
            conversation_id TEXT PRIMARY KEY,
            publication_pr_number INTEGER NULL,
            publication_pr_status TEXT NULL
        );
        CREATE TABLE agent_conversation_workspace_publication_events (
            id TEXT PRIMARY KEY,
            conversation_id TEXT NOT NULL,
            step TEXT NOT NULL,
            status TEXT NOT NULL,
            summary TEXT NOT NULL,
            classification TEXT NULL,
            created_at TEXT NOT NULL
        );
        CREATE INDEX idx_agent_workspace_publication_events_conversation
            ON agent_conversation_workspace_publication_events(conversation_id, created_at);",
    )
    .expect("seed workspace tables");
    conn
}

fn seed_workspace(conn: &Connection, id: &str, pr_number: Option<i64>, pr_status: Option<&str>) {
    conn.execute(
        "INSERT INTO agent_conversation_workspaces \
         (conversation_id, publication_pr_number, publication_pr_status) VALUES (?1, ?2, ?3)",
        rusqlite::params![id, pr_number, pr_status],
    )
    .expect("seed workspace row");
}

fn seed_event(conn: &Connection, id: &str, conversation_id: &str, step: &str, created_at: &str) {
    conn.execute(
        "INSERT INTO agent_conversation_workspace_publication_events \
         (id, conversation_id, step, status, summary, classification, created_at) \
         VALUES (?1, ?2, ?3, 'succeeded', 'summary', NULL, ?4)",
        rusqlite::params![id, conversation_id, step, created_at],
    )
    .expect("seed publication event");
}

fn verified_at(conn: &Connection, conversation_id: &str) -> Option<String> {
    conn.query_row(
        "SELECT publication_association_verified_at FROM agent_conversation_workspaces \
         WHERE conversation_id = ?1",
        rusqlite::params![conversation_id],
        |row| row.get(0),
    )
    .expect("read verified marker")
}

fn event_ids(conn: &Connection) -> Vec<String> {
    conn.prepare("SELECT id FROM agent_conversation_workspace_publication_events ORDER BY id")
        .expect("prepare event read")
        .query_map([], |row| row.get::<_, String>(0))
        .expect("query events")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect events")
}

#[test]
fn migration_adds_nullable_marker_column_idempotently() {
    let conn = seeded_connection();

    migrate(&conn).expect("first migration succeeds");
    migrate(&conn).expect("second migration succeeds");

    let column_names = conn
        .prepare("PRAGMA table_info(agent_conversation_workspaces)")
        .expect("prepare table inspection")
        .query_map([], |row| row.get::<_, String>(1))
        .expect("read table columns")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect table columns");
    assert!(column_names
        .iter()
        .any(|column| column == "publication_association_verified_at"));
}

#[test]
fn backfill_marks_only_terminal_rows_with_a_pr_number() {
    let conn = seeded_connection();
    seed_workspace(&conn, "merged-with-pr", Some(1000), Some("merged"));
    seed_workspace(&conn, "closed-with-pr", Some(1001), Some("closed"));
    seed_workspace(&conn, "open-with-pr", Some(1002), Some("open"));
    seed_workspace(&conn, "merged-without-pr", None, Some("merged"));
    seed_workspace(&conn, "no-publication", None, None);

    migrate(&conn).expect("migration succeeds");

    assert!(verified_at(&conn, "merged-with-pr").is_some());
    assert!(verified_at(&conn, "closed-with-pr").is_some());
    assert_eq!(verified_at(&conn, "open-with-pr"), None);
    assert_eq!(verified_at(&conn, "merged-without-pr"), None);
    assert_eq!(verified_at(&conn, "no-publication"), None);
}

#[test]
fn backfill_stamps_rfc3339_utc_and_does_not_overwrite_existing_markers() {
    let conn = seeded_connection();
    seed_workspace(&conn, "merged-with-pr", Some(1000), Some("merged"));
    seed_workspace(&conn, "already-verified", Some(1001), Some("merged"));

    migrate(&conn).expect("first migration succeeds");
    conn.execute(
        "UPDATE agent_conversation_workspaces \
         SET publication_association_verified_at = '2020-01-01T00:00:00+00:00' \
         WHERE conversation_id = 'already-verified'",
        [],
    )
    .expect("pin an existing marker");

    let stamped = verified_at(&conn, "merged-with-pr").expect("backfilled marker");
    assert!(
        stamped.ends_with("+00:00") && stamped.len() == "2026-08-20T17:47:06+00:00".len(),
        "expected RFC3339 UTC stamp, got {stamped}"
    );

    migrate(&conn).expect("second migration succeeds");
    assert_eq!(
        verified_at(&conn, "already-verified").as_deref(),
        Some("2020-01-01T00:00:00+00:00"),
        "re-running the migration must not overwrite an existing marker"
    );
    assert_eq!(
        verified_at(&conn, "merged-with-pr").as_deref(),
        Some(stamped.as_str())
    );
}

#[test]
fn dedupe_keeps_earliest_terminal_event_per_conversation_and_step() {
    let conn = seeded_connection();
    seed_workspace(&conn, "ws-a", Some(1000), Some("merged"));
    seed_workspace(&conn, "ws-b", Some(1001), Some("merged"));

    seed_event(
        &conn,
        "a-merged-3",
        "ws-a",
        "pr_merged",
        "2026-03-03T00:00:00+00:00",
    );
    seed_event(
        &conn,
        "a-merged-1",
        "ws-a",
        "pr_merged",
        "2026-01-01T00:00:00+00:00",
    );
    seed_event(
        &conn,
        "a-merged-2",
        "ws-a",
        "pr_merged",
        "2026-02-02T00:00:00+00:00",
    );
    seed_event(
        &conn,
        "b-merged-1",
        "ws-b",
        "pr_merged",
        "2026-01-05T00:00:00+00:00",
    );
    seed_event(
        &conn,
        "b-merged-2",
        "ws-b",
        "pr_merged",
        "2026-01-06T00:00:00+00:00",
    );

    migrate(&conn).expect("migration succeeds");

    assert_eq!(event_ids(&conn), vec!["a-merged-1", "b-merged-1"]);
}

#[test]
fn dedupe_breaks_created_at_ties_by_id_and_is_idempotent() {
    let conn = seeded_connection();
    seed_workspace(&conn, "ws-a", Some(1000), Some("merged"));

    let tied = "2026-01-01T00:00:00+00:00";
    seed_event(&conn, "tie-c", "ws-a", "pr_merged", tied);
    seed_event(&conn, "tie-a", "ws-a", "pr_merged", tied);
    seed_event(&conn, "tie-b", "ws-a", "pr_merged", tied);

    migrate(&conn).expect("first migration succeeds");
    assert_eq!(event_ids(&conn), vec!["tie-a"]);

    migrate(&conn).expect("second migration succeeds");
    assert_eq!(event_ids(&conn), vec!["tie-a"]);
}

#[test]
fn dedupe_leaves_singletons_and_non_terminal_steps_untouched() {
    let conn = seeded_connection();
    seed_workspace(&conn, "ws-a", Some(1000), Some("merged"));

    seed_event(
        &conn,
        "closed-single",
        "ws-a",
        "pr_closed",
        "2026-01-01T00:00:00+00:00",
    );
    seed_event(
        &conn,
        "corrected-1",
        "ws-a",
        "publication_association_corrected",
        "2026-01-02T00:00:00+00:00",
    );
    seed_event(
        &conn,
        "corrected-2",
        "ws-a",
        "publication_association_corrected",
        "2026-01-03T00:00:00+00:00",
    );
    seed_event(
        &conn,
        "external-1",
        "ws-a",
        "external_pr_merged",
        "2026-01-04T00:00:00+00:00",
    );
    seed_event(
        &conn,
        "external-2",
        "ws-a",
        "external_pr_merged",
        "2026-01-05T00:00:00+00:00",
    );

    migrate(&conn).expect("migration succeeds");

    assert_eq!(
        event_ids(&conn),
        vec![
            "closed-single",
            "corrected-1",
            "corrected-2",
            "external-1",
            "external-2"
        ],
        "only replayed pr_merged/pr_closed duplicates may be removed"
    );
}

#[test]
fn dedupe_scopes_duplicates_per_conversation_and_step() {
    let conn = seeded_connection();
    seed_workspace(&conn, "ws-a", Some(1000), Some("merged"));

    seed_event(
        &conn,
        "merged-1",
        "ws-a",
        "pr_merged",
        "2026-01-01T00:00:00+00:00",
    );
    seed_event(
        &conn,
        "merged-2",
        "ws-a",
        "pr_merged",
        "2026-01-02T00:00:00+00:00",
    );
    seed_event(
        &conn,
        "closed-1",
        "ws-a",
        "pr_closed",
        "2026-01-03T00:00:00+00:00",
    );
    seed_event(
        &conn,
        "closed-2",
        "ws-a",
        "pr_closed",
        "2026-01-04T00:00:00+00:00",
    );

    migrate(&conn).expect("migration succeeds");

    assert_eq!(
        event_ids(&conn),
        vec!["closed-1", "merged-1"],
        "each (conversation, step) pair keeps exactly its earliest event"
    );
}
