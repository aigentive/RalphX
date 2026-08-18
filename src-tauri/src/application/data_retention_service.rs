//! Orchestrates chat payload retention: the always-on time-window prune plus the
//! opt-in, oldest-first size-budget prune.
//!
//! Consumed by the startup job, the periodic runtime loop, and the "Run cleanup now"
//! command. Size pruning is fail-closed — it runs only when the policy row carries both
//! a budget and a recorded user confirmation, so a default install never deletes
//! anything inside its retention window.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::domain::entities::data_retention::{
    DataRetentionDefaults, DataRetentionRunStatus, DataRetentionSettings,
};
use crate::error::{AppError, AppResult};
use crate::infrastructure::agents::claude::{DatabaseMaintenanceConfig, StreamTimeoutsConfig};
use crate::infrastructure::sqlite::sqlite_chat_payload_retention_repo::{
    PayloadUsage, PruneCursor, SqliteChatPayloadRetentionRepository,
};
use crate::infrastructure::sqlite::sqlite_data_retention_settings_repo::SqliteDataRetentionSettingsRepository;

/// Upper bound on size-budget batches per cycle so an unreachable budget cannot spin forever.
const MAX_SIZE_BUDGET_BATCHES: u64 = 100_000;

/// Rows per bounded usage-measurement read. A single unbounded `SUM(LENGTH(...))` over the payload
/// table held a pooled connection for 175s on the production database, queueing every other
/// caller behind it at startup.
const USAGE_SCAN_BATCH_ROWS: u32 = 20_000;

/// Process-global because `AppState` builds application services per call: the command,
/// the detached startup cycle and the periodic loop each hold a *different* service
/// instance, so an instance field would guard nothing.
static RETENTION_CYCLE_RUNNING: AtomicBool = AtomicBool::new(false);

/// Serializes cycle-running tests across suites. The guard above is process-global by
/// design, so two overlapping test cycles would make one of them fail with `Conflict`.
#[doc(hidden)]
pub static CYCLE_TEST_SERIALIZER: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Releases the process-global cycle claim on drop, including on early return and panic.
pub struct RetentionCycleGuard;

impl RetentionCycleGuard {
    /// Claims the process-global cycle slot, or returns `None` when one is already running.
    #[must_use]
    pub fn try_acquire() -> Option<Self> {
        RETENTION_CYCLE_RUNNING
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
            .then_some(Self)
    }
}

impl Drop for RetentionCycleGuard {
    fn drop(&mut self) {
        RETENTION_CYCLE_RUNNING.store(false, Ordering::Release);
    }
}

/// Cadence and threshold knobs. All values come from `runtime_config` (no inline consts).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionTuning {
    pub batch_pause: Duration,
    pub checkpoint_batches: u64,
    pub advisory_threshold_bytes: u64,
    pub compaction_recommended_freelist_percent: u64,
}

impl RetentionTuning {
    #[must_use]
    pub fn new(stream: &StreamTimeoutsConfig, maintenance: &DatabaseMaintenanceConfig) -> Self {
        Self {
            batch_pause: Duration::from_millis(stream.chat_payload_retention_batch_pause_ms),
            checkpoint_batches: stream.chat_payload_retention_checkpoint_batches,
            advisory_threshold_bytes: stream.chat_payload_advisory_threshold_bytes,
            compaction_recommended_freelist_percent: maintenance
                .db_auto_compact_min_freelist_percent,
        }
    }
}

/// Why a cycle stopped short of enforcing the size budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetentionSkipReason {
    RetentionDisabled,
    SizeBudgetNotConfigured,
    SizeBudgetUnconfirmed,
    AlreadyUnderBudget,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetentionCycleReport {
    pub pruned_rows: u64,
    pub payload_bytes_after: Option<u64>,
    pub payload_rows_after: Option<u64>,
    pub database_bytes_after: u64,
    pub reclaimable_hint_bytes: u64,
    /// Deletes return pages to the freelist and never shrink the file; this is what tells
    /// the user the job is only half done.
    pub compaction_recommended: bool,
    pub size_budget_advised: bool,
    pub size_budget_active: bool,
    pub skipped_reason: Option<RetentionSkipReason>,
}

