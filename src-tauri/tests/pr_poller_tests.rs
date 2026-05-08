//! PrPollerRegistry lifecycle tests.
//!
//! Covers: MERGED/CLOSED/error transitions, duplicate prevention, stopping guard,
//! cascade stop, and the DashMap CAS creation guard.

mod common;

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use ralphx_lib::application::agent_conversation_workspace::resolve_agent_conversation_workspace_path;
use ralphx_lib::application::services::PrPollerRegistry;
use ralphx_lib::application::{AppState, ChatService, MockChatService, TaskTransitionService};
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::entities::plan_branch::PrStatus as DbPrStatus;
use ralphx_lib::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, ArtifactId, ChatConversationId,
    ExecutionPlanId, IdeationAnalysisBaseRefKind, IdeationSessionId, InternalStatus, PlanBranch,
    PlanBranchId, Project, ProjectId, ReviewOutcome, ReviewerType, Task, TaskCategory,
};
use ralphx_lib::domain::repositories::{
    AgentConversationWorkspaceRepository, PlanBranchRepository,
};
use ralphx_lib::domain::services::github_service::{
    GithubServiceTrait, PrReviewCommentFeedback, PrReviewFeedback, PrStatus,
};
use ralphx_lib::infrastructure::agents::claude::agent_names::AGENT_GENERAL_WORKER;
use ralphx_lib::infrastructure::memory::{
    MemoryAgentConversationWorkspaceRepository, MemoryPlanBranchRepository,
};

use common::MockGithubService;

// ============================================================================
// Shared helpers
// ============================================================================

fn empty_startup_blocked_projects() -> Arc<HashSet<ProjectId>> {
    Arc::new(HashSet::new())
}

fn build_transition_service(
    app_state: &AppState,
    execution_state: &Arc<ExecutionState>,
) -> Arc<TaskTransitionService<tauri::Wry>> {
    Arc::new(TaskTransitionService::new(
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.task_dependency_repo),
        Arc::clone(&app_state.project_repo),
        Arc::clone(&app_state.chat_message_repo),
        Arc::clone(&app_state.chat_attachment_repo),
        Arc::clone(&app_state.chat_conversation_repo),
        Arc::clone(&app_state.agent_run_repo),
        Arc::clone(&app_state.ideation_session_repo),
        Arc::clone(&app_state.activity_event_repo),
        Arc::clone(&app_state.message_queue),
        Arc::clone(&app_state.running_agent_registry),
        Arc::clone(execution_state),
        None,
        Arc::clone(&app_state.memory_event_repo),
    ))
}

fn build_transition_service_with_pr_deps(
    app_state: &AppState,
    execution_state: &Arc<ExecutionState>,
    plan_branch_repo: Arc<dyn PlanBranchRepository>,
) -> Arc<TaskTransitionService<tauri::Wry>> {
    Arc::new(
        TaskTransitionService::new(
            Arc::clone(&app_state.task_repo),
            Arc::clone(&app_state.task_dependency_repo),
            Arc::clone(&app_state.project_repo),
            Arc::clone(&app_state.chat_message_repo),
            Arc::clone(&app_state.chat_attachment_repo),
            Arc::clone(&app_state.chat_conversation_repo),
            Arc::clone(&app_state.agent_run_repo),
            Arc::clone(&app_state.ideation_session_repo),
            Arc::clone(&app_state.activity_event_repo),
            Arc::clone(&app_state.message_queue),
            Arc::clone(&app_state.running_agent_registry),
            Arc::clone(execution_state),
            None,
            Arc::clone(&app_state.memory_event_repo),
        )
        .with_plan_branch_repo(plan_branch_repo)
        .with_review_repo(Arc::clone(&app_state.review_repo)),
    )
}

fn make_agent_workspace(
    conversation_id: ChatConversationId,
    project_id: ProjectId,
) -> AgentConversationWorkspace {
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id,
        project_id,
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::CurrentBranch,
        "feature/agent-screen".to_string(),
        Some("Current branch (feature/agent-screen)".to_string()),
        Some("base-sha".to_string()),
        "ralphx/ralphx/agent-12345678".to_string(),
        "/tmp/agent-workspace".to_string(),
    );
    workspace.publication_pr_number = Some(72);
    workspace.publication_pr_url = Some("https://github.com/owner/repo/pull/72".to_string());
    workspace.publication_pr_status = Some("open".to_string());
    workspace.publication_push_status = Some("pushed".to_string());
    workspace
}

