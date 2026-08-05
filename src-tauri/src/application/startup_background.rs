use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use async_trait::async_trait;

use crate::application::agent_conversation_mode_switch::{
    system_switch_automation_run_to_edit, system_switch_automation_run_to_ideation,
};
use crate::application::agent_conversation_start_service::{
    AgentConversationStartDeps, AgentConversationStartService, StartAgentConversationInput,
};
use crate::application::agent_workspace_bridge::{
    dispatch_agent_workspace_bridge_events_once_with_deps, AgentWorkspaceBridgeDeps,
};
use crate::application::automation::integration_pr::GithubAutomationIntegrationPrPublisher;
use crate::application::automation::merged_run_finalizer::AppStateAutomationMergedRunFinalizer;
use crate::application::automation::plan_gate::{
    AutomationPlanVerificationStartOutcome, AutomationPlanVerificationStartRequest,
    AutomationPlanVerificationStarter, AutomationRunResumer, ResumeDelivery,
};
use crate::application::automation::provisioning::{
    AutomationRunStartOutcome, AutomationRunStartRequest, AutomationRunStarter,
};
use crate::application::automation::scheduler::{
    global_automation_scheduler_registry, AutomationScheduler, AutomationSchedulerConfig,
    GithubAutomationSignalChecker, HarnessAutomationJudgeInvoker,
    HarnessAutomationPlanJudgeInvoker,
};
use crate::application::automation::transition::TauriAutomationEventEmitter;
use crate::application::chat_service::{
    ChatService, ChatServiceError, SendCallerContext, SendMessageOptions,
};
use crate::application::harness_runtime_registry::resolve_default_external_mcp_bootstrap;
use crate::application::plan_artifact_approval::DbPlanArtifactApprovalWriter;
use crate::application::plan_verification_service::{
    get_plan_verification_status, request_plan_verification, PlanVerificationRequestOutcome,
    PlanVerificationRequestSource, PlanVerificationStatusKind,
};
use crate::application::runtime_factory::{build_chat_service_from_deps, ChatRuntimeFactoryDeps};
use crate::application::AppState;
use crate::commands::ExecutionState;
use crate::domain::entities::{ChatContextType, ChatConversationId, VerificationStatus};
use crate::domain::repositories::{
    ExternalEventsRepository, MemoryArchiveRepository, MemoryEntryRepository, ProjectRepository,
    TaskRepository,
};
use crate::error::{AppError, AppResult};
use crate::infrastructure::sqlite::SqlitePlanArtifactApprovalRepository;
use crate::infrastructure::{ExternalMcpHandle, ExternalMcpSupervisor};
use crate::utils::backend_endpoint::backend_http_port;
use tauri::Manager;
use tokio::time::MissedTickBehavior;
use tracing::{info, warn};

const AGENT_WORKSPACE_BRIDGE_DISPATCH_INTERVAL: Duration = Duration::from_secs(5);

static STARTUP_SERVICE_REGISTRY: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();

pub(crate) fn try_start_recurring_service(service: &'static str) -> bool {
    STARTUP_SERVICE_REGISTRY
        .get_or_init(|| Mutex::new(HashSet::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(service)
}

pub(crate) fn external_mcp_startup_timeout(
    config: &crate::infrastructure::agents::claude::ExternalMcpConfig,
) -> Duration {
    Duration::from_secs(config.startup_timeout_secs)
}

pub struct AgentConversationAutomationRunStarter<R: tauri::Runtime + 'static> {
    state: AppState,
    execution_state: Arc<ExecutionState>,
    app_handle: tauri::AppHandle<R>,
}

impl<R: tauri::Runtime + 'static> AgentConversationAutomationRunStarter<R> {
    pub fn new(
        state: AppState,
        execution_state: Arc<ExecutionState>,
        app_handle: tauri::AppHandle<R>,
    ) -> Self {
        Self {
            state,
            execution_state,
            app_handle,
        }
    }
}

#[cfg(test)]
pub(crate) fn automation_run_starter_for_test(
    state: AppState,
) -> AgentConversationAutomationRunStarter<tauri::test::MockRuntime> {
    AgentConversationAutomationRunStarter::new(
        state,
        Arc::new(ExecutionState::new()),
        crate::testing::create_mock_app_handle(),
    )
}

#[async_trait]
impl<R: tauri::Runtime + 'static> AutomationRunStarter
    for AgentConversationAutomationRunStarter<R>
{
    async fn start_run(
        &self,
        request: AutomationRunStartRequest,
    ) -> crate::error::AppResult<AutomationRunStartOutcome> {
        let start_input = request.into_start_input()?;
        let result = AgentConversationStartService::new(AgentConversationStartDeps {
            state: &self.state,
            execution_state: &self.execution_state,
            app_handle: self.app_handle.clone(),
        })
        .start(start_input)
        .await
        .map_err(crate::error::AppError::Agent)?;

        Ok(AutomationRunStartOutcome {
            branch_name: result.workspace.map(|workspace| workspace.branch_name),
        })
    }
}

pub struct AgentConversationAutomationRunResumer<R: tauri::Runtime + 'static> {
    state: AppState,
    execution_state: Arc<ExecutionState>,
    app_handle: tauri::AppHandle<R>,
}

impl<R: tauri::Runtime + 'static> AgentConversationAutomationRunResumer<R> {
    pub fn new(
        state: AppState,
        execution_state: Arc<ExecutionState>,
        app_handle: tauri::AppHandle<R>,
    ) -> Self {
        Self {
            state,
            execution_state,
            app_handle,
        }
    }

    fn chat_service(&self) -> crate::application::AppChatService<R> {
        let chat_deps = ChatRuntimeFactoryDeps::from_app_state(&self.state);
        build_chat_service_from_deps(
            Some(self.app_handle.clone()),
            Some(Arc::clone(&self.execution_state)),
            &chat_deps,
        )
    }
}

#[cfg(test)]
pub(crate) fn automation_run_resumer_for_test(
    state: AppState,
) -> AgentConversationAutomationRunResumer<tauri::test::MockRuntime> {
    AgentConversationAutomationRunResumer::new(
        state,
        Arc::new(ExecutionState::new()),
        crate::testing::create_mock_app_handle(),
    )
}

#[async_trait]
impl<R: tauri::Runtime + 'static> AutomationRunResumer
    for AgentConversationAutomationRunResumer<R>
{
    async fn is_agent_running(&self, conversation_id: &ChatConversationId) -> AppResult<bool> {
        let context_id = conversation_id.as_str();
        Ok(self
            .chat_service()
            .is_agent_running(ChatContextType::Project, &context_id)
            .await)
    }

    async fn is_ideation_agent_running(
        &self,
        session_id: &crate::domain::entities::IdeationSessionId,
    ) -> AppResult<bool> {
        Ok(self
            .chat_service()
            .is_agent_running(ChatContextType::Ideation, session_id.as_str())
            .await)
    }

    async fn launches_paused(&self) -> AppResult<bool> {
        Ok(self.execution_state.is_paused())
    }

    async fn switch_to_edit(&self, conversation_id: &ChatConversationId) -> AppResult<()> {
        system_switch_automation_run_to_edit(conversation_id, &self.state).await?;
        Ok(())
    }

    async fn switch_to_ideation(&self, conversation_id: &ChatConversationId) -> AppResult<()> {
        system_switch_automation_run_to_ideation(conversation_id, &self.state).await?;
        Ok(())
    }

    async fn resume_with_prompt(
        &self,
        conversation_id: &ChatConversationId,
        prompt: &str,
    ) -> AppResult<ResumeDelivery> {
        let chat_service = self.chat_service();
        resume_automation_run_with_prompt_via_chat_service(
            &self.state,
            &chat_service,
            conversation_id,
            prompt,
        )
        .await
    }

    async fn resume_ideation_with_prompt(
        &self,
        session_id: &crate::domain::entities::IdeationSessionId,
        prompt: &str,
    ) -> AppResult<ResumeDelivery> {
        let result = self
            .chat_service()
            .send_message(
                ChatContextType::Ideation,
                session_id.as_str(),
                prompt,
                SendMessageOptions {
                    caller_context: SendCallerContext::UserInitiated,
                    ..SendMessageOptions::default()
                },
            )
            .await
            .map_err(|error| {
                AppError::Infrastructure(format!("automation ideation bridge send failed: {error}"))
            })?;
        if result.was_queued {
            tracing::info!(
                session_id = %session_id,
                "Automation ideation bridge prompt is waiting for execution capacity"
            );
        }
        Ok(ResumeDelivery::Delivered)
    }
}

pub struct AgentConversationAutomationPlanVerificationStarter {
    state: AppState,
    execution_state: Arc<ExecutionState>,
}

impl AgentConversationAutomationPlanVerificationStarter {
    pub fn new<R: tauri::Runtime + 'static>(
        state: AppState,
        execution_state: Arc<ExecutionState>,
        _app_handle: tauri::AppHandle<R>,
    ) -> Self {
        Self {
            state,
            execution_state,
        }
    }
}

#[async_trait]
impl AutomationPlanVerificationStarter for AgentConversationAutomationPlanVerificationStarter {
    async fn start_verification(
        &self,
        request: AutomationPlanVerificationStartRequest,
    ) -> AppResult<AutomationPlanVerificationStartOutcome> {
        let session_id = request.session_id;
        let session = self
            .state
            .ideation_session_repo
            .get_by_id(&session_id)
            .await?
            .ok_or_else(|| {
                AppError::NotFound(format!(
                    "Planning session {} not found",
                    session_id.as_str()
                ))
            })?;

        if session
            .plan_artifact_id
            .as_ref()
            .is_none_or(|artifact_id| artifact_id.as_str() != request.artifact_id)
        {
            return Ok(AutomationPlanVerificationStartOutcome::Unavailable {
                detail: format!(
                    "current planning session artifact does not match parked artifact {}",
                    request.artifact_id
                ),
            });
        }

        let chat_service = self
            .state
            .build_chat_service_with_execution_state(Arc::clone(&self.execution_state));
        let outcome = request_plan_verification(
            &self.state,
            &chat_service,
            &session_id,
            PlanVerificationRequestSource::Automatic,
        )
        .await?;
        match outcome {
            PlanVerificationRequestOutcome::Queued => {
                Ok(AutomationPlanVerificationStartOutcome::Started { generation: 0 })
            }
            PlanVerificationRequestOutcome::AlreadyQueued
            | PlanVerificationRequestOutcome::AlreadyRunning => {
                Ok(AutomationPlanVerificationStartOutcome::AlreadyInProgress { generation: 0 })
            }
            PlanVerificationRequestOutcome::AlreadyVerified => {
                Ok(AutomationPlanVerificationStartOutcome::AlreadyTerminal {
                    generation: 0,
                    status: VerificationStatus::Verified,
                })
            }
            PlanVerificationRequestOutcome::NoPlan => {
                Ok(AutomationPlanVerificationStartOutcome::Unavailable {
                    detail: "planning session has no linked plan".to_string(),
                })
            }
        }
    }

    async fn verification_status(
        &self,
        request: &AutomationPlanVerificationStartRequest,
    ) -> AppResult<PlanVerificationStatusKind> {
        Ok(
            get_plan_verification_status(&self.state, &request.session_id)
                .await?
                .status,
        )
    }
}

pub(crate) async fn resume_automation_run_with_prompt_via_chat_service<S: ChatService + ?Sized>(
    state: &AppState,
    chat_service: &S,
    conversation_id: &ChatConversationId,
    prompt: &str,
) -> AppResult<ResumeDelivery> {
    let conversation = state
        .chat_conversation_repo
        .get_by_id(conversation_id)
        .await?
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "automation run conversation {} not found",
                conversation_id
            ))
        })?;
    if conversation.context_type != ChatContextType::Project {
        return Err(AppError::Validation(format!(
            "automation run conversation {} is not project-backed",
            conversation_id
        )));
    }

    let runtime_context_id = conversation_id.as_str();
    let result = chat_service
        .send_message(
            ChatContextType::Project,
            &conversation.context_id,
            prompt,
            SendMessageOptions {
                conversation_id_override: Some(conversation_id.clone()),
                caller_context: SendCallerContext::StartupResumption,
                ..SendMessageOptions::default()
            },
        )
        .await
        .map_err(|error| {
            AppError::Infrastructure(format!("automation plan gate send failed: {error}"))
        })?;

    if result.was_queued {
        if let Some(queued_message_id) = result.queued_message_id.as_deref() {
            if let Err(error) = chat_service
                .delete_queued_message(
                    ChatContextType::Project,
                    &runtime_context_id,
                    queued_message_id,
                )
                .await
            {
                warn!(
                    conversation_id = conversation_id.as_str(),
                    queued_message_id,
                    error = %error,
                    "Failed to purge queued automation plan gate prompt"
                );
            }
        }
        return Ok(ResumeDelivery::QueuedAndPurged);
    }

    Ok(ResumeDelivery::Delivered)
}

