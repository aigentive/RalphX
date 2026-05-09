use std::sync::Arc;

use crate::application::AgentClientBundle;
use crate::application::InteractiveProcessRegistry;
use crate::application::runtime_factory::{
    RuntimeFactoryDeps, build_transition_service_with_fallback,
};
use crate::application::task_transition_service::TaskTransitionService;
use crate::commands::ExecutionState;
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
}

impl StartupTransitionFactory {
    pub(crate) fn build(
        &self,
        mut deps: RuntimeFactoryDeps,
        app_handle: tauri::AppHandle,
    ) -> TaskTransitionService {
        deps.agent_clients = Some(self.agent_clients.clone());
        deps.execution_settings_repo = Some(Arc::clone(&self.execution_settings_repo));
        deps.agent_lane_settings_repo = Some(Arc::clone(&self.agent_lane_settings_repo));
        deps.agent_provider_settings_repo = Some(Arc::clone(&self.agent_provider_settings_repo));
        deps.plan_branch_repo = Some(Arc::clone(&self.plan_branch_repo));
        deps.interactive_process_registry = Some(Arc::clone(&self.interactive_process_registry));

        let mut service = build_transition_service_with_fallback(
            &Some(app_handle),
            Arc::clone(&self.execution_state),
            &deps,
        )
        .with_task_scheduler(Arc::clone(&self.task_scheduler))
        .with_step_repo(Arc::clone(&self.step_repo))
        .with_external_events_repo(Arc::clone(&self.external_events_repo))
        .with_session_merge_locks(Arc::clone(&self.session_merge_locks));

        if let Some(ref publisher) = self.webhook_publisher {
            service = service.with_webhook_publisher_for_emitter(Arc::clone(publisher));
        }

        service
    }
}
