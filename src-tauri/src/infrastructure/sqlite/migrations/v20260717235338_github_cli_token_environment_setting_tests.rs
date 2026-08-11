//! Tests for migration v20260717235338: github cli token environment setting

use rusqlite::Connection;

use super::{v14_app_state, v20260717235338_github_cli_token_environment_setting};

fn setup_test_db() -> Connection {
    Connection::open_in_memory().expect("Failed to create in-memory database")
}

#[test]
fn migration_adds_enabled_by_default_github_cli_token_environment_setting() {
    let conn = setup_test_db();
    v14_app_state::migrate(&conn).unwrap();
    v20260717235338_github_cli_token_environment_setting::migrate(&conn).unwrap();

    let enabled = conn
        .query_row(
            "SELECT remove_inherited_github_cli_tokens FROM app_state WHERE id = 1",
            [],
            |row| row.get::<_, bool>(0),
        )
        .unwrap();
    assert!(enabled);
}

#[test]
fn migration_is_idempotent_and_preserves_an_explicit_opt_out() {
    let conn = setup_test_db();
    v14_app_state::migrate(&conn).unwrap();
    v20260717235338_github_cli_token_environment_setting::migrate(&conn).unwrap();
    conn.execute(
        "UPDATE app_state SET remove_inherited_github_cli_tokens = 0 WHERE id = 1",
        [],
    )
    .unwrap();

    v20260717235338_github_cli_token_environment_setting::migrate(&conn).unwrap();

    let enabled = conn
        .query_row(
            "SELECT remove_inherited_github_cli_tokens FROM app_state WHERE id = 1",
            [],
            |row| row.get::<_, bool>(0),
        )
        .unwrap();
    assert!(!enabled);
}