pub async fn recover_memory_archive_jobs_on_startup(
    memory_archive_repo: Arc<dyn MemoryArchiveRepository>,
    memory_entry_repo: Arc<dyn MemoryEntryRepository>,
    project_repo: Arc<dyn ProjectRepository>,
) {
    info!("Recovering pending memory archive jobs...");
    let archive_service = Arc::new(crate::application::MemoryArchiveService::new(
        Arc::clone(&memory_archive_repo),
        memory_entry_repo,
        project_repo,
    ));

    let recovered_count = match memory_archive_repo.count_claimable().await {
        Ok(count) => {
            info!(pending_jobs = count, "Found memory archive jobs to recover");
            let mut processed = 0;
            while processed < count {
                match archive_service.process_next_job().await {
                    Ok(true) => processed += 1,
                    Ok(false) => break,
                    Err(e) => {
                        tracing::warn!(error = %e, "Failed to process archive job during recovery");
                        break;
                    }
                }
            }
            processed
        }
        Err(e) => {
            tracing::warn!(error = %e, "Failed to count claimable archive jobs");
            0
        }
    };

    if recovered_count > 0 {
        info!(
            recovered = recovered_count,
            "Completed memory archive job recovery"
        );
    }
}

pub fn spawn_watchdog(
    task_scheduler: Arc<dyn crate::domain::state_machine::services::TaskScheduler>,
    task_repo: Arc<dyn TaskRepository>,
    project_repo: Arc<dyn ProjectRepository>,
) {
    if !try_start_recurring_service("ready_watchdog") {
        tracing::debug!("Ready watchdog already started; skipping duplicate spawn");
        return;
    }
    tauri::async_runtime::spawn(async move {
        crate::application::ReadyWatchdog::new(task_scheduler, task_repo, project_repo)
            .run_loop()
            .await;
    });
}

pub fn spawn_automation_scheduler(
    state: AppState,
    execution_state: Arc<ExecutionState>,
    app_handle: tauri::AppHandle,
) {
    let registry = global_automation_scheduler_registry();
    if !registry.try_start_loop() {
        tracing::debug!("Automation scheduler already started; skipping duplicate spawn");
        return;
    }
    let starter = Arc::new(AgentConversationAutomationRunStarter::new(
        state.clone(),
        Arc::clone(&execution_state),
        app_handle.clone(),
    ));
    let resumer = Arc::new(AgentConversationAutomationRunResumer::new(
        state.clone(),
        Arc::clone(&execution_state),
        app_handle.clone(),
    ));
    let signal_checker = Arc::new(GithubAutomationSignalChecker::new(
        state.github_service.clone(),
    ));
    let integration_pr_publisher = Arc::new(GithubAutomationIntegrationPrPublisher::new(
        state.github_service.clone(),
        Arc::clone(&state.chat_conversation_repo),
        Arc::clone(&state.agent_conversation_workspace_repo),
        Arc::clone(&state.project_repo),
    ));
    let judge_invoker = Arc::new(HarnessAutomationJudgeInvoker::new(state.clone()));
    let plan_judge_invoker = Arc::new(HarnessAutomationPlanJudgeInvoker::new(state.clone()));
    let plan_verification_starter =
        Arc::new(AgentConversationAutomationPlanVerificationStarter::new(
            state.clone(),
            Arc::clone(&execution_state),
            app_handle.clone(),
        ));
    let event_emitter = Arc::new(TauriAutomationEventEmitter::new(app_handle.clone()));
    let merged_run_finalizer = Arc::new(AppStateAutomationMergedRunFinalizer::new(state.clone()));

    let scheduler = AutomationScheduler::new(
        Arc::clone(&state.automation_repo),
        Arc::clone(&state.automation_run_repo),
        Arc::clone(&state.agent_run_repo),
        Arc::clone(&state.chat_conversation_repo),
        Arc::clone(&state.agent_conversation_workspace_repo),
        Arc::clone(&state.ideation_session_repo),
        Arc::new(SqlitePlanArtifactApprovalRepository::new(state.db.clone())),
        Arc::new(DbPlanArtifactApprovalWriter::new(state.db.clone())),
        starter,
        resumer,
        signal_checker,
        integration_pr_publisher,
        judge_invoker,
        plan_judge_invoker,
        plan_verification_starter,
        merged_run_finalizer,
        event_emitter,
        Arc::clone(&state.artifact_repo),
        state.notification_service(),
        registry,
        AutomationSchedulerConfig::default(),
    );
    let poll_interval = scheduler.config().poll_interval;

    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(poll_interval);
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            match scheduler.tick_once().await {
                Ok(summary) => {
                    tracing::debug!(
                        total_automations = summary.total_automations,
                        active_automations = summary.active_automations,
                        leased_automations = summary.leased_automations,
                        active_without_runs = summary.active_without_runs,
                        active_with_runs = summary.active_with_runs,
                        provisioned_runs = summary.provisioned_runs,
                        published_runs = summary.published_runs,
                        merged_runs = summary.merged_runs,
                        closed_runs = summary.closed_runs,
                        failed_runs = summary.failed_runs,
                        judges_started = summary.judges_started,
                        judges_succeeded = summary.judges_succeeded,
                        judge_failures = summary.judge_failures,
                        successor_runs = summary.successor_runs,
                        signal_check_errors = summary.signal_check_errors,
                        paused_automations = summary.paused_automations,
                        resumed_automations = summary.resumed_automations,
                        completed_automations = summary.completed_automations,
                        provisioning_errors = summary.provisioning_errors,
                        automation_errors = summary.automation_errors,
                        "Automation scheduler tick completed"
                    );
                }
                Err(error) => {
                    tracing::warn!(error = %error, "Automation scheduler tick failed");
                }
            }
        }
    });
}

pub fn spawn_cleanup_loops(
    external_events_repo: Arc<dyn ExternalEventsRepository>,
    memory_archive_repo: Arc<dyn MemoryArchiveRepository>,
    memory_entry_repo: Arc<dyn MemoryEntryRepository>,
    project_repo: Arc<dyn ProjectRepository>,
) {
    if !try_start_recurring_service("cleanup_loops") {
        tracing::debug!("Cleanup loops already started; skipping duplicate spawn");
        return;
    }
    tauri::async_runtime::spawn(async move {
        crate::application::EventCleanupService::new(external_events_repo)
            .run_loop()
            .await;
    });

    tauri::async_runtime::spawn(async move {
        let archive_service = Arc::new(crate::application::MemoryArchiveService::new(
            memory_archive_repo,
            memory_entry_repo,
            project_repo,
        ));

        let mut backoff_duration = Duration::from_secs(0);
        loop {
            if !backoff_duration.is_zero() {
                tracing::debug!(
                    backoff_secs = backoff_duration.as_secs(),
                    "Memory archive job processor backing off after error"
                );
                tokio::time::sleep(backoff_duration).await;
                backoff_duration = Duration::from_secs(0);
            }

            match archive_service.process_next_job().await {
                Ok(true) => {
                    tracing::debug!("Memory archive job processed, checking for more");
                    backoff_duration = Duration::from_secs(0);
                }
                Ok(false) => {
                    tracing::debug!("No memory archive jobs available, sleeping");
                    tokio::time::sleep(Duration::from_secs(30)).await;
                }
                Err(e) => {
                    tracing::error!(error = %e, "Failed to process memory archive job");
                    backoff_duration = Duration::from_secs(60);
                    tokio::time::sleep(backoff_duration).await;
                }
            }
        }
    });
}

pub(crate) fn spawn_agent_workspace_bridge_dispatcher(
    bridge_deps: AgentWorkspaceBridgeDeps,
    chat_deps: ChatRuntimeFactoryDeps,
    execution_state: Arc<ExecutionState>,
    app_handle: tauri::AppHandle,
) {
    if !try_start_recurring_service("agent_workspace_bridge_dispatcher") {
        tracing::debug!(
            "Agent workspace bridge dispatcher already started; skipping duplicate spawn"
        );
        return;
    }
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(AGENT_WORKSPACE_BRIDGE_DISPATCH_INTERVAL);
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

        loop {
            interval.tick().await;
            let chat_service = build_chat_service_from_deps(
                Some(app_handle.clone()),
                Some(Arc::clone(&execution_state)),
                &chat_deps,
            );
            match dispatch_agent_workspace_bridge_events_once_with_deps(&bridge_deps, &chat_service)
                .await
            {
                Ok(summary) if summary.wake_up_count > 0 || summary.error_count > 0 => {
                    tracing::info!(
                        projects = summary.project_count,
                        workspaces = summary.workspace_count,
                        wakeups = summary.wake_up_count,
                        queued = summary.queued_wake_up_count,
                        errors = summary.error_count,
                        "Agent workspace bridge dispatcher tick completed"
                    );
                }
                Ok(_) => {}
                Err(error) => {
                    tracing::warn!(
                        error = %error,
                        "Agent workspace bridge dispatcher tick failed"
                    );
                }
            }
        }
    });
}

/// How often the host drains one pending remote conversation-start intent.
const REMOTE_CONVERSATION_START_DISPATCH_INTERVAL: Duration = Duration::from_secs(2);
/// `Starting` rows older than this at boot are swept to `FailedStale` and NEVER auto-respawned:
/// a lost race between a dead claim and a re-spawn is a double-conversation factory, so we fail
/// closed and let the user retry explicitly.
const REMOTE_CONVERSATION_START_STALE_LEASE_SECS: i64 = 300;

/// The host-owned driver for spawn-free remote conversation starts (contract §2.3).
///
/// This loop is the SOLE holder of spawn authority for the feature: the registered
/// `request_remote_agent_conversation_start` command only persists an intent, and only this
/// loop ever calls `AgentConversationStartService::start`. Per tick it CAS-claims one `Pending`
/// intent (`Pending -> Starting`), RE-VALIDATES provider/model/project at spawn time (any failure
/// -> `Failed`, never a silent substitution), starts the seeded conversation on the host, and
/// records `Started` + run id or `Failed` + error code.
pub(crate) fn spawn_remote_conversation_start_dispatcher(
    state: AppState,
    execution_state: Arc<ExecutionState>,
    app_handle: tauri::AppHandle,
) {
    if !try_start_recurring_service("remote_conversation_start_dispatcher") {
        tracing::debug!(
            "Remote conversation start dispatcher already started; skipping duplicate spawn"
        );
        return;
    }
    tauri::async_runtime::spawn(async move {
        // Startup reconciliation: crashed `Starting` claims are terminalised, never respawned.
        let now = chrono::Utc::now();
        let stale_cutoff =
            now - chrono::Duration::seconds(REMOTE_CONVERSATION_START_STALE_LEASE_SECS);
        match state
            .remote_conversation_start_request_repo
            .fail_stale_starting_start_requests(stale_cutoff, now)
            .await
        {
            Ok(count) if count > 0 => {
                tracing::warn!(
                    count,
                    "swept stale remote conversation-start claims to FailedStale"
                )
            }
            Ok(_) => {}
            Err(error) => {
                tracing::error!(%error, "remote conversation start stale sweep failed")
            }
        }

        let mut interval = tokio::time::interval(REMOTE_CONVERSATION_START_DISPATCH_INTERVAL);
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            if let Err(error) =
                dispatch_one_remote_conversation_start(&state, &execution_state, &app_handle).await
            {
                tracing::warn!(%error, "remote conversation start dispatcher tick failed");
            }
        }
    });
}

/// Claim + start at most one pending intent. Extracted (and generic over the runtime) so a test
/// can drive one tick with a mock handle and prove the re-validation-failure path never spawns.
pub(crate) async fn dispatch_one_remote_conversation_start<R: tauri::Runtime + 'static>(
    state: &AppState,
    execution_state: &Arc<ExecutionState>,
    app_handle: &tauri::AppHandle<R>,
) -> AppResult<()> {
    let claim_at = chrono::Utc::now();
    let Some(claimed) = state
        .remote_conversation_start_request_repo
        .claim_pending_start_request(claim_at)
        .await?
    else {
        return Ok(());
    };

    // Re-prove authority at spawn time (settings may have changed since persist).
    if let Err(error_code) = revalidate_remote_conversation_start(state, &claimed).await {
        if let Err(error) = state
            .remote_conversation_start_request_repo
            .fail_start_request(&claimed.id, &error_code, chrono::Utc::now())
            .await
        {
            tracing::error!(%error, request_id = %claimed.id, "failed to record remote conversation start revalidation failure");
        }
        return Ok(());
    }

    let input = build_remote_conversation_start_input(&claimed);
    let result = AgentConversationStartService::new(AgentConversationStartDeps {
        state,
        execution_state,
        app_handle: app_handle.clone(),
    })
    .start(input)
    .await;

    match result {
        Ok(started) => {
            let run_id = started.send_result.agent_run_id;
            if let Err(error) = state
                .remote_conversation_start_request_repo
                .complete_start_request(&claimed.id, &run_id, chrono::Utc::now())
                .await
            {
                tracing::error!(%error, request_id = %claimed.id, "failed to record remote conversation start completion");
            }
        }
        Err(error) => {
            // Setup/MCP-preflight/start failures land in `Failed` VISIBLY — they must not
            // masquerade as a hung `Starting`.
            if let Err(persist_error) = state
                .remote_conversation_start_request_repo
                .fail_start_request(
                    &claimed.id,
                    "REMOTE_CONV_START_HOST_START_FAILED",
                    chrono::Utc::now(),
                )
                .await
            {
                tracing::error!(%persist_error, request_id = %claimed.id, "failed to record remote conversation start failure");
            }
            tracing::warn!(%error, request_id = %claimed.id, "remote conversation start failed on host");
        }
    }

    Ok(())
}