fn requested_changes_feedback(review_id: &str) -> PrReviewFeedback {
    PrReviewFeedback {
        review_id: review_id.to_string(),
        author: "reviewer".to_string(),
        submitted_at: Some("2026-04-26T10:00:00Z".to_string()),
        body: Some("Please cover the publish retry path.".to_string()),
        comments: vec![PrReviewCommentFeedback {
            id: "comment-1".to_string(),
            author: "reviewer".to_string(),
            path: Some("src-tauri/src/application/services/pr_merge_poller.rs".to_string()),
            line: Some(42),
            body: "This should wake the workspace agent.".to_string(),
        }],
    }
}

fn initialize_git_workspace(path: &Path, branch_name: &str) {
    let status = std::process::Command::new("git")
        .arg("init")
        .arg("-b")
        .arg(branch_name)
        .arg(path)
        .status()
        .unwrap();
    assert!(status.success());

    let status = std::process::Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["config", "user.email", "test@example.com"])
        .status()
        .unwrap();
    assert!(status.success());

    let status = std::process::Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["config", "user.name", "Test User"])
        .status()
        .unwrap();
    assert!(status.success());

    let status = std::process::Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["commit", "--allow-empty", "-m", "init"])
        .status()
        .unwrap();
    assert!(status.success());
}

async fn seed_valid_agent_workspace_project(
    app_state: &AppState,
    conversation_id: ChatConversationId,
) -> (tempfile::TempDir, AgentConversationWorkspace) {
    let temp_dir = tempfile::tempdir_in(std::env::current_dir().unwrap()).unwrap();
    let project_root = temp_dir.path().join("repo");
    let worktree_parent = temp_dir.path().join("worktrees");
    std::fs::create_dir_all(&project_root).unwrap();
    std::fs::create_dir_all(&worktree_parent).unwrap();

    let mut project = Project::new(
        "Workspace PR Project".to_string(),
        project_root.to_string_lossy().to_string(),
    );
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut workspace = make_agent_workspace(conversation_id, project.id.clone());
    let expected_worktree_path =
        resolve_agent_conversation_workspace_path(&project, &workspace.conversation_id).unwrap();
    std::fs::create_dir_all(expected_worktree_path.parent().unwrap()).unwrap();
    initialize_git_workspace(&expected_worktree_path, &workspace.branch_name);
    workspace.worktree_path = expected_worktree_path.to_string_lossy().to_string();

    (temp_dir, workspace)
}

#[tokio::test]
async fn agent_workspace_review_feedback_routes_to_same_workspace_agent_once() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let conversation_id = ChatConversationId::from_string("22222222-2222-2222-2222-222222222222");
    let project_id = ProjectId::from_string("project-1".to_string());
    let workspace = make_agent_workspace(conversation_id, project_id.clone());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .unwrap();

    let github = Arc::new(MockGithubService::new());
    let feedback = requested_changes_feedback("review-1");
    github.will_return_review_feedback(feedback.clone());

    let registry = PrPollerRegistry::new(
        Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>),
        Arc::new(MemoryPlanBranchRepository::new()),
    );
    let chat_service = Arc::new(MockChatService::new());

    let routed = registry
        .process_agent_workspace_review_feedback_once(
            &workspace.conversation_id,
            72,
            std::path::Path::new("/tmp/agent-workspace"),
            Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
            Arc::clone(&chat_service) as Arc<dyn ChatService>,
        )
        .await
        .unwrap();

    assert!(routed);
    assert_eq!(chat_service.call_count(), 1);
    let messages = chat_service.get_sent_messages().await;
    assert_eq!(messages.len(), 1);
    assert!(messages[0].contains("GitHub PR #72 requested changes"));
    assert!(messages[0].contains("GitHub review id: review-1"));
    assert!(messages[0].contains("Please cover the publish retry path."));
    assert!(messages[0].contains("pr_merge_poller.rs:42"));

    let options = chat_service.get_sent_options().await;
    assert_eq!(options.len(), 1);
    assert_eq!(
        options[0].conversation_id_override,
        Some(workspace.conversation_id)
    );
    assert_eq!(
        options[0].agent_name_override.as_deref(),
        Some(AGENT_GENERAL_WORKER)
    );

    let updated = workspace_repo
        .get_by_conversation_id(&workspace.conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated.publication_pr_status.as_deref(),
        Some("changes_requested")
    );
    assert_eq!(
        updated.publication_push_status.as_deref(),
        Some("needs_agent")
    );

    let events = workspace_repo
        .list_publication_events(&workspace.conversation_id)
        .await
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].classification.as_deref(),
        Some("github_pr_review:review-1")
    );

    github.will_return_review_feedback(feedback);
    let routed_again = registry
        .process_agent_workspace_review_feedback_once(
            &workspace.conversation_id,
            72,
            std::path::Path::new("/tmp/agent-workspace"),
            Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
            Arc::clone(&chat_service) as Arc<dyn ChatService>,
        )
        .await
        .unwrap();

    assert!(!routed_again);
    assert_eq!(chat_service.call_count(), 1);
    assert_eq!(
        workspace_repo
            .list_publication_events(&workspace.conversation_id)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn recover_agent_workspace_pr_pollers_restarts_active_direct_workspaces() {
    let app_state = AppState::new_test();
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let conversation_id = ChatConversationId::from_string("33333333-3333-3333-3333-333333333333");
    let (_temp_dir, workspace) =
        seed_valid_agent_workspace_project(&app_state, conversation_id).await;

    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .unwrap();

    let github = Arc::new(MockGithubService::new());
    let registry = Arc::new(PrPollerRegistry::new(
        Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>),
        Arc::new(MemoryPlanBranchRepository::new()),
    ));
    let chat_service = Arc::new(MockChatService::new());

    ralphx_lib::application::pr_startup_recovery::recover_agent_workspace_pr_pollers(
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        Arc::clone(&app_state.project_repo),
        Arc::clone(&registry),
        Arc::clone(&chat_service) as Arc<dyn ChatService>,
        empty_startup_blocked_projects(),
    )
    .await;

    assert!(
        registry.is_agent_workspace_polling(&workspace.conversation_id),
        "active direct published workspace PR should restart a backend poller"
    );

    registry.stop_agent_workspace_polling(&workspace.conversation_id);
}

