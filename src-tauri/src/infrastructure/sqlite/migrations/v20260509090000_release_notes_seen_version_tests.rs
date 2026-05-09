use rusqlite::Connection;

use super::helpers::column_exists;
use super::{v14_app_state, v20260509090000_release_notes_seen_version};

#[test]
fn adds_last_seen_release_notes_version_to_app_state() {
    let conn = Connection::open_in_memory().unwrap();
    v14_app_state::migrate(&conn).unwrap();

    v20260509090000_release_notes_seen_version::migrate(&conn).unwrap();

    assert!(column_exists(
        &conn,
        "app_state",
        "last_seen_release_notes_version"
    ));
}

#[test]
fn migration_is_idempotent() {
    let conn = Connection::open_in_memory().unwrap();
    v14_app_state::migrate(&conn).unwrap();

    v20260509090000_release_notes_seen_version::migrate(&conn).unwrap();
    v20260509090000_release_notes_seen_version::migrate(&conn).unwrap();

    assert!(column_exists(
        &conn,
        "app_state",
        "last_seen_release_notes_version"
    ));
}
