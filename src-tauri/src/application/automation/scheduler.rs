use std::path::Path;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use dashmap::mapref::entry::Entry;
use dashmap::DashMap;

use crate::application::automation::judge::{
    append_automation_judge_retry_instruction, build_automation_judge_prompt,
    parse_automation_judge_verdict, AutomationJudgeAttachmentContext,
    AutomationJudgeContextRefSummary, AutomationJudgeValidationContext,
    BuildAutomationJudgePromptInput,
};
use crate::application::automation::provisioning::{
    AutomationRunProvisioner, AutomationRunStarter,
};
use crate::application::automation::service::{
    ApplyAutomationJudgeVerdictInput, AutomationJudgeApplyOutcome, AutomationService,
    CompleteAutomationJudgeInput,
};
use crate::application::automation::transition::{
    AutomationEventEmitter, AutomationTransitionService,
};
use crate::application::harness_runtime_registry::{
    default_automation_judge_timeout_secs, default_automation_max_run_duration_secs,
    default_automation_publish_grace_secs, default_automation_scheduler_poll_secs,
    default_automation_signal_failure_pause_threshold, resolve_harness_agent_bootstrap,
};
use crate::application::AppState;
use crate::domain::agents::{AgentConfig, AgentHarnessKind, AgentRole, DEFAULT_AGENT_HARNESS};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentRunStatus, Automation, AutomationId, AutomationJudgeState,
    AutomationRun, AutomationRunStatus, AutomationStatus,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, AgentRunRepository, AutomationRepository,
    AutomationRunPublicationMetadata, AutomationRunRepository, ChatConversationRepository,
};
use crate::domain::services::github_service::{GithubServiceTrait, PrStatus};
use crate::error::{AppError, AppResult};
use crate::infrastructure::agents::claude::agent_names;
use crate::infrastructure::agents::claude::AutomationsRuntimeConfig;
use crate::utils::path_safety::validate_absolute_non_root_path;

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
    pub completed_runs: usize,
    pub merged_runs: usize,
    pub closed_runs: usize,
    pub failed_runs: usize,
    pub judges_started: usize,
    pub judges_succeeded: usize,
    pub judge_failures: usize,
    pub successor_runs: usize,
    pub signal_check_errors: usize,
    pub paused_automations: usize,
    pub completed_automations: usize,
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