#[tokio::test]
async fn recover_agent_workspace_pr_pollers_skips_workspaces_waiting_on_agent() {
    let app_state = AppState::new_test();
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let conversation_id = ChatConversationId::from_string("44445555-3333-2222-1111-000000000000");
    let (_temp_dir, mut workspace) =
        seed_valid_agent_workspace_project(&app_state, conversation_id).await;
    workspace.publication_pr_status = Some("changes_requested".to_string());
    workspace.publication_push_status = Some("needs_agent".to_string());

    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .unwrap();

    let github = Arc::new(MockGithubService::new());
    let registry = Arc::new(PrPollerRegistry::new(
        Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>),
        Arc::new(MemoryPlanBranchRepository::new()),
    ));
    let chat_service = Arc::new(MockChatService::new());

    ralphx_lib::application::pr_startup_recovery::recover_agent_workspace_pr_pollers(
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        Arc::clone(&app_state.project_repo),
        Arc::clone(&registry),
        Arc::clone(&chat_service) as Arc<dyn ChatService>,
        empty_startup_blocked_projects(),
    )
    .await;

    assert!(
        !registry.is_agent_workspace_polling(&workspace.conversation_id),
        "workspace already waiting on the agent must not restart PR polling on app startup"
    );
    assert_eq!(github.review_feedback_calls(), 0);
    assert_eq!(chat_service.call_count(), 0);
}

#[tokio::test]
async fn agent_workspace_poller_stops_when_workspace_is_ideation_owned() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let conversation_id = ChatConversationId::from_string("12121212-3434-5656-7878-909090909090");
    let project_id = ProjectId::from_string("project-1".to_string());
    let mut workspace = make_agent_workspace(conversation_id, project_id);
    workspace.mode = AgentConversationWorkspaceMode::Ideation;
    workspace.linked_plan_branch_id = Some(PlanBranchId::from_string("plan-branch-1"));
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .unwrap();

    let github = Arc::new(MockGithubService::new());
    let registry = PrPollerRegistry::new(
        Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>),
        Arc::new(MemoryPlanBranchRepository::new()),
    );
    let chat_service = Arc::new(MockChatService::new());

    registry.start_agent_workspace_polling(
        workspace.conversation_id,
        72,
        std::path::PathBuf::from("/tmp/agent-workspace"),
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        Arc::clone(&chat_service) as Arc<dyn ChatService>,
    );

    for _ in 0..20 {
        if !registry.is_agent_workspace_polling(&workspace.conversation_id) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    assert!(
        !registry.is_agent_workspace_polling(&workspace.conversation_id),
        "workspace poller should exit once ideation/task-pipeline ownership takes over"
    );
    assert_eq!(github.check_calls(), 0);
    assert_eq!(github.review_feedback_calls(), 0);
    assert_eq!(chat_service.call_count(), 0);
}

// ============================================================================
// Test 1: start_polling with no github_service is a no-op
// ============================================================================