pub struct DataRetentionService {
    payload_repo: Arc<SqliteChatPayloadRetentionRepository>,
    settings_repo: Arc<SqliteDataRetentionSettingsRepository>,
    defaults: DataRetentionDefaults,
    tuning: RetentionTuning,
}

impl DataRetentionService {
    #[must_use]
    pub fn new(
        payload_repo: Arc<SqliteChatPayloadRetentionRepository>,
        settings_repo: Arc<SqliteDataRetentionSettingsRepository>,
        defaults: DataRetentionDefaults,
        tuning: RetentionTuning,
    ) -> Self {
        Self {
            payload_repo,
            settings_repo,
            defaults,
            tuning,
        }
    }

    /// Builds a service from a database handle and the live runtime config.
    ///
    /// `AppState` builds application services per call, so every caller (startup job,
    /// periodic loop, command) constructs its own instance; mutual exclusion comes from
    /// the process-global cycle guard, not from shared ownership.
    #[must_use]
    pub fn from_db(db: crate::infrastructure::sqlite::DbConnection) -> Self {
        let stream = crate::infrastructure::agents::claude::stream_timeouts();
        let maintenance = crate::infrastructure::agents::claude::database_maintenance_config();
        Self::new(
            Arc::new(SqliteChatPayloadRetentionRepository::from_db(db.clone())),
            Arc::new(SqliteDataRetentionSettingsRepository::from_db(db)),
            DataRetentionDefaults {
                enabled: stream.chat_payload_retention_enabled,
                days: stream.chat_payload_retention_days,
                archived_days: stream.chat_payload_retention_archived_days,
                batch_rows: stream.chat_payload_retention_batch_rows,
            },
            RetentionTuning::new(stream, maintenance),
        )
    }

    /// Reads (seeding when needed) the live policy row.
    ///
    /// # Errors
    ///
    /// Returns `AppError::Database` when the policy row cannot be read or seeded.
    pub async fn settings(&self) -> AppResult<DataRetentionSettings> {
        self.settings_repo.get_or_seed(self.defaults).await
    }

    /// Persists a user-authored policy change.
    ///
    /// # Errors
    ///
    /// Returns `AppError::Validation` when bounds or the size-budget consent invariant are
    /// violated, and `AppError::Database` on write failure.
    pub async fn update_policy(
        &self,
        update: crate::domain::entities::data_retention::DataRetentionPolicyUpdate,
    ) -> AppResult<DataRetentionSettings> {
        self.settings_repo.update(update).await
    }

    /// Read-only projection of what enabling (or lowering) a size budget would delete.
    ///
    /// # Errors
    ///
    /// Returns `AppError::Database` when the payload walk fails.
    pub async fn preview_size_budget(
        &self,
        budget: u64,
    ) -> AppResult<
        crate::infrastructure::sqlite::sqlite_chat_payload_retention_repo::SizeBudgetPreview,
    > {
        self.payload_repo.preview_size_budget_prune(budget).await
    }

    /// Runs one retention cycle: time-window prune, then the opt-in size-budget prune.
    ///
    /// # Errors
    ///
    /// Returns `AppError::Conflict` when another cycle already holds the process-global
    /// slot, and `AppError::Database` when a prune or measurement query fails.
    pub async fn run_cycle(&self, now: DateTime<Utc>) -> AppResult<RetentionCycleReport> {
        let _guard = RetentionCycleGuard::try_acquire().ok_or_else(cycle_already_running)?;
        self.run_cycle_inner(now).await
    }

    /// Runs a cycle when the caller already holds the process-global slot.
    ///
    /// # Errors
    ///
    /// Returns `AppError::Database` when a prune or measurement query fails.
    pub async fn run_cycle_with_guard(
        &self,
        _guard: &RetentionCycleGuard,
        now: DateTime<Utc>,
    ) -> AppResult<RetentionCycleReport> {
        self.run_cycle_inner(now).await
    }

