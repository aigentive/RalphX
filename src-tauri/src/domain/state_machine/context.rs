// TaskServices container and TaskContext for state machine
// These provide the shared context needed during state transitions

use super::mocks::{
    MockAgentSpawner, MockDependencyManager, MockEventEmitter, MockNotifier, MockReviewStarter,
    MockTaskScheduler,
};
use super::services::{
    AgentSpawner, BranchUpdateWorkflow, DependencyManager, EventEmitter, NotificationContext,
    Notifier, ReviewStarter, TaskScheduler, WebhookPublisher,
};
use super::types::Blocker;
use crate::application::ChatService;
use crate::application::PrPollerRegistry;
use crate::application::TaskTransitionService;
use crate::commands::ExecutionState;
use crate::domain::entities::PlanBranchId;
use crate::domain::repositories::{
    ActivityEventRepository, ArtifactRepository, BranchUpdateRepository, IdeationSessionRepository,
    PlanBranchRepository, ProjectRepository, TaskRepository, TaskStepRepository,
};
use crate::domain::services::github_service::GithubServiceTrait;
use crate::domain::services::PlanPrDescriptionDrafter;
use dashmap::DashMap;
use ralphx_domain::repositories::ExternalEventsRepository;
use ralphx_events::EventSink;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

/// Container for all services used by the state machine.
///
/// This is injected into the state machine and provides access to
/// external services like agent spawning, event emission, and notifications.
pub struct TaskServices {
    /// Service for spawning and managing AI agents
    pub agent_spawner: Arc<dyn AgentSpawner>,

    /// Service for emitting events to the frontend
    pub event_emitter: Arc<dyn EventEmitter>,

    /// Service for sending notifications to users
    pub notifier: Arc<dyn Notifier>,

    /// Context for the history row that authorized this entry action.
    pub notification_context: Option<NotificationContext>,

    /// Service for managing task dependencies
    pub dependency_manager: Arc<dyn DependencyManager>,

    /// Service for starting reviews on tasks
    pub review_starter: Arc<dyn ReviewStarter>,

    /// Unified chat service for worker execution (handles TaskExecution context).
    /// Worker spawning uses this service to persist output to database.
    pub chat_service: Arc<dyn ChatService>,

    /// Global execution state for tracking running task count.
    /// Used by TransitionHandler to decrement running count when exiting agent-active states.
    pub execution_state: Option<Arc<ExecutionState>>,

    /// Event sink for frontend/runtime events (optional).
    /// Used by TransitionHandler to emit execution and merge progress events.
    pub event_sink: Option<Arc<dyn EventSink>>,

    /// Task scheduler for auto-scheduling Ready tasks when slots are available.
    /// Used by TransitionHandler to trigger scheduling on slot free and on enter Ready.
    pub task_scheduler: Option<Arc<dyn TaskScheduler>>,

    /// Task repository for fetching and updating tasks during state transitions.
    /// Used by TransitionHandler to set task_branch and worktree_path on Executing entry.
    pub task_repo: Option<Arc<dyn TaskRepository>>,

    /// Durable branch-update and canonical Git target authority.
    pub branch_update_repo: Option<Arc<dyn BranchUpdateRepository>>,

    /// Application-owned branch-update effect adapter.
    pub branch_update_workflow: Option<Arc<dyn BranchUpdateWorkflow>>,

    /// Project repository for fetching project settings during state transitions.
    /// Used by TransitionHandler to get git_mode and worktree_parent_directory.
    pub project_repo: Option<Arc<dyn ProjectRepository>>,

    /// Plan branch repository for resolving feature branch targets during state transitions.
    /// Used by TransitionHandler to check if a task belongs to a plan with a feature branch.
    pub plan_branch_repo: Option<Arc<dyn PlanBranchRepository>>,

    /// Task step repository for updating step statuses during state transitions.
    /// Used by TransitionHandler to fail in-progress steps when task fails.
    pub step_repo: Option<Arc<dyn TaskStepRepository>>,

    /// Artifact repository for reading plan artifact content during PR generation.
    /// Optional — only needed for richer PR metadata/presentation.
    pub artifact_repo: Option<Arc<dyn ArtifactRepository>>,

    /// Application-level mutex for the concurrent merge guard critical section.
    /// Serializes the check-and-set in `try_programmatic_merge` so two tasks
    /// cannot both read "no blocker" and both proceed to merge simultaneously.
    /// Eliminates the TOCTOU race in the worktree-mode merge guard.
    pub merge_lock: Arc<Mutex<()>>,

