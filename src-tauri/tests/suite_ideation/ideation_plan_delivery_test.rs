// Integration tests for the ideation plan delivery flow.
//
// Verifies that `plan:delivered` external event fires correctly when all tasks
// in an ideation session reach `Merged` status (via the PlanMerge task's
// `on_enter(Merged)` handler path).
//
// Tests:
//   1. Happy path: all 3 tasks Merged → plan:delivered fires once, delivery_status=delivered
//   2. Partial merge guard: workers NOT yet Merged → no plan:delivered
//   3. Idempotency: on_enter(Merged) called twice → still exactly 1 plan:delivered

use std::fs;
use std::process::Command;
use std::sync::Arc;

use async_trait::async_trait;
use axum::extract::{Path, Query, State};
use axum::Json;
use dashmap::DashMap;
use ralphx_domain::entities::EventType;
use ralphx_lib::application::{AppState, TeamService, TeamStateTracker};
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::entities::{
    IdeationSessionBuilder, IdeationSessionId, InternalStatus, Project, ProjectId, Task,
    TaskCategory,
};
use ralphx_lib::domain::repositories::{
    ExternalEventsRepository, IdeationSessionRepository, ProjectRepository, TaskRepository,
};
use ralphx_lib::domain::state_machine::{
    State as MachineState, TaskContext, TaskServices, TaskStateMachine, TransitionHandler,
};
use ralphx_lib::domain::state_machine::services::WebhookPublisher as WebhookPublisherTrait;
use ralphx_lib::domain::state_machine::transition_handler::complete_merge_internal;
use ralphx_lib::http_server::handlers::{
    get_session_tasks_http, GetSessionTasksParams,
};
use ralphx_lib::http_server::project_scope::ProjectScope;
use ralphx_lib::http_server::types::HttpServerState;
use ralphx_lib::infrastructure::memory::{
    MemoryExternalEventsRepository, MemoryIdeationSessionRepository, MemoryProjectRepository,
    MemoryTaskRepository,
};
use std::sync::Mutex;
use tokio::sync::Mutex as TokioMutex;

// ============================================================================
// RecordingWebhookPublisher — captures publish() calls for assertion
// ============================================================================

#[derive(Default)]
struct RecordingWebhookPublisher {
    calls: Mutex<Vec<EventType>>,
}

impl RecordingWebhookPublisher {
    fn new() -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
        }
    }

    fn was_called_with(&self, event_type: EventType) -> bool {
        self.calls.lock().unwrap().contains(&event_type)
    }

    fn call_count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }

    fn count_calls_with(&self, event_type: EventType) -> usize {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|e| **e == event_type)
            .count()
    }
}

#[async_trait]
impl WebhookPublisherTrait for RecordingWebhookPublisher {
    async fn publish(
        &self,
        event_type: EventType,
        _project_id: &str,
        _payload: serde_json::Value,
    ) {
        self.calls.lock().unwrap().push(event_type);
    }
}

// ============================================================================
// TestSetup — holds all shared repos and IDs for a test run
// ============================================================================

struct TestSetup {
    project_id: String,
    session_id: String,
    task_repo: Arc<MemoryTaskRepository>,
    project_repo: Arc<MemoryProjectRepository>,
    events_repo: Arc<MemoryExternalEventsRepository>,
    session_repo: Arc<MemoryIdeationSessionRepository>,
    webhook_publisher: Arc<RecordingWebhookPublisher>,
    session_merge_locks: Arc<DashMap<String, Arc<TokioMutex<()>>>>,
    plan_merge_task_id: String,
}

impl TestSetup {
    /// All 3 tasks (2 workers + 1 PlanMerge) are in Merged state.
    async fn new_all_merged() -> Self {
        Self::build(InternalStatus::Merged).await
    }

    /// Workers are in Backlog; only the PlanMerge task is set to Merged.
    async fn new_partial_merge() -> Self {
        Self::build(InternalStatus::Backlog).await
    }

