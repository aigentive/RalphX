use std::sync::Arc;

use tauri::Manager;

use super::github_commands::{
    get_github_connection_status, get_pull_request_detail, GetPullRequestDetailInput,
    GithubConnectionStatusResponse,
};
use crate::application::pull_request_detail::types::PullRequestDetailState;
use crate::application::AppState;
use crate::domain::entities::Project;
use crate::domain::services::github_service::{GithubConnectionStatus, GithubServiceTrait};
use crate::tests::mock_github_service::MockGithubService;

fn test_app(state: AppState) -> tauri::App<tauri::test::MockRuntime> {
    tauri::test::mock_builder()
        .manage(state)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app should build")
}

#[test]
fn github_connection_status_response_preserves_public_fields() {
    let response = GithubConnectionStatusResponse::from(GithubConnectionStatus {
        gh_installed: true,
        authenticated: true,
        host: Some("github.com".to_string()),
        account: Some("reefagent".to_string()),
    });

    assert!(response.gh_installed);
    assert!(response.authenticated);
    assert_eq!(response.host.as_deref(), Some("github.com"));
    assert_eq!(response.account.as_deref(), Some("reefagent"));
}

#[tokio::test]
async fn get_github_connection_status_returns_unavailable_without_service() {
    let app = test_app(AppState::new_test());

    let response = get_github_connection_status(app.state::<AppState>())
        .await
        .expect("command should not fail");

    assert!(!response.gh_installed);
    assert!(!response.authenticated);
    assert!(response.host.is_none());
    assert!(response.account.is_none());
}

#[tokio::test]
async fn get_github_connection_status_uses_service_and_falls_back_on_error() {
    let github = Arc::new(MockGithubService::new());
    github.will_be_authenticated("github.com", "reefagent");
    let mut state = AppState::new_test();
    state.github_service = Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>);
    let app = test_app(state);

    let response = get_github_connection_status(app.state::<AppState>())
        .await
        .expect("authenticated status should load");

    assert!(response.gh_installed);
    assert!(response.authenticated);
    assert_eq!(response.host.as_deref(), Some("github.com"));
    assert_eq!(response.account.as_deref(), Some("reefagent"));
    assert_eq!(github.state().fetch_github_connection_status_calls, 1);

    {
        let mut github_state = github.state();
        github_state.fetch_github_connection_status_result = Some(Err(
            crate::error::AppError::Infrastructure("gh auth status failed".to_string()),
        ));
    }

    let unavailable = get_github_connection_status(app.state::<AppState>())
        .await
        .expect("command should collapse service errors");

    assert!(!unavailable.gh_installed);
    assert!(!unavailable.authenticated);
    assert_eq!(github.state().fetch_github_connection_status_calls, 2);
}

#[tokio::test]
async fn get_pull_request_detail_builds_deps_and_returns_typed_payload() {
    let state = AppState::new_test();
    let project = state
        .project_repo
        .create(Project::new(
            "GitHub Commands".to_string(),
            "/tmp/ralphx-github-command-test".to_string(),
        ))
        .await
        .expect("project should seed");
    let app = test_app(state);

    let detail = get_pull_request_detail(
        GetPullRequestDetailInput {
            project_id: project.id.0,
            pr_number: Some(42),
            branch: Some("ignored-when-number-present".to_string()),
        },
        app.state::<AppState>(),
    )
    .await
    .expect("command should not fail");

    assert_eq!(detail.state, PullRequestDetailState::RepoUnresolvable);
}
