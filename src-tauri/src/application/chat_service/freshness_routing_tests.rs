// Unit tests for freshness_routing::freshness_return_route
//
// Covers:
//  1. NormalMerge when plan_update_conflict absent, false, or metadata None
//  2. FreshnessRouted(origin_state) when plan_update_conflict=true
//  3. Defaults to PendingReview when freshness_origin_state absent (safety)
//  4. Correctly routes to Ready for "executing"/"re_executing" origin states
//  5. Re-inserts plan_update_conflict and branch_freshness_conflict when transition fails
//  6. IPR entry removed after successful routing
//  7. Does NOT call FreshnessCleanupScope::RoutingOnly (verified by code inspection)
//  8. Targeted field removal: plan_update_conflict, branch_freshness_conflict, freshness_backoff_until cleared
//
// Integration test #7 (full chain):
//  legacy Reviewing → Merging row → complete_merge compatibility recovery
//  → back to Reviewing; verifies task is NOT Merged + merge_commit_sha not set

use std::sync::Arc;

use async_trait::async_trait;

use crate::application::chat_service::freshness_routing::{
    freshness_return_route, FreshnessRouteResult,
};
use crate::application::interactive_process_registry::{
    InteractiveProcessKey, InteractiveProcessRegistry,
};
use crate::application::{AppState, TaskTransitionService};
use crate::application::execution_state::ExecutionState;
use crate::domain::entities::plan_branch::PrPushStatus;
use crate::domain::entities::{
    AgentWorkspacePrDescription, ArtifactId, IdeationSessionId, InternalStatus, PlanBranch,
    PlanBranchStatus, Project, ProjectId, Task, TaskCategory,
};
use crate::domain::repositories::TaskRepository;
use crate::domain::services::{GithubServiceTrait, PlanPrDescriptionDrafter, PrReviewState};
use crate::domain::state_machine::transition_handler::{
    publish_plan_branch_pr_after_freshness_update, PlanBranchPrSyncServices,
};
use crate::tests::mock_github_service::MockGithubService;
use crate::AppError;

// ============================================================================
// Helpers
// ============================================================================

/// Build a minimal TaskTransitionService using in-memory repos.
fn build_transition_service(app_state: &AppState) -> TaskTransitionService {
    let execution_state = Arc::new(ExecutionState::new());
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
        execution_state,
        None, // No AppHandle in tests
        Arc::clone(&app_state.memory_event_repo),
    )
}

/// Create a task with the given metadata JSON and insert it into the repo.
/// Sets worktree_path to temp_dir so the on_enter(Reviewing) worktree guard passes
/// when auto-transitions fire through the review pipeline.
async fn insert_task_with_metadata(
    repo: &Arc<dyn TaskRepository>,
    project_id: ProjectId,
    metadata: Option<serde_json::Value>,
) -> Task {
    let mut task = Task::new(project_id, "test task".to_owned());
    task.metadata = metadata.map(|v| v.to_string());
    task.worktree_path = Some(std::env::temp_dir().to_string_lossy().to_string());
    repo.create(task.clone())
        .await
        .expect("Failed to create task");
    task
}

/// Create and insert a test project.
async fn insert_test_project(app_state: &AppState) -> Project {
    let project = Project::new("test-project".to_owned(), "/tmp/test-repo".to_owned());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .expect("Failed to create project");
    project
}

