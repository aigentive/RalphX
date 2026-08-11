// PR-mode state machine integration tests
//
// Tests for Phase 3 PR integration: PendingMerge + Merging + Merged paths
// when pr_eligible=true and GithubServiceTrait is wired.
//
// Covered scenarios:
//   1. PR-mode with existing pr_number: push_branch + mark_pr_ready, no create_draft_pr
//   2. PR-mode without pr_number: create_draft_pr + mark_pr_ready
//   3. pr_eligible=false: skips PR path entirely (no github calls)
//   4. Re-entry guard: pr_polling_active=true, no registry → proceeds normally
//   5. AD14: PR-polling task in Merging does not block a second PendingMerge task
//   6. post_merge_cleanup idempotency: plan_branch.status == Merged → early return

use super::helpers::*;
use crate::application::PrPollerRegistry;
use crate::domain::entities::plan_branch::{PrPushStatus, PrStatus};
use crate::domain::entities::task_metadata::{
    MergeFailureSource, MergeRecoveryEventKind, MergeRecoveryMetadata,
};
use crate::domain::entities::{
    types::IdeationSessionId, Artifact, ArtifactId, ArtifactType, IdeationSession, InternalStatus,
    PlanBranch, PlanBranchStatus, Project, ProjectId, Task, TaskCategory, TaskId,
};
use crate::domain::repositories::{
    ArtifactRepository, BranchUpdateRepository, IdeationSessionRepository, PlanBranchRepository,
    ProjectRepository, TaskRepository,
};
use crate::domain::services::{
    github_service::GithubServiceTrait, PlanPrDescriptionDrafter, PrReviewState,
};
use crate::domain::state_machine::services::{NotificationContext, Notifier, TaskNotification};
use crate::domain::state_machine::transition_handler::{
    complete_merge_internal_with_pr_sync_and_notifier, PlanBranchPrSyncServices,
};
use crate::domain::state_machine::{State, TransitionHandler};
use crate::infrastructure::memory::{
    MemoryArtifactRepository, MemoryIdeationSessionRepository, MemoryPlanBranchRepository,
    MemoryProjectRepository, MemoryTaskRepository,
};
use crate::testing::{SqliteBranchUpdateRepository, SqliteTaskRepository, SqliteTestDb};
use crate::tests::mock_github_service::MockGithubService;
use crate::AppError;
use async_trait::async_trait;
use serde_json::Value;

// ─────────────────────────────────────────────────────────────────────────────
// Shared helpers
// ─────────────────────────────────────────────────────────────────────────────

fn make_pr_eligible_plan_branch(
    task_id: &TaskId,
    pr_number: Option<i64>,
    pr_polling_active: bool,
) -> PlanBranch {
    let mut pb = PlanBranch::new(
        ArtifactId::from_string("artifact-1".to_string()),
        IdeationSessionId::from_string("sess-1".to_string()),
        ProjectId::from_string("proj-1".to_string()),
        "plan/feature-branch".to_string(),
        "main".to_string(), // source_branch = base branch (PR target)
    );
    pb.merge_task_id = Some(task_id.clone());
    pb.pr_eligible = true;
    pb.pr_number = pr_number;
    pb.pr_polling_active = pr_polling_active;
    pb
}

async fn setup_project(project_repo: &MemoryProjectRepository) {
    setup_project_with_path(project_repo, "/tmp/pr-mode-test".to_string()).await;
}

async fn setup_project_with_path(
    project_repo: &MemoryProjectRepository,
    working_directory: String,
) {
    let mut project = Project::new("test-project".to_string(), working_directory);
    project.id = ProjectId::from_string("proj-1".to_string());
    project.base_branch = Some("main".to_string());
    project_repo.create(project).await.unwrap();
}

fn setup_plan_git_repo(branch_name: &str, ahead_of_base: bool) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path();

    std::process::Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(path)
        .output()
        .expect("git init");
    std::process::Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(path)
        .output()
        .expect("set git email");
    std::process::Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(path)
        .output()
        .expect("set git name");

    std::fs::write(path.join("README.md"), "# pr mode state machine repo\n").expect("write README");
    std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(path)
        .output()
        .expect("git add");
    std::process::Command::new("git")
        .args(["commit", "-m", "initial commit"])
        .current_dir(path)
        .output()
        .expect("initial commit");

    std::process::Command::new("git")
        .args(["checkout", "-b", branch_name])
        .current_dir(path)
        .output()
        .expect("create plan branch");
    if ahead_of_base {
        std::fs::write(path.join("plan.txt"), "plan branch work\n").expect("write plan file");
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(path)
            .output()
            .expect("git add plan file");
        std::process::Command::new("git")
            .args(["commit", "-m", "plan branch work"])
            .current_dir(path)
            .output()
            .expect("plan branch commit");
    }
    std::process::Command::new("git")
        .args(["checkout", "main"])
        .current_dir(path)
        .output()
        .expect("checkout main");
    run_git(
        path,
        &["remote", "add", "origin", "git@github.com:owner/repo.git"],
    );

    dir
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

