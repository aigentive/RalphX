use std::path::Path;
use std::sync::Arc;

use crate::domain::services::GithubServiceTrait;
use crate::domain::state_machine::transition_handler::{
    classify_commit_hook_failure_text, update_plan_from_main_isolated, update_source_from_target,
    CommitHookFailureKind, PlanUpdateResult, SourceUpdateResult,
};
use crate::error::AppResult;
use crate::{application::GitService, domain::entities::Project};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublishFailureClass {
    AgentFixable,
    Operational,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublishBranchFreshnessOutcome {
    AlreadyFresh {
        base_commit: String,
        target_ref: String,
    },
    Updated {
        base_commit: String,
        target_ref: String,
    },
    NeedsAgent {
        message: String,
        conflict_files: Vec<String>,
        base_commit: String,
        target_ref: String,
    },
    OperationalError {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishBranchFreshnessStatus {
    pub target_ref: String,
    pub captured_base_commit: Option<String>,
    pub target_base_commit: String,
    pub is_base_ahead: bool,
}

pub fn classify_publish_failure(error: &str) -> PublishFailureClass {
    let normalized = error.to_lowercase();

    if is_operational_failure(&normalized) {
        return PublishFailureClass::Operational;
    }

    if is_commit_hook_failure_context(&normalized) {
        match classify_commit_hook_failure_text(error) {
            CommitHookFailureKind::PolicyFailure => return PublishFailureClass::AgentFixable,
            CommitHookFailureKind::EnvironmentFailure => return PublishFailureClass::Operational,
            CommitHookFailureKind::Unknown => {}
        }
    }

    if is_agent_fixable_failure(&normalized) {
        return PublishFailureClass::AgentFixable;
    }

    PublishFailureClass::Operational
}

pub fn publish_push_status_for_failure(error: &str) -> &'static str {
    match classify_publish_failure(error) {
        PublishFailureClass::AgentFixable => "needs_agent",
        PublishFailureClass::Operational => "failed",
    }
}

pub fn review_base_for_publish<'a>(
    captured_base_commit: Option<&'a str>,
    base_ref: &str,
) -> Result<&'a str, String> {
    captured_base_commit
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            format!(
                "Agent conversation workspace is missing its captured base commit for base ref '{}'",
                base_ref
            )
        })
}

pub async fn count_publish_reviewable_commits(
    repo_path: &Path,
    source_branch: &str,
    review_base: &str,
) -> AppResult<u32> {
    GitService::count_commits_not_on_branch(repo_path, source_branch, review_base).await
}

pub async fn count_existing_publish_branch_reviewable_commits(
    repo_path: &Path,
    source_branch: &str,
    review_base: &str,
) -> AppResult<u32> {
    if !GitService::branch_exists(repo_path, source_branch)
        .await
        .unwrap_or(false)
    {
        return Ok(0);
    }

    count_publish_reviewable_commits(repo_path, source_branch, review_base).await
}

pub async fn count_unpublished_publish_commits(
    repo_path: &Path,
    source_branch: &str,
) -> AppResult<Option<u32>> {
    let remote_ref = remote_tracking_ref_for_publish(source_branch);
    if !GitService::ref_exists(repo_path, &remote_ref).await? {
        return Ok(None);
    }

    count_publish_reviewable_commits(repo_path, source_branch, &remote_ref)
        .await
        .map(Some)
}

pub async fn push_publish_branch(
    github: &Arc<dyn GithubServiceTrait>,
    repo_path: &Path,
    branch: &str,
) -> AppResult<()> {
    github.push_branch(repo_path, branch).await
}

