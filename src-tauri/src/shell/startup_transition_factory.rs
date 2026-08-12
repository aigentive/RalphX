use std::sync::Arc;
use std::time::Instant;

use crate::application::runtime_factory::{build_transition_service_from_deps, RuntimeFactoryDeps};
use crate::application::task_notification_producer::TaskPipelineNotificationProducer;
use crate::application::task_transition_service::TaskTransitionService;
use crate::application::InteractiveProcessRegistry;
use crate::application::{AgentClientBundle, NotificationService};
use crate::application::execution_state::ExecutionState;
use crate::domain::repositories::{
    AgentLaneSettingsRepository, AgentProviderSettingsRepository, ExecutionSettingsRepository,
    ExternalEventsRepository, PlanBranchRepository, TaskStepRepository,
};
use crate::domain::state_machine::services::{TaskScheduler, WebhookPublisher};

pub struct StartupTransitionFactory {
    pub execution_state: Arc<ExecutionState>,
    pub execution_settings_repo: Arc<dyn ExecutionSettingsRepository>,
    pub agent_lane_settings_repo: Arc<dyn AgentLaneSettingsRepository>,
    pub agent_provider_settings_repo: Arc<dyn AgentProviderSettingsRepository>,
    pub plan_branch_repo: Arc<dyn PlanBranchRepository>,
    pub interactive_process_registry: Arc<InteractiveProcessRegistry>,
    pub agent_clients: AgentClientBundle,
    pub task_scheduler: Arc<dyn TaskScheduler>,
    pub step_repo: Arc<dyn TaskStepRepository>,
    pub external_events_repo: Arc<dyn ExternalEventsRepository>,
    pub webhook_publisher: Option<Arc<dyn WebhookPublisher>>,
    pub session_merge_locks: Arc<dashmap::DashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    pub notification_service: Arc<NotificationService>,
}

impl StartupTransitionFactory {
    fn log_build_step(step: &'static str, started_at: Instant) {
        tracing::info!(
            step,
            elapsed_ms = started_at.elapsed().as_millis(),
            "Startup transition service build step completed"
        );
    }

    pub(crate) fn build(
        &self,
        mut deps: RuntimeFactoryDeps,
        app_handle: tauri::AppHandle,
    ) -> TaskTransitionService {
        let started_at = Instant::now();
        deps.agent_clients = Some(self.agent_clients.clone());
        deps.execution_settings_repo = Some(Arc::clone(&self.execution_settings_repo));
        deps.agent_lane_settings_repo = Some(Arc::clone(&self.agent_lane_settings_repo));
        deps.agent_provider_settings_repo = Some(Arc::clone(&self.agent_provider_settings_repo));
        deps.plan_branch_repo = Some(Arc::clone(&self.plan_branch_repo));
        deps.interactive_process_registry = Some(Arc::clone(&self.interactive_process_registry));
        Self::log_build_step("startup_transition_deps_overlay", started_at);

        let started_at = Instant::now();
        let mut service = build_transition_service_from_deps(
            Some(app_handle),
            Arc::clone(&self.execution_state),
            &deps,
        );
        service = service.with_notifier(Arc::new(TaskPipelineNotificationProducer::new(
            Arc::clone(&self.notification_service),
        )));
        Self::log_build_step("startup_transition_base_service", started_at);

        let started_at = Instant::now();
        service = service
            .with_task_scheduler(Arc::clone(&self.task_scheduler))
            .with_step_repo(Arc::clone(&self.step_repo))
            .with_external_events_repo(Arc::clone(&self.external_events_repo))
            .with_session_merge_locks(Arc::clone(&self.session_merge_locks));
        Self::log_build_step("startup_transition_startup_wiring", started_at);

        if let Some(ref publisher) = self.webhook_publisher {
            let started_at = Instant::now();
            service = service.with_webhook_publisher_for_emitter(Arc::clone(publisher));
            Self::log_build_step("startup_transition_webhook_wiring", started_at);
        }

        service
    }
}