/// Build the host start input from the seeded draft conversation, validated mode, and re-validated
/// provider/model/effort. Persona, base/branch, team, and attachments remain host-forced defaults.
fn build_remote_conversation_start_input(
    claimed: &crate::domain::entities::RemoteConversationStartRequest,
) -> StartAgentConversationInput {
    StartAgentConversationInput {
        project_id: Some(claimed.project_id.as_str().to_string()),
        content: claimed.content.clone(),
        persona_id: None,
        source_persona_id: None,
        conversation_id: Some(claimed.conversation_id.as_str()),
        parent_conversation_id: None,
        title: None,
        provider_harness: Some(claimed.provider.clone()),
        model_override: claimed.model.clone(),
        logical_effort: claimed
            .effort
            .as_deref()
            .and_then(|effort| effort.parse::<crate::domain::agents::LogicalEffort>().ok()),
        codex_fast_mode: None,
        mode: Some(claimed.mode.clone()),
        base_ref_kind: None,
        base_branch_mode: None,
        base_ref: None,
        base_display_name: None,
        base_source_pull_request: None,
        composer_project_references: Vec::new(),
        composer_integration_references: Vec::new(),
        composer_artifact_references: Vec::new(),
        composer_selection_snapshot: None,
        team_intent: None,
    }
}

/// Re-validate a claimed intent at spawn time. Returns the client-facing error code on failure.
async fn revalidate_remote_conversation_start(
    state: &AppState,
    claimed: &crate::domain::entities::RemoteConversationStartRequest,
) -> Result<(), String> {
    use crate::commands::remote_conversation_start_commands::{
        REMOTE_CONV_START_LOOKUP_FAILED, REMOTE_CONV_START_MODEL_NOT_ENABLED,
        REMOTE_CONV_START_PROJECT_NOT_FOUND, REMOTE_CONV_START_PROVIDER_NOT_ENABLED,
    };

    let provider = claimed
        .provider
        .parse::<crate::domain::agents::AgentHarnessKind>()
        .map_err(|_| REMOTE_CONV_START_PROVIDER_NOT_ENABLED.to_string())?;

    let stored = state
        .agent_provider_settings_repo
        .list()
        .await
        .map_err(|_| REMOTE_CONV_START_LOOKUP_FAILED.to_string())?;
    if !stored
        .iter()
        .any(|row| row.provider == provider && row.enabled)
    {
        return Err(REMOTE_CONV_START_PROVIDER_NOT_ENABLED.to_string());
    }

    if state
        .project_repo
        .get_by_id(&claimed.project_id)
        .await
        .map_err(|_| REMOTE_CONV_START_LOOKUP_FAILED.to_string())?
        .is_none()
    {
        return Err(REMOTE_CONV_START_PROJECT_NOT_FOUND.to_string());
    }

    if let Some(model_id) = claimed.model.as_deref() {
        let snapshot = crate::commands::agent_model_commands::load_agent_model_registry(state)
            .await
            .map_err(|_| REMOTE_CONV_START_LOOKUP_FAILED.to_string())?;
        if snapshot.find_enabled(provider, model_id).is_none() {
            return Err(REMOTE_CONV_START_MODEL_NOT_ENABLED.to_string());
        }
    }

    Ok(())
}

/// How often the host drains one pending remote conversation-message intent.
const REMOTE_CONVERSATION_MESSAGE_DISPATCH_INTERVAL: Duration = Duration::from_secs(2);
/// `Dispatching` rows older than this at boot are swept to `FailedStale` and NEVER auto-retried:
/// the outcome of a claim lost to a crash is unknown, and a retry would risk delivering the same
/// turn twice. We fail closed and let the user re-send explicitly.
const REMOTE_CONVERSATION_MESSAGE_STALE_LEASE_SECS: i64 = 300;

/// The host-owned driver for spawn-free remote conversation CONTINUATIONS (WP1).
///
/// This loop is the SOLE holder of spawn authority for the feature: the registered
/// `request_remote_agent_conversation_message` command only persists an intent, and only this
/// loop ever builds a `ChatService`. Per tick it CAS-claims one `Pending` intent
/// (`Pending -> Dispatching`), RE-VALIDATES conversation/provider/model AND re-proves that no run
/// went live in the meantime, then sends through `ChatService::send_message`.
///
/// The terminal call is deliberately `send_message` and NOT
/// `AgentConversationStartService::start`: `start` treats the conversation id as a draft and
/// mints a FRESH run, which would abandon the provider session. `send_message` goes through the
/// provider-session resume seam (`chat_service_context.rs`, `claude_resume_session_id` /
/// `--resume`), which is the entire point of "continue an existing conversation".
pub(crate) fn spawn_remote_conversation_message_dispatcher(
    state: AppState,
    execution_state: Arc<ExecutionState>,
    app_handle: tauri::AppHandle,
) {
    if !try_start_recurring_service("remote_conversation_message_dispatcher") {
        tracing::debug!(
            "Remote conversation message dispatcher already started; skipping duplicate spawn"
        );
        return;
    }
    tauri::async_runtime::spawn(async move {
        // Startup reconciliation: crashed `Dispatching` claims are terminalised, never retried.
        let now = chrono::Utc::now();
        let stale_cutoff =
            now - chrono::Duration::seconds(REMOTE_CONVERSATION_MESSAGE_STALE_LEASE_SECS);
        match state
            .remote_conversation_message_request_repo
            .fail_stale_dispatching_message_requests(stale_cutoff, now)
            .await
        {
            Ok(count) if count > 0 => {
                tracing::warn!(
                    count,
                    "swept stale remote conversation-message claims to FailedStale"
                )
            }
            Ok(_) => {}
            Err(error) => {
                tracing::error!(%error, "remote conversation message stale sweep failed")
            }
        }

        let mut interval = tokio::time::interval(REMOTE_CONVERSATION_MESSAGE_DISPATCH_INTERVAL);
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            if let Err(error) =
                dispatch_one_remote_conversation_message(&state, &execution_state, &app_handle)
                    .await
            {
                tracing::warn!(%error, "remote conversation message dispatcher tick failed");
            }
        }
    });
}

/// Claim + send at most one pending intent. Extracted (and generic over the runtime) so a test
/// can drive one tick and prove the re-validation-failure paths never send.
pub(crate) async fn dispatch_one_remote_conversation_message<R: tauri::Runtime + 'static>(
    state: &AppState,
    execution_state: &Arc<ExecutionState>,
    app_handle: &tauri::AppHandle<R>,
) -> AppResult<()> {
    let claim_at = chrono::Utc::now();
    let Some(claimed) = state
        .remote_conversation_message_request_repo
        .claim_pending_message_request(claim_at)
        .await?
    else {
        return Ok(());
    };

    // Re-prove authority at dispatch time (settings and run liveness may have changed since
    // persist). Every failure terminalises the row VISIBLY — a persisted-but-never-delivered
    // turn must never look like a sent message (chat-send design §7).
    let context = match revalidate_remote_conversation_message(state, &claimed).await {
        Ok(context) => context,
        Err(error_code) => {
            if let Err(error) = state
                .remote_conversation_message_request_repo
                .fail_message_request(&claimed.id, &error_code, chrono::Utc::now())
                .await
            {
                tracing::error!(%error, request_id = %claimed.id, "failed to record remote conversation message revalidation failure");
            }
            return Ok(());
        }
    };

    let service = build_chat_service_from_deps(
        Some(app_handle.clone()),
        Some(Arc::clone(execution_state)),
        &ChatRuntimeFactoryDeps::from_app_state(state),
    );

    let result = service
        .send_message(
            ChatContextType::Project,
            claimed.project_id.as_str(),
            &claimed.content,
            SendMessageOptions {
                // The conversation is named explicitly, so the send lands on the row the client
                // named rather than on whatever the project's "active" conversation happens to be.
                conversation_id_override: Some(claimed.conversation_id.clone()),
                harness_override: Some(context.provider),
                model_override: claimed.model_override.clone(),
                logical_effort_override: context.logical_effort,
                ..Default::default()
            },
        )
        .await;

    match result {
        Ok(send_result) => {
            if let Err(error) = state
                .remote_conversation_message_request_repo
                .complete_message_request(
                    &claimed.id,
                    &send_result.agent_run_id,
                    chrono::Utc::now(),
                )
                .await
            {
                tracing::error!(%error, request_id = %claimed.id, "failed to record remote conversation message completion");
            }
        }
        Err(error) => {
            if let Err(persist_error) = state
                .remote_conversation_message_request_repo
                .fail_message_request(
                    &claimed.id,
                    crate::commands::remote_conversation_message_commands::REMOTE_CONV_MESSAGE_HOST_SEND_FAILED,
                    chrono::Utc::now(),
                )
                .await
            {
                tracing::error!(%persist_error, request_id = %claimed.id, "failed to record remote conversation message failure");
            }
            tracing::warn!(%error, request_id = %claimed.id, "remote conversation message send failed on host");
        }
    }

    Ok(())
}

/// What re-validation proved, carried forward so the send does not re-derive it.
pub(crate) struct RemoteConversationMessageDispatchContext {
    pub provider: crate::domain::agents::AgentHarnessKind,
    pub logical_effort: Option<crate::domain::agents::LogicalEffort>,
}

/// Re-validate a claimed continuation intent at dispatch time. Returns the client-facing error
/// code on failure.
pub(crate) async fn revalidate_remote_conversation_message(
    state: &AppState,
    claimed: &crate::domain::entities::RemoteConversationMessageRequest,
) -> Result<RemoteConversationMessageDispatchContext, String> {
    use crate::commands::remote_conversation_message_commands::{
        REMOTE_CONV_MESSAGE_CONVERSATION_ARCHIVED, REMOTE_CONV_MESSAGE_CONVERSATION_NOT_FOUND,
        REMOTE_CONV_MESSAGE_LOOKUP_FAILED, REMOTE_CONV_MESSAGE_MODEL_NOT_ENABLED,
        REMOTE_CONV_MESSAGE_PROJECT_MISMATCH, REMOTE_CONV_MESSAGE_PROVIDER_NOT_ENABLED,
        REMOTE_CONV_MESSAGE_RUN_WENT_LIVE,
    };

    // 1. The conversation must still exist and still be continuable.
    let conversation = state
        .chat_conversation_repo
        .get_by_id(&claimed.conversation_id)
        .await
        .map_err(|_| REMOTE_CONV_MESSAGE_LOOKUP_FAILED.to_string())?
        .ok_or_else(|| REMOTE_CONV_MESSAGE_CONVERSATION_NOT_FOUND.to_string())?;
    if conversation.is_archived() {
        return Err(REMOTE_CONV_MESSAGE_CONVERSATION_ARCHIVED.to_string());
    }
    if conversation.context_type != ChatContextType::Project
        || conversation.context_id != claimed.project_id.as_str()
    {
        return Err(REMOTE_CONV_MESSAGE_PROJECT_MISMATCH.to_string());
    }

    // 2. The project must still exist.
    if state
        .project_repo
        .get_by_id(&claimed.project_id)
        .await
        .map_err(|_| REMOTE_CONV_MESSAGE_LOOKUP_FAILED.to_string())?
        .is_none()
    {
        return Err(REMOTE_CONV_MESSAGE_PROJECT_MISMATCH.to_string());
    }

    // 3. STILL no live run. If one went live between persist and claim, the live queue owns the
    //    turn and a fresh send would double it. Retryable and distinct so the client can tell the
    //    user to re-send through the live path rather than showing an opaque failure.
    if state
        .agent_run_repo
        .get_active_for_conversation(&claimed.conversation_id)
        .await
        .map_err(|_| REMOTE_CONV_MESSAGE_LOOKUP_FAILED.to_string())?
        .is_some()
    {
        return Err(REMOTE_CONV_MESSAGE_RUN_WENT_LIVE.to_string());
    }

    // 4. The recorded provider must still be enabled — never substituted silently.
    let provider = claimed
        .provider
        .parse::<crate::domain::agents::AgentHarnessKind>()
        .map_err(|_| REMOTE_CONV_MESSAGE_PROVIDER_NOT_ENABLED.to_string())?;
    let stored = state
        .agent_provider_settings_repo
        .list()
        .await
        .map_err(|_| REMOTE_CONV_MESSAGE_LOOKUP_FAILED.to_string())?;
    if !stored
        .iter()
        .any(|row| row.provider == provider && row.enabled)
    {
        return Err(REMOTE_CONV_MESSAGE_PROVIDER_NOT_ENABLED.to_string());
    }

    // 5. A named model must still be enabled for that provider.
    if let Some(model_id) = claimed.model_override.as_deref() {
        let snapshot = crate::commands::agent_model_commands::load_agent_model_registry(state)
            .await
            .map_err(|_| REMOTE_CONV_MESSAGE_LOOKUP_FAILED.to_string())?;
        if snapshot.find_enabled(provider, model_id).is_none() {
            return Err(REMOTE_CONV_MESSAGE_MODEL_NOT_ENABLED.to_string());
        }
    }

    Ok(RemoteConversationMessageDispatchContext {
        provider,
        logical_effort: claimed
            .logical_effort
            .as_deref()
            .and_then(|effort| effort.parse::<crate::domain::agents::LogicalEffort>().ok()),
    })
}

