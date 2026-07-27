use rusqlite::Connection;

use crate::domain::entities::IdeationSessionId;
use crate::domain::repositories::PlanApprovalActor;

use super::{
    approve_current_plan_artifact_for_displayed_bundle_sync, approve_current_plan_artifact_sync,
};

fn setup_db() -> Connection {
    let conn = Connection::open_in_memory().expect("open test db");
    conn.execute_batch(
        "
        CREATE TABLE ideation_sessions (
            id TEXT PRIMARY KEY,
            status TEXT NOT NULL,
            session_flow TEXT NOT NULL,
            plan_artifact_id TEXT,
            plan_blueprint_artifact_id TEXT,
            plan_contract_version INTEGER NOT NULL DEFAULT 1
        );
        CREATE TABLE artifacts (
            id TEXT PRIMARY KEY,
            type TEXT NOT NULL,
            name TEXT NOT NULL,
            content_type TEXT NOT NULL,
            content_text TEXT,
            content_path TEXT,
            bucket_id TEXT,
            task_id TEXT,
            process_id TEXT,
            created_by TEXT NOT NULL,
            version INTEGER NOT NULL,
            previous_version_id TEXT,
            created_at TEXT NOT NULL,
            metadata_json TEXT,
            archived_at TEXT
        );
        CREATE TABLE plan_artifact_approvals (
            session_id TEXT PRIMARY KEY,
            artifact_id TEXT NOT NULL,
            artifact_version INTEGER NOT NULL,
            blueprint_artifact_id TEXT,
            blueprint_artifact_version INTEGER,
            status TEXT NOT NULL,
            approved_at TEXT NOT NULL,
            approved_by TEXT NOT NULL
        );
        INSERT INTO ideation_sessions (id, status, session_flow, plan_artifact_id)
        VALUES ('session-1', 'active', 'planning', 'artifact-1');
        INSERT INTO artifacts (
            id, type, name, content_type, content_text, created_by, version, created_at
        ) VALUES (
            'artifact-1', 'specification', 'Run plan', 'inline', 'plan body', 'tester', 3,
            '2026-01-01T00:00:00Z'
        );
        ",
    )
    .expect("seed test db");
    conn
}

#[test]
fn approve_current_plan_artifact_records_user_actor() {
    let conn = setup_db();

    let approved = approve_current_plan_artifact_sync(
        &conn,
        IdeationSessionId::from_string("session-1"),
        Some("artifact-1"),
        PlanApprovalActor::User,
    )
    .expect("approve current plan");

    assert_eq!(approved.artifact.id.as_str(), "artifact-1");
    let row: (String, i64, String) = conn
        .query_row(
            "SELECT artifact_id, artifact_version, approved_by
             FROM plan_artifact_approvals WHERE session_id = 'session-1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("approval row");
    assert_eq!(row, ("artifact-1".to_string(), 3, "user".to_string()));
}

#[test]
fn approve_current_plan_artifact_supports_judge_actor() {
    let conn = setup_db();

    approve_current_plan_artifact_sync(
        &conn,
        IdeationSessionId::from_string("session-1"),
        Some("artifact-1"),
        PlanApprovalActor::Judge,
    )
    .expect("approve current plan by judge");

    let approved_by: String = conn
        .query_row(
            "SELECT approved_by FROM plan_artifact_approvals WHERE session_id = 'session-1'",
            [],
            |row| row.get(0),
        )
        .expect("approval actor");
    assert_eq!(approved_by, "judge");
}

#[test]
fn approve_current_plan_artifact_rejects_changed_requested_artifact() {
    let conn = setup_db();

    let err = approve_current_plan_artifact_sync(
        &conn,
        IdeationSessionId::from_string("session-1"),
        Some("stale-artifact"),
        PlanApprovalActor::User,
    )
    .expect_err("stale artifact approval should fail");

    assert!(err.to_string().contains("Plan changed before approval"));
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM plan_artifact_approvals", [], |row| {
            row.get(0)
        })
        .expect("approval count");
    assert_eq!(count, 0);
}

#[test]
fn approve_current_plan_artifact_rejects_a_blueprint_the_user_did_not_view() {
    let conn = setup_db();
    conn.execute(
        "INSERT INTO artifacts (
            id, type, name, content_type, content_text, created_by, version, created_at
        ) VALUES (
            'blueprint-2', 'specification', 'Blueprint v2', 'inline', 'new blueprint', 'tester', 2,
            '2026-01-02T00:00:00Z'
        )",
        [],
    )
    .expect("insert revised blueprint");
    conn.execute(
        "UPDATE ideation_sessions
         SET plan_blueprint_artifact_id = 'blueprint-2', plan_contract_version = 2
         WHERE id = 'session-1'",
        [],
    )
    .expect("point session at revised blueprint");

    let err = approve_current_plan_artifact_for_displayed_bundle_sync(
        &conn,
        IdeationSessionId::from_string("session-1"),
        Some("artifact-1"),
        Some("blueprint-1"),
        Some(1),
        PlanApprovalActor::User,
    )
    .expect_err("approval must reject a Blueprint changed after the user viewed it");

    assert!(err.to_string().contains("Plan changed before approval"));
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM plan_artifact_approvals", [], |row| {
            row.get(0)
        })
        .expect("approval count");
    assert_eq!(count, 0);
}