    async fn run_cycle_inner(&self, now: DateTime<Utc>) -> AppResult<RetentionCycleReport> {
        let settings = self.settings_repo.get_or_seed(self.defaults).await?;
        if !settings.enabled {
            return self
                .report(
                    0,
                    None,
                    None,
                    false,
                    false,
                    Some(RetentionSkipReason::RetentionDisabled),
                )
                .await;
        }

        let batch_rows = u32::try_from(settings.batch_rows)
            .unwrap_or(u32::MAX)
            .max(1);
        let mut pruned_rows = self.prune_time_window(&settings, batch_rows, now).await?;

        let (payload_usage, advised, skipped) = match settings.enforced_size_budget_bytes() {
            Some(budget) => {
                let (usage, skipped) = self.measure_for_budget(budget).await?;
                match (usage, skipped) {
                    (Some(usage), None) => {
                        let (extra_rows, remaining_bytes) = self
                            .prune_to_budget(usage.total_bytes, budget, batch_rows)
                            .await?;
                        pruned_rows += extra_rows;
                        (
                            Some((remaining_bytes, usage.row_count.saturating_sub(extra_rows))),
                            false,
                            None,
                        )
                    }
                    (usage, skipped) => (
                        usage.map(|usage| (usage.total_bytes, usage.row_count)),
                        false,
                        skipped,
                    ),
                }
            }
            None => {
                let (measurement, advised) = self.measure_for_advisory().await?;
                (
                    measurement,
                    advised,
                    Some(if settings.size_budget_bytes.is_some() {
                        RetentionSkipReason::SizeBudgetUnconfirmed
                    } else {
                        RetentionSkipReason::SizeBudgetNotConfigured
                    }),
                )
            }
        };

        self.checkpoint().await;

        let (payload_bytes_after, payload_rows_after) = match payload_usage {
            Some((bytes, rows)) => (Some(bytes), Some(rows)),
            None => (None, None),
        };

        self.settings_repo
            .record_run_status(DataRetentionRunStatus {
                pruned_rows,
                payload_bytes: payload_bytes_after,
                payload_rows: payload_rows_after,
                size_budget_advised: advised,
                ran_at: now,
            })
            .await?;

        self.report(
            pruned_rows,
            payload_bytes_after,
            payload_rows_after,
            advised,
            settings.size_budget_active(),
            skipped,
        )
        .await
    }

    async fn prune_time_window(
        &self,
        settings: &DataRetentionSettings,
        batch_rows: u32,
        now: DateTime<Utc>,
    ) -> AppResult<u64> {
        let before = now - chrono::Duration::days(settings.days as i64);
        let archived_before = now - chrono::Duration::days(settings.archived_days as i64);
        let mut cursor: PruneCursor = None;
        let mut pruned = 0;
        let mut batches = 0;
        loop {
            let outcome = self
                .payload_repo
                .prune_batch(before, archived_before, batch_rows, cursor)
                .await?;
            if outcome.deleted_rows == 0 {
                self.checkpoint().await;
                return Ok(pruned);
            }
            pruned += outcome.deleted_rows;
            cursor = outcome.next_cursor;
            batches += 1;
            self.pause_between_batches(batches).await;
        }
    }

    /// Returns the measured usage plus a skip reason when the budget is already satisfied.
    async fn measure_for_budget(
        &self,
        budget: u64,
    ) -> AppResult<(
        Option<crate::infrastructure::sqlite::sqlite_chat_payload_retention_repo::PayloadUsage>,
        Option<RetentionSkipReason>,
    )> {
        // Constant-time upper bound first: on a healthy install this skips the full scan.
        if self.payload_repo.database_size_hint().await? <= budget {
            return Ok((None, Some(RetentionSkipReason::AlreadyUnderBudget)));
        }
        let usage = self.measure_payload_usage_bounded().await?;
        if usage.total_bytes <= budget {
            return Ok((Some(usage), Some(RetentionSkipReason::AlreadyUnderBudget)));
        }
        Ok((Some(usage), None))
    }

    /// Measures payload usage only when the database is large enough for the advisory to
    /// matter. Never deletes anything; without it the opt-in would be invisible on exactly
    /// the installs that need it.
    async fn measure_for_advisory(&self) -> AppResult<(Option<(u64, u64)>, bool)> {
        if self.payload_repo.database_size_hint().await? <= self.tuning.advisory_threshold_bytes {
            return Ok((None, false));
        }
        let usage = self.measure_payload_usage_bounded().await?;
        Ok((Some((usage.total_bytes, usage.row_count)), true))
    }