fn setup_origin_with_remote_plan_branch_ahead(
    repo_path: &std::path::Path,
    branch_name: &str,
) -> (tempfile::TempDir, String, String) {
    let remote = tempfile::tempdir().expect("create bare origin dir");
    let output = std::process::Command::new("git")
        .args(["init", "--bare"])
        .current_dir(remote.path())
        .output()
        .expect("git init bare origin");
    assert!(
        output.status.success(),
        "git init bare failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let remote_path = remote.path().to_string_lossy().into_owned();
    run_git(repo_path, &["remote", "set-url", "origin", &remote_path]);
    run_git(repo_path, &["push", "origin", "main", branch_name]);

    let original_local_sha = run_git(repo_path, &["rev-parse", branch_name]);
    run_git(repo_path, &["checkout", branch_name]);
    std::fs::write(
        repo_path.join("remote-only.txt"),
        "remote plan branch commit\n",
    )
    .expect("write remote-only file");
    run_git(repo_path, &["add", "remote-only.txt"]);
    run_git(repo_path, &["commit", "-m", "remote plan branch update"]);
    run_git(repo_path, &["push", "origin", branch_name]);
    let remote_sha = run_git(repo_path, &["rev-parse", branch_name]);
    run_git(repo_path, &["reset", "--hard", &original_local_sha]);
    run_git(repo_path, &["checkout", "main"]);

    (remote, original_local_sha, remote_sha)
}

fn setup_origin_with_conflicting_remote_plan_branch_ahead(
    repo_path: &std::path::Path,
    branch_name: &str,
) -> (tempfile::TempDir, String, String) {
    let remote = tempfile::tempdir().expect("create bare origin dir");
    let output = std::process::Command::new("git")
        .args(["init", "--bare"])
        .current_dir(remote.path())
        .output()
        .expect("git init bare origin");
    assert!(
        output.status.success(),
        "git init bare failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let remote_path = remote.path().to_string_lossy().into_owned();
    run_git(repo_path, &["remote", "set-url", "origin", &remote_path]);
    run_git(repo_path, &["push", "origin", "main", branch_name]);

    let original_local_sha = run_git(repo_path, &["rev-parse", branch_name]);
    run_git(repo_path, &["checkout", branch_name]);
    std::fs::write(repo_path.join("plan.txt"), "remote PR branch version\n")
        .expect("write remote conflicting file");
    run_git(repo_path, &["add", "plan.txt"]);
    run_git(
        repo_path,
        &["commit", "-m", "remote conflicting plan update"],
    );
    run_git(repo_path, &["push", "origin", branch_name]);
    let remote_sha = run_git(repo_path, &["rev-parse", branch_name]);

    run_git(repo_path, &["reset", "--hard", &original_local_sha]);
    std::fs::write(repo_path.join("plan.txt"), "local task merge version\n")
        .expect("write local conflicting file");
    run_git(repo_path, &["add", "plan.txt"]);
    run_git(repo_path, &["commit", "-m", "local task merge update"]);
    let local_sha = run_git(repo_path, &["rev-parse", branch_name]);
    run_git(repo_path, &["checkout", "main"]);

    (remote, local_sha, remote_sha)
}

async fn create_pending_merge_task(task_repo: &MemoryTaskRepository, task_id_str: &str) -> TaskId {
    let mut task = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        "PR merge task".to_string(),
    );
    task.id = TaskId::from_string(task_id_str.to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.category = TaskCategory::PlanMerge;
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();
    task_id
}

#[derive(Default)]
struct StaticPlanPrDescriptionDrafter {
    calls: std::sync::Mutex<Vec<(String, String)>>,
}

#[async_trait]
impl PlanPrDescriptionDrafter for StaticPlanPrDescriptionDrafter {
    async fn draft_plan_description(
        &self,
        _project: &Project,
        plan_branch: &PlanBranch,
        review_base: &str,
        _review_state: PrReviewState,
    ) -> crate::error::AppResult<crate::domain::entities::AgentWorkspacePrDescription> {
        self.calls
            .lock()
            .expect("drafter calls lock should not be poisoned")
            .push((plan_branch.branch_name.clone(), review_base.to_string()));
        Ok(crate::domain::entities::AgentWorkspacePrDescription::new(
            None,
            "## Summary\n\nDrafted by plan PR describer".to_string(),
        ))
    }
}

struct FailingPlanPrDescriptionDrafter;

#[async_trait]
impl PlanPrDescriptionDrafter for FailingPlanPrDescriptionDrafter {
    async fn draft_plan_description(
        &self,
        _project: &Project,
        _plan_branch: &PlanBranch,
        _review_base: &str,
        _review_state: PrReviewState,
    ) -> crate::error::AppResult<crate::domain::entities::AgentWorkspacePrDescription> {
        Err(AppError::Infrastructure("draft failed".to_string()))
    }
}

#[derive(Default)]
struct RecordingNotifier {
    notifications: std::sync::Mutex<Vec<(NotificationContext, TaskNotification)>>,
}

impl RecordingNotifier {
    fn notifications(&self) -> Vec<(NotificationContext, TaskNotification)> {
        self.notifications
            .lock()
            .expect("recording notifier lock should not be poisoned")
            .clone()
    }
}

#[async_trait]
impl Notifier for RecordingNotifier {
    async fn notify(&self, context: NotificationContext, notification: TaskNotification) {
        self.notifications
            .lock()
            .expect("recording notifier lock should not be poisoned")
            .push((context, notification));
    }
}

#[test]
fn pr_branch_publication_failure_classifier_matches_non_fast_forward_pushes() {
    let cases = [
        "! [rejected] plan/test -> plan/test (non-fast-forward)",
        "failed to push some refs to 'github.com:owner/repo.git'",
        "updates were rejected because the tip of your current branch is behind",
        "hint: Updates were rejected because the remote contains work that you do not have locally",
        "fetch first",
    ];

    for case in cases {
        assert!(
            super::super::merge_helpers::is_non_fast_forward_pr_branch_publication_failure(case),
            "case should be classified as non-fast-forward: {case}"
        );
    }

    assert!(
        !super::super::merge_helpers::is_non_fast_forward_pr_branch_publication_failure(
            "remote rejected freshness branch"
        ),
        "generic push failures should stay fail-closed"
    );
}

#[test]
fn pr_branch_publication_conflict_helpers_format_and_classify_metadata() {
    let conflict = super::super::merge_helpers::PrBranchPublicationConflict {
        branch_name: "plan/feature".to_string(),
        remote_ref: "origin/plan/feature".to_string(),
        conflict_files: vec![
            std::path::PathBuf::from("src/lib.rs"),
            std::path::PathBuf::from("Cargo.toml"),
        ],
        pr_number: Some(42),
    };

    assert_eq!(
        conflict.conflict_files_as_strings(),
        vec!["src/lib.rs".to_string(), "Cargo.toml".to_string()]
    );
    assert!(conflict
        .description()
        .contains("origin/plan/feature into plan/feature: src/lib.rs, Cargo.toml"));

    let conflict_error = conflict.conflict_error();
    assert!(conflict_error
        .to_string()
        .contains("pr_branch_publication_conflict"));
    assert!(
        !super::super::merge_helpers::is_pr_branch_publication_conflict_routed_error(
            &conflict_error
        ),
        "plain conflict errors are not the routed sentinel"
    );

    let routed_error = conflict.routed_error();
    assert!(
        super::super::merge_helpers::is_pr_branch_publication_conflict_routed_error(&routed_error)
    );
    assert!(
        super::super::merge_helpers::is_pr_branch_publication_conflict_routed_error(
            &AppError::GitOperation("pr_branch_publication_conflict_routed".to_string())
        )
    );
    assert!(
        !super::super::merge_helpers::is_pr_branch_publication_conflict_routed_error(
            &AppError::Validation("plain validation failure".to_string())
        )
    );

    let unknown_files = super::super::merge_helpers::PrBranchPublicationConflict {
        branch_name: "plan/feature".to_string(),
        remote_ref: "origin/plan/feature".to_string(),
        conflict_files: Vec::new(),
        pr_number: None,
    };
    assert!(
        unknown_files.description().contains("unknown files"),
        "empty conflict lists should still produce actionable text"
    );

    let mut failed_task = make_task(None, None);
    failed_task.metadata = Some(
        serde_json::json!({
            "error_code": "pr_branch_publication_failed",
        })
        .to_string(),
    );
    assert!(super::super::merge_helpers::task_has_pr_branch_publication_failure(&failed_task));
    assert!(!super::super::merge_helpers::task_has_pr_branch_publication_conflict(&failed_task));

    let mut conflict_task = make_task(None, None);
    conflict_task.metadata = Some(
        serde_json::json!({
            "error_code": "pr_branch_publication_conflict",
        })
        .to_string(),
    );
    assert!(super::super::merge_helpers::task_has_pr_branch_publication_conflict(&conflict_task));

    let mut conflict_flag_task = make_task(None, None);
    conflict_flag_task.metadata = Some(
        serde_json::json!({
            "pr_branch_publication_conflict": true,
        })
        .to_string(),
    );
    assert!(
        super::super::merge_helpers::task_has_pr_branch_publication_conflict(&conflict_flag_task)
    );
}

#[tokio::test]
async fn sync_plan_branch_pr_if_needed_pushes_pending_existing_pr_and_marks_pushed() {
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    setup_project(&project_repo).await;
    let project = project_repo
        .get_by_id(&ProjectId::from_string("proj-1".to_string()))
        .await
        .unwrap()
        .unwrap();

    let mut plan_branch = make_plan_branch(
        "artifact-1",
        "plan/sync-existing-pr",
        PlanBranchStatus::Active,
        None,
    );
    plan_branch.pr_eligible = true;
    plan_branch.pr_number = Some(42);
    plan_branch.pr_push_status = PrPushStatus::Pending;
    let branch_id = plan_branch.id.clone();
    plan_branch_repo.create(plan_branch.clone()).await.unwrap();

    let github = Arc::new(MockGithubService::new());
    let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
    let plan_branch_repo_trait: Arc<dyn PlanBranchRepository> = plan_branch_repo.clone();
    super::super::merge_helpers::sync_plan_branch_pr_if_needed(
        &project,
        &plan_branch,
        &github_trait,
        &plan_branch_repo_trait,
    )
    .await
    .expect("pending existing PR branch should be pushed");

    assert_eq!(github.state().push_branch_calls, 1);
    let updated_plan_branch = plan_branch_repo
        .get_by_id(&branch_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated_plan_branch.pr_push_status, PrPushStatus::Pushed);
}

#[tokio::test]
async fn sync_plan_branch_pr_if_needed_marks_failed_for_generic_push_error() {
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    setup_project(&project_repo).await;
    let project = project_repo
        .get_by_id(&ProjectId::from_string("proj-1".to_string()))
        .await
        .unwrap()
        .unwrap();

    let mut plan_branch = make_plan_branch(
        "artifact-1",
        "plan/sync-existing-pr-push-fails",
        PlanBranchStatus::Active,
        None,
    );
    plan_branch.pr_eligible = true;
    plan_branch.pr_number = Some(43);
    plan_branch.pr_push_status = PrPushStatus::Pending;
    let branch_id = plan_branch.id.clone();
    plan_branch_repo.create(plan_branch.clone()).await.unwrap();

    let github = Arc::new(MockGithubService::new());
    github.state().push_branch_result = Some(Err(AppError::GitOperation(
        "remote rejected freshness branch".to_string(),
    )));
    let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
    let plan_branch_repo_trait: Arc<dyn PlanBranchRepository> = plan_branch_repo.clone();
    let error = super::super::merge_helpers::sync_plan_branch_pr_if_needed(
        &project,
        &plan_branch,
        &github_trait,
        &plan_branch_repo_trait,
    )
    .await
    .expect_err("generic push failure should fail closed");

    assert!(
        error
            .to_string()
            .contains("remote rejected freshness branch"),
        "unexpected error: {error}"
    );
    assert_eq!(github.state().push_branch_calls, 1);
    let updated_plan_branch = plan_branch_repo
        .get_by_id(&branch_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated_plan_branch.pr_push_status, PrPushStatus::Failed);
}

#[tokio::test]
async fn draft_plan_pr_description_for_write_uses_resolved_base() {
    let mut project = Project::new(
        "PR mode project".to_string(),
        "/tmp/pr-mode-test".to_string(),
    );
    project.id = ProjectId::from_string("proj-1".to_string());
    project.base_branch = Some("develop".to_string());

    let mut plan_branch = make_plan_branch(
        "artifact-1",
        "plan/feature-branch",
        PlanBranchStatus::Active,
        None,
    );
    plan_branch.base_branch_override = Some("release/2026-06".to_string());

    let drafter = Arc::new(StaticPlanPrDescriptionDrafter::default());
    let drafter_trait: Arc<dyn PlanPrDescriptionDrafter> = drafter.clone();
    let description = super::super::merge_helpers::draft_plan_pr_description_for_write(
        &project,
        &plan_branch,
        Some(&drafter_trait),
        PrReviewState::Ready,
    )
    .await
    .expect("description should be drafted");

    assert_eq!(
        description.body_markdown,
        "## Summary\n\nDrafted by plan PR describer"
    );
    let calls = drafter
        .calls
        .lock()
        .expect("drafter calls lock should not be poisoned")
        .clone();
    assert_eq!(
        calls,
        vec![(
            "plan/feature-branch".to_string(),
            "release/2026-06".to_string()
        )],
        "drafter should receive the branch-specific PR base"
    );
}

#[tokio::test]
async fn draft_plan_pr_description_for_write_requires_configured_drafter() {
    let mut project = Project::new(
        "PR mode project".to_string(),
        "/tmp/pr-mode-test".to_string(),
    );
    project.id = ProjectId::from_string("proj-1".to_string());
    let plan_branch = make_plan_branch(
        "artifact-1",
        "plan/feature-branch",
        PlanBranchStatus::Active,
        None,
    );

    let result = super::super::merge_helpers::draft_plan_pr_description_for_write(
        &project,
        &plan_branch,
        None,
        PrReviewState::Draft,
    )
    .await;

    match result {
        Err(AppError::Infrastructure(message)) => {
            assert_eq!(message, "plan PR describer is not configured");
        }
        Err(other) => panic!("expected infrastructure error, got {other:?}"),
        Ok(_) => panic!("missing drafter should fail closed"),
    }
}

#[tokio::test]
async fn sync_existing_plan_branch_pr_details_uses_drafted_body() {
    let project = Project::new(
        "PR mode project".to_string(),
        "/tmp/pr-mode-test".to_string(),
    );
    let mut task = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        "Refresh existing PR".to_string(),
    );
    task.id = TaskId::from_string("task-refresh-existing-pr".to_string());
    let mut plan_branch = make_plan_branch(
        "artifact-1",
        "plan/feature-branch",
        PlanBranchStatus::Active,
        None,
    );
    plan_branch.pr_number = Some(321);

    let github = Arc::new(MockGithubService::new());
    let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
    let drafter: Arc<dyn PlanPrDescriptionDrafter> =
        Arc::new(StaticPlanPrDescriptionDrafter::default());

    super::super::merge_helpers::sync_existing_plan_branch_pr_details(
        &task,
        &project,
        &plan_branch,
        &github_trait,
        Some(&drafter),
        None,
        None,
        PrReviewState::Ready,
    )
    .await
    .expect("existing PR details should sync");

    let state = github.state();
    assert_eq!(state.update_pr_details_calls, 1);
    let body = state
        .last_update_pr_details_body
        .as_deref()
        .expect("updated PR body should be captured");
    assert!(body.starts_with("## Summary\n\nDrafted by plan PR describer"));
    assert!(!body.contains("## RalphX Status"));
    assert!(!body.contains("## How To Review"));
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 1: PR-mode with existing pr_number → push_branch + mark_pr_ready
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn persisted_pr_authority_pending_merge_without_github_capability_stays_merge_incomplete() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    let branch_name = "plan/persisted-pr-authority-unavailable";
    let repo = setup_plan_git_repo(branch_name, true);
    run_git(repo.path(), &["remote", "remove", "origin"]);
    let main_before = run_git(repo.path(), &["rev-parse", "main"]);
    let mut project = Project::new(
        "persisted PR authority without GitHub capability".to_string(),
        repo.path().to_string_lossy().into_owned(),
    );
    project.id = ProjectId::from_string("proj-1".to_string());
    project.base_branch = Some("main".to_string());
    project.github_pr_enabled = false;
    project_repo.create(project).await.unwrap();

    let task_id = create_pending_merge_task(&task_repo, "task-persisted-pr-no-github").await;
    let mut plan_branch = make_pr_eligible_plan_branch(&task_id, Some(812), false);
    plan_branch.pr_eligible = false;
    plan_branch.pr_url = Some("https://github.com/owner/repo/pull/812".to_string());
    plan_branch.pr_status = Some(PrStatus::Open);
    let plan_branch_id = plan_branch.id.clone();
    plan_branch_repo.create(plan_branch).await.unwrap();

    let services = TaskServices::new_mock()
        .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
        .with_project_repo(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>)
        .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>);
    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let result = handler.on_enter(&State::PendingMerge).await;
    assert!(
        result.is_ok(),
        "PendingMerge should fail closed through the canonical merge-incomplete path: {result:?}"
    );

    let updated_task = task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("merge task should remain");
    assert_eq!(
        updated_task.internal_status,
        InternalStatus::MergeIncomplete,
        "persisted PRs without GitHub supervision must stay recoverable instead of locally merging"
    );
    let metadata: Value = serde_json::from_str(
        updated_task
            .metadata
            .as_deref()
            .expect("merge-incomplete transition should persist diagnostics"),
    )
    .expect("diagnostics should remain valid JSON");
    assert_eq!(
        metadata["error_code"],
        Value::String("github_pr_capability_unavailable".to_string())
    );
    assert_eq!(metadata["pr_number"], Value::from(812_i64));
    assert_ne!(
        updated_task.internal_status,
        InternalStatus::Merged,
        "unavailable GitHub capability must never report a local merge success"
    );

    let updated_plan_branch = plan_branch_repo
        .get_by_id(&plan_branch_id)
        .await
        .unwrap()
        .expect("plan branch should remain");
    assert_eq!(updated_plan_branch.status, PlanBranchStatus::Active);
    assert_eq!(updated_plan_branch.pr_number, Some(812));
    assert_eq!(
        updated_plan_branch.pr_url.as_deref(),
        Some("https://github.com/owner/repo/pull/812")
    );
    assert_eq!(updated_plan_branch.pr_status, Some(PrStatus::Open));

    assert_eq!(
        run_git(repo.path(), &["rev-parse", "main"]),
        main_before,
        "the unavailable-capability path must not locally merge the plan branch into the base"
    );
}

#[tokio::test]
async fn plan_merge_without_plan_branch_row_stays_merge_incomplete_without_local_merge() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    let branch_name = "plan/missing-plan-branch-record";
    let repo = setup_plan_git_repo(branch_name, true);
    let main_before = run_git(repo.path(), &["rev-parse", "main"]);
    let worktree_root = repo.path().join("worktrees");
    std::fs::create_dir_all(&worktree_root).expect("create isolated merge worktree root");

    let mut project = Project::new(
        "Plan merge without plan branch record".to_string(),
        repo.path().to_string_lossy().into_owned(),
    );
    project.id = ProjectId::from_string("proj-1".to_string());
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktree_root.to_string_lossy().into_owned());
    project_repo.create(project).await.unwrap();

    let task_id = create_pending_merge_task(&task_repo, "task-missing-plan-branch-record").await;
    let mut task = task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("merge task should exist");
    task.task_branch = Some(branch_name.to_string());
    task_repo.update(&task).await.unwrap();

    let services = with_default_test_branch_update_authority(
        TaskServices::new_mock(),
        Arc::clone(&task_repo) as Arc<dyn TaskRepository>,
    )
    .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
    .with_project_repo(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>)
    .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>);
    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    handler
        .on_enter(&State::PendingMerge)
        .await
        .expect("missing plan branch should use the canonical merge-incomplete transition");

    let updated_task = task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("merge task should remain");
    assert_eq!(
        updated_task.internal_status,
        InternalStatus::MergeIncomplete
    );
    let metadata: Value = serde_json::from_str(
        updated_task
            .metadata
            .as_deref()
            .expect("merge-incomplete transition should persist diagnostics"),
    )
    .expect("diagnostics should remain valid JSON");
    assert_eq!(
        metadata["error_code"],
        Value::String("plan_branch_missing".to_string())
    );
    assert_eq!(
        run_git(repo.path(), &["rev-parse", "main"]),
        main_before,
        "a missing plan branch record must never authorize a local plan merge"
    );
}

#[tokio::test]
async fn plan_merge_without_a_plan_branch_repository_stays_merge_incomplete() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    setup_project(&project_repo).await;
    let task_id =
        create_pending_merge_task(&task_repo, "task-plan-branch-repository-unavailable").await;

    let services = TaskServices::new_mock()
        .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
        .with_project_repo(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>);
    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    handler.on_enter(&State::PendingMerge).await.expect(
        "missing plan branch repository should use the canonical merge-incomplete transition",
    );

    let updated_task = task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("merge task should remain");
    assert_eq!(
        updated_task.internal_status,
        InternalStatus::MergeIncomplete
    );
    let metadata: Value = serde_json::from_str(
        updated_task
            .metadata
            .as_deref()
            .expect("merge-incomplete transition should persist diagnostics"),
    )
    .expect("diagnostics should remain valid JSON");
    assert_eq!(
        metadata["error_code"],
        Value::String("plan_branch_repository_unavailable".to_string())
    );
}

#[tokio::test]
async fn plan_merge_with_a_plan_branch_lookup_error_stays_merge_incomplete() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    setup_project(&project_repo).await;
    let task_id = create_pending_merge_task(&task_repo, "task-plan-branch-lookup-error").await;
    plan_branch_repo.fail_next_merge_task_lookup("planned repository outage");

    let services = TaskServices::new_mock()
        .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
        .with_project_repo(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>)
        .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>);
    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    handler
        .on_enter(&State::PendingMerge)
        .await
        .expect("plan branch lookup failures should use the canonical merge-incomplete transition");

    let updated_task = task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("merge task should remain");
    assert_eq!(
        updated_task.internal_status,
        InternalStatus::MergeIncomplete
    );
    let metadata: Value = serde_json::from_str(
        updated_task
            .metadata
            .as_deref()
            .expect("merge-incomplete transition should persist diagnostics"),
    )
    .expect("diagnostics should remain valid JSON");
    assert_eq!(
        metadata["error_code"],
        Value::String("plan_branch_lookup_failed".to_string())
    );
    assert_eq!(
        metadata["cause"],
        Value::String("Infrastructure error: planned repository outage".to_string())
    );
}