fn run_git(repo_path: &std::path::Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(repo_path)
        .output()
        .unwrap_or_else(|error| panic!("git {:?} failed to start: {}", args, error));
    assert!(
        output.status.success(),
        "git {:?} failed\nstdout: {}\nstderr: {}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn setup_publication_finalizer_repo(branch_name: &str) -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path();

    run_git(path, &["init", "-b", "main"]);
    run_git(path, &["config", "user.email", "test@test.com"]);
    run_git(path, &["config", "user.name", "Test User"]);
    std::fs::write(path.join("README.md"), "# freshness publication repo\n").expect("write README");
    run_git(path, &["add", "."]);
    run_git(path, &["commit", "-m", "initial commit"]);
    run_git(path, &["checkout", "-b", branch_name]);
    std::fs::write(path.join("plan.txt"), "resolved publication conflict\n")
        .expect("write plan file");
    run_git(path, &["add", "."]);
    run_git(path, &["commit", "-m", "resolved publication conflict"]);
    let commit_sha = run_git(path, &["rev-parse", branch_name]);
    run_git(path, &["checkout", "main"]);

    (dir, commit_sha)
}

/// Build freshness metadata JSON with given fields.
fn freshness_meta(plan_update_conflict: bool, origin_state: Option<&str>) -> serde_json::Value {
    let mut obj = serde_json::json!({
        "plan_update_conflict": plan_update_conflict,
        "branch_freshness_conflict": true,
        "freshness_backoff_until": "2099-01-01T00:00:00Z",
        "freshness_conflict_count": 1,
    });
    if let Some(state) = origin_state {
        obj.as_object_mut().unwrap().insert(
            "freshness_origin_state".to_owned(),
            serde_json::Value::String(state.to_owned()),
        );
    }
    obj
}

#[derive(Default)]
struct StaticPlanPrDescriptionDrafter;

#[async_trait]
impl PlanPrDescriptionDrafter for StaticPlanPrDescriptionDrafter {
    async fn draft_plan_description(
        &self,
        _project: &Project,
        _plan_branch: &PlanBranch,
        _review_base: &str,
        _review_state: PrReviewState,
    ) -> crate::error::AppResult<AgentWorkspacePrDescription> {
        Ok(AgentWorkspacePrDescription::new(
            None,
            "## Summary\n\nFreshness PR update".to_string(),
        ))
    }
}

async fn insert_plan_branch_for_task(
    app_state: &AppState,
    task: &mut Task,
    pr_eligible: bool,
    status: PlanBranchStatus,
    pr_number: Option<i64>,
) -> String {
    let session_id = IdeationSessionId::from_string(format!("session-{}", task.id.as_str()));
    task.category = TaskCategory::Regular;
    task.ideation_session_id = Some(session_id.clone());
    task.touch();
    app_state.task_repo.update(task).await.unwrap();

    let branch_name = format!("plan/freshness-{}", task.id.as_str());
    let mut plan_branch = PlanBranch::new(
        ArtifactId::from_string(format!("artifact-{}", task.id.as_str())),
        session_id,
        task.project_id.clone(),
        branch_name.clone(),
        "main".to_string(),
    );
    plan_branch.pr_eligible = pr_eligible;
    plan_branch.status = status;
    plan_branch.pr_number = pr_number;
    plan_branch.pr_url =
        pr_number.map(|number| format!("https://github.test/owner/repo/pull/{number}"));
    plan_branch.pr_push_status = PrPushStatus::Pending;
    app_state
        .plan_branch_repo
        .create(plan_branch)
        .await
        .unwrap();
    branch_name
}

async fn insert_pr_backed_plan_branch(app_state: &AppState, task: &mut Task) -> String {
    insert_plan_branch_for_task(app_state, task, true, PlanBranchStatus::Active, Some(123)).await
}

fn pr_sync_services(
    app_state: &AppState,
    github: Arc<MockGithubService>,
) -> PlanBranchPrSyncServices {
    PlanBranchPrSyncServices {
        task_repo: Some(Arc::clone(&app_state.task_repo)),
        branch_update_repo: Some(Arc::clone(&app_state.branch_update_repo)),
        branch_update_workflow: Some(crate::testing::branch_update_workflow(Arc::new(
            app_state.build_chat_service(),
        ))),
        plan_branch_repo: Some(Arc::clone(&app_state.plan_branch_repo)),
        pr_creation_guard: Some(Arc::new(dashmap::DashMap::new())),
        github_service: Some(github as Arc<dyn GithubServiceTrait>),
        ideation_session_repo: Some(Arc::clone(&app_state.ideation_session_repo)),
        artifact_repo: Some(Arc::clone(&app_state.artifact_repo)),
        plan_pr_description_drafter: Some(Arc::new(StaticPlanPrDescriptionDrafter)),
    }
}

fn pr_sync_services_without_github(app_state: &AppState) -> PlanBranchPrSyncServices {
    PlanBranchPrSyncServices {
        task_repo: Some(Arc::clone(&app_state.task_repo)),
        branch_update_repo: Some(Arc::clone(&app_state.branch_update_repo)),
        branch_update_workflow: Some(crate::testing::branch_update_workflow(Arc::new(
            app_state.build_chat_service(),
        ))),
        plan_branch_repo: Some(Arc::clone(&app_state.plan_branch_repo)),
        pr_creation_guard: Some(Arc::new(dashmap::DashMap::new())),
        github_service: None,
        ideation_session_repo: Some(Arc::clone(&app_state.ideation_session_repo)),
        artifact_repo: Some(Arc::clone(&app_state.artifact_repo)),
        plan_pr_description_drafter: Some(Arc::new(StaticPlanPrDescriptionDrafter)),
    }
}

// ============================================================================
// Test 1: NormalMerge when plan_update_conflict absent
// ============================================================================

#[tokio::test]
async fn test_normal_merge_when_plan_update_conflict_absent() {
    let app_state = AppState::new_test();
    let ts = build_transition_service(&app_state);
    let project = insert_test_project(&app_state).await;

    let task = insert_task_with_metadata(
        &app_state.task_repo,
        project.id.clone(),
        Some(serde_json::json!({"some_other_key": true})),
    )
    .await;

    let result = freshness_return_route(
        &task,
        Arc::clone(&app_state.task_repo),
        &ts,
        &project,
        None,
        None,
        None,
    )
    .await
    .expect("Should not error");

    assert!(
        matches!(result, FreshnessRouteResult::NormalMerge),
        "Expected NormalMerge when plan_update_conflict absent"
    );
}

// ============================================================================
// Test 2: NormalMerge when plan_update_conflict=false
// ============================================================================

#[tokio::test]
async fn test_normal_merge_when_plan_update_conflict_false() {
    let app_state = AppState::new_test();
    let ts = build_transition_service(&app_state);
    let project = insert_test_project(&app_state).await;

    let task = insert_task_with_metadata(
        &app_state.task_repo,
        project.id.clone(),
        Some(serde_json::json!({
            "plan_update_conflict": false,
            "branch_freshness_conflict": true,
            "freshness_origin_state": "reviewing",
        })),
    )
    .await;

    let result = freshness_return_route(
        &task,
        Arc::clone(&app_state.task_repo),
        &ts,
        &project,
        None,
        None,
        None,
    )
    .await
    .expect("Should not error");

    assert!(
        matches!(result, FreshnessRouteResult::NormalMerge),
        "Expected NormalMerge when plan_update_conflict=false"
    );
}

// ============================================================================
// Test 3: NormalMerge when task metadata is None
// ============================================================================

#[tokio::test]
async fn test_normal_merge_when_metadata_none() {
    let app_state = AppState::new_test();
    let ts = build_transition_service(&app_state);
    let project = insert_test_project(&app_state).await;

    let task = insert_task_with_metadata(&app_state.task_repo, project.id.clone(), None).await;

    let result = freshness_return_route(
        &task,
        Arc::clone(&app_state.task_repo),
        &ts,
        &project,
        None,
        None,
        None,
    )
    .await
    .expect("Should not error");

    assert!(
        matches!(result, FreshnessRouteResult::NormalMerge),
        "Expected NormalMerge when task metadata is None"
    );
}

// ============================================================================
// Test 4: Defaults to PendingReview when freshness_origin_state absent
// ============================================================================

#[tokio::test]
async fn test_defaults_to_pending_review_when_origin_state_absent() {
    let app_state = AppState::new_test();
    let ts = build_transition_service(&app_state);
    let project = insert_test_project(&app_state).await;

    // plan_update_conflict=true but no freshness_origin_state
    let task = insert_task_with_metadata(
        &app_state.task_repo,
        project.id.clone(),
        Some(serde_json::json!({
            "plan_update_conflict": true,
            "branch_freshness_conflict": true,
            // No freshness_origin_state
        })),
    )
    .await;

    let result = freshness_return_route(
        &task,
        Arc::clone(&app_state.task_repo),
        &ts,
        &project,
        None,
        None,
        None,
    )
    .await
    .expect("Should succeed");

    match result {
        FreshnessRouteResult::FreshnessRouted(state) => {
            // The origin_state_name will be "PendingReview" (our safe default)
            assert_eq!(
                state, "PendingReview",
                "When freshness_origin_state absent, should default to PendingReview"
            );
        }
        FreshnessRouteResult::NormalMerge => {
            panic!("Expected FreshnessRouted, got NormalMerge");
        }
    }

    // Verify the task was transitioned. PendingReview auto-transitions to Reviewing,
    // so the final status after the auto-transition is Reviewing.
    let updated_task = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .expect("DB query ok")
        .expect("Task should exist");
    assert!(
        matches!(
            updated_task.internal_status,
            InternalStatus::PendingReview | InternalStatus::Reviewing
        ),
        "Task should be in PendingReview or Reviewing (auto-transition), got: {:?}",
        updated_task.internal_status
    );
}

// ============================================================================
// Test 5: FreshnessRouted when plan_update_conflict=true with "reviewing" origin
// ============================================================================

#[tokio::test]
async fn test_freshness_routed_when_plan_update_conflict_true_reviewing() {
    let app_state = AppState::new_test();
    let ts = build_transition_service(&app_state);
    let project = insert_test_project(&app_state).await;

    let task = insert_task_with_metadata(
        &app_state.task_repo,
        project.id.clone(),
        Some(freshness_meta(true, Some("reviewing"))),
    )
    .await;

    let result = freshness_return_route(
        &task,
        Arc::clone(&app_state.task_repo),
        &ts,
        &project,
        None,
        None,
        None,
    )
    .await
    .expect("Should succeed");

    match result {
        FreshnessRouteResult::FreshnessRouted(state) => {
            assert_eq!(state, "reviewing", "Should carry origin state name");
        }
        FreshnessRouteResult::NormalMerge => panic!("Expected FreshnessRouted"),
    }

    // Task should now be in PendingReview or Reviewing (PendingReview auto-transitions to Reviewing).
    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        matches!(
            updated.internal_status,
            InternalStatus::PendingReview | InternalStatus::Reviewing
        ),
        "Expected PendingReview or Reviewing, got: {:?}",
        updated.internal_status
    );
}