    async fn build(worker_status: InternalStatus) -> Self {
        let project_id = "proj-plan-delivery-test".to_string();

        // Build a session and get its ID
        let session = IdeationSessionBuilder::new()
            .project_id(ProjectId::from_string(project_id.clone()))
            .build();
        let session_id = session.id.as_str().to_string();

        let task_repo = Arc::new(MemoryTaskRepository::new());
        let project_repo = Arc::new(MemoryProjectRepository::new());
        let events_repo = Arc::new(MemoryExternalEventsRepository::new());
        let session_repo = Arc::new(MemoryIdeationSessionRepository::new());
        let webhook_publisher = Arc::new(RecordingWebhookPublisher::new());
        let session_merge_locks = Arc::new(DashMap::new());

        // Persist the session so the HTTP handler can load it
        session_repo.create(session).await.expect("create session");

        // Worker task 1
        let mut worker1 = Task::new(
            ProjectId::from_string(project_id.clone()),
            "Worker task 1".to_string(),
        );
        worker1.ideation_session_id =
            Some(IdeationSessionId::from_string(session_id.clone()));
        worker1.internal_status = worker_status;
        task_repo.create(worker1).await.expect("create worker1");

        // Worker task 2
        let mut worker2 = Task::new(
            ProjectId::from_string(project_id.clone()),
            "Worker task 2".to_string(),
        );
        worker2.ideation_session_id =
            Some(IdeationSessionId::from_string(session_id.clone()));
        worker2.internal_status = worker_status;
        task_repo.create(worker2).await.expect("create worker2");

        // PlanMerge task — category=PlanMerge, always Merged (it just entered this state)
        let mut plan_merge = Task::new(
            ProjectId::from_string(project_id.clone()),
            "Plan merge task".to_string(),
        );
        plan_merge.ideation_session_id =
            Some(IdeationSessionId::from_string(session_id.clone()));
        plan_merge.category = TaskCategory::PlanMerge;
        plan_merge.internal_status = InternalStatus::Merged;
        let plan_merge_task_id = plan_merge.id.as_str().to_string();
        task_repo.create(plan_merge).await.expect("create plan_merge");

        Self {
            project_id,
            session_id,
            task_repo,
            project_repo,
            events_repo,
            session_repo,
            webhook_publisher,
            session_merge_locks,
            plan_merge_task_id,
        }
    }

    /// Build TaskServices wired with this test's repos.
    fn build_task_services(&self) -> TaskServices {
        TaskServices::new_mock()
            .with_task_repo(
                Arc::clone(&self.task_repo) as Arc<dyn TaskRepository>,
            )
            .with_project_repo(
                Arc::clone(&self.project_repo) as Arc<dyn ProjectRepository>,
            )
            .with_external_events_repo(
                Arc::clone(&self.events_repo) as Arc<dyn ExternalEventsRepository>,
            )
            .with_webhook_publisher(
                Arc::clone(&self.webhook_publisher) as Arc<dyn WebhookPublisherTrait>,
            )
            .with_session_merge_locks(Arc::clone(&self.session_merge_locks))
    }
}

// ============================================================================
// Test helpers
// ============================================================================

/// Trigger `on_enter(Merged)` for the PlanMerge task using the setup's repos.
async fn call_on_enter_merged(setup: &TestSetup) {
    let services = setup.build_task_services();
    let context = TaskContext::new(&setup.plan_merge_task_id, &setup.project_id, services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);
    handler
        .on_enter(&MachineState::Merged)
        .await
        .expect("on_enter(Merged) should not return an error");
}

/// Count `plan:delivered` events for this test's project in the events repo.
async fn count_plan_delivered(setup: &TestSetup) -> usize {
    setup
        .events_repo
        .get_events_after_cursor(std::slice::from_ref(&setup.project_id), 0, 1000)
        .await
        .expect("get_events_after_cursor")
        .into_iter()
        .filter(|e| e.event_type == "plan:delivered")
        .count()
}