#[tokio::test]
async fn github_eligible_pre_pr_branch_without_github_service_stays_merge_incomplete() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    let branch_name = "plan/github-without-service";
    let repo = setup_plan_git_repo(branch_name, true);
    let main_before = run_git(repo.path(), &["rev-parse", "main"]);
    let mut project = Project::new(
        "GitHub pre-PR branch without GitHub service".to_string(),
        repo.path().to_string_lossy().into_owned(),
    );
    project.id = ProjectId::from_string("proj-1".to_string());
    project.base_branch = Some("main".to_string());
    project_repo.create(project).await.unwrap();

    let task_id = create_pending_merge_task(&task_repo, "task-github-without-service").await;
    let mut plan_branch = make_pr_eligible_plan_branch(&task_id, None, false);
    plan_branch.branch_name = branch_name.to_string();
    let plan_branch_id = plan_branch.id.clone();
    plan_branch_repo.create(plan_branch).await.unwrap();

    let services = TaskServices::new_mock()
        .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
        .with_project_repo(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>)
        .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>);
    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    handler
        .on_enter(&State::PendingMerge)
        .await
        .expect("missing GitHub supervision should use the canonical merge-incomplete transition");

    let updated_task = task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("merge task should remain");
    assert_eq!(
        updated_task.internal_status,
        InternalStatus::MergeIncomplete
    );
    let metadata: Value = serde_json::from_str(
        updated_task
            .metadata
            .as_deref()
            .expect("merge-incomplete transition should persist diagnostics"),
    )
    .expect("diagnostics should remain valid JSON");
    assert_eq!(
        metadata["error_code"],
        Value::String("github_pr_capability_unavailable".to_string())
    );
    assert_eq!(
        metadata["branch_name"],
        Value::String(branch_name.to_string())
    );

    let updated_plan_branch = plan_branch_repo
        .get_by_id(&plan_branch_id)
        .await
        .unwrap()
        .expect("plan branch should remain");
    assert_eq!(updated_plan_branch.status, PlanBranchStatus::Active);
    assert_eq!(
        run_git(repo.path(), &["rev-parse", "main"]),
        main_before,
        "a GitHub-capable branch without GitHub supervision must not enter the local merge path"
    );
}

#[tokio::test]
async fn reviewable_diff_read_failure_stays_merge_incomplete_without_github_or_local_merge() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    let repo = setup_plan_git_repo("plan/reviewable-diff-fixture", true);
    let main_before = run_git(repo.path(), &["rev-parse", "main"]);
    let mut project = Project::new(
        "Reviewable diff failure must fail closed".to_string(),
        repo.path().to_string_lossy().into_owned(),
    );
    project.id = ProjectId::from_string("proj-1".to_string());
    project.base_branch = Some("missing-review-base".to_string());
    project_repo.create(project).await.unwrap();

    let task_id = create_pending_merge_task(&task_repo, "task-reviewable-diff-read-failure").await;
    let mut plan_branch = make_pr_eligible_plan_branch(&task_id, None, false);
    plan_branch.branch_name = "plan/reviewable-diff-fixture".to_string();
    let plan_branch_id = plan_branch.id.clone();
    plan_branch_repo.create(plan_branch).await.unwrap();

    let mock_github = Arc::new(MockGithubService::new());
    let services = TaskServices::new_mock()
        .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
        .with_project_repo(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>)
        .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>)
        .with_github_service(Arc::clone(&mock_github) as Arc<dyn GithubServiceTrait>);
    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    handler.on_enter(&State::PendingMerge).await.expect(
        "reviewable-diff read failures should use the canonical merge-incomplete transition",
    );

    {
        let github_state = mock_github.state();
        assert_eq!(github_state.push_branch_calls, 0);
        assert_eq!(github_state.create_draft_pr_calls, 0);
        assert_eq!(github_state.mark_pr_ready_calls, 0);
    }

    let updated_task = task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("merge task should remain");
    assert_eq!(
        updated_task.internal_status,
        InternalStatus::MergeIncomplete
    );
    let metadata: Value = serde_json::from_str(
        updated_task
            .metadata
            .as_deref()
            .expect("merge-incomplete transition should persist diagnostics"),
    )
    .expect("diagnostics should remain valid JSON");
    assert_eq!(
        metadata["error_code"],
        Value::String("plan_branch_reviewable_diff_check_failed".to_string())
    );

    let updated_plan_branch = plan_branch_repo
        .get_by_id(&plan_branch_id)
        .await
        .unwrap()
        .expect("plan branch should remain");
    assert_eq!(updated_plan_branch.status, PlanBranchStatus::Active);
    assert_eq!(
        run_git(repo.path(), &["rev-parse", "main"]),
        main_before,
        "a reviewable-diff read failure must not authorize local merge"
    );
}

#[tokio::test]
async fn stale_pre_pr_eligibility_without_origin_uses_local_merge_without_github_calls() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    let branch_name = "plan/stale-pre-pr-local";
    let repo = setup_plan_git_repo(branch_name, true);
    run_git(repo.path(), &["remote", "remove", "origin"]);
    let worktree_root = repo.path().join("worktrees");
    std::fs::create_dir_all(&worktree_root).expect("create isolated merge worktree root");

    let mut project = Project::new(
        "stale pre-PR local capability".to_string(),
        repo.path().to_string_lossy().into_owned(),
    );
    project.id = ProjectId::from_string("proj-1".to_string());
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktree_root.to_string_lossy().into_owned());
    project_repo.create(project).await.unwrap();

    let task_id = create_pending_merge_task(&task_repo, "task-stale-pre-pr-local").await;
    let mut plan_branch = make_pr_eligible_plan_branch(&task_id, None, false);
    plan_branch.branch_name = branch_name.to_string();
    let plan_branch_id = plan_branch.id.clone();
    plan_branch_repo.create(plan_branch).await.unwrap();

    let mock_github = Arc::new(MockGithubService::new());
    let services = with_default_test_branch_update_authority(
        TaskServices::new_mock(),
        Arc::clone(&task_repo) as Arc<dyn TaskRepository>,
    )
    .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
    .with_project_repo(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>)
    .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>)
    .with_github_service(Arc::clone(&mock_github) as Arc<dyn GithubServiceTrait>);
    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    handler
        .on_enter(&State::PendingMerge)
        .await
        .expect("local-only PendingMerge entry should complete the canonical local merge");

    {
        let github_state = mock_github.state();
        assert_eq!(github_state.push_branch_calls, 0);
        assert_eq!(github_state.create_draft_pr_calls, 0);
        assert_eq!(github_state.mark_pr_ready_calls, 0);
    }

    let updated_task = task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("merge task should remain");
    assert_eq!(updated_task.internal_status, InternalStatus::Merged);
    assert_eq!(
        run_git(repo.path(), &["show", "main:plan.txt"]),
        "plan branch work",
        "the stale eligible branch must take the local merge pipeline"
    );

    let updated_plan_branch = plan_branch_repo
        .get_by_id(&plan_branch_id)
        .await
        .unwrap()
        .expect("plan branch should remain");
    assert!(!updated_plan_branch.pr_eligible);
    assert_eq!(updated_plan_branch.pr_number, None);
}

#[tokio::test]
async fn stale_pre_pr_eligibility_update_failure_stays_merge_incomplete() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    let branch_name = "plan/stale-pre-pr-update-failure";
    let repo = setup_plan_git_repo(branch_name, true);
    run_git(repo.path(), &["remote", "remove", "origin"]);
    let main_before = run_git(repo.path(), &["rev-parse", "main"]);
    let mut project = Project::new(
        "stale pre-PR eligibility update failure".to_string(),
        repo.path().to_string_lossy().into_owned(),
    );
    project.id = ProjectId::from_string("proj-1".to_string());
    project.base_branch = Some("main".to_string());
    project_repo.create(project).await.unwrap();

    let task_id = create_pending_merge_task(&task_repo, "task-stale-pre-pr-update-failure").await;
    let mut plan_branch = make_pr_eligible_plan_branch(&task_id, None, false);
    plan_branch.branch_name = branch_name.to_string();
    let plan_branch_id = plan_branch.id.clone();
    plan_branch_repo.create(plan_branch).await.unwrap();
    plan_branch_repo.fail_next_pr_eligibility_update("planned persistence outage");

    let services = TaskServices::new_mock()
        .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
        .with_project_repo(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>)
        .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>);
    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    handler
        .on_enter(&State::PendingMerge)
        .await
        .expect("stale eligibility persistence failures should use the canonical merge-incomplete transition");

    let updated_task = task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("merge task should remain");
    assert_eq!(
        updated_task.internal_status,
        InternalStatus::MergeIncomplete
    );
    let metadata: Value = serde_json::from_str(
        updated_task
            .metadata
            .as_deref()
            .expect("merge-incomplete transition should persist diagnostics"),
    )
    .expect("diagnostics should remain valid JSON");
    assert_eq!(
        metadata["error_code"],
        Value::String("plan_branch_pr_eligibility_update_failed".to_string())
    );
    let updated_plan_branch = plan_branch_repo
        .get_by_id(&plan_branch_id)
        .await
        .unwrap()
        .expect("plan branch should remain");
    assert!(
        updated_plan_branch.pr_eligible,
        "failed persistence must not claim the branch was routed to local merge"
    );
    assert_eq!(
        run_git(repo.path(), &["rev-parse", "main"]),
        main_before,
        "failed eligibility persistence must never start a local merge"
    );
}

#[tokio::test]
async fn pre_pr_origin_inspection_failure_blocks_without_local_merge_or_github_calls() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    let branch_name = "plan/pre-pr-inspection-failure";
    let repo = setup_plan_git_repo(branch_name, true);
    let main_ref_path = repo.path().join(".git/refs/heads/main");
    let main_before = std::fs::read_to_string(&main_ref_path).expect("read main ref");
    std::fs::remove_file(repo.path().join(".git/config")).expect("remove Git config file");
    std::fs::create_dir(repo.path().join(".git/config")).expect("corrupt Git config path");

    let mut project = Project::new(
        "pre-PR origin inspection failure".to_string(),
        repo.path().to_string_lossy().into_owned(),
    );
    project.id = ProjectId::from_string("proj-1".to_string());
    project.base_branch = Some("main".to_string());
    project.github_pr_enabled = true;
    project_repo.create(project).await.unwrap();

    let task_id = create_pending_merge_task(&task_repo, "task-pre-pr-inspection-failure").await;
    let plan_branch = make_pr_eligible_plan_branch(&task_id, None, false);
    plan_branch_repo.create(plan_branch).await.unwrap();

    let mock_github = Arc::new(MockGithubService::new());
    let services = TaskServices::new_mock()
        .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
        .with_project_repo(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>)
        .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>)
        .with_github_service(Arc::clone(&mock_github) as Arc<dyn GithubServiceTrait>);
    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    handler
        .on_enter(&State::PendingMerge)
        .await
        .expect("origin inspection failure should use the canonical merge-incomplete transition");

    {
        let github_state = mock_github.state();
        assert_eq!(github_state.push_branch_calls, 0);
        assert_eq!(github_state.create_draft_pr_calls, 0);
        assert_eq!(github_state.mark_pr_ready_calls, 0);
    }

    let updated_task = task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("merge task should remain");
    assert_eq!(
        updated_task.internal_status,
        InternalStatus::MergeIncomplete
    );
    let metadata: Value = serde_json::from_str(
        updated_task
            .metadata
            .as_deref()
            .expect("merge-incomplete transition should persist diagnostics"),
    )
    .expect("diagnostics should remain valid JSON");
    assert_eq!(
        metadata["error_code"],
        Value::String("repository_capability_inspection_failed".to_string())
    );
    assert_eq!(
        std::fs::read_to_string(&main_ref_path).expect("read main ref after failure"),
        main_before,
        "origin inspection failure must never locally merge the plan branch"
    );
}

/// PR-mode: plan_branch has pr_number=42.
/// Expected: push_branch(42) + mark_pr_ready(42) called, create_draft_pr NOT called.
#[tokio::test]
async fn test_pr_mode_with_existing_pr_number_calls_push_and_mark_ready() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    setup_project(&project_repo).await;
    let task_id = create_pending_merge_task(&task_repo, "task-pr-existing").await;

    let pb = make_pr_eligible_plan_branch(&task_id, Some(42), false);
    plan_branch_repo.create(pb).await.unwrap();

    let mock_github = Arc::new(MockGithubService::new());

    let services = TaskServices::new_mock()
        .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
        .with_project_repo(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>)
        .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>)
        .with_github_service(Arc::clone(&mock_github) as Arc<dyn GithubServiceTrait>)
        .with_plan_pr_description_drafter(Arc::new(StaticPlanPrDescriptionDrafter::default()));

    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let result = handler.on_enter(&State::PendingMerge).await;
    assert!(
        result.is_ok(),
        "on_enter(PendingMerge) should succeed: {:?}",
        result
    );

    {
        let state = mock_github.state();
        assert_eq!(
            state.push_branch_calls, 1,
            "push_branch should be called once"
        );
        assert_eq!(
            state.mark_pr_ready_calls, 1,
            "mark_pr_ready should be called once"
        );
        assert_eq!(
            state.update_pr_details_calls, 1,
            "PR details should be refreshed before marking ready"
        );
        assert_eq!(
            state.create_draft_pr_calls, 0,
            "create_draft_pr should NOT be called when pr_number already exists"
        );
    }
    let updated_task = task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("task should exist");
    assert_eq!(
        updated_task.internal_status,
        InternalStatus::WaitingOnPr,
        "PR-backed final merge should wait on the GitHub PR instead of entering local Merging"
    );
}

