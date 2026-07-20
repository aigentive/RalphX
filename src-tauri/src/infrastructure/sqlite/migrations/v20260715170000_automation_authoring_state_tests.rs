use rusqlite::Connection;

use super::v20260715170000_automation_authoring_state;

#[test]
fn migration_adds_nullable_authoring_state_and_is_idempotent() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute("CREATE TABLE automations (id TEXT PRIMARY KEY)", [])
        .unwrap();

    v20260715170000_automation_authoring_state::migrate(&conn).unwrap();
    v20260715170000_automation_authoring_state::migrate(&conn).unwrap();

    let mut statement = conn.prepare("PRAGMA table_info(automations)").unwrap();
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(columns
        .iter()
        .any(|column| column == "authoring_state_json"));
}
