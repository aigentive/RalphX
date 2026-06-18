use crate::infrastructure::sqlite::migrations::v61_ideation_settings_verification::migrate;
use rusqlite::Connection;

fn setup_base_table(conn: &Connection) {
    conn.execute_batch(
        "CREATE TABLE ideation_settings (
            id INTEGER PRIMARY KEY DEFAULT 1 CHECK (id = 1),
            plan_mode TEXT NOT NULL DEFAULT 'optional',
            require_plan_approval INTEGER NOT NULL DEFAULT 0,
            suggest_plans_for_complex INTEGER NOT NULL DEFAULT 1,
            auto_link_proposals INTEGER NOT NULL DEFAULT 1,
            updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))
        );
        INSERT INTO ideation_settings (id) VALUES (1);",
    )
    .unwrap();
}

#[test]
fn adds_require_verification_for_accept_column() {
    let conn = Connection::open_in_memory().unwrap();
    setup_base_table(&conn);

    migrate(&conn).unwrap();

    let val: i64 = conn
        .query_row(
            "SELECT require_verification_for_accept FROM ideation_settings WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(val, 0);
}

#[test]
fn adds_require_verification_for_proposals_column() {
    let conn = Connection::open_in_memory().unwrap();
    setup_base_table(&conn);

    migrate(&conn).unwrap();

    let val: i64 = conn
        .query_row(
            "SELECT require_verification_for_proposals FROM ideation_settings WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(val, 0);
}

#[test]
fn idempotent() {
    let conn = Connection::open_in_memory().unwrap();
    setup_base_table(&conn);

    migrate(&conn).unwrap();
    migrate(&conn).unwrap(); // Should not fail
}