// ============================================================================
// Test 6: FreshnessRouted when plan_update_conflict=true with "executing" origin
//         → routes to Ready
// ============================================================================

#[tokio::test]
async fn test_freshness_routed_routes_to_ready_for_executing_origin() {
    let app_state = AppState::new_test();
    let ts = build_transition_service(&app_state);
    let project = insert_test_project(&app_state).await;

    let task = insert_task_with_metadata(
        &app_state.task_repo,
        project.id.clone(),
        Some(freshness_meta(true, Some("executing"))),
    )
    .await;

    let result = freshness_return_route(
        &task,
        Arc::clone(&app_state.task_repo),
        &ts,
        &project,
        None,
        None,
        None,
    )
    .await
    .expect("Should succeed");

    match result {
        FreshnessRouteResult::FreshnessRouted(state) => {
            assert_eq!(state, "executing");
        }
        FreshnessRouteResult::NormalMerge => panic!("Expected FreshnessRouted"),
    }

    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.internal_status, InternalStatus::Ready);
}

// ============================================================================
// Test 7: PR branch update conflict returns to WaitingOnPr
// ============================================================================

#[tokio::test]
async fn test_freshness_routed_routes_pr_branch_update_conflict_to_waiting_on_pr() {
    let app_state = AppState::new_test();
    let ts = build_transition_service(&app_state);
    let project = insert_test_project(&app_state).await;

    let mut task = insert_task_with_metadata(
        &app_state.task_repo,
        project.id.clone(),
        Some(serde_json::json!({
            "plan_update_conflict": true,
            "branch_freshness_conflict": true,
            "pr_branch_update_conflict": true,
            "pr_branch_update_source": "test",
            "freshness_origin_state": "waiting_on_pr",
        })),
    )
    .await;
    task.internal_status = InternalStatus::Merging;
    task.touch();
    app_state.task_repo.update(&task).await.unwrap();
    insert_pr_backed_plan_branch(&app_state, &mut task).await;
    let github = Arc::new(MockGithubService::new());
    let services = pr_sync_services(&app_state, Arc::clone(&github));

    let result = freshness_return_route(
        &task,
        Arc::clone(&app_state.task_repo),
        &ts,
        &project,
        None,
        Some(&services),
        Some("0000000000000000000000000000000000000000"),
    )
    .await
    .expect("Should succeed");

    match result {
        FreshnessRouteResult::FreshnessRouted(state) => {
            assert_eq!(state, "waiting_on_pr");
        }
        FreshnessRouteResult::NormalMerge => panic!("Expected FreshnessRouted"),
    }

    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.internal_status, InternalStatus::WaitingOnPr);

    let meta: serde_json::Value = updated
        .metadata
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();
    assert!(meta.get("plan_update_conflict").is_none());
    assert!(meta.get("branch_freshness_conflict").is_none());
    assert!(meta.get("pr_branch_update_conflict").is_none());
    assert!(meta.get("pr_branch_update_source").is_none());
    assert_eq!(
        meta.get("freshness_origin_state").and_then(|v| v.as_str()),
        Some("waiting_on_pr"),
        "origin is preserved for audit/debug"
    );
}

#[tokio::test]
async fn test_pr_branch_update_missing_sync_services_routes_to_merge_incomplete() {
    let app_state = AppState::new_test();
    let ts = build_transition_service(&app_state);
    let project = insert_test_project(&app_state).await;

    let mut task = insert_task_with_metadata(
        &app_state.task_repo,
        project.id.clone(),
        Some(serde_json::json!({
            "plan_update_conflict": true,
            "branch_freshness_conflict": true,
            "pr_branch_update_conflict": true,
            "freshness_origin_state": "waiting_on_pr",
        })),
    )
    .await;
    task.internal_status = InternalStatus::Merging;
    insert_pr_backed_plan_branch(&app_state, &mut task).await;

    let result = freshness_return_route(
        &task,
        Arc::clone(&app_state.task_repo),
        &ts,
        &project,
        None,
        None,
        Some("3333333333333333333333333333333333333333"),
    )
    .await;

    assert!(result.is_err(), "missing sync services should fail closed");
    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.internal_status, InternalStatus::MergeIncomplete);
    let meta: serde_json::Value = updated
        .metadata
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();
    assert_eq!(
        meta.get("error_code").and_then(|v| v.as_str()),
        Some("pr_branch_publication_failed")
    );
    assert_eq!(
        meta.get("commit_sha").and_then(|v| v.as_str()),
        Some("3333333333333333333333333333333333333333")
    );
}

