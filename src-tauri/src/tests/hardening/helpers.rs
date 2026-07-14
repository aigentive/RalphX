// Shared test helpers for hardening test suite
//
// Provides factory functions and builders for creating test fixtures.
// Reuses existing infrastructure: MemoryTaskRepository, MemoryProjectRepository,
// MockAgentSpawner, MockEventEmitter, MockChatService, etc.

use std::collections::HashSet;
use std::sync::Arc;

use crate::application::MockChatService;
use crate::commands::ExecutionState;
use crate::domain::entities::{GitMode, InternalStatus, Project, ProjectId, Task};
use crate::domain::state_machine::context::{TaskContext, TaskServices};
use crate::domain::state_machine::machine::{State, TaskStateMachine};
use crate::domain::state_machine::mocks::{
    MockAgentSpawner, MockDependencyManager, MockEventEmitter, MockNotifier, MockReviewStarter,
    MockTaskScheduler,
};
use crate::domain::state_machine::transition_handler::TransitionHandler;
use crate::infrastructure::memory::{
    MemoryPlanBranchRepository, MemoryProjectRepository, MemoryTaskRepository,
    MemoryTaskStepRepository,
};

/// Create a test task with sensible defaults.
/// Defaults: category="feature", status=Backlog, priority=0
pub fn create_test_task(project_id: &ProjectId, title: &str) -> Task {
    Task::new(project_id.clone(), title.to_string())
}

/// Create a test task with a specific status.
pub fn create_test_task_with_status(
    project_id: &ProjectId,
    title: &str,
    status: InternalStatus,
) -> Task {
    let mut task = Task::new(project_id.clone(), title.to_string());
    task.internal_status = status;
    task
}

/// Create a test project with default git mode (Worktree).
pub fn create_test_project(name: &str) -> Project {
    Project::new(name.to_string(), "/tmp/test-project".to_string())
}

/// Create a test project with a specific git mode.
pub fn create_test_project_with_git_mode(name: &str, git_mode: GitMode) -> Project {
    let mut project = Project::new(name.to_string(), "/tmp/test-project".to_string());
    project.git_mode = git_mode;
    project
}

/// All mock services bundled together for inspection in tests.
pub struct HardeningServices {
    pub task_repo: Arc<MemoryTaskRepository>,
    pub project_repo: Arc<MemoryProjectRepository>,
    pub plan_branch_repo: Arc<MemoryPlanBranchRepository>,
    pub step_repo: Arc<MemoryTaskStepRepository>,
    pub spawner: Arc<MockAgentSpawner>,
    pub emitter: Arc<MockEventEmitter>,
    pub notifier: Arc<MockNotifier>,
    pub dependency_manager: Arc<MockDependencyManager>,
    pub review_starter: Arc<MockReviewStarter>,
    pub chat_service: Arc<MockChatService>,
    pub scheduler: Arc<MockTaskScheduler>,
    pub execution_state: Arc<ExecutionState>,
    /// Shared merge lock — pass to both tasks to simulate real concurrent merge guard behavior.
    pub merge_lock: Arc<tokio::sync::Mutex<()>>,
    /// Shared merges_in_flight set — pass to both tasks for self-dedup guard tests.
    pub merges_in_flight: Arc<std::sync::Mutex<HashSet<String>>>,
}

/// Create a full set of mock services wired together for hardening tests.
pub fn create_hardening_services() -> HardeningServices {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    let step_repo = Arc::new(MemoryTaskStepRepository::new());
    let spawner = Arc::new(MockAgentSpawner::new());
    let emitter = Arc::new(MockEventEmitter::new());
    let notifier = Arc::new(MockNotifier::new());
    let dependency_manager = Arc::new(MockDependencyManager::new());
    let review_starter = Arc::new(MockReviewStarter::new());
    let chat_service = Arc::new(MockChatService::new());
    let scheduler = Arc::new(MockTaskScheduler::new());
    let execution_state = Arc::new(ExecutionState::with_max_concurrent(2));

    HardeningServices {
        task_repo,
        project_repo,
        plan_branch_repo,
        step_repo,
        spawner,
        emitter,
        notifier,
        dependency_manager,
        review_starter,
        chat_service,
        scheduler,
        execution_state,
        merge_lock: Arc::new(tokio::sync::Mutex::new(())),
        merges_in_flight: Arc::new(std::sync::Mutex::new(HashSet::new())),
    }
}

