fn setup_repo(repo: &Path) -> String {
    std::fs::create_dir_all(repo).expect("repo should be created");
    git(repo, &["init", "-b", "main"]);
    git(repo, &["config", "user.email", "test@example.com"]);
    git(repo, &["config", "user.name", "Test User"]);
    std::fs::write(repo.join("README.md"), "base\n").expect("fixture should be written");
    git(repo, &["add", "."]);
    git(repo, &["commit", "-m", "base"]);
    git(repo, &["rev-parse", "HEAD"])
}

#[test]
fn plan_update_outcome_maps_current_states_to_freshness_results() {
    assert_eq!(
        publish_branch_freshness_outcome_from_plan_update(
            PlanUpdateResult::AlreadyUpToDate,
            "main",
            "base-sha",
        ),
        PublishBranchFreshnessOutcome::AlreadyFresh {
            base_commit: "base-sha".to_string(),
            target_ref: "main".to_string(),
        }
    );
    assert_eq!(
        publish_branch_freshness_outcome_from_plan_update(
            PlanUpdateResult::NotPlanBranch,
            "main",
            "base-sha",
        ),
        PublishBranchFreshnessOutcome::AlreadyFresh {
            base_commit: "base-sha".to_string(),
            target_ref: "main".to_string(),
        }
    );
    assert_eq!(
        publish_branch_freshness_outcome_from_plan_update(
            PlanUpdateResult::Updated,
            "origin/main",
            "new-base",
        ),
        PublishBranchFreshnessOutcome::Updated {
            base_commit: "new-base".to_string(),
            target_ref: "origin/main".to_string(),
        }
    );
}

#[test]
fn plan_update_outcome_maps_conflicts_and_errors() {
    let conflict = publish_branch_freshness_outcome_from_plan_update(
        PlanUpdateResult::Conflicts {
            conflict_files: vec![PathBuf::from("src/lib.rs")],
        },
        "main",
        "base-sha",
    );
    assert_eq!(
        conflict,
        PublishBranchFreshnessOutcome::NeedsAgent {
            message: "Merge conflict updating plan branch from main: src/lib.rs".to_string(),
            conflict_files: vec!["src/lib.rs".to_string()],
            base_commit: "base-sha".to_string(),
            target_ref: "main".to_string(),
        }
    );
    assert_eq!(
        publish_branch_freshness_outcome_from_plan_update(
            PlanUpdateResult::Conflicts {
                conflict_files: Vec::new(),
            },
            "main",
            "base-sha",
        ),
        PublishBranchFreshnessOutcome::NeedsAgent {
            message: "Merge conflict updating plan branch from main: unknown files".to_string(),
            conflict_files: Vec::new(),
            base_commit: "base-sha".to_string(),
            target_ref: "main".to_string(),
        }
    );

    assert_eq!(
        publish_branch_freshness_outcome_from_plan_update(
            PlanUpdateResult::Error("git failed".to_string()),
            "main",
            "base-sha",
        ),
        PublishBranchFreshnessOutcome::OperationalError {
            message: "git failed".to_string(),
        }
    );
}

#[tokio::test]
async fn ensure_plan_publish_branch_fresh_reports_already_fresh() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo = temp.path().join("repo");
    let worktrees = temp.path().join("worktrees");
    let main_sha = setup_repo(&repo);
    git(&repo, &["branch", "feature/plan"]);
    let mut project = Project::new(
        "Plan freshness".to_string(),
        repo.to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktrees.to_string_lossy().to_string());

    let outcome = ensure_plan_publish_branch_fresh(
        &repo,
        &project,
        "feature/plan",
        "main",
        "conversation-plan-freshness",
        None,
    )
    .await;

    assert_eq!(
        outcome,
        PublishBranchFreshnessOutcome::AlreadyFresh {
            base_commit: main_sha,
            target_ref: "main".to_string(),
        }
    );
}