/// Build an HttpServerState backed by this setup's task and session repos.
fn build_http_state(setup: &TestSetup) -> HttpServerState {
    let mut app_state = AppState::new_test();
    app_state.task_repo = Arc::clone(&setup.task_repo) as Arc<dyn TaskRepository>;
    app_state.ideation_session_repo =
        Arc::clone(&setup.session_repo) as Arc<dyn IdeationSessionRepository>;
    let app_state = Arc::new(app_state);
    let execution_state = Arc::new(ExecutionState::new());
    let tracker = TeamStateTracker::new();
    let team_service = Arc::new(TeamService::new_without_events(Arc::new(tracker.clone())));
    HttpServerState {
        app_state,
        execution_state,
        team_tracker: tracker,
        team_service,
        delegation_service: Default::default(),
    }
}

// ============================================================================
// Test 1 — Happy path: all tasks merged → plan:delivered fires once
// ============================================================================

#[tokio::test]
async fn test_plan_delivered_fires_after_all_tasks_merged() {
    let setup = TestSetup::new_all_merged().await;

    // Trigger: PlanMerge task enters Merged state (all workers already Merged)
    call_on_enter_merged(&setup).await;

    // Assert: exactly 1 plan:delivered event in external_events
    let count = count_plan_delivered(&setup).await;
    assert_eq!(count, 1, "Expected exactly one plan:delivered event");

    // Assert: the event payload contains the correct session_id
    let events = setup
        .events_repo
        .get_events_after_cursor(std::slice::from_ref(&setup.project_id), 0, 1000)
        .await
        .unwrap();
    let delivered_event = events
        .iter()
        .find(|e| e.event_type == "plan:delivered")
        .expect("plan:delivered event must exist");
    assert!(
        delivered_event.payload.contains(&setup.session_id),
        "plan:delivered payload must contain session_id"
    );

    // Assert: webhook publisher was called with PlanDelivered
    assert!(
        setup
            .webhook_publisher
            .was_called_with(EventType::PlanDelivered),
        "webhook publisher should have received PlanDelivered"
    );
    assert_eq!(
        setup.webhook_publisher.call_count(),
        1,
        "webhook publisher should be called exactly once"
    );

    // Assert: delivery_status=delivered via the session tasks HTTP handler
    let http_state = build_http_state(&setup);
    let result = get_session_tasks_http(
        State(http_state),
        ProjectScope(None),
        Path(setup.session_id.clone()),
        Query(GetSessionTasksParams { changed_since: None }),
    )
    .await
    .expect("get_session_tasks_http should succeed");
    let Json(body) = result;
    assert_eq!(
        body.delivery_status, "delivered",
        "delivery_status must be 'delivered' when all tasks are Merged"
    );
    assert_eq!(body.task_count, 3, "session should have 3 tasks");
}

// ============================================================================
// Test 2 — Partial merge guard: workers NOT yet Merged → no plan:delivered
// ============================================================================

#[tokio::test]
async fn test_plan_delivered_not_fired_on_partial_merge() {
    let setup = TestSetup::new_partial_merge().await;

    // Trigger: PlanMerge task enters Merged, but workers are still in Backlog
    call_on_enter_merged(&setup).await;

    // Assert: no plan:delivered event
    let count = count_plan_delivered(&setup).await;
    assert_eq!(
        count, 0,
        "plan:delivered must NOT fire when worker tasks are not yet Merged"
    );

    // Assert: webhook publisher was not called
    assert_eq!(
        setup.webhook_publisher.call_count(),
        0,
        "webhook publisher must not be called on partial merge"
    );
}

// ============================================================================
// Test 3 — Idempotency: calling on_enter(Merged) twice → only 1 plan:delivered
// ============================================================================

#[tokio::test]
async fn test_plan_delivered_idempotent() {
    let setup = TestSetup::new_all_merged().await;

    // First trigger: inserts the plan:delivered event
    call_on_enter_merged(&setup).await;

    // Second trigger: simulates duplicate / concurrent call (e.g. reconciler re-fires)
    // Must be blocked by the idempotency guard (event_exists check)
    call_on_enter_merged(&setup).await;

    // Assert: still exactly 1 plan:delivered — not 2
    let count = count_plan_delivered(&setup).await;
    assert_eq!(
        count, 1,
        "plan:delivered must fire only once even if on_enter(Merged) is called twice"
    );

    // Assert: webhook publisher was called exactly once (not twice)
    assert_eq!(
        setup.webhook_publisher.call_count(),
        1,
        "webhook publisher must be called only once (idempotency guard)"
    );
}