/// `PrPollerRegistry::new(None, ...)` + `start_polling` must always leave
/// `is_polling()` = false.  This is the fallback contract: without a github_service
/// the registry never starts real tasks.
#[tokio::test]
async fn test_start_polling_noop_without_github_service() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());

    let project = Project::new("Test Project".to_string(), "/tmp/test-repo".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "No-op poller task".to_string());
    task.internal_status = InternalStatus::Merging;
    app_state.task_repo.create(task.clone()).await.unwrap();

    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    let mut pb = PlanBranch::new(
        ArtifactId::from_string("test-artifact".to_string()),
        IdeationSessionId::from_string("test-session".to_string()),
        project.id.clone(),
        "plan/feature".to_string(),
        "main".to_string(),
    );
    pb.merge_task_id = Some(task.id.clone());
    pb.pr_number = Some(42);
    pb.pr_eligible = true;
    pb.pr_polling_active = true;
    let plan_branch_id = pb.id.clone();
    plan_branch_repo.create(pb).await.unwrap();

    // Registry with NO github_service
    let registry = Arc::new(PrPollerRegistry::new(
        None,
        Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>,
    ));

    let transition_service = build_transition_service(&app_state, &execution_state);

    registry.start_polling(
        task.id.clone(),
        plan_branch_id,
        42,
        std::path::PathBuf::from("/tmp/test-repo"),
        "main".to_string(),
        transition_service,
    );

    assert!(
        !registry.is_polling(&task.id),
        "start_polling without github_service must be a no-op — is_polling() must remain false"
    );
}

// ============================================================================
// Test 2: start_polling with github_service creates a live poller
// ============================================================================

/// When a real (mock) github_service is supplied, `start_polling` creates a
/// live JoinHandle and `is_polling()` returns true.  Also verifies that the
/// `pr_creation_guard` DashMap field is publicly accessible.
#[tokio::test]
async fn test_start_polling_creates_live_poller_with_github_service() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());

    let project = Project::new("Test Project".to_string(), "/tmp/test-repo".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Live poller task".to_string());
    task.internal_status = InternalStatus::Merging;
    app_state.task_repo.create(task.clone()).await.unwrap();

    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    let mut pb = PlanBranch::new(
        ArtifactId::from_string("test-artifact".to_string()),
        IdeationSessionId::from_string("test-session".to_string()),
        project.id.clone(),
        "plan/feature".to_string(),
        "main".to_string(),
    );
    pb.merge_task_id = Some(task.id.clone());
    pb.pr_number = Some(42);
    pb.pr_eligible = true;
    pb.pr_polling_active = true;
    let plan_branch_id = pb.id.clone();
    plan_branch_repo.create(pb).await.unwrap();

    let mock = Arc::new(MockGithubService::new());
    mock.will_return_status(PrStatus::Open);

    let registry = Arc::new(PrPollerRegistry::new(
        Some(mock.clone()
            as Arc<dyn ralphx_lib::domain::services::github_service::GithubServiceTrait>),
        Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>,
    ));

    let transition_service = build_transition_service(&app_state, &execution_state);

    registry.start_polling(
        task.id.clone(),
        plan_branch_id.clone(),
        42,
        std::path::PathBuf::from("/tmp/test-repo"),
        "main".to_string(),
        transition_service,
    );

    assert!(
        registry.is_polling(&task.id),
        "start_polling with a github_service must create a live poller — is_polling() must be true"
    );

    // Verify pr_creation_guard is accessible as a public DashMap field
    let guard_ref = &registry.pr_creation_guard;
    assert!(
        guard_ref.is_empty(),
        "pr_creation_guard DashMap must be accessible and start empty"
    );
}

// ============================================================================
// Test 3: stop_polling removes the handle
// ============================================================================

/// After `start_polling` + `stop_polling`, `is_polling()` must return false.
#[tokio::test]
async fn test_stop_polling_removes_handle() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());

    let project = Project::new("Test Project".to_string(), "/tmp/test-repo".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Stop poller task".to_string());
    task.internal_status = InternalStatus::Merging;
    app_state.task_repo.create(task.clone()).await.unwrap();

    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    let mut pb = PlanBranch::new(
        ArtifactId::from_string("test-artifact".to_string()),
        IdeationSessionId::from_string("test-session".to_string()),
        project.id.clone(),
        "plan/feature".to_string(),
        "main".to_string(),
    );
    pb.merge_task_id = Some(task.id.clone());
    pb.pr_number = Some(42);
    pb.pr_eligible = true;
    pb.pr_polling_active = true;
    let plan_branch_id = pb.id.clone();
    plan_branch_repo.create(pb).await.unwrap();

    let mock = Arc::new(MockGithubService::new());

    let registry = Arc::new(PrPollerRegistry::new(
        Some(mock as Arc<dyn ralphx_lib::domain::services::github_service::GithubServiceTrait>),
        Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>,
    ));

    let transition_service = build_transition_service(&app_state, &execution_state);

    // Start then immediately stop
    registry.start_polling(
        task.id.clone(),
        plan_branch_id,
        42,
        std::path::PathBuf::from("/tmp/test-repo"),
        "main".to_string(),
        transition_service,
    );

    assert!(
        registry.is_polling(&task.id),
        "poller must be live after start_polling"
    );

    registry.stop_polling(&task.id);

    assert!(
        !registry.is_polling(&task.id),
        "is_polling() must return false immediately after stop_polling"
    );
}

