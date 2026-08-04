use std::collections::HashMap;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use ralphx_domain::entities::automation::latest_run_holds_goal_authority;
use ralphx_domain::repositories::automation_run_repository::AutomationJudgeTransitionGuard;

use crate::application::automation::integration_pr::AutomationIntegrationPrPublisher;
use crate::application::automation::judge::{
    append_automation_judge_retry_instruction, build_automation_judge_prompt,
    mark_current_goal_item_in_progress, parse_automation_judge_verdict,
    revert_in_progress_goal_items_to_pending, truncate_utf8_to_bytes,
    AutomationJudgeAttachmentContext, AutomationJudgeContextRefSummary,
    AutomationJudgeValidationContext, BuildAutomationJudgePromptInput, SPEC_ATTACHMENT_MAX_BYTES,
};
use crate::application::automation::merged_run_finalizer::AutomationMergedRunFinalizer;
use crate::application::automation::plan_gate::{
    approval_delivery_prompt, clear_plan_phase_publication_metadata,
    current_plan_artifact_ids_for_workspace, ideation_bridge_delivery_prompt,
    is_plan_gate_pause_reason, matching_plan_approval_for_workspace, refresh_plan_park_baseline,
    revision_delivery_prompt, AutomationPlanVerificationStartOutcome,
    AutomationPlanVerificationStartRequest, AutomationPlanVerificationStarter,
    AutomationRunResumer, ResumeDelivery, AUTOMATION_PLAN_REMINDER_PROMPT,
    PLAN_JUDGE_FAILED_PAUSED_REASON_CODE, PLAN_RESUME_FAILED_ERROR_CODE,
    PLAN_REVISION_EXHAUSTED_PAUSED_REASON_CODE,
};
use crate::application::automation::plan_judge::{
    append_automation_plan_judge_retry_instruction, build_automation_plan_judge_prompt,
    parse_automation_plan_judge_verdict, plan_blueprint_truncation_policy,
    AutomationPlanJudgeDecision, AutomationPlanJudgeValidationContext, AutomationPlanJudgeVerdict,
    AutomationPlanVerificationGapSummary, AutomationPlanVerificationJudgeContext,
    BuildAutomationPlanJudgePromptInput,
};
use crate::application::automation::provisioning::{
    AutomationRunProvisioner, AutomationRunStarter,
};
use crate::application::automation::service::{
    AutomationJudgeApplyOutcome, AutomationService, CompleteAutomationJudgeInput,
    PendingGoalReplanApplyOutcome, IDEATION_BRIDGE_RUN_MODE, IDEATION_FINALIZED_COMPLETION_SIGNAL,
};
use crate::application::automation::transition::{
    AutomationEventEmitter, AutomationTransitionService,
};
use crate::application::automation::utility_agent::{
    invoke_automation_utility_agent, AutomationUtilityModelPolicy,
};
use crate::application::harness_runtime_registry::{
    default_automation_judge_timeout_secs, default_automation_max_run_duration_secs,
    default_automation_plan_judge_models, default_automation_plan_max_revision_rounds,
    default_automation_publish_grace_secs, default_automation_scheduler_poll_secs,
    default_automation_signal_failure_pause_threshold,
};
use crate::application::plan_artifact_approval::PlanArtifactApprovalWriter;
use crate::application::plan_verification_service::PlanVerificationStatusKind;
use crate::application::services::pr_auto_merge_status::{
    AUTO_MERGE_ENABLE_FAILURE_SUMMARY_PREFIX, AUTO_MERGE_ENABLE_WARNING_CODE,
    AUTO_MERGE_SUPERVISION_STATUS_WAITING,
};
use crate::application::AppState;
use crate::application::NotificationService;
use crate::domain::agents::{plan_judge_model_for_provider, AgentHarnessKind};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentRun, AgentRunStatus,
    ArtifactContent, ArtifactId, Automation, AutomationId, AutomationJudgeState,
    AutomationPlanApprovalMode, AutomationPlanJudgeState, AutomationPrMergeMode, AutomationRun,
    AutomationRunStatus, AutomationStatus, ChatContextType, ChatConversationId, IdeationSession,
    IdeationSessionStatus, VerificationRunSnapshot, VerificationStatus,
    DEFAULT_AGENT_WORKSPACE_PR_AUTO_MERGE_METHOD,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, AgentRunRepository, ArtifactRepository,
    AutomationRepository, AutomationRunPublicationMetadata, AutomationRunRepository,
    ChatConversationRepository, IdeationSessionRepository, PlanApprovalActor, PlanArtifactApproval,
    PlanArtifactApprovalRepository,
};
use crate::domain::services::github_service::{GithubServiceTrait, PrStatus};
use crate::domain::services::{
    gap_score, load_current_verification_snapshot_or_default, load_effective_verification_status,
};
use crate::error::{AppError, AppResult};
use crate::infrastructure::agents::claude::agent_names;
use crate::infrastructure::agents::claude::AutomationsRuntimeConfig;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationSchedulerConfig {
    pub poll_interval: Duration,
    pub signal_failure_pause_threshold: u64,
    pub judge_timeout: Duration,
    pub publish_grace: Duration,
    pub max_run_duration: Duration,
    pub plan_judge_models: HashMap<AgentHarnessKind, String>,
    pub plan_max_revision_rounds: i64,
    pub plan_verification_hold_timeout: Duration,
}

