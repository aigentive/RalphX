// Shared test helpers for transition_handler tests
//
// Consolidated from side_effects.rs and tests.rs test infrastructure.
// All test files should `use super::helpers::*;` to access these.

pub use crate::application::{ChatService, MockChatService};
use crate::domain::entities::types::IdeationSessionId;
use crate::domain::entities::{
    ArtifactId, IdeationSession, IdeationSessionStatus, InternalStatus, PlanBranch,
    PlanBranchStatus, Project, ProjectId, Task, TaskCategory, TaskId,
};
pub use crate::domain::repositories::{ProjectRepository, TaskRepository};
pub use crate::domain::state_machine::context::{TaskContext, TaskServices};
pub use crate::domain::state_machine::mocks::{
    MockAgentSpawner, MockDependencyManager, MockEventEmitter, MockNotifier, MockReviewStarter,
};
pub use crate::domain::state_machine::services::{
    AgentSpawner, DependencyManager, EventEmitter, Notifier, ReviewStarter,
};
pub use crate::infrastructure::memory::{MemoryProjectRepository, MemoryTaskRepository};
pub use std::sync::Arc;
use std::time::Duration;

// ==================
// Entity factories (from side_effects.rs tests)
// ==================

pub fn make_project(base_branch: Option<&str>) -> Project {
    let mut p = Project::new("test-project".into(), "/tmp/test".into());
    p.base_branch = base_branch.map(|s| s.to_string());
    p.worktree_parent_directory = Some("/tmp/test/worktrees".into());
    p
}

pub fn make_real_git_project(repo_path: &str) -> Project {
    let mut project = Project::new("test-project".to_string(), repo_path.to_string());
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(
        std::path::Path::new(repo_path)
            .join("worktrees")
            .to_string_lossy()
            .to_string(),
    );
    project
}

pub fn make_task(plan_artifact_id: Option<&str>, task_branch: Option<&str>) -> Task {
    make_task_with_session(plan_artifact_id, task_branch, None)
}

pub fn make_task_with_session(
    plan_artifact_id: Option<&str>,
    task_branch: Option<&str>,
    ideation_session_id: Option<&str>,
) -> Task {
    let mut t = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        "Test task".into(),
    );
    t.plan_artifact_id = plan_artifact_id.map(|s| ArtifactId::from_string(s));
    t.task_branch = task_branch.map(|s| s.to_string());
    t.ideation_session_id = ideation_session_id.map(|s| IdeationSessionId::from_string(s));
    t
}

pub fn make_plan_branch(
    plan_artifact_id: &str,
    branch_name: &str,
    status: PlanBranchStatus,
    merge_task_id: Option<&str>,
) -> PlanBranch {
    let mut pb = PlanBranch::new(
        ArtifactId::from_string(plan_artifact_id),
        IdeationSessionId::from_string("sess-1"),
        ProjectId::from_string("proj-1".to_string()),
        branch_name.to_string(),
        "main".to_string(),
    );
    pb.status = status;
    pb.merge_task_id = merge_task_id.map(|s| TaskId::from_string(s.to_string()));
    pb
}

pub fn make_task_with_status(task_id: &str, status: InternalStatus) -> Task {
    let mut t = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        "Test task".into(),
    );
    t.id = TaskId::from_string(task_id.to_string());
    t.internal_status = status;
    t
}

pub fn make_task_with_category(category: TaskCategory) -> Task {
    let mut t = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        format!("Task with category {}", category),
    );
    t.category = category;
    t
}