/// How often the host drains one pending remote STOP intent.
///
/// Deliberately much shorter than the conversation-start interval: a brake that takes seconds
/// to bite reads as broken, and the user's next move is to tap Stop again. The tick is a
/// repository read against an indexed status column, so a 1s cadence is cheap.
const REMOTE_AGENT_STOP_DISPATCH_INTERVAL: Duration = Duration::from_secs(1);
/// `Stopping` rows older than this at boot are swept to `FailedStale` and NEVER re-driven: a
/// lost race between a dead claim and a re-drain could terminate a run the user has since
/// restarted, so we fail closed and let the client retry explicitly.
const REMOTE_AGENT_STOP_STALE_LEASE_SECS: i64 = 120;

/// The host-owned driver for spawn-free remote agent stops (WP2).
///
/// This loop is the SOLE holder of the process-terminating stop path for the feature: the
/// registered `request_remote_agent_stop` command only persists an intent, and only this loop
/// ever reaches `ChatService::stop_agent` (which resolves `pkill`). Per tick it CAS-claims one
/// `Pending` intent (`Pending -> Stopping`), re-resolves the conversation at drain time, calls
/// the host-local stop, and records `Stopped`, the benign `NoLiveRun` terminal, or `Failed`.
pub(crate) fn spawn_remote_agent_stop_dispatcher(
    state: AppState,
    execution_state: Arc<ExecutionState>,
    app_handle: tauri::AppHandle,
) {
    if !try_start_recurring_service("remote_agent_stop_dispatcher") {
        tracing::debug!("Remote agent stop dispatcher already started; skipping duplicate spawn");
        return;
    }
    tauri::async_runtime::spawn(async move {
        // Startup reconciliation: crashed `Stopping` claims are terminalised, never re-driven.
        let now = chrono::Utc::now();
        let stale_cutoff = now - chrono::Duration::seconds(REMOTE_AGENT_STOP_STALE_LEASE_SECS);
        match state
            .remote_agent_stop_request_repo
            .fail_stale_stopping_stop_requests(stale_cutoff, now)
            .await
        {
            Ok(count) if count > 0 => {
                tracing::warn!(count, "swept stale remote agent-stop claims to FailedStale")
            }
            Ok(_) => {}
            Err(error) => tracing::error!(%error, "remote agent stop stale sweep failed"),
        }

        let mut interval = tokio::time::interval(REMOTE_AGENT_STOP_DISPATCH_INTERVAL);
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            // Drain opportunistically: a burst of intents (one per conversation) should not be
            // spread across one tick each. The loop stops as soon as nothing is claimable.
            loop {
                match drain_one_remote_agent_stop(&state, &execution_state, &app_handle).await {
                    Ok(RemoteAgentStopDrain::Idle) => break,
                    Ok(RemoteAgentStopDrain::Drained) => continue,
                    Err(error) => {
                        tracing::warn!(%error, "remote agent stop dispatcher tick failed");
                        break;
                    }
                }
            }
        }
    });
}

/// Whether a dispatcher pass found work. Distinguishing "nothing claimable" from "drained one"
/// is what lets the opportunistic drain terminate instead of spinning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RemoteAgentStopDrain {
    Idle,
    Drained,
}

/// Claim + stop at most one pending intent. Extracted (and generic over the runtime) so a test
/// can drive one pass with a mock handle and prove each terminal is reached for the right reason.
pub(crate) async fn drain_one_remote_agent_stop<R: tauri::Runtime + 'static>(
    state: &AppState,
    execution_state: &Arc<ExecutionState>,
    app_handle: &tauri::AppHandle<R>,
) -> AppResult<RemoteAgentStopDrain> {
    let claim_at = chrono::Utc::now();
    let Some(claimed) = state
        .remote_agent_stop_request_repo
        .claim_pending_stop_request(claim_at)
        .await?
    else {
        return Ok(RemoteAgentStopDrain::Idle);
    };

    // Re-resolve the target at drain time: the intent is a conversation id, and the row it
    // named may have been archived or deleted since. Fail-closed — an errored read terminalises
    // as Failed rather than proceeding to kill something we could not identify.
    let conversation = match state
        .chat_conversation_repo
        .get_by_id(&claimed.conversation_id)
        .await
    {
        Ok(Some(conversation)) => conversation,
        Ok(None) => {
            settle_stop_failure(
                state,
                &claimed.id,
                crate::commands::remote_agent_stop_commands::REMOTE_AGENT_STOP_CONVERSATION_GONE,
            )
            .await;
            return Ok(RemoteAgentStopDrain::Drained);
        }
        Err(error) => {
            tracing::warn!(%error, request_id = %claimed.id, "remote agent stop could not re-resolve its conversation");
            settle_stop_failure(
                state,
                &claimed.id,
                crate::commands::remote_agent_stop_commands::REMOTE_AGENT_STOP_LOOKUP_FAILED,
            )
            .await;
            return Ok(RemoteAgentStopDrain::Drained);
        }
    };

    // The host-local stop path, host-owned end to end: `AppChatService::stop_agent` drops the
    // interactive process and reaches `running_agent_registry` -> `kill_process` ->
    // `Command::new(resolve_pkill_cli_path())`. That sink lives HERE, in a loop no client can
    // reach, which is the entire point of the intent redesign.
    //
    // The runtime context id mirrors `resolve_agent_run_model_fields`: a Project conversation
    // is keyed by its own id, every other context by its context id.
    let context_type = conversation.context_type;
    let context_id = if context_type == ChatContextType::Project {
        conversation.id.as_str()
    } else {
        conversation.context_id.clone()
    };
    let service = crate::commands::unified_chat_commands::create_chat_service(
        state,
        app_handle.clone(),
        execution_state,
    );

    match service.stop_agent(context_type, &context_id).await {
        // `false` is not a failure: there was simply nothing running. Recording it as `Failed`
        // would make the ordinary finished-between-tap-and-drain race look like a broken host.
        Ok(false) => {
            if let Err(error) = state
                .remote_agent_stop_request_repo
                .resolve_stop_request_no_live_run(&claimed.id, chrono::Utc::now())
                .await
            {
                tracing::error!(%error, request_id = %claimed.id, "failed to record remote agent stop no-live-run");
            }
        }
        Ok(true) => {
            if let Err(error) = state
                .remote_agent_stop_request_repo
                .complete_stop_request(&claimed.id, chrono::Utc::now())
                .await
            {
                tracing::error!(%error, request_id = %claimed.id, "failed to record remote agent stop completion");
            }
        }
        Err(error) => {
            // Stop failures land in `Failed` VISIBLY — they must not masquerade as a hung
            // `Stopping` the client polls forever.
            tracing::warn!(%error, request_id = %claimed.id, "remote agent stop failed on host");
            settle_stop_failure(
                state,
                &claimed.id,
                crate::commands::remote_agent_stop_commands::REMOTE_AGENT_STOP_HOST_STOP_FAILED,
            )
            .await;
        }
    }

    Ok(RemoteAgentStopDrain::Drained)
}

/// How often the host drains one pending remote MODE SWITCH intent.
///
/// Between the stop brake's 1s and the start dispatcher's cadence: a mode switch is a direct
/// response to a picker the user just moved, so it should feel prompt, but each drain can prepare
/// a git worktree and is materially more expensive than a repository read.
const REMOTE_CONVERSATION_MODE_SWITCH_DISPATCH_INTERVAL: Duration = Duration::from_secs(2);
/// `Switching` rows older than this at boot are swept to `FailedStale` and NEVER re-driven.
const REMOTE_RESUME_STALE_LEASE_SECS: i64 = 300;

/// Claims and drains at most one queued SEND-NOW intent. A stale/lost claim is never replayed:
/// re-driving it could kill a second provider and launch a duplicate second turn.
pub(crate) async fn dispatch_one_remote_queued_send<R: tauri::Runtime + 'static>(
    state: &AppState,
    execution_state: &Arc<ExecutionState>,
    app_handle: &tauri::AppHandle<R>,
) -> AppResult<()> {
    use crate::application::remote_queue_send_intent::*;
    use crate::domain::services::QueueKey;
    let Some(row) = state
        .remote_queued_send_request_repo
        .claim_pending(chrono::Utc::now())
        .await?
    else {
        return Ok(());
    };
    let conversation_id = ChatConversationId::from_string(row.conversation_id.clone());
    let Some(conversation) = state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await?
    else {
        state
            .remote_queued_send_request_repo
            .fail(
                &row.id,
                REMOTE_QUEUE_SEND_CONVERSATION_NOT_FOUND,
                None,
                chrono::Utc::now(),
            )
            .await?;
        return Ok(());
    };
    if conversation.is_archived() {
        state
            .remote_queued_send_request_repo
            .fail(
                &row.id,
                REMOTE_QUEUE_SEND_CONVERSATION_ARCHIVED,
                None,
                chrono::Utc::now(),
            )
            .await?;
        return Ok(());
    }
    if conversation.context_type != ChatContextType::Project {
        state
            .remote_queued_send_request_repo
            .fail(
                &row.id,
                REMOTE_QUEUE_SEND_CONVERSATION_NOT_PROJECT,
                None,
                chrono::Utc::now(),
            )
            .await?;
        return Ok(());
    }
    if let Some(expected) = row.expected_active_run_id.as_deref() {
        let active = state
            .agent_run_repo
            .get_active_for_conversation(&conversation_id)
            .await?;
        if active.as_ref().map(|run| run.id.as_str()).as_deref() != Some(expected) {
            state
                .remote_queued_send_request_repo
                .fail(
                    &row.id,
                    REMOTE_QUEUE_SEND_RUN_CHANGED,
                    None,
                    chrono::Utc::now(),
                )
                .await?;
            return Ok(());
        }
    }
    let key = QueueKey::new(ChatContextType::Project, row.conversation_id.clone());
    let mut queued = state.queued_message_repo.list(&key).await?;
    queued.extend(
        state
            .message_queue
            .get_queued(ChatContextType::Project, &row.conversation_id),
    );
    let Some(entry) = queued
        .into_iter()
        .find(|entry| entry.id == row.queued_message_id)
    else {
        state
            .remote_queued_send_request_repo
            .fail(
                &row.id,
                REMOTE_QUEUE_SEND_ENTRY_GONE,
                None,
                chrono::Utc::now(),
            )
            .await?;
        return Ok(());
    };
    if let Some(provider) = entry.harness_override {
        let enabled = state
            .agent_provider_settings_repo
            .list()
            .await
            .map_err(|error| AppError::Infrastructure(error.to_string()))?
            .iter()
            .any(|stored| stored.provider == provider && stored.enabled);
        if !enabled {
            state
                .remote_queued_send_request_repo
                .fail(
                    &row.id,
                    REMOTE_QUEUE_SEND_PROVIDER_DISABLED,
                    None,
                    chrono::Utc::now(),
                )
                .await?;
            return Ok(());
        }
    }
    let service = build_chat_service_from_deps(
        Some(app_handle.clone()),
        Some(Arc::clone(execution_state)),
        &ChatRuntimeFactoryDeps::from_app_state(state),
    );
    match service
        .send_queued_message_now(
            ChatContextType::Project,
            &row.conversation_id,
            &row.queued_message_id,
        )
        .await
    {
        Ok(sent) => {
            state
                .remote_queued_send_request_repo
                .complete(
                    &row.id,
                    serde_json::json!({"runId":sent.agent_run_id}),
                    chrono::Utc::now(),
                )
                .await?
        }
        Err(ChatServiceError::ContextNotFound(_)) => {
            state
                .remote_queued_send_request_repo
                .fail(
                    &row.id,
                    REMOTE_QUEUE_SEND_ALREADY_SENT,
                    Some(serde_json::json!({"benign":true,"rehydrateQueue":true})),
                    chrono::Utc::now(),
                )
                .await?
        }
        Err(error) => {
            state
                .remote_queued_send_request_repo
                .fail(
                    &row.id,
                    REMOTE_QUEUE_SEND_HOST_FAILED,
                    Some(serde_json::json!({"restoredToFront":true,"rehydrateQueue":true})),
                    chrono::Utc::now(),
                )
                .await?;
            tracing::warn!(%error,request_id=%row.id,"remote queued SEND-NOW host dispatch failed");
        }
    }
    Ok(())
}

