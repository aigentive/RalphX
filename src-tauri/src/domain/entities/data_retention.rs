//! Chat payload retention policy types.
//!
//! The live policy lives in the `data_retention_settings` table. Config only supplies
//! defaults for installs the user has never customized, and it can never supply an
//! active size budget: size-based pruning deletes payloads that are still inside the
//! time window, so it requires a recorded user confirmation.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

/// Smallest size budget a user may set. Below this the cap would prune constantly.
pub const MIN_SIZE_BUDGET_BYTES: u64 = 268_435_456; // 256 MiB
pub const MAX_RETENTION_BATCH_ROWS: u64 = 10_000;
pub const MIN_RETENTION_BATCH_ROWS: u64 = 1;
pub const MIN_RETENTION_DAYS: u64 = 1;

/// Shipped defaults sourced from `config/ralphx.yaml` (+ env overrides).
///
/// Deliberately carries no size-budget member: config must not be able to enable
/// deletion inside the retention window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataRetentionDefaults {
    pub enabled: bool,
    pub days: u64,
    pub archived_days: u64,
    pub batch_rows: u64,
}

impl DataRetentionDefaults {
    /// Clamps env/config values into the same bounds `update` enforces.
    ///
    /// A `batch_rows` of `0` would make every `DELETE` a silent no-op while the
    /// retention cycle still reported success, so seeding may never persist it.
    #[must_use]
    pub fn clamped(self) -> Self {
        Self {
            enabled: self.enabled,
            days: self.days.max(MIN_RETENTION_DAYS),
            archived_days: self.archived_days.max(MIN_RETENTION_DAYS),
            batch_rows: self
                .batch_rows
                .clamp(MIN_RETENTION_BATCH_ROWS, MAX_RETENTION_BATCH_ROWS),
        }
    }
}

/// The persisted policy row plus its last-run measurements.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataRetentionSettings {
    pub enabled: bool,
    pub days: u64,
    pub archived_days: u64,
    pub batch_rows: u64,
    /// `None` = size-based pruning disabled. This is the shipped state.
    pub size_budget_bytes: Option<u64>,
    /// Server-recorded consent. Without it a budget is inert (and rejected at write time).
    pub size_budget_confirmed_at: Option<DateTime<Utc>>,
    /// True while the row still tracks config defaults (never edited by the user).
    pub seeded_pristine: bool,
    /// Persisted advisory: payload data is large enough that a size budget would help.
    pub size_budget_advised: bool,
    pub last_run_at: Option<DateTime<Utc>>,
    pub last_run_pruned_rows: Option<u64>,
    pub last_run_payload_bytes: Option<u64>,
    pub last_run_payload_rows: Option<u64>,
    pub updated_at: DateTime<Utc>,
}

impl DataRetentionSettings {
    /// Size pruning is fail-closed: it needs both a budget and a recorded confirmation.
    #[must_use]
    pub fn size_budget_active(&self) -> bool {
        self.size_budget_bytes.is_some() && self.size_budget_confirmed_at.is_some()
    }

    /// The budget to enforce, or `None` when either half of the opt-in is missing.
    #[must_use]
    pub fn enforced_size_budget_bytes(&self) -> Option<u64> {
        if self.size_budget_active() {
            self.size_budget_bytes
        } else {
            None
        }
    }
}

/// A user-authored policy change. The only writer of the two size-budget columns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataRetentionPolicyUpdate {
    pub enabled: bool,
    pub days: u64,
    pub archived_days: u64,
    pub batch_rows: u64,
    pub size_budget_bytes: Option<u64>,
    /// Stamped server-side when the user confirms; never accepted from a caller.
    pub size_budget_confirmed_at: Option<DateTime<Utc>>,
}

impl DataRetentionPolicyUpdate {
    /// Validates policy bounds and the size-budget consent invariant.
    ///
    /// # Errors
    ///
    /// Returns `AppError::Validation` when a window/batch bound is violated, when the
    /// budget is below [`MIN_SIZE_BUDGET_BYTES`], or when a budget is submitted without
    /// a confirmation timestamp (an unconfirmed budget must be unrepresentable).
    pub fn validate(&self) -> AppResult<()> {
        if self.days < MIN_RETENTION_DAYS || self.archived_days < MIN_RETENTION_DAYS {
            return Err(AppError::Validation(
                "Retention windows must be at least 1 day.".to_string(),
            ));
        }
        if !(MIN_RETENTION_BATCH_ROWS..=MAX_RETENTION_BATCH_ROWS).contains(&self.batch_rows) {
            return Err(AppError::Validation(format!(
                "Retention batch size must be between {MIN_RETENTION_BATCH_ROWS} and {MAX_RETENTION_BATCH_ROWS} rows."
            )));
        }
        match (self.size_budget_bytes, self.size_budget_confirmed_at) {
            (Some(budget), Some(_)) if budget < MIN_SIZE_BUDGET_BYTES => Err(AppError::Validation(
                format!("Size budget must be at least {MIN_SIZE_BUDGET_BYTES} bytes."),
            )),
            (Some(_), None) => Err(AppError::Validation(
                "A size budget requires an explicit user confirmation.".to_string(),
            )),
            (None, Some(_)) => Err(AppError::Validation(
                "Clearing the size budget must clear its confirmation.".to_string(),
            )),
            _ => Ok(()),
        }
    }
}

/// Measurements persisted after a retention cycle so Settings can render without a scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataRetentionRunStatus {
    pub pruned_rows: u64,
    pub payload_bytes: Option<u64>,
    pub payload_rows: Option<u64>,
    pub size_budget_advised: bool,
    pub ran_at: DateTime<Utc>,
}
