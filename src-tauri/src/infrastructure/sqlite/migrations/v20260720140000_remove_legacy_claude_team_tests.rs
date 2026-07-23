use std::time::Duration;

use super::{
    get_schema_version,
    helpers::{column_exists, table_exists},
    run_migrations, run_migrations_through, v20260720140000_remove_legacy_claude_team,
    SCHEMA_VERSION,
};
use crate::infrastructure::sqlite::open_memory_connection;
use crate::testing::SqliteTestDb;

const PREVIOUS_SCHEMA_VERSION: i64 = 20260718182035;

#[test]
fn migration_removes_legacy_state_and_preserves_native_team_artifacts() {
    let conn = open_memory_connection().expect("create memory db");
    run_migrations_through(&conn, PREVIOUS_SCHEMA_VERSION).expect("run prior migrations");
    conn.execute(
        "INSERT INTO projects (id, name, working_directory) VALUES ('project-1', 'Project', '/tmp/project')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO tasks (id, project_id, title, metadata)
         VALUES ('task-1', 'project-1', 'Task', '{\"agent_variant\":\"team\",\"kept\":true}')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO ideation_sessions (id, project_id, title, team_mode, team_config_json)
         VALUES ('session-1', 'project-1', 'Session', 'research', '{}')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO chat_conversations (id, context_type, context_id, coordination_mode)
         VALUES ('conversation-1', 'ideation', 'session-1', 'legacy_claude_team')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO artifacts
            (id, type, name, content_type, content_text, bucket_id, created_by, metadata_json)
         VALUES
            ('finding-1', 'verification_finding', 'Finding', 'text', 'retired', 'team-findings', 'team-lead', '{\"author\":\"team-lead\"}'),
            ('summary-1', 'team_summary', 'Summary', 'text', 'kept', 'team-findings', 'team-lead', '{\"author_teammate\":\"team-lead\",\"unrelated_owner\":\"team-lead\"}')",
        [],
    )
    .unwrap();
    conn.execute(
        "UPDATE artifacts SET previous_version_id = 'finding-1' WHERE id = 'summary-1'",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO artifact_relations (id, from_artifact_id, to_artifact_id, relation_type)
         VALUES ('relation-1', 'summary-1', 'finding-1', 'references')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO plan_artifact_approvals
            (session_id, artifact_id, artifact_version, approved_at)
         VALUES ('session-1', 'finding-1', 1, '2026-07-20T00:00:00Z')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO plan_complexity_assessments
            (id, session_id, artifact_id, artifact_version, level, score,
             recommended_action, confidence, reason_summary, created_at, updated_at)
         VALUES
            ('assessment-1', 'session-1', 'finding-1', 1, 'simple', 10,
             'implement_directly', 1.0, 'retired',
             '2026-07-20T00:00:00Z', '2026-07-20T00:00:00Z')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO plan_branches
            (id, plan_artifact_id, session_id, project_id, branch_name, source_branch)
         VALUES ('branch-1', 'finding-1', 'session-1', 'project-1', 'finding', 'main')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO agent_conversation_workspaces
            (conversation_id, project_id, mode, base_ref_kind, base_ref, branch_name,
             worktree_path, linked_plan_branch_id, created_at, updated_at)
         VALUES
            ('conversation-1', 'project-1', 'edit', 'project_default', 'main',
             'review-branch', '/tmp/review', 'branch-1',
             '2026-07-20T00:00:00Z', '2026-07-20T00:00:00Z')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO agent_workspace_pr_review_monitors
            (conversation_id, project_id, pr_number, review_artifact_id,
             review_artifact_version, review_artifact_head_sha, review_artifact_updated_at,
             created_at, updated_at)
         VALUES
            ('conversation-1', 'project-1', 810, 'finding-1', 1, 'head-sha',
             '2026-07-20T00:00:00Z', '2026-07-20T00:00:00Z', '2026-07-20T00:00:00Z')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO agent_workspace_review_monitors
            (conversation_id, project_id, review_artifact_id, review_artifact_version,
             review_artifact_updated_at, previous_version_id,
             review_gate_bypassed_at, review_gate_bypassed_target_scope,
             review_gate_bypassed_diff_fingerprint, review_gate_bypassed_artifact_id,
             review_gate_bypassed_artifact_version, created_at, updated_at)
         VALUES
            ('conversation-1', 'project-1', 'finding-1', 1,
             '2026-07-20T00:00:00Z', 'finding-1',
             '2026-07-20T00:00:00Z', 'branch', 'diff', 'finding-1', 1,
             '2026-07-20T00:00:00Z', '2026-07-20T00:00:00Z')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO agent_workspace_review_hunk_annotations
            (id, conversation_id, project_id, artifact_id, artifact_version,
             target_scope, diff_fingerprint, path, diff_source, hunk_header,
             old_start, old_lines, new_start, new_lines, message, level, created_at)
         VALUES
            ('annotation-1', 'conversation-1', 'project-1', 'finding-1', 1,
             'branch', 'diff', 'src/lib.rs', 'local', '@@',
             1, 1, 1, 1, 'retired', 'warning', '2026-07-20T00:00:00Z')",
        [],
    )
    .unwrap();
    conn.execute(
        "UPDATE tasks SET plan_artifact_id = 'finding-1' WHERE id = 'task-1'",
        [],
    )
    .unwrap();
    conn.execute(
        "UPDATE ideation_sessions
         SET plan_artifact_id = 'finding-1',
             inherited_plan_artifact_id = 'finding-1',
             verified_plan_artifact_id = 'finding-1',
             verified_plan_agent_run_id = 'run-1'
         WHERE id = 'session-1'",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO notifications
            (id, created_at, project_id, category, severity, title)
         VALUES
            ('team-plan-notification-1', '2026-07-20T00:00:00Z', 'project-1',
             'team_plan_approval', 'action_required', 'Retired team plan')",
        [],
    )
    .unwrap();
    conn.execute_batch(
        "CREATE INDEX idx_chat_conversations_cleanup_test
             ON chat_conversations(context_type);
         CREATE TRIGGER trg_chat_conversations_cleanup_test
             AFTER INSERT ON chat_conversations
             BEGIN
                 SELECT 1;
             END;",
    )
    .unwrap();
    conn.execute("PRAGMA foreign_keys = ON", []).unwrap();
    conn.execute("PRAGMA legacy_alter_table = OFF", []).unwrap();

    run_migrations(&conn).expect("registered cleanup migration must run during upgrade");
    v20260720140000_remove_legacy_claude_team::migrate(&conn)
        .expect("legacy cleanup is idempotent");
    run_migrations(&conn).expect("completed migration chain is idempotent");

    assert_eq!(get_schema_version(&conn).unwrap(), SCHEMA_VERSION);
    assert!(super::v20260521222911_agent_plan_mode::foreign_keys_enabled(&conn).unwrap());
    assert!(!super::v20260521222911_agent_plan_mode::legacy_alter_table_enabled(&conn).unwrap());
    assert!(!table_exists(&conn, "team_messages"));
    assert!(!table_exists(&conn, "team_sessions"));
    assert!(!column_exists(&conn, "ideation_sessions", "team_mode"));
    assert!(!column_exists(
        &conn,
        "ideation_sessions",
        "team_config_json"
    ));
    assert_eq!(
        conn.query_row(
            "SELECT coordination_mode FROM chat_conversations WHERE id = 'conversation-1'",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap(),
        "rx_native_team"
    );
    let metadata: String = conn
        .query_row(
            "SELECT metadata FROM tasks WHERE id = 'task-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(metadata, "{\"kept\":true}");
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM artifacts WHERE type = 'verification_finding'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        0
    );
    for table in [
        "plan_artifact_approvals",
        "plan_complexity_assessments",
        "agent_workspace_review_hunk_annotations",
        "plan_branches",
    ] {
        assert_eq!(
            conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| row
                .get::<_, i64>(0),)
                .unwrap(),
            0,
            "dependent rows in {table} should be removed"
        );
    }
    let task_plan_artifact_id: Option<String> = conn
        .query_row(
            "SELECT plan_artifact_id FROM tasks WHERE id = 'task-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(task_plan_artifact_id, None);
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM notifications WHERE category = 'team_plan_approval'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        0
    );
    let session_artifact_ids: (
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    ) = conn
        .query_row(
            "SELECT plan_artifact_id, inherited_plan_artifact_id,
                    verified_plan_artifact_id, verified_plan_agent_run_id
             FROM ideation_sessions WHERE id = 'session-1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(session_artifact_ids, (None, None, None, None));
    let linked_plan_branch_id: Option<String> = conn
        .query_row(
            "SELECT linked_plan_branch_id FROM agent_conversation_workspaces
             WHERE conversation_id = 'conversation-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(linked_plan_branch_id, None);
    let pr_review_artifact: (Option<String>, Option<i64>, Option<String>, Option<String>) = conn
        .query_row(
            "SELECT review_artifact_id, review_artifact_version,
                    review_artifact_head_sha, review_artifact_updated_at
             FROM agent_workspace_pr_review_monitors
             WHERE conversation_id = 'conversation-1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(pr_review_artifact, (None, None, None, None));
    let review_artifact: (
        Option<String>,
        Option<i64>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<i64>,
    ) = conn
        .query_row(
            "SELECT review_artifact_id, review_artifact_version, previous_version_id,
                    review_gate_bypassed_at, review_gate_bypassed_artifact_id,
                    review_gate_bypassed_artifact_version
             FROM agent_workspace_review_monitors
             WHERE conversation_id = 'conversation-1'",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(review_artifact, (None, None, None, None, None, None));
    let preserved: (String, String, Option<String>) = conn
        .query_row(
            "SELECT created_by, metadata_json, previous_version_id FROM artifacts WHERE id = 'summary-1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        preserved,
        (
            "system".into(),
            "{\"author_teammate\":\"system\",\"unrelated_owner\":\"team-lead\"}".into(),
            None
        )
    );
    let bucket_config: String = conn
        .query_row(
            "SELECT config_json FROM artifact_buckets WHERE id = 'team-findings'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        bucket_config,
        "{\"accepted_types\":[\"team_research\",\"team_analysis\",\"team_summary\"],\"writers\":[\"system\"],\"readers\":[\"all\"]}"
    );
    let create_sql: String = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'chat_conversations'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(!create_sql.contains("legacy_claude_team"));
    for coordination_mode in [
        "solo",
        "rx_native_team",
        "rx_native_workflow",
        "codex_native_ultra",
    ] {
        conn.execute(
            "UPDATE chat_conversations SET coordination_mode = ?1 WHERE id = 'conversation-1'",
            [coordination_mode],
        )
        .unwrap();
    }
    assert!(conn
        .execute(
            "UPDATE chat_conversations
             SET coordination_mode = 'legacy_claude_team'
             WHERE id = 'conversation-1'",
            [],
        )
        .is_err());
    for object_name in [
        "idx_chat_conversations_cleanup_test",
        "trg_chat_conversations_cleanup_test",
    ] {
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name = ?1",
                [object_name],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            1,
            "dependent object {object_name} should survive the CHECK rewrite"
        );
    }
}

#[test]
fn migration_rolls_back_all_state_when_destructive_cleanup_fails() {
    let conn = open_memory_connection().expect("create memory db");
    run_migrations_through(&conn, PREVIOUS_SCHEMA_VERSION).expect("run prior migrations");
    conn.execute(
        "INSERT INTO projects (id, name, working_directory) VALUES ('project-1', 'Project', '/tmp/project')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO chat_conversations (id, context_type, context_id, coordination_mode)
         VALUES ('conversation-1', 'project', 'project-1', 'legacy_claude_team')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO artifacts
            (id, type, name, content_type, content_text, created_by)
         VALUES ('finding-1', 'verification_finding', 'Finding', 'text', 'retired', 'system')",
        [],
    )
    .unwrap();
    conn.execute_batch(
        "CREATE TRIGGER reject_finding_delete
         BEFORE DELETE ON artifacts
         WHEN OLD.type = 'verification_finding'
         BEGIN
             SELECT RAISE(ABORT, 'injected cleanup failure');
         END;",
    )
    .unwrap();
    conn.execute("PRAGMA foreign_keys = ON", []).unwrap();
    conn.execute("PRAGMA legacy_alter_table = OFF", []).unwrap();

    let error = v20260720140000_remove_legacy_claude_team::migrate(&conn)
        .expect_err("injected failure must abort migration");
    assert!(error.to_string().contains("injected cleanup failure"));
    assert_eq!(
        conn.query_row(
            "SELECT coordination_mode FROM chat_conversations WHERE id = 'conversation-1'",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap(),
        "legacy_claude_team"
    );
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM artifacts WHERE id = 'finding-1'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        1
    );
    assert!(table_exists(&conn, "team_messages"));
    assert!(table_exists(&conn, "team_sessions"));
    assert!(!table_exists(&conn, "chat_conversations_new_plan_mode"));
    assert!(!table_exists(&conn, "chat_conversations_old_plan_mode"));
    assert!(super::v20260521222911_agent_plan_mode::foreign_keys_enabled(&conn).unwrap());
    assert!(!super::v20260521222911_agent_plan_mode::legacy_alter_table_enabled(&conn).unwrap());
}

#[test]
fn migration_restores_pragmas_when_begin_immediate_is_locked() {
    let db = SqliteTestDb::new("legacy-team-removal-begin-lock");
    let lock_conn = db.new_connection();
    lock_conn
        .execute_batch("BEGIN IMMEDIATE")
        .expect("acquire competing write lock");

    let conn = db.new_connection();
    conn.busy_timeout(Duration::ZERO)
        .expect("disable busy wait for injected lock failure");
    conn.execute("PRAGMA foreign_keys = ON", []).unwrap();
    conn.execute("PRAGMA legacy_alter_table = OFF", []).unwrap();

    let error = v20260720140000_remove_legacy_claude_team::migrate(&conn)
        .expect_err("competing write lock must reject BEGIN IMMEDIATE");
    assert!(error.to_string().contains("database is locked"));
    assert!(super::v20260521222911_agent_plan_mode::foreign_keys_enabled(&conn).unwrap());
    assert!(!super::v20260521222911_agent_plan_mode::legacy_alter_table_enabled(&conn).unwrap());

    lock_conn
        .execute_batch("ROLLBACK")
        .expect("release competing write lock");
}

/// Live databases carry orphan rows this migration neither created nor cleans up:
/// foreign keys are enforced by default, but migrations that rewrite tables turn
/// them off, so deletes inside those windows can leave children behind. A
/// database-wide integrity check counted those pre-existing violations as
/// migration damage, so the migration failed, `AppState` initialization panicked,
/// and the app aborted on every launch with no way to recover.
#[test]
fn migration_ignores_preexisting_unrelated_foreign_key_violations() {
    let conn = open_memory_connection().expect("create memory db");
    run_migrations_through(&conn, PREVIOUS_SCHEMA_VERSION).expect("run prior migrations");
    conn.execute("PRAGMA foreign_keys = OFF", []).unwrap();
    conn.execute(
        "INSERT INTO external_issue_sync_records
            (id, link_id, sync_kind, idempotency_key, status)
         VALUES ('sync-orphan', 'missing-link', 'push', 'idem-orphan', 'pending')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO projects (id, name, working_directory) VALUES ('project-1', 'Project', '/tmp/project')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO chat_conversations (id, context_type, context_id, coordination_mode)
         VALUES ('conversation-1', 'project', 'project-1', 'legacy_claude_team')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO artifacts
            (id, type, name, content_type, content_text, created_by)
         VALUES ('finding-1', 'verification_finding', 'Finding', 'text', 'retired', 'system')",
        [],
    )
    .unwrap();

    v20260720140000_remove_legacy_claude_team::migrate(&conn)
        .expect("pre-existing unrelated violations must not block the migration");

    assert_eq!(
        conn.query_row(
            "SELECT coordination_mode FROM chat_conversations WHERE id = 'conversation-1'",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap(),
        "rx_native_team"
    );
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM artifacts WHERE id = 'finding-1'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        0
    );
    assert!(!table_exists(&conn, "team_sessions"));
    // The migration must not silently repair unrelated data it does not own.
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM external_issue_sync_records WHERE id = 'sync-orphan'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        1
    );
}

#[test]
fn migration_still_aborts_when_cleanup_introduces_new_foreign_key_violations() {
    let conn = open_memory_connection().expect("create memory db");
    run_migrations_through(&conn, PREVIOUS_SCHEMA_VERSION).expect("run prior migrations");
    conn.execute("PRAGMA foreign_keys = OFF", []).unwrap();
    // A pre-existing orphan on the same parent must not mask a violation the
    // migration itself introduces on that parent.
    conn.execute(
        "INSERT INTO external_issue_sync_records
            (id, link_id, sync_kind, idempotency_key, status)
         VALUES ('sync-orphan', 'missing-link', 'push', 'idem-orphan', 'pending')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO artifacts
            (id, type, name, content_type, content_text, created_by)
         VALUES ('finding-1', 'verification_finding', 'Finding', 'text', 'retired', 'system')",
        [],
    )
    .unwrap();
    conn.execute_batch(
        "CREATE TRIGGER inject_orphan_on_finding_delete
         AFTER DELETE ON artifacts
         WHEN OLD.type = 'verification_finding'
         BEGIN
             INSERT INTO external_issue_sync_records
                 (id, link_id, sync_kind, idempotency_key, status)
             VALUES ('sync-injected', 'missing-link', 'push', 'idem-injected', 'pending');
         END;",
    )
    .unwrap();

    let error = v20260720140000_remove_legacy_claude_team::migrate(&conn)
        .expect_err("newly introduced violations must abort the migration");
    assert!(
        error.to_string().contains("foreign-key violations"),
        "unexpected error: {error}"
    );
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM artifacts WHERE id = 'finding-1'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        1
    );
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM external_issue_sync_records WHERE id = 'sync-injected'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        0
    );
}