#[derive(Debug, Clone)]
pub struct AutomationJudgeInvocation {
    pub automation: Automation,
    pub runs: Vec<AutomationRun>,
    pub previous_run: AutomationRun,
    pub retry_reminder: bool,
    pub timeout: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationJudgeInvocationOutput {
    pub raw_output: String,
    pub model_id: Option<String>,
}

#[async_trait]
pub trait AutomationJudgeInvoker: Send + Sync {
    async fn invoke(
        &self,
        input: AutomationJudgeInvocation,
    ) -> AppResult<AutomationJudgeInvocationOutput>;
}

#[derive(Clone)]
pub struct HarnessAutomationJudgeInvoker {
    state: AppState,
}

impl HarnessAutomationJudgeInvoker {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl AutomationJudgeInvoker for HarnessAutomationJudgeInvoker {
    async fn invoke(
        &self,
        input: AutomationJudgeInvocation,
    ) -> AppResult<AutomationJudgeInvocationOutput> {
        let mut prompt = build_automation_judge_prompt(BuildAutomationJudgePromptInput {
            automation: &input.automation,
            runs: &input.runs,
            previous_run: &input.previous_run,
            attachments: &[] as &[AutomationJudgeAttachmentContext],
            context_refs: &[] as &[AutomationJudgeContextRefSummary],
        })?;
        if input.retry_reminder && !append_automation_judge_retry_instruction(&mut prompt) {
            tracing::warn!(
                automation_id = %input.automation.id,
                run_id = %input.previous_run.id,
                "Automation judge retry instruction omitted because the prompt is at the argv-safe budget"
            );
        }

        let harness = AgentHarnessKind::from_str(input.automation.provider_harness.trim())
            .map_err(AppError::Validation)?;
        let runtime = AppState::lock_utility_agent_runtime_model(
            self.state
                .resolve_background_agent_runtime_for_harness(harness, "automation judge")
                .await?,
        );
        let helper_harness = runtime.harness.unwrap_or(DEFAULT_AGENT_HARNESS);
        let project = self
            .state
            .project_repo
            .get_by_id(&input.automation.project_id)
            .await?
            .ok_or_else(|| {
                AppError::NotFound(format!(
                    "automation judge project {} not found",
                    input.automation.project_id.as_str()
                ))
            })?;
        let project_working_directory = validate_absolute_non_root_path(
            Path::new(&project.working_directory),
            "automation judge project checkout",
        )?;
        let bootstrap = resolve_harness_agent_bootstrap(
            helper_harness,
            agent_names::AGENT_AUTOMATION_JUDGE,
            project_working_directory,
        );
        let env = runtime.env_with_overrides(bootstrap.env);
        let client = Arc::clone(&runtime.client);
        let model_id = runtime.model.clone();
        let handle = client
            .spawn_agent(AgentConfig {
                role: AgentRole::Custom(bootstrap.agent_role.clone()),
                prompt,
                working_directory: bootstrap.working_directory,
                plugin_dir: Some(bootstrap.plugin_dir),
                agent: Some(bootstrap.agent_name),
                model: runtime.model,
                harness: runtime.harness,
                cli_path_override: runtime.cli_path_override,
                logical_effort: runtime.logical_effort,
                approval_policy: runtime.approval_policy,
                sandbox_mode: runtime.sandbox_mode,
                service_tier: runtime.service_tier,
                max_tokens: None,
                timeout_secs: Some(input.timeout.as_secs().max(1)),
                env,
            })
            .await
            .map_err(|error| {
                AppError::Infrastructure(format!("failed to spawn automation judge: {error}"))
            })?;
        let output = client.wait_for_completion(&handle).await.map_err(|error| {
            AppError::Infrastructure(format!("automation judge failed: {error}"))
        })?;
        if !output.success {
            return Err(AppError::Infrastructure(format!(
                "automation judge exited unsuccessfully: {}",
                output.content.trim()
            )));
        }
        Ok(AutomationJudgeInvocationOutput {
            raw_output: output.content,
            model_id,
        })
    }
}

#[derive(Clone)]
struct AutomationJudgeTask {
    service: AutomationService,
    transition_service: AutomationTransitionService,
    judge_invoker: Arc<dyn AutomationJudgeInvoker>,
    config: AutomationSchedulerConfig,
}

#[derive(Debug, Default)]
struct AutomationJudgeTaskOutcome {
    judge_succeeded: bool,
    judge_failed: bool,
    successor_created: bool,
    terminal_automation_status: Option<AutomationStatus>,
}

impl AutomationJudgeTaskOutcome {
    fn from_apply_outcome(outcome: AutomationJudgeApplyOutcome) -> Self {
        Self {
            judge_succeeded: true,
            judge_failed: false,
            successor_created: outcome.successor_run.is_some(),
            terminal_automation_status: outcome.terminal_automation_status,
        }
    }
}

impl AutomationJudgeTask {
    async fn run_for_terminal_run(
        &self,
        automation: Automation,
        runs: Vec<AutomationRun>,
        run: AutomationRun,
    ) -> AppResult<AutomationJudgeTaskOutcome> {
        let parsed = match self
            .invoke_and_parse_judge(&automation, &runs, &run, false)
            .await
        {
            Ok(parsed) => parsed,
            Err(JudgeInvocationFailure::InvalidOutput { .. }) => match self
                .invoke_and_parse_judge(&automation, &runs, &run, true)
                .await
            {
                Ok(parsed) => parsed,
                Err(error) => {
                    self.mark_judge_failed(&automation, &run, error.detail())
                        .await?;
                    return Ok(AutomationJudgeTaskOutcome {
                        judge_failed: true,
                        ..AutomationJudgeTaskOutcome::default()
                    });
                }
            },
            Err(error) => {
                self.mark_judge_failed(&automation, &run, error.detail())
                    .await?;
                return Ok(AutomationJudgeTaskOutcome {
                    judge_failed: true,
                    ..AutomationJudgeTaskOutcome::default()
                });
            }
        };

        let outcome = self
            .service
            .complete_judge_verdict(CompleteAutomationJudgeInput {
                automation,
                previous_run: run,
                verdict: parsed.verdict,
                verdict_json: parsed.verdict_json,
                judge_model_id: parsed.model_id,
            })
            .await?;
        Ok(AutomationJudgeTaskOutcome::from_apply_outcome(outcome))
    }