#[tokio::test]
async fn ensure_plan_publish_branch_fresh_reports_missing_base_ref() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo = temp.path().join("repo");
    let worktrees = temp.path().join("worktrees");
    setup_repo(&repo);
    git(&repo, &["branch", "feature/plan"]);
    let mut project = Project::new(
        "Plan freshness".to_string(),
        repo.to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktrees.to_string_lossy().to_string());

    let outcome = ensure_plan_publish_branch_fresh(
        &repo,
        &project,
        "feature/plan",
        "missing-base",
        "conversation-plan-freshness",
        None,
    )
    .await;

    match outcome {
        PublishBranchFreshnessOutcome::OperationalError { message } => {
            assert!(message.contains("Failed to resolve publish base ref 'missing-base'"));
        }
        other => panic!("expected operational error, got {other:?}"),
    }
}

fn automation_publish_fixture(
    base_ref: &str,
    kind: IdeationAnalysisBaseRefKind,
    automation: bool,
) -> (ChatConversation, AgentConversationWorkspace) {
    use crate::domain::entities::{AgentConversationWorkspaceMode, AutomationId, ProjectId};
    let project_id = ProjectId::from_string("project-b1".to_string());
    let mut conversation = ChatConversation::new_project(project_id.clone());
    if automation {
        conversation.automation_id = Some(AutomationId::from_string("automation-b1"));
    }
    let workspace = AgentConversationWorkspace::new(
        conversation.id.clone(),
        project_id,
        AgentConversationWorkspaceMode::Edit,
        kind,
        base_ref.to_string(),
        Some(base_ref.to_string()),
        Some("0".repeat(40)),
        "ralphx/ralphx/head-branch".to_string(),
        "/tmp/b1-worktree".to_string(),
    );
    (conversation, workspace)
}

#[tokio::test]
async fn ensure_publish_base_pushed_pushes_local_automation_base_when_origin_absent() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    setup_repo(&repo);
    let base = "ralphx/ralphx/automation-abc";
    git(&repo, &["branch", base]);
    let (conversation, workspace) =
        automation_publish_fixture(base, IdeationAnalysisBaseRefKind::LocalBranch, true);
    let github = Arc::new(crate::tests::mock_github_service::MockGithubService::new());
    let github_trait: Arc<dyn GithubServiceTrait> = github.clone();

    ensure_publish_base_pushed(&github_trait, &repo, &conversation, &workspace)
        .await
        .expect("base push succeeds");

    let state = github.state();
    assert_eq!(
        state.push_branch_calls, 1,
        "automation base should be pushed once"
    );
    assert_eq!(state.last_push_branch_name.as_deref(), Some(base));
}

#[tokio::test]
async fn ensure_publish_base_pushed_is_idempotent_when_origin_present() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    setup_repo(&repo);
    let base = "ralphx/ralphx/automation-present";
    git(&repo, &["branch", base]);
    // Seed the remote-tracking ref so origin/<base> already exists.
    git(
        &repo,
        &["update-ref", &format!("refs/remotes/origin/{base}"), "HEAD"],
    );
    let (conversation, workspace) =
        automation_publish_fixture(base, IdeationAnalysisBaseRefKind::LocalBranch, true);
    let github = Arc::new(crate::tests::mock_github_service::MockGithubService::new());
    let github_trait: Arc<dyn GithubServiceTrait> = github.clone();

    ensure_publish_base_pushed(&github_trait, &repo, &conversation, &workspace)
        .await
        .expect("idempotent skip succeeds");

    assert_eq!(
        github.state().push_branch_calls,
        0,
        "present origin base must not be re-pushed"
    );
}

#[tokio::test]
async fn ensure_publish_base_pushed_skips_non_automation_local_branch() {
    // Scope belt: a non-automation Edit workspace on a local-only branch must
    // NOT be pushed as a base branch even when origin/<base> is absent.
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    setup_repo(&repo);
    let base = "feature/local-only";
    git(&repo, &["branch", base]);
    let (conversation, workspace) =
        automation_publish_fixture(base, IdeationAnalysisBaseRefKind::LocalBranch, false);
    let github = Arc::new(crate::tests::mock_github_service::MockGithubService::new());
    let github_trait: Arc<dyn GithubServiceTrait> = github.clone();

    ensure_publish_base_pushed(&github_trait, &repo, &conversation, &workspace)
        .await
        .expect("no-op succeeds");

    assert_eq!(
        github.state().push_branch_calls,
        0,
        "non-automation base must not be pushed"
    );
}