// ============================================================================
// Test 4: duplicate start_polling is idempotent
// ============================================================================

/// Calling `start_polling` twice for the same `TaskId` must be idempotent —
/// the second call is a no-op; the first handle stays alive.
/// After one `stop_polling`, the poller is gone.
#[tokio::test]
async fn test_duplicate_start_polling_is_idempotent() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());

    let project = Project::new("Test Project".to_string(), "/tmp/test-repo".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Duplicate poller task".to_string());
    task.internal_status = InternalStatus::Merging;
    app_state.task_repo.create(task.clone()).await.unwrap();

    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    let mut pb = PlanBranch::new(
        ArtifactId::from_string("test-artifact".to_string()),
        IdeationSessionId::from_string("test-session".to_string()),
        project.id.clone(),
        "plan/feature".to_string(),
        "main".to_string(),
    );
    pb.merge_task_id = Some(task.id.clone());
    pb.pr_number = Some(42);
    pb.pr_eligible = true;
    pb.pr_polling_active = true;
    let plan_branch_id = pb.id.clone();
    plan_branch_repo.create(pb).await.unwrap();

    let mock = Arc::new(MockGithubService::new());

    let registry = Arc::new(PrPollerRegistry::new(
        Some(mock as Arc<dyn ralphx_lib::domain::services::github_service::GithubServiceTrait>),
        Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>,
    ));

    let transition_service = build_transition_service(&app_state, &execution_state);

    // First call — creates the live handle
    registry.start_polling(
        task.id.clone(),
        plan_branch_id.clone(),
        42,
        std::path::PathBuf::from("/tmp/test-repo"),
        "main".to_string(),
        Arc::clone(&transition_service),
    );

    assert!(
        registry.is_polling(&task.id),
        "poller must be live after first start_polling"
    );

    // Second call — must be idempotent (no-op, first handle still live)
    registry.start_polling(
        task.id.clone(),
        plan_branch_id,
        42,
        std::path::PathBuf::from("/tmp/test-repo"),
        "main".to_string(),
        transition_service,
    );

    assert!(
        registry.is_polling(&task.id),
        "poller must still be live after second (duplicate) start_polling"
    );

    // One stop_polling removes the entry
    registry.stop_polling(&task.id);

    assert!(
        !registry.is_polling(&task.id),
        "is_polling() must be false after stop_polling even after duplicate start calls"
    );
}

// ============================================================================
// Test 5: pr_creation_guard DashMap CAS pattern
// ============================================================================

/// Verifies that `pr_creation_guard` supports the insert/contains/remove pattern
/// used by the create-draft-PR idempotency guard.
#[tokio::test]
async fn test_pr_creation_guard_is_dashmap() {
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    let registry = PrPollerRegistry::new(
        None,
        Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>,
    );

    let pb_id = PlanBranchId::from_string("guard-test-branch".to_string());

    // Initially empty
    assert!(
        !registry.pr_creation_guard.contains_key(&pb_id),
        "pr_creation_guard must not contain the key before insertion"
    );

    // Insert — simulating CAS guard acquisition
    registry.pr_creation_guard.insert(pb_id.clone(), ());

    assert!(
        registry.pr_creation_guard.contains_key(&pb_id),
        "pr_creation_guard must contain the key after insertion"
    );

    // Remove — simulating guard release
    registry.pr_creation_guard.remove(&pb_id);

    assert!(
        !registry.pr_creation_guard.contains_key(&pb_id),
        "pr_creation_guard must not contain the key after removal"
    );
}

// ============================================================================
// Test 6: poller calls check_pr_status immediately after jitter elapses
// ============================================================================

