use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use dashmap::mapref::entry::Entry;
use dashmap::DashMap;

use crate::application::automation::provisioning::{
    AutomationRunProvisioner, AutomationRunStarter,
};
use crate::application::automation::service::AutomationService;
use crate::application::automation::transition::AutomationTransitionService;
use crate::application::automation::transition::NoopAutomationEventEmitter;
use crate::application::harness_runtime_registry::{
    default_automation_judge_timeout_secs, default_automation_max_run_duration_secs,
    default_automation_publish_grace_secs, default_automation_scheduler_poll_secs,
    default_automation_signal_failure_pause_threshold,
};
use crate::domain::entities::{
    AgentConversationWorkspace, Automation, AutomationId, AutomationRun, AutomationRunStatus,
    AutomationStatus,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, AutomationRepository, AutomationRunPublicationMetadata,
    AutomationRunRepository, ChatConversationRepository,
};
use crate::domain::services::github_service::{GithubServiceTrait, PrStatus};
use crate::error::{AppError, AppResult};
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
    pub provisioned_runs: usize,
    pub published_runs: usize,
    pub merged_runs: usize,
    pub closed_runs: usize,
    pub failed_runs: usize,
    pub signal_check_errors: usize,
    pub paused_automations: usize,
    pub provisioning_errors: usize,
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

#[async_trait]
pub trait AutomationSignalChecker: Send + Sync {
    async fn check_pr_status(
        &self,
        workspace: &AgentConversationWorkspace,
        pr_number: i64,
    ) -> AppResult<PrStatus>;
}

pub struct GithubAutomationSignalChecker {
    github: Option<Arc<dyn GithubServiceTrait>>,
}

impl GithubAutomationSignalChecker {
    pub fn new(github: Option<Arc<dyn GithubServiceTrait>>) -> Self {
        Self { github }
    }
}

#[async_trait]
impl AutomationSignalChecker for GithubAutomationSignalChecker {
    async fn check_pr_status(
        &self,
        workspace: &AgentConversationWorkspace,
        pr_number: i64,
    ) -> AppResult<PrStatus> {
        let Some(github) = self.github.as_ref() else {
            return Err(AppError::Validation(
                "GitHub service is unavailable for automation PR signal check".to_string(),
            ));
        };
        github
            .check_pr_status(std::path::Path::new(&workspace.worktree_path), pr_number)
            .await
    }
}

pub struct AutomationScheduler {
    service: AutomationService,
    provisioner: AutomationRunProvisioner,
    run_repo: Arc<dyn AutomationRunRepository>,
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    transition_service: AutomationTransitionService,
    signal_checker: Arc<dyn AutomationSignalChecker>,
    registry: Arc<AutomationSchedulerRegistry>,
    config: AutomationSchedulerConfig,
}

