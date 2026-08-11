// Migration v20260720140000: remove legacy Claude team persistence

use rusqlite::Connection;

use crate::error::{AppError, AppResult};

use super::helpers::{
    column_exists, foreign_key_violation_counts, introduced_violations, table_exists,
};

pub fn migrate(conn: &Connection) -> AppResult<()> {
    let foreign_keys_was_enabled =
        super::v20260521222911_agent_plan_mode::foreign_keys_enabled(conn)?;
    let legacy_alter_table_was_enabled =
        super::v20260521222911_agent_plan_mode::legacy_alter_table_enabled(conn)?;
    let migration_result = (|| {
        conn.execute("PRAGMA foreign_keys = OFF", [])
            .map_err(|error| AppError::Database(error.to_string()))?;
        conn.execute("PRAGMA legacy_alter_table = ON", [])
            .map_err(|error| AppError::Database(error.to_string()))?;
        conn.execute_batch("BEGIN IMMEDIATE")
            .map_err(|error| AppError::Database(error.to_string()))?;

        // Live databases carry orphan rows unrelated to legacy Claude team state.
        // Foreign keys are enforced by default (`SQLITE_DEFAULT_FOREIGN_KEYS=1`),
        // but migrations that rewrite tables disable them, so rows deleted inside
        // those windows can leave children behind. Baseline the violations here and
        // gate only on ones this migration introduces; counting pre-existing damage
        // as our own aborts startup permanently with no way to recover.
        let baseline_violations = foreign_key_violation_counts(conn)?;
        if !baseline_violations.is_empty() {
            tracing::warn!(
                "legacy Claude team removal: ignoring {} pre-existing foreign-key violation(s) not owned by this migration",
                baseline_violations.values().sum::<i64>()
            );
        }

        migrate_inner(conn).and_then(|()| {
            let introduced = introduced_violations(&baseline_violations, &foreign_key_violation_counts(conn)?);
            if !introduced.is_empty() {
                let violation_count: i64 = introduced.iter().map(|(_, _, count)| count).sum();
                let details = introduced
                    .iter()
                    .map(|(table, parent, count)| format!("{table} -> {parent} ({count})"))
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(AppError::Database(format!(
                    "legacy Claude team removal left {violation_count} foreign-key violations: {details}"
                )));
            }
            conn.execute_batch("COMMIT")
                .map_err(|error| AppError::Database(error.to_string()))
        })
    })();

    if migration_result.is_err() {
        let _ = conn.execute_batch("ROLLBACK");
    }
    let legacy_restore_result = conn
        .execute(
            if legacy_alter_table_was_enabled {
                "PRAGMA legacy_alter_table = ON"
            } else {
                "PRAGMA legacy_alter_table = OFF"
            },
            [],
        )
        .map_err(|error| AppError::Database(error.to_string()));
    let foreign_key_restore_result = conn
        .execute(
            if foreign_keys_was_enabled {
                "PRAGMA foreign_keys = ON"
            } else {
                "PRAGMA foreign_keys = OFF"
            },
            [],
        )
        .map_err(|error| AppError::Database(error.to_string()));

    migration_result?;
    legacy_restore_result?;
    foreign_key_restore_result?;
    Ok(())
}