    /// Set of task IDs that currently have an `attempt_programmatic_merge` call in flight.
    /// Prevents double-click / double-trigger from spawning two merge attempts for the same task.
    /// Uses std::sync::Mutex (not tokio) so Drop impls can clean up synchronously.
    pub merges_in_flight: Arc<std::sync::Mutex<HashSet<String>>>,

    /// Ideation session repository for fetching live session titles.
    /// Used by TransitionHandler to build descriptive plan merge commit messages.
    pub ideation_session_repo: Option<Arc<dyn IdeationSessionRepository>>,

    /// Task-keyed CancellationTokens for in-flight post-merge validations.
    /// Inserted in handle_outcome_success before validation, cancelled in
    /// pre_merge_cleanup when a new merge attempt starts for the same task.
    pub validation_tokens: Arc<DashMap<String, CancellationToken>>,

    /// Activity event repository for emitting merge pipeline audit events.
    /// Optional — if not wired, merge activity events are silently skipped.
    pub activity_event_repo: Option<Arc<dyn ActivityEventRepository>>,

    /// Registry of active GitHub PR polling tasks (AD18).
    /// Optional — None disables PR integration (tests, non-PR workflows).
    pub pr_poller_registry: Option<Arc<PrPollerRegistry>>,

    /// Optional service for generating PR descriptions for plan branches.
    /// When available, Ready-state plan PRs get reviewer-focused descriptions
    /// instead of empty template stubs.
    pub plan_pr_description_drafter: Option<Arc<dyn PlanPrDescriptionDrafter>>,

    /// Guard preventing duplicate draft PR creation per plan branch (AD10).
    /// Shared with PrPollerRegistry — same underlying DashMap.
    /// Optional — None disables PR creation guarding.
    pub pr_creation_guard: Option<Arc<DashMap<PlanBranchId, ()>>>,

    /// GitHub service for PR operations (create PR, push branch, mark ready, etc.).
    /// Optional — None when GitHub integration is disabled or in tests.
    pub github_service: Option<Arc<dyn GithubServiceTrait>>,

    /// Task transition service for triggering state transitions from within state machine actions.
    /// Used by PR merge poller (started in on_enter(Merging)) to fire Merging → Merged when
    /// GitHub reports the PR as merged. Optional — None when not wired (tests, non-PR paths).
    pub transition_service: Option<Arc<TaskTransitionService>>,

    /// Webhook publisher for broadcasting events to registered external endpoints.
    /// Optional — None when webhook delivery is not configured.
    pub webhook_publisher: Option<Arc<dyn WebhookPublisher>>,

    /// External events repository for persisting lifecycle events to the external_events table.
    /// Optional — None when external event persistence is not configured.
    pub external_events_repo: Option<Arc<dyn ExternalEventsRepository>>,

    /// Per-session mutex map for serializing concurrent plan:delivered checks.
    /// Shared across all TaskServices instances so concurrent PlanMerge completions
    /// in the same session cannot race on the all-merged check and fire plan:delivered twice.
    /// ONE shared Arc across both Tauri IPC and HTTP server paths.
    pub session_merge_locks: Arc<DashMap<String, Arc<Mutex<()>>>>,
}

impl TaskServices {
    /// Creates a new TaskServices with the given service implementations
    pub fn new(
        agent_spawner: Arc<dyn AgentSpawner>,
        event_emitter: Arc<dyn EventEmitter>,
        notifier: Arc<dyn Notifier>,
        dependency_manager: Arc<dyn DependencyManager>,
        review_starter: Arc<dyn ReviewStarter>,
        chat_service: Arc<dyn ChatService>,
    ) -> Self {
        Self {
            agent_spawner,
            event_emitter,
            notifier,
            notification_context: None,
            dependency_manager,
            review_starter,
            chat_service,
            execution_state: None,
            event_sink: None,
            task_scheduler: None,
            task_repo: None,
            branch_update_repo: None,
            branch_update_workflow: None,
            project_repo: None,
            plan_branch_repo: None,
            step_repo: None,
            artifact_repo: None,
            merge_lock: Arc::new(Mutex::new(())),
            merges_in_flight: Arc::new(std::sync::Mutex::new(HashSet::new())),
            ideation_session_repo: None,
            validation_tokens: Arc::new(DashMap::new()),
            activity_event_repo: None,
            pr_poller_registry: None,
            plan_pr_description_drafter: None,
            pr_creation_guard: None,
            github_service: None,
            transition_service: None,
            webhook_publisher: None,
            external_events_repo: None,
            session_merge_locks: Arc::new(DashMap::new()),
        }
    }