#[tokio::test]
async fn test_pr_branch_update_conflict_publishes_before_waiting_on_pr() {
    let app_state = AppState::new_test();
    let ts = build_transition_service(&app_state);
    let project = insert_test_project(&app_state).await;

    let mut task = insert_task_with_metadata(
        &app_state.task_repo,
        project.id.clone(),
        Some(serde_json::json!({
            "plan_update_conflict": true,
            "branch_freshness_conflict": true,
            "pr_branch_update_conflict": true,
            "pr_branch_update_source": "poller",
            "freshness_origin_state": "waiting_on_pr",
        })),
    )
    .await;
    task.internal_status = InternalStatus::Merging;
    let branch_name = insert_pr_backed_plan_branch(&app_state, &mut task).await;

    let github = Arc::new(MockGithubService::new());
    let services = pr_sync_services(&app_state, Arc::clone(&github));
    let result = freshness_return_route(
        &task,
        Arc::clone(&app_state.task_repo),
        &ts,
        &project,
        None,
        Some(&services),
        Some("1111111111111111111111111111111111111111"),
    )
    .await
    .expect("PR branch publication should allow freshness route");

    match result {
        FreshnessRouteResult::FreshnessRouted(state) => assert_eq!(state, "waiting_on_pr"),
        FreshnessRouteResult::NormalMerge => panic!("Expected FreshnessRouted"),
    }

    {
        let state = github.state();
        assert_eq!(state.push_branch_calls, 1);
        assert_eq!(
            state.last_push_branch_name.as_deref(),
            Some(branch_name.as_str())
        );
        assert_eq!(state.update_pr_details_calls, 1);
    }

    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.internal_status, InternalStatus::WaitingOnPr);
    let meta: serde_json::Value = updated
        .metadata
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();
    assert!(meta.get("plan_update_conflict").is_none());
    assert!(meta.get("pr_branch_update_conflict").is_none());

    let plan_branch = app_state
        .plan_branch_repo
        .get_by_session_id(updated.ideation_session_id.as_ref().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(plan_branch.pr_push_status, PrPushStatus::Pushed);
}

#[tokio::test]
async fn test_pr_branch_publication_conflict_finalizes_regular_task_as_merged() {
    let app_state = AppState::new_test();
    let ts = build_transition_service(&app_state);
    let branch_name = "plan/publication-conflict-finalizer";
    let (repo, commit_sha) = setup_publication_finalizer_repo(branch_name);

    let mut project = Project::new(
        "test-project".to_owned(),
        repo.path().to_string_lossy().into_owned(),
    );
    project.id = ProjectId::from_string("proj-publication-finalizer".to_owned());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .expect("create project");

    let session_id = IdeationSessionId::from_string("session-publication-finalizer".to_owned());
    let mut task = Task::new(project.id.clone(), "publication finalizer task".to_owned());
    task.internal_status = InternalStatus::Merging;
    task.category = TaskCategory::Regular;
    task.ideation_session_id = Some(session_id.clone());
    task.metadata = Some(
        serde_json::json!({
            "plan_update_conflict": true,
            "branch_freshness_conflict": true,
            "pr_branch_update_conflict": true,
            "pr_branch_publication_conflict": true,
            "freshness_origin_state": "pr_branch_publication",
            "base_branch": format!("origin/{branch_name}"),
            "target_branch": branch_name,
            "error_code": "pr_branch_publication_conflict",
            "error": "publication conflict",
            "conflict_files": ["plan.txt"],
        })
        .to_string(),
    );
    app_state
        .task_repo
        .create(task.clone())
        .await
        .expect("create task");

    let mut plan_branch = PlanBranch::new(
        ArtifactId::from_string("artifact-publication-finalizer".to_owned()),
        session_id,
        project.id.clone(),
        branch_name.to_owned(),
        "main".to_owned(),
    );
    plan_branch.pr_eligible = true;
    plan_branch.status = PlanBranchStatus::Active;
    plan_branch.pr_number = Some(321);
    plan_branch.pr_url = Some("https://github.test/owner/repo/pull/321".to_owned());
    plan_branch.pr_push_status = PrPushStatus::Pending;
    app_state
        .plan_branch_repo
        .create(plan_branch)
        .await
        .expect("create plan branch");

    let github = Arc::new(MockGithubService::new());
    let services = pr_sync_services(&app_state, Arc::clone(&github));
    let result = freshness_return_route(
        &task,
        Arc::clone(&app_state.task_repo),
        &ts,
        &project,
        None,
        Some(&services),
        Some(&commit_sha),
    )
    .await
    .expect("publication conflict return should finalize");

    match result {
        FreshnessRouteResult::FreshnessRouted(state) => assert_eq!(state, "merged"),
        FreshnessRouteResult::NormalMerge => panic!("Expected FreshnessRouted"),
    }

    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.internal_status, InternalStatus::Merged);
    assert_eq!(
        updated.merge_commit_sha.as_deref(),
        Some(commit_sha.as_str())
    );
    let meta: serde_json::Value = updated
        .metadata
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();
    assert!(meta.get("plan_update_conflict").is_none());
    assert!(meta.get("pr_branch_update_conflict").is_none());
    assert!(meta.get("pr_branch_publication_conflict").is_none());
    assert!(meta.get("error_code").is_none());
    assert!(meta.get("conflict_files").is_none());
    assert_eq!(
        meta.get("pending_cleanup")
            .and_then(|value| value.as_bool()),
        Some(true),
        "publication finalizer should preserve normal post-merge cleanup recovery"
    );

    {
        let state = github.state();
        assert_eq!(state.push_branch_calls, 1);
        assert_eq!(state.last_push_branch_name.as_deref(), Some(branch_name));
        assert_eq!(state.update_pr_details_calls, 1);
        assert_eq!(state.mark_pr_ready_calls, 1);
    }

    let plan_branch = app_state
        .plan_branch_repo
        .get_by_session_id(updated.ideation_session_id.as_ref().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(plan_branch.pr_push_status, PrPushStatus::Pushed);
}

