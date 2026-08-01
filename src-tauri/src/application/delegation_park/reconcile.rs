use chrono::{Duration, Utc};

use crate::domain::entities::{DelegationParkState, DelegationWakeDecision, DelegationWakeReason};
use crate::domain::repositories::AgentRunRepository;
use crate::error::{AppError, AppResult};
use crate::infrastructure::agents::claude::delegation_config;

use super::DelegationParkService;

/// Counts durable parks and delegated jobs observed during one reconciliation pass.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DelegationParkReconcileSummary {
    pub parks_examined: usize,
    pub jobs_settled: usize,
    pub wake_attempts: usize,
}

impl DelegationParkService {
    /// Reconcile durable parks after startup or when live settlement notification was missed.
    ///
    /// # Errors
    ///
    /// Returns read/write errors so startup can record a failed reconciliation rather than
    /// treating an unreadable park as harmlessly idle.
    pub async fn reconcile_all(
        &self,
        agent_run_repo: &dyn AgentRunRepository,
    ) -> AppResult<DelegationParkReconcileSummary> {
        let now = Utc::now();
        let parks = self.park_repo.list_armed().await?;
        let mut summary = DelegationParkReconcileSummary {
            parks_examined: parks.len(),
            ..Default::default()
        };
        for park in parks {
            self.reconcile_park(&park, agent_run_repo, now, &mut summary)
                .await?;
        }

        let config = delegation_config();
        let retry_backoff_secs =
            i64::try_from(config.park_wake_retry_backoff_secs).map_err(|_| {
                AppError::Validation(
                    "delegation wake retry backoff exceeds supported duration".to_string(),
                )
            })?;
        let retry_window_secs =
            i64::from(config.park_wake_retry_max).saturating_mul(retry_backoff_secs);
        // A full configured retry window plus one configured backoff gives an in-flight dispatcher
        // time to settle; only a claim older than that is considered abandoned after a crash.
        let stale_threshold =
            Duration::seconds(retry_window_secs.saturating_add(retry_backoff_secs));
        let stalled = self
            .park_repo
            .list_wake_stalled(now - stale_threshold)
            .await?;
        summary.parks_examined += stalled.len();
        for park in stalled {
            if !self.park_repo.reset_wake_claim(&park.id).await? {
                continue;
            }
            let updated = self.park_repo.get(&park.id).await?.ok_or_else(|| {
                AppError::NotFound(format!(
                    "delegation park not found after wake reset: {}",
                    park.id
                ))
            })?;
            self.reconcile_park(&updated, agent_run_repo, now, &mut summary)
                .await?;
        }
        Ok(summary)
    }

    async fn reconcile_park(
        &self,
        park: &crate::domain::entities::DelegationPark,
        agent_run_repo: &dyn AgentRunRepository,
        now: chrono::DateTime<Utc>,
        summary: &mut DelegationParkReconcileSummary,
    ) -> AppResult<()> {
        for job in park.jobs.iter().filter(|job| job.settled_status.is_none()) {
            let run = agent_run_repo
                .get_by_id(&job.delegated_agent_run_id)
                .await?;
            if let Some(run) = run.filter(|run| run.status.is_terminal()) {
                self.park_repo
                    .record_job_settled(
                        &park.id,
                        &job.delegated_agent_run_id,
                        &run.status.to_string(),
                    )
                    .await?;
                summary.jobs_settled += 1;
            }
        }

        let Some(updated) = self.park_repo.get(&park.id).await? else {
            return Ok(());
        };
        if updated.is_expired(now) {
            self.dispatch_wake_as(
                &updated,
                DelegationWakeReason::Deadline,
                DelegationParkState::Expired,
            )
            .await?;
            summary.wake_attempts += 1;
        } else if let DelegationWakeDecision::Wake(reason) = updated.wake_decision() {
            self.dispatch_wake(&updated, reason).await?;
            summary.wake_attempts += 1;
        }
        Ok(())
    }
}