// ============================================================================
// Helpers for complete_merge_internal tests
// ============================================================================

/// Init a git repo with main + a plan branch, merge plan→main, return (TempDir, merge SHA).
///
/// This is a synchronous helper — git CLI calls are blocking.  Callers in async
/// tests may call it directly (blocking the tokio thread briefly, which is fine
/// in tests) or wrap with `spawn_blocking`.
fn setup_repo_with_merged_plan_branch() -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().expect("create temp dir");
    let repo = dir.path();

    // Init repo and configure git identity
    for args in [
        vec!["init"],
        vec!["config", "user.email", "test@test.com"],
        vec!["config", "user.name", "Test User"],
    ] {
        Command::new("git")
            .args(&args)
            .current_dir(repo)
            .output()
            .expect("git setup command failed");
    }

    // Initial commit on main
    fs::write(repo.join("README.md"), "# Test\n").expect("write README.md");
    Command::new("git").args(["add", "."]).current_dir(repo).output().expect("git add");
    Command::new("git")
        .args(["commit", "-m", "Initial commit"])
        .current_dir(repo)
        .output()
        .expect("git commit");
    Command::new("git")
        .args(["branch", "-M", "main"])
        .current_dir(repo)
        .output()
        .expect("git branch -M main");

    // Create plan branch with one commit
    Command::new("git")
        .args(["checkout", "-b", "plan/test-plan"])
        .current_dir(repo)
        .output()
        .expect("git checkout -b plan/test-plan");
    fs::write(repo.join("plan.md"), "Plan content\n").expect("write plan.md");
    Command::new("git").args(["add", "."]).current_dir(repo).output().expect("git add plan");
    Command::new("git")
        .args(["commit", "-m", "Add plan"])
        .current_dir(repo)
        .output()
        .expect("git commit plan");

    // Return to main and merge plan branch (--no-ff produces a real merge commit)
    Command::new("git")
        .args(["checkout", "main"])
        .current_dir(repo)
        .output()
        .expect("git checkout main");
    Command::new("git")
        .args(["merge", "--no-ff", "plan/test-plan", "-m", "Merge plan/test-plan into main"])
        .current_dir(repo)
        .output()
        .expect("git merge plan branch");

    // Capture the resulting merge commit SHA
    let out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo)
        .output()
        .expect("git rev-parse HEAD");
    let sha = String::from_utf8_lossy(&out.stdout).trim().to_string();

    (dir, sha)
}

// ============================================================================
// Test 4 — complete_merge_internal runtime path: PlanMerge → plan:delivered
// ============================================================================