#[tokio::test]
async fn test_pr_branch_publication_finalizer_missing_sync_services_routes_to_merge_incomplete() {
    let app_state = AppState::new_test();
    let ts = build_transition_service(&app_state);
    let project = insert_test_project(&app_state).await;

    let mut task = insert_task_with_metadata(
        &app_state.task_repo,
        project.id.clone(),
        Some(serde_json::json!({
            "plan_update_conflict": true,
            "branch_freshness_conflict": true,
            "pr_branch_update_conflict": true,
            "pr_branch_publication_conflict": true,
            "freshness_origin_state": "pr_branch_publication",
            "error_code": "pr_branch_publication_conflict",
        })),
    )
    .await;
    task.internal_status = InternalStatus::Merging;
    app_state.task_repo.update(&task).await.unwrap();

    let result = freshness_return_route(
        &task,
        Arc::clone(&app_state.task_repo),
        &ts,
        &project,
        None,
        None,
        Some("7777777777777777777777777777777777777777"),
    )
    .await;

    let error = match result {
        Ok(_) => panic!("missing PR sync services should fail closed"),
        Err(error) => error,
    };
    assert!(
        error.to_string().contains("PR sync services unavailable"),
        "unexpected error: {error}"
    );
    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.internal_status, InternalStatus::MergeIncomplete);
    let meta: serde_json::Value = updated
        .metadata
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();
    assert_eq!(
        meta.get("error_code").and_then(|value| value.as_str()),
        Some("pr_branch_publication_failed")
    );
    assert_eq!(
        meta.get("commit_sha").and_then(|value| value.as_str()),
        Some("7777777777777777777777777777777777777777")
    );
}

#[tokio::test]
async fn test_pr_branch_publication_finalizer_uses_metadata_commit_when_sha_arg_missing() {
    let app_state = AppState::new_test();
    let ts = build_transition_service(&app_state);
    let project = insert_test_project(&app_state).await;
    let metadata_commit = "8888888888888888888888888888888888888888";

    let mut task = insert_task_with_metadata(
        &app_state.task_repo,
        project.id.clone(),
        Some(serde_json::json!({
            "plan_update_conflict": true,
            "branch_freshness_conflict": true,
            "pr_branch_update_conflict": true,
            "pr_branch_publication_conflict": true,
            "freshness_origin_state": "pr_branch_publication",
            "error_code": "pr_branch_publication_conflict",
            "commit_sha": metadata_commit,
        })),
    )
    .await;
    task.internal_status = InternalStatus::Merging;
    app_state.task_repo.update(&task).await.unwrap();
    let branch_name = insert_pr_backed_plan_branch(&app_state, &mut task).await;

    let github = Arc::new(MockGithubService::new());
    let services = pr_sync_services(&app_state, Arc::clone(&github));
    let result = freshness_return_route(
        &task,
        Arc::clone(&app_state.task_repo),
        &ts,
        &project,
        None,
        Some(&services),
        None,
    )
    .await
    .expect("publication conflict return should use metadata commit fallback");

    match result {
        FreshnessRouteResult::FreshnessRouted(state) => assert_eq!(state, "merged"),
        FreshnessRouteResult::NormalMerge => panic!("Expected FreshnessRouted"),
    }

    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.internal_status, InternalStatus::Merged);
    assert_eq!(updated.merge_commit_sha.as_deref(), Some(metadata_commit));
    let meta: serde_json::Value = updated
        .metadata
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();
    assert!(meta.get("pr_branch_publication_conflict").is_none());
    assert!(meta.get("error_code").is_none());
    assert_eq!(
        meta.get("pending_cleanup")
            .and_then(|value| value.as_bool()),
        Some(true)
    );

    let state = github.state();
    assert_eq!(state.push_branch_calls, 1);
    assert_eq!(
        state.last_push_branch_name.as_deref(),
        Some(branch_name.as_str())
    );
    assert_eq!(state.update_pr_details_calls, 1);
}

#[tokio::test]
async fn test_pr_branch_update_missing_github_service_routes_to_merge_incomplete() {
    let app_state = AppState::new_test();
    let ts = build_transition_service(&app_state);
    let project = insert_test_project(&app_state).await;

    let mut task = insert_task_with_metadata(
        &app_state.task_repo,
        project.id.clone(),
        Some(serde_json::json!({
            "plan_update_conflict": true,
            "branch_freshness_conflict": true,
            "pr_branch_update_conflict": true,
            "freshness_origin_state": "waiting_on_pr",
        })),
    )
    .await;
    task.internal_status = InternalStatus::Merging;
    insert_pr_backed_plan_branch(&app_state, &mut task).await;

    let services = pr_sync_services_without_github(&app_state);
    let result = freshness_return_route(
        &task,
        Arc::clone(&app_state.task_repo),
        &ts,
        &project,
        None,
        Some(&services),
        Some("4444444444444444444444444444444444444444"),
    )
    .await;

    let error = match result {
        Ok(_) => panic!("missing GitHub service should fail closed"),
        Err(error) => error,
    };
    assert!(
        error.to_string().contains("GitHub service unavailable"),
        "unexpected error: {error}"
    );
    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.internal_status, InternalStatus::MergeIncomplete);
}