pub(crate) fn spawn_remote_resume_dispatchers(
    state: AppState,
    active_project_state: Arc<crate::application::ActiveProjectState>,
    execution_state: Arc<ExecutionState>,
    app_handle: tauri::AppHandle,
) {
    if !try_start_recurring_service("remote_resume_dispatchers") {
        return;
    }
    tauri::async_runtime::spawn(async move {
        let now = chrono::Utc::now();
        let cutoff = now - chrono::Duration::seconds(REMOTE_RESUME_STALE_LEASE_SECS);
        if let Err(error) = state
            .remote_execution_resume_request_repo
            .fail_stale(cutoff, now)
            .await
        {
            tracing::error!(%error,"remote execution-resume stale sweep failed");
        }
        if let Err(error) = state
            .remote_task_action_request_repo
            .fail_stale(cutoff, now)
            .await
        {
            tracing::error!(%error,"remote task-action stale sweep failed");
        }
        if let Err(error) = state
            .remote_plan_approval_request_repo
            .fail_stale(cutoff, now)
            .await
        {
            tracing::error!(%error, "remote plan-approval stale sweep failed");
        }
        // `Starting` rows older than this at boot are swept to `FailedStale` and NEVER auto-respawned:
        // a lost race between a dead claim and a re-spawn is a double-conversation factory, so we fail
        // closed and let the user retry explicitly.
        if let Err(error) = state
            .remote_finalize_decision_request_repo
            .fail_stale(cutoff, now)
            .await
        {
            tracing::error!(%error, "remote finalize-decision stale sweep failed");
        }
        if let Err(error) = state
            .remote_plan_edit_request_repo
            .fail_stale(cutoff, now)
            .await
        {
            tracing::error!(%error, "remote plan-edit stale sweep failed");
        }
        if let Err(error) = state
            .remote_queued_send_request_repo
            .fail_stale(cutoff, now)
            .await
        {
            tracing::error!(%error, "remote queued SEND-NOW stale sweep failed");
        }
        let mut interval = tokio::time::interval(Duration::from_secs(2));
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            if let Err(error) = dispatch_one_remote_execution_resume(
                &state,
                &active_project_state,
                &execution_state,
            )
            .await
            {
                tracing::warn!(%error,"remote execution-resume dispatch failed");
            }
            if let Err(error) =
                dispatch_one_remote_task_action(&state, &execution_state, &app_handle).await
            {
                tracing::warn!(%error,"remote task-action dispatch failed");
            }
            if let Err(error) = dispatch_one_remote_plan_approval(&state).await {
                tracing::warn!(%error, "remote plan-approval dispatch failed");
            }
            if let Err(error) = dispatch_one_remote_plan_edit(&state).await {
                tracing::warn!(%error, "remote plan-edit dispatch failed");
            }
            if let Err(error) =
                dispatch_one_remote_queued_send(&state, &execution_state, &app_handle).await
            {
                tracing::warn!(%error, "remote queued SEND-NOW dispatch failed");
            }
            if let Err(error) =
                dispatch_one_remote_finalize_decision(&state, &execution_state, Some(&app_handle))
                    .await
            {
                tracing::warn!(%error, "remote finalize-decision dispatch failed");
            }
        }
    });
}

pub(crate) async fn dispatch_one_remote_plan_edit(state: &AppState) -> AppResult<()> {
    use crate::application::remote_plan_edit_intent::*;
    use crate::infrastructure::sqlite::SqliteArtifactRepository as ArtifactRepo;
    let Some(row) = state
        .remote_plan_edit_request_repo
        .claim_pending(chrono::Utc::now())
        .await?
    else {
        return Ok(());
    };
    let id = row.artifact_id.clone();
    let artifact = state
        .db
        .run(move |conn| {
            let latest = ArtifactRepo::resolve_latest_sync(conn, &id)?;
            ArtifactRepo::get_by_id_sync(conn, &latest)?
                .ok_or_else(|| crate::error::AppError::NotFound(latest))
        })
        .await?;
    if artifact.metadata.version != row.expected_version {
        state
            .remote_plan_edit_request_repo
            .fail(
                &row.id,
                REMOTE_PLAN_EDIT_VERSION_CONFLICT,
                chrono::Utc::now(),
            )
            .await?;
        return Ok(());
    }
    match crate::application::plan_artifact_edit::update_plan_artifact_for_state(
        state,
        row.artifact_id,
        row.content,
        None,
        None,
    )
    .await
    {
        Ok(result) => {
            state
                .remote_plan_edit_request_repo
                .complete(
                    &row.id,
                    serialize_remote_dispatch_result_or_null(
                        result.artifact,
                        "plan artifact edit",
                        &row.id,
                    ),
                    chrono::Utc::now(),
                )
                .await?
        }
        Err(crate::error::AppError::Conflict(_)) => {
            state
                .remote_plan_edit_request_repo
                .fail(&row.id, REMOTE_PLAN_EDIT_FROZEN, chrono::Utc::now())
                .await?
        }
        Err(
            error @ (crate::error::AppError::Database(_)
            | crate::error::AppError::Infrastructure(_)),
        ) => return Err(error),
        Err(error) => {
            tracing::warn!(request_id=%row.id,%error,"remote plan edit host seam failed");
            state
                .remote_plan_edit_request_repo
                .fail(&row.id, REMOTE_PLAN_EDIT_HOST_FAILED, chrono::Utc::now())
                .await?
        }
    }
    Ok(())
}

pub(crate) async fn dispatch_one_remote_finalize_decision(
    state: &AppState,
    execution: &Arc<ExecutionState>,
    app: Option<&tauri::AppHandle>,
) -> AppResult<()> {
    use crate::application::remote_finalize_decision_intent::{
        REMOTE_FINALIZE_AUTHORITY_CHANGED, REMOTE_FINALIZE_HOST_FAILED, REMOTE_FINALIZE_NOT_PENDING,
    };
    use crate::domain::entities::{
        AcceptanceStatus, IdeationSessionStatus, RemoteFinalizeDecision,
    };
    let Some(row) = state
        .remote_finalize_decision_request_repo
        .claim_pending(chrono::Utc::now())
        .await?
    else {
        return Ok(());
    };
    let Some(session) = state
        .ideation_session_repo
        .get_by_id(&row.session_id)
        .await?
    else {
        state
            .remote_finalize_decision_request_repo
            .fail(
                &row.id,
                REMOTE_FINALIZE_AUTHORITY_CHANGED,
                chrono::Utc::now(),
            )
            .await?;
        return Ok(());
    };
    if session.status != IdeationSessionStatus::Active {
        state
            .remote_finalize_decision_request_repo
            .fail(
                &row.id,
                REMOTE_FINALIZE_AUTHORITY_CHANGED,
                chrono::Utc::now(),
            )
            .await?;
        return Ok(());
    }
    if session.acceptance_status != Some(AcceptanceStatus::Pending) {
        state
            .remote_finalize_decision_request_repo
            .fail(&row.id, REMOTE_FINALIZE_NOT_PENDING, chrono::Utc::now())
            .await?;
        return Ok(());
    }
    let result = match row.decision {
        RemoteFinalizeDecision::Accept => {
            match crate::application::ideation_finalize_execution::apply_pending_proposals_core_for_session(
                state,
                row.session_id.as_str(),
            ).await {
                Ok(value) => Ok((
                    serialize_remote_dispatch_result_or_null(
                        value.created_task_ids,
                        "ideation finalize accept",
                        &row.id,
                    ),
                    value.any_ready_tasks,
                )),
                Err(crate::error::AppError::Validation(message))
                    if message == crate::application::ideation_finalize_execution::SESSION_NO_LONGER_AWAITING_ACCEPTANCE =>
                {
                    Err(REMOTE_FINALIZE_NOT_PENDING)
                }
                Err(error @ (crate::error::AppError::Database(_)
                    | crate::error::AppError::Infrastructure(_))) => return Err(error),
                Err(error) => {
                    tracing::warn!(request_id=%row.id,%error,"remote finalize accept host seam failed");
                    Err(REMOTE_FINALIZE_HOST_FAILED)
                }
            }
        }
        RemoteFinalizeDecision::Reject => match state.ideation_session_repo
            .update_acceptance_status(&row.session_id, Some(AcceptanceStatus::Pending), None).await {
            Ok(true) => Ok((serde_json::json!({"status": "rejected", "sessionId": row.session_id.as_str()}), false)),
            Ok(false) => Err(REMOTE_FINALIZE_NOT_PENDING),
            Err(error) => return Err(error),
        },
    };
    match result {
        Ok((value, any_ready_tasks)) => {
            state
                .remote_finalize_decision_request_repo
                .complete(&row.id, value, chrono::Utc::now())
                .await?;
            crate::application::spawn_ready_task_scheduler_if_needed(
                state,
                Arc::clone(execution),
                app.cloned(),
                any_ready_tasks,
            );
        }
        Err(code) => {
            state
                .remote_finalize_decision_request_repo
                .fail(&row.id, code, chrono::Utc::now())
                .await?
        }
    }
    Ok(())
}

pub(crate) async fn dispatch_one_remote_plan_approval(state: &AppState) -> AppResult<()> {
    use crate::domain::entities::IdeationSessionStatus;
    let Some(row) = state
        .remote_plan_approval_request_repo
        .claim_pending(chrono::Utc::now())
        .await?
    else {
        return Ok(());
    };
    let active = state
        .ideation_session_repo
        .get_by_id(&row.session_id)
        .await?
        .is_some_and(|session| session.status == IdeationSessionStatus::Active);
    if !active {
        state
            .remote_plan_approval_request_repo
            .fail(
                &row.id,
                crate::application::remote_plan_approval_intent::REMOTE_PLAN_APPROVAL_AUTHORITY_CHANGED,
                chrono::Utc::now(),
            )
            .await?;
        return Ok(());
    }
    match crate::application::plan_artifact_approval::approve_plan_artifact_for_state(
        state,
        row.session_id.as_str().to_string(),
        Some(row.artifact_id),
        row.blueprint_artifact_id,
        row.blueprint_artifact_version,
    )
    .await
    {
        Ok(value) => {
            state
                .remote_plan_approval_request_repo
                .complete(
                    &row.id,
                    serialize_remote_dispatch_result_or_null(
                        value.artifact,
                        "plan approval",
                        &row.id,
                    ),
                    chrono::Utc::now(),
                )
                .await?
        }
        Err(crate::error::AppError::Conflict(_)) => state
            .remote_plan_approval_request_repo
            .fail(
                &row.id,
                crate::application::remote_plan_approval_intent::REMOTE_PLAN_APPROVAL_PLAN_CHANGED,
                chrono::Utc::now(),
            )
            .await?,
        Err(error) => {
            tracing::warn!(request_id=%row.id,%error,"remote plan approval host seam failed");
            state
                .remote_plan_approval_request_repo
                .fail(
                    &row.id,
                    crate::application::remote_plan_approval_intent::REMOTE_PLAN_APPROVAL_HOST_FAILED,
                    chrono::Utc::now(),
                )
                .await?;
        }
    }
    Ok(())
}

pub(crate) async fn dispatch_one_remote_execution_resume(
    state: &AppState,
    active: &Arc<crate::application::ActiveProjectState>,
    execution: &Arc<ExecutionState>,
) -> AppResult<()> {
    let Some(row) = state
        .remote_execution_resume_request_repo
        .claim_pending(chrono::Utc::now())
        .await?
    else {
        return Ok(());
    };
    if let Some(project_id) = &row.project_id {
        if state.project_repo.get_by_id(project_id).await?.is_none() {
            state
                .remote_execution_resume_request_repo
                .fail(
                    &row.id,
                    crate::application::remote_resume_intent::REMOTE_RESUME_AUTHORITY_CHANGED,
                    chrono::Utc::now(),
                )
                .await?;
            return Ok(());
        }
    }
    let result = crate::application::execution_resume::resume_execution_for_state(
        row.project_id.as_ref().map(|id| id.as_str().to_string()),
        active,
        execution,
        state,
    )
    .await;
    match result {
        Ok(value) => {
            state
                .remote_execution_resume_request_repo
                .complete(
                    &row.id,
                    serialize_remote_dispatch_result_or_null(value, "execution resume", &row.id),
                    chrono::Utc::now(),
                )
                .await?
        }
        Err(error) => {
            tracing::warn!(request_id=%row.id,%error,"remote execution resume host seam failed");
            state
                .remote_execution_resume_request_repo
                .fail(
                    &row.id,
                    crate::application::remote_resume_intent::REMOTE_RESUME_HOST_FAILED,
                    chrono::Utc::now(),
                )
                .await?;
        }
    }
    Ok(())
}