#[tokio::test]
async fn pr_mode_pending_merge_reenables_auto_merge_after_review_correction() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    setup_project(&project_repo).await;
    let task_id = create_pending_merge_task(&task_repo, "task-pr-auto-merge-restore").await;
    let mut task = task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("task should exist");
    task.metadata = Some(
        serde_json::json!({
            "github_auto_merge_disabled_for_correction": true,
            "github_auto_merge_pr_number": 42,
            "github_auto_merge_method": "rebase",
            "github_auto_merge_disabled_at": "2026-07-10T12:00:00Z",
            "github_auto_merge_disabled_source": "github_review_feedback",
        })
        .to_string(),
    );
    task_repo.update(&task).await.unwrap();

    let pb = make_pr_eligible_plan_branch(&task_id, Some(42), false);
    plan_branch_repo.create(pb).await.unwrap();

    let mock_github = Arc::new(MockGithubService::new());
    let services = TaskServices::new_mock()
        .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
        .with_project_repo(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>)
        .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>)
        .with_github_service(Arc::clone(&mock_github) as Arc<dyn GithubServiceTrait>)
        .with_plan_pr_description_drafter(Arc::new(StaticPlanPrDescriptionDrafter::default()));

    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let result = handler.on_enter(&State::PendingMerge).await;
    assert!(
        result.is_ok(),
        "on_enter(PendingMerge) should succeed: {:?}",
        result
    );

    {
        let state = mock_github.state();
        assert_eq!(state.push_branch_calls, 1);
        assert_eq!(state.mark_pr_ready_calls, 1);
        assert_eq!(state.enable_pr_auto_merge_calls, 1);
        assert_eq!(
            state.last_enable_pr_auto_merge_args.as_ref(),
            Some(&(42, "rebase".to_string()))
        );
    }

    let updated_task = task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("task should exist");
    assert_eq!(updated_task.internal_status, InternalStatus::WaitingOnPr);
    let metadata: Value = serde_json::from_str(
        updated_task
            .metadata
            .as_deref()
            .expect("metadata should exist"),
    )
    .expect("metadata should be valid JSON");
    assert!(
        metadata
            .get("github_auto_merge_disabled_for_correction")
            .is_none(),
        "active disabled marker should be consumed after successful restore"
    );
    assert!(metadata.get("github_auto_merge_method").is_none());
    assert_eq!(
        metadata["github_auto_merge_reenabled_source"],
        Value::String("pr_mode_pending_merge".to_string())
    );
    assert_eq!(
        metadata["github_auto_merge_reenabled_method"],
        Value::String("rebase".to_string())
    );
    assert!(
        metadata["github_auto_merge_reenabled_at"].is_string(),
        "successful restore should record an audit timestamp"
    );
}

#[tokio::test]
async fn pr_mode_pending_merge_keeps_auto_merge_marker_when_reenable_fails() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    setup_project(&project_repo).await;
    let task_id = create_pending_merge_task(&task_repo, "task-pr-auto-merge-restore-fails").await;
    let mut task = task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("task should exist");
    task.metadata = Some(
        serde_json::json!({
            "github_auto_merge_disabled_for_correction": true,
            "github_auto_merge_pr_number": 42,
            "github_auto_merge_method": "squash",
            "github_auto_merge_disabled_at": "2026-07-10T12:00:00Z",
            "github_auto_merge_disabled_source": "github_review_feedback",
        })
        .to_string(),
    );
    task_repo.update(&task).await.unwrap();

    let pb = make_pr_eligible_plan_branch(&task_id, Some(42), false);
    let plan_branch_id = pb.id.clone();
    plan_branch_repo.create(pb).await.unwrap();

    let mock_github = Arc::new(MockGithubService::new());
    mock_github.state().enable_pr_auto_merge_result = Some(Err(AppError::Infrastructure(
        "auto-merge enable denied".to_string(),
    )));
    let services = TaskServices::new_mock()
        .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
        .with_project_repo(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>)
        .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>)
        .with_github_service(Arc::clone(&mock_github) as Arc<dyn GithubServiceTrait>)
        .with_plan_pr_description_drafter(Arc::new(StaticPlanPrDescriptionDrafter::default()));

    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let result = handler.on_enter(&State::PendingMerge).await;
    assert!(
        result.is_ok(),
        "on_enter(PendingMerge) should surface restore failure as merge-incomplete state: {:?}",
        result
    );

    {
        let state = mock_github.state();
        assert_eq!(state.push_branch_calls, 1);
        assert_eq!(state.mark_pr_ready_calls, 1);
        assert_eq!(state.enable_pr_auto_merge_calls, 1);
    }

    let updated_task = task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("task should exist");
    assert_eq!(
        updated_task.internal_status,
        InternalStatus::MergeIncomplete
    );
    let metadata: Value = serde_json::from_str(
        updated_task
            .metadata
            .as_deref()
            .expect("metadata should exist"),
    )
    .expect("metadata should be valid JSON");
    assert_eq!(
        metadata["github_auto_merge_disabled_for_correction"],
        Value::Bool(true),
        "failed restore must keep the active marker for a later retry"
    );
    assert_eq!(
        metadata["github_auto_merge_reenable_error"],
        Value::String("Infrastructure error: auto-merge enable denied".to_string())
    );
    assert!(metadata["github_auto_merge_reenable_failed_at"].is_string());

    let updated_plan_branch = plan_branch_repo
        .get_by_id(&plan_branch_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated_plan_branch.pr_push_status, PrPushStatus::Failed);
}

#[tokio::test]
async fn test_pr_mode_with_existing_pr_number_push_failure_stays_merge_incomplete() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    setup_project(&project_repo).await;
    let task_id = create_pending_merge_task(&task_repo, "task-pr-existing-push-fails").await;

    let pb = make_pr_eligible_plan_branch(&task_id, Some(42), false);
    let plan_branch_id = pb.id.clone();
    plan_branch_repo.create(pb).await.unwrap();

    let mock_github = Arc::new(MockGithubService::new());
    mock_github.state().push_branch_result = Some(Err(AppError::GitOperation(
        "fatal: could not read Username for 'https://github.com': terminal prompts disabled"
            .to_string(),
    )));

    let services = TaskServices::new_mock()
        .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
        .with_project_repo(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>)
        .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>)
        .with_github_service(Arc::clone(&mock_github) as Arc<dyn GithubServiceTrait>)
        .with_plan_pr_description_drafter(Arc::new(StaticPlanPrDescriptionDrafter::default()));

    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let result = handler.on_enter(&State::PendingMerge).await;
    assert!(
        result.is_ok(),
        "on_enter(PendingMerge) should handle PR push failure visibly: {:?}",
        result
    );

    {
        let state = mock_github.state();
        assert_eq!(state.push_branch_calls, 1);
        assert_eq!(
            state.mark_pr_ready_calls, 0,
            "PRs must not be marked ready when the branch push failed"
        );
    }

    let updated_task = task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("task should exist");
    assert_eq!(
        updated_task.internal_status,
        InternalStatus::MergeIncomplete,
        "failed PR branch publication must not advance to WaitingOnPr"
    );

    let updated_plan_branch = plan_branch_repo
        .get_by_id(&plan_branch_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated_plan_branch.pr_push_status,
        PrPushStatus::Failed,
        "failed push should be durable on the plan branch"
    );

    let recovery = MergeRecoveryMetadata::from_task_metadata(updated_task.metadata.as_deref())
        .expect("task metadata should parse")
        .expect("merge recovery metadata should be persisted");
    let attempt_failed = recovery
        .events
        .iter()
        .rev()
        .find(|event| matches!(event.kind, MergeRecoveryEventKind::AttemptFailed))
        .expect("PR push failure should record an AttemptFailed recovery event");
    assert_eq!(
        attempt_failed.failure_source,
        Some(MergeFailureSource::AuthFailure),
        "auth-shaped PR push failures must preserve the classified source"
    );
    assert!(
        attempt_failed.message.contains("PR operation failed"),
        "structured event should keep the PR operation context"
    );
}

#[tokio::test]
async fn test_pr_mode_marks_concurrently_created_pr_ready_after_guard_clears() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    let repo = setup_plan_git_repo("plan/feature-branch", true);
    setup_project_with_path(&project_repo, repo.path().to_string_lossy().into_owned()).await;
    let task_id = create_pending_merge_task(&task_repo, "task-pr-concurrent-ready").await;

    let pb = make_pr_eligible_plan_branch(&task_id, None, false);
    let plan_branch_id = pb.id.clone();
    plan_branch_repo.create(pb).await.unwrap();

    let registry = Arc::new(PrPollerRegistry::new(
        None,
        Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>,
    ));
    registry
        .pr_creation_guard
        .insert(plan_branch_id.clone(), ());

    let repo_for_concurrent = Arc::clone(&plan_branch_repo);
    let guard_for_concurrent = Arc::clone(&registry.pr_creation_guard);
    let branch_id_for_concurrent = plan_branch_id.clone();
    tokio::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        repo_for_concurrent
            .update_pr_info(
                &branch_id_for_concurrent,
                314,
                "https://github.com/owner/repo/pull/314".to_string(),
                PrStatus::Open,
                true,
            )
            .await
            .expect("concurrent PR info update should succeed");
        guard_for_concurrent.remove(&branch_id_for_concurrent);
    });

    let mock_github = Arc::new(MockGithubService::new());

    let services = TaskServices::new_mock()
        .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
        .with_project_repo(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>)
        .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>)
        .with_pr_poller_registry(Arc::clone(&registry))
        .with_pr_creation_guard(Arc::clone(&registry.pr_creation_guard))
        .with_github_service(Arc::clone(&mock_github) as Arc<dyn GithubServiceTrait>)
        .with_plan_pr_description_drafter(Arc::new(StaticPlanPrDescriptionDrafter::default()));

    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let result = handler.on_enter(&State::PendingMerge).await;
    assert!(
        result.is_ok(),
        "on_enter(PendingMerge) should succeed: {:?}",
        result
    );

    {
        let state = mock_github.state();
        assert_eq!(state.push_branch_calls, 1);
        assert_eq!(state.mark_pr_ready_calls, 1);
        assert_eq!(state.update_pr_details_calls, 1);
        assert_eq!(
            state.create_draft_pr_calls, 0,
            "handler should reuse the concurrently-created PR instead of creating another"
        );
    }

    let updated_task = task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("task should exist");
    assert_eq!(updated_task.internal_status, InternalStatus::WaitingOnPr);

    let updated_plan_branch = plan_branch_repo
        .get_by_id(&plan_branch_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated_plan_branch.pr_number, Some(314));
    assert_eq!(updated_plan_branch.pr_push_status, PrPushStatus::Pushed);
}

#[tokio::test]
async fn test_pr_mode_with_existing_pr_number_uses_drafted_description_override() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    setup_project(&project_repo).await;
    let task_id = create_pending_merge_task(&task_repo, "task-pr-existing-drafted").await;

    let pb = make_pr_eligible_plan_branch(&task_id, Some(42), false);
    plan_branch_repo.create(pb).await.unwrap();

    let mock_github = Arc::new(MockGithubService::new());
    let drafter = Arc::new(StaticPlanPrDescriptionDrafter::default());
    let drafter_trait: Arc<dyn PlanPrDescriptionDrafter> = drafter.clone();

    let services = TaskServices::new_mock()
        .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
        .with_project_repo(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>)
        .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>)
        .with_github_service(Arc::clone(&mock_github) as Arc<dyn GithubServiceTrait>)
        .with_plan_pr_description_drafter(drafter_trait);

    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let result = handler.on_enter(&State::PendingMerge).await;
    assert!(
        result.is_ok(),
        "on_enter(PendingMerge) should succeed: {:?}",
        result
    );

    let calls = drafter
        .calls
        .lock()
        .expect("drafter calls lock should not be poisoned")
        .clone();
    assert_eq!(
        calls,
        vec![("plan/feature-branch".to_string(), "main".to_string())],
        "drafter should receive the plan branch and resolved PR base"
    );

    let state = mock_github.state();
    assert_eq!(
        state.update_pr_details_calls, 1,
        "PR details should be refreshed with the drafted description"
    );
    let body = state
        .last_update_pr_details_body
        .as_deref()
        .expect("updated PR body should be captured");
    assert!(body.starts_with("## Summary\n\nDrafted by plan PR describer"));
    assert!(!body.contains("## RalphX Status"));
    assert!(!body.contains("## How To Review"));
    assert!(body.contains("<summary>View full plan</summary>"));
    assert!(body.contains("_Generated by [RalphX]("));
}