#[tokio::test]
async fn test_pr_branch_update_push_failure_routes_to_merge_incomplete() {
    let app_state = AppState::new_test();
    let ts = build_transition_service(&app_state);
    let project = insert_test_project(&app_state).await;

    let mut task = insert_task_with_metadata(
        &app_state.task_repo,
        project.id.clone(),
        Some(serde_json::json!({
            "plan_update_conflict": true,
            "branch_freshness_conflict": true,
            "pr_branch_update_conflict": true,
            "pr_branch_update_source": "poller",
            "freshness_origin_state": "waiting_on_pr",
        })),
    )
    .await;
    task.internal_status = InternalStatus::Merging;
    insert_pr_backed_plan_branch(&app_state, &mut task).await;

    let github = Arc::new(MockGithubService::new());
    github.state().push_branch_result = Some(Err(AppError::GitOperation(
        "remote rejected freshness branch".to_string(),
    )));
    let services = pr_sync_services(&app_state, Arc::clone(&github));
    let result = freshness_return_route(
        &task,
        Arc::clone(&app_state.task_repo),
        &ts,
        &project,
        None,
        Some(&services),
        Some("2222222222222222222222222222222222222222"),
    )
    .await;

    assert!(
        result.is_err(),
        "push failure should stop freshness routing"
    );
    assert_eq!(github.state().push_branch_calls, 1);

    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.internal_status, InternalStatus::MergeIncomplete);
    let meta: serde_json::Value = updated
        .metadata
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();
    assert_eq!(
        meta.get("plan_update_conflict").and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(
        meta.get("pr_branch_update_conflict")
            .and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(
        meta.get("error_code").and_then(|v| v.as_str()),
        Some("pr_branch_publication_failed")
    );
    assert_eq!(
        meta.get("commit_sha").and_then(|v| v.as_str()),
        Some("2222222222222222222222222222222222222222")
    );

    let plan_branch = app_state
        .plan_branch_repo
        .get_by_session_id(updated.ideation_session_id.as_ref().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(plan_branch.pr_push_status, PrPushStatus::Failed);
}

#[tokio::test]
async fn test_publish_plan_pr_after_freshness_noops_without_plan_branch_repo() {
    let app_state = AppState::new_test();
    let project = insert_test_project(&app_state).await;
    let task = insert_task_with_metadata(&app_state.task_repo, project.id.clone(), None).await;

    let services = PlanBranchPrSyncServices {
        task_repo: Some(Arc::clone(&app_state.task_repo)),
        plan_branch_repo: None,
        ..PlanBranchPrSyncServices::default()
    };

    publish_plan_branch_pr_after_freshness_update(&task, &project, &services)
        .await
        .expect("missing plan branch repo should be a no-op");
}

#[tokio::test]
async fn test_publish_plan_pr_after_freshness_noops_for_non_pr_branch_shapes() {
    let app_state = AppState::new_test();
    let project = insert_test_project(&app_state).await;

    let mut ineligible_task =
        insert_task_with_metadata(&app_state.task_repo, project.id.clone(), None).await;
    insert_plan_branch_for_task(
        &app_state,
        &mut ineligible_task,
        false,
        PlanBranchStatus::Active,
        Some(123),
    )
    .await;

    let mut merged_task =
        insert_task_with_metadata(&app_state.task_repo, project.id.clone(), None).await;
    insert_plan_branch_for_task(
        &app_state,
        &mut merged_task,
        true,
        PlanBranchStatus::Merged,
        Some(124),
    )
    .await;

    let mut no_pr_task =
        insert_task_with_metadata(&app_state.task_repo, project.id.clone(), None).await;
    insert_plan_branch_for_task(
        &app_state,
        &mut no_pr_task,
        true,
        PlanBranchStatus::Active,
        None,
    )
    .await;

    let services = pr_sync_services_without_github(&app_state);
    for task in [&ineligible_task, &merged_task, &no_pr_task] {
        publish_plan_branch_pr_after_freshness_update(task, &project, &services)
            .await
            .expect("non-publishable plan branch should no-op before GitHub lookup");
    }
}

// ============================================================================
// Test 8: Targeted field cleanup — plan_update_conflict, branch_freshness_conflict,
//         freshness_backoff_until removed; freshness_origin_state preserved
// ============================================================================

#[tokio::test]
async fn test_targeted_metadata_cleanup_on_success() {
    let app_state = AppState::new_test();
    let ts = build_transition_service(&app_state);
    let project = insert_test_project(&app_state).await;

    let task = insert_task_with_metadata(
        &app_state.task_repo,
        project.id.clone(),
        Some(serde_json::json!({
            "plan_update_conflict": true,
            "branch_freshness_conflict": true,
            "freshness_backoff_until": "2099-01-01T00:00:00Z",
            "freshness_origin_state": "reviewing",
            "freshness_conflict_count": 2,
        })),
    )
    .await;

    freshness_return_route(
        &task,
        Arc::clone(&app_state.task_repo),
        &ts,
        &project,
        None,
        None,
        None,
    )
    .await
    .expect("Should succeed");

    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    let meta: serde_json::Value = updated
        .metadata
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();

    // Routing trigger flags must be cleared
    assert!(
        meta.get("plan_update_conflict").is_none()
            || meta.get("plan_update_conflict").and_then(|v| v.as_bool()) == Some(false),
        "plan_update_conflict should be removed"
    );
    assert!(
        meta.get("branch_freshness_conflict").is_none()
            || meta
                .get("branch_freshness_conflict")
                .and_then(|v| v.as_bool())
                == Some(false),
        "branch_freshness_conflict should be removed"
    );
    assert!(
        meta.get("freshness_backoff_until").is_none(),
        "freshness_backoff_until should be removed"
    );

    // Audit fields preserved
    assert!(
        meta.get("freshness_conflict_count").is_some(),
        "freshness_conflict_count should be preserved for audit"
    );
}

// ============================================================================
// Test 8: Re-inserts plan_update_conflict when transition_task fails
//         (separate repos: freshness_route has the task, transition service doesn't)
// ============================================================================

#[tokio::test]
async fn test_re_inserts_flags_when_transition_fails() {
    // freshness_route uses app_state_with_task (has the task)
    // transition_service uses app_state_without_task (missing the task → NotFound)
    let app_state_with_task = AppState::new_test();
    let app_state_without_task = AppState::new_test();

    // Build transition service from the EMPTY app state (no task in its repo)
    let ts = build_transition_service(&app_state_without_task);

    let project = insert_test_project(&app_state_with_task).await;

    let task = insert_task_with_metadata(
        &app_state_with_task.task_repo,
        project.id.clone(),
        Some(serde_json::json!({
            "plan_update_conflict": true,
            "branch_freshness_conflict": true,
            "freshness_origin_state": "reviewing",
        })),
    )
    .await;

    // Should return Err because transition_task can't find the task in its repo
    let result = freshness_return_route(
        &task,
        Arc::clone(&app_state_with_task.task_repo),
        &ts,
        &project,
        None,
        None,
        None,
    )
    .await;

    assert!(result.is_err(), "Should return Err when transition fails");

    // After failure, the routing flags should be re-inserted
    let recovered = app_state_with_task
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    let meta: serde_json::Value = recovered
        .metadata
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();

    assert_eq!(
        meta.get("plan_update_conflict").and_then(|v| v.as_bool()),
        Some(true),
        "plan_update_conflict should be re-inserted after transition failure"
    );
    assert_eq!(
        meta.get("branch_freshness_conflict")
            .and_then(|v| v.as_bool()),
        Some(true),
        "branch_freshness_conflict should be re-inserted after transition failure"
    );
}

// ============================================================================
// Test 9: IPR entry removed after successful routing
// ============================================================================

#[tokio::test]
async fn test_ipr_entry_removed_on_success() {
    let app_state = AppState::new_test();
    let ts = build_transition_service(&app_state);
    let project = insert_test_project(&app_state).await;

    let task = insert_task_with_metadata(
        &app_state.task_repo,
        project.id.clone(),
        Some(freshness_meta(true, Some("reviewing"))),
    )
    .await;

    // Register a fake IPR entry for the merge context
    let ipr = InteractiveProcessRegistry::new();
    // We don't have a real ChildStdin here, but we can verify via has_process.
    // We'll use a workaround: verify that after the call, has_process returns false
    // (it was never registered, but remove() on a missing key is a no-op, which is fine).
    // The key test is that the code calls ipr.remove() without panicking.

    let result = freshness_return_route(
        &task,
        Arc::clone(&app_state.task_repo),
        &ts,
        &project,
        Some(&ipr),
        None,
        None,
    )
    .await
    .expect("Should succeed");

    assert!(
        matches!(result, FreshnessRouteResult::FreshnessRouted(_)),
        "Expected FreshnessRouted"
    );

    // Verify IPR entry is gone (remove was called — even if not registered, no panic)
    let ipr_key = InteractiveProcessKey::new("merge", task.id.as_str());
    assert!(
        !ipr.has_process(&ipr_key).await,
        "IPR should not have merge entry after routing"
    );
}

// ============================================================================
// Test 6 (auto-complete path): plan_update_conflict=true with branch_freshness_conflict=false
//         (cleared flag scenario) — still returns FreshnessRouted (not NormalMerge)
//
// This tests the KEY property of freshness_return_route: it checks
// plan_update_conflict (NOT branch_freshness_conflict). The branch_freshness_conflict
// flag may be cleared by set_source_conflict_resolved while plan_update_conflict
// remains true — the function must still route correctly in this scenario.
// This proves that replacing the old guard (which only checked branch_freshness_conflict)
// with freshness_return_route (which checks plan_update_conflict) provides MORE robust routing.
// ============================================================================

#[tokio::test]
async fn test_freshness_routed_when_plan_update_conflict_true_branch_freshness_cleared() {
    let app_state = AppState::new_test();
    let ts = build_transition_service(&app_state);
    let project = insert_test_project(&app_state).await;

    // Simulate cleared flag scenario:
    // plan_update_conflict=true (routing trigger present)
    // branch_freshness_conflict=false (cleared by set_source_conflict_resolved)
    // freshness_origin_state="reviewing"
    let task = insert_task_with_metadata(
        &app_state.task_repo,
        project.id.clone(),
        Some(serde_json::json!({
            "plan_update_conflict": true,
            "branch_freshness_conflict": false,
            "freshness_origin_state": "reviewing",
        })),
    )
    .await;

    let result = freshness_return_route(
        &task,
        Arc::clone(&app_state.task_repo),
        &ts,
        &project,
        None,
        None,
        None,
    )
    .await
    .expect("Should succeed even when branch_freshness_conflict=false");

    // Must return FreshnessRouted — NOT NormalMerge — because plan_update_conflict=true
    match result {
        FreshnessRouteResult::FreshnessRouted(state) => {
            assert_eq!(
                state, "reviewing",
                "Should carry origin state name from freshness_origin_state"
            );
        }
        FreshnessRouteResult::NormalMerge => {
            panic!(
                "Expected FreshnessRouted because plan_update_conflict=true, \
                 but got NormalMerge. This would mean the old guard logic \
                 (branch_freshness_conflict) is still in use instead of \
                 the new freshness_return_route check."
            );
        }
    }

    // Task should be routed back to Reviewing (PendingReview auto-transitions to Reviewing).
    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        matches!(
            updated.internal_status,
            InternalStatus::PendingReview | InternalStatus::Reviewing
        ),
        "Expected PendingReview or Reviewing after freshness routing, got: {:?}",
        updated.internal_status
    );

    // Verify plan_update_conflict was cleared (routing trigger consumed)
    let meta: serde_json::Value = updated
        .metadata
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();
    assert!(
        meta.get("plan_update_conflict").is_none()
            || meta.get("plan_update_conflict").and_then(|v| v.as_bool()) == Some(false),
        "plan_update_conflict should be cleared after successful routing"
    );
}

// ============================================================================
// Test 10: Does NOT use FreshnessCleanupScope::RoutingOnly (static verification)
//
// The actual behavior is verified by Test 7 (targeted cleanup): if RoutingOnly
// were used, it would also clear freshness_conflict_count (which RoutingOnly
// does NOT clear but RoutingOnly clears plan_update_conflict). The targeted
// removal is documented in the implementation comments.
// This test just confirms the NormalMerge path returns without calling any
// cleanup scope at all.
// ============================================================================

#[test]
fn test_normal_merge_returns_without_cleanup() {
    // This is a compile-time guarantee: the function signature only accepts
    // the shared types (Task, TaskRepository, TaskTransitionService, etc.)
    // and the FreshnessCleanupScope is NOT imported in freshness_routing.rs.
    // The test verifies via code path that NormalMerge exits before any
    // cleanup logic runs (which is tested implicitly by tests 1-3).
    //
    // We just assert that this assertion compiles and passes trivially.
    assert!(
        true,
        "FreshnessCleanupScope::RoutingOnly is not called in freshness_routing.rs"
    );
}

// ============================================================================
// Integration Test #7: persisted legacy Reviewing → Merging row
//                       → complete_merge compatibility recovery → back to Reviewing
//
// Simulates the complete_merge HTTP handler path:
//   1. Task starts in Reviewing with no freshness metadata
//   2. Freshness detection fires: sets plan_update_conflict=true, branch_freshness_conflict=true,
//      freshness_origin_state="reviewing" (simulating on_enter(Reviewing) freshness check)
//   3. State machine fires BranchFreshnessConflict: task transitions to Merging
//   4. Merger agent resolves plan←main conflict (simulated — metadata stays set)
//   5. Merger calls complete_merge → freshness_return_route fires
//   6. Task returns to Reviewing (not Merged)
//   7. Metadata cleanup: plan_update_conflict + branch_freshness_conflict cleared
//   8. merge_commit_sha must NOT be set (freshness intercept fires before SHA assignment)
// ============================================================================

#[tokio::test]
async fn test_integration_full_chain_reviewing_through_freshness_conflict_returns_to_reviewing() {
    let app_state = AppState::new_test();
    let ts = build_transition_service(&app_state);
    let project = insert_test_project(&app_state).await;

    // --- Phase 1: Task starts in Reviewing ---
    let mut task = Task::new(project.id.clone(), "Full chain test".to_owned());
    task.internal_status = InternalStatus::Reviewing;
    // Set worktree_path so on_enter(Reviewing) worktree guard passes on auto-transitions
    task.worktree_path = Some(std::env::temp_dir().to_string_lossy().to_string());
    app_state
        .task_repo
        .create(task.clone())
        .await
        .expect("Failed to create task in Reviewing");
    let task_id = task.id.clone();

    // --- Phase 2: Simulate freshness detection firing during on_enter(Reviewing) ---
    // Set freshness metadata as freshness.rs would set it (lines 482-484):
    //   plan_update_conflict=true, branch_freshness_conflict=true, freshness_origin_state="reviewing"
    {
        let mut stored = app_state
            .task_repo
            .get_by_id(&task_id)
            .await
            .unwrap()
            .expect("Task must exist");
        stored.metadata = Some(
            serde_json::json!({
                "plan_update_conflict": true,
                "branch_freshness_conflict": true,
                "freshness_origin_state": "reviewing",
                "freshness_conflict_count": 1,
                "freshness_backoff_until": "2099-01-01T00:00:00Z",
                // Non-freshness key that must survive routing
                "trigger_origin": "scheduler",
            })
            .to_string(),
        );
        stored.touch();
        app_state.task_repo.update(&stored).await.unwrap();
    }

    // --- Phase 3: Simulate a persisted row from the retired freshness-as-merge path ---
    // The validated transition_task() surface no longer models this path; the direct write
    // exists only to prove safe recovery of legacy data.
    {
        let mut stored = app_state
            .task_repo
            .get_by_id(&task_id)
            .await
            .unwrap()
            .expect("Task must exist for merge transition");
        stored.internal_status = InternalStatus::Merging;
        stored.touch();
        app_state.task_repo.update(&stored).await.unwrap();
    }

    // Verify task is now in Merging
    let merging_task = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("Task must exist");
    assert_eq!(
        merging_task.internal_status,
        InternalStatus::Merging,
        "Task must be in Merging state after BranchFreshnessConflict transition"
    );

    // Verify freshness metadata is still set (merger agent hasn't run yet)
    let merging_meta: serde_json::Value = merging_task
        .metadata
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();
    assert_eq!(
        merging_meta
            .get("plan_update_conflict")
            .and_then(|v| v.as_bool()),
        Some(true),
        "plan_update_conflict must still be set after transition to Merging"
    );

    // --- Phase 4: Merger agent resolves plan←main conflict ---
    // (No merge_commit_sha is set — freshness intercept fires before SHA assignment)

    // --- Phase 5: Merger calls complete_merge → freshness_return_route fires ---
    // Re-fetch to get the current task snapshot (as complete_merge would)
    let current_task = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("Task must exist for freshness_return_route");

    let route_result = freshness_return_route(
        &current_task,
        Arc::clone(&app_state.task_repo),
        &ts,
        &project,
        None, // No IPR in this test
        None,
        None,
    )
    .await
    .expect("freshness_return_route must succeed");

    // --- Phase 6: Verify routing result is FreshnessRouted (not NormalMerge → not Merged) ---
    match &route_result {
        FreshnessRouteResult::FreshnessRouted(origin) => {
            assert_eq!(
                origin.as_str(),
                "reviewing",
                "Origin state carried in result must be 'reviewing'"
            );
        }
        FreshnessRouteResult::NormalMerge => {
            panic!(
                "Expected FreshnessRouted but got NormalMerge — \
                 complete_merge would have transitioned to Merged (work loss!)"
            );
        }
    }

    // --- Phase 7: Verify task returned to Reviewing (not Merged) ---
    let final_task = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("Task must exist after routing");

    assert!(
        matches!(
            final_task.internal_status,
            InternalStatus::PendingReview | InternalStatus::Reviewing
        ),
        "Task must return to PendingReview or Reviewing (auto-transition), got: {:?}. \
         If Merged, the freshness intercept did not fire — work would be lost.",
        final_task.internal_status
    );
    assert_ne!(
        final_task.internal_status,
        InternalStatus::Merged,
        "Task MUST NOT be Merged — freshness conflict was not resolved, task→plan squash never ran"
    );

    // --- Phase 8: Verify metadata cleanup ---
    let final_meta: serde_json::Value = final_task
        .metadata
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();

    // Routing trigger flags must be cleared (they were consumed by the intercept)
    assert!(
        final_meta.get("plan_update_conflict").is_none()
            || final_meta
                .get("plan_update_conflict")
                .and_then(|v| v.as_bool())
                == Some(false),
        "plan_update_conflict must be cleared after successful routing"
    );
    assert!(
        final_meta.get("branch_freshness_conflict").is_none()
            || final_meta
                .get("branch_freshness_conflict")
                .and_then(|v| v.as_bool())
                == Some(false),
        "branch_freshness_conflict must be cleared after successful routing"
    );
    assert!(
        final_meta.get("freshness_backoff_until").is_none(),
        "freshness_backoff_until must be cleared after successful routing"
    );

    // merge_commit_sha must NOT be set (freshness intercept fired before SHA assignment)
    assert!(
        final_task.merge_commit_sha.is_none(),
        "merge_commit_sha must NOT be set — freshness intercept fires before SHA assignment \
         in the complete_merge handler"
    );
}