    /// Measures total payload usage in bounded batches, pausing on the same cadence as the prune
    /// loops. Equivalent to `payload_repo.payload_usage()` but never holds a pooled connection for
    /// more than one batch.
    async fn measure_payload_usage_bounded(&self) -> AppResult<PayloadUsage> {
        let mut usage = PayloadUsage::default();
        let mut cursor: Option<String> = None;
        let mut batches = 0;
        loop {
            let (partial, next_cursor) = self
                .payload_repo
                .payload_usage_batch(USAGE_SCAN_BATCH_ROWS, cursor)
                .await?;
            usage.total_bytes = usage.total_bytes.saturating_add(partial.total_bytes);
            usage.row_count = usage.row_count.saturating_add(partial.row_count);
            let Some(next_cursor) = next_cursor else {
                return Ok(usage);
            };
            cursor = Some(next_cursor);
            batches += 1;
            self.pause_between_batches(batches).await;
        }
    }

    /// Deletes oldest-first until the running total falls under budget. The total is kept
    /// incrementally from per-batch bytes — `payload_usage()` is never called a second time.
    async fn prune_to_budget(
        &self,
        total_bytes: u64,
        budget: u64,
        batch_rows: u32,
    ) -> AppResult<(u64, u64)> {
        let mut remaining = total_bytes;
        let mut cursor: PruneCursor = None;
        let mut pruned = 0;
        let mut batches = 0;
        while remaining > budget && batches < MAX_SIZE_BUDGET_BATCHES {
            let outcome = self
                .payload_repo
                .prune_oldest_batch(batch_rows, cursor)
                .await?;
            if outcome.deleted_rows == 0 {
                break;
            }
            remaining = remaining.saturating_sub(outcome.payload_bytes);
            pruned += outcome.deleted_rows;
            cursor = outcome.next_cursor;
            batches += 1;
            self.pause_between_batches(batches).await;
        }
        Ok((pruned, remaining))
    }

    async fn pause_between_batches(&self, batches: u64) {
        #[allow(unknown_lints, clippy::manual_is_multiple_of)]
        if self.tuning.checkpoint_batches != 0 && batches % self.tuning.checkpoint_batches == 0 {
            self.checkpoint().await;
        }
        if self.tuning.batch_pause.is_zero() {
            tokio::task::yield_now().await;
        } else {
            tokio::time::sleep(self.tuning.batch_pause).await;
        }
    }

    /// Best-effort WAL containment: a busy or failed checkpoint is logged, never fatal.
    async fn checkpoint(&self) {
        match self.payload_repo.checkpoint_truncate().await {
            Ok(outcome) => tracing::debug!(?outcome, "Retention WAL checkpoint"),
            Err(error) => tracing::warn!(
                error = %error,
                "Retention WAL checkpoint failed; retrying on the next cadence"
            ),
        }
    }

    async fn report(
        &self,
        pruned_rows: u64,
        payload_bytes_after: Option<u64>,
        payload_rows_after: Option<u64>,
        size_budget_advised: bool,
        size_budget_active: bool,
        skipped_reason: Option<RetentionSkipReason>,
    ) -> AppResult<RetentionCycleReport> {
        let database_bytes_after = self.payload_repo.database_size_hint().await?;
        let reclaimable_hint_bytes = self.payload_repo.reclaimable_hint_bytes().await?;
        Ok(RetentionCycleReport {
            pruned_rows,
            payload_bytes_after,
            payload_rows_after,
            database_bytes_after,
            reclaimable_hint_bytes,
            compaction_recommended: compaction_recommended(
                database_bytes_after,
                reclaimable_hint_bytes,
                self.tuning.compaction_recommended_freelist_percent,
            ),
            size_budget_advised,
            size_budget_active,
            skipped_reason,
        })
    }
}

/// True once the freelist is a significant share of the file — a prune reclaimed logical
/// space that only a compaction can hand back to the filesystem.
#[must_use]
pub fn compaction_recommended(
    database_bytes: u64,
    reclaimable_bytes: u64,
    min_freelist_percent: u64,
) -> bool {
    if database_bytes == 0 || min_freelist_percent == 0 {
        return false;
    }
    reclaimable_bytes.saturating_mul(100) / database_bytes >= min_freelist_percent
}

fn cycle_already_running() -> AppError {
    AppError::Conflict("A data retention cleanup is already running.".to_string())
}