#[tokio::test]
async fn test_pr_mode_missing_drafter_fails_before_pr_write() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    setup_project(&project_repo).await;
    let task_id = create_pending_merge_task(&task_repo, "task-pr-missing-drafter").await;

    let plan_branch = make_pr_eligible_plan_branch(&task_id, Some(42), false);
    let plan_branch_id = plan_branch.id.clone();
    plan_branch_repo.create(plan_branch).await.unwrap();

    let mock_github = Arc::new(MockGithubService::new());
    let services = TaskServices::new_mock()
        .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
        .with_project_repo(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>)
        .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>)
        .with_github_service(Arc::clone(&mock_github) as Arc<dyn GithubServiceTrait>);

    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let result = handler.on_enter(&State::PendingMerge).await;
    assert!(
        result.is_ok(),
        "on_enter(PendingMerge) should record merge-incomplete state: {:?}",
        result
    );

    {
        let state = mock_github.state();
        assert_eq!(state.push_branch_calls, 0);
        assert_eq!(state.update_pr_details_calls, 0);
        assert_eq!(state.mark_pr_ready_calls, 0);
    }

    let updated_task = task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("task should exist");
    assert_eq!(
        updated_task.internal_status,
        InternalStatus::MergeIncomplete
    );

    let updated_plan_branch = plan_branch_repo
        .get_by_id(&plan_branch_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated_plan_branch.pr_push_status, PrPushStatus::Failed);
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2: PR-mode without pr_number → creates new PR
// ─────────────────────────────────────────────────────────────────────────────

/// PR-mode: plan_branch has no pr_number yet.
/// Expected: push_branch called, create_draft_pr called (returns pr#99), mark_pr_ready called.
#[tokio::test]
async fn test_pr_mode_without_pr_number_creates_new_pr() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    let branch_name = "plan/feature-branch";
    let repo = setup_plan_git_repo(branch_name, true);
    setup_project_with_path(&project_repo, repo.path().to_string_lossy().into_owned()).await;
    let task_id = create_pending_merge_task(&task_repo, "task-pr-new").await;

    // No pr_number — should trigger PR creation path
    let pb = make_pr_eligible_plan_branch(&task_id, None, false);
    plan_branch_repo.create(pb).await.unwrap();

    let mock_github = Arc::new(MockGithubService::new());
    mock_github.will_create_pr(99, "https://github.com/owner/repo/pull/99");

    let services = TaskServices::new_mock()
        .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
        .with_project_repo(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>)
        .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>)
        .with_github_service(Arc::clone(&mock_github) as Arc<dyn GithubServiceTrait>)
        .with_plan_pr_description_drafter(Arc::new(StaticPlanPrDescriptionDrafter::default()));

    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let result = handler.on_enter(&State::PendingMerge).await;
    assert!(
        result.is_ok(),
        "on_enter(PendingMerge) should succeed: {:?}",
        result
    );

    {
        let state = mock_github.state();
        assert_eq!(
            state.push_branch_calls, 1,
            "push_branch should be called once"
        );
        assert_eq!(
            state.create_draft_pr_calls, 1,
            "create_draft_pr should be called when pr_number is absent"
        );
        assert_eq!(
            state.mark_pr_ready_calls, 1,
            "mark_pr_ready should be called after creation"
        );
        assert_eq!(
            state.update_pr_details_calls, 1,
            "newly-created final PR should be refreshed with ready-state title/body before marking ready"
        );
    } // drop MutexGuard before await

    // Verify pr_info was stored in plan branch repo
    let updated_pb = plan_branch_repo
        .get_by_merge_task_id(&task_id)
        .await
        .unwrap()
        .expect("plan branch should still exist");
    assert_eq!(
        updated_pb.pr_number,
        Some(99),
        "pr_number should be persisted after creation"
    );
    let updated_task = task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("task should exist");
    assert_eq!(
        updated_task.internal_status,
        InternalStatus::WaitingOnPr,
        "newly-created final PR should put the merge task into WaitingOnPr"
    );
}

#[tokio::test]
async fn test_pr_mode_without_pr_number_skips_empty_plan_branch() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    let branch_name = "plan/feature-branch";
    let repo = setup_plan_git_repo(branch_name, false);
    setup_project_with_path(&project_repo, repo.path().to_string_lossy().into_owned()).await;
    let task_id = create_pending_merge_task(&task_repo, "task-pr-empty").await;

    let pb = make_pr_eligible_plan_branch(&task_id, None, false);
    plan_branch_repo.create(pb).await.unwrap();

    let mock_github = Arc::new(MockGithubService::new());

    let services = TaskServices::new_mock()
        .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
        .with_project_repo(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>)
        .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>)
        .with_github_service(Arc::clone(&mock_github) as Arc<dyn GithubServiceTrait>)
        .with_plan_pr_description_drafter(Arc::new(StaticPlanPrDescriptionDrafter::default()));

    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let result = handler.on_enter(&State::PendingMerge).await;
    assert!(
        result.is_ok(),
        "on_enter(PendingMerge) should succeed: {:?}",
        result
    );

    let state = mock_github.state();
    assert_eq!(
        state.push_branch_calls, 0,
        "empty plan branch should not be pushed to GitHub"
    );
    assert_eq!(
        state.create_draft_pr_calls, 0,
        "empty plan branch should not create a PR in PendingMerge"
    );
    assert_eq!(
        state.mark_pr_ready_calls, 0,
        "empty plan branch should not enter the PR-ready flow"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 3: pr_eligible=false → skips PR path, no github calls
// ─────────────────────────────────────────────────────────────────────────────

/// When pr_eligible=false, the PR fork is not taken.
/// The push-to-main path runs instead (fails fast on nonexistent dir).
/// No GitHub service calls should be made.
#[tokio::test]
async fn test_pr_eligible_false_skips_pr_path() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    setup_project(&project_repo).await;
    let task_id = create_pending_merge_task(&task_repo, "task-push-to-main").await;

    // pr_eligible = false → should NOT trigger PR path
    let mut pb = make_pr_eligible_plan_branch(&task_id, None, false);
    pb.pr_eligible = false;
    plan_branch_repo.create(pb).await.unwrap();

    let mock_github = Arc::new(MockGithubService::new());

    let services = TaskServices::new_mock()
        .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
        .with_project_repo(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>)
        .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>)
        .with_github_service(Arc::clone(&mock_github) as Arc<dyn GithubServiceTrait>)
        .with_plan_pr_description_drafter(Arc::new(StaticPlanPrDescriptionDrafter::default()));

    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let result = handler.on_enter(&State::PendingMerge).await;
    assert!(
        result.is_ok(),
        "on_enter(PendingMerge) should succeed: {:?}",
        result
    );

    let state = mock_github.state();
    assert_eq!(
        state.push_branch_calls, 0,
        "push_branch should NOT be called when pr_eligible=false"
    );
    assert_eq!(
        state.mark_pr_ready_calls, 0,
        "mark_pr_ready should NOT be called when pr_eligible=false"
    );
    assert_eq!(
        state.create_draft_pr_calls, 0,
        "create_draft_pr should NOT be called when pr_eligible=false"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 4: Re-entry guard — pr_polling_active=true, no registry → proceeds
// ─────────────────────────────────────────────────────────────────────────────

/// pr_polling_active=true but no PrPollerRegistry wired.
/// The re-entry guard only triggers when BOTH flags are set AND registry.is_polling().
/// Without a registry, guard is skipped → PR operations proceed normally.
#[tokio::test]
async fn test_pr_mode_reentry_guard_no_registry_proceeds() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    setup_project(&project_repo).await;
    let task_id = create_pending_merge_task(&task_repo, "task-reentry").await;

    // pr_polling_active=true simulates a previous run that set the flag.
    // Without a registry, is_polling() can't be checked → guard bypassed.
    let pb = make_pr_eligible_plan_branch(&task_id, Some(77), true);
    plan_branch_repo.create(pb).await.unwrap();

    let mock_github = Arc::new(MockGithubService::new());

    let services = TaskServices::new_mock()
        .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
        .with_project_repo(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>)
        .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>)
        .with_github_service(Arc::clone(&mock_github) as Arc<dyn GithubServiceTrait>)
        .with_plan_pr_description_drafter(Arc::new(StaticPlanPrDescriptionDrafter::default()));
    // NOTE: no .with_pr_poller_registry() — guard must be bypassed

    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let result = handler.on_enter(&State::PendingMerge).await;
    assert!(
        result.is_ok(),
        "on_enter(PendingMerge) should succeed: {:?}",
        result
    );

    // With no registry, re-entry guard doesn't fire → operations proceed
    let state = mock_github.state();
    assert_eq!(
        state.push_branch_calls, 1,
        "push_branch should be called when no registry prevents re-entry"
    );
    assert_eq!(
        state.mark_pr_ready_calls, 1,
        "mark_pr_ready should be called when no registry prevents re-entry"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 5: AD14 — PR-polling task in Merging does not block second PendingMerge task
// ─────────────────────────────────────────────────────────────────────────────

/// AD14: find_blocking_merge_task skips tasks whose merge_task_id is in pr_polling_ids.
///
/// Setup:
///   - Task A: in Merging, plan_branch has pr_polling_active=true
///   - Task B: in PendingMerge (pr_eligible=true, pr_number=55), test subject
///
/// Without AD14, Task A (Merging) would block Task B (PendingMerge) from proceeding.
/// With AD14, Task A is excluded from blocking → Task B proceeds → push_branch called.
#[tokio::test]
async fn test_ad14_pr_polling_task_does_not_block_pending_merge() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    setup_project(&project_repo).await;

    // Task A: in Merging, is a PR-polling task
    let mut task_a = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        "Task A (merging)".to_string(),
    );
    task_a.id = TaskId::from_string("task-a-merging".to_string());
    task_a.internal_status = InternalStatus::Merging;
    task_a.category = TaskCategory::PlanMerge;
    let task_a_id = task_a.id.clone();
    task_repo.create(task_a).await.unwrap();

    // Plan branch for Task A: pr_polling_active=true makes it excluded by AD14
    let mut pb_a = PlanBranch::new(
        ArtifactId::from_string("artifact-a".to_string()),
        IdeationSessionId::from_string("sess-1".to_string()),
        ProjectId::from_string("proj-1".to_string()),
        "plan/branch-a".to_string(),
        "main".to_string(),
    );
    pb_a.merge_task_id = Some(task_a_id.clone());
    pb_a.pr_eligible = true;
    pb_a.pr_number = Some(10);
    pb_a.pr_polling_active = true;
    plan_branch_repo.create(pb_a).await.unwrap();

    // Task B: in PendingMerge — this is what we're testing
    let task_b_id = create_pending_merge_task(&task_repo, "task-b-pending").await;

    // Plan branch for Task B: pr_eligible=true, pr_number=55
    let pb_b = make_pr_eligible_plan_branch(&task_b_id, Some(55), false);
    plan_branch_repo.create(pb_b).await.unwrap();

    let mock_github = Arc::new(MockGithubService::new());

    let services = TaskServices::new_mock()
        .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
        .with_project_repo(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>)
        .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>)
        .with_github_service(Arc::clone(&mock_github) as Arc<dyn GithubServiceTrait>)
        .with_plan_pr_description_drafter(Arc::new(StaticPlanPrDescriptionDrafter::default()));

    let context = TaskContext::new(task_b_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let result = handler.on_enter(&State::PendingMerge).await;
    assert!(
        result.is_ok(),
        "on_enter(PendingMerge) should succeed: {:?}",
        result
    );

    // Task B should proceed (not deferred by Task A) and call push_branch
    let state = mock_github.state();
    assert_eq!(
        state.push_branch_calls, 1,
        "Task B should proceed despite Task A being in Merging (AD14 excludes PR-polling tasks)"
    );
    assert_eq!(
        state.mark_pr_ready_calls, 1,
        "Task B should mark PR ready after proceeding"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 6: post_merge_cleanup idempotency — Merged plan branch returns early
// ─────────────────────────────────────────────────────────────────────────────

/// on_enter(Merged) calls post_merge_cleanup for PlanMerge tasks.
/// If plan_branch.status is already Merged, the cleanup returns early (idempotency guard).
/// Test verifies: no error, no infinite loop, expected guard branch executes.
#[tokio::test]
async fn test_post_merge_cleanup_idempotency_already_merged_plan_branch() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    setup_project(&project_repo).await;

    // Task in Merged state (simulating successful merge)
    let mut task = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        "Merged task".to_string(),
    );
    task.id = TaskId::from_string("task-already-merged".to_string());
    task.internal_status = InternalStatus::Merged;
    task.category = TaskCategory::PlanMerge;
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    // Plan branch already in Merged status — idempotency guard should trigger
    let mut pb = make_pr_eligible_plan_branch(&task_id, Some(88), false);
    pb.status = PlanBranchStatus::Merged;
    plan_branch_repo.create(pb).await.unwrap();

    let services = TaskServices::new_mock()
        .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
        .with_project_repo(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>)
        .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>);

    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    // on_enter(Merged) → post_merge_cleanup → idempotency guard → early return
    let result = handler.on_enter(&State::Merged).await;
    assert!(
        result.is_ok(),
        "on_enter(Merged) with already-merged plan branch should succeed without error: {:?}",
        result
    );

    // Plan branch status should remain Merged (not double-transitioned)
    let pb_after = plan_branch_repo
        .get_by_merge_task_id(&task_id)
        .await
        .unwrap()
        .expect("plan branch should still exist");
    assert_eq!(
        pb_after.status,
        PlanBranchStatus::Merged,
        "plan branch should still be Merged after idempotent cleanup"
    );
}

