//! Tests for migration v20260405045108: ideation external overrides

use rusqlite::Connection;

use super::v20260405045108_ideation_external_overrides;

fn setup_test_db_with_ideation_settings() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute_batch(
        "CREATE TABLE ideation_settings (
            id INTEGER PRIMARY KEY,
            plan_mode TEXT NOT NULL DEFAULT 'optional',
            require_plan_approval INTEGER NOT NULL DEFAULT 0,
            suggest_plans_for_complex INTEGER NOT NULL DEFAULT 1,
            auto_link_proposals INTEGER NOT NULL DEFAULT 1,
            require_verification_for_accept INTEGER NOT NULL DEFAULT 0,
            require_verification_for_proposals INTEGER NOT NULL DEFAULT 0,
            require_accept_for_finalize INTEGER DEFAULT NULL,
            updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))
        );
        INSERT INTO ideation_settings (id) VALUES (1);",
    )
    .expect("Failed to create ideation_settings table");
    conn
}

#[test]
fn test_migration_adds_three_columns() {
    let conn = setup_test_db_with_ideation_settings();
    v20260405045108_ideation_external_overrides::migrate(&conn).unwrap();

    // Verify all 3 new columns exist and default to NULL
    let row: (Option<i64>, Option<i64>, Option<i64>) = conn
        .query_row(
            "SELECT ext_require_verification_for_accept,
                    ext_require_verification_for_proposals,
                    ext_require_accept_for_finalize
             FROM ideation_settings WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("Failed to query new columns");

    assert_eq!(
        row.0, None,
        "ext_require_verification_for_accept should default to NULL"
    );
    assert_eq!(
        row.1, None,
        "ext_require_verification_for_proposals should default to NULL"
    );
    assert_eq!(
        row.2, None,
        "ext_require_accept_for_finalize should default to NULL"
    );
}

#[test]
fn test_migration_existing_row_retains_values() {
    let conn = setup_test_db_with_ideation_settings();
    // Update the existing row with known values
    conn.execute(
        "UPDATE ideation_settings SET require_verification_for_accept = 1, require_accept_for_finalize = 1 WHERE id = 1",
        [],
    )
    .expect("Failed to update row");

    v20260405045108_ideation_external_overrides::migrate(&conn).unwrap();

    // Existing columns should retain their values
    let (rva, raf): (i64, Option<i64>) = conn
        .query_row(
            "SELECT require_verification_for_accept, require_accept_for_finalize FROM ideation_settings WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("Failed to query existing columns");

    assert_eq!(
        rva, 1,
        "require_verification_for_accept should retain value"
    );
    assert_eq!(
        raf,
        Some(1),
        "require_accept_for_finalize should retain value"
    );
}
