//! Tests for migration v20260513143000: orphan agent worktree cleanup markers

use rusqlite::Connection;

use super::helpers;
use super::v20260513143000_orphan_worktree_cleanup_markers;

#[test]
fn orphan_worktree_cleanup_marker_table_is_created() {
    let conn = Connection::open_in_memory().expect("create in-memory db");

    v20260513143000_orphan_worktree_cleanup_markers::migrate(&conn).unwrap();

    assert!(helpers::table_exists(
        &conn,
        "orphan_agent_worktree_cleanup_markers"
    ));
    assert!(helpers::index_exists(
        &conn,
        "idx_orphan_agent_worktree_cleanup_recent"
    ));
}

#[test]
fn orphan_worktree_cleanup_marker_migration_is_idempotent() {
    let conn = Connection::open_in_memory().expect("create in-memory db");

    v20260513143000_orphan_worktree_cleanup_markers::migrate(&conn).unwrap();
    v20260513143000_orphan_worktree_cleanup_markers::migrate(&conn).unwrap();

    assert!(helpers::table_exists(
        &conn,
        "orphan_agent_worktree_cleanup_markers"
    ));
}