    async fn invoke_and_parse_judge(
        &self,
        automation: &Automation,
        runs: &[AutomationRun],
        run: &AutomationRun,
        retry_reminder: bool,
    ) -> Result<ParsedJudgeInvocation, JudgeInvocationFailure> {
        let output = self
            .judge_invoker
            .invoke(AutomationJudgeInvocation {
                automation: automation.clone(),
                runs: runs.to_vec(),
                previous_run: run.clone(),
                retry_reminder,
                timeout: self.config.judge_timeout,
            })
            .await
            .map_err(|error| JudgeInvocationFailure::Invocation {
                detail: error.to_string(),
            })?;
        let verdict = parse_automation_judge_verdict(
            &output.raw_output,
            AutomationJudgeValidationContext {
                automation,
                previous_run: run,
            },
        )
        .map_err(|error| JudgeInvocationFailure::InvalidOutput {
            detail: error.to_string(),
            raw_output: output.raw_output.clone(),
        })?;
        let verdict_json = serde_json::to_string(&verdict).map_err(|error| {
            JudgeInvocationFailure::InvalidOutput {
                detail: format!("failed to serialize normalized judge verdict: {error}"),
                raw_output: output.raw_output,
            }
        })?;
        Ok(ParsedJudgeInvocation {
            verdict,
            verdict_json,
            model_id: output.model_id,
        })
    }