impl AutomationScheduler {
    pub fn new(
        automation_repo: Arc<dyn AutomationRepository>,
        run_repo: Arc<dyn AutomationRunRepository>,
        conversation_repo: Arc<dyn ChatConversationRepository>,
        workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
        starter: Arc<dyn AutomationRunStarter>,
        signal_checker: Arc<dyn AutomationSignalChecker>,
        registry: Arc<AutomationSchedulerRegistry>,
        config: AutomationSchedulerConfig,
    ) -> Self {
        let event_emitter = Arc::new(NoopAutomationEventEmitter);
        let service = AutomationService::new(
            Arc::clone(&automation_repo),
            Arc::clone(&run_repo),
            event_emitter.clone(),
        );
        let transition_service = AutomationTransitionService::new(
            Arc::clone(&automation_repo),
            Arc::clone(&run_repo),
            event_emitter.clone(),
        );
        let provisioner = AutomationRunProvisioner::new(
            automation_repo,
            Arc::clone(&run_repo),
            conversation_repo,
            Arc::clone(&workspace_repo),
            starter,
            event_emitter,
        );
        Self {
            service,
            provisioner,
            run_repo,
            workspace_repo,
            transition_service,
            signal_checker,
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
                    match self
                        .provisioner
                        .provision_first_run(&detail.automation)
                        .await
                    {
                        Ok(Some(_run)) => {
                            summary.provisioned_runs += 1;
                        }
                        Ok(None) => {}
                        Err(error) => {
                            summary.provisioning_errors += 1;
                            tracing::warn!(
                                automation_id = %automation.id,
                                error = %error,
                                "Automation scheduler failed to provision first run"
                            );
                        }
                    }
                }
                Ok(detail) => {
                    summary.active_with_runs += 1;
                    if let Some(latest_run) = detail.runs.last() {
                        if let Err(error) = self
                            .observe_latest_run(&detail.automation, latest_run, &mut summary)
                            .await
                        {
                            summary.automation_errors += 1;
                            tracing::warn!(
                                automation_id = %detail.automation.id,
                                run_id = %latest_run.id,
                                error = %error,
                                "Automation scheduler failed to observe latest run"
                            );
                        }
                    }
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

    async fn observe_latest_run(
        &self,
        automation: &Automation,
        run: &AutomationRun,
        summary: &mut AutomationSchedulerTickSummary,
    ) -> AppResult<()> {
        match run.status {
            AutomationRunStatus::Running => {
                self.observe_running_run(run, summary).await?;
            }
            AutomationRunStatus::Published => {
                self.observe_published_run(automation, run, summary).await?;
            }
            AutomationRunStatus::Provisioning
                if run_has_exceeded(run, self.config.max_run_duration) =>
            {
                if self
                    .transition_service
                    .transition_run_status(
                        &run.id,
                        AutomationRunStatus::Provisioning,
                        AutomationRunStatus::AgentFailed,
                        Some("provisioning_timeout".to_string()),
                        Some(
                            "Automation run stayed provisioning beyond max_run_duration_secs"
                                .to_string(),
                        ),
                    )
                    .await?
                {
                    summary.failed_runs += 1;
                }
            }
            _ => {}
        }
        Ok(())
    }

    async fn observe_running_run(
        &self,
        run: &AutomationRun,
        summary: &mut AutomationSchedulerTickSummary,
    ) -> AppResult<()> {
        if run_has_exceeded(run, self.config.max_run_duration) {
            if self
                .transition_service
                .transition_run_status(
                    &run.id,
                    AutomationRunStatus::Running,
                    AutomationRunStatus::AgentFailed,
                    Some("timeout".to_string()),
                    Some("Automation run exceeded max_run_duration_secs".to_string()),
                )
                .await?
            {
                summary.failed_runs += 1;
            }
            return Ok(());
        }

        let Some(conversation_id) = run.conversation_id.as_ref() else {
            return Ok(());
        };
        let Some(workspace) = self
            .workspace_repo
            .get_by_conversation_id(conversation_id)
            .await?
        else {
            return Ok(());
        };

        if workspace.publication_pr_number.is_some() {
            let metadata = publication_metadata_from_workspace(&workspace);
            self.run_repo
                .update_publication_metadata(&run.id, metadata)
                .await?;
            if self
                .transition_service
                .transition_run_status(
                    &run.id,
                    AutomationRunStatus::Running,
                    AutomationRunStatus::Published,
                    None,
                    None,
                )
                .await?
            {
                summary.published_runs += 1;
            }
            return Ok(());
        }

        match workspace.publication_push_status.as_deref() {
            Some("no_changes") => {
                self.fail_running_run(
                    run,
                    "no_changes",
                    "Auto-publish found no committed changes to publish",
                    summary,
                )
                .await?;
            }
            Some("failed" | "description_failed") => {
                self.fail_running_run(
                    run,
                    "publish_failed",
                    "Auto-publish failed before opening a pull request",
                    summary,
                )
                .await?;
            }
            Some("needs_agent")
                if elapsed_since(workspace.updated_at)
                    .is_some_and(|elapsed| elapsed >= self.config.publish_grace) =>
            {
                self.fail_running_run(
                    run,
                    "publish_failed",
                    "Auto-publish repair did not recover before the scheduler grace period",
                    summary,
                )
                .await?;
            }
            _ => {}
        }

        Ok(())
    }

    async fn fail_running_run(
        &self,
        run: &AutomationRun,
        code: &str,
        detail: &str,
        summary: &mut AutomationSchedulerTickSummary,
    ) -> AppResult<()> {
        if self
            .transition_service
            .transition_run_status(
                &run.id,
                AutomationRunStatus::Running,
                AutomationRunStatus::AgentFailed,
                Some(code.to_string()),
                Some(detail.to_string()),
            )
            .await?
        {
            summary.failed_runs += 1;
        }
        Ok(())
    }

    async fn observe_published_run(
        &self,
        automation: &Automation,
        run: &AutomationRun,
        summary: &mut AutomationSchedulerTickSummary,
    ) -> AppResult<()> {
        let Some(conversation_id) = run.conversation_id.as_ref() else {
            return Ok(());
        };
        let Some(workspace) = self
            .workspace_repo
            .get_by_conversation_id(conversation_id)
            .await?
        else {
            return Ok(());
        };
        let Some(pr_number) = run.pr_number.or(workspace.publication_pr_number) else {
            return Ok(());
        };

        if run.pr_number.is_none() {
            self.run_repo
                .update_publication_metadata(
                    &run.id,
                    publication_metadata_from_workspace(&workspace),
                )
                .await?;
        }

        match self
            .signal_checker
            .check_pr_status(&workspace, pr_number)
            .await
        {
            Ok(PrStatus::Open) => {
                self.run_repo.reset_signal_check_failures(&run.id).await?;
            }
            Ok(PrStatus::Merged {
                merge_commit_sha,
                merged_at,
            }) => {
                let pr_merged_at = parse_github_datetime(merged_at.as_deref());
                self.run_repo
                    .update_merge_metadata(&run.id, merge_commit_sha, pr_merged_at)
                    .await?;
                self.workspace_repo
                    .update_publication(
                        conversation_id,
                        Some(pr_number),
                        workspace.publication_pr_url.as_deref(),
                        Some("merged"),
                        workspace.publication_push_status.as_deref(),
                    )
                    .await?;
                if self
                    .transition_service
                    .transition_run_status(
                        &run.id,
                        AutomationRunStatus::Published,
                        AutomationRunStatus::Merged,
                        None,
                        None,
                    )
                    .await?
                {
                    summary.merged_runs += 1;
                }
            }
            Ok(PrStatus::Closed) => {
                self.workspace_repo
                    .update_publication(
                        conversation_id,
                        Some(pr_number),
                        workspace.publication_pr_url.as_deref(),
                        Some("closed"),
                        workspace.publication_push_status.as_deref(),
                    )
                    .await?;
                if self
                    .transition_service
                    .transition_run_status(
                        &run.id,
                        AutomationRunStatus::Published,
                        AutomationRunStatus::PrClosed,
                        Some("pr_closed".to_string()),
                        Some("Pull request was closed without merging".to_string()),
                    )
                    .await?
                {
                    summary.closed_runs += 1;
                }
            }
            Err(error) => {
                summary.signal_check_errors += 1;
                let updated = self
                    .run_repo
                    .increment_signal_check_failures(&run.id)
                    .await?;
                let failures = updated
                    .as_ref()
                    .map_or(run.signal_check_failures + 1, |run| {
                        run.signal_check_failures
                    });
                if failures as u64 >= self.config.signal_failure_pause_threshold
                    && self
                        .transition_service
                        .transition_automation_status(
                            &automation.id,
                            AutomationStatus::Active,
                            AutomationStatus::Paused,
                            Some("signal_verification_failed".to_string()),
                            Some(format!(
                                "Scheduler could not verify PR #{pr_number} after {failures} attempts: {error}"
                            )),
                        )
                        .await?
                {
                    summary.paused_automations += 1;
                }
            }
        }
        Ok(())
    }
}

fn publication_metadata_from_workspace(
    workspace: &AgentConversationWorkspace,
) -> AutomationRunPublicationMetadata {
    AutomationRunPublicationMetadata {
        pr_number: workspace.publication_pr_number,
        pr_url: workspace.publication_pr_url.clone(),
        pr_title: None,
        pr_head_ref_name: Some(workspace.branch_name.clone()),
        pr_base_ref_name: Some(workspace.base_ref.clone()),
    }
}

fn parse_github_datetime(value: Option<&str>) -> Option<DateTime<Utc>> {
    value.and_then(|value| {
        DateTime::parse_from_rfc3339(value)
            .ok()
            .map(|datetime| datetime.with_timezone(&Utc))
    })
}

fn run_has_exceeded(run: &AutomationRun, limit: Duration) -> bool {
    let started_at = run.started_at.unwrap_or(run.created_at);
    elapsed_since(started_at).is_some_and(|elapsed| elapsed >= limit)
}

fn elapsed_since(started_at: DateTime<Utc>) -> Option<Duration> {
    Utc::now().signed_duration_since(started_at).to_std().ok()
}