#[tokio::test]
async fn test_regular_plan_task_merged_state_creates_draft_pr_after_first_merge() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    let session_repo = Arc::new(MemoryIdeationSessionRepository::new());
    let artifact_repo = Arc::new(MemoryArtifactRepository::new());

    let branch_name = "plan/feature-branch";
    let repo = setup_plan_git_repo(branch_name, true);
    setup_project_with_path(&project_repo, repo.path().to_string_lossy().into_owned()).await;

    let session = IdeationSession::new_with_title(
        ProjectId::from_string("proj-1".to_string()),
        "Fix graph crash when no active plan selected",
    );
    let session_id = session.id.clone();
    session_repo.create(session).await.unwrap();

    let plan_artifact = Artifact::new_inline(
        "Execution Plan",
        ArtifactType::Specification,
        "## Goal\n\n- Preserve the empty state\n- Thread `executionPlanId` through the timeline components\n",
        "ralphx-plan",
    );
    let plan_artifact_id = plan_artifact.id.clone();
    artifact_repo.create(plan_artifact).await.unwrap();

    let mut task = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        "Merged plan task".to_string(),
    );
    task.id = TaskId::from_string("task-plan-merged".to_string());
    task.internal_status = InternalStatus::Merged;
    task.category = TaskCategory::Regular;
    task.ideation_session_id = Some(session_id.clone());
    task.plan_artifact_id = Some(plan_artifact_id.clone());
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let mut plan_branch = make_plan_branch(
        plan_artifact_id.as_str(),
        branch_name,
        PlanBranchStatus::Active,
        None,
    );
    plan_branch.session_id = session_id;
    plan_branch.pr_eligible = true;
    let branch_id = plan_branch.id.clone();
    plan_branch_repo.create(plan_branch).await.unwrap();

    let mock_github = Arc::new(MockGithubService::new());
    mock_github.will_create_pr(123, "https://github.com/owner/repo/pull/123");

    let services = TaskServices::new_mock()
        .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
        .with_project_repo(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>)
        .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>)
        .with_ideation_session_repo(Arc::clone(&session_repo) as Arc<dyn IdeationSessionRepository>)
        .with_artifact_repo(Arc::clone(&artifact_repo) as Arc<dyn ArtifactRepository>)
        .with_pr_creation_guard(Arc::new(dashmap::DashMap::new()))
        .with_github_service(Arc::clone(&mock_github) as Arc<dyn GithubServiceTrait>)
        .with_plan_pr_description_drafter(Arc::new(StaticPlanPrDescriptionDrafter::default()));

    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let result = handler.on_enter(&State::Merged).await;
    assert!(
        result.is_ok(),
        "on_enter(Merged) should succeed: {:?}",
        result
    );

    {
        let state = mock_github.state();
        assert_eq!(
            state.push_branch_calls, 1,
            "first merged plan task should push the plan branch"
        );
        assert_eq!(
            state.create_draft_pr_calls, 1,
            "first merged plan task should create the draft PR once the plan branch has reviewable changes"
        );
    }

    let updated_plan_branch = plan_branch_repo
        .get_by_id(&branch_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated_plan_branch.pr_number, Some(123));

    let state = mock_github.state();
    let (_, _, title, _) = state
        .last_create_draft_pr_args
        .clone()
        .expect("expected draft PR arguments to be recorded");
    assert_eq!(title, "Plan: Fix graph crash when no active plan selected");

    let body = state
        .last_create_draft_pr_body
        .clone()
        .expect("expected draft PR body to be captured");
    assert!(body.starts_with("## Summary\n\nDrafted by plan PR describer"));
    assert!(!body.contains("## RalphX Status"));
    assert!(!body.contains("## How To Review"));
    assert!(!body.contains("Merge this PR in GitHub"));
    assert!(body.contains("## Plan"));
    assert!(body.contains("<details>"));
    assert!(body.contains("<summary>View full plan</summary>"));
    assert!(body.contains("Thread `executionPlanId` through the timeline components"));
    assert!(body.contains("</details>"));
    assert!(body.contains("Generated by [RalphX](https://github.com/aigentive/ralphx.app)"));
    assert!(!body.contains("## Delivered Changes"));
    assert!(!body.contains("Changed files"));

    assert_eq!(
        state.update_pr_details_calls, 1,
        "single-task plan should refresh PR details before marking ready"
    );
    assert_eq!(
        state.mark_pr_ready_calls, 1,
        "single-task plan should mark the PR ready immediately after creating it"
    );
    let (updated_pr_number, updated_title, _) = state
        .last_update_pr_details_args
        .clone()
        .expect("expected ready PR update arguments");
    assert_eq!(updated_pr_number, 123);
    assert_eq!(
        updated_title,
        "Fix graph crash when no active plan selected"
    );
    let ready_body = state
        .last_update_pr_details_body
        .clone()
        .expect("expected ready PR body");
    assert!(ready_body.starts_with("## Summary\n\nDrafted by plan PR describer"));
    assert!(ready_body.contains("<details>"));
    assert!(ready_body.contains("<summary>View full plan</summary>"));
    assert!(!ready_body.contains("opened this draft PR"));
}

#[tokio::test]
async fn create_draft_pr_if_needed_marks_failed_when_describer_fails() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    let branch_name = "plan/describer-fails";
    let repo = setup_plan_git_repo(branch_name, true);
    setup_project_with_path(&project_repo, repo.path().to_string_lossy().into_owned()).await;
    let project = project_repo
        .get_by_id(&ProjectId::from_string("proj-1".to_string()))
        .await
        .unwrap()
        .unwrap();

    let mut task = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        "Merged plan task".to_string(),
    );
    task.id = TaskId::from_string("task-plan-draft-describer-fails".to_string());
    task.internal_status = InternalStatus::Merged;
    task.category = TaskCategory::Regular;
    task.ideation_session_id = Some(IdeationSessionId::from_string("sess-1".to_string()));
    task_repo.create(task.clone()).await.unwrap();

    let mut plan_branch =
        make_plan_branch("artifact-1", branch_name, PlanBranchStatus::Active, None);
    plan_branch.pr_eligible = true;
    let plan_branch_id = plan_branch.id.clone();
    plan_branch_repo.create(plan_branch.clone()).await.unwrap();

    let mock_github = Arc::new(MockGithubService::new());
    let github_trait: Arc<dyn GithubServiceTrait> = mock_github.clone();
    let plan_branch_repo_trait: Arc<dyn PlanBranchRepository> = plan_branch_repo.clone();
    let failing_drafter: Arc<dyn PlanPrDescriptionDrafter> =
        Arc::new(FailingPlanPrDescriptionDrafter);
    let guard = Arc::new(dashmap::DashMap::new());

    super::super::merge_helpers::create_draft_pr_if_needed(
        &task,
        &project,
        &plan_branch,
        &guard,
        &github_trait,
        &plan_branch_repo_trait,
        Some(&failing_drafter),
        None,
        None,
    )
    .await;

    {
        let state = mock_github.state();
        assert_eq!(state.push_branch_calls, 1);
        assert_eq!(
            state.create_draft_pr_calls, 0,
            "describer failure should stop before creating a GitHub PR"
        );
    }

    assert!(guard.is_empty(), "creation guard should be released");
    let updated_plan_branch = plan_branch_repo
        .get_by_id(&plan_branch_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated_plan_branch.pr_number, None);
    assert_eq!(updated_plan_branch.pr_push_status, PrPushStatus::Failed);
}

#[tokio::test]
async fn test_regular_plan_task_completion_creates_draft_pr_after_first_local_merge() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    let branch_name = "plan/feature-branch";
    let repo = setup_plan_git_repo(branch_name, true);
    setup_project_with_path(&project_repo, repo.path().to_string_lossy().into_owned()).await;
    let project = project_repo
        .get_by_id(&ProjectId::from_string("proj-1".to_string()))
        .await
        .unwrap()
        .unwrap();

    let mut task = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        "Merged by programmatic merge".to_string(),
    );
    task.id = TaskId::from_string("task-programmatic-plan-merge".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.category = TaskCategory::Regular;
    task.ideation_session_id = Some(IdeationSessionId::from_string("sess-1".to_string()));
    task.plan_artifact_id = Some(ArtifactId::from_string("artifact-1".to_string()));
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let mut plan_branch =
        make_plan_branch("artifact-1", branch_name, PlanBranchStatus::Active, None);
    plan_branch.pr_eligible = true;
    let branch_id = plan_branch.id.clone();
    plan_branch_repo.create(plan_branch).await.unwrap();

    let commit_output = std::process::Command::new("git")
        .args(["rev-parse", branch_name])
        .current_dir(repo.path())
        .output()
        .expect("read plan branch sha");
    assert!(
        commit_output.status.success(),
        "rev-parse plan branch should succeed"
    );
    let commit_sha = String::from_utf8_lossy(&commit_output.stdout)
        .trim()
        .to_string();

    let mock_github = Arc::new(MockGithubService::new());
    mock_github.will_create_pr(456, "https://github.com/owner/repo/pull/456");

    let mut task_for_merge = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    let task_repo_dyn: Arc<dyn TaskRepository> = Arc::clone(&task_repo) as Arc<dyn TaskRepository>;
    let result = complete_merge_internal_with_pr_sync_and_notifier(
        &mut task_for_merge,
        &project,
        &commit_sha,
        "task/feature",
        branch_name,
        &task_repo_dyn,
        None,
        None,
        None,
        None,
        Some(PlanBranchPrSyncServices {
            task_repo: Some(Arc::clone(&task_repo) as Arc<dyn TaskRepository>),
            branch_update_repo: None,
            branch_update_workflow: None,
            plan_branch_repo: Some(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>),
            pr_creation_guard: Some(Arc::new(dashmap::DashMap::new())),
            github_service: Some(Arc::clone(&mock_github) as Arc<dyn GithubServiceTrait>),
            ideation_session_repo: None,
            artifact_repo: None,
            plan_pr_description_drafter: Some(Arc::new(StaticPlanPrDescriptionDrafter::default())),
        }),
        None,
    )
    .await;
    assert!(
        result.is_ok(),
        "complete_merge_internal_with_pr_sync_and_notifier should succeed: {:?}",
        result
    );

    {
        let state = mock_github.state();
        assert_eq!(
            state.push_branch_calls, 1,
            "programmatic local plan-task merge should push the plan branch"
        );
        assert_eq!(
            state.create_draft_pr_calls, 1,
            "programmatic local plan-task merge should create the first draft PR"
        );
    }

    let final_task = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(final_task.internal_status, InternalStatus::Merged);

    let updated_plan_branch = plan_branch_repo
        .get_by_id(&branch_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated_plan_branch.pr_number, Some(456));
}

#[tokio::test]
async fn test_regular_plan_task_completion_pushes_existing_pr_after_local_merge() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    let branch_name = "plan/existing-pr-branch";
    let repo = setup_plan_git_repo(branch_name, true);
    setup_project_with_path(&project_repo, repo.path().to_string_lossy().into_owned()).await;
    let project = project_repo
        .get_by_id(&ProjectId::from_string("proj-1".to_string()))
        .await
        .unwrap()
        .unwrap();

    let mut task = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        "Merged follow-up via programmatic merge".to_string(),
    );
    task.id = TaskId::from_string("task-programmatic-plan-pr-sync".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.category = TaskCategory::Regular;
    task.ideation_session_id = Some(IdeationSessionId::from_string("sess-1".to_string()));
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let mut merge_task = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        "Plan PR merge".to_string(),
    );
    merge_task.id = TaskId::from_string("task-programmatic-plan-pr-sync-merge".to_string());
    merge_task.internal_status = InternalStatus::Blocked;
    merge_task.category = TaskCategory::PlanMerge;
    merge_task.metadata = Some(
        serde_json::json!({
            "github_auto_merge_disabled_for_correction": true,
            "github_auto_merge_pr_number": 789,
            "github_auto_merge_method": "merge",
            "github_auto_merge_disabled_at": "2026-07-10T12:00:00Z",
            "github_auto_merge_disabled_source": "github_review_feedback",
        })
        .to_string(),
    );
    let merge_task_id = merge_task.id.clone();
    task_repo.create(merge_task).await.unwrap();

    let mut plan_branch =
        make_plan_branch("artifact-1", branch_name, PlanBranchStatus::Active, None);
    plan_branch.merge_task_id = Some(merge_task_id.clone());
    plan_branch.pr_eligible = true;
    plan_branch.pr_number = Some(789);
    plan_branch.pr_url = Some("https://github.com/owner/repo/pull/789".to_string());
    plan_branch.pr_push_status = crate::domain::entities::plan_branch::PrPushStatus::Pushed;
    let branch_id = plan_branch.id.clone();
    plan_branch_repo.create(plan_branch).await.unwrap();

    let commit_output = std::process::Command::new("git")
        .args(["rev-parse", branch_name])
        .current_dir(repo.path())
        .output()
        .expect("read plan branch sha");
    assert!(
        commit_output.status.success(),
        "rev-parse plan branch should succeed"
    );
    let commit_sha = String::from_utf8_lossy(&commit_output.stdout)
        .trim()
        .to_string();

    let mock_github = Arc::new(MockGithubService::new());

    let mut task_for_merge = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    let task_repo_dyn: Arc<dyn TaskRepository> = Arc::clone(&task_repo) as Arc<dyn TaskRepository>;
    let result = complete_merge_internal_with_pr_sync_and_notifier(
        &mut task_for_merge,
        &project,
        &commit_sha,
        "task/feature",
        branch_name,
        &task_repo_dyn,
        None,
        None,
        None,
        None,
        Some(PlanBranchPrSyncServices {
            task_repo: Some(Arc::clone(&task_repo) as Arc<dyn TaskRepository>),
            branch_update_repo: None,
            branch_update_workflow: None,
            plan_branch_repo: Some(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>),
            pr_creation_guard: Some(Arc::new(dashmap::DashMap::new())),
            github_service: Some(Arc::clone(&mock_github) as Arc<dyn GithubServiceTrait>),
            ideation_session_repo: None,
            artifact_repo: None,
            plan_pr_description_drafter: Some(Arc::new(StaticPlanPrDescriptionDrafter::default())),
        }),
        None,
    )
    .await;
    assert!(
        result.is_ok(),
        "complete_merge_internal_with_pr_sync_and_notifier should succeed: {:?}",
        result
    );

    {
        let state = mock_github.state();
        assert_eq!(
            state.push_branch_calls, 1,
            "programmatic local plan-task merge should push existing PR branch updates"
        );
        assert_eq!(
            state.create_draft_pr_calls, 0,
            "existing PR-backed branches should sync instead of creating another PR"
        );
        assert_eq!(
            state.enable_pr_auto_merge_calls, 1,
            "regular correction completion should restore disabled PR auto-merge"
        );
        assert_eq!(
            state.last_enable_pr_auto_merge_args.as_ref(),
            Some(&(789, "merge".to_string()))
        );
        assert_eq!(
            state.last_push_branch_name.as_deref(),
            Some(branch_name),
            "push should target the plan branch"
        );
    }

    let updated_plan_branch = plan_branch_repo
        .get_by_id(&branch_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated_plan_branch.pr_push_status,
        crate::domain::entities::plan_branch::PrPushStatus::Pushed
    );

    let updated_merge_task = task_repo
        .get_by_id(&merge_task_id)
        .await
        .unwrap()
        .expect("merge task should exist");
    let metadata: Value = serde_json::from_str(
        updated_merge_task
            .metadata
            .as_deref()
            .expect("metadata should exist"),
    )
    .expect("metadata should be valid JSON");
    assert!(
        metadata
            .get("github_auto_merge_disabled_for_correction")
            .is_none(),
        "successful regular correction PR sync should consume the active marker"
    );
    assert_eq!(
        metadata["github_auto_merge_reenabled_method"],
        Value::String("merge".to_string())
    );
}

