use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::application::git_service::GitService;
use crate::domain::entities::{AgentConversationWorkspace, IdeationAnalysisBaseRefKind, Project};
use crate::domain::services::{GithubServiceTrait, PrStatus};
use crate::error::{AppError, AppResult};

pub const BLOCK_REASON_MISSING_BASE_COMMIT: &str =
    "Saved base branch is unavailable and the workspace is missing its captured base commit";
pub const BLOCK_REASON_NOT_CONTAINED: &str =
    "Saved base commit is not contained in the default branch";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BaseStatus {
    Valid,
    Retargeted,
    Blocked,
}

impl BaseStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            BaseStatus::Valid => "valid",
            BaseStatus::Retargeted => "retargeted",
            BaseStatus::Blocked => "blocked",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaseResolutionResult {
    pub status: BaseStatus,
    pub old_base_ref: String,
    pub effective_base_ref: Option<String>,
    pub effective_checkout_ref: Option<String>,
    pub effective_base_commit: Option<String>,
    pub display_name: Option<String>,
    pub block_reason: Option<String>,
    #[serde(skip)]
    pub merged_source_pull_request_number: Option<i64>,
}

impl BaseResolutionResult {
    pub fn valid(
        old_base_ref: String,
        checkout_ref: String,
        base_commit: String,
        display_name: Option<String>,
    ) -> Self {
        Self {
            status: BaseStatus::Valid,
            effective_base_ref: Some(old_base_ref.clone()),
            effective_checkout_ref: Some(checkout_ref),
            effective_base_commit: Some(base_commit),
            display_name,
            old_base_ref,
            block_reason: None,
            merged_source_pull_request_number: None,
        }
    }

    fn retargeted(
        old_base_ref: String,
        default_branch: String,
        checkout_ref: String,
        base_commit: String,
    ) -> Self {
        Self {
            status: BaseStatus::Retargeted,
            old_base_ref,
            effective_base_ref: Some(default_branch.clone()),
            effective_checkout_ref: Some(checkout_ref),
            effective_base_commit: Some(base_commit),
            display_name: Some(retarget_display_name(&default_branch)),
            block_reason: None,
            merged_source_pull_request_number: None,
        }
    }

    pub fn retargeted_merged_source_pull_request(
        old_base_ref: String,
        target_ref: String,
        checkout_ref: String,
        base_commit: String,
        pull_request_number: i64,
    ) -> Self {
        Self {
            status: BaseStatus::Retargeted,
            old_base_ref,
            effective_base_ref: Some(target_ref.clone()),
            effective_checkout_ref: Some(checkout_ref),
            effective_base_commit: Some(base_commit),
            display_name: Some(format!("{target_ref} (PR #{pull_request_number} merged)")),
            block_reason: None,
            merged_source_pull_request_number: Some(pull_request_number),
        }
    }

    fn blocked(old_base_ref: String, reason: impl Into<String>) -> Self {
        Self {
            status: BaseStatus::Blocked,
            old_base_ref,
            effective_base_ref: None,
            effective_checkout_ref: None,
            effective_base_commit: None,
            display_name: None,
            block_reason: Some(reason.into()),
            merged_source_pull_request_number: None,
        }
    }

    pub fn effective_base_ref(&self) -> AppResult<&str> {
        self.effective_base_ref.as_deref().ok_or_else(|| {
            AppError::Validation(
                self.block_reason
                    .clone()
                    .unwrap_or_else(|| "Agent workspace base is blocked".to_string()),
            )
        })
    }

    pub fn effective_checkout_ref(&self) -> AppResult<&str> {
        self.effective_checkout_ref.as_deref().ok_or_else(|| {
            AppError::Validation(
                self.block_reason
                    .clone()
                    .unwrap_or_else(|| "Agent workspace base is blocked".to_string()),
            )
        })
    }

    pub fn effective_base_commit(&self) -> AppResult<&str> {
        self.effective_base_commit.as_deref().ok_or_else(|| {
            AppError::Validation(
                self.block_reason
                    .clone()
                    .unwrap_or_else(|| "Agent workspace base is blocked".to_string()),
            )
        })
    }

    pub fn retargeted_from_merged_source_pull_request(&self) -> bool {
        self.merged_source_pull_request_number.is_some()
    }
}

