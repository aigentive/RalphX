use std::sync::Arc;
use std::time::Duration;

use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::domain::services::{RunningAgentKey, RunningAgentRegistry};

/// Keeps an owned `pid == 0` registry reservation alive during pre-spawn awaits.
pub(super) struct LaunchReservationGuard {
    stop: CancellationToken,
    task: JoinHandle<()>,
}

impl LaunchReservationGuard {
    pub(super) fn new(
        registry: Arc<dyn RunningAgentRegistry>,
        key: RunningAgentKey,
        agent_run_id: String,
        lease: Duration,
    ) -> Self {
        let stop = CancellationToken::new();
        let stop_for_task = stop.clone();
        let renewal_interval = lease
            .checked_div(3)
            .unwrap_or(Duration::from_secs(1))
            .max(Duration::from_millis(1));
        let task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(renewal_interval);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            interval.tick().await;
            loop {
                tokio::select! {
                    _ = stop_for_task.cancelled() => break,
                    _ = interval.tick() => {
                        match registry
                            .renew_reservation(&key, &agent_run_id, chrono::Utc::now())
                            .await
                        {
                            Ok(true) => {}
                            Ok(false) => {
                                tracing::warn!(
                                    context_type = %key.context_type,
                                    context_id = %key.context_id,
                                    agent_run_id,
                                    "Launch reservation renewal lost ownership"
                                );
                                break;
                            }
                            Err(error) => {
                                tracing::error!(
                                    context_type = %key.context_type,
                                    context_id = %key.context_id,
                                    agent_run_id,
                                    error = %error,
                                    "Launch reservation renewal failed"
                                );
                                break;
                            }
                        }
                    }
                }
            }
        });

        Self { stop, task }
    }

    pub(super) fn stop(&self) {
        self.stop.cancel();
    }
}

impl Drop for LaunchReservationGuard {
    fn drop(&mut self) {
        self.stop.cancel();
        self.task.abort();
    }
}