/// Verifies that the poll loop calls `check_pr_status` at least once after the
/// initial jitter sleep without waiting for the normal 60s poll interval.
/// Uses `start_paused = true` + `tokio::time::advance`.
#[tokio::test(start_paused = true)]
async fn test_poller_calls_check_pr_status_immediately_after_jitter() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());

    let project = Project::new("Test Project".to_string(), "/tmp/test-repo".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Status check task".to_string());
    task.internal_status = InternalStatus::Merging;
    app_state.task_repo.create(task.clone()).await.unwrap();

    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    let mut pb = PlanBranch::new(
        ArtifactId::from_string("test-artifact".to_string()),
        IdeationSessionId::from_string("test-session".to_string()),
        project.id.clone(),
        "plan/feature".to_string(),
        "main".to_string(),
    );
    pb.merge_task_id = Some(task.id.clone());
    pb.pr_number = Some(42);
    pb.pr_eligible = true;
    pb.pr_polling_active = true;
    let plan_branch_id = pb.id.clone();
    plan_branch_repo.create(pb).await.unwrap();

    let mock = Arc::new(MockGithubService::new());
    // Return Open so the poller does not self-terminate after the first check
    mock.will_return_status(PrStatus::Open);
    mock.will_return_status(PrStatus::Open);

    let registry = Arc::new(PrPollerRegistry::new(
        Some(mock.clone()
            as Arc<dyn ralphx_lib::domain::services::github_service::GithubServiceTrait>),
        Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>,
    ));

    let transition_service = build_transition_service(&app_state, &execution_state);

    registry.start_polling(
        task.id.clone(),
        plan_branch_id,
        42,
        std::path::PathBuf::from("/tmp/test-repo"),
        "main".to_string(),
        transition_service,
    );

    // Step-by-step advancement to account for tokio mock clock semantics:
    // `sleep(n)` registers a timer at `Instant::now() + n`.  If we advance
    // before the task starts, the task's sleep target is *after* the advance.
    // We must let the task start and register each sleep BEFORE advancing past it.
    //
    // Step 1: Let the spawned task start and register its jitter sleep.
    for _ in 0..5 {
        tokio::task::yield_now().await;
    }
    // Step 2: Advance past the maximum jitter window (31 s > max 30 s jitter).
    tokio::time::advance(std::time::Duration::from_secs(31)).await;
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
    assert!(
        mock.check_calls() >= 1,
        "check_pr_status must be called at least once after jitter elapses (got {} calls)",
        mock.check_calls()
    );
}

// ============================================================================
// Test 7: MERGED status causes the poller to stop itself
// ============================================================================

/// When `check_pr_status` returns `PrStatus::Merged`, the poll loop performs the
/// Merging→Merged transition and then exits, removing itself from `is_polling`.
#[tokio::test(start_paused = true)]
async fn test_poller_merged_stops_poller() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());

    let project = Project::new("Test Project".to_string(), "/tmp/test-repo".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Merged poller task".to_string());
    task.internal_status = InternalStatus::Merging;
    app_state.task_repo.create(task.clone()).await.unwrap();

    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    let mut pb = PlanBranch::new(
        ArtifactId::from_string("test-artifact".to_string()),
        IdeationSessionId::from_string("test-session".to_string()),
        project.id.clone(),
        "plan/feature".to_string(),
        "main".to_string(),
    );
    pb.merge_task_id = Some(task.id.clone());
    pb.pr_number = Some(42);
    pb.pr_eligible = true;
    pb.pr_polling_active = true;
    let plan_branch_id = pb.id.clone();
    plan_branch_repo.create(pb).await.unwrap();

    let mock = Arc::new(MockGithubService::new());
    // Return Merged — the poller should process the transition and exit
    mock.will_return_status(PrStatus::Merged {
        merge_commit_sha: Some("abc123".to_string()),
    });

    let registry = Arc::new(PrPollerRegistry::new(
        Some(mock.clone()
            as Arc<dyn ralphx_lib::domain::services::github_service::GithubServiceTrait>),
        Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>,
    ));

    let transition_service = build_transition_service(&app_state, &execution_state);

    registry.start_polling(
        task.id.clone(),
        plan_branch_id.clone(),
        42,
        std::path::PathBuf::from("/tmp/test-repo"),
        "main".to_string(),
        transition_service,
    );

    // Step-by-step advancement — see test_poller_calls_check_pr_status_immediately_after_jitter for rationale.
    // Step 1: Let the spawned task start and register its jitter sleep.
    for _ in 0..5 {
        tokio::task::yield_now().await;
    }
    // Step 2: Advance past the maximum jitter (31 s).
    tokio::time::advance(std::time::Duration::from_secs(31)).await;
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
    // Step 3: Extra time should not be required for the first status check, but
    // advancing here keeps the assertion stable while the merged cleanup exits.
    tokio::time::advance(std::time::Duration::from_secs(61)).await;
    for _ in 0..30 {
        tokio::task::yield_now().await;
    }

    // The poller must have invoked check_pr_status at least once
    assert!(
        mock.check_calls() >= 1,
        "check_pr_status must be called at least once before the poller exits (got {} calls)",
        mock.check_calls()
    );

    // After processing Merged the poller removes itself — is_polling must become false.
    assert!(
        !registry.is_polling(&task.id),
        "poller must remove itself from the registry after a MERGED status is processed"
    );

    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .expect("task must still exist after merged poller exit");
    assert_eq!(
        updated.internal_status,
        InternalStatus::Merged,
        "MERGED PR status must transition a Merging task to Merged"
    );

    let updated_branch = plan_branch_repo
        .get_by_id(&plan_branch_id)
        .await
        .unwrap()
        .expect("plan branch must still exist after merged poller exit");
    assert_eq!(
        updated_branch.pr_status,
        Some(DbPrStatus::Merged),
        "MERGED PR status must be persisted on the plan branch"
    );
    assert!(
        !updated_branch.pr_polling_active,
        "natural MERGED poller completion must clear pr_polling_active"
    );
}