#[tokio::test]
async fn test_regular_plan_task_completion_updates_existing_pr_as_draft_when_plan_open() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    setup_project(&project_repo).await;
    let project = project_repo
        .get_by_id(&ProjectId::from_string("proj-1".to_string()))
        .await
        .unwrap()
        .unwrap();

    let mut task = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        "Merged follow-up while plan remains open".to_string(),
    );
    task.id = TaskId::from_string("task-programmatic-plan-pr-draft".to_string());
    task.internal_status = InternalStatus::Merged;
    task.category = TaskCategory::Regular;
    task.ideation_session_id = Some(IdeationSessionId::from_string("sess-1".to_string()));
    task_repo.create(task.clone()).await.unwrap();

    let mut open_sibling = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        "Still executing sibling".to_string(),
    );
    open_sibling.id = TaskId::from_string("task-programmatic-plan-open-sibling".to_string());
    open_sibling.internal_status = InternalStatus::Executing;
    open_sibling.category = TaskCategory::Regular;
    open_sibling.ideation_session_id = Some(IdeationSessionId::from_string("sess-1".to_string()));
    task_repo.create(open_sibling).await.unwrap();

    let branch_name = "plan/existing-pr-draft-update";
    let mut plan_branch =
        make_plan_branch("artifact-1", branch_name, PlanBranchStatus::Active, None);
    plan_branch.pr_eligible = true;
    plan_branch.pr_number = Some(789);
    plan_branch.pr_url = Some("https://github.com/owner/repo/pull/789".to_string());
    plan_branch.pr_push_status = PrPushStatus::Pushed;
    let branch_id = plan_branch.id.clone();
    plan_branch_repo.create(plan_branch).await.unwrap();

    let mock_github = Arc::new(MockGithubService::new());
    let result = super::super::merge_helpers::sync_plan_branch_pr_after_regular_task_merge(
        &task,
        &project,
        &PlanBranchPrSyncServices {
            task_repo: Some(Arc::clone(&task_repo) as Arc<dyn TaskRepository>),
            branch_update_repo: None,
            branch_update_workflow: None,
            plan_branch_repo: Some(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>),
            pr_creation_guard: Some(Arc::new(dashmap::DashMap::new())),
            github_service: Some(Arc::clone(&mock_github) as Arc<dyn GithubServiceTrait>),
            ideation_session_repo: None,
            artifact_repo: None,
            plan_pr_description_drafter: Some(Arc::new(StaticPlanPrDescriptionDrafter::default())),
        },
    )
    .await;

    assert!(
        result.is_ok(),
        "draft PR sync should succeed while other plan tasks remain open: {:?}",
        result
    );

    {
        let state = mock_github.state();
        assert_eq!(state.push_branch_calls, 1);
        assert_eq!(
            state.update_pr_details_calls, 1,
            "existing PR should be refreshed with draft details"
        );
        assert_eq!(
            state.mark_pr_ready_calls, 0,
            "open sibling tasks should keep the existing PR in draft mode"
        );
        let (_, title, _) = state
            .last_update_pr_details_args
            .as_ref()
            .expect("update PR details args");
        assert_eq!(title, "Plan: Merged follow-up while plan remains open");
        let body = state
            .last_update_pr_details_body
            .as_ref()
            .expect("update PR details body");
        assert!(body.starts_with("## Summary\n\nDrafted by plan PR describer"));
        assert!(!body.contains("## RalphX Status"));
        assert!(!body.contains("## How To Review"));
    }

    let updated_plan_branch = plan_branch_repo
        .get_by_id(&branch_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated_plan_branch.pr_push_status, PrPushStatus::Pushed);
}

#[tokio::test]
async fn test_regular_plan_task_completion_push_failure_does_not_mark_task_merged() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    let branch_name = "plan/existing-pr-push-fails";
    let repo = setup_plan_git_repo(branch_name, true);
    setup_project_with_path(&project_repo, repo.path().to_string_lossy().into_owned()).await;
    let project = project_repo
        .get_by_id(&ProjectId::from_string("proj-1".to_string()))
        .await
        .unwrap()
        .unwrap();

    let mut task = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        "Merged follow-up with failed publication".to_string(),
    );
    task.id = TaskId::from_string("task-programmatic-plan-pr-push-fails".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.category = TaskCategory::Regular;
    task.ideation_session_id = Some(IdeationSessionId::from_string("sess-1".to_string()));
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let mut plan_branch =
        make_plan_branch("artifact-1", branch_name, PlanBranchStatus::Active, None);
    plan_branch.pr_eligible = true;
    plan_branch.pr_number = Some(789);
    plan_branch.pr_url = Some("https://github.com/owner/repo/pull/789".to_string());
    plan_branch.pr_push_status = PrPushStatus::Pushed;
    let branch_id = plan_branch.id.clone();
    plan_branch_repo.create(plan_branch).await.unwrap();

    let commit_output = std::process::Command::new("git")
        .args(["rev-parse", branch_name])
        .current_dir(repo.path())
        .output()
        .expect("read plan branch sha");
    assert!(
        commit_output.status.success(),
        "rev-parse plan branch should succeed"
    );
    let commit_sha = String::from_utf8_lossy(&commit_output.stdout)
        .trim()
        .to_string();

    let mock_github = Arc::new(MockGithubService::new());
    mock_github.state().push_branch_result =
        Some(Err(AppError::GitOperation("push rejected".to_string())));

    let recording_notifier = Arc::new(RecordingNotifier::default());
    let notifier: Arc<dyn Notifier> = Arc::clone(&recording_notifier) as Arc<dyn Notifier>;

    let mut task_for_merge = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    let task_repo_dyn: Arc<dyn TaskRepository> = Arc::clone(&task_repo) as Arc<dyn TaskRepository>;
    let result = complete_merge_internal_with_pr_sync_and_notifier(
        &mut task_for_merge,
        &project,
        &commit_sha,
        "task/feature",
        branch_name,
        &task_repo_dyn,
        None,
        None,
        None,
        None,
        Some(PlanBranchPrSyncServices {
            task_repo: Some(Arc::clone(&task_repo) as Arc<dyn TaskRepository>),
            branch_update_repo: None,
            branch_update_workflow: None,
            plan_branch_repo: Some(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>),
            pr_creation_guard: Some(Arc::new(dashmap::DashMap::new())),
            github_service: Some(Arc::clone(&mock_github) as Arc<dyn GithubServiceTrait>),
            ideation_session_repo: None,
            artifact_repo: None,
            plan_pr_description_drafter: Some(Arc::new(StaticPlanPrDescriptionDrafter::default())),
        }),
        Some(&notifier),
    )
    .await;

    assert!(
        result.is_err(),
        "merge completion must fail closed when the PR branch cannot be pushed"
    );

    let final_task = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(
        final_task.internal_status,
        InternalStatus::MergeIncomplete,
        "failed PR branch publication must not mark the task Merged"
    );
    let notifications = recording_notifier.notifications();
    assert_eq!(notifications.len(), 1);
    let (context, notification) = &notifications[0];
    assert!(matches!(
        notification,
        TaskNotification::StateEntered(InternalStatus::MergeIncomplete)
    ));
    assert_eq!(context.task.id, task_id);
    assert_eq!(context.project_id, final_task.project_id);
    assert!(
        uuid::Uuid::parse_str(&context.history_entry_id).is_ok(),
        "the direct merge-completion path must receive the committed history entry id"
    );
    let history = task_repo
        .get_status_history(&task_id)
        .await
        .expect("direct merge completion should persist status history");
    assert!(history.iter().any(|entry| {
        entry.from == InternalStatus::PendingMerge
            && entry.to == InternalStatus::MergeIncomplete
            && entry.trigger == "pr_branch_publication_failed"
    }));
    let recovery = MergeRecoveryMetadata::from_task_metadata(final_task.metadata.as_deref())
        .unwrap()
        .unwrap_or_else(MergeRecoveryMetadata::new);
    assert!(
        !recovery
            .events
            .iter()
            .any(|event| matches!(event.kind, MergeRecoveryEventKind::AttemptSucceeded)),
        "failed PR branch publication must not record a successful merge attempt"
    );

    let updated_plan_branch = plan_branch_repo
        .get_by_id(&branch_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated_plan_branch.pr_push_status, PrPushStatus::Failed);
}