impl AutomationSchedulerConfig {
    pub fn from_runtime(config: &AutomationsRuntimeConfig) -> Self {
        Self {
            poll_interval: Duration::from_secs(config.scheduler_poll_secs.max(1)),
            signal_failure_pause_threshold: config.signal_failure_pause_threshold.max(1),
            judge_timeout: Duration::from_secs(config.judge_timeout_secs.max(1)),
            publish_grace: Duration::from_secs(config.publish_grace_secs),
            max_run_duration: Duration::from_secs(config.max_run_duration_secs.max(1)),
            plan_judge_models: config.plan_judge_model.clone(),
            plan_max_revision_rounds: i64::try_from(config.plan_max_revision_rounds.max(1))
                .unwrap_or(i64::MAX),
            plan_verification_hold_timeout: Duration::from_secs(5_400),
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
            plan_judge_model: default_automation_plan_judge_models(),
            plan_max_revision_rounds: default_automation_plan_max_revision_rounds(),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PlanRedeliveryTrigger {
    None,
    LatestRunMissing,
    RestartOrphan,
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
    pub resumed_automations: usize,
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

/// Loads the automation's linked spec artifact as inline judge context.
///
/// The judge is a one-shot agent with no MCP artifact tools, so the spec is
/// inlined as an [`AutomationJudgeAttachmentContext`], pre-truncated to
/// [`SPEC_ATTACHMENT_MAX_BYTES`] to keep the surrounding `original_inputs` JSON
/// wrapper intact. The spec is advisory context only — no state transition
/// depends on it — so every failure mode fails open to an empty attachment list
/// (D6): no `spec_artifact_id`, a missing artifact, a file-backed artifact, or a
/// repository error all yield `vec![]` after a warning.
pub(crate) async fn load_spec_attachment(
    artifact_repo: &Arc<dyn ArtifactRepository>,
    automation: &Automation,
) -> Vec<AutomationJudgeAttachmentContext> {
    let Some(spec_id) = automation
        .spec_artifact_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
    else {
        return Vec::new();
    };

    let artifact_id = ArtifactId::from_string(spec_id.to_string());
    let artifact = match artifact_repo.get_by_id(&artifact_id).await {
        Ok(Some(artifact)) => artifact,
        Ok(None) => {
            tracing::warn!(
                automation_id = %automation.id,
                spec_artifact_id = spec_id,
                "Automation judge spec artifact not found; continuing without spec context"
            );
            return Vec::new();
        }
        Err(error) => {
            tracing::warn!(
                automation_id = %automation.id,
                spec_artifact_id = spec_id,
                %error,
                "Failed to load automation judge spec artifact; continuing without spec context"
            );
            return Vec::new();
        }
    };

    let text = match &artifact.content {
        ArtifactContent::Inline { text } => text,
        ArtifactContent::File { .. } => {
            tracing::warn!(
                automation_id = %automation.id,
                spec_artifact_id = spec_id,
                "Automation judge spec artifact is file-backed; continuing without spec context"
            );
            return Vec::new();
        }
    };

    let (spec_text, _) = truncate_utf8_to_bytes(text, SPEC_ATTACHMENT_MAX_BYTES);
    let file_size = spec_text.len() as i64;
    vec![AutomationJudgeAttachmentContext {
        file_name: artifact.name.clone(),
        mime_type: Some("text/markdown".to_string()),
        file_size: Some(file_size),
        text_content: Some(spec_text),
    }]
}

#[async_trait]
impl AutomationJudgeInvoker for HarnessAutomationJudgeInvoker {
    async fn invoke(
        &self,
        input: AutomationJudgeInvocation,
    ) -> AppResult<AutomationJudgeInvocationOutput> {
        let attachments = load_spec_attachment(&self.state.artifact_repo, &input.automation).await;
        let mut prompt = build_automation_judge_prompt(BuildAutomationJudgePromptInput {
            automation: &input.automation,
            runs: &input.runs,
            previous_run: &input.previous_run,
            attachments: &attachments,
            context_refs: &[] as &[AutomationJudgeContextRefSummary],
        })?;
        if input.retry_reminder && !append_automation_judge_retry_instruction(&mut prompt) {
            tracing::warn!(
                automation_id = %input.automation.id,
                run_id = %input.previous_run.id,
                "Automation judge retry instruction omitted because the prompt is at the argv-safe budget"
            );
        }

        let output = invoke_automation_utility_agent(
            &self.state,
            &input.automation,
            agent_names::AGENT_AUTOMATION_JUDGE,
            "automation judge",
            prompt,
            input.timeout,
            AutomationUtilityModelPolicy::LockedDefault,
        )
        .await?;
        Ok(AutomationJudgeInvocationOutput {
            raw_output: output.raw_output,
            model_id: output.model_id,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationPlanJudgeInvocation {
    pub automation: Automation,
    pub run: AutomationRun,
    pub overview_artifact_id: String,
    pub overview_content: String,
    pub blueprint_artifact_id: Option<String>,
    pub blueprint_content: Option<String>,
    pub verification_context: Option<AutomationPlanVerificationJudgeContext>,
    pub previous_verdict_json: Option<String>,
    pub retry_reminder: bool,
    pub timeout: Duration,
    pub plan_judge_model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationPlanJudgeInvocationOutput {
    pub raw_output: String,
    pub model_id: Option<String>,
}

#[async_trait]
pub trait AutomationPlanJudgeInvoker: Send + Sync {
    async fn invoke(
        &self,
        input: AutomationPlanJudgeInvocation,
    ) -> AppResult<AutomationPlanJudgeInvocationOutput>;
}

#[derive(Clone)]
pub struct HarnessAutomationPlanJudgeInvoker {
    state: AppState,
}

impl HarnessAutomationPlanJudgeInvoker {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl AutomationPlanJudgeInvoker for HarnessAutomationPlanJudgeInvoker {
    async fn invoke(
        &self,
        input: AutomationPlanJudgeInvocation,
    ) -> AppResult<AutomationPlanJudgeInvocationOutput> {
        let attachments = load_spec_attachment(&self.state.artifact_repo, &input.automation).await;
        let mut prompt = build_automation_plan_judge_prompt(BuildAutomationPlanJudgePromptInput {
            automation: &input.automation,
            run: &input.run,
            evaluated_overview_artifact_id: &input.overview_artifact_id,
            overview_content: &input.overview_content,
            evaluated_blueprint_artifact_id: input.blueprint_artifact_id.as_deref(),
            blueprint_content: input.blueprint_content.as_deref(),
            verification_context: input.verification_context.as_ref(),
            spec_attachments: &attachments,
            previous_verdict_json: input.previous_verdict_json.as_deref(),
        })?;
        if input.retry_reminder && !append_automation_plan_judge_retry_instruction(&mut prompt) {
            tracing::warn!(
                automation_id = %input.automation.id,
                run_id = %input.run.id,
                "Automation plan judge retry instruction omitted because the prompt is at the argv-safe budget"
            );
        }

        let output = invoke_automation_utility_agent(
            &self.state,
            &input.automation,
            agent_names::AGENT_AUTOMATION_PLAN_JUDGE,
            "automation plan judge",
            prompt,
            input.timeout,
            AutomationUtilityModelPolicy::Override(input.plan_judge_model.clone()),
        )
        .await?;
        Ok(AutomationPlanJudgeInvocationOutput {
            raw_output: output.raw_output,
            model_id: output.model_id,
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
    discard_reason: Option<String>,
}

impl AutomationJudgeTaskOutcome {
    fn from_apply_outcome(outcome: AutomationJudgeApplyOutcome) -> Self {
        let AutomationJudgeApplyOutcome {
            successor_run,
            terminal_automation_status,
            noop_reason,
            reason,
        } = outcome;
        let discard_reason =
            noop_reason.map(|reason_kind| reason.unwrap_or_else(|| format!("{reason_kind:?}")));
        Self {
            judge_succeeded: true,
            judge_failed: false,
            successor_created: successor_run.is_some(),
            terminal_automation_status,
            discard_reason,
        }
    }
}

#[derive(Clone)]
struct AutomationPlanJudgeTask {
    transition_service: AutomationTransitionService,
    run_repo: Arc<dyn AutomationRunRepository>,
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    ideation_session_repo: Arc<dyn IdeationSessionRepository>,
    plan_approval_repo: Arc<dyn PlanArtifactApprovalRepository>,
    plan_approval_writer: Arc<dyn PlanArtifactApprovalWriter>,
    artifact_repo: Arc<dyn ArtifactRepository>,
    plan_judge_invoker: Arc<dyn AutomationPlanJudgeInvoker>,
    config: AutomationSchedulerConfig,
    verification_context: Option<AutomationPlanVerificationJudgeContext>,
}

#[derive(Debug, Default)]
struct AutomationPlanJudgeTaskOutcome {
    judge_succeeded: bool,
    judge_failed: bool,
    paused_automation: bool,
}

impl AutomationPlanJudgeTask {
    async fn run_for_parked_run(
        &self,
        automation: Automation,
        run: AutomationRun,
    ) -> AppResult<AutomationPlanJudgeTaskOutcome> {
        let payload = match self.load_plan_payload(&run).await {
            Ok(payload) => payload,
            Err(detail) => {
                self.mark_plan_judge_failed(&automation, &run, detail)
                    .await?;
                return Ok(AutomationPlanJudgeTaskOutcome {
                    judge_failed: true,
                    paused_automation: true,
                    ..AutomationPlanJudgeTaskOutcome::default()
                });
            }
        };
        let parsed = match self
            .invoke_and_parse_plan_judge(&automation, &run, &payload, false)
            .await
        {
            Ok(parsed) => parsed,
            Err(JudgeInvocationFailure::InvalidOutput { .. }) => match self
                .invoke_and_parse_plan_judge(&automation, &run, &payload, true)
                .await
            {
                Ok(parsed) => parsed,
                Err(error) => {
                    self.mark_plan_judge_failed(&automation, &run, error.detail())
                        .await?;
                    return Ok(AutomationPlanJudgeTaskOutcome {
                        judge_failed: true,
                        paused_automation: true,
                        ..AutomationPlanJudgeTaskOutcome::default()
                    });
                }
            },
            Err(error) => {
                self.mark_plan_judge_failed(&automation, &run, error.detail())
                    .await?;
                return Ok(AutomationPlanJudgeTaskOutcome {
                    judge_failed: true,
                    paused_automation: true,
                    ..AutomationPlanJudgeTaskOutcome::default()
                });
            }
        };

        let apply = self
            .apply_fresh_plan_judge_verdict(&automation, &run, parsed)
            .await?;
        Ok(apply)
    }

    async fn load_plan_payload(
        &self,
        run: &AutomationRun,
    ) -> Result<AutomationPlanJudgePayload, String> {
        let conversation_id = run
            .conversation_id
            .as_ref()
            .ok_or_else(|| "automation plan judge run has no conversation id".to_string())?;
        let workspace = self
            .workspace_repo
            .get_by_conversation_id(conversation_id)
            .await
            .map_err(|error| format!("failed to read plan workspace: {error}"))?
            .ok_or_else(|| "automation plan judge workspace not found".to_string())?;
        let session_id = workspace
            .linked_ideation_session_id
            .clone()
            .ok_or_else(|| "automation plan judge workspace has no planning session".to_string())?;
        let session = self
            .ideation_session_repo
            .get_by_id(&session_id)
            .await
            .map_err(|error| format!("failed to read planning session: {error}"))?
            .ok_or_else(|| "automation plan judge planning session not found".to_string())?;
        let bundle = session.plan_artifact_bundle().ok_or_else(|| {
            "automation plan judge planning session has no complete plan bundle".to_string()
        })?;
        let plan_artifact_id = bundle.overview_id.clone();
        let artifact = self
            .artifact_repo
            .get_by_id(&plan_artifact_id)
            .await
            .map_err(|error| {
                format!(
                    "failed to read plan artifact {}: {error}",
                    plan_artifact_id.as_str()
                )
            })?
            .ok_or_else(|| {
                format!(
                    "automation plan judge plan artifact {} not found",
                    plan_artifact_id.as_str()
                )
            })?;
        let ArtifactContent::Inline {
            text: overview_content,
        } = artifact.content
        else {
            return Err(format!(
                "automation plan judge plan artifact {} is not inline-readable",
                plan_artifact_id.as_str()
            ));
        };
        let (blueprint_artifact_id, blueprint_content) =
            if let Some(blueprint_id) = bundle.blueprint_id.as_ref() {
                let blueprint = self
                    .artifact_repo
                    .get_by_id(blueprint_id)
                    .await
                    .map_err(|error| {
                        format!(
                            "failed to read plan blueprint {}: {error}",
                            blueprint_id.as_str()
                        )
                    })?
                    .ok_or_else(|| {
                        format!(
                            "automation plan judge blueprint {} not found",
                            blueprint_id.as_str()
                        )
                    })?;
                let ArtifactContent::Inline {
                    text: blueprint_content,
                } = blueprint.content
                else {
                    return Err(format!(
                        "automation plan judge blueprint {} is not inline-readable",
                        blueprint_id.as_str()
                    ));
                };
                (
                    Some(blueprint_id.as_str().to_string()),
                    Some(blueprint_content),
                )
            } else {
                (None, None)
            };
        Ok(AutomationPlanJudgePayload {
            overview_artifact_id: plan_artifact_id.as_str().to_string(),
            overview_content,
            blueprint_artifact_id,
            blueprint_content,
            verification_context: self.verification_context.clone(),
        })
    }

    async fn invoke_and_parse_plan_judge(
        &self,
        automation: &Automation,
        run: &AutomationRun,
        payload: &AutomationPlanJudgePayload,
        retry_reminder: bool,
    ) -> Result<ParsedPlanJudgeInvocation, JudgeInvocationFailure> {
        let harness =
            AgentHarnessKind::from_str(automation.provider_harness.trim()).map_err(|error| {
                JudgeInvocationFailure::Invocation {
                    detail: error.to_string(),
                }
            })?;
        let output = self
            .plan_judge_invoker
            .invoke(AutomationPlanJudgeInvocation {
                automation: automation.clone(),
                run: run.clone(),
                overview_artifact_id: payload.overview_artifact_id.clone(),
                overview_content: payload.overview_content.clone(),
                blueprint_artifact_id: payload.blueprint_artifact_id.clone(),
                blueprint_content: payload.blueprint_content.clone(),
                verification_context: payload.verification_context.clone(),
                previous_verdict_json: run.plan_judge_verdict_json.clone(),
                retry_reminder,
                timeout: self.config.judge_timeout,
                plan_judge_model: Some(self.plan_judge_model_for_harness(harness)),
            })
            .await
            .map_err(|error| JudgeInvocationFailure::Invocation {
                detail: error.to_string(),
            })?;
        let verdict = parse_automation_plan_judge_verdict(
            &output.raw_output,
            AutomationPlanJudgeValidationContext {
                expected_overview_artifact_id: Some(&payload.overview_artifact_id),
                expected_blueprint_artifact_id: payload.blueprint_artifact_id.as_deref(),
                blueprint_truncation_blocks_approval: plan_blueprint_truncation_policy(
                    payload.blueprint_content.as_deref(),
                    run.plan_revision_round,
                )
                .blocks_approval(),
            },
        )
        .map_err(|error| JudgeInvocationFailure::InvalidOutput {
            detail: error.to_string(),
            raw_output: output.raw_output.clone(),
        })?;
        let verdict_json = serde_json::to_string(&verdict).map_err(|error| {
            JudgeInvocationFailure::InvalidOutput {
                detail: format!("failed to serialize normalized plan judge verdict: {error}"),
                raw_output: output.raw_output,
            }
        })?;
        Ok(ParsedPlanJudgeInvocation {
            verdict,
            verdict_json,
        })
    }

    fn plan_judge_model_for_harness(&self, harness: AgentHarnessKind) -> String {
        self.config
            .plan_judge_models
            .get(&harness)
            .cloned()
            .unwrap_or_else(|| plan_judge_model_for_provider(harness).to_string())
    }

    async fn apply_fresh_plan_judge_verdict(
        &self,
        automation: &Automation,
        run: &AutomationRun,
        parsed: ParsedPlanJudgeInvocation,
    ) -> AppResult<AutomationPlanJudgeTaskOutcome> {
        let Some(current) = self.current_plan_application_context(run).await? else {
            self.mark_plan_judge_failed(
                automation,
                run,
                "automation plan judge could not re-read current plan context".to_string(),
            )
            .await?;
            return Ok(AutomationPlanJudgeTaskOutcome {
                judge_failed: true,
                paused_automation: true,
                ..AutomationPlanJudgeTaskOutcome::default()
            });
        };
        if current.overview_artifact_id != parsed.verdict.evaluated_overview_artifact_id
            || current.blueprint_artifact_id != parsed.verdict.evaluated_blueprint_artifact_id
        {
            tracing::warn!(
                automation_id = %automation.id,
                run_id = %run.id,
                evaluated_overview_artifact_id = parsed.verdict.evaluated_overview_artifact_id,
                evaluated_blueprint_artifact_id = ?parsed.verdict.evaluated_blueprint_artifact_id,
                current_overview_artifact_id = current.overview_artifact_id,
                current_blueprint_artifact_id = ?current.blueprint_artifact_id,
                "Discarding automation plan judge verdict because the plan bundle changed"
            );
            self.transition_service
                .transition_plan_judge_state(
                    &run.id,
                    AutomationPlanJudgeState::InProgress,
                    AutomationPlanJudgeState::None,
                    None,
                    None,
                )
                .await?;
            self.transition_service
                .clear_plan_judge_verdict(&run.id)
                .await?;
            return Ok(AutomationPlanJudgeTaskOutcome {
                judge_succeeded: true,
                ..AutomationPlanJudgeTaskOutcome::default()
            });
        }

        match parsed.verdict.decision {
            AutomationPlanJudgeDecision::Approve => {
                if !self
                    .transition_service
                    .transition_plan_judge_state(
                        &run.id,
                        AutomationPlanJudgeState::InProgress,
                        AutomationPlanJudgeState::Done,
                        Some(parsed.verdict_json.clone()),
                        None,
                    )
                    .await?
                {
                    tracing::debug!(
                        automation_id = %automation.id,
                        run_id = %run.id,
                        "Discarding automation plan judge approval because the judge cycle was superseded"
                    );
                    return Ok(AutomationPlanJudgeTaskOutcome {
                        judge_succeeded: true,
                        ..AutomationPlanJudgeTaskOutcome::default()
                    });
                }
                if self
                    .plan_approval_repo
                    .get_by_session(&current.session_id)
                    .await?
                    .is_some_and(|approval| current.matches_approval(&approval))
                {
                    return Ok(AutomationPlanJudgeTaskOutcome {
                        judge_succeeded: true,
                        ..AutomationPlanJudgeTaskOutcome::default()
                    });
                }
                match self
                    .plan_approval_writer
                    .approve_current_plan_artifact(
                        current.session_id.clone(),
                        Some(current.overview_artifact_id.clone()),
                        PlanApprovalActor::Judge,
                    )
                    .await
                {
                    Ok(_) => {}
                    Err(AppError::Conflict(detail)) => {
                        tracing::debug!(
                            automation_id = %automation.id,
                            run_id = %run.id,
                            detail,
                            "Discarding automation plan judge approval because the plan changed before approval write"
                        );
                        self.transition_service
                            .transition_plan_judge_state(
                                &run.id,
                                AutomationPlanJudgeState::Done,
                                AutomationPlanJudgeState::None,
                                None,
                                None,
                            )
                            .await?;
                        self.transition_service
                            .clear_plan_judge_verdict(&run.id)
                            .await?;
                    }
                    Err(error) => return Err(error),
                }
            }
            AutomationPlanJudgeDecision::Revise => {
                if let Some(approval) = self
                    .plan_approval_repo
                    .get_by_session(&current.session_id)
                    .await?
                    .filter(|approval| current.matches_approval(approval))
                {
                    tracing::warn!(
                        automation_id = %automation.id,
                        run_id = %run.id,
                        artifact_id = %approval.artifact_id,
                        "Discarding automation plan judge revision because the plan is already approved"
                    );
                    if !self
                        .transition_service
                        .transition_plan_judge_state(
                            &run.id,
                            AutomationPlanJudgeState::InProgress,
                            AutomationPlanJudgeState::Done,
                            None,
                            None,
                        )
                        .await?
                    {
                        tracing::debug!(
                            automation_id = %automation.id,
                            run_id = %run.id,
                            "Discarding automation plan judge already-approved revision because the judge cycle was superseded"
                        );
                    }
                    return Ok(AutomationPlanJudgeTaskOutcome {
                        judge_succeeded: true,
                        ..AutomationPlanJudgeTaskOutcome::default()
                    });
                }

                let instructions = parsed
                    .verdict
                    .revision_instructions
                    .as_deref()
                    .unwrap_or("");
                if prior_revision_instruction_repeats(
                    run.plan_judge_verdict_json.as_deref(),
                    &current.overview_artifact_id,
                    current.blueprint_artifact_id.as_deref(),
                    instructions,
                ) {
                    if !self
                        .transition_service
                        .transition_plan_judge_state(
                            &run.id,
                            AutomationPlanJudgeState::InProgress,
                            AutomationPlanJudgeState::Failed,
                            None,
                            None,
                        )
                        .await?
                    {
                        tracing::debug!(
                            automation_id = %automation.id,
                            run_id = %run.id,
                            "Discarding repeated automation plan judge revision because the judge cycle was superseded"
                        );
                        return Ok(AutomationPlanJudgeTaskOutcome {
                            judge_succeeded: true,
                            ..AutomationPlanJudgeTaskOutcome::default()
                        });
                    }
                    let paused = self
                        .transition_service
                        .transition_automation_status(
                            &automation.id,
                            AutomationStatus::Active,
                            AutomationStatus::Paused,
                            Some(PLAN_REVISION_EXHAUSTED_PAUSED_REASON_CODE.to_string()),
                            Some(
                                "Automation plan judge repeated revision instructions".to_string(),
                            ),
                        )
                        .await?;
                    return Ok(AutomationPlanJudgeTaskOutcome {
                        judge_failed: true,
                        paused_automation: paused,
                        ..AutomationPlanJudgeTaskOutcome::default()
                    });
                }

                if !self
                    .transition_service
                    .transition_plan_judge_state(
                        &run.id,
                        AutomationPlanJudgeState::InProgress,
                        AutomationPlanJudgeState::Done,
                        Some(parsed.verdict_json),
                        None,
                    )
                    .await?
                {
                    tracing::debug!(
                        automation_id = %automation.id,
                        run_id = %run.id,
                        "Discarding automation plan judge revision because the judge cycle was superseded"
                    );
                    return Ok(AutomationPlanJudgeTaskOutcome {
                        judge_succeeded: true,
                        ..AutomationPlanJudgeTaskOutcome::default()
                    });
                }
                self.run_repo
                    .set_plan_pending_instructions(&run.id, Some(instructions.to_string()))
                    .await?;
            }
        }

        Ok(AutomationPlanJudgeTaskOutcome {
            judge_succeeded: true,
            ..AutomationPlanJudgeTaskOutcome::default()
        })
    }

    async fn current_plan_application_context(
        &self,
        run: &AutomationRun,
    ) -> AppResult<Option<PlanApplicationContext>> {
        let Some(conversation_id) = run.conversation_id.as_ref() else {
            return Ok(None);
        };
        let Some(workspace) = self
            .workspace_repo
            .get_by_conversation_id(conversation_id)
            .await?
        else {
            return Ok(None);
        };
        let Some(session_id) = workspace.linked_ideation_session_id.as_ref() else {
            return Ok(None);
        };
        let Some(session) = self.ideation_session_repo.get_by_id(session_id).await? else {
            return Ok(None);
        };
        let Some(bundle) = session.plan_artifact_bundle() else {
            return Ok(None);
        };
        Ok(Some(PlanApplicationContext {
            session_id: session_id.clone(),
            overview_artifact_id: bundle.overview_id.as_str().to_string(),
            blueprint_artifact_id: bundle.blueprint_id.map(|id| id.as_str().to_string()),
        }))
    }

    async fn mark_plan_judge_failed(
        &self,
        automation: &Automation,
        run: &AutomationRun,
        detail: String,
    ) -> AppResult<bool> {
        let transitioned = self
            .transition_service
            .transition_plan_judge_state(
                &run.id,
                AutomationPlanJudgeState::InProgress,
                AutomationPlanJudgeState::Failed,
                None,
                None,
            )
            .await?;
        if !transitioned {
            tracing::debug!(
                automation_id = %automation.id,
                run_id = %run.id,
                "Discarding automation plan judge failure because the judge cycle was superseded"
            );
        }
        if transitioned {
            self.transition_service
                .transition_automation_status(
                    &automation.id,
                    AutomationStatus::Active,
                    AutomationStatus::Paused,
                    Some(PLAN_JUDGE_FAILED_PAUSED_REASON_CODE.to_string()),
                    Some(detail),
                )
                .await
        } else {
            Ok(false)
        }
    }
}

#[derive(Debug, Clone)]
struct AutomationPlanJudgePayload {
    overview_artifact_id: String,
    overview_content: String,
    blueprint_artifact_id: Option<String>,
    blueprint_content: Option<String>,
    verification_context: Option<AutomationPlanVerificationJudgeContext>,
}

#[derive(Debug, Clone)]
struct PlanApplicationContext {
    session_id: crate::domain::entities::IdeationSessionId,
    overview_artifact_id: String,
    blueprint_artifact_id: Option<String>,
}

impl PlanApplicationContext {
    fn matches_approval(
        &self,
        approval: &crate::domain::repositories::PlanArtifactApproval,
    ) -> bool {
        approval.artifact_id.as_str() == self.overview_artifact_id
            && approval
                .blueprint_artifact_id
                .as_ref()
                .map(|id| id.as_str())
                == self.blueprint_artifact_id.as_deref()
    }
}

#[derive(Debug, Clone, Default)]
struct PlanVerificationJudgeGate {
    hold_judge: bool,
    context: Option<AutomationPlanVerificationJudgeContext>,
}

fn verification_status_is_terminal(status: VerificationStatus, in_progress: bool) -> bool {
    !in_progress && status != VerificationStatus::Reviewing
}

fn verification_status_is_in_progress(status: VerificationStatus, in_progress: bool) -> bool {
    in_progress || status == VerificationStatus::Reviewing
}

fn verification_status_allows_deep_start(status: VerificationStatus, in_progress: bool) -> bool {
    !in_progress
        && matches!(
            status,
            VerificationStatus::Unverified | VerificationStatus::Skipped
        )
}

fn verification_hold_timed_out(session: &IdeationSession, timeout: Duration) -> bool {
    Utc::now()
        .signed_duration_since(session.updated_at)
        .to_std()
        .unwrap_or_default()
        >= timeout
}

fn verification_snapshot_judge_context(
    snapshot: &VerificationRunSnapshot,
) -> AutomationPlanVerificationJudgeContext {
    AutomationPlanVerificationJudgeContext {
        status: snapshot.status.to_string(),
        in_progress: snapshot.in_progress,
        generation: Some(snapshot.generation),
        current_round: (snapshot.current_round > 0).then_some(snapshot.current_round),
        max_rounds: (snapshot.max_rounds > 0).then_some(snapshot.max_rounds),
        convergence_reason: snapshot.convergence_reason.clone(),
        gap_count: Some(snapshot.current_gaps.len()),
        gap_score: (!snapshot.current_gaps.is_empty()).then(|| gap_score(&snapshot.current_gaps)),
        gaps: snapshot
            .current_gaps
            .iter()
            .take(20)
            .map(|gap| AutomationPlanVerificationGapSummary {
                severity: gap.severity.clone(),
                category: gap.category.clone(),
                description: truncate_utf8_to_bytes(&gap.description, 1024).0,
                why_it_matters: gap
                    .why_it_matters
                    .as_ref()
                    .map(|text| truncate_utf8_to_bytes(text, 1024).0),
                source: gap.source.clone(),
            })
            .collect(),
        unavailable_reason: None,
    }
}

fn verification_summary_judge_context(
    session: &IdeationSession,
    status: VerificationStatus,
    in_progress: bool,
) -> AutomationPlanVerificationJudgeContext {
    AutomationPlanVerificationJudgeContext {
        status: status.to_string(),
        in_progress,
        generation: Some(session.verification_generation),
        current_round: session.verification_current_round,
        max_rounds: session.verification_max_rounds,
        convergence_reason: session.verification_convergence_reason.clone(),
        gap_count: Some(session.verification_gap_count as usize),
        gap_score: session.verification_gap_score,
        gaps: Vec::new(),
        unavailable_reason: None,
    }
}

fn model_native_verification_judge_context(
    status: PlanVerificationStatusKind,
) -> AutomationPlanVerificationJudgeContext {
    AutomationPlanVerificationJudgeContext {
        status: status.as_str().to_string(),
        in_progress: status.is_in_progress(),
        generation: None,
        current_round: None,
        max_rounds: None,
        convergence_reason: None,
        gap_count: None,
        gap_score: None,
        gaps: Vec::new(),
        unavailable_reason: None,
    }
}

fn verification_unavailable_judge_context(
    session: Option<&IdeationSession>,
    detail: String,
) -> AutomationPlanVerificationJudgeContext {
    AutomationPlanVerificationJudgeContext {
        status: "unavailable".to_string(),
        in_progress: false,
        generation: session.map(|session| session.verification_generation),
        current_round: session.and_then(|session| session.verification_current_round),
        max_rounds: session.and_then(|session| session.verification_max_rounds),
        convergence_reason: None,
        gap_count: None,
        gap_score: None,
        gaps: Vec::new(),
        unavailable_reason: Some(detail),
    }
}

impl AutomationJudgeTask {
    async fn run_for_terminal_run(
        &self,
        automation: Automation,
        runs: Vec<AutomationRun>,
        run: AutomationRun,
        judge_lease_expires_at: DateTime<Utc>,
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
                    self.mark_judge_failed(
                        &automation,
                        &run,
                        judge_lease_expires_at,
                        error.detail(),
                    )
                    .await?;
                    return Ok(AutomationJudgeTaskOutcome {
                        judge_failed: true,
                        ..AutomationJudgeTaskOutcome::default()
                    });
                }
            },
            Err(error) => {
                self.mark_judge_failed(&automation, &run, judge_lease_expires_at, error.detail())
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
                judge_lease_expires_at,
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
        judge_lease_expires_at: DateTime<Utc>,
        detail: String,
    ) -> AppResult<bool> {
        if !self
            .transition_service
            .transition_judge_state(
                &run.id,
                AutomationJudgeState::InProgress,
                AutomationJudgeState::Failed,
                AutomationJudgeTransitionGuard::Settle(judge_lease_expires_at),
                None,
                None,
                None,
                Some(detail.clone()),
            )
            .await?
        {
            return Ok(false);
        }
        let paused = self
            .transition_service
            .transition_automation_status(
                &automation.id,
                AutomationStatus::Active,
                AutomationStatus::Paused,
                Some("judge_failed".to_string()),
                Some(detail),
            )
            .await?;
        if paused {
            self.service
                .sync_goal_items_for_closed_run_without_successor(&automation.id)
                .await;
        }
        Ok(paused)
    }
}

pub struct AutomationScheduler {
    service: AutomationService,
    provisioner: AutomationRunProvisioner,
    agent_run_repo: Arc<dyn AgentRunRepository>,
    conversation_repo: Arc<dyn ChatConversationRepository>,
    run_repo: Arc<dyn AutomationRunRepository>,
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    ideation_session_repo: Arc<dyn IdeationSessionRepository>,
    plan_approval_repo: Arc<dyn PlanArtifactApprovalRepository>,
    plan_approval_writer: Arc<dyn PlanArtifactApprovalWriter>,
    artifact_repo: Arc<dyn ArtifactRepository>,
    transition_service: AutomationTransitionService,
    resumer: Arc<dyn AutomationRunResumer>,
    signal_checker: Arc<dyn AutomationSignalChecker>,
    integration_pr_publisher: Arc<dyn AutomationIntegrationPrPublisher>,
    judge_invoker: Arc<dyn AutomationJudgeInvoker>,
    plan_judge_invoker: Arc<dyn AutomationPlanJudgeInvoker>,
    plan_verification_starter: Arc<dyn AutomationPlanVerificationStarter>,
    merged_run_finalizer: Arc<dyn AutomationMergedRunFinalizer>,
    registry: Arc<AutomationSchedulerRegistry>,
    config: AutomationSchedulerConfig,
}

impl AutomationScheduler {
    pub(crate) fn new(
        automation_repo: Arc<dyn AutomationRepository>,
        run_repo: Arc<dyn AutomationRunRepository>,
        agent_run_repo: Arc<dyn AgentRunRepository>,
        conversation_repo: Arc<dyn ChatConversationRepository>,
        workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
        ideation_session_repo: Arc<dyn IdeationSessionRepository>,
        plan_approval_repo: Arc<dyn PlanArtifactApprovalRepository>,
        plan_approval_writer: Arc<dyn PlanArtifactApprovalWriter>,
        starter: Arc<dyn AutomationRunStarter>,
        resumer: Arc<dyn AutomationRunResumer>,
        signal_checker: Arc<dyn AutomationSignalChecker>,
        integration_pr_publisher: Arc<dyn AutomationIntegrationPrPublisher>,
        judge_invoker: Arc<dyn AutomationJudgeInvoker>,
        plan_judge_invoker: Arc<dyn AutomationPlanJudgeInvoker>,
        plan_verification_starter: Arc<dyn AutomationPlanVerificationStarter>,
        merged_run_finalizer: Arc<dyn AutomationMergedRunFinalizer>,
        event_emitter: Arc<dyn AutomationEventEmitter>,
        artifact_repo: Arc<dyn ArtifactRepository>,
        notification_service: Arc<NotificationService>,
        registry: Arc<AutomationSchedulerRegistry>,
        config: AutomationSchedulerConfig,
    ) -> Self {
        let service = AutomationService::new(
            Arc::clone(&automation_repo),
            Arc::clone(&run_repo),
            event_emitter.clone(),
            Arc::clone(&artifact_repo),
            notification_service.clone(),
        );
        let transition_service = AutomationTransitionService::new(
            Arc::clone(&automation_repo),
            Arc::clone(&run_repo),
            event_emitter.clone(),
            notification_service.clone(),
        );
        let provisioner = AutomationRunProvisioner::new(
            automation_repo,
            Arc::clone(&run_repo),
            Arc::clone(&conversation_repo),
            Arc::clone(&workspace_repo),
            starter,
            event_emitter,
            Arc::clone(&artifact_repo),
            notification_service,
        );
        Self {
            service,
            provisioner,
            agent_run_repo,
            conversation_repo,
            run_repo,
            workspace_repo,
            ideation_session_repo,
            plan_approval_repo,
            plan_approval_writer,
            artifact_repo,
            transition_service,
            resumer,
            signal_checker,
            integration_pr_publisher,
            judge_invoker,
            plan_judge_invoker,
            plan_verification_starter,
            merged_run_finalizer,
            registry,
            config,
        }
    }

    pub fn config(&self) -> &AutomationSchedulerConfig {
        &self.config
    }

    #[cfg(test)]
    pub(crate) fn with_merged_run_finalizer(
        mut self,
        merged_run_finalizer: Arc<dyn AutomationMergedRunFinalizer>,
    ) -> Self {
        self.merged_run_finalizer = merged_run_finalizer;
        self
    }

    pub async fn tick_once(&self) -> AppResult<AutomationSchedulerTickSummary> {
        let automations = self.service.list_automations(None).await?;
        let mut summary = AutomationSchedulerTickSummary {
            total_automations: automations.len(),
            ..AutomationSchedulerTickSummary::default()
        };

        for automation in automations {
            match automation.status {
                AutomationStatus::Active => {
                    summary.active_automations += 1;
                    let Some(_lease) = self.registry.try_acquire_automation(
                        &automation.id,
                        Instant::now(),
                        self.config.poll_interval,
                    ) else {
                        continue;
                    };
                    summary.leased_automations += 1;

                    let mut should_sweep_goal_items = false;
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
                                match run_could_need_goal_item_sweep(
                                    &detail.automation,
                                    latest_run,
                                    true,
                                ) {
                                    Ok(true) => {
                                        should_sweep_goal_items = true;
                                    }
                                    Ok(false) => {}
                                    Err(error) => {
                                        summary.automation_errors += 1;
                                        tracing::warn!(
                                            automation_id = %detail.automation.id,
                                            run_id = %latest_run.id,
                                            error = %error,
                                            "Automation scheduler failed to pre-screen goal item sweep"
                                        );
                                    }
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
                    if should_sweep_goal_items {
                        if let Err(error) =
                            self.sweep_goal_item_consistency(&automation.id, true).await
                        {
                            summary.automation_errors += 1;
                            tracing::warn!(
                                automation_id = %automation.id,
                                error = %error,
                                "Automation scheduler failed to sweep automation goal items"
                            );
                        }
                    }
                }
                AutomationStatus::Paused
                    if is_plan_gate_pause_reason(automation.paused_reason_code.as_deref()) =>
                {
                    let Some(_lease) = self.registry.try_acquire_automation(
                        &automation.id,
                        Instant::now(),
                        self.config.poll_interval,
                    ) else {
                        continue;
                    };
                    summary.leased_automations += 1;
                    if let Err(error) = self
                        .resume_plan_gate_paused_automation_on_approval(&automation, &mut summary)
                        .await
                    {
                        summary.automation_errors += 1;
                        tracing::warn!(
                            automation_id = %automation.id,
                            error = %error,
                            "Automation scheduler failed to scan paused plan gate approval"
                        );
                    }
                    if let Err(error) = self
                        .sweep_goal_item_consistency(&automation.id, false)
                        .await
                    {
                        summary.automation_errors += 1;
                        tracing::warn!(
                            automation_id = %automation.id,
                            error = %error,
                            "Automation scheduler failed to sweep automation goal items"
                        );
                    }
                }
                _ => {
                    if let Err(error) = self
                        .sweep_goal_item_consistency(&automation.id, false)
                        .await
                    {
                        summary.automation_errors += 1;
                        tracing::warn!(
                            automation_id = %automation.id,
                            error = %error,
                            "Automation scheduler failed to sweep automation goal items"
                        );
                    }
                }
            }
        }

        Ok(summary)
    }

    async fn sweep_goal_item_consistency(
        &self,
        automation_id: &AutomationId,
        allow_forward_fill: bool,
    ) -> AppResult<()> {
        let detail = self.service.get_automation_detail(automation_id).await?;
        let Some(latest_run) = detail.runs.last() else {
            return Ok(());
        };
        let plan_gate_paused = detail.automation.status == AutomationStatus::Paused
            && is_plan_gate_pause_reason(detail.automation.paused_reason_code.as_deref());

        let repair = if !plan_gate_paused && !latest_run_holds_goal_authority(latest_run) {
            revert_in_progress_goal_items_to_pending(detail.automation.goal_items_json.as_deref())?
                .map(|goal_items_json| ("revert", goal_items_json))
        } else if allow_forward_fill
            && detail.automation.status == AutomationStatus::Active
            && matches!(
                latest_run.status,
                AutomationRunStatus::Running
                    | AutomationRunStatus::AwaitingPlanApproval
                    | AutomationRunStatus::Published
            )
        {
            mark_current_goal_item_in_progress(detail.automation.goal_items_json.as_deref())?
                .map(|goal_items_json| ("forward_fill", goal_items_json))
        } else {
            None
        };

        let Some((repair_kind, goal_items_json)) = repair else {
            return Ok(());
        };
        let updated = self
            .service
            .update_goal_items_json_if_unchanged(
                automation_id,
                detail.automation.goal_items_json.clone(),
                Some(goal_items_json),
            )
            .await?;
        if updated {
            tracing::info!(
                automation_id = %automation_id,
                run_id = %latest_run.id,
                repair = repair_kind,
                "Automation scheduler repaired goal item progress"
            );
        } else {
            tracing::warn!(
                automation_id = %automation_id,
                run_id = %latest_run.id,
                repair = repair_kind,
                "Skipped automation goal item sweep repair because stored goal items changed"
            );
        }
        Ok(())
    }

    async fn resume_plan_gate_paused_automation_on_approval(
        &self,
        automation: &Automation,
        summary: &mut AutomationSchedulerTickSummary,
    ) -> AppResult<()> {
        let detail = self.service.get_automation_detail(&automation.id).await?;
        let Some(latest_run) = detail.runs.last() else {
            return Ok(());
        };
        if latest_run.status != AutomationRunStatus::AwaitingPlanApproval {
            return Ok(());
        }
        let Some(conversation_id) = latest_run.conversation_id.as_ref() else {
            return Ok(());
        };
        let Some(workspace) = self
            .workspace_repo
            .get_by_conversation_id(conversation_id)
            .await?
        else {
            return Ok(());
        };
        if matching_plan_approval_for_workspace(
            &self.ideation_session_repo,
            &self.plan_approval_repo,
            &workspace,
        )
        .await?
        .is_none()
        {
            return Ok(());
        }

        if self
            .transition_service
            .transition_automation_status(
                &automation.id,
                AutomationStatus::Paused,
                AutomationStatus::Active,
                None,
                None,
            )
            .await?
        {
            summary.resumed_automations += 1;
        }
        Ok(())
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
            AutomationRunStatus::AwaitingPlanApproval => {
                self.observe_awaiting_plan_approval_run(automation, run, summary)
                    .await?;
            }
            AutomationRunStatus::Published => {
                self.observe_published_run(automation, run, summary).await?;
            }
            AutomationRunStatus::Merged
            | AutomationRunStatus::Completed
            | AutomationRunStatus::PrClosed
            | AutomationRunStatus::AgentFailed => {
                if run.status == AutomationRunStatus::Completed
                    && automation.completion_signal == IDEATION_FINALIZED_COMPLETION_SIGNAL
                {
                    self.transition_service
                        .transition_automation_status(
                            &automation.id,
                            AutomationStatus::Active,
                            AutomationStatus::Completed,
                            None,
                            None,
                        )
                        .await?;
                    return Ok(());
                }
                if run.status == AutomationRunStatus::Merged
                    && !self.finalize_merged_run_conversation(automation, run).await
                {
                    return Ok(());
                }
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
        let Some(conversation_id) = run.conversation_id.as_ref() else {
            if running_run_has_exceeded(run, self.config.max_run_duration) {
                self.fail_running_run(
                    run,
                    "timeout",
                    "Automation run exceeded max_run_duration_secs",
                    summary,
                )
                .await?;
            }
            return Ok(());
        };

        let workspace = self
            .workspace_repo
            .get_by_conversation_id(conversation_id)
            .await?;
        if automation.completion_signal == IDEATION_FINALIZED_COMPLETION_SIGNAL {
            self.observe_ideation_bridge_run(automation, run, workspace.as_ref(), summary)
                .await?;
            return Ok(());
        }
        if let Some(workspace) = workspace.as_ref() {
            if workspace.mode == AgentConversationWorkspaceMode::Plan {
                self.observe_plan_phase_running_run(automation, run, workspace, summary)
                    .await?;
                return Ok(());
            }
        }

        if running_run_has_exceeded(run, self.config.max_run_duration) {
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

        let latest_agent_run = self
            .latest_agent_run_for_current_phase(conversation_id, run)
            .await?;
        let latest_agent_run_is_system_cancelled = latest_agent_run
            .as_ref()
            .is_some_and(agent_run_is_system_cancelled);

        if latest_agent_run.is_none() || latest_agent_run_is_system_cancelled {
            self.redeliver_plan_approval_after_crashed_resume(
                run,
                workspace.as_ref(),
                latest_agent_run.as_ref(),
                summary,
            )
            .await?;
        }

        if automation.completion_signal == "agent_completed" {
            match latest_agent_run.as_ref().map(|run| run.status) {
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
                Some(AgentRunStatus::Cancelled) if latest_agent_run_is_system_cancelled => {
                    // Recover in place (F2); the approval redelivery above owns this orphan.
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
                        self.service
                            .sync_goal_items_for_closed_run_without_successor(&automation.id)
                            .await;
                    }
                    return Ok(());
                }
                Some(AgentRunStatus::Running) | None => return Ok(()),
            }
        }
        let Some(workspace) = workspace else {
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
                if automation.pr_merge_mode == AutomationPrMergeMode::Automatic {
                    self.enable_run_auto_merge_preference_for_run(&run.id, &workspace)
                        .await;
                }
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
            // Settled pre-publication state: no auto-publish is in flight (no push
            // status yet, or only a base `refreshed`). If the run's agent process has
            // already terminated (exited, errored, or killed and pruned as
            // `pid_missing` -> agent_run `Cancelled`) without opening a pull request,
            // the run can never progress. Fail it now instead of leaving it Running
            // until the `max_run_duration` backstop hours later. Any in-flight
            // publish status (`pushing`/`pushed`/`checking`/`describing`, or a
            // `needs_agent` repair still within grace) and any unrecognized status is
            // left untouched by the final arm so an in-flight publish is not raced.
            push_status if publication_push_status_is_settled_pre_publication(push_status) => {
                if let Some(status) = latest_agent_run.as_ref().map(|agent_run| agent_run.status) {
                    // Only a genuinely dead agent is failed promptly here: `Failed`, or a
                    // process killed and pruned as `pid_missing` (-> agent_run `Cancelled`).
                    // A `Completed` agent is deliberately NOT failed: a cleanly-finished
                    // run is legitimately awaiting the workspace review -> auto-publish
                    // handoff, which can take minutes and does not set a push status until
                    // review passes. The scheduler is intentionally review-unaware, so
                    // failing on `Completed` would kill healthy runs mid-review; the
                    // `max_run_duration` backstop covers the rare genuinely-stuck
                    // `Completed` case instead.
                    // Recover in place (F2) before treating a restart orphan as terminal.
                    if matches!(status, AgentRunStatus::Failed | AgentRunStatus::Cancelled)
                        && !latest_agent_run_is_system_cancelled
                    {
                        self.fail_running_run(
                            run,
                            "agent_failed",
                            "Automation run agent exited before opening a pull request",
                            summary,
                        )
                        .await?;
                    }
                }
            }
            _ => {}
        }

        Ok(())
    }

    async fn observe_ideation_bridge_run(
        &self,
        automation: &Automation,
        run: &AutomationRun,
        workspace: Option<&AgentConversationWorkspace>,
        summary: &mut AutomationSchedulerTickSummary,
    ) -> AppResult<()> {
        let Some(workspace) = workspace else {
            self.fail_running_run(
                run,
                "ideation_bridge_missing_session",
                "Automation ideation bridge lost its planning workspace",
                summary,
            )
            .await?;
            return Ok(());
        };
        let Some(session_id) = workspace.linked_ideation_session_id.as_ref() else {
            self.fail_running_run(
                run,
                "ideation_bridge_missing_session",
                "Automation ideation bridge has no linked planning session",
                summary,
            )
            .await?;
            return Ok(());
        };
        let Some(session) = self.ideation_session_repo.get_by_id(session_id).await? else {
            self.fail_running_run(
                run,
                "ideation_bridge_missing_session",
                "Automation ideation bridge planning session was not found",
                summary,
            )
            .await?;
            return Ok(());
        };

        if session.status == IdeationSessionStatus::Accepted {
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
                self.transition_service
                    .transition_automation_status(
                        &automation.id,
                        AutomationStatus::Active,
                        AutomationStatus::Completed,
                        None,
                        None,
                    )
                    .await?;
            }
            return Ok(());
        }
        if session.status != IdeationSessionStatus::Active {
            self.fail_running_run(
                run,
                "ideation_bridge_not_finalized",
                &format!(
                    "Automation ideation bridge session entered {} before finalization",
                    session.status
                ),
                summary,
            )
            .await?;
            return Ok(());
        }
        if running_run_has_exceeded(run, self.config.max_run_duration) {
            self.fail_running_run(
                run,
                "timeout",
                "Automation ideation bridge exceeded max_run_duration_secs",
                summary,
            )
            .await?;
            return Ok(());
        }

        let bridge_conversation = self
            .conversation_repo
            .get_active_for_context(ChatContextType::Ideation, session_id.as_str())
            .await?;
        let bridge_agent_run = match bridge_conversation.as_ref() {
            Some(conversation) => {
                self.latest_agent_run_for_current_phase(&conversation.id, run)
                    .await?
            }
            None => None,
        };
        let system_cancelled = bridge_agent_run
            .as_ref()
            .is_some_and(agent_run_is_system_cancelled);
        match bridge_agent_run.as_ref().map(|agent_run| agent_run.status) {
            Some(AgentRunStatus::Failed) => {
                self.fail_running_run(
                    run,
                    "ideation_bridge_agent_failed",
                    "Automation ideation bridge agent exited before finalizing proposals",
                    summary,
                )
                .await?;
            }
            Some(AgentRunStatus::Cancelled) if !system_cancelled => {
                self.fail_running_run(
                    run,
                    "ideation_bridge_agent_failed",
                    "Automation ideation bridge agent exited before finalizing proposals",
                    summary,
                )
                .await?;
            }
            Some(AgentRunStatus::Completed) => {
                self.fail_running_run(
                    run,
                    "ideation_bridge_not_finalized",
                    "Automation ideation bridge agent completed without finalizing proposals",
                    summary,
                )
                .await?;
            }
            Some(AgentRunStatus::Running) => {}
            Some(AgentRunStatus::Cancelled) | None => {
                if session.pending_initial_prompt.is_some()
                    || self.resumer.is_ideation_agent_running(session_id).await?
                {
                    return Ok(());
                }
                let Some(approval) = matching_plan_approval_for_workspace(
                    &self.ideation_session_repo,
                    &self.plan_approval_repo,
                    workspace,
                )
                .await?
                else {
                    self.fail_running_run(
                        run,
                        "ideation_bridge_approval_missing",
                        "Automation ideation bridge lost its approved plan",
                        summary,
                    )
                    .await?;
                    return Ok(());
                };
                self.resumer
                    .resume_ideation_with_prompt(
                        session_id,
                        &ideation_bridge_delivery_prompt(&approval),
                    )
                    .await?;
            }
        }
        Ok(())
    }

    async fn observe_plan_phase_running_run(
        &self,
        automation: &Automation,
        run: &AutomationRun,
        workspace: &AgentConversationWorkspace,
        summary: &mut AutomationSchedulerTickSummary,
    ) -> AppResult<()> {
        let Some(conversation_id) = run.conversation_id.as_ref() else {
            return Ok(());
        };

        let latest_agent_run = self
            .latest_agent_run_for_current_phase(conversation_id, run)
            .await?;

        let latest_agent_run_is_system_cancelled = latest_agent_run
            .as_ref()
            .is_some_and(agent_run_is_system_cancelled);

        match latest_agent_run.as_ref().map(|agent_run| agent_run.status) {
            Some(AgentRunStatus::Failed) => {
                self.fail_running_run(
                    run,
                    "agent_failed",
                    "Automation run agent failed during the planning phase",
                    summary,
                )
                .await?;
            }
            Some(AgentRunStatus::Cancelled) if latest_agent_run_is_system_cancelled => {
                self.observe_recoverable_plan_phase_running_run(
                    run,
                    conversation_id,
                    PlanRedeliveryTrigger::RestartOrphan,
                    summary,
                )
                .await?;
            }
            Some(AgentRunStatus::Cancelled) => {
                if self
                    .transition_service
                    .transition_run_status(
                        &run.id,
                        AutomationRunStatus::Running,
                        AutomationRunStatus::Cancelled,
                        Some("agent_cancelled".to_string()),
                        Some("Automation run planning agent was cancelled".to_string()),
                    )
                    .await?
                {
                    summary.failed_runs += 1;
                    self.service
                        .sync_goal_items_for_closed_run_without_successor(&automation.id)
                        .await;
                }
            }
            Some(AgentRunStatus::Completed) => {
                if self
                    .park_run_at_plan_approval_if_artifact_exists(
                        automation,
                        run,
                        workspace,
                        AutomationRunStatus::Running,
                        summary,
                    )
                    .await?
                {
                    return Ok(());
                }

                self.handle_missing_plan_artifact_after_completed_turn(run, summary)
                    .await?;
            }
            Some(AgentRunStatus::Running) | None => {
                self.observe_recoverable_plan_phase_running_run(
                    run,
                    conversation_id,
                    if latest_agent_run.is_none() {
                        PlanRedeliveryTrigger::LatestRunMissing
                    } else {
                        PlanRedeliveryTrigger::None
                    },
                    summary,
                )
                .await?;
            }
        }

        Ok(())
    }

    async fn observe_recoverable_plan_phase_running_run(
        &self,
        run: &AutomationRun,
        conversation_id: &ChatConversationId,
        redelivery_trigger: PlanRedeliveryTrigger,
        summary: &mut AutomationSchedulerTickSummary,
    ) -> AppResult<()> {
        if running_run_has_exceeded(run, self.config.max_run_duration) {
            self.fail_running_run(
                run,
                "timeout",
                "Automation run exceeded max_run_duration_secs",
                summary,
            )
            .await?;
            return Ok(());
        }
        if redelivery_trigger != PlanRedeliveryTrigger::None {
            self.redeliver_plan_reminder_after_crashed_resume(
                run,
                conversation_id,
                redelivery_trigger,
                summary,
            )
            .await?;
        }
        Ok(())
    }

    async fn park_run_at_plan_approval_if_artifact_exists(
        &self,
        automation: &Automation,
        run: &AutomationRun,
        workspace: &AgentConversationWorkspace,
        from_status: AutomationRunStatus,
        summary: &mut AutomationSchedulerTickSummary,
    ) -> AppResult<bool> {
        let Some(plan_artifacts) =
            current_plan_artifact_ids_for_workspace(&self.ideation_session_repo, workspace).await?
        else {
            return Ok(false);
        };
        let plan_artifact_id = Some(plan_artifacts.target_id.clone());

        if self
            .transition_service
            .transition_run_status(
                &run.id,
                from_status,
                AutomationRunStatus::AwaitingPlanApproval,
                None,
                None,
            )
            .await?
        {
            let baseline_changed = refresh_plan_park_baseline(
                &self.transition_service,
                &self.run_repo,
                run,
                Some(plan_artifacts.overview_id),
                plan_artifacts.blueprint_id,
            )
            .await?;
            let parked_run = self
                .run_repo
                .get_by_id(&run.id)
                .await?
                .unwrap_or_else(|| run.clone());
            if automation.plan_deep_verification
                && matching_plan_approval_for_workspace(
                    &self.ideation_session_repo,
                    &self.plan_approval_repo,
                    workspace,
                )
                .await?
                .is_none()
            {
                let verification_gate = self
                    .build_plan_verification_gate(
                        automation,
                        workspace,
                        plan_artifact_id.as_deref(),
                        baseline_changed,
                    )
                    .await;
                if automation.plan_approval_mode == AutomationPlanApprovalMode::Automatic {
                    self.observe_automatic_plan_judge(
                        automation,
                        &parked_run,
                        &verification_gate,
                        summary,
                    )
                    .await?;
                }
            }
        }
        Ok(true)
    }

    async fn latest_agent_run_for_current_phase(
        &self,
        conversation_id: &ChatConversationId,
        run: &AutomationRun,
    ) -> AppResult<Option<AgentRun>> {
        let Some(agent_run) = self
            .agent_run_repo
            .get_latest_for_conversation(conversation_id)
            .await?
        else {
            return Ok(None);
        };
        if agent_run_is_current_for_phase(run, &agent_run) {
            Ok(Some(agent_run))
        } else {
            Ok(None)
        }
    }

    // pub(super) so scheduler_tests can exercise the defensive current-run guard directly.
    pub(super) async fn redeliver_plan_approval_after_crashed_resume(
        &self,
        run: &AutomationRun,
        workspace: Option<&AgentConversationWorkspace>,
        latest_agent_run: Option<&AgentRun>,
        summary: &mut AutomationSchedulerTickSummary,
    ) -> AppResult<()> {
        if latest_agent_run.is_some_and(|agent_run| !agent_run_is_system_cancelled(agent_run)) {
            return Ok(());
        }
        if run.agent_phase_started_at.is_none() {
            return Ok(());
        }
        let Some(workspace) = workspace else {
            return Ok(());
        };
        if workspace.mode != AgentConversationWorkspaceMode::Edit {
            return Ok(());
        }
        let Some(conversation_id) = run.conversation_id.as_ref() else {
            return Ok(());
        };
        let Some(approval) = matching_plan_approval_for_workspace(
            &self.ideation_session_repo,
            &self.plan_approval_repo,
            workspace,
        )
        .await?
        else {
            return Ok(());
        };
        if self.resumer.is_agent_running(conversation_id).await? {
            return Ok(());
        }
        if self.resumer.launches_paused().await? {
            return Ok(());
        }

        self.resumer.switch_to_edit(conversation_id).await?;
        let prompt = approval_delivery_prompt(&approval);
        match self
            .resumer
            .resume_with_prompt(conversation_id, &prompt)
            .await
        {
            Ok(ResumeDelivery::Delivered | ResumeDelivery::QueuedAndPurged) => {}
            Err(error) => {
                self.fail_running_run(
                    run,
                    PLAN_RESUME_FAILED_ERROR_CODE,
                    &format!("Automation plan approval delivery failed: {error}"),
                    summary,
                )
                .await?;
            }
        }
        Ok(())
    }

    async fn redeliver_plan_reminder_after_crashed_resume(
        &self,
        run: &AutomationRun,
        conversation_id: &ChatConversationId,
        redelivery_trigger: PlanRedeliveryTrigger,
        summary: &mut AutomationSchedulerTickSummary,
    ) -> AppResult<()> {
        if run.agent_phase_started_at.is_none()
            || (run.plan_reminder_count == 0
                && redelivery_trigger != PlanRedeliveryTrigger::RestartOrphan)
        {
            return Ok(());
        }
        if self.resumer.is_agent_running(conversation_id).await? {
            return Ok(());
        }
        if self.resumer.launches_paused().await? {
            return Ok(());
        }

        match self
            .resumer
            .resume_with_prompt(conversation_id, AUTOMATION_PLAN_REMINDER_PROMPT)
            .await
        {
            Ok(ResumeDelivery::Delivered) => {
                if run.plan_reminder_count == 0 {
                    self.run_repo.set_plan_reminder_count(&run.id, 1).await?;
                }
            }
            Ok(ResumeDelivery::QueuedAndPurged) => {}
            Err(error) => {
                self.fail_running_run(
                    run,
                    "plan_reminder_failed",
                    &format!("Automation plan reminder failed: {error}"),
                    summary,
                )
                .await?;
            }
        }

        Ok(())
    }

    async fn handle_missing_plan_artifact_after_completed_turn(
        &self,
        run: &AutomationRun,
        summary: &mut AutomationSchedulerTickSummary,
    ) -> AppResult<()> {
        let Some(conversation_id) = run.conversation_id.as_ref() else {
            return Ok(());
        };

        if run.plan_reminder_count > 0 {
            self.fail_running_run(
                run,
                "plan_not_submitted",
                "Automation planning turn ended without submitting a plan artifact",
                summary,
            )
            .await?;
            return Ok(());
        }

        if self.resumer.is_agent_running(conversation_id).await? {
            return Ok(());
        }
        if self.resumer.launches_paused().await? {
            return Ok(());
        }

        match self
            .resumer
            .resume_with_prompt(conversation_id, AUTOMATION_PLAN_REMINDER_PROMPT)
            .await
        {
            Ok(ResumeDelivery::Delivered) => {
                self.run_repo
                    .set_plan_reminder_count(&run.id, run.plan_reminder_count.saturating_add(1))
                    .await?;
            }
            Ok(ResumeDelivery::QueuedAndPurged) => {}
            Err(error) => {
                self.fail_running_run(
                    run,
                    "plan_reminder_failed",
                    &format!("Automation plan reminder failed: {error}"),
                    summary,
                )
                .await?;
            }
        }

        Ok(())
    }

    async fn build_plan_verification_gate(
        &self,
        automation: &Automation,
        workspace: &AgentConversationWorkspace,
        plan_artifact_id: Option<&str>,
        baseline_changed: bool,
    ) -> PlanVerificationJudgeGate {
        if !automation.plan_deep_verification {
            return PlanVerificationJudgeGate::default();
        }

        let Some(session_id) = workspace.linked_ideation_session_id.clone() else {
            return PlanVerificationJudgeGate {
                context: Some(verification_unavailable_judge_context(
                    None,
                    "planning session is unavailable".to_string(),
                )),
                ..PlanVerificationJudgeGate::default()
            };
        };

        let mut session = match self.ideation_session_repo.get_by_id(&session_id).await {
            Ok(Some(session)) => session,
            Ok(None) => {
                return PlanVerificationJudgeGate {
                    context: Some(verification_unavailable_judge_context(
                        None,
                        "planning session was not found".to_string(),
                    )),
                    ..PlanVerificationJudgeGate::default()
                };
            }
            Err(error) => {
                tracing::warn!(
                    automation_id = %automation.id,
                    session_id = %session_id.as_str(),
                    error = %error,
                    "Automation plan verification state read failed"
                );
                return PlanVerificationJudgeGate {
                    context: Some(verification_unavailable_judge_context(
                        None,
                        format!("verification state read failed: {error}"),
                    )),
                    ..PlanVerificationJudgeGate::default()
                };
            }
        };

        let Some(artifact_id) = plan_artifact_id else {
            return PlanVerificationJudgeGate {
                context: Some(verification_unavailable_judge_context(
                    Some(&session),
                    "planning session has no linked plan".to_string(),
                )),
                ..PlanVerificationJudgeGate::default()
            };
        };
        let verification_request = AutomationPlanVerificationStartRequest {
            session_id: session_id.clone(),
            artifact_id: artifact_id.to_string(),
            provider_harness: AgentHarnessKind::from_str(automation.provider_harness.trim()).ok(),
        };
        let mut action_status = match self
            .plan_verification_starter
            .verification_status(&verification_request)
            .await
        {
            Ok(status) => status,
            Err(error) => {
                tracing::warn!(
                    automation_id = %automation.id,
                    session_id = %session_id.as_str(),
                    error = %error,
                    "Automation model-native plan verification status read failed"
                );
                return PlanVerificationJudgeGate {
                    context: Some(verification_unavailable_judge_context(
                        Some(&session),
                        format!("model-native verification status read failed: {error}"),
                    )),
                    ..PlanVerificationJudgeGate::default()
                };
            }
        };

        let mut effective_status =
            match load_effective_verification_status(self.ideation_session_repo.as_ref(), &session)
                .await
            {
                Ok(status) => status,
                Err(error) => {
                    tracing::warn!(
                        automation_id = %automation.id,
                        session_id = %session_id.as_str(),
                        error = %error,
                        "Automation plan verification effective status read failed"
                    );
                    return PlanVerificationJudgeGate {
                        context: Some(verification_unavailable_judge_context(
                            Some(&session),
                            format!("verification effective status read failed: {error}"),
                        )),
                        ..PlanVerificationJudgeGate::default()
                    };
                }
            };

        let mut force_hold_after_start = false;
        if action_status == PlanVerificationStatusKind::Unverified
            && baseline_changed
            && verification_status_allows_deep_start(effective_status.0, effective_status.1)
        {
            match self
                .plan_verification_starter
                .start_verification(verification_request.clone())
                .await
            {
                Ok(AutomationPlanVerificationStartOutcome::Unavailable { detail }) => {
                    tracing::warn!(
                        automation_id = %automation.id,
                        session_id = %session_id.as_str(),
                        artifact_id,
                        detail,
                        "Automation plan verification could not be started"
                    );
                    return PlanVerificationJudgeGate {
                        context: Some(verification_unavailable_judge_context(
                            Some(&session),
                            detail,
                        )),
                        ..PlanVerificationJudgeGate::default()
                    };
                }
                Ok(outcome) => {
                    force_hold_after_start = matches!(
                        &outcome,
                        AutomationPlanVerificationStartOutcome::Started { .. }
                            | AutomationPlanVerificationStartOutcome::AlreadyInProgress { .. }
                    );
                    action_status = match outcome {
                        AutomationPlanVerificationStartOutcome::Started { .. }
                        | AutomationPlanVerificationStartOutcome::AlreadyInProgress { .. } => {
                            match self
                                .plan_verification_starter
                                .verification_status(&verification_request)
                                .await
                            {
                                Ok(status) => status,
                                Err(error) => {
                                    tracing::warn!(
                                        automation_id = %automation.id,
                                        session_id = %session_id.as_str(),
                                        error = %error,
                                        "Automation post-start model-native verification status read failed"
                                    );
                                    return PlanVerificationJudgeGate {
                                            hold_judge: true,
                                            context: Some(verification_unavailable_judge_context(
                                                Some(&session),
                                                format!("verification started but status read failed: {error}"),
                                            )),
                                        };
                                }
                            }
                        }
                        AutomationPlanVerificationStartOutcome::AlreadyTerminal {
                            status:
                                VerificationStatus::Verified | VerificationStatus::ImportedVerified,
                            ..
                        } => PlanVerificationStatusKind::Verified,
                        AutomationPlanVerificationStartOutcome::AlreadyTerminal { .. } => {
                            PlanVerificationStatusKind::Failed
                        }
                        AutomationPlanVerificationStartOutcome::Unavailable { .. } => {
                            PlanVerificationStatusKind::Unverified
                        }
                    };
                    match self.ideation_session_repo.get_by_id(&session_id).await {
                        Ok(Some(updated)) => {
                            session = updated;
                            effective_status = match load_effective_verification_status(
                                self.ideation_session_repo.as_ref(),
                                &session,
                            )
                            .await
                            {
                                Ok(status) => status,
                                Err(error) => {
                                    tracing::warn!(
                                        automation_id = %automation.id,
                                        session_id = %session_id.as_str(),
                                        error = %error,
                                        "Automation plan verification post-start effective status read failed"
                                    );
                                    return PlanVerificationJudgeGate {
                                            context: Some(verification_unavailable_judge_context(
                                                Some(&session),
                                                format!(
                                                    "verification post-start effective status read failed: {error}"
                                                ),
                                            )),
                                            ..PlanVerificationJudgeGate::default()
                                        };
                                }
                            };
                        }
                        Ok(None) => {
                            return PlanVerificationJudgeGate {
                                context: Some(verification_unavailable_judge_context(
                                    None,
                                    "planning session disappeared after verification start"
                                        .to_string(),
                                )),
                                ..PlanVerificationJudgeGate::default()
                            };
                        }
                        Err(error) => {
                            tracing::warn!(
                                automation_id = %automation.id,
                                session_id = %session_id.as_str(),
                                error = %error,
                                "Automation plan verification post-start state read failed"
                            );
                            return PlanVerificationJudgeGate {
                                context: Some(verification_unavailable_judge_context(
                                    Some(&session),
                                    format!("verification post-start state read failed: {error}"),
                                )),
                                ..PlanVerificationJudgeGate::default()
                            };
                        }
                    }
                }
                Err(error) => {
                    tracing::warn!(
                        automation_id = %automation.id,
                        session_id = %session_id.as_str(),
                        artifact_id,
                        error = %error,
                        "Automation plan verification start failed"
                    );
                    return PlanVerificationJudgeGate {
                        context: Some(verification_unavailable_judge_context(
                            Some(&session),
                            format!("verification start failed: {error}"),
                        )),
                        ..PlanVerificationJudgeGate::default()
                    };
                }
            }
        }

        if action_status != PlanVerificationStatusKind::Unverified {
            let context = model_native_verification_judge_context(action_status);
            if action_status.is_in_progress() {
                if verification_hold_timed_out(&session, self.config.plan_verification_hold_timeout)
                {
                    return PlanVerificationJudgeGate {
                        context: Some(verification_unavailable_judge_context(
                            Some(&session),
                            format!(
                                "verification did not reach a terminal state within {} seconds",
                                self.config.plan_verification_hold_timeout.as_secs()
                            ),
                        )),
                        ..PlanVerificationJudgeGate::default()
                    };
                }
                return PlanVerificationJudgeGate {
                    hold_judge: true,
                    context: Some(context),
                };
            }
            return PlanVerificationJudgeGate {
                hold_judge: false,
                context: Some(context),
            };
        }

        let (status, in_progress) = effective_status;

        let context = match load_current_verification_snapshot_or_default(
            self.ideation_session_repo.as_ref(),
            &session,
            status,
            in_progress,
        )
        .await
        {
            Ok(snapshot) => verification_snapshot_judge_context(&snapshot),
            Err(error) => {
                tracing::warn!(
                    automation_id = %automation.id,
                    session_id = %session_id.as_str(),
                    error = %error,
                    "Automation plan verification snapshot read failed"
                );
                verification_summary_judge_context(&session, status, in_progress)
            }
        };

        if force_hold_after_start && verification_status_allows_deep_start(status, in_progress) {
            return PlanVerificationJudgeGate {
                hold_judge: true,
                context: Some(context),
            };
        }

        if verification_status_is_in_progress(status, in_progress) {
            if verification_hold_timed_out(&session, self.config.plan_verification_hold_timeout) {
                tracing::warn!(
                    automation_id = %automation.id,
                    session_id = %session_id.as_str(),
                    timeout_secs = self.config.plan_verification_hold_timeout.as_secs(),
                    "Automation plan verification hold timed out; proceeding with advisory marker"
                );
                return PlanVerificationJudgeGate {
                    context: Some(verification_unavailable_judge_context(
                        Some(&session),
                        format!(
                            "verification did not reach a terminal state within {} seconds",
                            self.config.plan_verification_hold_timeout.as_secs()
                        ),
                    )),
                    ..PlanVerificationJudgeGate::default()
                };
            }

            return PlanVerificationJudgeGate {
                hold_judge: true,
                context: Some(context),
            };
        }

        if verification_status_is_terminal(status, in_progress) {
            return PlanVerificationJudgeGate {
                hold_judge: false,
                context: Some(context),
            };
        }

        PlanVerificationJudgeGate {
            context: Some(context),
            ..PlanVerificationJudgeGate::default()
        }
    }

    async fn observe_awaiting_plan_approval_run(
        &self,
        automation: &Automation,
        run: &AutomationRun,
        summary: &mut AutomationSchedulerTickSummary,
    ) -> AppResult<()> {
        let Some(conversation_id) = run.conversation_id.as_ref() else {
            return Ok(());
        };
        let workspace = self
            .workspace_repo
            .get_by_conversation_id(conversation_id)
            .await?;
        let Some(workspace) = workspace else {
            if automation.run_mode == IDEATION_BRIDGE_RUN_MODE {
                self.pause_ideation_bridge_for_missing_session(
                    automation,
                    "Automation ideation bridge lost its planning workspace",
                    summary,
                )
                .await?;
            }
            return Ok(());
        };
        if automation.run_mode == IDEATION_BRIDGE_RUN_MODE
            && workspace.linked_ideation_session_id.is_none()
        {
            self.pause_ideation_bridge_for_missing_session(
                automation,
                "Automation ideation bridge has no linked planning session",
                summary,
            )
            .await?;
            return Ok(());
        }

        if self.resumer.is_agent_running(conversation_id).await? {
            let agent_phase_started_at = self
                .agent_run_repo
                .get_active_for_conversation(conversation_id)
                .await?
                .map(|agent_run| agent_run.started_at)
                .unwrap_or_else(Utc::now);
            self.transition_service
                .transition_run_status_with_agent_phase_started_at(
                    &run.id,
                    AutomationRunStatus::AwaitingPlanApproval,
                    AutomationRunStatus::Running,
                    agent_phase_started_at,
                    None,
                    None,
                )
                .await?;
            return Ok(());
        }

        let plan_artifacts =
            current_plan_artifact_ids_for_workspace(&self.ideation_session_repo, &workspace)
                .await?;
        let plan_artifact_id = plan_artifacts
            .as_ref()
            .map(|artifacts| artifacts.target_id.clone());
        let baseline_changed = refresh_plan_park_baseline(
            &self.transition_service,
            &self.run_repo,
            run,
            plan_artifacts
                .as_ref()
                .map(|artifacts| artifacts.overview_id.clone()),
            plan_artifacts.and_then(|artifacts| artifacts.blueprint_id),
        )
        .await?;
        let run = self
            .run_repo
            .get_by_id(&run.id)
            .await?
            .unwrap_or_else(|| run.clone());

        if let Some(approval) = matching_plan_approval_for_workspace(
            &self.ideation_session_repo,
            &self.plan_approval_repo,
            &workspace,
        )
        .await?
        {
            self.deliver_plan_approval(automation, &run, &workspace, &approval, summary)
                .await?;
            return Ok(());
        }

        let verification_gate = self
            .build_plan_verification_gate(
                automation,
                &workspace,
                plan_artifact_id.as_deref(),
                baseline_changed,
            )
            .await;

        if let Some(instructions) = run.plan_pending_instructions.as_deref() {
            if workspace.mode == AgentConversationWorkspaceMode::Plan {
                self.deliver_plan_revision(&run, conversation_id, instructions, summary)
                    .await?;
            }
            return Ok(());
        }

        if automation.plan_approval_mode == AutomationPlanApprovalMode::Automatic {
            self.observe_automatic_plan_judge(automation, &run, &verification_gate, summary)
                .await?;
        }
        Ok(())
    }

    async fn observe_automatic_plan_judge(
        &self,
        automation: &Automation,
        run: &AutomationRun,
        verification_gate: &PlanVerificationJudgeGate,
        summary: &mut AutomationSchedulerTickSummary,
    ) -> AppResult<()> {
        // N = max judge-issued revisions; the (N+1)th plan version parks for human review.
        if run.plan_revision_round.saturating_sub(1) >= self.config.plan_max_revision_rounds {
            self.pause_automation_for_plan_revision_exhaustion(
                automation,
                "Automation plan revision round limit reached".to_string(),
                summary,
            )
            .await?;
            return Ok(());
        }

        if verification_gate.hold_judge {
            return Ok(());
        }

        match run.plan_judge_state {
            AutomationPlanJudgeState::None => {
                if run.plan_pending_instructions.is_some() {
                    return Ok(());
                }
                if self
                    .transition_service
                    .transition_plan_judge_state(
                        &run.id,
                        AutomationPlanJudgeState::None,
                        AutomationPlanJudgeState::InProgress,
                        None,
                        Some(automation_judge_lease_expires_at(self.config.judge_timeout)),
                    )
                    .await?
                {
                    summary.judges_started += 1;
                    spawn_automation_plan_judge_task(
                        self.transition_service.clone(),
                        Arc::clone(&self.run_repo),
                        Arc::clone(&self.workspace_repo),
                        Arc::clone(&self.ideation_session_repo),
                        Arc::clone(&self.plan_approval_repo),
                        Arc::clone(&self.plan_approval_writer),
                        Arc::clone(&self.artifact_repo),
                        Arc::clone(&self.plan_judge_invoker),
                        self.config.clone(),
                        automation.clone(),
                        run.clone(),
                        verification_gate.context.clone(),
                    );
                }
            }
            AutomationPlanJudgeState::InProgress
                if plan_judge_has_exceeded(run, self.config.judge_timeout) =>
            {
                self.mark_plan_judge_failed(
                    automation,
                    run,
                    "Automation plan judge exceeded judge_timeout_secs".to_string(),
                    summary,
                )
                .await?;
            }
            AutomationPlanJudgeState::Done => {
                self.apply_stored_plan_judge_verdict(automation, run, summary)
                    .await?;
            }
            AutomationPlanJudgeState::InProgress | AutomationPlanJudgeState::Failed => {}
        }
        Ok(())
    }

    async fn deliver_plan_approval(
        &self,
        automation: &Automation,
        run: &AutomationRun,
        workspace: &AgentConversationWorkspace,
        approval: &PlanArtifactApproval,
        summary: &mut AutomationSchedulerTickSummary,
    ) -> AppResult<()> {
        let Some(conversation_id) = run.conversation_id.as_ref() else {
            return Ok(());
        };
        if self.resumer.is_agent_running(conversation_id).await? {
            return Ok(());
        }
        if self.resumer.launches_paused().await? {
            return Ok(());
        }

        let bridge_session_id = if automation.run_mode == IDEATION_BRIDGE_RUN_MODE {
            let Some(session_id) = workspace.linked_ideation_session_id.as_ref() else {
                self.pause_ideation_bridge_for_missing_session(
                    automation,
                    "Automation ideation bridge has no linked planning session",
                    summary,
                )
                .await?;
                return Ok(());
            };
            let verified = self
                .ideation_session_repo
                .get_by_id(session_id)
                .await?
                .is_some_and(|session| session.verification_status == VerificationStatus::Verified);
            if !verified {
                if self
                    .transition_service
                    .transition_automation_status(
                        &automation.id,
                        AutomationStatus::Active,
                        AutomationStatus::Paused,
                        Some("ideation_bridge_verification_failed".to_string()),
                        Some(
                            "The approved automation bridge plan did not complete deep verification"
                                .to_string(),
                        ),
                    )
                    .await?
                {
                    summary.paused_automations += 1;
                }
                return Ok(());
            }
            Some(session_id.clone())
        } else {
            None
        };

        if self
            .service
            .apply_pending_goal_replan_for_run(&automation.id, run)
            .await?
            == PendingGoalReplanApplyOutcome::Stale
        {
            if self
                .transition_service
                .transition_automation_status(
                    &automation.id,
                    AutomationStatus::Active,
                    AutomationStatus::Paused,
                    Some("goal_replan_stale".to_string()),
                    Some(
                        "Goal items changed after the judge proposed a structural re-plan; review the proposal before resuming"
                            .to_string(),
                    ),
                )
                .await?
            {
                summary.paused_automations += 1;
            }
            return Ok(());
        }

        clear_plan_phase_publication_metadata(&self.run_repo, &self.workspace_repo, run, workspace)
            .await?;
        if bridge_session_id.is_some() {
            self.resumer.switch_to_ideation(conversation_id).await?;
        } else {
            self.resumer.switch_to_edit(conversation_id).await?;
        }
        if !self
            .transition_service
            .transition_run_status_clearing_plan_pending_instructions(
                &run.id,
                AutomationRunStatus::AwaitingPlanApproval,
                AutomationRunStatus::Running,
                None,
                None,
            )
            .await?
        {
            tracing::debug!(
                run_id = %run.id,
                conversation_id = %conversation_id,
                "Skipped automation plan approval delivery because run status changed"
            );
            return Ok(());
        }

        let delivery = match bridge_session_id.as_ref() {
            Some(session_id) => {
                self.resumer
                    .resume_ideation_with_prompt(
                        session_id,
                        &ideation_bridge_delivery_prompt(approval),
                    )
                    .await
            }
            None => {
                self.resumer
                    .resume_with_prompt(conversation_id, &approval_delivery_prompt(approval))
                    .await
            }
        };
        match delivery {
            Ok(ResumeDelivery::Delivered) => {}
            Ok(ResumeDelivery::QueuedAndPurged) => {
                self.run_repo
                    .set_agent_phase_started_at(&run.id, Some(Utc::now()))
                    .await?;
            }
            Err(error) => {
                self.fail_running_run(
                    run,
                    PLAN_RESUME_FAILED_ERROR_CODE,
                    &format!("Automation plan approval delivery failed: {error}"),
                    summary,
                )
                .await?;
            }
        }
        Ok(())
    }

    async fn pause_ideation_bridge_for_missing_session(
        &self,
        automation: &Automation,
        detail: &str,
        summary: &mut AutomationSchedulerTickSummary,
    ) -> AppResult<()> {
        if self
            .transition_service
            .transition_automation_status(
                &automation.id,
                AutomationStatus::Active,
                AutomationStatus::Paused,
                Some("ideation_bridge_missing_session".to_string()),
                Some(detail.to_string()),
            )
            .await?
        {
            summary.paused_automations += 1;
        }
        Ok(())
    }

    async fn deliver_plan_revision(
        &self,
        run: &AutomationRun,
        conversation_id: &ChatConversationId,
        instructions: &str,
        summary: &mut AutomationSchedulerTickSummary,
    ) -> AppResult<()> {
        if self.resumer.is_agent_running(conversation_id).await? {
            return Ok(());
        }
        if self.resumer.launches_paused().await? {
            return Ok(());
        }

        if !self
            .transition_service
            .transition_run_status_clearing_plan_pending_instructions(
                &run.id,
                AutomationRunStatus::AwaitingPlanApproval,
                AutomationRunStatus::Running,
                None,
                None,
            )
            .await?
        {
            tracing::debug!(
                run_id = %run.id,
                conversation_id = %conversation_id,
                "Skipped automation plan revision delivery because run status changed"
            );
            return Ok(());
        }

        self.transition_service
            .transition_plan_judge_state(
                &run.id,
                AutomationPlanJudgeState::Done,
                AutomationPlanJudgeState::None,
                None,
                None,
            )
            .await?;

        let prompt = revision_delivery_prompt(instructions);
        match self
            .resumer
            .resume_with_prompt(conversation_id, &prompt)
            .await
        {
            Ok(ResumeDelivery::Delivered) => {}
            Ok(ResumeDelivery::QueuedAndPurged) => {
                self.run_repo
                    .set_plan_pending_instructions(&run.id, Some(instructions.to_string()))
                    .await?;
                if !self
                    .transition_service
                    .transition_run_status(
                        &run.id,
                        AutomationRunStatus::Running,
                        AutomationRunStatus::AwaitingPlanApproval,
                        None,
                        None,
                    )
                    .await?
                {
                    tracing::warn!(
                        run_id = %run.id,
                        conversation_id = %conversation_id,
                        "Failed to restore automation plan revision after queued delivery"
                    );
                }
            }
            Err(error) => {
                self.fail_running_run(
                    run,
                    PLAN_RESUME_FAILED_ERROR_CODE,
                    &format!("Automation plan revision delivery failed: {error}"),
                    summary,
                )
                .await?;
            }
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
        } else {
            tracing::warn!(
                run_id = %run.id,
                from_status = run.status.as_str(),
                error_code = code,
                error_detail = detail,
                "Discarded automation run failure because run status changed"
            );
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
        self.ensure_first_merged_run_integration_pr(automation, run.run_index, run.status)
            .await?;

        match run.judge_state {
            AutomationJudgeState::None | AutomationJudgeState::Failed => {
                let from = run.judge_state;
                let judge_lease_expires_at =
                    automation_judge_lease_expires_at(self.config.judge_timeout);
                if self
                    .transition_service
                    .transition_judge_state(
                        &run.id,
                        from,
                        AutomationJudgeState::InProgress,
                        AutomationJudgeTransitionGuard::Dispatch,
                        None,
                        None,
                        Some(judge_lease_expires_at),
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
                        judge_lease_expires_at,
                    );
                } else {
                    tracing::warn!(
                        automation_id = %automation.id,
                        run_id = %run.id,
                        from_judge_state = from.as_str(),
                        "Discarded automation judge start because judge state changed"
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
                if let Some(judge_lease_expires_at) = run.judge_lease_expires_at {
                    self.mark_judge_failed(
                        automation,
                        run,
                        judge_lease_expires_at,
                        "Automation judge exceeded judge_timeout_secs".to_string(),
                        summary,
                    )
                    .await?;
                } else {
                    self.mark_legacy_null_lease_judge_failed(
                        automation,
                        run,
                        "Automation judge exceeded judge_timeout_secs with legacy null lease"
                            .to_string(),
                        summary,
                    )
                    .await?;
                }
            }
            AutomationJudgeState::InProgress | AutomationJudgeState::Skipped => {}
        }
        Ok(())
    }

    async fn ensure_first_merged_run_integration_pr(
        &self,
        automation: &Automation,
        run_index: i64,
        run_status: AutomationRunStatus,
    ) -> AppResult<()> {
        if first_merged_run_requires_integration_pr(automation, run_index, run_status) {
            self.integration_pr_publisher
                .ensure_integration_pr(automation)
                .await?;
        }
        Ok(())
    }

    async fn mark_judge_failed(
        &self,
        automation: &Automation,
        run: &AutomationRun,
        judge_lease_expires_at: DateTime<Utc>,
        detail: String,
        summary: &mut AutomationSchedulerTickSummary,
    ) -> AppResult<()> {
        if self
            .transition_service
            .transition_judge_state(
                &run.id,
                AutomationJudgeState::InProgress,
                AutomationJudgeState::Failed,
                AutomationJudgeTransitionGuard::Settle(judge_lease_expires_at),
                None,
                None,
                None,
                Some(detail.clone()),
            )
            .await?
        {
            summary.judge_failures += 1;
            self.pause_automation_after_judge_failed(automation, detail, summary)
                .await?;
        } else {
            tracing::warn!(
                automation_id = %automation.id,
                run_id = %run.id,
                lease_expires_at = %judge_lease_expires_at.to_rfc3339(),
                "Discarded automation judge failure because judge state or lease changed"
            );
        }
        Ok(())
    }

    async fn mark_legacy_null_lease_judge_failed(
        &self,
        automation: &Automation,
        run: &AutomationRun,
        detail: String,
        summary: &mut AutomationSchedulerTickSummary,
    ) -> AppResult<()> {
        tracing::warn!(
            automation_id = %automation.id,
            run_id = %run.id,
            "Timing out legacy automation judge with null lease token"
        );
        if self
            .transition_service
            .transition_judge_state(
                &run.id,
                AutomationJudgeState::InProgress,
                AutomationJudgeState::Failed,
                AutomationJudgeTransitionGuard::LegacyNullLease,
                None,
                None,
                None,
                Some(detail.clone()),
            )
            .await?
        {
            summary.judge_failures += 1;
            self.pause_automation_after_judge_failed(automation, detail, summary)
                .await?;
        } else {
            tracing::warn!(
                automation_id = %automation.id,
                run_id = %run.id,
                "Discarded legacy automation judge failure because judge state or lease changed"
            );
        }
        Ok(())
    }

    async fn pause_automation_after_judge_failed(
        &self,
        automation: &Automation,
        detail: String,
        summary: &mut AutomationSchedulerTickSummary,
    ) -> AppResult<()> {
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
            self.service
                .sync_goal_items_for_closed_run_without_successor(&automation.id)
                .await;
        }
        Ok(())
    }

    async fn mark_plan_judge_failed(
        &self,
        automation: &Automation,
        run: &AutomationRun,
        detail: String,
        summary: &mut AutomationSchedulerTickSummary,
    ) -> AppResult<()> {
        if self
            .transition_service
            .transition_plan_judge_state(
                &run.id,
                AutomationPlanJudgeState::InProgress,
                AutomationPlanJudgeState::Failed,
                None,
                None,
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
                    Some(PLAN_JUDGE_FAILED_PAUSED_REASON_CODE.to_string()),
                    Some(detail),
                )
                .await?
            {
                summary.paused_automations += 1;
            }
        }
        Ok(())
    }

    async fn mark_corrupt_stored_plan_judge_verdict_failed(
        &self,
        automation: &Automation,
        run: &AutomationRun,
        detail: String,
        summary: &mut AutomationSchedulerTickSummary,
    ) -> AppResult<()> {
        if self
            .transition_service
            .transition_plan_judge_state(
                &run.id,
                AutomationPlanJudgeState::Done,
                AutomationPlanJudgeState::Failed,
                None,
                None,
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
                    Some(PLAN_JUDGE_FAILED_PAUSED_REASON_CODE.to_string()),
                    Some(detail),
                )
                .await?
            {
                summary.paused_automations += 1;
            }
        }
        Ok(())
    }

    async fn pause_automation_for_plan_revision_exhaustion(
        &self,
        automation: &Automation,
        detail: String,
        summary: &mut AutomationSchedulerTickSummary,
    ) -> AppResult<()> {
        if self
            .transition_service
            .transition_automation_status(
                &automation.id,
                AutomationStatus::Active,
                AutomationStatus::Paused,
                Some(PLAN_REVISION_EXHAUSTED_PAUSED_REASON_CODE.to_string()),
                Some(detail),
            )
            .await?
        {
            summary.paused_automations += 1;
        }
        Ok(())
    }

    async fn apply_stored_judge_verdict(
        &self,
        automation: &Automation,
        run: &AutomationRun,
        summary: &mut AutomationSchedulerTickSummary,
    ) -> AppResult<()> {
        let outcome = self
            .service
            .apply_stored_judge_verdict(&automation.id, &run.id)
            .await?;
        self.record_judge_apply_outcome(automation, run, outcome, summary);
        Ok(())
    }

    fn record_judge_apply_outcome(
        &self,
        automation: &Automation,
        run: &AutomationRun,
        outcome: crate::application::automation::service::AutomationJudgeApplyOutcome,
        summary: &mut AutomationSchedulerTickSummary,
    ) {
        if outcome.successor_run.is_some() {
            summary.successor_runs += 1;
        }
        if let Some(noop_reason) = outcome.noop_reason {
            tracing::warn!(
                automation_id = %automation.id,
                run_id = %run.id,
                noop_reason = ?noop_reason,
                reason = ?outcome.reason,
                "Discarded stored automation judge verdict"
            );
        }
        match outcome.terminal_automation_status {
            Some(AutomationStatus::Paused) => summary.paused_automations += 1,
            Some(AutomationStatus::Completed) => summary.completed_automations += 1,
            _ => {}
        }
    }

    async fn apply_stored_plan_judge_verdict(
        &self,
        automation: &Automation,
        run: &AutomationRun,
        _summary: &mut AutomationSchedulerTickSummary,
    ) -> AppResult<()> {
        let Some(verdict_json) = run.plan_judge_verdict_json.as_deref() else {
            return Ok(());
        };
        let verdict = match parse_automation_plan_judge_verdict(
            verdict_json,
            AutomationPlanJudgeValidationContext {
                expected_overview_artifact_id: None,
                expected_blueprint_artifact_id: None,
                blueprint_truncation_blocks_approval: false,
            },
        ) {
            Ok(verdict) => verdict,
            Err(error) => {
                self.mark_corrupt_stored_plan_judge_verdict_failed(
                    automation,
                    run,
                    format!("Automation stored plan judge verdict is invalid: {error}"),
                    _summary,
                )
                .await?;
                return Ok(());
            }
        };
        let Some(current) = self.current_plan_application_context(run).await? else {
            return Ok(());
        };
        if current.overview_artifact_id != verdict.evaluated_overview_artifact_id
            || current.blueprint_artifact_id != verdict.evaluated_blueprint_artifact_id
        {
            tracing::warn!(
                automation_id = %automation.id,
                run_id = %run.id,
                evaluated_overview_artifact_id = verdict.evaluated_overview_artifact_id,
                evaluated_blueprint_artifact_id = ?verdict.evaluated_blueprint_artifact_id,
                current_overview_artifact_id = current.overview_artifact_id,
                current_blueprint_artifact_id = ?current.blueprint_artifact_id,
                "Ignoring stored automation plan judge verdict because the plan bundle changed"
            );
            self.transition_service
                .transition_plan_judge_state(
                    &run.id,
                    AutomationPlanJudgeState::Done,
                    AutomationPlanJudgeState::None,
                    None,
                    None,
                )
                .await?;
            self.transition_service
                .clear_plan_judge_verdict(&run.id)
                .await?;
            return Ok(());
        }

        match verdict.decision {
            AutomationPlanJudgeDecision::Approve => {
                if self
                    .plan_approval_repo
                    .get_by_session(&current.session_id)
                    .await?
                    .is_some_and(|approval| current.matches_approval(&approval))
                {
                    return Ok(());
                }
                self.plan_approval_writer
                    .approve_current_plan_artifact(
                        current.session_id.clone(),
                        Some(current.overview_artifact_id.clone()),
                        PlanApprovalActor::Judge,
                    )
                    .await?;
            }
            AutomationPlanJudgeDecision::Revise => {
                if self
                    .plan_approval_repo
                    .get_by_session(&current.session_id)
                    .await?
                    .is_some_and(|approval| current.matches_approval(&approval))
                {
                    return Ok(());
                }
                let Some(instructions) = verdict.revision_instructions.as_deref() else {
                    return Ok(());
                };
                if run.plan_pending_instructions.as_deref() == Some(instructions) {
                    return Ok(());
                }
                self.run_repo
                    .set_plan_pending_instructions(&run.id, Some(instructions.to_string()))
                    .await?;
            }
        }
        Ok(())
    }

    async fn current_plan_application_context(
        &self,
        run: &AutomationRun,
    ) -> AppResult<Option<PlanApplicationContext>> {
        let Some(conversation_id) = run.conversation_id.as_ref() else {
            return Ok(None);
        };
        let Some(workspace) = self
            .workspace_repo
            .get_by_conversation_id(conversation_id)
            .await?
        else {
            return Ok(None);
        };
        let Some(session_id) = workspace.linked_ideation_session_id.as_ref() else {
            return Ok(None);
        };
        let Some(session) = self.ideation_session_repo.get_by_id(session_id).await? else {
            return Ok(None);
        };
        let Some(bundle) = session.plan_artifact_bundle() else {
            return Ok(None);
        };
        Ok(Some(PlanApplicationContext {
            session_id: session_id.clone(),
            overview_artifact_id: bundle.overview_id.as_str().to_string(),
            blueprint_artifact_id: bundle.blueprint_id.map(|id| id.as_str().to_string()),
        }))
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

        if automation.pr_merge_mode == AutomationPrMergeMode::Automatic
            && !workspace.pr_auto_merge_desired
        {
            self.enable_run_auto_merge_preference_for_run(&run.id, &workspace)
                .await;
        }

        self.sync_auto_merge_enable_warning_from_workspace(automation, run, &workspace)
            .await?;

        match self
            .check_pr_status_with_transient_retry(&workspace, pr_number)
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
                    .transition_run_status_with_merge_metadata(
                        &run.id,
                        AutomationRunStatus::Published,
                        AutomationRunStatus::Merged,
                        merge_commit_sha,
                        pr_merged_at,
                    )
                    .await?
                {
                    summary.merged_runs += 1;
                    self.ensure_first_merged_run_integration_pr(
                        automation,
                        run.run_index,
                        AutomationRunStatus::Merged,
                    )
                    .await?;
                    self.finalize_merged_run_conversation(automation, run).await;
                } else {
                    tracing::warn!(
                        automation_id = %automation.id,
                        run_id = %run.id,
                        pr_number,
                        "Discarded automation merged transition because run status changed"
                    );
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
                } else {
                    tracing::warn!(
                        automation_id = %automation.id,
                        run_id = %run.id,
                        pr_number,
                        "Discarded automation PR-closed transition because run status changed"
                    );
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

    async fn check_pr_status_with_transient_retry(
        &self,
        workspace: &AgentConversationWorkspace,
        pr_number: i64,
    ) -> AppResult<PrStatus> {
        let first = self
            .signal_checker
            .check_pr_status(workspace, pr_number)
            .await;
        if !matches!(first, Err(AppError::Infrastructure(_))) {
            return first;
        }
        tracing::warn!(
            conversation_id = %workspace.conversation_id,
            pr_number,
            "Retrying transient automation PR signal check failure"
        );
        tokio::task::yield_now().await;
        self.signal_checker
            .check_pr_status(workspace, pr_number)
            .await
    }

    async fn enable_run_auto_merge_preference(
        &self,
        workspace: &AgentConversationWorkspace,
    ) -> AppResult<()> {
        self.workspace_repo
            .update_pr_supervision_preferences(
                &workspace.conversation_id,
                workspace.pr_autofix_enabled,
                true,
                DEFAULT_AGENT_WORKSPACE_PR_AUTO_MERGE_METHOD,
            )
            .await?;
        Ok(())
    }

    async fn enable_run_auto_merge_preference_for_run(
        &self,
        run_id: &crate::domain::entities::AutomationRunId,
        workspace: &AgentConversationWorkspace,
    ) {
        if let Err(error) = self.enable_run_auto_merge_preference(workspace).await {
            tracing::warn!(
                run_id = run_id.as_str(),
                conversation_id = workspace.conversation_id.as_str(),
                error = %error,
                "Automation scheduler could not arm automatic PR auto-merge preference; continuing publication"
            );
        }
    }

    async fn finalize_merged_run_conversation(
        &self,
        automation: &Automation,
        run: &AutomationRun,
    ) -> bool {
        let Some(conversation_id) = run.conversation_id.as_ref() else {
            return true;
        };
        match self
            .merged_run_finalizer
            .finalize_merged_conversation(conversation_id)
            .await
        {
            Ok(()) => true,
            Err(error) => {
                tracing::warn!(
                    automation_id = %automation.id,
                    run_id = %run.id,
                    conversation_id = conversation_id.as_str(),
                    error = %error,
                    "Automation merged-run cleanup/archive remains pending"
                );
                false
            }
        }
    }

    async fn sync_auto_merge_enable_warning_from_workspace(
        &self,
        automation: &Automation,
        run: &AutomationRun,
        workspace: &AgentConversationWorkspace,
    ) -> AppResult<()> {
        if let Some(detail) = auto_merge_enable_warning_from_workspace(workspace) {
            if run
                .error_code
                .as_deref()
                .is_some_and(|code| code != AUTO_MERGE_ENABLE_WARNING_CODE)
            {
                return Ok(());
            }
            if run.error_code.as_deref() == Some(AUTO_MERGE_ENABLE_WARNING_CODE)
                && run.error_detail.as_deref() == Some(detail.as_str())
            {
                return Ok(());
            }
            self.run_repo
                .update_published_run_error(
                    &run.id,
                    Some(AUTO_MERGE_ENABLE_WARNING_CODE.to_string()),
                    Some(detail.clone()),
                )
                .await?;
            self.transition_service
                .record_auto_merge_enable_warning(automation, run, &detail)
                .await;
        } else if run.error_code.as_deref() == Some(AUTO_MERGE_ENABLE_WARNING_CODE) {
            self.run_repo
                .update_published_run_error(&run.id, None, None)
                .await?;
        }
        Ok(())
    }
}

fn auto_merge_enable_warning_from_workspace(
    workspace: &AgentConversationWorkspace,
) -> Option<String> {
    if !workspace.pr_auto_merge_desired {
        return None;
    }
    if workspace.pr_auto_merge_current != Some(false) {
        return None;
    }
    if workspace.pr_supervision_status.as_deref() != Some(AUTO_MERGE_SUPERVISION_STATUS_WAITING) {
        return None;
    }
    let summary = workspace.pr_supervision_summary.as_deref()?.trim();
    if !summary.contains(AUTO_MERGE_ENABLE_FAILURE_SUMMARY_PREFIX) {
        return None;
    }
    Some(summary.to_string())
}

fn run_could_need_goal_item_sweep(
    automation: &Automation,
    latest_run: &AutomationRun,
    allow_forward_fill: bool,
) -> AppResult<bool> {
    let plan_gate_paused = automation.status == AutomationStatus::Paused
        && is_plan_gate_pause_reason(automation.paused_reason_code.as_deref());
    if !plan_gate_paused && !latest_run_holds_goal_authority(latest_run) {
        return revert_in_progress_goal_items_to_pending(automation.goal_items_json.as_deref())
            .map(|repair| repair.is_some());
    }
    if allow_forward_fill
        && automation.status == AutomationStatus::Active
        && matches!(
            latest_run.status,
            AutomationRunStatus::Running
                | AutomationRunStatus::AwaitingPlanApproval
                | AutomationRunStatus::Published
        )
    {
        return mark_current_goal_item_in_progress(automation.goal_items_json.as_deref())
            .map(|repair| repair.is_some());
    }
    Ok(false)
}

pub(crate) fn spawn_automation_judge_task(
    service: AutomationService,
    transition_service: AutomationTransitionService,
    judge_invoker: Arc<dyn AutomationJudgeInvoker>,
    config: AutomationSchedulerConfig,
    automation: Automation,
    runs: Vec<AutomationRun>,
    run: AutomationRun,
    judge_lease_expires_at: DateTime<Utc>,
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
        match task
            .run_for_terminal_run(automation, runs, run, judge_lease_expires_at)
            .await
        {
            Ok(outcome) => {
                tracing::info!(
                    automation_id = %automation_id,
                    run_id = %run_id,
                    judge_succeeded = outcome.judge_succeeded,
                    judge_failed = outcome.judge_failed,
                    successor_created = outcome.successor_created,
                    terminal_status = ?outcome.terminal_automation_status,
                    discard_reason = ?outcome.discard_reason,
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

#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_automation_plan_judge_task(
    transition_service: AutomationTransitionService,
    run_repo: Arc<dyn AutomationRunRepository>,
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    ideation_session_repo: Arc<dyn IdeationSessionRepository>,
    plan_approval_repo: Arc<dyn PlanArtifactApprovalRepository>,
    plan_approval_writer: Arc<dyn PlanArtifactApprovalWriter>,
    artifact_repo: Arc<dyn ArtifactRepository>,
    plan_judge_invoker: Arc<dyn AutomationPlanJudgeInvoker>,
    config: AutomationSchedulerConfig,
    automation: Automation,
    run: AutomationRun,
    verification_context: Option<AutomationPlanVerificationJudgeContext>,
) {
    let task = AutomationPlanJudgeTask {
        transition_service,
        run_repo,
        workspace_repo,
        ideation_session_repo,
        plan_approval_repo,
        plan_approval_writer,
        artifact_repo,
        plan_judge_invoker,
        config,
        verification_context,
    };
    tauri::async_runtime::spawn(async move {
        let automation_id = automation.id.clone();
        let run_id = run.id.clone();
        match task.run_for_parked_run(automation, run).await {
            Ok(outcome) => {
                tracing::info!(
                    automation_id = %automation_id,
                    run_id = %run_id,
                    judge_succeeded = outcome.judge_succeeded,
                    judge_failed = outcome.judge_failed,
                    paused_automation = outcome.paused_automation,
                    "Automation plan judge task completed"
                );
            }
            Err(error) => {
                tracing::warn!(
                    automation_id = %automation_id,
                    run_id = %run_id,
                    error = %error,
                    "Automation plan judge task failed"
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

struct ParsedPlanJudgeInvocation {
    verdict: AutomationPlanJudgeVerdict,
    verdict_json: String,
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

fn running_run_has_exceeded(run: &AutomationRun, limit: Duration) -> bool {
    let started_at = run
        .agent_phase_started_at
        .unwrap_or_else(|| run.started_at.unwrap_or(run.created_at));
    elapsed_since(started_at).is_some_and(|elapsed| elapsed >= limit)
}

/// Publication push statuses representing a *settled, pre-publication* workspace
/// where no auto-publish operation is in flight. In these states a genuinely dead
/// current agent (`Failed`/`Cancelled`) can never open a pull request, so the
/// scheduler may fail the run promptly instead of waiting for the
/// `max_run_duration` backstop hours later.
///
/// `refreshed` means only that the workspace base was updated (base freshness); it
/// does NOT indicate an opened or in-flight publication. `None` means no publish has
/// started. Every other status (`checking`/`describing`/`pushing`/`pushed`, a
/// `needs_agent` repair within grace, or any unrecognized/future status) may be
/// racing an in-flight publish and is deliberately left to its own arm or hands-off.
fn publication_push_status_is_settled_pre_publication(status: Option<&str>) -> bool {
    matches!(status, None | Some("refreshed"))
}

fn agent_run_is_current_for_phase(run: &AutomationRun, agent_run: &AgentRun) -> bool {
    run.agent_phase_started_at
        .is_none_or(|phase_started_at| agent_run.started_at >= phase_started_at)
}

fn agent_run_is_system_cancelled(agent_run: &AgentRun) -> bool {
    let reason = agent_run.error_message.as_deref();
    agent_run.status == AgentRunStatus::Cancelled
        && (reason == Some(crate::domain::repositories::ORPHANED_AGENT_RUN_ON_APP_RESTART)
            || reason == Some(crate::domain::repositories::PRUNED_STALE_AGENT_RUN))
}

fn first_merged_run_requires_integration_pr(
    automation: &Automation,
    run_index: i64,
    run_status: AutomationRunStatus,
) -> bool {
    run_index == 1
        && run_status == AutomationRunStatus::Merged
        && automation.setup_conversation_id.is_some()
}

fn judge_has_exceeded(run: &AutomationRun, limit: Duration) -> bool {
    if let Some(expires_at) = run.judge_lease_expires_at {
        return Utc::now() >= expires_at;
    }
    elapsed_since(run.updated_at).is_some_and(|elapsed| elapsed >= limit)
}

fn plan_judge_has_exceeded(run: &AutomationRun, limit: Duration) -> bool {
    if let Some(expires_at) = run.plan_judge_lease_expires_at {
        return Utc::now() >= expires_at;
    }
    elapsed_since(run.updated_at).is_some_and(|elapsed| elapsed >= limit)
}

fn prior_revision_instruction_repeats(
    prior_verdict_json: Option<&str>,
    current_overview_artifact_id: &str,
    current_blueprint_artifact_id: Option<&str>,
    next_instructions: &str,
) -> bool {
    let Some(prior_verdict_json) = prior_verdict_json else {
        return false;
    };
    let Ok(prior) = parse_automation_plan_judge_verdict(
        prior_verdict_json,
        AutomationPlanJudgeValidationContext {
            expected_overview_artifact_id: None,
            expected_blueprint_artifact_id: None,
            blueprint_truncation_blocks_approval: false,
        },
    ) else {
        return false;
    };
    let Some(prior_instructions) = prior.revision_instructions.as_deref() else {
        return false;
    };
    if prior.evaluated_overview_artifact_id != current_overview_artifact_id
        || prior.evaluated_blueprint_artifact_id.as_deref() != current_blueprint_artifact_id
    {
        return false;
    }
    crate::application::automation::judge::normalized_prompt_fingerprint(prior_instructions)
        == crate::application::automation::judge::normalized_prompt_fingerprint(next_instructions)
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