/// A stale PR poller must not be able to regress an already-merged task back to
/// MergeIncomplete when GitHub reports the PR as closed. The validated transition
/// guard should reject `Merged -> MergeIncomplete`, and the poller should just stop.
#[tokio::test(start_paused = true)]
async fn test_poller_closed_does_not_regress_already_merged_task() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());

    let project = Project::new("Test Project".to_string(), "/tmp/test-repo".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Closed stale poller task".to_string());
    task.internal_status = InternalStatus::Merged;
    app_state.task_repo.create(task.clone()).await.unwrap();

    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    let mut pb = PlanBranch::new(
        ArtifactId::from_string("test-artifact".to_string()),
        IdeationSessionId::from_string("test-session".to_string()),
        project.id.clone(),
        "plan/feature".to_string(),
        "main".to_string(),
    );
    pb.merge_task_id = Some(task.id.clone());
    pb.pr_number = Some(42);
    pb.pr_eligible = true;
    pb.pr_polling_active = true;
    let plan_branch_id = pb.id.clone();
    plan_branch_repo.create(pb).await.unwrap();

    let mock = Arc::new(MockGithubService::new());
    mock.will_return_status(PrStatus::Closed);

    let registry = Arc::new(PrPollerRegistry::new(
        Some(mock.clone()
            as Arc<dyn ralphx_lib::domain::services::github_service::GithubServiceTrait>),
        Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>,
    ));

    let transition_service = build_transition_service(&app_state, &execution_state);

    registry.start_polling(
        task.id.clone(),
        plan_branch_id.clone(),
        42,
        std::path::PathBuf::from("/tmp/test-repo"),
        "main".to_string(),
        transition_service,
    );

    for _ in 0..5 {
        tokio::task::yield_now().await;
    }
    tokio::time::advance(std::time::Duration::from_secs(31)).await;
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
    tokio::time::advance(std::time::Duration::from_secs(61)).await;
    for _ in 0..30 {
        tokio::task::yield_now().await;
    }

    assert!(
        !registry.is_polling(&task.id),
        "closed PR poller must stop even when the transition is rejected"
    );

    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .expect("task must still exist after closed poller exit");
    assert_eq!(
        updated.internal_status,
        InternalStatus::Merged,
        "A stale closed-PR poller must not regress an already-merged task"
    );

    let updated_branch = plan_branch_repo
        .get_by_id(&plan_branch_id)
        .await
        .unwrap()
        .expect("plan branch must still exist after closed poller exit");
    assert_eq!(
        updated_branch.pr_status,
        Some(DbPrStatus::Closed),
        "CLOSED PR status must be persisted on the plan branch"
    );
    assert!(
        !updated_branch.pr_polling_active,
        "natural CLOSED poller completion must clear pr_polling_active"
    );
}

