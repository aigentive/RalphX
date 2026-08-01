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
use crate::application::chat_service::{ChatService, SendCallerContext, SendMessageOptions};
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

/// Build the host-forced start input: the seeded draft conversation, `chat` mode, and the
/// re-validated provider/model/effort. Everything else (persona, base/branch, team, attachments)
/// is a host-forced default — none of it is client-expressible in v1.5.
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
        mode: Some("chat".to_string()),
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

    use super::dispatch_one_remote_conversation_start;
    use crate::application::AppState;
    use crate::commands::remote_conversation_start_commands::REMOTE_CONV_START_PROVIDER_NOT_ENABLED;
    use crate::commands::ExecutionState;
    use crate::domain::entities::{
        ChatConversationId, ProjectId, RemoteConversationStartRequest,
        RemoteConversationStartStatus,
    };
    use crate::infrastructure::memory::MemoryAgentProviderSettingsRepository;

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
