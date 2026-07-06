use super::*;

/// Periodic watchdog that detects tasks stuck in Ready state and reschedules them.
///
/// Safety net for scenarios S5, S6, S7, S8 where the primary scheduling trigger
/// (on_enter(Ready) or on_exit completion) may have been missed due to:
/// - Lock contention in try_lock()
/// - Scheduler unavailable (None) when task became Ready
/// - Timing races with the 600ms spawn delay
/// - Max concurrent capacity temporarily blocking schedule
///
/// The watchdog scans for Ready tasks older than `stale_threshold_secs` every
/// `interval_secs` and calls `try_schedule_ready_tasks()` to reschedule them.
pub struct ReadyWatchdog {
    pub(super) scheduler: Arc<dyn TaskScheduler>,
    pub(super) task_repo: Arc<dyn crate::domain::repositories::TaskRepository>,
    pub(super) project_repo: Arc<dyn ProjectRepository>,
    /// How often to run the watchdog scan.
    pub(super) interval_secs: u64,
    /// How long a task must be in Ready state before being considered stale.
    pub(super) stale_threshold_secs: u64,
}

impl ReadyWatchdog {
    /// Create a new ReadyWatchdog with configuration from the default scheduler runtime config.
    pub fn new(
        scheduler: Arc<dyn TaskScheduler>,
        task_repo: Arc<dyn crate::domain::repositories::TaskRepository>,
        project_repo: Arc<dyn ProjectRepository>,
    ) -> Self {
        let sched_cfg = default_scheduler_runtime_config();
        Self {
            scheduler,
            task_repo,
            project_repo,
            interval_secs: sched_cfg.watchdog_interval_secs,
            stale_threshold_secs: sched_cfg.watchdog_stale_threshold_secs,
        }
    }

    /// Override the scan interval (builder pattern).
    pub fn with_interval_secs(mut self, interval_secs: u64) -> Self {
        self.interval_secs = interval_secs;
        self
    }

    /// Override the staleness threshold (builder pattern).
    pub fn with_stale_threshold_secs(mut self, threshold_secs: u64) -> Self {
        self.stale_threshold_secs = threshold_secs;
        self
    }

    #[doc(hidden)]
    pub fn stale_threshold_secs_for_test(&self) -> u64 {
        self.stale_threshold_secs
    }

    #[doc(hidden)]
    pub fn interval_secs_for_test(&self) -> u64 {
        self.interval_secs
    }

    /// Run one watchdog cycle: scan for stale Ready tasks and reschedule if any are found.
    ///
    /// Returns the number of stale/retryable tasks found (0 means no action was taken).
    pub async fn run_once(&self) -> usize {
        let stale_ready_tasks = match self
            .task_repo
            .get_stale_ready_tasks(self.stale_threshold_secs)
            .await
        {
            Ok(stale_tasks) => stale_tasks,
            Err(e) => {
                tracing::error!(
                    error = %e,
                    "Watchdog: failed to query stale Ready tasks"
                );
                return 0;
            }
        };

        let retryable_pending_review_count = match self
            .count_retryable_pending_review_tasks()
            .await
        {
            Ok(count) => count,
            Err(e) => {
                tracing::error!(
                    error = %e,
                    "Watchdog: failed to query retryable PendingReview tasks"
                );
                return stale_ready_tasks.len();
            }
        };

        let stale_ready_count = stale_ready_tasks.len();
        let total_count = stale_ready_count + retryable_pending_review_count;

        if total_count > 0 {
            tracing::warn!(
                stale_ready_count = stale_ready_count,
                retryable_pending_review_count = retryable_pending_review_count,
                threshold_secs = self.stale_threshold_secs,
                "Watchdog: found retryable tasks, triggering reschedule"
            );
            self.scheduler.try_schedule_ready_tasks().await;
        } else {
            tracing::debug!("Watchdog: no stale Ready or retryable PendingReview tasks found");
        }

        total_count
    }

    /// Run the watchdog loop indefinitely, sleeping `interval_secs` between cycles.
    ///
    /// This is intended to be spawned as a background task at application startup.
    pub async fn run_loop(&self) {
        let interval = std::time::Duration::from_secs(self.interval_secs);
        loop {
            tokio::time::sleep(interval).await;
            self.run_once().await;
        }
    }

    pub(super) async fn count_retryable_pending_review_tasks(
        &self,
    ) -> crate::error::AppResult<usize> {
        let projects = self.project_repo.get_all().await?;
        let now = Utc::now();
        let mut count = 0usize;

        for project in projects {
            let tasks = self
                .task_repo
                .get_by_status(&project.id, InternalStatus::PendingReview)
                .await?;

            for task in tasks {
                let Some(metadata_str) = task.metadata.as_deref() else {
                    continue;
                };
                let Ok(metadata_val) = serde_json::from_str::<serde_json::Value>(metadata_str) else {
                    continue;
                };
                let freshness = FreshnessMetadata::from_task_metadata(&metadata_val);
                let Some(backoff_until) = freshness.freshness_backoff_until else {
                    continue;
                };
                if freshness.freshness_origin_state.as_deref() == Some("reviewing")
                    && now >= backoff_until
                {
                    count += 1;
                }
            }
        }

        Ok(count)
    }
}
