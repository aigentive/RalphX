// Reconciliation runner for agent-active task states
//
// Ensures tasks don't get stuck when agent runs finish without transitions.
// Can be used on startup and during runtime polling.
//
// Submodules:
// - policy.rs: types + pure decision logic (RecoveryPolicy, RecoveryContext, etc.)
// - handlers/: reconcile_* methods, orchestration, apply_recovery_decision
//   - execution.rs: execution, review, QA, paused handlers + orchestration
//   - merge.rs: merge-specific handlers (Merging, PendingMerge, MergeIncomplete, MergeConflict)
// - metadata.rs: retry counters, SHA tracking, backoff delays
// - events.rs: evidence building, event recording, prompts, lookups

pub(crate) mod events;
pub(crate) mod handlers;
pub(crate) mod metadata;
pub(crate) mod policy;
pub mod recovery_queue;
pub mod verification_handoff;
pub mod verification_reconciliation;

use ralphx_events::{EventSink, NullEventSink};
use std::collections::HashMap;
use std::sync::Arc;
use tauri::AppHandle;
use tokio::sync::Mutex;

use crate::application::interactive_process_registry::InteractiveProcessRegistry;
use crate::application::runtime_factory::ChatRuntimeFactoryDeps;
use crate::application::{NotificationService, TaskTransitionService};
use crate::application::execution_state::ExecutionState;
use crate::domain::repositories::{
    ActivityEventRepository, AgentRunRepository, ArtifactRepository, BranchUpdateRepository,
    ChatAttachmentRepository, ChatConversationRepository, ChatMessageRepository,
    ExecutionSettingsRepository, IdeationSessionRepository, MemoryEventRepository,
    PlanBranchRepository, ProjectRepository, ReviewRepository, TaskDependencyRepository,
    TaskRepository,
};
use crate::domain::services::{MessageQueue, RunningAgentRegistry};

pub use policy::UserRecoveryAction;
#[doc(hidden)]
pub use policy::{
    RecoveryActionKind, RecoveryContext, RecoveryDecision, RecoveryEvidence, RecoveryPolicy,
};

pub struct ReconciliationRunner {
    pub(crate) task_repo: Arc<dyn TaskRepository>,
    pub(crate) task_dep_repo: Arc<dyn TaskDependencyRepository>,
    pub(crate) project_repo: Arc<dyn ProjectRepository>,
    pub(crate) artifact_repo: Arc<dyn ArtifactRepository>,
    pub(crate) chat_conversation_repo: Arc<dyn ChatConversationRepository>,
    pub(crate) chat_message_repo: Arc<dyn ChatMessageRepository>,
    pub(crate) chat_attachment_repo: Arc<dyn ChatAttachmentRepository>,
    pub(crate) ideation_session_repo: Arc<dyn IdeationSessionRepository>,
    pub(crate) activity_event_repo: Arc<dyn ActivityEventRepository>,
    pub(crate) message_queue: Arc<MessageQueue>,
    pub(crate) running_agent_registry: Arc<dyn RunningAgentRegistry>,
    pub(crate) memory_event_repo: Arc<dyn MemoryEventRepository>,
    pub(crate) agent_run_repo: Arc<dyn AgentRunRepository>,
    pub(crate) transition_service: Arc<TaskTransitionService>,
    pub(crate) execution_state: Arc<ExecutionState>,
    pub(crate) execution_settings_repo: Option<Arc<dyn ExecutionSettingsRepository>>,
    pub(crate) plan_branch_repo: Option<Arc<dyn PlanBranchRepository>>,
    pub(crate) branch_update_repo: Option<Arc<dyn BranchUpdateRepository>>,
    pub(crate) interactive_process_registry: Option<Arc<InteractiveProcessRegistry>>,
    pub(crate) review_repo: Option<Arc<dyn ReviewRepository>>,
    pub(crate) app_handle: Option<AppHandle>,
    pub(crate) events: Arc<dyn EventSink>,
    pub(crate) chat_runtime_deps: Option<ChatRuntimeFactoryDeps>,
    pub(crate) policy: RecoveryPolicy,
    /// Optional application notification effect, injected for non-Tauri runners and tests.
    pub(crate) notification_service: Option<Arc<NotificationService>>,
    /// Active recovery prompt key -> concrete prompt instance id.
    pub(crate) prompt_tracker: Arc<Mutex<HashMap<String, String>>>,
    /// PR poller registry for GitHub PR polling (own DI, separate from TaskServices — AD18).
    pub(crate) pr_poller_registry: Option<Arc<crate::application::PrPollerRegistry>>,
}

