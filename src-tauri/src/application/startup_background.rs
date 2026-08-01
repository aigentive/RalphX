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
