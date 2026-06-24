// GitHub commands — read-only visibility surface over the locally-authenticated
// `gh` CLI. RalphX stores no GitHub token (Decision 1): connection status is a
// live reflection of `gh auth status` only.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::application::pull_request_detail::types::PullRequestDetail;
use crate::application::pull_request_detail::{
    load_pull_request_detail, PullRequestDetailDeps, PullRequestDetailRequest,
};
use crate::application::AppState;
use crate::domain::entities::ProjectId;
use crate::domain::services::github_service::GithubConnectionStatus;

/// Tauri DTO for GitHub connection status (camelCase for the frontend).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubConnectionStatusResponse {
    pub gh_installed: bool,
    pub authenticated: bool,
    pub host: Option<String>,
    pub account: Option<String>,
}

impl From<GithubConnectionStatus> for GithubConnectionStatusResponse {
    fn from(status: GithubConnectionStatus) -> Self {
        Self {
            gh_installed: status.gh_installed,
            authenticated: status.authenticated,
            host: status.host,
            account: status.account,
        }
    }
}

/// Report whether `gh` is installed and authenticated, plus the active host/account.
///
/// Never panics or returns an `Err`: a missing GitHub service, an absent/unauthenticated
/// `gh`, or any underlying failure all collapse to a typed "unavailable" status so the
/// UI can render distinct not-installed / not-authenticated / connected states.
#[tauri::command]
pub async fn get_github_connection_status(
    state: State<'_, AppState>,
) -> Result<GithubConnectionStatusResponse, String> {
    let Some(service) = state.github_service.as_ref() else {
        return Ok(GithubConnectionStatus::unavailable().into());
    };

    let status = service
        .fetch_github_connection_status()
        .await
        .unwrap_or_else(|_| GithubConnectionStatus::unavailable());

    Ok(status.into())
}

/// Tauri input for `get_pull_request_detail`. Provide either `prNumber` or
/// `branch` (number takes precedence when both are present).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetPullRequestDetailInput {
    pub project_id: String,
    pub pr_number: Option<i64>,
    pub branch: Option<String>,
}

/// Return the full PR-detail graph (description + checks/review + comments +
/// live review thread + attached RalphX conversations) for a `(project, pr)` or
/// `(project, branch)` selector.
///
/// Never panics or returns `Err`: every failure collapses into a typed
/// `state` on the payload (no-PR / gh-unauthenticated / repo-unresolvable /
/// fetch-timeout / rate-limited), so the UI renders distinct empty/error
/// surfaces. Repo is resolved from the project's `working_directory` (Decision 5).
#[tauri::command]
pub async fn get_pull_request_detail(
    input: GetPullRequestDetailInput,
    state: State<'_, AppState>,
) -> Result<PullRequestDetail, String> {
    let deps = PullRequestDetailDeps {
        github_service: state.github_service.clone(),
        project_repo: Arc::clone(&state.project_repo),
        workspace_repo: Arc::clone(&state.agent_conversation_workspace_repo),
        plan_branch_repo: Arc::clone(&state.plan_branch_repo),
    };
    let request = PullRequestDetailRequest {
        project_id: ProjectId::from_string(input.project_id),
        pr_number: input.pr_number,
        branch: input.branch,
    };

    Ok(load_pull_request_detail(deps, request).await)
}