    /// Set the execution state (builder pattern)
    pub fn with_execution_state(mut self, state: Arc<ExecutionState>) -> Self {
        self.execution_state = Some(state);
        self
    }

    pub fn with_notification_context(mut self, context: NotificationContext) -> Self {
        self.notification_context = Some(context);
        self
    }

    /// Set the event sink for frontend/runtime event emission (builder pattern)
    pub fn with_event_sink(mut self, sink: Arc<dyn EventSink>) -> Self {
        self.event_sink = Some(sink);
        self
    }

    /// Set the task scheduler (builder pattern)
    pub fn with_task_scheduler(mut self, scheduler: Arc<dyn TaskScheduler>) -> Self {
        self.task_scheduler = Some(scheduler);
        self
    }

    /// Replace the chat service (builder pattern).
    /// Useful in tests to inject a custom MockChatService after `new_mock()`.
    pub fn with_chat_service(mut self, svc: Arc<dyn ChatService>) -> Self {
        self.chat_service = svc;
        self
    }

    /// Set the task repository (builder pattern)
    pub fn with_task_repo(mut self, repo: Arc<dyn TaskRepository>) -> Self {
        self.task_repo = Some(repo);
        self
    }

    pub fn with_branch_update_repo(mut self, repo: Arc<dyn BranchUpdateRepository>) -> Self {
        self.branch_update_repo = Some(repo);
        self
    }

    pub fn with_branch_update_workflow(mut self, workflow: Arc<dyn BranchUpdateWorkflow>) -> Self {
        self.branch_update_workflow = Some(workflow);
        self
    }

    /// Set the project repository (builder pattern)
    pub fn with_project_repo(mut self, repo: Arc<dyn ProjectRepository>) -> Self {
        self.project_repo = Some(repo);
        self
    }

    /// Set the plan branch repository (builder pattern)
    pub fn with_plan_branch_repo(mut self, repo: Arc<dyn PlanBranchRepository>) -> Self {
        self.plan_branch_repo = Some(repo);
        self
    }

    /// Set the step repository (builder pattern)
    pub fn with_step_repo(mut self, repo: Arc<dyn TaskStepRepository>) -> Self {
        self.step_repo = Some(repo);
        self
    }

    /// Set the artifact repository (builder pattern)
    pub fn with_artifact_repo(mut self, repo: Arc<dyn ArtifactRepository>) -> Self {
        self.artifact_repo = Some(repo);
        self
    }

    /// Set a shared merge lock (builder pattern).
    /// Use this to share the same mutex across multiple TaskServices instances
    /// (e.g., when two tasks run concurrently in the same process).
    pub fn with_merge_lock(mut self, lock: Arc<Mutex<()>>) -> Self {
        self.merge_lock = lock;
        self
    }

    /// Set a shared merges_in_flight set (builder pattern).
    /// Use this to share the same dedup set across multiple TaskServices instances.
    pub fn with_merges_in_flight(mut self, set: Arc<std::sync::Mutex<HashSet<String>>>) -> Self {
        self.merges_in_flight = set;
        self
    }

    /// Set the ideation session repository (builder pattern)
    pub fn with_ideation_session_repo(mut self, repo: Arc<dyn IdeationSessionRepository>) -> Self {
        self.ideation_session_repo = Some(repo);
        self
    }

    /// Set shared validation tokens DashMap (builder pattern).
    /// Use this to share tokens across multiple TaskServices instances.
    pub fn with_validation_tokens(
        mut self,
        tokens: Arc<DashMap<String, CancellationToken>>,
    ) -> Self {
        self.validation_tokens = tokens;
        self
    }

    /// Set the activity event repository for merge pipeline audit events (builder pattern).
    pub fn with_activity_event_repo(mut self, repo: Arc<dyn ActivityEventRepository>) -> Self {
        self.activity_event_repo = Some(repo);
        self
    }

    /// Set the PR poller registry for GitHub PR polling (builder pattern).
    pub fn with_pr_poller_registry(mut self, registry: Arc<PrPollerRegistry>) -> Self {
        self.pr_poller_registry = Some(registry);
        self
    }

    pub fn with_plan_pr_description_drafter(
        mut self,
        drafter: Arc<dyn PlanPrDescriptionDrafter>,
    ) -> Self {
        self.plan_pr_description_drafter = Some(drafter);
        self
    }