pub fn make_session_no_title(session_id: &str) -> IdeationSession {
    let id = IdeationSessionId::from_string(session_id.to_string());
    IdeationSession {
        id,
        project_id: ProjectId::from_string("proj-1".to_string()),
        title: None,
        status: IdeationSessionStatus::default(),
        plan_artifact_id: None,
        plan_blueprint_artifact_id: None,
        inherited_plan_artifact_id: None,
        inherited_plan_blueprint_artifact_id: None,
        verified_plan_artifact_id: None,
        verified_plan_blueprint_artifact_id: None,
        verified_plan_agent_run_id: None,
        seed_task_id: None,
        parent_session_id: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        archived_at: None,
        converted_at: None,
        title_source: None,
        verification_status: Default::default(),
        verification_in_progress: false,
        verification_generation: 0,
        verification_current_round: None,
        verification_max_rounds: None,
        verification_gap_count: 0,
        verification_gap_score: None,
        verification_convergence_reason: None,
        source_project_id: None,
        source_session_id: None,
        source_task_id: None,
        source_context_type: None,
        source_context_id: None,
        spawn_reason: None,
        blocker_fingerprint: None,
        session_purpose: Default::default(),
        session_flow: Default::default(),
        cross_project_checked: true,
        plan_version_last_read: None,
        blueprint_version_last_read: None,
        plan_contract_version: 1,
        origin: Default::default(),
        expected_proposal_count: None,
        auto_accept_status: None,
        auto_accept_started_at: None,
        api_key_id: None,
        idempotency_key: None,
        external_activity_phase: None,
        external_last_read_message_id: None,
        dependencies_acknowledged: false,
        pending_initial_prompt: None,
        acceptance_status: None,
        verification_confirmation_status: None,
        analysis: Default::default(),
        last_effective_model: None,
    }
}

pub fn make_session_with_title_for_test(session_id: &str, title: &str) -> IdeationSession {
    let id = IdeationSessionId::from_string(session_id.to_string());
    IdeationSession {
        id,
        project_id: ProjectId::from_string("proj-1".to_string()),
        title: Some(title.to_string()),
        status: IdeationSessionStatus::default(),
        plan_artifact_id: None,
        plan_blueprint_artifact_id: None,
        inherited_plan_artifact_id: None,
        inherited_plan_blueprint_artifact_id: None,
        verified_plan_artifact_id: None,
        verified_plan_blueprint_artifact_id: None,
        verified_plan_agent_run_id: None,
        seed_task_id: None,
        parent_session_id: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        archived_at: None,
        converted_at: None,
        title_source: None,
        verification_status: Default::default(),
        verification_in_progress: false,
        verification_generation: 0,
        verification_current_round: None,
        verification_max_rounds: None,
        verification_gap_count: 0,
        verification_gap_score: None,
        verification_convergence_reason: None,
        source_project_id: None,
        source_session_id: None,
        source_task_id: None,
        source_context_type: None,
        source_context_id: None,
        spawn_reason: None,
        blocker_fingerprint: None,
        session_purpose: Default::default(),
        session_flow: Default::default(),
        cross_project_checked: true,
        plan_version_last_read: None,
        blueprint_version_last_read: None,
        plan_contract_version: 1,
        origin: Default::default(),
        expected_proposal_count: None,
        auto_accept_status: None,
        auto_accept_started_at: None,
        api_key_id: None,
        idempotency_key: None,
        external_activity_phase: None,
        external_last_read_message_id: None,
        dependencies_acknowledged: false,
        pending_initial_prompt: None,
        acceptance_status: None,
        verification_confirmation_status: None,
        analysis: Default::default(),
        last_effective_model: None,
    }
}

pub fn make_plan_task(session_id_str: &str, title: &str) -> Task {
    let mut t = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        title.to_string(),
    );
    t.ideation_session_id = Some(IdeationSessionId::from_string(session_id_str.to_string()));
    t
}

// ==================
// Service factories (from tests.rs)
// ==================

pub fn create_test_services() -> (
    Arc<MockAgentSpawner>,
    Arc<MockEventEmitter>,
    Arc<MockNotifier>,
    Arc<MockDependencyManager>,
    Arc<MockReviewStarter>,
    TaskServices,
) {
    let spawner = Arc::new(MockAgentSpawner::new());
    let emitter = Arc::new(MockEventEmitter::new());
    let notifier = Arc::new(MockNotifier::new());
    let dep_manager = Arc::new(MockDependencyManager::new());
    let review_starter = Arc::new(MockReviewStarter::new());
    let chat_service = Arc::new(MockChatService::new());

    let services = TaskServices::new(
        Arc::clone(&spawner) as Arc<dyn AgentSpawner>,
        Arc::clone(&emitter) as Arc<dyn EventEmitter>,
        Arc::clone(&notifier) as Arc<dyn Notifier>,
        Arc::clone(&dep_manager) as Arc<dyn DependencyManager>,
        Arc::clone(&review_starter) as Arc<dyn ReviewStarter>,
        Arc::clone(&chat_service) as Arc<dyn ChatService>,
    )
    .with_branch_update_repo(crate::testing::memory_branch_update_repository())
    .with_branch_update_workflow(crate::testing::branch_update_workflow(
        Arc::clone(&chat_service) as Arc<dyn ChatService>,
    ));

    (
        spawner,
        emitter,
        notifier,
        dep_manager,
        review_starter,
        services,
    )
}

