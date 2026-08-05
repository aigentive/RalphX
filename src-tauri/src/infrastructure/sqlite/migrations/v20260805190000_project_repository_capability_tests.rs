use rusqlite::Connection;

use super::v20260805190000_project_repository_capability;

#[test]
fn migration_has_all_columns_and_is_idempotent() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("CREATE TABLE projects (id TEXT PRIMARY KEY)")
        .unwrap();
    v20260805190000_project_repository_capability::migrate(&conn).unwrap();
    v20260805190000_project_repository_capability::migrate(&conn).unwrap();
    let mut statement = conn
        .prepare("PRAGMA table_info(project_repository_capability)")
        .unwrap();
    let columns: Vec<String> = statement
        .query_map([], |row| row.get(1))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        columns,
        [
            "project_id",
            "kind",
            "fetch_url",
            "push_url",
            "message",
            "inspected_at",
            "working_directory"
        ]
    );
}

#[test]
fn migration_row_round_trips() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys=ON; CREATE TABLE projects (id TEXT PRIMARY KEY); INSERT INTO projects VALUES ('p1')").unwrap();
    v20260805190000_project_repository_capability::migrate(&conn).unwrap();
    conn.execute(
        "INSERT INTO project_repository_capability VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![
            "p1",
            "github",
            "fetch",
            "push",
            Option::<String>::None,
            "2026-08-05T19:00:00+00:00",
            "/repo"
        ],
    )
    .unwrap();
    let row: (String, String, String) = conn.query_row("SELECT kind, push_url, working_directory FROM project_repository_capability WHERE project_id='p1'", [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap();
    assert_eq!(row, ("github".into(), "push".into(), "/repo".into()));
}
