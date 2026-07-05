use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use dashmap::mapref::entry::Entry;
use dashmap::DashMap;

use crate::application::automation::service::AutomationService;
use crate::application::automation::transition::NoopAutomationEventEmitter;
use crate::application::harness_runtime_registry::{
    default_automation_judge_timeout_secs, default_automation_max_run_duration_secs,
    default_automation_publish_grace_secs, default_automation_scheduler_poll_secs,
    default_automation_signal_failure_pause_threshold,
};
use crate::domain::entities::{AutomationId, AutomationStatus};
use crate::domain::repositories::{AutomationRepository, AutomationRunRepository};
use crate::error::AppResult;
use crate::infrastructure::agents::claude::AutomationsRuntimeConfig;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationSchedulerConfig {
    pub poll_interval: Duration,
    pub signal_failure_pause_threshold: u64,
    pub judge_timeout: Duration,
    pub publish_grace: Duration,
    pub max_run_duration: Duration,
}

impl AutomationSchedulerConfig {
    pub fn from_runtime(config: &AutomationsRuntimeConfig) -> Self {
        Self {
            poll_interval: Duration::from_secs(config.scheduler_poll_secs.max(1)),
            signal_failure_pause_threshold: config.signal_failure_pause_threshold.max(1),
            judge_timeout: Duration::from_secs(config.judge_timeout_secs.max(1)),
            publish_grace: Duration::from_secs(config.publish_grace_secs),
            max_run_duration: Duration::from_secs(config.max_run_duration_secs.max(1)),
        }
    }
}

impl Default for AutomationSchedulerConfig {
    fn default() -> Self {
        Self::from_runtime(&AutomationsRuntimeConfig {
            scheduler_poll_secs: default_automation_scheduler_poll_secs(),
            signal_failure_pause_threshold: default_automation_signal_failure_pause_threshold(),
            judge_timeout_secs: default_automation_judge_timeout_secs(),
            publish_grace_secs: default_automation_publish_grace_secs(),
            max_run_duration_secs: default_automation_max_run_duration_secs(),
        })
    }
}

#[derive(Debug, Default)]
pub struct AutomationSchedulerTickSummary {
    pub total_automations: usize,
    pub active_automations: usize,
    pub leased_automations: usize,
    pub active_without_runs: usize,
    pub active_with_runs: usize,
    pub automation_errors: usize,
}

#[derive(Debug, Default)]
pub struct AutomationSchedulerRegistry {
    loop_started: AtomicBool,
    automation_leases: DashMap<String, Instant>,
}

impl AutomationSchedulerRegistry {
    pub fn try_start_loop(&self) -> bool {
        self.loop_started
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    pub fn has_started_loop(&self) -> bool {
        self.loop_started.load(Ordering::SeqCst)
    }

    pub fn try_acquire_automation<'a>(
        &'a self,
        automation_id: &AutomationId,
        now: Instant,
        ttl: Duration,
    ) -> Option<AutomationSchedulerLease<'a>> {
        let key = automation_id.as_str().to_string();
        match self.automation_leases.entry(key.clone()) {
            Entry::Occupied(entry) if *entry.get() > now => None,
            Entry::Occupied(mut entry) => {
                entry.insert(now + ttl);
                Some(AutomationSchedulerLease {
                    registry: self,
                    key,
                })
            }
            Entry::Vacant(entry) => {
                entry.insert(now + ttl);
                Some(AutomationSchedulerLease {
                    registry: self,
                    key,
                })
            }
        }
    }

    fn release_automation(&self, key: &str) {
        self.automation_leases.remove(key);
    }
}

pub struct AutomationSchedulerLease<'a> {
    registry: &'a AutomationSchedulerRegistry,
    key: String,
}

impl Drop for AutomationSchedulerLease<'_> {
    fn drop(&mut self) {
        self.registry.release_automation(&self.key);
    }
}

pub fn global_automation_scheduler_registry() -> Arc<AutomationSchedulerRegistry> {
    static REGISTRY: OnceLock<Arc<AutomationSchedulerRegistry>> = OnceLock::new();
    Arc::clone(REGISTRY.get_or_init(|| Arc::new(AutomationSchedulerRegistry::default())))
}

pub struct AutomationScheduler {
    service: AutomationService,
    registry: Arc<AutomationSchedulerRegistry>,
    config: AutomationSchedulerConfig,
}

impl AutomationScheduler {
    pub fn new(
        automation_repo: Arc<dyn AutomationRepository>,
        run_repo: Arc<dyn AutomationRunRepository>,
        registry: Arc<AutomationSchedulerRegistry>,
        config: AutomationSchedulerConfig,
    ) -> Self {
        let service = AutomationService::new(
            automation_repo,
            run_repo,
            Arc::new(NoopAutomationEventEmitter),
        );
        Self {
            service,
            registry,
            config,
        }
    }

    pub fn config(&self) -> &AutomationSchedulerConfig {
        &self.config
    }

    pub async fn tick_once(&self) -> AppResult<AutomationSchedulerTickSummary> {
        let automations = self.service.list_automations(None).await?;
        let mut summary = AutomationSchedulerTickSummary {
            total_automations: automations.len(),
            ..AutomationSchedulerTickSummary::default()
        };

        for automation in automations
            .into_iter()
            .filter(|automation| automation.status == AutomationStatus::Active)
        {
            summary.active_automations += 1;
            let Some(_lease) = self.registry.try_acquire_automation(
                &automation.id,
                Instant::now(),
                self.config.poll_interval,
            ) else {
                continue;
            };
            summary.leased_automations += 1;

            match self.service.get_automation_detail(&automation.id).await {
                Ok(detail) if detail.runs.is_empty() => {
                    summary.active_without_runs += 1;
                }
                Ok(_) => {
                    summary.active_with_runs += 1;
                }
                Err(error) => {
                    summary.automation_errors += 1;
                    tracing::warn!(
                        automation_id = %automation.id,
                        error = %error,
                        "Automation scheduler failed to load automation detail"
                    );
                }
            }
        }

        Ok(summary)
    }
}
