//! Durable record of the last startup compaction attempt.
//!
//! Compaction runs strictly before the SQLite pool opens, so its outcome cannot be
//! written to the database it is compacting. Without this sidecar a skipped manual
//! request consumed its marker and told the user nothing.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

pub const COMPACTION_OUTCOME_FILE_NAME: &str = "ralphx.db.compaction-outcome.json";

/// Terminal states of `compact_before_pool_opens_at`, as persisted for the UI.
pub const OUTCOME_COMPACTED: &str = "compacted";
pub const OUTCOME_SKIPPED: &str = "skipped";
pub const OUTCOME_ERROR: &str = "error";

/// Breadcrumb written before the non-atomic rename window of the swap. It survives only if
/// the process dies inside that window, where the live path holds no database and the
/// original sits in the backup directory. Any normal exit from the swap overwrites it.
pub const REASON_SWAP_INTERRUPTED: &str = "swap_interrupted";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactionRecord {
    /// One of [`OUTCOME_COMPACTED`], [`OUTCOME_SKIPPED`], [`OUTCOME_ERROR`].
    pub outcome: String,
    /// Skip reason or failing phase — the thing the old logs-only path threw away.
    pub reason: Option<String>,
    pub reclaimed_bytes: Option<u64>,
    pub database_bytes_before: u64,
    pub at_rfc3339: String,
}

impl CompactionRecord {
    #[must_use]
    pub fn compacted(database_bytes_before: u64, reclaimed_bytes: u64) -> Self {
        Self {
            outcome: OUTCOME_COMPACTED.to_string(),
            reason: None,
            reclaimed_bytes: Some(reclaimed_bytes),
            database_bytes_before,
            at_rfc3339: chrono::Utc::now().to_rfc3339(),
        }
    }

    #[must_use]
    pub fn skipped(database_bytes_before: u64, reason: &str) -> Self {
        Self {
            outcome: OUTCOME_SKIPPED.to_string(),
            reason: Some(reason.to_string()),
            reclaimed_bytes: None,
            database_bytes_before,
            at_rfc3339: chrono::Utc::now().to_rfc3339(),
        }
    }

    #[must_use]
    pub fn error(database_bytes_before: u64, phase: &str) -> Self {
        Self {
            outcome: OUTCOME_ERROR.to_string(),
            reason: Some(phase.to_string()),
            reclaimed_bytes: None,
            database_bytes_before,
            at_rfc3339: chrono::Utc::now().to_rfc3339(),
        }
    }
}

/// Best-effort write. A sidecar failure is never allowed to block startup.
pub fn write_record(outcome_path: &Path, record: &CompactionRecord) {
    let Ok(serialized) = serde_json::to_vec_pretty(record) else {
        tracing::warn!("Compaction outcome could not be serialized");
        return;
    };
    if let Some(parent) = outcome_path.parent() {
        if let Err(error) = fs::create_dir_all(parent) {
            tracing::warn!(%error, "Compaction outcome directory could not be created");
            return;
        }
    }
    if let Err(error) = fs::write(outcome_path, serialized) {
        tracing::warn!(%error, "Compaction outcome could not be recorded");
    }
}

/// Missing or corrupt sidecar reads as "no record", never as an error.
#[must_use]
pub fn read_record(outcome_path: &Path) -> Option<CompactionRecord> {
    let raw = fs::read(outcome_path).ok()?;
    serde_json::from_slice(&raw).ok()
}