/// Resolve the current state of a workspace's saved base ref.
pub async fn resolve_workspace_base(
    project: &Project,
    workspace: &AgentConversationWorkspace,
) -> AppResult<BaseResolutionResult> {
    resolve_workspace_base_with_github(project, workspace, None).await
}

/// Resolve the workspace base with an optional GitHub PR status reader.
///
/// Fresh callers with app state provide GitHub so a selected source PR that
/// has merged can be retargeted to its merge target. Missing or failed GitHub
/// reads intentionally preserve the ordinary local/remote base resolution.
pub async fn resolve_workspace_base_with_github(
    project: &Project,
    workspace: &AgentConversationWorkspace,
    github: Option<&dyn GithubServiceTrait>,
) -> AppResult<BaseResolutionResult> {
    resolve_workspace_base_with_fetch_mode(
        project,
        workspace,
        WorkspaceBaseFetchMode::Fresh,
        github,
    )
    .await
}

/// Resolve the workspace base using only local refs already present in the repo.
///
/// This is used on the hot path before spawning a continuation agent. It avoids
/// blocking first response on remote fetch latency; later publish/PR paths still
/// perform fresh remote checks through `resolve_workspace_base`.
pub async fn resolve_workspace_base_from_local_snapshot(
    project: &Project,
    workspace: &AgentConversationWorkspace,
) -> AppResult<BaseResolutionResult> {
    resolve_workspace_base_with_fetch_mode(
        project,
        workspace,
        WorkspaceBaseFetchMode::LocalOnly,
        None,
    )
    .await
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkspaceBaseFetchMode {
    Fresh,
    LocalOnly,
}

async fn resolve_workspace_base_with_fetch_mode(
    project: &Project,
    workspace: &AgentConversationWorkspace,
    fetch_mode: WorkspaceBaseFetchMode,
    github: Option<&dyn GithubServiceTrait>,
) -> AppResult<BaseResolutionResult> {
    let repo_path = PathBuf::from(&project.working_directory);
    if fetch_mode == WorkspaceBaseFetchMode::Fresh {
        GitService::fetch_origin(&repo_path).await?;
    }

    let has_origin = GitService::has_origin_remote(&repo_path).await;
    let old_base_ref = canonical_base_ref(&workspace.base_ref);
    if old_base_ref.is_empty() {
        return Ok(BaseResolutionResult::blocked(
            old_base_ref,
            "Agent conversation workspace base ref is empty",
        ));
    }

    if fetch_mode == WorkspaceBaseFetchMode::Fresh {
        if let (Some(source_pull_request), Some(github)) =
            (workspace.source_pull_request.as_ref(), github)
        {
            let source_head_ref = canonical_base_ref(&source_pull_request.head_ref_name);
            if source_head_ref == old_base_ref
                && matches!(
                    github
                        .check_pr_status(&repo_path, source_pull_request.number)
                        .await,
                    Ok(PrStatus::Merged { .. })
                )
            {
                let default_branch = canonical_base_ref(
                    &GitService::resolve_project_default_branch(
                        &repo_path,
                        project.base_branch.as_deref(),
                    )
                    .await,
                );
                let target_ref = source_pull_request
                    .base_ref_name
                    .as_deref()
                    .map(canonical_base_ref)
                    .filter(|base_ref| !base_ref.is_empty())
                    .unwrap_or(default_branch);
                if let Some(checkout_ref) =
                    existing_checkout_ref_for_base(&repo_path, &target_ref, has_origin).await?
                {
                    let base_commit = GitService::get_branch_sha(&repo_path, &checkout_ref).await?;
                    return Ok(BaseResolutionResult::retargeted_merged_source_pull_request(
                        old_base_ref,
                        target_ref,
                        checkout_ref,
                        base_commit,
                        source_pull_request.number,
                    ));
                }
            }
        }
    }

    if let Some(checkout_ref) =
        existing_checkout_ref_for_base(&repo_path, &old_base_ref, has_origin).await?
    {
        let base_commit = GitService::get_branch_sha(&repo_path, &checkout_ref).await?;
        return Ok(BaseResolutionResult::valid(
            old_base_ref,
            checkout_ref,
            base_commit,
            workspace.base_display_name.clone(),
        ));
    }

    let default_branch = canonical_base_ref(
        &GitService::resolve_project_default_branch(&repo_path, project.base_branch.as_deref())
            .await,
    );
    let Some(default_checkout_ref) =
        existing_default_checkout_ref(&repo_path, &default_branch, has_origin).await?
    else {
        return Ok(BaseResolutionResult::blocked(
            old_base_ref,
            format!("Project default branch '{default_branch}' cannot be resolved"),
        ));
    };

    let saved_base_commit = match workspace
        .base_commit
        .as_deref()
        .map(str::trim)
        .filter(|commit| !commit.is_empty())
    {
        Some(commit) => commit,
        None => {
            return Ok(BaseResolutionResult::blocked(
                old_base_ref,
                BLOCK_REASON_MISSING_BASE_COMMIT,
            ));
        }
    };

    if !is_commit_contained_in(&repo_path, saved_base_commit, &default_checkout_ref).await? {
        return Ok(BaseResolutionResult::blocked(
            old_base_ref,
            BLOCK_REASON_NOT_CONTAINED,
        ));
    }

    let default_commit = GitService::get_branch_sha(&repo_path, &default_checkout_ref).await?;
    Ok(BaseResolutionResult::retargeted(
        old_base_ref,
        default_branch,
        default_checkout_ref,
        default_commit,
    ))
}

/// Check whether `commit` is an ancestor of `target_ref`.
pub async fn is_commit_contained_in(
    project_path: &Path,
    commit: &str,
    target_ref: &str,
) -> AppResult<bool> {
    GitService::is_ancestor(project_path, commit, target_ref).await
}

pub fn retarget_display_name(default_branch: &str) -> String {
    format!("Project default ({default_branch})")
}

pub fn apply_workspace_base_resolution(
    workspace: &mut AgentConversationWorkspace,
    resolution: &BaseResolutionResult,
) -> AppResult<bool> {
    match resolution.status {
        BaseStatus::Valid => Ok(false),
        BaseStatus::Retargeted => {
            workspace.base_ref_kind = IdeationAnalysisBaseRefKind::ProjectDefault;
            workspace.base_ref = resolution.effective_base_ref()?.to_string();
            workspace.base_display_name = resolution.display_name.clone();
            workspace.base_commit = Some(resolution.effective_base_commit()?.to_string());
            if resolution.retargeted_from_merged_source_pull_request() {
                workspace.source_pull_request = None;
            }
            workspace.updated_at = Utc::now();
            Ok(true)
        }
        BaseStatus::Blocked => Err(AppError::Validation(
            resolution
                .block_reason
                .clone()
                .unwrap_or_else(|| "Agent workspace base is blocked".to_string()),
        )),
    }
}

fn canonical_base_ref(base_ref: &str) -> String {
    base_ref
        .trim()
        .strip_prefix("origin/")
        .unwrap_or_else(|| base_ref.trim())
        .trim()
        .to_string()
}

fn remote_tracking_ref(base_ref: &str) -> String {
    format!("origin/{base_ref}")
}

async fn existing_checkout_ref_for_base(
    repo_path: &Path,
    base_ref: &str,
    has_origin: bool,
) -> AppResult<Option<String>> {
    // Prefer the remote-tracking ref when an origin exists so published bases stay
    // authoritative. Fall back to a local branch of the same name before giving up:
    // automation base branches (and other isolated local-only bases) live only as
    // local worktree branches and are never pushed. Without this fallback the base
    // resolver treats a present local base as "missing" and silently retargets an
    // automation run's publish base to the project default, breaking chain_mode
    // (merged_base / pr_head_stacked). Mirrors `existing_default_checkout_ref`.
    if has_origin {
        let remote_ref = remote_tracking_ref(base_ref);
        if GitService::ref_exists(repo_path, &remote_ref).await? {
            return Ok(Some(remote_ref));
        }
    }

    if GitService::ref_exists(repo_path, base_ref).await? {
        return Ok(Some(base_ref.to_string()));
    }

    Ok(None)
}

async fn existing_default_checkout_ref(
    repo_path: &Path,
    default_branch: &str,
    has_origin: bool,
) -> AppResult<Option<String>> {
    if has_origin {
        let remote_ref = remote_tracking_ref(default_branch);
        if GitService::ref_exists(repo_path, &remote_ref).await? {
            return Ok(Some(remote_ref));
        }
    }

    if GitService::ref_exists(repo_path, default_branch).await? {
        return Ok(Some(default_branch.to_string()));
    }

    Ok(None)
}