#[test]
fn approve_current_plan_artifact_fails_closed_on_corrupt_session_status() {
    let conn = setup_db();
    conn.execute(
        "UPDATE ideation_sessions SET status = 'unknown-status' WHERE id = 'session-1'",
        [],
    )
    .expect("corrupt session status");

    let err = approve_current_plan_artifact_sync(
        &conn,
        IdeationSessionId::from_string("session-1"),
        Some("artifact-1"),
        PlanApprovalActor::User,
    )
    .expect_err("corrupt status should fail closed");

    assert!(err.to_string().contains("unknown ideation session status"));
}

#[test]
fn approve_current_plan_artifact_rejects_archived_or_accepted_sessions() {
    for status in ["archived", "accepted"] {
        let conn = setup_db();
        conn.execute(
            "UPDATE ideation_sessions SET status = ?1 WHERE id = 'session-1'",
            [status],
        )
        .expect("set terminal status");

        let err = approve_current_plan_artifact_sync(
            &conn,
            IdeationSessionId::from_string("session-1"),
            Some("artifact-1"),
            PlanApprovalActor::User,
        )
        .expect_err("terminal sessions should reject plan approval");

        assert!(err.to_string().contains("Cannot modify"));
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM plan_artifact_approvals", [], |row| {
                row.get(0)
            })
            .expect("approval count");
        assert_eq!(count, 0);
    }
}

#[test]
fn approve_current_plan_artifact_rejects_non_planning_or_missing_plan_artifact() {
    let conn = setup_db();
    conn.execute(
        "UPDATE ideation_sessions SET session_flow = 'ideation' WHERE id = 'session-1'",
        [],
    )
    .expect("set ideation flow");
    let err = approve_current_plan_artifact_sync(
        &conn,
        IdeationSessionId::from_string("session-1"),
        Some("artifact-1"),
        PlanApprovalActor::User,
    )
    .expect_err("non-planning sessions should reject approval");
    assert!(err
        .to_string()
        .contains("only available for planning sessions"));

    conn.execute(
        "UPDATE ideation_sessions
         SET session_flow = 'planning', plan_artifact_id = NULL
         WHERE id = 'session-1'",
        [],
    )
    .expect("clear plan artifact");
    let err = approve_current_plan_artifact_sync(
        &conn,
        IdeationSessionId::from_string("session-1"),
        None,
        PlanApprovalActor::User,
    )
    .expect_err("missing owned artifact should reject approval");
    assert!(err
        .to_string()
        .contains("does not have an owned plan artifact"));
}

#[test]
fn approve_current_plan_artifact_rejects_deleted_plan_artifact() {
    let conn = setup_db();
    conn.execute("DELETE FROM artifacts WHERE id = 'artifact-1'", [])
        .expect("delete artifact");

    let err = approve_current_plan_artifact_sync(
        &conn,
        IdeationSessionId::from_string("session-1"),
        Some("artifact-1"),
        PlanApprovalActor::User,
    )
    .expect_err("missing artifact row should fail closed");

    assert!(err
        .to_string()
        .contains("Plan artifact artifact-1 not found"));
}

#[test]
fn approve_current_plan_artifact_upserts_latest_artifact_and_actor() {
    let conn = setup_db();
    approve_current_plan_artifact_sync(
        &conn,
        IdeationSessionId::from_string("session-1"),
        Some("artifact-1"),
        PlanApprovalActor::User,
    )
    .expect("initial approval");

    conn.execute(
        "INSERT INTO artifacts (
            id, type, name, content_type, content_text, created_by, version, created_at
        ) VALUES (
            'artifact-2', 'specification', 'Run plan v2', 'inline', 'plan v2', 'tester', 4,
            '2026-01-02T00:00:00Z'
        )",
        [],
    )
    .expect("insert new artifact");
    conn.execute(
        "UPDATE ideation_sessions SET plan_artifact_id = 'artifact-2' WHERE id = 'session-1'",
        [],
    )
    .expect("point session at new artifact");

    let approved = approve_current_plan_artifact_sync(
        &conn,
        IdeationSessionId::from_string("session-1"),
        Some("artifact-2"),
        PlanApprovalActor::Judge,
    )
    .expect("approval should upsert");
    assert_eq!(approved.artifact.id.as_str(), "artifact-2");

    let row: (String, i64, String) = conn
        .query_row(
            "SELECT artifact_id, artifact_version, approved_by
             FROM plan_artifact_approvals WHERE session_id = 'session-1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("approval row");
    assert_eq!(row, ("artifact-2".to_string(), 4, "judge".to_string()));
}