#[tokio::test]
async fn test_plan_delivered_fires_from_complete_merge_internal() {
    let (temp_dir, commit_sha) = setup_repo_with_merged_plan_branch();
    let repo_path = temp_dir.path().to_str().unwrap().to_string();

    // Create project pointing at the real git repo
    let mut project = Project::new("cmi-test-project".to_string(), repo_path.clone());
    project.base_branch = Some("main".to_string());
    let project_id = project.id.to_string();

    let session_id_str = "session-cmi-pdm307-test".to_string();
    let session_id = IdeationSessionId::from_string(session_id_str.clone());

    // Create PlanMerge task in PendingMerge status
    let mut task = Task::new(project.id.clone(), "PlanMerge task".to_string());
    task.category = TaskCategory::PlanMerge;
    task.internal_status = InternalStatus::PendingMerge;
    task.ideation_session_id = Some(session_id);
    task.task_branch = Some("plan/test-plan".to_string());
    let task_id_str = task.id.as_str().to_string();

    // Pre-insert task into repo so state freshness check passes
    let task_repo: Arc<dyn TaskRepository> = Arc::new(MemoryTaskRepository::new());
    task_repo.create(task.clone()).await.expect("create task");

    // Wire up external events and webhook publisher
    let raw_events = Arc::new(MemoryExternalEventsRepository::new());
    let raw_publisher = Arc::new(RecordingWebhookPublisher::new());
    let events_dyn: Arc<dyn ExternalEventsRepository> =
        Arc::clone(&raw_events) as Arc<dyn ExternalEventsRepository>;
    let publisher_dyn: Arc<dyn WebhookPublisherTrait> =
        Arc::clone(&raw_publisher) as Arc<dyn WebhookPublisherTrait>;

    // Call complete_merge_internal directly — the primary fix path from PDM-307
    let result = complete_merge_internal::<tauri::test::MockRuntime>(
        &mut task,
        &project,
        &commit_sha,
        "plan/test-plan",
        "main",
        &task_repo,
        Some(&events_dyn),
        Some(&publisher_dyn),
        None,
        Some("Test Delivery Session".to_string()),
    )
    .await;

    assert!(result.is_ok(), "complete_merge_internal failed: {:?}", result);
    assert_eq!(task.internal_status, InternalStatus::Merged);
    assert_eq!(task.merge_commit_sha.as_deref(), Some(commit_sha.as_str()));

    // Assert: exactly one plan:delivered event in the events repo
    let events = raw_events
        .get_events_after_cursor(std::slice::from_ref(&project_id), 0, 1000)
        .await
        .expect("get_events_after_cursor");
    let delivered: Vec<_> = events.iter().filter(|e| e.event_type == "plan:delivered").collect();
    assert_eq!(
        delivered.len(),
        1,
        "Expected exactly one plan:delivered event, got {}",
        delivered.len()
    );

    // Assert: payload contains required fields — using JSON parsing (not string .contains())
    let payload_json: serde_json::Value = serde_json::from_str(&delivered[0].payload)
        .expect("plan:delivered payload must be valid JSON");

    // Backward compat: original fields must still be present
    assert_eq!(
        payload_json["session_id"].as_str().unwrap(),
        session_id_str,
        "session_id must be present"
    );
    assert_eq!(
        payload_json["project_id"].as_str().unwrap(),
        project_id,
        "project_id must be present"
    );
    assert_eq!(
        payload_json["task_id"].as_str().unwrap(),
        task_id_str,
        "task_id must be present"
    );
    assert_eq!(
        payload_json["commit_sha"].as_str().unwrap(),
        commit_sha,
        "commit_sha must be present"
    );
    assert_eq!(
        payload_json["target_branch"].as_str().unwrap(),
        "main",
        "target_branch must be present"
    );
    assert!(
        payload_json.get("timestamp").is_some(),
        "timestamp field must be present"
    );

    // Enrichment fields
    assert_eq!(
        payload_json["project_name"].as_str().unwrap(),
        "cmi-test-project",
        "project_name must match the project name"
    );
    assert_eq!(
        payload_json["task_title"].as_str().unwrap(),
        "PlanMerge task",
        "task_title must match the task title"
    );
    assert_eq!(
        payload_json["session_title"].as_str().unwrap(),
        "Test Delivery Session",
        "session_title must be present when provided"
    );
    assert_eq!(
        payload_json["presentation_kind"].as_str().unwrap(),
        "plan_delivered",
        "presentation_kind must be plan_delivered"
    );
    let hc = payload_json["human_context"].as_str().unwrap();
    assert!(!hc.is_empty(), "human_context must not be empty");
    assert!(hc.contains("cmi-test-project"), "human_context must contain project_name");
    assert!(hc.contains("Test Delivery Session"), "human_context must contain session_title");
    assert!(hc.contains("PlanMerge task"), "human_context must contain task_title");

    // Assert: webhook publisher called once with PlanDelivered
    assert!(
        raw_publisher.was_called_with(EventType::PlanDelivered),
        "publisher must be called with PlanDelivered"
    );
    assert_eq!(
        raw_publisher.count_calls_with(EventType::PlanDelivered),
        1,
        "PlanDelivered must be published exactly once"
    );
}

// ============================================================================
// Test 5 — Idempotency via complete_merge_internal: calling twice → 1 event
// ============================================================================