    /// Set the PR creation guard DashMap (builder pattern).
    /// Should be the same Arc as PrPollerRegistry::pr_creation_guard.
    pub fn with_pr_creation_guard(mut self, guard: Arc<DashMap<PlanBranchId, ()>>) -> Self {
        self.pr_creation_guard = Some(guard);
        self
    }

    /// Set the GitHub service for PR integration (builder pattern).
    pub fn with_github_service(mut self, svc: Arc<dyn GithubServiceTrait>) -> Self {
        self.github_service = Some(svc);
        self
    }

    /// Set the task transition service for PR merge poller (builder pattern).
    /// The poller uses this to fire Merging → Merged when GitHub reports the PR as merged.
    pub fn with_transition_service(mut self, svc: Arc<TaskTransitionService>) -> Self {
        self.transition_service = Some(svc);
        self
    }

    /// Set the webhook publisher for broadcasting events to external endpoints (builder pattern).
    pub fn with_webhook_publisher(mut self, publisher: Arc<dyn WebhookPublisher>) -> Self {
        self.webhook_publisher = Some(publisher);
        self
    }

    /// Set the external events repository for persisting lifecycle events (builder pattern).
    pub fn with_external_events_repo(mut self, repo: Arc<dyn ExternalEventsRepository>) -> Self {
        self.external_events_repo = Some(repo);
        self
    }

    /// Set the shared session merge locks DashMap (builder pattern).
    /// Use this to share the same map across multiple TaskServices instances.
    pub fn with_session_merge_locks(mut self, locks: Arc<DashMap<String, Arc<Mutex<()>>>>) -> Self {
        self.session_merge_locks = locks;
        self
    }

    /// Creates a TaskServices with all mock implementations for testing
    pub fn new_mock() -> Self {
        use crate::application::MockChatService;

        Self {
            agent_spawner: Arc::new(MockAgentSpawner::new()),
            event_emitter: Arc::new(MockEventEmitter::new()),
            notifier: Arc::new(MockNotifier::new()),
            notification_context: None,
            dependency_manager: Arc::new(MockDependencyManager::new()),
            review_starter: Arc::new(MockReviewStarter::new()),
            chat_service: Arc::new(MockChatService::new()),
            execution_state: None,
            event_sink: None,
            task_scheduler: Some(Arc::new(MockTaskScheduler::new())),
            task_repo: None,
            branch_update_repo: None,
            branch_update_workflow: None,
            project_repo: None,
            plan_branch_repo: None,
            step_repo: None,
            artifact_repo: None,
            merge_lock: Arc::new(Mutex::new(())),
            merges_in_flight: Arc::new(std::sync::Mutex::new(HashSet::new())),
            ideation_session_repo: None,
            validation_tokens: Arc::new(DashMap::new()),
            activity_event_repo: None,
            pr_poller_registry: None,
            plan_pr_description_drafter: None,
            pr_creation_guard: None,
            github_service: None,
            transition_service: None,
            webhook_publisher: None,
            external_events_repo: None,
            session_merge_locks: Arc::new(DashMap::new()),
        }
    }
}

impl std::fmt::Debug for TaskServices {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TaskServices")
            .field("agent_spawner", &"<AgentSpawner>")
            .field("event_emitter", &"<EventEmitter>")
            .field("notifier", &"<Notifier>")
            .field("dependency_manager", &"<DependencyManager>")
            .field("review_starter", &"<ReviewStarter>")
            .field("chat_service", &"<ChatService>")
            .field(
                "execution_state",
                &self.execution_state.as_ref().map(|_| "<ExecutionState>"),
            )
            .field(
                "event_sink",
                &self.event_sink.as_ref().map(|_| "<EventSink>"),
            )
            .field(
                "task_scheduler",
                &self.task_scheduler.as_ref().map(|_| "<TaskScheduler>"),
            )
            .field(
                "task_repo",
                &self.task_repo.as_ref().map(|_| "<TaskRepository>"),
            )
            .field(
                "project_repo",
                &self.project_repo.as_ref().map(|_| "<ProjectRepository>"),
            )
            .field(
                "plan_branch_repo",
                &self
                    .plan_branch_repo
                    .as_ref()
                    .map(|_| "<PlanBranchRepository>"),
            )
            .field(
                "artifact_repo",
                &self.artifact_repo.as_ref().map(|_| "<ArtifactRepository>"),
            )
            .field("merge_lock", &"<Mutex<()>>")
            .field("merges_in_flight", &"<Mutex<HashSet<String>>>")
            .field(
                "validation_tokens",
                &format!("<DashMap len={}>", self.validation_tokens.len()),
            )
            .field(
                "ideation_session_repo",
                &self
                    .ideation_session_repo
                    .as_ref()
                    .map(|_| "<IdeationSessionRepository>"),
            )
            .field(
                "activity_event_repo",
                &self
                    .activity_event_repo
                    .as_ref()
                    .map(|_| "<ActivityEventRepository>"),
            )
            .field(
                "pr_poller_registry",
                &self
                    .pr_poller_registry
                    .as_ref()
                    .map(|_| "<PrPollerRegistry>"),
            )
            .field(
                "pr_creation_guard",
                &self
                    .pr_creation_guard
                    .as_ref()
                    .map(|g| format!("<DashMap len={}>", g.len())),
            )
            .field(
                "github_service",
                &self.github_service.as_ref().map(|_| "<GithubService>"),
            )
            .field(
                "transition_service",
                &self
                    .transition_service
                    .as_ref()
                    .map(|_| "<TaskTransitionService>"),
            )
            .field(
                "webhook_publisher",
                &self
                    .webhook_publisher
                    .as_ref()
                    .map(|_| "<WebhookPublisher>"),
            )
            .field(
                "external_events_repo",
                &self
                    .external_events_repo
                    .as_ref()
                    .map(|_| "<ExternalEventsRepository>"),
            )
            .field(
                "session_merge_locks",
                &format!("<DashMap len={}>", self.session_merge_locks.len()),
            )
            .finish()
    }
}

