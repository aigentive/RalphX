use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

use crate::application::agent_conversation_mode_switch::system_switch_automation_run_to_edit;
use crate::application::agent_conversation_start_service::{
    AgentConversationStartDeps, AgentConversationStartService,
};
use crate::application::agent_workspace_bridge::{
    dispatch_agent_workspace_bridge_events_once_with_deps, AgentWorkspaceBridgeDeps,
};
use crate::application::automation::plan_gate::{
    AutomationPlanVerificationStartOutcome, AutomationPlanVerificationStartRequest,
    AutomationPlanVerificationStarter, AutomationRunResumer, ResumeDelivery,
};
use crate::application::automation::merged_run_finalizer::AppStateAutomationMergedRunFinalizer;
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
use crate::application::runtime_factory::{build_chat_service_from_deps, ChatRuntimeFactoryDeps};
use crate::application::verification_child_session::{
    repair_blank_orphaned_verification_generation, spawn_verification_agent,
    trigger_auto_verify_generation,
};
use crate::application::{AppState, TeamService};
use crate::commands::ExecutionState;
use crate::domain::entities::{
    ChatContextType, ChatConversationId, VerificationConfirmationStatus, VerificationStatus,
};
use crate::domain::repositories::{
    ExternalEventsRepository, MemoryArchiveRepository, MemoryEntryRepository, ProjectRepository,
    TaskRepository,
};
use crate::domain::services::load_effective_verification_status;
use crate::error::{AppError, AppResult};
use crate::infrastructure::sqlite::SqlitePlanArtifactApprovalRepository;
use crate::infrastructure::{ExternalMcpHandle, ExternalMcpSupervisor};
use crate::utils::backend_endpoint::backend_http_port;
use tauri::Manager;
use tokio::time::MissedTickBehavior;
use tracing::{info, warn};

const AGENT_WORKSPACE_BRIDGE_DISPATCH_INTERVAL: Duration = Duration::from_secs(5);

pub struct AgentConversationAutomationRunStarter<R: tauri::Runtime + 'static> {
    state: AppState,
    execution_state: Arc<ExecutionState>,
    team_service: Option<Arc<TeamService>>,
    app_handle: tauri::AppHandle<R>,
}

impl<R: tauri::Runtime + 'static> AgentConversationAutomationRunStarter<R> {
    pub fn new(
        state: AppState,
        execution_state: Arc<ExecutionState>,
        team_service: Option<Arc<TeamService>>,
        app_handle: tauri::AppHandle<R>,
    ) -> Self {
        Self {
            state,
            execution_state,
            team_service,
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
        None,
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
            team_service: self.team_service.clone(),
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

    async fn launches_paused(&self) -> AppResult<bool> {
        Ok(self.execution_state.is_paused())
    }

    async fn switch_to_edit(&self, conversation_id: &ChatConversationId) -> AppResult<()> {
        system_switch_automation_run_to_edit(conversation_id, &self.state).await?;
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
}

pub struct AgentConversationAutomationPlanVerificationStarter<R: tauri::Runtime + 'static> {
    state: AppState,
    execution_state: Arc<ExecutionState>,
    app_handle: tauri::AppHandle<R>,
}

impl<R: tauri::Runtime + 'static> AgentConversationAutomationPlanVerificationStarter<R> {
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

    fn chat_service(&self) -> impl ChatService {
        let chat_deps = ChatRuntimeFactoryDeps::from_app_state(&self.state);
        build_chat_service_from_deps(
            Some(self.app_handle.clone()),
            Some(Arc::clone(&self.execution_state)),
            &chat_deps,
        )
    }
}

#[async_trait]
impl<R: tauri::Runtime + 'static> AutomationPlanVerificationStarter
    for AgentConversationAutomationPlanVerificationStarter<R>
{
    async fn start_verification(
        &self,
        request: AutomationPlanVerificationStartRequest,
    ) -> AppResult<AutomationPlanVerificationStartOutcome> {
        let session_id = request.session_id;
        let provider_harness = request.provider_harness;
        self.state
            .ideation_session_repo
            .set_verification_confirmation_status(
                &session_id,
                Some(VerificationConfirmationStatus::Accepted),
            )
            .await?;

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

        repair_blank_orphaned_verification_generation(&self.state, &session).await?;

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

        let (status, in_progress) =
            load_effective_verification_status(self.state.ideation_session_repo.as_ref(), &session)
                .await?;
        if in_progress || status == VerificationStatus::Reviewing {
            return Ok(AutomationPlanVerificationStartOutcome::AlreadyInProgress {
                generation: session.verification_generation,
            });
        }

        let maybe_generation = trigger_auto_verify_generation(&self.state, &session_id).await?;

        let Some(generation) = maybe_generation else {
            let Some((status, in_progress)) = self
                .state
                .ideation_session_repo
                .get_verification_status(&session_id)
                .await?
            else {
                return Ok(AutomationPlanVerificationStartOutcome::Unavailable {
                    detail: "verification trigger did not update a known session".to_string(),
                });
            };
            if in_progress || status == VerificationStatus::Reviewing {
                return Ok(AutomationPlanVerificationStartOutcome::AlreadyInProgress {
                    generation: session.verification_generation,
                });
            }
            return Ok(AutomationPlanVerificationStartOutcome::AlreadyTerminal {
                generation: session.verification_generation,
                status,
            });
        };

        let spawn = spawn_verification_agent(
            &self.state,
            &session_id,
            generation,
            provider_harness,
            &[],
            |_| self.chat_service(),
        )
        .await;
        if spawn.spawned {
            Ok(AutomationPlanVerificationStartOutcome::Started { generation })
        } else {
            Ok(AutomationPlanVerificationStartOutcome::Unavailable {
                detail: spawn.failure_detail.unwrap_or_else(|| {
                    "verification agent failed to spawn for an unknown reason".to_string()
                }),
            })
        }
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
    let team_service = app_handle
        .try_state::<Arc<crate::application::TeamService>>()
        .map(|state| state.inner().clone());
    let starter = Arc::new(AgentConversationAutomationRunStarter::new(
        state.clone(),
        Arc::clone(&execution_state),
        team_service,
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
    let wait_started_at = std::time::Instant::now();
    match wait_for_backend_ready(backend_port, Duration::from_secs(30)).await {
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
            match Arc::clone(&supervisor)
                .start(bootstrap.node_path, bootstrap.entry_path)
                .await
            {
                Ok(()) => {
                    let handle = app_handle.state::<ExternalMcpHandle>();
                    if handle.set(supervisor).is_err() {
                        warn!("ExternalMcpHandle already initialized");
                    } else {
                        info!(
                            supervisor_elapsed_ms = supervisor_started_at.elapsed().as_millis(),
                            elapsed_ms = started_at.elapsed().as_millis(),
                            "External MCP supervisor started and registered"
                        );
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

pub async fn startup_scan_verification_reconciliation(
    svc: Arc<
        crate::application::reconciliation::verification_reconciliation::VerificationReconciliationService,
    >,
    startup_ideation_recovery_claims: &HashSet<String>,
) {
    svc.startup_scan_excluding_external_archive_sessions(startup_ideation_recovery_claims)
        .await;
    tauri::async_runtime::spawn(async move { svc.run_periodic().await });
}

pub fn spawn_recovery_queue_processor(
    recovery_processor: crate::application::reconciliation::recovery_queue::RecoveryQueueProcessor,
) {
    tauri::async_runtime::spawn(async move {
        recovery_processor.run().await;
    });
}