pub(crate) async fn dispatch_one_remote_task_action(
    state: &AppState,
    execution: &Arc<ExecutionState>,
    app: &tauri::AppHandle,
) -> AppResult<()> {
    use crate::domain::entities::RemoteTaskAction;
    let Some(row) = claim_and_revalidate_remote_task_action(state).await? else {
        return Ok(());
    };
    let result = match row.action {
        RemoteTaskAction::Resume => {
            crate::application::task_resume_execution::resume_task_for_state(
                row.task_id.expect("validated task id").as_str().to_string(),
                state,
                execution,
                app.clone(),
            )
            .await
            .map(|value| serialize_remote_dispatch_result_or_null(value, "task resume", &row.id))
        }
        RemoteTaskAction::Restart => crate::application::task_restart::restart_task_for_state(
            row.task_id.expect("validated task id").as_str().to_string(),
            row.force,
            row.note,
            state,
            execution,
        )
        .await
        .map(|value| serialize_remote_dispatch_result_or_null(value, "task restart", &row.id)),
        RemoteTaskAction::GroupResume => {
            crate::application::task_resume_execution::resume_tasks_in_group_for_state(
                row.group_kind.expect("validated group kind"),
                row.group_id.expect("validated group id"),
                row.project_id.as_str().to_string(),
                state,
                execution,
                app.clone(),
            )
            .await
            .map(|value| {
                serialize_remote_dispatch_result_or_null(value, "task group resume", &row.id)
            })
        }
    };
    match result {
        Ok(value) => {
            state
                .remote_task_action_request_repo
                .complete(&row.id, value, chrono::Utc::now())
                .await?
        }
        Err(error) => {
            tracing::warn!(request_id=%row.id,%error,"remote task action host seam failed");
            state
                .remote_task_action_request_repo
                .fail(
                    &row.id,
                    crate::application::remote_resume_intent::REMOTE_RESUME_HOST_FAILED,
                    chrono::Utc::now(),
                )
                .await?;
        }
    }
    Ok(())
}

fn serialize_remote_dispatch_result_or_null<T: serde::Serialize>(
    value: T,
    action: &str,
    request_id: &str,
) -> serde_json::Value {
    serde_json::to_value(value).unwrap_or_else(|error| {
        tracing::warn!(
            %request_id,
            %error,
            action,
            "remote host action succeeded but its result could not be serialized"
        );
        serde_json::Value::Null
    })
}

pub(crate) async fn claim_and_revalidate_remote_task_action(
    state: &AppState,
) -> AppResult<Option<crate::domain::entities::RemoteTaskActionRequest>> {
    use crate::domain::entities::{InternalStatus, RemoteTaskAction};
    let Some(row) = state
        .remote_task_action_request_repo
        .claim_pending(chrono::Utc::now())
        .await?
    else {
        return Ok(None);
    };
    let valid = match row.action {
        RemoteTaskAction::Resume => match &row.task_id {
            Some(id) => state
                .task_repo
                .get_by_id(id)
                .await?
                .is_some_and(|task| task.internal_status == InternalStatus::Paused),
            None => false,
        },
        RemoteTaskAction::Restart => match &row.task_id {
            Some(id) => state.task_repo.get_by_id(id).await?.is_some_and(|task| {
                matches!(
                    task.internal_status,
                    InternalStatus::Stopped | InternalStatus::Failed
                )
            }),
            None => false,
        },
        RemoteTaskAction::GroupResume => {
            state
                .project_repo
                .get_by_id(&row.project_id)
                .await?
                .is_some()
                && matches!(
                    row.group_kind.as_deref(),
                    Some("status" | "session" | "uncategorized")
                )
        }
    };
    if !valid {
        state
            .remote_task_action_request_repo
            .fail(
                &row.id,
                crate::application::remote_resume_intent::REMOTE_RESUME_AUTHORITY_CHANGED,
                chrono::Utc::now(),
            )
            .await?;
        return Ok(None);
    }
    Ok(Some(row))
}

/// Longer than the stop lease because a switch can legitimately take a while: preparing a
/// worktree for a large repository is the slow path here.
const REMOTE_CONVERSATION_MODE_SWITCH_STALE_LEASE_SECS: i64 = 600;

/// The host-owned driver for spawn-free remote conversation MODE SWITCHES (WP5a).
///
/// This loop is the SOLE holder of the workspace-preparing switch path for the feature: the
/// registered `request_remote_agent_conversation_mode_switch` command only persists an intent,
/// and only this loop ever reaches `switch_agent_conversation_mode_for_state` (which reaches
/// `GitService::ref_exists` and `ensure_git_worktree`). Per tick it CAS-claims one `Pending`
/// intent (`Pending -> Switching`), re-validates the conversation/ownership/liveness/target at
/// drain time, performs the switch, and records `Switched`, the benign `AlreadyInMode` terminal,
/// or `Failed`.
///
/// The terminal call is deliberately `switch_agent_conversation_mode_for_state` — the
/// `ModeSwitchRunningAgentPolicy::Reject` variant — and NOT the `..._stopping_running_agent`
/// variant the local Tauri command uses. Reusing the stopping variant would put
/// `ChatService::stop_agent` (and therefore `pkill`) inside this loop, making a dropdown change
/// able to terminate a live run as a side effect. Stopping stays WP2's separate, explicitly
/// user-initiated intent. See `remote_conversation_mode_switch_commands`' module doc.
pub(crate) fn spawn_remote_conversation_mode_switch_dispatcher(state: AppState) {
    if !try_start_recurring_service("remote_conversation_mode_switch_dispatcher") {
        tracing::debug!(
            "Remote conversation mode switch dispatcher already started; skipping duplicate spawn"
        );
        return;
    }
    tauri::async_runtime::spawn(async move {
        // Startup reconciliation: crashed `Switching` claims are terminalised, never re-driven. A
        // half-applied switch that crossed the plan/review boundary must not be replayed blindly.
        let now = chrono::Utc::now();
        let stale_cutoff =
            now - chrono::Duration::seconds(REMOTE_CONVERSATION_MODE_SWITCH_STALE_LEASE_SECS);
        match state
            .remote_conversation_mode_switch_request_repo
            .fail_stale_switching_mode_switch_requests(stale_cutoff, now)
            .await
        {
            Ok(count) if count > 0 => {
                tracing::warn!(
                    count,
                    "swept stale remote mode-switch claims to FailedStale"
                )
            }
            Ok(_) => {}
            Err(error) => tracing::error!(%error, "remote mode switch stale sweep failed"),
        }

        let mut interval = tokio::time::interval(REMOTE_CONVERSATION_MODE_SWITCH_DISPATCH_INTERVAL);
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            // Drain opportunistically: a burst of intents (one per conversation) should not be
            // spread across one tick each. The loop stops as soon as nothing is claimable.
            loop {
                match drain_one_remote_conversation_mode_switch(&state).await {
                    Ok(RemoteConversationModeSwitchDrain::Idle) => break,
                    Ok(RemoteConversationModeSwitchDrain::Drained) => continue,
                    Err(error) => {
                        tracing::warn!(%error, "remote mode switch dispatcher tick failed");
                        break;
                    }
                }
            }
        }
    });
}

/// Whether a dispatcher pass found work. Distinguishing "nothing claimable" from "drained one" is
/// what lets the opportunistic drain terminate instead of spinning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RemoteConversationModeSwitchDrain {
    Idle,
    Drained,
}

/// Claim + switch at most one pending intent. Extracted so a test can drive one pass and prove
/// each terminal is reached for the right reason.
///
/// Note the signature: no `AppHandle`, no `ExecutionState`. The switch seam this loop uses is
/// AppState-only, which is what keeps the process-terminating path out of this file's reach.
pub(crate) async fn drain_one_remote_conversation_mode_switch(
    state: &AppState,
) -> AppResult<RemoteConversationModeSwitchDrain> {
    let claim_at = chrono::Utc::now();
    let Some(claimed) = state
        .remote_conversation_mode_switch_request_repo
        .claim_pending_mode_switch_request(claim_at)
        .await?
    else {
        return Ok(RemoteConversationModeSwitchDrain::Idle);
    };

    // Re-prove authority at dispatch time. Every failure terminalises the row VISIBLY — a mode the
    // client rendered as switched but the host never applied is exactly the frontend/backend
    // drift this surface exists to avoid.
    match revalidate_remote_conversation_mode_switch(state, &claimed).await {
        Ok(RemoteModeSwitchRevalidation::Proceed) => {}
        Ok(RemoteModeSwitchRevalidation::AlreadyInMode) => {
            // The conversation reached the requested mode on its own between persist and drain
            // (another device, or the user on the host). Benign terminal, not a failure.
            if let Err(error) = state
                .remote_conversation_mode_switch_request_repo
                .resolve_mode_switch_request_already_in_mode(&claimed.id, chrono::Utc::now())
                .await
            {
                tracing::error!(%error, request_id = %claimed.id, "failed to record remote mode switch already-in-mode");
            }
            return Ok(RemoteConversationModeSwitchDrain::Drained);
        }
        Err(error_code) => {
            settle_mode_switch_failure(state, &claimed.id, &error_code).await;
            return Ok(RemoteConversationModeSwitchDrain::Drained);
        }
    }

    // The host-local switch path, host-owned end to end. This is the REJECT-policy variant: it
    // re-checks `running_agent_registry` itself and refuses rather than stopping, which makes the
    // registry the FINAL authority on liveness even though the revalidation above already
    // consulted `agent_run_repo`. Two fail-closed checks in the same direction, never opposing.
    let result = crate::commands::unified_chat_commands::switch_agent_conversation_mode_for_state(
        crate::commands::unified_chat_commands::SwitchAgentConversationModeInput {
            conversation_id: claimed.conversation_id.as_str(),
            mode: claimed.target_mode.to_string(),
            // All ABSENT by construction — the wire has no field for any of them, so the host
            // resolves workspace preparation from its own defaults. See the command module doc.
            runtime_override: None,
            base_ref_kind: None,
            base_branch_mode: None,
            base_ref: None,
            base_display_name: None,
            base_source_pull_request: None,
        },
        state,
    )
    .await;

    match result {
        Ok(_) => {
            if let Err(error) = state
                .remote_conversation_mode_switch_request_repo
                .complete_mode_switch_request(&claimed.id, chrono::Utc::now())
                .await
            {
                tracing::error!(%error, request_id = %claimed.id, "failed to record remote mode switch completion");
            }
        }
        Err(error) => {
            // Switch failures land in `Failed` VISIBLY — they must not masquerade as a hung
            // `Switching` the client polls forever. The host's message is logged rather than
            // returned: it is prose, and the client branches on the typed code.
            tracing::warn!(%error, request_id = %claimed.id, "remote mode switch failed on host");
            settle_mode_switch_failure(
                state,
                &claimed.id,
                crate::commands::remote_conversation_mode_switch_commands::REMOTE_MODE_SWITCH_HOST_SWITCH_FAILED,
            )
            .await;
        }
    }

    Ok(RemoteConversationModeSwitchDrain::Drained)
}

/// What re-validation concluded. `AlreadyInMode` is separated from `Proceed` because it settles on
/// a BENIGN terminal, and from `Err` because it is not a failure.
pub(crate) enum RemoteModeSwitchRevalidation {
    Proceed,
    AlreadyInMode,
}