/// Build TaskServices from HardeningServices (consumes shared refs).
pub fn build_task_services(s: &HardeningServices) -> TaskServices {
    TaskServices::new(
        s.spawner.clone() as Arc<dyn crate::domain::state_machine::services::AgentSpawner>,
        s.emitter.clone() as Arc<dyn crate::domain::state_machine::services::EventEmitter>,
        s.notifier.clone() as Arc<dyn crate::domain::state_machine::services::Notifier>,
        s.dependency_manager.clone()
            as Arc<dyn crate::domain::state_machine::services::DependencyManager>,
        s.review_starter.clone() as Arc<dyn crate::domain::state_machine::services::ReviewStarter>,
        s.chat_service.clone() as Arc<dyn crate::application::ChatService>,
    )
    .with_execution_state(s.execution_state.clone())
    .with_task_scheduler(
        s.scheduler.clone() as Arc<dyn crate::domain::state_machine::services::TaskScheduler>
    )
    .with_task_repo(s.task_repo.clone() as Arc<dyn crate::domain::repositories::TaskRepository>)
    .with_project_repo(
        s.project_repo.clone() as Arc<dyn crate::domain::repositories::ProjectRepository>
    )
    .with_plan_branch_repo(
        s.plan_branch_repo.clone() as Arc<dyn crate::domain::repositories::PlanBranchRepository>
    )
    .with_step_repo(s.step_repo.clone() as Arc<dyn crate::domain::repositories::TaskStepRepository>)
    .with_merge_lock(Arc::clone(&s.merge_lock))
    .with_merges_in_flight(Arc::clone(&s.merges_in_flight))
}

/// Create a TaskStateMachine with the given context.
pub fn create_state_machine(
    task_id: &str,
    project_id: &str,
    services: TaskServices,
) -> TaskStateMachine {
    let context = TaskContext::new(task_id, project_id, services);
    TaskStateMachine { context }
}

/// Create a TransitionHandler wrapping a TaskStateMachine.
pub fn create_transition_handler(machine: &mut TaskStateMachine) -> TransitionHandler<'_> {
    TransitionHandler::new(machine)
}

/// Map InternalStatus to State (for state machine operations).
#[allow(dead_code)]
pub fn status_to_state(status: InternalStatus) -> State {
    match status {
        InternalStatus::Backlog => State::Backlog,
        InternalStatus::Ready => State::Ready,
        InternalStatus::Blocked => State::Blocked,
        InternalStatus::Executing => State::Executing,
        InternalStatus::QaRefining => State::QaRefining,
        InternalStatus::QaTesting => State::QaTesting,
        InternalStatus::QaPassed => State::QaPassed,
        InternalStatus::QaFailed => State::QaFailed(Default::default()),
        InternalStatus::PendingReview => State::PendingReview,
        InternalStatus::Reviewing => State::Reviewing,
        InternalStatus::ReviewPassed => State::ReviewPassed,
        InternalStatus::Escalated => State::Escalated,
        InternalStatus::RevisionNeeded => State::RevisionNeeded,
        InternalStatus::ReExecuting => State::ReExecuting,
        InternalStatus::Approved => State::Approved,
        InternalStatus::PendingMerge => State::PendingMerge,
        InternalStatus::Merging => State::Merging,
        InternalStatus::WaitingOnPr => State::WaitingOnPr,
        InternalStatus::MergeIncomplete => State::MergeIncomplete,
        InternalStatus::MergeConflict => State::MergeConflict,
        InternalStatus::Merged => State::Merged,
        InternalStatus::Failed => State::Failed(Default::default()),
        InternalStatus::Cancelled => State::Cancelled,
        InternalStatus::Paused => State::Paused,
        InternalStatus::Stopped => State::Stopped,
        InternalStatus::UpdatingPlanBranch => State::UpdatingPlanBranch,
        InternalStatus::UpdatingTaskBranch => State::UpdatingTaskBranch,
        InternalStatus::BranchUpdateBlocked => State::BranchUpdateBlocked,
    }
}