pub async fn ensure_publish_branch_fresh(
    repo_path: &Path,
    project: &Project,
    source_branch: &str,
    base_ref: &str,
    conversation_id: &str,
    app_handle: Option<&tauri::AppHandle>,
) -> PublishBranchFreshnessOutcome {
    if let Err(error) = GitService::fetch_origin(repo_path).await {
        return PublishBranchFreshnessOutcome::OperationalError {
            message: format!("Failed to refresh git remotes before publishing: {error}"),
        };
    }

    let target_ref = resolve_publish_freshness_target(repo_path, base_ref).await;
    let target_sha = match GitService::get_branch_sha(repo_path, &target_ref).await {
        Ok(sha) => sha,
        Err(error) => {
            return PublishBranchFreshnessOutcome::OperationalError {
                message: format!(
                    "Failed to resolve publish base ref '{}' before publishing: {}",
                    target_ref, error
                ),
            };
        }
    };

    let result = update_source_from_target(
        repo_path,
        source_branch,
        &target_ref,
        project,
        conversation_id,
        app_handle,
    )
    .await;

    publish_branch_freshness_outcome_from_source_update(result, &target_ref, &target_sha)
}

pub async fn ensure_plan_publish_branch_fresh(
    repo_path: &Path,
    project: &Project,
    plan_branch: &str,
    base_ref: &str,
    conversation_id: &str,
    app_handle: Option<&tauri::AppHandle>,
) -> PublishBranchFreshnessOutcome {
    if let Err(error) = GitService::fetch_origin(repo_path).await {
        return PublishBranchFreshnessOutcome::OperationalError {
            message: format!("Failed to refresh git remotes before publishing: {error}"),
        };
    }

    let target_ref = resolve_publish_freshness_target(repo_path, base_ref).await;
    let target_sha = match GitService::get_branch_sha(repo_path, &target_ref).await {
        Ok(sha) => sha,
        Err(error) => {
            return PublishBranchFreshnessOutcome::OperationalError {
                message: format!(
                    "Failed to resolve publish base ref '{}' before publishing: {}",
                    target_ref, error
                ),
            };
        }
    };

    let result = update_plan_from_main_isolated(
        repo_path,
        plan_branch,
        &target_ref,
        project,
        conversation_id,
        app_handle,
    )
    .await;

    publish_branch_freshness_outcome_from_plan_update(result, &target_ref, &target_sha)
}

pub async fn inspect_publish_branch_freshness(
    repo_path: &Path,
    base_ref: &str,
    captured_base_commit: Option<&str>,
) -> AppResult<PublishBranchFreshnessStatus> {
    GitService::fetch_origin(repo_path).await?;
    let target_ref = resolve_publish_freshness_target(repo_path, base_ref).await;
    let target_sha = GitService::get_branch_sha(repo_path, &target_ref).await?;

    Ok(publish_branch_freshness_status_from_commits(
        captured_base_commit,
        &target_ref,
        &target_sha,
    ))
}

pub async fn inspect_publish_branch_freshness_for_source(
    repo_path: &Path,
    base_ref: &str,
    source_branch: &str,
    captured_base_commit: Option<&str>,
) -> AppResult<PublishBranchFreshnessStatus> {
    inspect_publish_branch_freshness_for_source_with_fetch(
        repo_path,
        base_ref,
        source_branch,
        captured_base_commit,
        true,
    )
    .await
}

pub async fn inspect_publish_branch_freshness_for_source_after_fetch(
    repo_path: &Path,
    base_ref: &str,
    source_branch: &str,
    captured_base_commit: Option<&str>,
) -> AppResult<PublishBranchFreshnessStatus> {
    inspect_publish_branch_freshness_for_source_with_fetch(
        repo_path,
        base_ref,
        source_branch,
        captured_base_commit,
        false,
    )
    .await
}

async fn inspect_publish_branch_freshness_for_source_with_fetch(
    repo_path: &Path,
    base_ref: &str,
    source_branch: &str,
    captured_base_commit: Option<&str>,
    should_fetch: bool,
) -> AppResult<PublishBranchFreshnessStatus> {
    if should_fetch {
        GitService::fetch_origin(repo_path).await?;
    }
    let target_ref = resolve_publish_freshness_target(repo_path, base_ref).await;
    let target_sha = GitService::get_branch_sha(repo_path, &target_ref).await?;
    let source_contains_target =
        GitService::is_ancestor(repo_path, &target_sha, source_branch).await?;

    Ok(publish_branch_freshness_status_from_commits_and_branch(
        captured_base_commit,
        &target_ref,
        &target_sha,
        source_contains_target,
    ))
}