fn migrate_inner(conn: &Connection) -> AppResult<()> {
    conn.execute(
        "UPDATE chat_conversations
         SET coordination_mode = 'rx_native_team'
         WHERE coordination_mode = 'legacy_claude_team'",
        [],
    )
    .map_err(|error| AppError::Database(error.to_string()))?;

    conn.execute(
        "UPDATE tasks
         SET metadata = NULLIF(json_remove(metadata, '$.agent_variant'), '{}')
         WHERE metadata IS NOT NULL
           AND json_valid(metadata)
           AND json_type(metadata, '$.agent_variant') IS NOT NULL",
        [],
    )
    .map_err(|error| AppError::Database(error.to_string()))?;

    conn.execute(
        "DELETE FROM notifications WHERE category = 'team_plan_approval'",
        [],
    )
    .map_err(|error| AppError::Database(error.to_string()))?;

    // Materialize the retiring ids before deleting the artifacts. Foreign keys are
    // disabled for the table rewrites below, so every FK and denormalized artifact
    // pointer must be cleaned explicitly before the final integrity check.
    conn.execute_batch(
        "DROP TABLE IF EXISTS temp.retired_legacy_team_artifact_ids;
         CREATE TEMP TABLE retired_legacy_team_artifact_ids (
             id TEXT PRIMARY KEY
         ) WITHOUT ROWID;
         INSERT INTO retired_legacy_team_artifact_ids (id)
         SELECT id FROM artifacts WHERE type = 'verification_finding';

         DELETE FROM artifact_relations
         WHERE from_artifact_id IN (SELECT id FROM retired_legacy_team_artifact_ids)
            OR to_artifact_id IN (SELECT id FROM retired_legacy_team_artifact_ids);
         UPDATE artifacts
         SET previous_version_id = NULL
         WHERE previous_version_id IN (SELECT id FROM retired_legacy_team_artifact_ids);

         DELETE FROM plan_artifact_approvals
         WHERE artifact_id IN (SELECT id FROM retired_legacy_team_artifact_ids);
         DELETE FROM plan_complexity_assessments
         WHERE artifact_id IN (SELECT id FROM retired_legacy_team_artifact_ids);
         DELETE FROM agent_workspace_review_hunk_annotations
         WHERE artifact_id IN (SELECT id FROM retired_legacy_team_artifact_ids);

         UPDATE tasks
         SET plan_artifact_id = NULL
         WHERE plan_artifact_id IN (SELECT id FROM retired_legacy_team_artifact_ids);
         UPDATE ideation_sessions
         SET plan_artifact_id = CASE
                 WHEN plan_artifact_id IN (SELECT id FROM retired_legacy_team_artifact_ids)
                 THEN NULL ELSE plan_artifact_id END,
             inherited_plan_artifact_id = CASE
                 WHEN inherited_plan_artifact_id IN (SELECT id FROM retired_legacy_team_artifact_ids)
                 THEN NULL ELSE inherited_plan_artifact_id END,
             verified_plan_artifact_id = CASE
                 WHEN verified_plan_artifact_id IN (SELECT id FROM retired_legacy_team_artifact_ids)
                 THEN NULL ELSE verified_plan_artifact_id END,
             verified_plan_agent_run_id = CASE
                 WHEN verified_plan_artifact_id IN (SELECT id FROM retired_legacy_team_artifact_ids)
                 THEN NULL ELSE verified_plan_agent_run_id END
         WHERE plan_artifact_id IN (SELECT id FROM retired_legacy_team_artifact_ids)
            OR inherited_plan_artifact_id IN (SELECT id FROM retired_legacy_team_artifact_ids)
            OR verified_plan_artifact_id IN (SELECT id FROM retired_legacy_team_artifact_ids);
         UPDATE task_proposals
         SET plan_artifact_id = NULL
         WHERE plan_artifact_id IN (SELECT id FROM retired_legacy_team_artifact_ids);

         UPDATE agent_conversation_workspaces
         SET linked_plan_branch_id = NULL
         WHERE linked_plan_branch_id IN (
             SELECT id FROM plan_branches
             WHERE plan_artifact_id IN (SELECT id FROM retired_legacy_team_artifact_ids)
         );
         DELETE FROM plan_branches
         WHERE plan_artifact_id IN (SELECT id FROM retired_legacy_team_artifact_ids);

         UPDATE automations
         SET spec_artifact_id = NULL
         WHERE spec_artifact_id IN (SELECT id FROM retired_legacy_team_artifact_ids);
         UPDATE automation_runs
         SET plan_last_parked_artifact_id = NULL
         WHERE plan_last_parked_artifact_id IN (SELECT id FROM retired_legacy_team_artifact_ids);
         UPDATE personas
         SET artifact_id = NULL
         WHERE artifact_id IN (SELECT id FROM retired_legacy_team_artifact_ids);

         UPDATE agent_workspace_pr_review_monitors
         SET review_artifact_id = NULL,
             review_artifact_version = NULL,
             review_artifact_head_sha = NULL,
             review_artifact_updated_at = NULL
         WHERE review_artifact_id IN (SELECT id FROM retired_legacy_team_artifact_ids);
         UPDATE agent_workspace_review_monitors
         SET review_artifact_id = CASE
                 WHEN review_artifact_id IN (SELECT id FROM retired_legacy_team_artifact_ids)
                 THEN NULL ELSE review_artifact_id END,
             review_artifact_version = CASE
                 WHEN review_artifact_id IN (SELECT id FROM retired_legacy_team_artifact_ids)
                 THEN NULL ELSE review_artifact_version END,
             review_artifact_updated_at = CASE
                 WHEN review_artifact_id IN (SELECT id FROM retired_legacy_team_artifact_ids)
                 THEN NULL ELSE review_artifact_updated_at END,
             previous_version_id = CASE
                 WHEN previous_version_id IN (SELECT id FROM retired_legacy_team_artifact_ids)
                 THEN NULL ELSE previous_version_id END,
             review_gate_bypassed_at = CASE
                 WHEN review_gate_bypassed_artifact_id IN (SELECT id FROM retired_legacy_team_artifact_ids)
                 THEN NULL ELSE review_gate_bypassed_at END,
             review_gate_bypassed_target_scope = CASE
                 WHEN review_gate_bypassed_artifact_id IN (SELECT id FROM retired_legacy_team_artifact_ids)
                 THEN NULL ELSE review_gate_bypassed_target_scope END,
             review_gate_bypassed_diff_fingerprint = CASE
                 WHEN review_gate_bypassed_artifact_id IN (SELECT id FROM retired_legacy_team_artifact_ids)
                 THEN NULL ELSE review_gate_bypassed_diff_fingerprint END,
             review_gate_bypassed_artifact_id = CASE
                 WHEN review_gate_bypassed_artifact_id IN (SELECT id FROM retired_legacy_team_artifact_ids)
                 THEN NULL ELSE review_gate_bypassed_artifact_id END,
             review_gate_bypassed_artifact_version = CASE
                 WHEN review_gate_bypassed_artifact_id IN (SELECT id FROM retired_legacy_team_artifact_ids)
                 THEN NULL ELSE review_gate_bypassed_artifact_version END
         WHERE review_artifact_id IN (SELECT id FROM retired_legacy_team_artifact_ids)
            OR previous_version_id IN (SELECT id FROM retired_legacy_team_artifact_ids)
            OR review_gate_bypassed_artifact_id IN (SELECT id FROM retired_legacy_team_artifact_ids);

         DELETE FROM artifacts
         WHERE id IN (SELECT id FROM retired_legacy_team_artifact_ids);
         DROP TABLE retired_legacy_team_artifact_ids;",
    )
    .map_err(|error| AppError::Database(error.to_string()))?;

    // Preserve active team artifacts while removing the retired synthetic lead identity.
    conn.execute(
        "UPDATE artifacts
         SET created_by = 'system'
         WHERE type IN ('team_research', 'team_analysis', 'team_summary')
           AND created_by = 'team-lead'",
        [],
    )
    .map_err(|error| AppError::Database(error.to_string()))?;
    for attribution_path in [
        "$.author",
        "$.author_teammate",
        "$.team_metadata.author_teammate",
    ] {
        conn.execute(
            "UPDATE artifacts
             SET metadata_json = json_set(metadata_json, ?1, 'system')
             WHERE type IN ('team_research', 'team_analysis', 'team_summary')
               AND metadata_json IS NOT NULL
               AND json_valid(metadata_json)
               AND json_extract(metadata_json, ?1) = 'team-lead'",
            [attribution_path],
        )
        .map_err(|error| AppError::Database(error.to_string()))?;
    }
    conn.execute(
        "UPDATE artifact_buckets
         SET config_json = '{\"accepted_types\":[\"team_research\",\"team_analysis\",\"team_summary\"],\"writers\":[\"system\"],\"readers\":[\"all\"]}'
         WHERE id = 'team-findings'",
        [],
    )
    .map_err(|error| AppError::Database(error.to_string()))?;

    // Messages must be removed before their parent sessions.
    conn.execute_batch(
        "DROP TABLE IF EXISTS team_messages;
         DROP TABLE IF EXISTS team_sessions;",
    )
    .map_err(|error| AppError::Database(error.to_string()))?;

    if column_exists(conn, "ideation_sessions", "team_config_json") {
        conn.execute(
            "ALTER TABLE ideation_sessions DROP COLUMN team_config_json",
            [],
        )
        .map_err(|error| AppError::Database(error.to_string()))?;
    }
    if column_exists(conn, "ideation_sessions", "team_mode") {
        conn.execute("ALTER TABLE ideation_sessions DROP COLUMN team_mode", [])
            .map_err(|error| AppError::Database(error.to_string()))?;
    }

    if table_exists(conn, "chat_conversations") {
        let create_sql = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'chat_conversations'",
                [],
                |row| row.get::<_, String>(0),
            )
            .map_err(|error| AppError::Database(error.to_string()))?;
        if create_sql.contains("legacy_claude_team") {
            super::v20260521222911_agent_plan_mode::rewrite_table_check_constraint(
                conn,
                "chat_conversations",
                "__legacy_claude_team_removed__",
                &[
                (
                    "CHECK(coordination_mode IN ('solo', 'legacy_claude_team', 'rx_native_team', 'rx_native_workflow', 'codex_native_ultra'))",
                    "CHECK(coordination_mode IN ('solo', 'rx_native_team', 'rx_native_workflow', 'codex_native_ultra'))",
                ),
                (
                    "CHECK (coordination_mode IN ('solo', 'legacy_claude_team', 'rx_native_team', 'rx_native_workflow', 'codex_native_ultra'))",
                    "CHECK (coordination_mode IN ('solo', 'rx_native_team', 'rx_native_workflow', 'codex_native_ultra'))",
                ),
                (
                    "CHECK(coordination_mode IN ('solo', 'legacy_claude_team', 'rx_native_team'))",
                    "CHECK(coordination_mode IN ('solo', 'rx_native_team'))",
                ),
                (
                    "CHECK (coordination_mode IN ('solo', 'legacy_claude_team', 'rx_native_team'))",
                    "CHECK (coordination_mode IN ('solo', 'rx_native_team'))",
                ),
                ],
                "legacy Claude team removal",
            )?;
        }
    }

    Ok(())
}