impl ReconciliationRunner {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        task_repo: Arc<dyn TaskRepository>,
        task_dep_repo: Arc<dyn TaskDependencyRepository>,
        project_repo: Arc<dyn ProjectRepository>,
        artifact_repo: Arc<dyn ArtifactRepository>,
        chat_conversation_repo: Arc<dyn ChatConversationRepository>,
        chat_message_repo: Arc<dyn ChatMessageRepository>,
        chat_attachment_repo: Arc<dyn ChatAttachmentRepository>,
        ideation_session_repo: Arc<dyn IdeationSessionRepository>,
        activity_event_repo: Arc<dyn ActivityEventRepository>,
        message_queue: Arc<MessageQueue>,
        running_agent_registry: Arc<dyn RunningAgentRegistry>,
        memory_event_repo: Arc<dyn MemoryEventRepository>,
        agent_run_repo: Arc<dyn AgentRunRepository>,
        transition_service: Arc<TaskTransitionService>,
        execution_state: Arc<ExecutionState>,
        app_handle: Option<AppHandle>,
    ) -> Self {
        Self {
            task_repo,
            task_dep_repo,
            project_repo,
            artifact_repo,
            chat_conversation_repo,
            chat_message_repo,
            chat_attachment_repo,
            ideation_session_repo,
            activity_event_repo,
            message_queue,
            running_agent_registry,
            memory_event_repo,
            agent_run_repo,
            transition_service,
            execution_state,
            execution_settings_repo: None,
            plan_branch_repo: None,
            branch_update_repo: None,
            interactive_process_registry: None,
            review_repo: None,
            app_handle,
            events: Arc::new(NullEventSink),
            chat_runtime_deps: None,
            policy: RecoveryPolicy,
            notification_service: None,
            prompt_tracker: Arc::new(Mutex::new(HashMap::new())),
            pr_poller_registry: None,
        }
    }

    pub fn with_app_handle(mut self, app_handle: AppHandle) -> Self {
        self.app_handle = Some(app_handle);
        self
    }

    pub fn with_events(mut self, events: Arc<dyn EventSink>) -> Self {
        self.events = events;
        self
    }

    pub(crate) fn with_chat_runtime_deps(mut self, deps: ChatRuntimeFactoryDeps) -> Self {
        self.chat_runtime_deps = Some(deps);
        self
    }

    pub fn with_notification_service(mut self, service: Arc<NotificationService>) -> Self {
        self.notification_service = Some(service);
        self
    }

    pub fn with_plan_branch_repo(mut self, repo: Arc<dyn PlanBranchRepository>) -> Self {
        self.plan_branch_repo = Some(repo);
        self
    }

    pub fn with_branch_update_repo(mut self, repo: Arc<dyn BranchUpdateRepository>) -> Self {
        self.branch_update_repo = Some(repo);
        self
    }

    pub fn with_execution_settings_repo(
        mut self,
        repo: Arc<dyn ExecutionSettingsRepository>,
    ) -> Self {
        self.execution_settings_repo = Some(repo);
        self
    }

    pub fn with_interactive_process_registry(
        mut self,
        registry: Arc<InteractiveProcessRegistry>,
    ) -> Self {
        self.interactive_process_registry = Some(registry);
        self
    }

    pub fn with_review_repo(
        mut self,
        repo: Arc<dyn crate::domain::repositories::ReviewRepository>,
    ) -> Self {
        self.review_repo = Some(repo);
        self
    }

    pub fn with_pr_poller_registry(
        mut self,
        registry: Arc<crate::application::PrPollerRegistry>,
    ) -> Self {
        self.pr_poller_registry = Some(registry);
        self
    }
}
