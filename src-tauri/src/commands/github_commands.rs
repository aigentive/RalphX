// GitHub commands — read-only visibility surface over the locally-authenticated
// `gh` CLI. RalphX stores no GitHub token (Decision 1): connection status is a
// live reflection of `gh auth status` only.

use serde::Serialize;
use tauri::State;

use crate::application::AppState;
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