/// Re-validate a claimed mode-switch intent at dispatch time. Returns the client-facing error code
/// on failure.
pub(crate) async fn revalidate_remote_conversation_mode_switch(
    state: &AppState,
    claimed: &crate::domain::entities::RemoteConversationModeSwitchRequest,
) -> Result<RemoteModeSwitchRevalidation, String> {
    use crate::commands::remote_conversation_mode_switch_commands::{
        REMOTE_MODE_SWITCH_CONVERSATION_ARCHIVED, REMOTE_MODE_SWITCH_CONVERSATION_GONE,
        REMOTE_MODE_SWITCH_LOOKUP_FAILED, REMOTE_MODE_SWITCH_PROJECT_MISMATCH,
        REMOTE_MODE_SWITCH_RUN_WENT_LIVE,
    };
    use crate::domain::entities::AgentConversationWorkspaceMode;

    // 1. The conversation must still exist and still be switchable.
    let conversation = state
        .chat_conversation_repo
        .get_by_id(&claimed.conversation_id)
        .await
        .map_err(|_| REMOTE_MODE_SWITCH_LOOKUP_FAILED.to_string())?
        .ok_or_else(|| REMOTE_MODE_SWITCH_CONVERSATION_GONE.to_string())?;
    if conversation.is_archived() {
        return Err(REMOTE_MODE_SWITCH_CONVERSATION_ARCHIVED.to_string());
    }
    if conversation.context_type != ChatContextType::Project
        || conversation.context_id != claimed.project_id.as_str()
    {
        return Err(REMOTE_MODE_SWITCH_PROJECT_MISMATCH.to_string());
    }

    // 2. The project must still exist.
    if state
        .project_repo
        .get_by_id(&claimed.project_id)
        .await
        .map_err(|_| REMOTE_MODE_SWITCH_LOOKUP_FAILED.to_string())?
        .is_none()
    {
        return Err(REMOTE_MODE_SWITCH_PROJECT_MISMATCH.to_string());
    }

    // 3. STILL no live run — the live-run rule re-proved at drain time. If one went live between
    //    persist and claim, switching would race a running agent's workspace. Retryable and
    //    distinct so the client can tell the user to stop the agent first.
    if state
        .agent_run_repo
        .get_active_for_conversation(&claimed.conversation_id)
        .await
        .map_err(|_| REMOTE_MODE_SWITCH_LOOKUP_FAILED.to_string())?
        .is_some()
    {
        return Err(REMOTE_MODE_SWITCH_RUN_WENT_LIVE.to_string());
    }

    // 4. The mode must STILL differ. Resolved exactly as the host switch path resolves it.
    let existing_workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation.id)
        .await
        .map_err(|_| REMOTE_MODE_SWITCH_LOOKUP_FAILED.to_string())?;
    let current_mode = conversation
        .agent_mode
        .or_else(|| existing_workspace.as_ref().map(|workspace| workspace.mode))
        .unwrap_or(AgentConversationWorkspaceMode::Chat);
    if current_mode == claimed.target_mode {
        return Ok(RemoteModeSwitchRevalidation::AlreadyInMode);
    }

    Ok(RemoteModeSwitchRevalidation::Proceed)
}

async fn settle_mode_switch_failure(state: &AppState, request_id: &str, error_code: &str) {
    if let Err(error) = state
        .remote_conversation_mode_switch_request_repo
        .fail_mode_switch_request(request_id, error_code, chrono::Utc::now())
        .await
    {
        tracing::error!(%error, %request_id, "failed to record remote mode switch failure");
    }
}

async fn settle_stop_failure(state: &AppState, request_id: &str, error_code: &str) {
    if let Err(error) = state
        .remote_agent_stop_request_repo
        .fail_stop_request(request_id, error_code, chrono::Utc::now())
        .await
    {
        tracing::error!(%error, %request_id, "failed to record remote agent stop failure");
    }
}