#[tokio::test]
async fn test_plan_delivered_idempotent_from_complete_merge_internal() {
    let (temp_dir, commit_sha) = setup_repo_with_merged_plan_branch();
    let repo_path = temp_dir.path().to_str().unwrap().to_string();

    let mut project = Project::new("cmi-idem-project".to_string(), repo_path.clone());
    project.base_branch = Some("main".to_string());
    let project_id = project.id.to_string();

    let session_id_str = "session-cmi-idem-test".to_string();
    let session_id = IdeationSessionId::from_string(session_id_str.clone());

    let mut task = Task::new(project.id.clone(), "PlanMerge idempotency task".to_string());
    task.category = TaskCategory::PlanMerge;
    task.internal_status = InternalStatus::PendingMerge;
    task.ideation_session_id = Some(session_id);
    task.task_branch = Some("plan/test-plan".to_string());

    let task_repo: Arc<dyn TaskRepository> = Arc::new(MemoryTaskRepository::new());
    task_repo.create(task.clone()).await.expect("create task");

    let raw_events = Arc::new(MemoryExternalEventsRepository::new());
    let raw_publisher = Arc::new(RecordingWebhookPublisher::new());
    let events_dyn: Arc<dyn ExternalEventsRepository> =
        Arc::clone(&raw_events) as Arc<dyn ExternalEventsRepository>;
    let publisher_dyn: Arc<dyn WebhookPublisherTrait> =
        Arc::clone(&raw_publisher) as Arc<dyn WebhookPublisherTrait>;

    // First call: inserts plan:delivered
    let result1 = complete_merge_internal::<tauri::test::MockRuntime>(
        &mut task,
        &project,
        &commit_sha,
        "plan/test-plan",
        "main",
        &task_repo,
        Some(&events_dyn),
        Some(&publisher_dyn),
        None,
        None,
    )
    .await;
    assert!(result1.is_ok(), "First call failed: {:?}", result1);

    // Simulate a concurrent duplicate: reset task to PendingMerge so the state
    // freshness guard doesn't abort the second call before reaching event_exists.
    task.internal_status = InternalStatus::PendingMerge;
    task_repo.update(&task).await.expect("reset task to PendingMerge");

    // Second call: event_exists returns true → idempotency guard prevents duplicate
    let result2 = complete_merge_internal::<tauri::test::MockRuntime>(
        &mut task,
        &project,
        &commit_sha,
        "plan/test-plan",
        "main",
        &task_repo,
        Some(&events_dyn),
        Some(&publisher_dyn),
        None,
        None,
    )
    .await;
    assert!(result2.is_ok(), "Second call failed: {:?}", result2);

    // Assert: still exactly one plan:delivered — not two
    let events = raw_events
        .get_events_after_cursor(std::slice::from_ref(&project_id), 0, 1000)
        .await
        .expect("get_events_after_cursor");
    let delivered_count = events.iter().filter(|e| e.event_type == "plan:delivered").count();
    assert_eq!(
        delivered_count,
        1,
        "plan:delivered must fire only once even when complete_merge_internal is called twice"
    );

    // Assert: PlanDelivered webhook published only once (idempotency guard)
    assert_eq!(
        raw_publisher.count_calls_with(EventType::PlanDelivered),
        1,
        "PlanDelivered webhook must be published exactly once (idempotency guard)"
    );
}

// ============================================================================
// Test 6 — merge:completed enrichment: all presentation fields present in payload
// ============================================================================

