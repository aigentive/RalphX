//! Tests for migration v20260718162852: clear detected validation commands

use rusqlite::{params, Connection};
use serde_json::{json, Value};

use super::v20260718162852_clear_detected_validation_commands;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute_batch(
        "CREATE TABLE projects (
            id TEXT PRIMARY KEY,
            detected_analysis TEXT,
            custom_analysis TEXT
        );",
    )
    .expect("Failed to create projects table");
    conn
}

#[test]
fn clears_detected_validation_without_touching_entry_metadata_or_custom_analysis() {
    let conn = setup_test_db();

    let detected = json!([
        {
            "path": ".",
            "label": "Rust backend",
            "install": null,
            "validate": ["cargo test", "cargo clippy --all-targets"],
            "worktree_setup": []
        },
        {
            "path": "frontend",
            "label": "Frontend",
            "install": "npm install",
            "worktree_setup": ["ln -s source target"]
        }
    ]);
    let custom = json!([{
        "path": ".",
        "validate": ["custom focused check"]
    }]);
    conn.execute(
        "INSERT INTO projects (id, detected_analysis, custom_analysis) VALUES (?1, ?2, ?3)",
        params!["project-1", detected.to_string(), custom.to_string()],
    )
    .unwrap();

    v20260718162852_clear_detected_validation_commands::migrate(&conn).unwrap();

    let (updated_detected, updated_custom): (String, String) = conn
        .query_row(
            "SELECT detected_analysis, custom_analysis FROM projects WHERE id = 'project-1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let updated_detected: Value = serde_json::from_str(&updated_detected).unwrap();

    assert_eq!(updated_detected[0]["validate"], json!([]));
    assert_eq!(updated_detected[1]["validate"], json!([]));
    assert_eq!(updated_detected[0]["label"], "Rust backend");
    assert_eq!(updated_detected[1]["install"], "npm install");
    assert_eq!(
        updated_detected[1]["worktree_setup"],
        json!(["ln -s source target"])
    );
    assert_eq!(updated_custom, custom.to_string());
}

#[test]
fn preserves_unparseable_detected_analysis_and_is_idempotent() {
    let conn = setup_test_db();
    conn.execute(
        "INSERT INTO projects (id, detected_analysis, custom_analysis) VALUES (?1, ?2, NULL)",
        params!["invalid", "not-json"],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO projects (id, detected_analysis, custom_analysis) VALUES (?1, ?2, NULL)",
        params!["valid", r#"[{"path":".","validate":["cargo test"]}]"#],
    )
    .unwrap();

    v20260718162852_clear_detected_validation_commands::migrate(&conn).unwrap();
    let after_first: String = conn
        .query_row(
            "SELECT detected_analysis FROM projects WHERE id = 'valid'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    v20260718162852_clear_detected_validation_commands::migrate(&conn).unwrap();
    let after_second: String = conn
        .query_row(
            "SELECT detected_analysis FROM projects WHERE id = 'valid'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let invalid: String = conn
        .query_row(
            "SELECT detected_analysis FROM projects WHERE id = 'invalid'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(after_first, after_second);
    assert_eq!(invalid, "not-json");
}