pub async fn maybe_start_external_mcp(
    app_handle: tauri::AppHandle,
    wait_for_backend_ready: impl Fn(
        u16,
        Duration,
    ) -> futures::future::BoxFuture<'static, Result<(), String>>,
) {
    let started_at = std::time::Instant::now();
    let bootstrap = match resolve_default_external_mcp_bootstrap() {
        Ok(None) => return,
        Ok(Some(bootstrap)) => bootstrap,
        Err(error) => {
            warn!(
                "External MCP bootstrap unavailable, skipping start: {}",
                error
            );
            return;
        }
    };

    let backend_port = backend_http_port();
    let startup_timeout = external_mcp_startup_timeout(&bootstrap.config);
    let wait_started_at = std::time::Instant::now();
    match wait_for_backend_ready(backend_port, startup_timeout).await {
        Err(e) => {
            warn!(
                elapsed_ms = started_at.elapsed().as_millis(),
                backend_wait_ms = wait_started_at.elapsed().as_millis(),
                "Backend not ready, skipping external MCP start: {}",
                e
            );
        }
        Ok(()) => {
            info!(
                port = backend_port,
                backend_wait_ms = wait_started_at.elapsed().as_millis(),
                "Backend ready, starting external MCP server"
            );
            let supervisor_started_at = std::time::Instant::now();
            let app_data_dir = app_handle
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| PathBuf::from("."));
            let supervisor = Arc::new(ExternalMcpSupervisor::new(
                bootstrap.config,
                app_handle.clone(),
                app_data_dir,
            ));
            let handle = app_handle.state::<ExternalMcpHandle>();
            if handle.set(Arc::clone(&supervisor)).is_err() {
                warn!("ExternalMcpHandle already initialized");
                return;
            }
            match Arc::clone(&supervisor)
                .start(bootstrap.node_path, bootstrap.entry_path)
                .await
            {
                Ok(()) => {
                    let readiness_budget = startup_timeout.saturating_sub(started_at.elapsed());
                    match supervisor.await_ready(readiness_budget).await {
                        Ok(()) => info!(
                            supervisor_elapsed_ms = supervisor_started_at.elapsed().as_millis(),
                            elapsed_ms = started_at.elapsed().as_millis(),
                            "External MCP startup reached readiness"
                        ),
                        Err(error) => warn!(
                            supervisor_elapsed_ms = supervisor_started_at.elapsed().as_millis(),
                            elapsed_ms = started_at.elapsed().as_millis(),
                            "External MCP startup did not reach readiness: {}",
                            error
                        ),
                    }
                }
                Err(e) => {
                    warn!(
                        supervisor_elapsed_ms = supervisor_started_at.elapsed().as_millis(),
                        elapsed_ms = started_at.elapsed().as_millis(),
                        "Failed to start external MCP: {}",
                        e
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod remote_conversation_message_dispatcher_tests {
    use std::sync::Arc;

    use super::dispatch_one_remote_conversation_message;
    use crate::application::AppState;
    use crate::commands::remote_conversation_message_commands::{
        REMOTE_CONV_MESSAGE_CONVERSATION_ARCHIVED, REMOTE_CONV_MESSAGE_CONVERSATION_NOT_FOUND,
        REMOTE_CONV_MESSAGE_MODEL_NOT_ENABLED, REMOTE_CONV_MESSAGE_PROJECT_MISMATCH,
        REMOTE_CONV_MESSAGE_PROVIDER_NOT_ENABLED, REMOTE_CONV_MESSAGE_RUN_WENT_LIVE,
    };
    use crate::commands::ExecutionState;
    use crate::domain::entities::{
        AgentRun, ChatConversation, Project, ProjectId, RemoteConversationMessageRequest,
        RemoteConversationMessageStatus,
    };
    use crate::infrastructure::memory::MemoryAgentProviderSettingsRepository;
    use ralphx_domain::agents::{AgentHarnessKind, AgentProviderSettings};

    fn claude_enabled_default() -> AgentProviderSettings {
        AgentProviderSettings {
            enabled: true,
            is_default: true,
            model: Some("sonnet".to_string()),
            ..AgentProviderSettings::disabled_defaults(AgentHarnessKind::Claude)
        }
    }

    /// A state with an enabled Claude provider, a project, and one idle project conversation.
    async fn seeded_state() -> (AppState, ProjectId, ChatConversation) {
        let mut state = AppState::new_test();
        state.agent_provider_settings_repo = Arc::new(MemoryAgentProviderSettingsRepository::new());
        state
            .agent_provider_settings_repo
            .upsert(&claude_enabled_default())
            .await
            .expect("seed provider");
        let project = state
            .project_repo
            .create(Project::new(
                "Continue".to_string(),
                "/tmp/continue".to_string(),
            ))
            .await
            .expect("seed project");
        let conversation = state
            .chat_conversation_repo
            .create(ChatConversation::new_project(project.id.clone()))
            .await
            .expect("seed conversation");
        (state, project.id, conversation)
    }

    async fn seed_intent(
        state: &AppState,
        conversation: &ChatConversation,
        project_id: &ProjectId,
        provider: &str,
        model_override: Option<&str>,
    ) {
        let now = chrono::Utc::now();
        state
            .remote_conversation_message_request_repo
            .create_message_request(RemoteConversationMessageRequest {
                id: "intent-1".to_string(),
                conversation_id: conversation.id.clone(),
                project_id: project_id.clone(),
                content: "keep going".to_string(),
                provider: provider.to_string(),
                model_override: model_override.map(str::to_string),
                logical_effort: None,
                status: RemoteConversationMessageStatus::Pending,
                error_code: None,
                requested_by_device_id: String::new(),
                agent_run_id: None,
                claimed_at: None,
                created_at: now,
                updated_at: now,
            })
            .await
            .expect("seed intent");
    }

    async fn tick(state: &AppState) {
        let execution_state = Arc::new(ExecutionState::new());
        let app_handle = crate::testing::create_mock_app_handle();
        dispatch_one_remote_conversation_message(state, &execution_state, &app_handle)
            .await
            .expect("dispatcher tick");
    }

    async fn stored_intent(state: &AppState) -> RemoteConversationMessageRequest {
        state
            .remote_conversation_message_request_repo
            .get_message_request("intent-1")
            .await
            .expect("read intent")
            .expect("intent exists")
    }

    /// The revalidation that only the dispatcher can perform: a run that went live between
    /// persist and claim. The live queue owns the turn, so a fresh send here would DOUBLE it.
    /// The intent must terminalise with the distinct retryable code, and no second run may exist.
    #[tokio::test]
    async fn a_run_that_went_live_between_persist_and_claim_fails_the_intent() {
        let (state, project_id, conversation) = seeded_state().await;
        seed_intent(&state, &conversation, &project_id, "claude", None).await;
        // The race: a run goes live AFTER the command validated "no active run".
        state
            .agent_run_repo
            .create(AgentRun::new(conversation.id.clone()))
            .await
            .expect("live run");

        tick(&state).await;

        let stored = stored_intent(&state).await;
        assert_eq!(stored.status, RemoteConversationMessageStatus::Failed);
        assert_eq!(
            stored.error_code.as_deref(),
            Some(REMOTE_CONV_MESSAGE_RUN_WENT_LIVE)
        );
        assert!(stored.agent_run_id.is_none());
    }

    /// A provider disabled since persist must terminalise the intent as `Failed` and NEVER send —
    /// authority-before-effects, fail-closed, and no silent substitution to another provider.
    #[tokio::test]
    async fn a_provider_disabled_since_persist_fails_the_intent_and_never_sends() {
        let (state, project_id, conversation) = seeded_state().await;
        // The intent named codex, which was never enabled here.
        seed_intent(&state, &conversation, &project_id, "codex", None).await;

        tick(&state).await;

        let stored = stored_intent(&state).await;
        assert_eq!(stored.status, RemoteConversationMessageStatus::Failed);
        assert_eq!(
            stored.error_code.as_deref(),
            Some(REMOTE_CONV_MESSAGE_PROVIDER_NOT_ENABLED)
        );
        // Absence: re-validation failure sends nothing.
        assert!(
            state
                .agent_run_repo
                .get_active_for_conversation(&conversation.id)
                .await
                .expect("run read")
                .is_none(),
            "a re-validation failure must not start a run"
        );
    }

    /// A model disabled since persist is rejected rather than silently swapped for the default.
    #[tokio::test]
    async fn a_model_disabled_since_persist_fails_the_intent() {
        let (state, project_id, conversation) = seeded_state().await;
        seed_intent(
            &state,
            &conversation,
            &project_id,
            "claude",
            Some("totally-not-a-real-model"),
        )
        .await;

        tick(&state).await;

        let stored = stored_intent(&state).await;
        assert_eq!(stored.status, RemoteConversationMessageStatus::Failed);
        assert_eq!(
            stored.error_code.as_deref(),
            Some(REMOTE_CONV_MESSAGE_MODEL_NOT_ENABLED)
        );
    }

    /// A conversation archived since persist must not be continued.
    #[tokio::test]
    async fn a_conversation_archived_since_persist_fails_the_intent() {
        let (state, project_id, conversation) = seeded_state().await;
        seed_intent(&state, &conversation, &project_id, "claude", None).await;
        state
            .chat_conversation_repo
            .archive(&conversation.id)
            .await
            .expect("archive");

        tick(&state).await;

        let stored = stored_intent(&state).await;
        assert_eq!(stored.status, RemoteConversationMessageStatus::Failed);
        assert_eq!(
            stored.error_code.as_deref(),
            Some(REMOTE_CONV_MESSAGE_CONVERSATION_ARCHIVED)
        );
    }

    /// The intent's own scope is re-proved at dispatch: a row whose project no longer matches the
    /// persisted conversation is refused rather than sent into whichever project it names.
    #[tokio::test]
    async fn a_project_mismatch_at_dispatch_fails_the_intent() {
        let (state, _, conversation) = seeded_state().await;
        seed_intent(
            &state,
            &conversation,
            &ProjectId::from_string("some-other-project".to_string()),
            "claude",
            None,
        )
        .await;

        tick(&state).await;

        let stored = stored_intent(&state).await;
        assert_eq!(stored.status, RemoteConversationMessageStatus::Failed);
        assert_eq!(
            stored.error_code.as_deref(),
            Some(REMOTE_CONV_MESSAGE_PROJECT_MISMATCH)
        );
    }

    /// A conversation deleted since persist terminalises visibly rather than hanging in
    /// `Dispatching` — the ghost-message hazard is a stuck row just as much as a lost one.
    #[tokio::test]
    async fn a_conversation_deleted_since_persist_fails_the_intent() {
        let (state, project_id, conversation) = seeded_state().await;
        seed_intent(&state, &conversation, &project_id, "claude", None).await;
        state
            .chat_conversation_repo
            .delete(&conversation.id)
            .await
            .expect("delete conversation");

        tick(&state).await;

        let stored = stored_intent(&state).await;
        assert_eq!(stored.status, RemoteConversationMessageStatus::Failed);
        assert_eq!(
            stored.error_code.as_deref(),
            Some(REMOTE_CONV_MESSAGE_CONVERSATION_NOT_FOUND)
        );
    }

    /// An empty table is a no-op, not an error: the loop ticks every two seconds forever.
    #[tokio::test]
    async fn an_empty_queue_is_a_no_op() {
        let (state, _, _) = seeded_state().await;
        tick(&state).await;
        assert!(state
            .remote_conversation_message_request_repo
            .get_message_request("intent-1")
            .await
            .expect("read intent")
            .is_none());
    }

    /// The claim is CAS-guarded, so a second tick cannot re-claim a row the first already took.
    /// Without this, two ticks racing would deliver the same turn twice.
    #[tokio::test]
    async fn a_claimed_intent_is_never_reclaimed_by_a_later_tick() {
        let (state, project_id, conversation) = seeded_state().await;
        seed_intent(&state, &conversation, &project_id, "codex", None).await;

        tick(&state).await;
        let after_first = stored_intent(&state).await;
        assert_eq!(after_first.status, RemoteConversationMessageStatus::Failed);

        // A second tick must find nothing to claim and must not touch the settled row.
        tick(&state).await;
        let after_second = stored_intent(&state).await;
        assert_eq!(after_second.status, RemoteConversationMessageStatus::Failed);
        assert_eq!(after_second.updated_at, after_first.updated_at);
    }
}

#[cfg(test)]
mod remote_conversation_start_dispatcher_tests {
    use std::sync::Arc;

    use super::{build_remote_conversation_start_input, dispatch_one_remote_conversation_start};
    use crate::application::AppState;
    use crate::commands::remote_conversation_start_commands::REMOTE_CONV_START_PROVIDER_NOT_ENABLED;
    use crate::commands::ExecutionState;
    use crate::domain::entities::{
        ChatConversationId, ProjectId, RemoteConversationStartRequest,
        RemoteConversationStartStatus,
    };
    use crate::infrastructure::memory::MemoryAgentProviderSettingsRepository;

    #[test]
    fn claimed_mode_reaches_the_host_start_input_without_a_chat_rewrite() {
        let now = chrono::Utc::now();
        let claimed = RemoteConversationStartRequest {
            id: "intent-plan".to_string(),
            conversation_id: ChatConversationId::new(),
            project_id: ProjectId::from_string("proj-plan".to_string()),
            content: "plan the change".to_string(),
            provider: "codex".to_string(),
            model: None,
            effort: None,
            mode: "plan".to_string(),
            status: RemoteConversationStartStatus::Starting,
            error_code: None,
            requested_by_device_id: String::new(),
            agent_run_id: None,
            claimed_at: Some(now),
            created_at: now,
            updated_at: now,
        };

        let input = build_remote_conversation_start_input(&claimed);
        assert_eq!(input.mode.as_deref(), Some("plan"));
    }

    /// Claim-time re-validation failure (a provider disabled since persist) must terminalise the
    /// intent as `Failed` and NEVER spawn — authority-before-effects, fail-closed.
    #[tokio::test]
    async fn revalidation_failure_marks_failed_and_never_spawns() {
        let mut state = AppState::new_test();
        // Empty provider repo => "codex" is not enabled => re-validation fails before any spawn.
        state.agent_provider_settings_repo = Arc::new(MemoryAgentProviderSettingsRepository::new());

        let conversation_id = ChatConversationId::new();
        let now = chrono::Utc::now();
        state
            .remote_conversation_start_request_repo
            .create_start_request(RemoteConversationStartRequest {
                id: "intent-1".to_string(),
                conversation_id: conversation_id.clone(),
                project_id: ProjectId::from_string("proj-1".to_string()),
                content: "explore".to_string(),
                provider: "codex".to_string(),
                model: None,
                effort: None,
                mode: "chat".to_string(),
                status: RemoteConversationStartStatus::Pending,
                error_code: None,
                requested_by_device_id: String::new(),
                agent_run_id: None,
                claimed_at: None,
                created_at: now,
                updated_at: now,
            })
            .await
            .expect("seed intent");

        let execution_state = Arc::new(ExecutionState::new());
        let app_handle = crate::testing::create_mock_app_handle();

        dispatch_one_remote_conversation_start(&state, &execution_state, &app_handle)
            .await
            .expect("dispatcher tick");

        let stored = state
            .remote_conversation_start_request_repo
            .get_start_request("intent-1")
            .await
            .expect("read intent")
            .expect("intent exists");
        assert_eq!(stored.status, RemoteConversationStartStatus::Failed);
        assert_eq!(
            stored.error_code.as_deref(),
            Some(REMOTE_CONV_START_PROVIDER_NOT_ENABLED)
        );
        assert!(stored.agent_run_id.is_none());
        // Absence: re-validation failure spawns nothing.
        let active = state
            .agent_run_repo
            .get_active_for_conversation(&conversation_id)
            .await
            .expect("run read");
        assert!(
            active.is_none(),
            "re-validation failure must not spawn a run"
        );
    }
}

#[cfg(test)]
mod remote_agent_stop_dispatcher_tests {
    use std::sync::Arc;

    use super::{drain_one_remote_agent_stop, RemoteAgentStopDrain};
    use crate::application::AppState;
    use crate::commands::remote_agent_stop_commands::REMOTE_AGENT_STOP_CONVERSATION_GONE;
    use crate::commands::ExecutionState;
    use crate::domain::entities::{
        ChatContextType, ChatConversation, ChatConversationId, ProjectId, RemoteAgentStopRequest,
        RemoteAgentStopStatus,
    };
    use crate::domain::services::running_agent_registry::RunningAgentKey;

    async fn seed_conversation(state: &AppState) -> ChatConversationId {
        let conversation =
            ChatConversation::new_project(ProjectId::from_string("project-1".to_string()));
        state
            .chat_conversation_repo
            .create(conversation)
            .await
            .expect("seed conversation")
            .id
    }

    async fn seed_pending_stop(state: &AppState, id: &str, conversation_id: &ChatConversationId) {
        let now = chrono::Utc::now();
        state
            .remote_agent_stop_request_repo
            .create_stop_request(RemoteAgentStopRequest {
                id: id.to_string(),
                conversation_id: conversation_id.clone(),
                status: RemoteAgentStopStatus::Pending,
                error_code: None,
                requested_by_device_id: String::new(),
                claimed_at: None,
                created_at: now,
                updated_at: now,
            })
            .await
            .expect("seed intent");
    }

    async fn status(state: &AppState, id: &str) -> RemoteAgentStopStatus {
        state
            .remote_agent_stop_request_repo
            .get_stop_request(id)
            .await
            .expect("read intent")
            .expect("intent exists")
            .status
    }

    /// A live run is terminated and the intent settles `Stopped`.
    ///
    /// The registry entry carries `pid = 0`, which `kill_process` refuses by its own safety
    /// guard — so this exercises the production drain path end to end without the test ever
    /// resolving `pkill` or signalling a real process.
    #[tokio::test]
    async fn a_live_run_is_stopped_and_the_intent_settles_stopped() {
        let state = AppState::new_test();
        let conversation_id = seed_conversation(&state).await;
        state
            .running_agent_registry
            .register(
                RunningAgentKey::new(
                    ChatContextType::Project.to_string(),
                    conversation_id.as_str(),
                ),
                0,
                conversation_id.as_str(),
                "run-1".to_string(),
                None,
                None,
            )
            .await;
        seed_pending_stop(&state, "stop-1", &conversation_id).await;

        let execution_state = Arc::new(ExecutionState::new());
        let app_handle = crate::testing::create_mock_app_handle();
        let outcome = drain_one_remote_agent_stop(&state, &execution_state, &app_handle)
            .await
            .expect("drain pass");

        assert_eq!(outcome, RemoteAgentStopDrain::Drained);
        assert_eq!(
            status(&state, "stop-1").await,
            RemoteAgentStopStatus::Stopped
        );
        assert!(
            state
                .running_agent_registry
                .get(&RunningAgentKey::new(
                    ChatContextType::Project.to_string(),
                    conversation_id.as_str(),
                ))
                .await
                .is_none(),
            "the run must be gone from the registry after the brake bites"
        );
    }

    /// Nothing running is a BENIGN terminal. Spelling it `Failed` would make the ordinary
    /// finished-between-tap-and-drain race look like a broken host and push the client into a
    /// retry loop against an idle conversation.
    #[tokio::test]
    async fn an_idle_conversation_settles_no_live_run_without_an_error_code() {
        let state = AppState::new_test();
        let conversation_id = seed_conversation(&state).await;
        seed_pending_stop(&state, "stop-1", &conversation_id).await;

        let execution_state = Arc::new(ExecutionState::new());
        let app_handle = crate::testing::create_mock_app_handle();
        drain_one_remote_agent_stop(&state, &execution_state, &app_handle)
            .await
            .expect("drain pass");

        let stored = state
            .remote_agent_stop_request_repo
            .get_stop_request("stop-1")
            .await
            .expect("read intent")
            .expect("intent exists");
        assert_eq!(stored.status, RemoteAgentStopStatus::NoLiveRun);
        assert!(stored.error_code.is_none());
    }

    /// The conversation the intent named is gone at drain time: terminalise VISIBLY as `Failed`
    /// rather than leaving a `Stopping` row the client polls forever.
    #[tokio::test]
    async fn a_vanished_conversation_settles_failed_with_a_code() {
        let state = AppState::new_test();
        seed_pending_stop(
            &state,
            "stop-1",
            &ChatConversationId::from_string("never-existed".to_string()),
        )
        .await;

        let execution_state = Arc::new(ExecutionState::new());
        let app_handle = crate::testing::create_mock_app_handle();
        drain_one_remote_agent_stop(&state, &execution_state, &app_handle)
            .await
            .expect("drain pass");

        let stored = state
            .remote_agent_stop_request_repo
            .get_stop_request("stop-1")
            .await
            .expect("read intent")
            .expect("intent exists");
        assert_eq!(stored.status, RemoteAgentStopStatus::Failed);
        assert_eq!(
            stored.error_code.as_deref(),
            Some(REMOTE_AGENT_STOP_CONVERSATION_GONE)
        );
    }

    /// Nothing claimable reports `Idle`, which is what terminates the opportunistic drain loop.
    /// If this ever reported `Drained`, the dispatcher would spin at 100% CPU.
    #[tokio::test]
    async fn an_empty_queue_reports_idle() {
        let state = AppState::new_test();
        let execution_state = Arc::new(ExecutionState::new());
        let app_handle = crate::testing::create_mock_app_handle();

        let outcome = drain_one_remote_agent_stop(&state, &execution_state, &app_handle)
            .await
            .expect("drain pass");
        assert_eq!(outcome, RemoteAgentStopDrain::Idle);
    }

    /// A claim that already settled cannot be re-driven: the second pass must find nothing
    /// pending, so a restarted run is never killed by a stale intent.
    #[tokio::test]
    async fn a_settled_intent_is_not_re_drained() {
        let state = AppState::new_test();
        let conversation_id = seed_conversation(&state).await;
        seed_pending_stop(&state, "stop-1", &conversation_id).await;

        let execution_state = Arc::new(ExecutionState::new());
        let app_handle = crate::testing::create_mock_app_handle();
        drain_one_remote_agent_stop(&state, &execution_state, &app_handle)
            .await
            .expect("first pass");
        let second = drain_one_remote_agent_stop(&state, &execution_state, &app_handle)
            .await
            .expect("second pass");

        assert_eq!(second, RemoteAgentStopDrain::Idle);
        assert_eq!(
            status(&state, "stop-1").await,
            RemoteAgentStopStatus::NoLiveRun
        );
    }
}