#[tokio::test]
async fn test_merge_completed_includes_presentation_fields() {
    let (temp_dir, commit_sha) = setup_repo_with_merged_plan_branch();
    let repo_path = temp_dir.path().to_str().unwrap().to_string();

    // Create project with a recognizable name for enrichment assertion
    let mut project = Project::new("Presentation Test Project".to_string(), repo_path.clone());
    project.base_branch = Some("main".to_string());
    let project_id = project.id.to_string();

    let session_id_str = "session-mc-enrichment-test".to_string();
    let session_id = IdeationSessionId::from_string(session_id_str.clone());

    // Use a regular worker task (NOT PlanMerge) so only merge:completed fires,
    // not plan:delivered. This isolates the merge:completed enrichment assertion.
    let mut task = Task::new(project.id.clone(), "My Worker Task".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.ideation_session_id = Some(session_id);
    task.task_branch = Some("plan/test-plan".to_string());
    let task_id_str = task.id.as_str().to_string();

    let task_repo: Arc<dyn TaskRepository> = Arc::new(MemoryTaskRepository::new());
    task_repo.create(task.clone()).await.expect("create task");

    let raw_events = Arc::new(MemoryExternalEventsRepository::new());
    let raw_publisher = Arc::new(RecordingWebhookPublisher::new());
    let events_dyn: Arc<dyn ExternalEventsRepository> =
        Arc::clone(&raw_events) as Arc<dyn ExternalEventsRepository>;
    let publisher_dyn: Arc<dyn WebhookPublisherTrait> =
        Arc::clone(&raw_publisher) as Arc<dyn WebhookPublisherTrait>;

    let result = complete_merge_internal::<tauri::test::MockRuntime>(
        &mut task,
        &project,
        &commit_sha,
        "plan/test-plan",
        "main",
        &task_repo,
        Some(&events_dyn),
        Some(&publisher_dyn),
        None,
        Some("My Ideation Session".to_string()),
    )
    .await;

    assert!(result.is_ok(), "complete_merge_internal failed: {:?}", result);
    assert_eq!(task.internal_status, InternalStatus::Merged);

    let events = raw_events
        .get_events_after_cursor(std::slice::from_ref(&project_id), 0, 1000)
        .await
        .expect("get_events_after_cursor");

    // Assert: exactly one merge:completed event
    let mc_events: Vec<_> = events.iter().filter(|e| e.event_type == "merge:completed").collect();
    assert_eq!(mc_events.len(), 1, "Expected exactly one merge:completed event");

    let payload_json: serde_json::Value = serde_json::from_str(&mc_events[0].payload)
        .expect("merge:completed payload must be valid JSON");

    // Backward compat: original fields must still be present
    assert_eq!(
        payload_json["session_id"].as_str().unwrap(),
        session_id_str,
        "session_id must be present"
    );
    assert_eq!(
        payload_json["project_id"].as_str().unwrap(),
        project_id,
        "project_id must be present"
    );
    assert_eq!(
        payload_json["task_id"].as_str().unwrap(),
        task_id_str,
        "task_id must be present"
    );
    assert_eq!(
        payload_json["commit_sha"].as_str().unwrap(),
        commit_sha,
        "commit_sha must be present"
    );
    assert_eq!(
        payload_json["target_branch"].as_str().unwrap(),
        "main",
        "target_branch must be present"
    );
    assert!(payload_json.get("timestamp").is_some(), "timestamp must be present");

    // Enrichment fields
    assert_eq!(
        payload_json["project_name"].as_str().unwrap(),
        "Presentation Test Project",
        "project_name must match the project name"
    );
    assert_eq!(
        payload_json["task_title"].as_str().unwrap(),
        "My Worker Task",
        "task_title must match the task title"
    );
    assert_eq!(
        payload_json["session_title"].as_str().unwrap(),
        "My Ideation Session",
        "session_title must be present when provided"
    );
    assert_eq!(
        payload_json["presentation_kind"].as_str().unwrap(),
        "merge_completed",
        "presentation_kind must be merge_completed"
    );
    let hc = payload_json["human_context"].as_str().unwrap();
    assert!(!hc.is_empty(), "human_context must not be empty");
    assert!(
        hc.contains("Presentation Test Project"),
        "human_context must contain project_name"
    );
    assert!(
        hc.contains("My Ideation Session"),
        "human_context must contain session_title"
    );
    assert!(
        hc.contains("My Worker Task"),
        "human_context must contain task_title"
    );

    // No plan:delivered must fire for a regular (non-PlanMerge) task
    let pd_count = events.iter().filter(|e| e.event_type == "plan:delivered").count();
    assert_eq!(
        pd_count,
        0,
        "plan:delivered must not fire for a non-PlanMerge task"
    );

    // Webhook publisher called with MergeCompleted
    assert!(
        raw_publisher.was_called_with(EventType::MergeCompleted),
        "publisher must be called with MergeCompleted"
    );
}