pub fn create_context_with_services(
    task_id: &str,
    project_id: &str,
    services: TaskServices,
) -> TaskContext {
    TaskContext::new(task_id, project_id, services)
}

/// Attach the same durable branch-update authority required by production
/// transition factories to a bespoke test service graph.
pub fn with_test_branch_update_authority(
    services: TaskServices,
    task_repo: Arc<dyn TaskRepository>,
    chat_service: Arc<dyn ChatService>,
) -> TaskServices {
    services
        .with_branch_update_repo(
            crate::testing::memory_branch_update_repository_with_task_repository(task_repo),
        )
        .with_branch_update_workflow(crate::testing::branch_update_workflow(chat_service))
}

pub fn with_default_test_branch_update_authority(
    services: TaskServices,
    task_repo: Arc<dyn TaskRepository>,
) -> TaskServices {
    with_test_branch_update_authority(services, task_repo, Arc::new(MockChatService::new()))
}

pub use crate::domain::state_machine::mocks::MockTaskScheduler;
pub use crate::domain::state_machine::TaskStateMachine;

/// Create a `TaskStateMachine` wired with a `MockTaskScheduler`.
///
/// Returns `(machine, scheduler)` — caller creates `TransitionHandler::new(&mut machine)`.
/// Covers the most common merge-test pattern (scheduler + default mocks).
pub fn new_machine_with_scheduler(
    task_id: &str,
    project_id: &str,
) -> (TaskStateMachine, Arc<MockTaskScheduler>) {
    let scheduler = Arc::new(MockTaskScheduler::new());
    let services = TaskServices::new_mock()
        .with_branch_update_repo(crate::testing::memory_branch_update_repository())
        .with_branch_update_workflow(crate::testing::branch_update_workflow(Arc::new(
            MockChatService::new(),
        )))
        .with_task_scheduler(Arc::clone(&scheduler)
            as Arc<dyn crate::domain::state_machine::services::TaskScheduler>);
    let context = create_context_with_services(task_id, project_id, services);
    (TaskStateMachine::new(context), scheduler)
}

/// Return type for `setup_pending_merge_repos`.
pub struct PendingMergeSetup {
    pub task_id: TaskId,
    pub task_repo: Arc<MemoryTaskRepository>,
    pub project_repo: Arc<MemoryProjectRepository>,
}

impl PendingMergeSetup {
    /// Build a `TaskStateMachine` with repos wired into default mock services.
    ///
    /// Returns (machine, task_repo, task_id) so callers can query post-test state.
    /// Most merge-path tests just need `let handler = TransitionHandler::new(&mut machine)`.
    pub fn into_machine(self) -> (TaskStateMachine, Arc<MemoryTaskRepository>, TaskId) {
        let task_id = self.task_id.clone();
        let services = TaskServices::new_mock()
            .with_branch_update_repo(
                crate::testing::memory_branch_update_repository_with_task_repository(Arc::clone(
                    &self.task_repo,
                )
                    as Arc<dyn TaskRepository>),
            )
            .with_branch_update_workflow(crate::testing::branch_update_workflow(Arc::new(
                MockChatService::new(),
            )))
            .with_task_repo(Arc::clone(&self.task_repo) as Arc<dyn TaskRepository>)
            .with_project_repo(Arc::clone(&self.project_repo) as Arc<dyn ProjectRepository>);
        let context = TaskContext::new(self.task_id.as_str(), "proj-1", services);
        (TaskStateMachine::new(context), self.task_repo, task_id)
    }
}

