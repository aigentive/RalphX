//! Migration v20260811194643: durable Workspace Review settlement evidence.
//!
//! Three column groups, all additive and nullable:
//!
//! 1. **Recorded artifact outcome** — the typed disposition the reviewer stamps on its final
//!    artifact write, plus the run that stamped it and the blocking summary captured at that
//!    moment. Degraded settlement reads these when the reviewer wrapper times out. The run id is
//!    load-bearing: without it a re-review of an unchanged delta could settle from the previous
//!    run's evidence.
//! 2. **Annotator authority** — the run the backend registered as the post-settlement hunk
//!    annotator, plus `file_patch_hash` on annotation rows so unchanged files can carry their
//!    annotations forward to the next review cycle instead of being re-annotated.
//! 3. **Previous-review snapshot** — captured once at review start, before the current run
//!    overwrites the live `reviewed_*` fields, so incremental re-review can never self-reference.

use rusqlite::Connection;

use crate::error::AppResult;
use crate::infrastructure::sqlite::migrations::helpers;

pub fn migrate(conn: &Connection) -> AppResult<()> {
    for (column, ty) in [
        // Recorded artifact outcome
        ("review_artifact_recorded_outcome", "TEXT"),
        ("review_artifact_recorded_outcome_run_id", "TEXT"),
        ("review_artifact_recorded_blocking_summary", "TEXT"),
        ("review_settlement_source", "TEXT"),
        // Annotator authority
        ("annotation_run_id", "TEXT"),
        // Previous-review snapshot
        ("previous_review_artifact_id", "TEXT"),
        ("previous_review_requested_changes_artifact_id", "TEXT"),
        ("previous_review_artifact_version", "INTEGER"),
        ("previous_review_diff_fingerprint", "TEXT"),
        ("previous_review_head_sha", "TEXT"),
        ("previous_review_outcome", "TEXT"),
    ] {
        helpers::add_column_if_not_exists(conn, "agent_workspace_review_monitors", column, ty)?;
    }

    helpers::add_column_if_not_exists(
        conn,
        "agent_workspace_review_hunk_annotations",
        "file_patch_hash",
        "TEXT",
    )
}
