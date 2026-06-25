// V65 migration tests - unique working_directory index

use crate::infrastructure::sqlite::{open_memory_connection, run_migrations};

#[test]
fn test_v65_unique_index_exists() {
    let conn = open_memory_connection().unwrap();
    run_migrations(&conn).unwrap();

    let index_names: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='index' AND tbl_name='projects'")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .flatten()
        .collect();

    assert!(
        index_names.iter().any(|n| n == "idx_projects_working_dir"),
        "unique index idx_projects_working_dir should exist, got: {:?}",
        index_names
    );
}

#[test]
fn test_v65_unique_constraint_enforced() {
    let conn = open_memory_connection().unwrap();
    run_migrations(&conn).unwrap();

    // Insert first project
    conn.execute(
        "INSERT INTO projects (id, name, working_directory) VALUES ('p1', 'A', '/home/user/proj')",
        [],
    )
    .unwrap();

    // Inserting a second with the same working_directory should fail
    let result = conn.execute(
        "INSERT INTO projects (id, name, working_directory) VALUES ('p2', 'B', '/home/user/proj')",
        [],
    );

    assert!(
        result.is_err(),
        "duplicate working_directory should be rejected by unique index"
    );
}

#[test]
fn test_v65_different_directories_allowed() {
    let conn = open_memory_connection().unwrap();
    run_migrations(&conn).unwrap();

    // Two projects with distinct working_directory values must both be insertable
    conn.execute(
        "INSERT INTO projects (id, name, working_directory) VALUES ('p1', 'A', '/home/user/proj1')",
        [],
    )
    .unwrap();

    let result = conn.execute(
        "INSERT INTO projects (id, name, working_directory) VALUES ('p2', 'B', '/home/user/proj2')",
        [],
    );

    assert!(
        result.is_ok(),
        "two projects with distinct directories should both be insertable"
    );
}

#[test]
fn test_v65_idempotent() {
    let conn = open_memory_connection().unwrap();
    run_migrations(&conn).unwrap();
    // Running twice should not error (IF NOT EXISTS in index creation)
    run_migrations(&conn).unwrap();

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='idx_projects_working_dir'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(
        count, 1,
        "index should exist exactly once after idempotent run"
    );
}