// ============================================================================
// TaskContext - Container for task-specific state machine context
// ============================================================================

/// Runtime context for a specific task's state machine.
///
/// Each task has its own TaskContext that holds:
/// - The task's ID and project ID for database operations
/// - Shared services for performing actions
/// - Task-specific state like blockers
pub struct TaskContext {
    /// ID of the task being processed
    pub task_id: String,

    /// ID of the project this task belongs to
    pub project_id: String,

    /// Shared services for state machine actions
    pub services: TaskServices,

    /// Current blockers preventing task progress
    pub blockers: Vec<Blocker>,

    /// Whether QA is enabled for this task's workflow
    pub qa_enabled: bool,

    /// Whether QA prep has completed (used to skip wait_for in QaRefining)
    pub qa_prep_complete: bool,

    /// Feedback from review (used when transitioning to RevisionNeeded)
    pub review_feedback: Option<String>,

    /// Error message from failed execution or QA
    pub error: Option<String>,
}

impl TaskContext {
    /// Creates a new TaskContext for a task
    pub fn new(task_id: &str, project_id: &str, services: TaskServices) -> Self {
        Self {
            task_id: task_id.to_string(),
            project_id: project_id.to_string(),
            services,
            blockers: Vec::new(),
            qa_enabled: false,
            qa_prep_complete: false,
            review_feedback: None,
            error: None,
        }
    }

    /// Creates a TaskContext with mock services for testing
    pub fn new_test(task_id: &str, project_id: &str) -> Self {
        Self::new(task_id, project_id, TaskServices::new_mock())
    }

    /// Enable QA for this context (builder pattern for tests)
    pub fn with_qa_enabled(mut self) -> Self {
        self.qa_enabled = true;
        self
    }

    /// Mark QA prep as complete (builder pattern for tests)
    pub fn with_qa_prep_complete(mut self) -> Self {
        self.qa_prep_complete = true;
        self
    }

    /// Add a blocker to this context
    pub fn add_blocker(&mut self, blocker: Blocker) {
        self.blockers.push(blocker);
    }

    /// Clear all blockers
    pub fn clear_blockers(&mut self) {
        self.blockers.clear();
    }

    /// Resolve all blockers (alias for clear_blockers)
    pub fn resolve_all_blockers(&mut self) {
        self.blockers.clear();
    }

    /// Check if there are any blockers
    pub fn has_blockers(&self) -> bool {
        !self.blockers.is_empty()
    }

    /// Clear the review feedback
    pub fn clear_review_feedback(&mut self) {
        self.review_feedback = None;
    }

    /// Clear the error message
    pub fn clear_error(&mut self) {
        self.error = None;
    }
}

impl std::fmt::Debug for TaskContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TaskContext")
            .field("task_id", &self.task_id)
            .field("project_id", &self.project_id)
            .field("services", &self.services)
            .field("blockers", &self.blockers)
            .field("qa_enabled", &self.qa_enabled)
            .field("qa_prep_complete", &self.qa_prep_complete)
            .field("review_feedback", &self.review_feedback)
            .field("error", &self.error)
            .finish()
    }
}

#[cfg(test)]
#[path = "context_tests.rs"]
mod tests;