#[tokio::test]
async fn ensure_publish_base_pushed_skips_project_default_base() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    setup_repo(&repo);
    let (conversation, workspace) =
        automation_publish_fixture("main", IdeationAnalysisBaseRefKind::ProjectDefault, true);
    let github = Arc::new(crate::tests::mock_github_service::MockGithubService::new());
    let github_trait: Arc<dyn GithubServiceTrait> = github.clone();

    ensure_publish_base_pushed(&github_trait, &repo, &conversation, &workspace)
        .await
        .expect("no-op succeeds");

    assert_eq!(github.state().push_branch_calls, 0);
}

#[tokio::test]
async fn ensure_publish_base_pushed_fails_closed_on_push_error() {
    // B5: a base-push failure surfaces as an error so the caller aborts the
    // publish — it must never silently retarget to main.
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    setup_repo(&repo);
    let base = "ralphx/ralphx/automation-fail";
    git(&repo, &["branch", base]);
    let github = Arc::new(crate::tests::mock_github_service::MockGithubService::new());
    github.state().push_branch_result = Some(Err(crate::error::AppError::Infrastructure(
        "push denied".to_string(),
    )));
    let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
    let (conversation, workspace) =
        automation_publish_fixture(base, IdeationAnalysisBaseRefKind::LocalBranch, true);

    let result = ensure_publish_base_pushed(&github_trait, &repo, &conversation, &workspace).await;

    assert!(result.is_err(), "push failure must surface as an error");
    assert_eq!(github.state().push_branch_calls, 1);
}

#[tokio::test]
async fn ensure_publish_base_pushed_skips_already_remote_pr_head_stacked_base() {
    // B6: a pr_head_stacked successor bases on the previous run's pushed pr_head
    // branch, which already lives on origin — no extra push.
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    setup_repo(&repo);
    let head_base = "ralphx/ralphx/task-run1-head";
    git(&repo, &["branch", head_base]);
    git(
        &repo,
        &[
            "update-ref",
            &format!("refs/remotes/origin/{head_base}"),
            "HEAD",
        ],
    );
    let (conversation, workspace) =
        automation_publish_fixture(head_base, IdeationAnalysisBaseRefKind::LocalBranch, true);
    let github = Arc::new(crate::tests::mock_github_service::MockGithubService::new());
    let github_trait: Arc<dyn GithubServiceTrait> = github.clone();

    ensure_publish_base_pushed(&github_trait, &repo, &conversation, &workspace)
        .await
        .expect("idempotent skip succeeds");

    assert_eq!(github.state().push_branch_calls, 0);
}

#[tokio::test]
async fn inspect_publish_branch_freshness_after_fetch_uses_existing_refs() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo = temp.path().join("repo");
    let main_sha = setup_repo(&repo);
    git(&repo, &["branch", "feature/current"]);

    let status = inspect_publish_branch_freshness_for_source_after_fetch(
        &repo,
        "main",
        "feature/current",
        Some("old-base"),
    )
    .await
    .expect("freshness should inspect without fetching");

    assert_eq!(status.target_ref, "main");
    assert_eq!(status.target_base_commit, main_sha.as_str());
    assert!(!status.is_base_ahead);
    assert_eq!(status.captured_base_commit, Some(main_sha));
}
use super::publish_resilience::*;
use crate::domain::entities::{
    AgentConversationWorkspace, ChatConversation, IdeationAnalysisBaseRefKind, Project,
};
use crate::domain::services::GithubServiceTrait;
use crate::domain::state_machine::transition_handler::PlanUpdateResult;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git command should spawn");
    assert!(
        output.status.success(),
        "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}