pub fn publish_branch_freshness_status_from_commits(
    captured_base_commit: Option<&str>,
    target_ref: &str,
    target_base_commit: &str,
) -> PublishBranchFreshnessStatus {
    let captured_base_commit = captured_base_commit
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let is_base_ahead = captured_base_commit
        .as_deref()
        .map(|captured| captured != target_base_commit)
        .unwrap_or(false);

    PublishBranchFreshnessStatus {
        target_ref: target_ref.to_string(),
        captured_base_commit,
        target_base_commit: target_base_commit.to_string(),
        is_base_ahead,
    }
}

pub fn publish_branch_freshness_status_from_commits_and_branch(
    captured_base_commit: Option<&str>,
    target_ref: &str,
    target_base_commit: &str,
    source_contains_target_base: bool,
) -> PublishBranchFreshnessStatus {
    if source_contains_target_base {
        return PublishBranchFreshnessStatus {
            target_ref: target_ref.to_string(),
            captured_base_commit: Some(target_base_commit.to_string()),
            target_base_commit: target_base_commit.to_string(),
            is_base_ahead: false,
        };
    }

    publish_branch_freshness_status_from_commits(
        captured_base_commit,
        target_ref,
        target_base_commit,
    )
}

pub struct AgentWorkspaceRepairCompletionCheck<'a> {
    pub freshness_status: &'a PublishBranchFreshnessStatus,
    pub workspace_base_ref: &'a str,
    pub resolved_base_ref: &'a str,
    pub resolved_base_commit: &'a str,
    pub repair_commit_sha: &'a str,
    pub workspace_head_sha: &'a str,
    pub has_uncommitted_changes: bool,
    pub is_merge_in_progress: bool,
    pub is_rebase_in_progress: bool,
    pub has_conflict_markers: bool,
}

pub fn verify_agent_workspace_repair_completion(
    check: AgentWorkspaceRepairCompletionCheck<'_>,
) -> Result<(), String> {
    let target_ref = check.freshness_status.target_ref.as_str();
    if check.resolved_base_ref != check.workspace_base_ref && check.resolved_base_ref != target_ref
    {
        return Err(format!(
            "resolved_base_ref '{}' does not match workspace base '{}' or target '{}'",
            check.resolved_base_ref, check.workspace_base_ref, target_ref
        ));
    }

    if check.resolved_base_commit != check.freshness_status.target_base_commit {
        return Err(format!(
            "resolved_base_commit '{}' does not match current target base '{}'",
            check.resolved_base_commit, check.freshness_status.target_base_commit
        ));
    }

    if check.freshness_status.is_base_ahead {
        return Err(format!(
            "workspace branch is still behind {} at {}",
            check.freshness_status.target_ref, check.freshness_status.target_base_commit
        ));
    }

    if check.workspace_head_sha != check.repair_commit_sha {
        return Err(format!(
            "repair_commit_sha '{}' is not the current workspace HEAD '{}'",
            check.repair_commit_sha, check.workspace_head_sha
        ));
    }

    if check.has_uncommitted_changes {
        return Err("workspace has uncommitted changes".to_string());
    }

    if check.is_merge_in_progress {
        return Err("workspace merge is still in progress".to_string());
    }

    if check.is_rebase_in_progress {
        return Err("workspace rebase is still in progress".to_string());
    }

    if check.has_conflict_markers {
        return Err("workspace still contains conflict markers".to_string());
    }

    Ok(())
}

pub(crate) fn publish_branch_freshness_outcome_from_source_update(
    result: SourceUpdateResult,
    target_ref: &str,
    target_sha: &str,
) -> PublishBranchFreshnessOutcome {
    match result {
        SourceUpdateResult::AlreadyUpToDate => PublishBranchFreshnessOutcome::AlreadyFresh {
            base_commit: target_sha.to_string(),
            target_ref: target_ref.to_string(),
        },
        SourceUpdateResult::Updated => PublishBranchFreshnessOutcome::Updated {
            base_commit: target_sha.to_string(),
            target_ref: target_ref.to_string(),
        },
        SourceUpdateResult::Conflicts { conflict_files } => {
            let conflict_files = conflict_files
                .into_iter()
                .map(|path| path.to_string_lossy().to_string())
                .collect::<Vec<_>>();
            let files_label = if conflict_files.is_empty() {
                "unknown files".to_string()
            } else {
                conflict_files.join(", ")
            };
            PublishBranchFreshnessOutcome::NeedsAgent {
                message: format!(
                    "Merge conflict updating agent workspace branch from {target_ref}: {files_label}"
                ),
                conflict_files,
                base_commit: target_sha.to_string(),
                target_ref: target_ref.to_string(),
            }
        }
        SourceUpdateResult::Error(message) => {
            PublishBranchFreshnessOutcome::OperationalError { message }
        }
    }
}