#[tokio::test]
async fn test_regular_plan_task_completion_repairs_non_fast_forward_pr_publication() {
    let db = SqliteTestDb::new("pr-publication-authority-fast-forward");
    let task_repo = Arc::new(SqliteTaskRepository::from_shared(db.shared_conn()));
    let branch_update_repo = Arc::new(SqliteBranchUpdateRepository::from_shared(db.shared_conn()));
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    let branch_name = "plan/existing-pr-non-fast-forward";
    let repo = setup_plan_git_repo(branch_name, true);
    let (_remote, commit_sha, remote_sha) =
        setup_origin_with_remote_plan_branch_ahead(repo.path(), branch_name);
    let workspace_parent = tempfile::tempdir().unwrap();
    let mut project = db.seed_project("pr-publication-authority-fast-forward");
    project.working_directory = repo.path().to_string_lossy().into_owned();
    project.worktree_parent_directory =
        Some(workspace_parent.path().to_string_lossy().into_owned());

    let mut task = Task::new(
        project.id.clone(),
        "Merged follow-up with stale local PR branch".to_string(),
    );
    task.id = TaskId::from_string("task-programmatic-plan-pr-nff".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.category = TaskCategory::Regular;
    task.ideation_session_id = Some(IdeationSessionId::from_string("sess-1".to_string()));
    let task_id = task.id.clone();
    db.insert_task(task);

    let mut plan_branch =
        make_plan_branch("artifact-1", branch_name, PlanBranchStatus::Active, None);
    plan_branch.pr_eligible = true;
    plan_branch.pr_number = Some(789);
    plan_branch.pr_url = Some("https://github.com/owner/repo/pull/789".to_string());
    plan_branch.pr_push_status = PrPushStatus::Pushed;
    let branch_id = plan_branch.id.clone();
    plan_branch_repo.create(plan_branch).await.unwrap();

    let mock_github = Arc::new(MockGithubService::new());
    mock_github.state().push_branch_result = Some(Err(AppError::GitOperation(
        "! [rejected] plan/existing-pr-non-fast-forward -> plan/existing-pr-non-fast-forward (non-fast-forward)\nupdates were rejected because the tip of your current branch is behind its remote counterpart".to_string(),
    )));

    let mut task_for_merge = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    let task_repo_dyn: Arc<dyn TaskRepository> = Arc::clone(&task_repo) as Arc<dyn TaskRepository>;
    let result = complete_merge_internal_with_pr_sync_and_notifier(
        &mut task_for_merge,
        &project,
        &commit_sha,
        "task/feature",
        branch_name,
        &task_repo_dyn,
        None,
        None,
        None,
        None,
        Some(PlanBranchPrSyncServices {
            task_repo: Some(Arc::clone(&task_repo) as Arc<dyn TaskRepository>),
            branch_update_repo: Some(Arc::clone(&branch_update_repo)
                as Arc<dyn crate::domain::repositories::BranchUpdateRepository>),
            branch_update_workflow: Some(crate::testing::branch_update_workflow(Arc::new(
                MockChatService::new(),
            ))),
            plan_branch_repo: Some(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>),
            pr_creation_guard: Some(Arc::new(dashmap::DashMap::new())),
            github_service: Some(Arc::clone(&mock_github) as Arc<dyn GithubServiceTrait>),
            ideation_session_repo: None,
            artifact_repo: None,
            plan_pr_description_drafter: Some(Arc::new(StaticPlanPrDescriptionDrafter::default())),
        }),
        None,
    )
    .await;

    assert!(
        result.is_ok(),
        "non-fast-forward PR branch publication should repair and finish: {:?}",
        result
    );

    {
        let state = mock_github.state();
        assert_eq!(
            state.push_branch_calls, 1,
            "the rejected adapter push must not be retried outside durable authority"
        );
        assert_eq!(
            state.last_push_branch_name.as_deref(),
            Some(branch_name),
            "push should still target the plan branch"
        );
    }

    let final_task = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(final_task.internal_status, InternalStatus::Merged);
    let recovery = MergeRecoveryMetadata::from_task_metadata(final_task.metadata.as_deref())
        .unwrap()
        .expect("successful completion should write merge recovery metadata");
    assert!(
        recovery
            .events
            .iter()
            .any(|event| matches!(event.kind, MergeRecoveryEventKind::AttemptSucceeded)),
        "successful publication should record attempt success"
    );

    let updated_plan_branch = plan_branch_repo
        .get_by_id(&branch_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated_plan_branch.pr_push_status, PrPushStatus::Pushed);

    let contains_remote_sha = std::process::Command::new("git")
        .args(["merge-base", "--is-ancestor", &remote_sha, branch_name])
        .current_dir(repo.path())
        .status()
        .expect("check repaired branch ancestry")
        .success();
    assert!(
        contains_remote_sha,
        "local plan branch should include the remote PR branch head after repair"
    );
}

#[tokio::test]
async fn test_regular_plan_task_completion_routes_conflicting_pr_publication_to_merger() {
    let db = SqliteTestDb::new("pr-publication-authority-conflict");
    let task_repo = Arc::new(SqliteTaskRepository::from_shared(db.shared_conn()));
    let branch_update_repo = Arc::new(SqliteBranchUpdateRepository::from_shared(db.shared_conn()));
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    let branch_name = "plan/existing-pr-publication-conflict";
    let repo = setup_plan_git_repo(branch_name, true);
    let (_remote, local_commit_sha, remote_sha) =
        setup_origin_with_conflicting_remote_plan_branch_ahead(repo.path(), branch_name);
    let workspace_parent = tempfile::tempdir().unwrap();
    let mut project = db.seed_project("pr-publication-authority-conflict");
    project.working_directory = repo.path().to_string_lossy().into_owned();
    project.worktree_parent_directory =
        Some(workspace_parent.path().to_string_lossy().into_owned());

    let mut task = Task::new(
        project.id.clone(),
        "Merged follow-up with conflicting remote PR branch".to_string(),
    );
    task.id = TaskId::from_string("task-programmatic-plan-pr-conflict".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.category = TaskCategory::Regular;
    task.ideation_session_id = Some(IdeationSessionId::from_string("sess-1".to_string()));
    let task_id = task.id.clone();
    db.insert_task(task);

    let mut plan_branch =
        make_plan_branch("artifact-1", branch_name, PlanBranchStatus::Active, None);
    plan_branch.pr_eligible = true;
    plan_branch.pr_number = Some(789);
    plan_branch.pr_url = Some("https://github.com/owner/repo/pull/789".to_string());
    plan_branch.pr_push_status = PrPushStatus::Pushed;
    let branch_id = plan_branch.id.clone();
    plan_branch_repo.create(plan_branch).await.unwrap();

    let mock_github = Arc::new(MockGithubService::new());
    mock_github.state().push_branch_result = Some(Err(AppError::GitOperation(
        "! [rejected] plan/existing-pr-publication-conflict -> plan/existing-pr-publication-conflict (non-fast-forward)\nupdates were rejected because the tip of your current branch is behind its remote counterpart".to_string(),
    )));

    let mut task_for_merge = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    let task_repo_dyn: Arc<dyn TaskRepository> = Arc::clone(&task_repo) as Arc<dyn TaskRepository>;
    let result = complete_merge_internal_with_pr_sync_and_notifier(
        &mut task_for_merge,
        &project,
        &local_commit_sha,
        "task/feature",
        branch_name,
        &task_repo_dyn,
        None,
        None,
        None,
        None,
        Some(PlanBranchPrSyncServices {
            task_repo: Some(Arc::clone(&task_repo) as Arc<dyn TaskRepository>),
            branch_update_repo: Some(Arc::clone(&branch_update_repo)
                as Arc<dyn crate::domain::repositories::BranchUpdateRepository>),
            branch_update_workflow: Some(crate::testing::branch_update_workflow(Arc::new(
                MockChatService::new(),
            ))),
            plan_branch_repo: Some(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>),
            pr_creation_guard: Some(Arc::new(dashmap::DashMap::new())),
            github_service: Some(Arc::clone(&mock_github) as Arc<dyn GithubServiceTrait>),
            ideation_session_repo: None,
            artifact_repo: None,
            plan_pr_description_drafter: Some(Arc::new(StaticPlanPrDescriptionDrafter::default())),
        }),
        None,
    )
    .await;

    let error = result.expect_err("conflicting publication repair should route, not finish");
    assert!(
        super::super::merge_helpers::is_pr_branch_publication_conflict_routed_error(&error),
        "unexpected error: {error}"
    );

    let final_task = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(
        final_task.internal_status,
        InternalStatus::UpdatingPlanBranch,
        "publication conflicts should route to the branch updater, not merge recovery"
    );
    let metadata: serde_json::Value =
        serde_json::from_str(final_task.metadata.as_deref().unwrap()).unwrap();
    assert_eq!(
        metadata["error_code"], "pr_branch_publication_conflict",
        "metadata should distinguish conflict routing from failed publication"
    );
    assert_eq!(
        metadata["freshness_origin_state"], "pr_branch_publication",
        "regular-task publication conflicts need a distinct finalizer route"
    );
    assert_eq!(metadata["pr_branch_update_conflict"], true);
    assert_eq!(metadata["pr_branch_publication_conflict"], true);
    assert_eq!(metadata["base_branch"], format!("origin/{branch_name}"));
    assert_eq!(metadata["target_branch"], branch_name);
    let operation = branch_update_repo
        .get_active_operation(&task_id)
        .await
        .unwrap()
        .expect("branch update operation");
    assert!(
        operation
            .workspace_path
            .as_deref()
            .is_some_and(std::path::Path::exists),
        "routed publication conflict should create an operation-owned worktree"
    );
    assert_eq!(
        operation
            .conflict_files
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
        vec!["plan.txt"]
    );

    let recovery = MergeRecoveryMetadata::from_task_metadata(final_task.metadata.as_deref())
        .unwrap()
        .unwrap_or_else(MergeRecoveryMetadata::new);
    assert!(
        !recovery
            .events
            .iter()
            .any(|event| matches!(event.kind, MergeRecoveryEventKind::AttemptSucceeded)),
        "routed conflict must not record a successful merge attempt yet"
    );

    let updated_plan_branch = plan_branch_repo
        .get_by_id(&branch_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated_plan_branch.pr_push_status, PrPushStatus::Pending);
    assert_eq!(
        mock_github.state().push_branch_calls,
        1,
        "conflicting repair should not retry push until the merger resolves conflicts"
    );

    let remote_still_missing_local = std::process::Command::new("git")
        .args([
            "merge-base",
            "--is-ancestor",
            &local_commit_sha,
            &remote_sha,
        ])
        .current_dir(repo.path())
        .status()
        .expect("check remote ancestry")
        .success();
    assert!(
        !remote_still_missing_local,
        "test setup must keep local and remote PR branch heads diverged"
    );
}

#[tokio::test]
async fn test_regular_plan_task_completion_without_github_service_does_not_mark_task_merged() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    let branch_name = "plan/existing-pr-no-github";
    let repo = setup_plan_git_repo(branch_name, true);
    setup_project_with_path(&project_repo, repo.path().to_string_lossy().into_owned()).await;
    let project = project_repo
        .get_by_id(&ProjectId::from_string("proj-1".to_string()))
        .await
        .unwrap()
        .unwrap();

    let mut task = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        "Merged follow-up without GitHub service".to_string(),
    );
    task.id = TaskId::from_string("task-programmatic-plan-pr-no-github".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.category = TaskCategory::Regular;
    task.ideation_session_id = Some(IdeationSessionId::from_string("sess-1".to_string()));
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let mut plan_branch =
        make_plan_branch("artifact-1", branch_name, PlanBranchStatus::Active, None);
    plan_branch.pr_eligible = true;
    plan_branch.pr_number = Some(789);
    plan_branch.pr_url = Some("https://github.com/owner/repo/pull/789".to_string());
    plan_branch.pr_push_status = PrPushStatus::Pushed;
    let branch_id = plan_branch.id.clone();
    plan_branch_repo.create(plan_branch).await.unwrap();

    let commit_output = std::process::Command::new("git")
        .args(["rev-parse", branch_name])
        .current_dir(repo.path())
        .output()
        .expect("read plan branch sha");
    assert!(
        commit_output.status.success(),
        "rev-parse plan branch should succeed"
    );
    let commit_sha = String::from_utf8_lossy(&commit_output.stdout)
        .trim()
        .to_string();

    let mut task_for_merge = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    let task_repo_dyn: Arc<dyn TaskRepository> = Arc::clone(&task_repo) as Arc<dyn TaskRepository>;
    let result = complete_merge_internal_with_pr_sync_and_notifier(
        &mut task_for_merge,
        &project,
        &commit_sha,
        "task/feature",
        branch_name,
        &task_repo_dyn,
        None,
        None,
        None,
        None,
        Some(PlanBranchPrSyncServices {
            task_repo: Some(Arc::clone(&task_repo) as Arc<dyn TaskRepository>),
            branch_update_repo: None,
            branch_update_workflow: None,
            plan_branch_repo: Some(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>),
            pr_creation_guard: Some(Arc::new(dashmap::DashMap::new())),
            github_service: None,
            ideation_session_repo: None,
            artifact_repo: None,
            plan_pr_description_drafter: Some(Arc::new(StaticPlanPrDescriptionDrafter::default())),
        }),
        None,
    )
    .await;

    assert!(
        result.is_err(),
        "merge completion must fail closed when an existing PR cannot be published"
    );

    let final_task = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(
        final_task.internal_status,
        InternalStatus::MergeIncomplete,
        "missing GitHub service must not mark the task Merged"
    );
    let metadata: serde_json::Value =
        serde_json::from_str(final_task.metadata.as_deref().unwrap()).unwrap();
    assert_eq!(
        metadata["error_code"], "pr_branch_publication_failed",
        "MergeIncomplete should carry the PR publication failure reason"
    );

    let updated_plan_branch = plan_branch_repo
        .get_by_id(&branch_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated_plan_branch.pr_push_status, PrPushStatus::Pending);
}

#[tokio::test]
async fn test_regular_plan_task_merged_state_pushes_existing_pr_after_local_update() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    let branch_name = "plan/feature-branch";
    let repo = setup_plan_git_repo(branch_name, true);
    setup_project_with_path(&project_repo, repo.path().to_string_lossy().into_owned()).await;

    let mut task = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        "Merged follow-up task".to_string(),
    );
    task.id = TaskId::from_string("task-plan-pr-sync".to_string());
    task.internal_status = InternalStatus::Merged;
    task.category = TaskCategory::Regular;
    task.ideation_session_id = Some(IdeationSessionId::from_string("sess-1".to_string()));
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let mut plan_branch =
        make_plan_branch("artifact-1", branch_name, PlanBranchStatus::Active, None);
    plan_branch.pr_eligible = true;
    plan_branch.pr_number = Some(321);
    plan_branch.pr_url = Some("https://github.com/owner/repo/pull/321".to_string());
    plan_branch.pr_push_status = crate::domain::entities::plan_branch::PrPushStatus::Pushed;
    let branch_id = plan_branch.id.clone();
    plan_branch_repo.create(plan_branch).await.unwrap();

    let mock_github = Arc::new(MockGithubService::new());

    let services = TaskServices::new_mock()
        .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
        .with_project_repo(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>)
        .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>)
        .with_pr_creation_guard(Arc::new(dashmap::DashMap::new()))
        .with_github_service(Arc::clone(&mock_github) as Arc<dyn GithubServiceTrait>)
        .with_plan_pr_description_drafter(Arc::new(StaticPlanPrDescriptionDrafter::default()));

    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let result = handler.on_enter(&State::Merged).await;
    assert!(
        result.is_ok(),
        "on_enter(Merged) should succeed: {:?}",
        result
    );

    {
        let state = mock_github.state();
        assert_eq!(
            state.push_branch_calls, 1,
            "existing PR branches should be pushed again when new local plan-branch work lands"
        );
        assert_eq!(
            state.create_draft_pr_calls, 0,
            "existing PR branches should sync instead of recreating the PR"
        );
    }

    let updated_plan_branch = plan_branch_repo
        .get_by_id(&branch_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated_plan_branch.pr_push_status,
        crate::domain::entities::plan_branch::PrPushStatus::Pushed
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 7: No github_service → falls through to push-to-main path
// ─────────────────────────────────────────────────────────────────────────────

/// pr_eligible=true but no GithubServiceTrait wired.
/// pr_mode = pr_eligible && github_service.is_some() → false.
/// Falls through to push-to-main path (no PR calls).
#[tokio::test]
async fn test_pr_eligible_true_but_no_github_service_falls_through() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    setup_project(&project_repo).await;
    let task_id = create_pending_merge_task(&task_repo, "task-no-github-svc").await;

    let pb = make_pr_eligible_plan_branch(&task_id, Some(99), false);
    plan_branch_repo.create(pb).await.unwrap();

    // No github_service wired → pr_mode = false
    let services = TaskServices::new_mock()
        .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
        .with_project_repo(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>)
        .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>);
    // NOTE: no .with_github_service()

    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    // Should run the push-to-main path (fails fast on nonexistent git dir) without PR calls
    let result = handler.on_enter(&State::PendingMerge).await;
    assert!(
        result.is_ok(),
        "on_enter(PendingMerge) without github_service should fall through gracefully: {:?}",
        result
    );
    // No assertions on MockGithubService since it was never wired
}