    async fn mark_judge_failed(
        &self,
        automation: &Automation,
        run: &AutomationRun,
        detail: String,
    ) -> AppResult<bool> {
        if !self
            .transition_service
            .transition_judge_state(
                &run.id,
                AutomationJudgeState::InProgress,
                AutomationJudgeState::Failed,
                None,
                None,
                None,
                Some(detail.clone()),
            )
            .await?
        {
            return Ok(false);
        }
        self.transition_service
            .transition_automation_status(
                &automation.id,
                AutomationStatus::Active,
                AutomationStatus::Paused,
                Some("judge_failed".to_string()),
                Some(detail),
            )
            .await
    }
}

pub struct AutomationScheduler {
    service: AutomationService,
    provisioner: AutomationRunProvisioner,
    agent_run_repo: Arc<dyn AgentRunRepository>,
    run_repo: Arc<dyn AutomationRunRepository>,
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    transition_service: AutomationTransitionService,
    signal_checker: Arc<dyn AutomationSignalChecker>,
    judge_invoker: Arc<dyn AutomationJudgeInvoker>,
    registry: Arc<AutomationSchedulerRegistry>,
    config: AutomationSchedulerConfig,
}

impl AutomationScheduler {
    pub fn new(
        automation_repo: Arc<dyn AutomationRepository>,
        run_repo: Arc<dyn AutomationRunRepository>,
        agent_run_repo: Arc<dyn AgentRunRepository>,
        conversation_repo: Arc<dyn ChatConversationRepository>,
        workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
        starter: Arc<dyn AutomationRunStarter>,
        signal_checker: Arc<dyn AutomationSignalChecker>,
        judge_invoker: Arc<dyn AutomationJudgeInvoker>,
        event_emitter: Arc<dyn AutomationEventEmitter>,
        registry: Arc<AutomationSchedulerRegistry>,
        config: AutomationSchedulerConfig,
    ) -> Self {
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
            agent_run_repo,
            run_repo,
            workspace_repo,
            transition_service,
            signal_checker,
            judge_invoker,
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
                            .observe_latest_run(
                                &detail.automation,
                                &detail.runs,
                                latest_run,
                                &mut summary,
                            )
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
        runs: &[AutomationRun],
        run: &AutomationRun,
        summary: &mut AutomationSchedulerTickSummary,
    ) -> AppResult<()> {
        match run.status {
            AutomationRunStatus::Pending => {
                if self
                    .provisioner
                    .provision_pending_run(automation, run)
                    .await?
                    .is_some()
                {
                    summary.provisioned_runs += 1;
                }
            }
            AutomationRunStatus::Running => {
                self.observe_running_run(automation, run, summary).await?;
            }
            AutomationRunStatus::Published => {
                self.observe_published_run(automation, run, summary).await?;
            }
            AutomationRunStatus::Merged
            | AutomationRunStatus::Completed
            | AutomationRunStatus::PrClosed
            | AutomationRunStatus::AgentFailed => {
                self.observe_signal_terminal_run(automation, runs, run, summary)
                    .await?;
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
        automation: &Automation,
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
        if automation.completion_signal == "agent_completed" {
            match self
                .agent_run_repo
                .get_latest_for_conversation(conversation_id)
                .await?
                .map(|run| run.status)
            {
                Some(AgentRunStatus::Completed) => {
                    if self
                        .transition_service
                        .transition_run_status(
                            &run.id,
                            AutomationRunStatus::Running,
                            AutomationRunStatus::Completed,
                            None,
                            None,
                        )
                        .await?
                    {
                        summary.completed_runs += 1;
                    }
                    return Ok(());
                }
                Some(AgentRunStatus::Failed) => {
                    self.fail_running_run(
                        run,
                        "agent_failed",
                        "Automation run agent failed",
                        summary,
                    )
                    .await?;
                    return Ok(());
                }
                Some(AgentRunStatus::Cancelled) => {
                    if self
                        .transition_service
                        .transition_run_status(
                            &run.id,
                            AutomationRunStatus::Running,
                            AutomationRunStatus::Cancelled,
                            Some("agent_cancelled".to_string()),
                            Some("Automation run agent was cancelled".to_string()),
                        )
                        .await?
                    {
                        summary.failed_runs += 1;
                    }
                    return Ok(());
                }
                Some(AgentRunStatus::Running) | None => return Ok(()),
            }
        }
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

    async fn observe_signal_terminal_run(
        &self,
        automation: &Automation,
        runs: &[AutomationRun],
        run: &AutomationRun,
        summary: &mut AutomationSchedulerTickSummary,
    ) -> AppResult<()> {
        match run.judge_state {
            AutomationJudgeState::None | AutomationJudgeState::Failed => {
                let from = run.judge_state;
                if self
                    .transition_service
                    .transition_judge_state(
                        &run.id,
                        from,
                        AutomationJudgeState::InProgress,
                        None,
                        None,
                        Some(automation_judge_lease_expires_at(self.config.judge_timeout)),
                        None,
                    )
                    .await?
                {
                    summary.judges_started += 1;
                    spawn_automation_judge_task(
                        self.service.clone(),
                        self.transition_service.clone(),
                        Arc::clone(&self.judge_invoker),
                        self.config.clone(),
                        automation.clone(),
                        runs.to_vec(),
                        run.clone(),
                    );
                }
            }
            AutomationJudgeState::Done => {
                self.apply_stored_judge_verdict(automation, run, summary)
                    .await?;
            }
            AutomationJudgeState::InProgress
                if judge_has_exceeded(run, self.config.judge_timeout) =>
            {
                self.mark_judge_failed(
                    automation,
                    run,
                    "Automation judge exceeded judge_timeout_secs".to_string(),
                    summary,
                )
                .await?;
            }
            AutomationJudgeState::InProgress | AutomationJudgeState::Skipped => {}
        }
        Ok(())
    }

    async fn mark_judge_failed(
        &self,
        automation: &Automation,
        run: &AutomationRun,
        detail: String,
        summary: &mut AutomationSchedulerTickSummary,
    ) -> AppResult<()> {
        if self
            .transition_service
            .transition_judge_state(
                &run.id,
                AutomationJudgeState::InProgress,
                AutomationJudgeState::Failed,
                None,
                None,
                None,
                Some(detail.clone()),
            )
            .await?
        {
            summary.judge_failures += 1;
            if self
                .transition_service
                .transition_automation_status(
                    &automation.id,
                    AutomationStatus::Active,
                    AutomationStatus::Paused,
                    Some("judge_failed".to_string()),
                    Some(detail),
                )
                .await?
            {
                summary.paused_automations += 1;
            }
        }
        Ok(())
    }

    async fn apply_stored_judge_verdict(
        &self,
        automation: &Automation,
        run: &AutomationRun,
        summary: &mut AutomationSchedulerTickSummary,
    ) -> AppResult<()> {
        let Some(verdict_json) = run.judge_verdict_json.as_deref() else {
            return Ok(());
        };
        let verdict = parse_automation_judge_verdict(
            verdict_json,
            AutomationJudgeValidationContext {
                automation,
                previous_run: run,
            },
        )?;
        let outcome = self
            .service
            .apply_persisted_judge_verdict(ApplyAutomationJudgeVerdictInput {
                automation: automation.clone(),
                previous_run: run.clone(),
                verdict,
            })
            .await?;
        self.record_judge_apply_outcome(outcome, summary);
        Ok(())
    }

    fn record_judge_apply_outcome(
        &self,
        outcome: crate::application::automation::service::AutomationJudgeApplyOutcome,
        summary: &mut AutomationSchedulerTickSummary,
    ) {
        if outcome.successor_run.is_some() {
            summary.successor_runs += 1;
        }
        match outcome.terminal_automation_status {
            Some(AutomationStatus::Paused) => summary.paused_automations += 1,
            Some(AutomationStatus::Completed) => summary.completed_automations += 1,
            _ => {}
        }
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

pub(crate) fn spawn_automation_judge_task(
    service: AutomationService,
    transition_service: AutomationTransitionService,
    judge_invoker: Arc<dyn AutomationJudgeInvoker>,
    config: AutomationSchedulerConfig,
    automation: Automation,
    runs: Vec<AutomationRun>,
    run: AutomationRun,
) {
    let task = AutomationJudgeTask {
        service,
        transition_service,
        judge_invoker,
        config,
    };
    tauri::async_runtime::spawn(async move {
        let automation_id = automation.id.clone();
        let run_id = run.id.clone();
        match task.run_for_terminal_run(automation, runs, run).await {
            Ok(outcome) => {
                tracing::info!(
                    automation_id = %automation_id,
                    run_id = %run_id,
                    judge_succeeded = outcome.judge_succeeded,
                    judge_failed = outcome.judge_failed,
                    successor_created = outcome.successor_created,
                    terminal_status = ?outcome.terminal_automation_status,
                    "Automation judge task completed"
                );
            }
            Err(error) => {
                tracing::warn!(
                    automation_id = %automation_id,
                    run_id = %run_id,
                    error = %error,
                    "Automation judge task failed"
                );
            }
        }
    });
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

struct ParsedJudgeInvocation {
    verdict: crate::application::automation::judge::AutomationJudgeVerdict,
    verdict_json: String,
    model_id: Option<String>,
}

enum JudgeInvocationFailure {
    Invocation { detail: String },
    InvalidOutput { detail: String, raw_output: String },
}

impl JudgeInvocationFailure {
    fn detail(self) -> String {
        match self {
            Self::Invocation { detail } => detail,
            Self::InvalidOutput { detail, raw_output } => {
                let raw_excerpt = raw_output.chars().take(1000).collect::<String>();
                format!("{detail}; raw_output: {raw_excerpt}")
            }
        }
    }
}

fn run_has_exceeded(run: &AutomationRun, limit: Duration) -> bool {
    let started_at = run.started_at.unwrap_or(run.created_at);
    elapsed_since(started_at).is_some_and(|elapsed| elapsed >= limit)
}

fn judge_has_exceeded(run: &AutomationRun, limit: Duration) -> bool {
    if let Some(expires_at) = run.judge_lease_expires_at {
        return Utc::now() >= expires_at;
    }
    elapsed_since(run.updated_at).is_some_and(|elapsed| elapsed >= limit)
}

pub(crate) fn automation_judge_lease_expires_at(limit: Duration) -> DateTime<Utc> {
    let Ok(limit) = chrono::Duration::from_std(limit) else {
        return Utc::now();
    };
    Utc::now() + limit
}

fn elapsed_since(started_at: DateTime<Utc>) -> Option<Duration> {
    Utc::now().signed_duration_since(started_at).to_std().ok()
}
