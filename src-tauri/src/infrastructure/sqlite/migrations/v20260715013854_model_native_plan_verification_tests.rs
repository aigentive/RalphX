//! Tests for migration v20260715013854: model native plan verification

use rusqlite::Connection;

use super::v20260715013854_model_native_plan_verification;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute_batch(
        "CREATE TABLE ideation_settings (id INTEGER PRIMARY KEY);
         INSERT INTO ideation_settings (id) VALUES (1);
         CREATE TABLE ideation_sessions (
            id TEXT PRIMARY KEY,
            plan_artifact_id TEXT,
            verification_status TEXT NOT NULL DEFAULT 'unverified'
         );
         INSERT INTO ideation_sessions (id, plan_artifact_id, verification_status)
         VALUES
            ('verified', 'artifact-current', 'verified'),
            ('ambiguous', NULL, 'verified'),
            ('unverified', 'artifact-draft', 'unverified');
         CREATE TABLE agent_runs (id TEXT PRIMARY KEY, status TEXT NOT NULL);",
    )
    .expect("create preceding schema");
    conn
}

#[test]
fn test_migration_adds_policy_proof_and_action_metadata() {
    let conn = setup_test_db();
    v20260715013854_model_native_plan_verification::migrate(&conn).unwrap();

    let policy: (i64, Option<i64>) = conn
        .query_row(
            "SELECT auto_verify_plans, ext_auto_verify_plans FROM ideation_settings WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(policy, (0, None));

    let proofs = ["verified", "ambiguous", "unverified"].map(|id| {
        conn.query_row(
            "SELECT verified_plan_artifact_id FROM ideation_sessions WHERE id = ?1",
            [id],
            |row| row.get::<_, Option<String>>(0),
        )
        .unwrap()
    });
    assert_eq!(proofs[0].as_deref(), Some("artifact-current"));
    assert_eq!(proofs[1], None);
    assert_eq!(proofs[2], None);

    conn.execute(
        "INSERT INTO agent_runs (id, status, action_kind, action_context_id, action_target_id)
         VALUES ('run-1', 'running', 'verify_plan', 'session-1', 'artifact-current')",
        [],
    )
    .unwrap();

    conn.execute(
        "UPDATE ideation_sessions SET verified_plan_agent_run_id = 'run-1' WHERE id = 'verified'",
        [],
    )
    .unwrap();
    conn.execute(
        "UPDATE agent_runs SET status = 'cancelled' WHERE id = 'run-1'",
        [],
    )
    .unwrap();
    let cleared: (Option<String>, Option<String>) = conn
        .query_row(
            "SELECT verified_plan_artifact_id, verified_plan_agent_run_id
             FROM ideation_sessions WHERE id = 'verified'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(cleared, (None, None));
}
