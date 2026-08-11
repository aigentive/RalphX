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
    AgentConversationStartDeps, AgentConversationStartService,
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
