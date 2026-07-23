use rusqlite::Connection;

use super::v20260723111500_project_skill_evidence_batches;

fn setup() -> Connection {
    let connection = Connection::open_in_memory().unwrap();
    connection
        .execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE projects (id TEXT PRIMARY KEY);
             CREATE TABLE task_outcomes (
                id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE
             );
             CREATE TABLE project_skills (
                id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE
             );
             INSERT INTO projects (id) VALUES ('project-1');
             INSERT INTO task_outcomes (id, project_id) VALUES ('outcome-1', 'project-1');
             INSERT INTO project_skills (id, project_id) VALUES ('skill-1', 'project-1');",
        )
        .unwrap();
    v20260723111500_project_skill_evidence_batches::migrate(&connection).unwrap();
    connection
}

fn insert_batch(connection: &Connection, id: &str, fingerprint: &str) {
    connection
        .execute(
            "INSERT INTO project_skill_evidence_batches (
                id, project_id, fingerprint, bucket, status, created_at, updated_at
             ) VALUES (?1, 'project-1', ?2, 'execution', 'pending', 'now', 'now')",
            rusqlite::params![id, fingerprint],
        )
        .unwrap();
}

#[test]
fn migration_enforces_batch_state_identity_and_item_bounds() {
    let connection = setup();
    insert_batch(&connection, "batch-1", &"a".repeat(64));

    assert!(connection
        .execute(
            "INSERT INTO project_skill_evidence_batches (
                id, project_id, fingerprint, bucket, status, created_at, updated_at
             ) VALUES (
                'batch-duplicate', 'project-1', ?1, 'execution', 'pending', 'now', 'now'
             )",
            [&"a".repeat(64)],
        )
        .is_err());
    assert!(connection
        .execute(
            "INSERT INTO project_skill_evidence_batches (
                id, project_id, fingerprint, bucket, status, created_at, updated_at
             ) VALUES (
                'batch-invalid', 'project-1', ?1, 'execution', 'consumed', 'now', 'now'
             )",
            [&"b".repeat(64)],
        )
        .is_err());

    connection
        .execute(
            "INSERT INTO project_skill_evidence_batch_items (
                batch_id, outcome_id, ordinal, digest
             ) VALUES ('batch-1', 'outcome-1', 0, ?1)",
            [&"🦀".repeat(1_200)],
        )
        .unwrap();
    assert!(connection
        .execute(
            "INSERT INTO project_skill_evidence_batch_items (
                batch_id, outcome_id, ordinal, digest
             ) VALUES ('batch-1', 'missing-outcome', 1, 'digest')",
            [],
        )
        .is_err());
    assert!(connection
        .execute(
            "INSERT INTO project_skill_evidence_batch_items (
                batch_id, outcome_id, ordinal, digest
             ) VALUES ('batch-1', 'outcome-1', 8, 'digest')",
            [],
        )
        .is_err());
    assert!(connection
        .execute(
            "UPDATE project_skill_evidence_batch_items SET digest = ?1
             WHERE batch_id = 'batch-1' AND ordinal = 0",
            [&"x".repeat(1_201)],
        )
        .is_err());
}

#[test]
fn migration_accepts_only_complete_success_markers() {
    let connection = setup();
    insert_batch(&connection, "batch-1", &"a".repeat(64));

    assert!(connection
        .execute(
            "UPDATE project_skill_evidence_batches
             SET status = 'consumed', claim_token = 'claim', claimed_at = 'now',
                 completed_project_skill_id = 'skill-1'
             WHERE id = 'batch-1'",
            [],
        )
        .is_err());
    connection
        .execute(
            "UPDATE project_skill_evidence_batches
             SET status = 'consumed', claim_token = 'claim', claimed_at = 'now',
                 completed_project_skill_id = 'skill-1',
                 resolution_action = 'create_new', completed_at = 'now'
             WHERE id = 'batch-1'",
            [],
        )
        .unwrap();
}