#[tokio::test(start_paused = true)]
async fn test_poller_changes_requested_creates_plan_correction_task() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());

    let project = Project::new("Test Project".to_string(), "/tmp/test-repo".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let execution_plan_id = ExecutionPlanId::from_string("exec-plan-pr-review".to_string());
    let mut merge_task = Task::new_with_category(
        project.id.clone(),
        "Merge plan into main".to_string(),
        TaskCategory::PlanMerge,
    );
    merge_task.internal_status = InternalStatus::WaitingOnPr;
    merge_task.execution_plan_id = Some(execution_plan_id.clone());
    merge_task.ideation_session_id =
        Some(IdeationSessionId::from_string("test-session".to_string()));
    merge_task.plan_artifact_id = Some(ArtifactId::from_string("test-artifact".to_string()));
    app_state
        .task_repo
        .create(merge_task.clone())
        .await
        .unwrap();

    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    let mut pb = PlanBranch::new(
        ArtifactId::from_string("test-artifact".to_string()),
        IdeationSessionId::from_string("test-session".to_string()),
        project.id.clone(),
        "plan/feature".to_string(),
        "main".to_string(),
    );
    pb.merge_task_id = Some(merge_task.id.clone());
    pb.execution_plan_id = Some(execution_plan_id.clone());
    pb.pr_number = Some(42);
    pb.pr_eligible = true;
    pb.pr_polling_active = true;
    let plan_branch_id = pb.id.clone();
    plan_branch_repo.create(pb).await.unwrap();

    let mock = Arc::new(MockGithubService::new());
    mock.will_return_status(PrStatus::Open);
    mock.will_return_review_feedback(PrReviewFeedback {
        review_id: "4136652897".to_string(),
        author: "octocat".to_string(),
        submitted_at: Some("2026-04-22T08:00:00Z".to_string()),
        body: Some("Please fix the edge case before merging.".to_string()),
        comments: vec![PrReviewCommentFeedback {
            id: "3107615689".to_string(),
            author: "octocat".to_string(),
            path: Some("src/lib.rs".to_string()),
            line: Some(17),
            body: "This branch misses the nil case.".to_string(),
        }],
    });

    let registry = Arc::new(PrPollerRegistry::new(
        Some(mock.clone()
            as Arc<dyn ralphx_lib::domain::services::github_service::GithubServiceTrait>),
        Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>,
    ));

    let transition_service = build_transition_service_with_pr_deps(
        &app_state,
        &execution_state,
        Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>,
    );

    registry.start_polling(
        merge_task.id.clone(),
        plan_branch_id.clone(),
        42,
        std::path::PathBuf::from("/tmp/test-repo"),
        "main".to_string(),
        transition_service,
    );

    for _ in 0..5 {
        tokio::task::yield_now().await;
    }
    tokio::time::advance(std::time::Duration::from_secs(31)).await;
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
    tokio::time::advance(std::time::Duration::from_secs(61)).await;
    for _ in 0..30 {
        tokio::task::yield_now().await;
    }

    assert!(
        mock.review_feedback_calls() >= 1,
        "open PR poll must query GitHub review feedback"
    );

    let updated_merge = app_state
        .task_repo
        .get_by_id(&merge_task.id)
        .await
        .unwrap()
        .expect("merge task must still exist");
    assert_eq!(updated_merge.internal_status, InternalStatus::Blocked);
    assert!(updated_merge
        .blocked_reason
        .as_deref()
        .unwrap_or_default()
        .contains("GitHub PR #42 requested changes"));

    let tasks = app_state
        .task_repo
        .list_paginated(&project.id, None, 0, 100, false, None, None, None)
        .await
        .unwrap();
    let correction = tasks
        .iter()
        .find(|task| {
            task.category == TaskCategory::Regular
                && task.title.contains("Address GitHub PR #42 review feedback")
        })
        .expect("changes_requested review should create a regular correction task");
    assert_eq!(correction.internal_status, InternalStatus::Ready);
    assert_eq!(
        correction.execution_plan_id.as_ref(),
        Some(&execution_plan_id)
    );

    assert!(
        app_state
            .task_dependency_repo
            .has_dependency(&merge_task.id, &correction.id)
            .await
            .unwrap(),
        "final plan merge task must depend on the GitHub correction task"
    );

    let notes = app_state
        .review_repo
        .get_notes_by_task_id(&correction.id)
        .await
        .unwrap();
    let note = notes
        .iter()
        .find(|note| note.outcome == ReviewOutcome::ChangesRequested)
        .expect("correction task should carry requested-changes feedback");
    assert_eq!(note.reviewer, ReviewerType::Human);
    assert!(note
        .notes
        .as_deref()
        .unwrap_or_default()
        .contains("Please fix the edge case"));

    let refreshed_branch = plan_branch_repo
        .get_by_id(&plan_branch_id)
        .await
        .unwrap()
        .expect("plan branch must still exist");
    assert!(
        !refreshed_branch.pr_polling_active,
        "polling should stop while correction task is active"
    );
}