pub(crate) fn publish_branch_freshness_outcome_from_plan_update(
    result: PlanUpdateResult,
    target_ref: &str,
    target_sha: &str,
) -> PublishBranchFreshnessOutcome {
    match result {
        PlanUpdateResult::AlreadyUpToDate | PlanUpdateResult::NotPlanBranch => {
            PublishBranchFreshnessOutcome::AlreadyFresh {
                base_commit: target_sha.to_string(),
                target_ref: target_ref.to_string(),
            }
        }
        PlanUpdateResult::Updated => PublishBranchFreshnessOutcome::Updated {
            base_commit: target_sha.to_string(),
            target_ref: target_ref.to_string(),
        },
        PlanUpdateResult::Conflicts { conflict_files } => {
            let conflict_files = conflict_files
                .into_iter()
                .map(|path| path.to_string_lossy().to_string())
                .collect::<Vec<_>>();
            let files_label = if conflict_files.is_empty() {
                "unknown files".to_string()
            } else {
                conflict_files.join(", ")
            };
            PublishBranchFreshnessOutcome::NeedsAgent {
                message: format!(
                    "Merge conflict updating plan branch from {target_ref}: {files_label}"
                ),
                conflict_files,
                base_commit: target_sha.to_string(),
                target_ref: target_ref.to_string(),
            }
        }
        PlanUpdateResult::Error(message) => {
            PublishBranchFreshnessOutcome::OperationalError { message }
        }
    }
}

pub fn remote_tracking_ref_for_publish(base_ref: &str) -> String {
    if base_ref.starts_with("origin/") {
        base_ref.to_string()
    } else {
        format!("origin/{base_ref}")
    }
}

async fn resolve_publish_freshness_target(repo_path: &Path, base_ref: &str) -> String {
    let remote_ref = remote_tracking_ref_for_publish(base_ref);
    if remote_ref != base_ref
        && GitService::ref_exists(repo_path, &remote_ref)
            .await
            .unwrap_or(false)
    {
        remote_ref
    } else {
        base_ref.to_string()
    }
}

fn is_agent_fixable_failure(normalized: &str) -> bool {
    const PATTERNS: &[&str] = &[
        "conflict",
        "unmerged paths",
        "<<<<<<<",
        "pre-commit",
        "precommit",
        "typecheck",
        "tsc",
        "clippy",
        "lint",
        "test failed",
        "tests failed",
        "non-fast-forward",
        "failed to push some refs",
        "updates were rejected",
        "fetch first",
    ];

    PATTERNS.iter().any(|pattern| normalized.contains(pattern))
}

fn is_commit_hook_failure_context(normalized: &str) -> bool {
    const PATTERNS: &[&str] = &[
        "pre-commit",
        "precommit",
        "[pre-commit]",
        "commit-msg",
        "prepare-commit-msg",
        "husky",
        "hook declined",
    ];

    PATTERNS.iter().any(|pattern| normalized.contains(pattern))
}

fn is_operational_failure(normalized: &str) -> bool {
    const PATTERNS: &[&str] = &[
        "github integration is not available",
        "git authentication",
        "workspace not found",
        "conversation not found",
        "project not found",
        "authentication",
        "authorization",
        "permission denied",
        "cannot find package",
        "could not resolve",
    ];

    PATTERNS.iter().any(|pattern| normalized.contains(pattern))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::process::Command;

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
                message: "Merge conflict updating plan branch from main: unknown files"
                    .to_string(),
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
}