/// Create in-memory repos pre-loaded with a task in PendingMerge and a project
/// pointing to a nonexistent git directory.
///
/// For the common case (default mock services), call `.into_machine()`:
/// ```ignore
/// let (mut machine, task_repo) = setup_pending_merge_repos("test", Some("feature/x"))
///     .await.into_machine();
/// let handler = TransitionHandler::new(&mut machine);
/// ```
///
/// For custom services (e.g. wiring a scheduler), use the fields directly:
/// ```ignore
/// let setup = setup_pending_merge_repos("test", Some("feature/x")).await;
/// let services = TaskServices::new_mock()
///     .with_task_repo(Arc::clone(&setup.task_repo) as Arc<dyn TaskRepository>)
///     .with_project_repo(Arc::clone(&setup.project_repo) as Arc<dyn ProjectRepository>)
///     .with_task_scheduler(scheduler);
/// ```
pub async fn setup_pending_merge_repos(
    title: &str,
    task_branch: Option<&str>,
) -> PendingMergeSetup {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(project_id.clone(), title.to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.task_branch = task_branch.map(|s| s.to_string());
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let mut project = Project::new(
        "test-project".to_string(),
        "/tmp/nonexistent-merge-test".to_string(),
    );
    project.id = project_id;
    project.base_branch = Some("main".to_string());
    project_repo.create(project).await.unwrap();

    PendingMergeSetup {
        task_id,
        task_repo,
        project_repo,
    }
}

/// Poll a condition until it returns true or timeout expires.
///
/// Replaces arbitrary `tokio::time::sleep` calls in tests with deterministic
/// condition polling. Checks every 50ms up to `timeout_ms`.
pub async fn wait_for_condition<F, Fut>(mut check: F, timeout_ms: u64) -> bool
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = tokio::time::Instant::now() + Duration::from_millis(timeout_ms);
    while tokio::time::Instant::now() < deadline {
        if check().await {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}

// ==================
// Real git repo helpers (for integration tests)
// ==================

/// A real git repository created in a temp directory.
///
/// The `TempDir` is owned so the directory persists for the test's lifetime.
/// When this struct is dropped, the temp directory is cleaned up.
#[allow(dead_code)]
pub struct RealGitRepo {
    pub dir: tempfile::TempDir,
    pub _main_branch: String,
    pub task_branch: String,
}

impl RealGitRepo {
    pub fn path(&self) -> &std::path::Path {
        self.dir.path()
    }

    pub fn path_string(&self) -> String {
        self.dir.path().to_string_lossy().to_string()
    }
}

/// Create a real git repo with `main` branch (initial commit) and a task branch
/// with one additional commit, then checkout `main`.
pub fn setup_real_git_repo() -> RealGitRepo {
    let dir = tempfile::TempDir::new().expect("create temp dir");
    let path = dir.path();

    // git init -b main
    let _ = std::process::Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(path)
        .output()
        .expect("git init");

    // Configure git user (required for commits)
    let _ = std::process::Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(path)
        .output();
    let _ = std::process::Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(path)
        .output();

    // Initial commit on main
    std::fs::write(path.join("README.md"), "# test repo").unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(path)
        .output();
    let _ = std::process::Command::new("git")
        .args(["commit", "-m", "initial commit"])
        .current_dir(path)
        .output();

    // Create task branch with a feature commit
    let task_branch = "task/test-task-branch".to_string();
    let _ = std::process::Command::new("git")
        .args(["checkout", "-b", &task_branch])
        .current_dir(path)
        .output();
    std::fs::write(path.join("feature.rs"), "// feature code\nfn feature() {}").unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(path)
        .output();
    let _ = std::process::Command::new("git")
        .args(["commit", "-m", "add feature"])
        .current_dir(path)
        .output();

    // Back to main
    let _ = std::process::Command::new("git")
        .args(["checkout", "main"])
        .current_dir(path)
        .output();

    RealGitRepo {
        dir,
        _main_branch: "main".to_string(),
        task_branch,
    }
}

/// Create in-memory repos pre-loaded with a task in PendingMerge and a project
/// pointing to a REAL git directory (from `RealGitRepo`).
///
/// Unlike `setup_pending_merge_repos` which uses a nonexistent path, this wires
/// the project's `working_directory` to an actual git repo so merge strategy
/// dispatch is exercised.
pub async fn setup_pending_merge_with_real_repo(
    title: &str,
    task_branch: &str,
    repo_path: &str,
    merge_strategy: crate::domain::entities::MergeStrategy,
) -> PendingMergeSetup {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(project_id.clone(), title.to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.task_branch = Some(task_branch.to_string());
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let mut project = make_real_git_project(repo_path);
    project.id = project_id;
    project.merge_strategy = merge_strategy;
    project_repo.create(project).await.unwrap();

    PendingMergeSetup {
        task_id,
        task_repo,
        project_repo,
    }
}
