use std::process::Command;
use std::sync::Arc;

use tauri::Manager;

use super::github_commands::{
    get_github_branch_overview, get_github_connection_status, get_pull_request_detail,
    GetGithubBranchOverviewInput, GetPullRequestDetailInput, GithubConnectionStatusResponse,
};
use crate::application::pull_request_detail::types::PullRequestDetailState;
use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationJiraIssueLink, AgentConversationWorkspace, AgentConversationWorkspaceMode,
    ChatConversation, IdeationAnalysisBaseRefKind, Project,
};
use crate::domain::services::github_service::{
    GithubConnectionDiagnostic, GithubConnectionState, GithubConnectionStatus, GithubServiceTrait,
    PrBranchMatch, PrSearchResult, PrStatus,
};
use crate::infrastructure::tool_paths::resolve_git_cli_path;
use crate::tests::mock_github_service::MockGithubService;
use crate::utils::path_safety::validate_absolute_non_root_path;

fn test_app(state: AppState) -> tauri::App<tauri::test::MockRuntime> {
    tauri::test::mock_builder()
        .manage(state)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app should build")
}

#[test]
fn github_connection_status_response_preserves_public_fields() {
    let response = GithubConnectionStatusResponse::from(GithubConnectionStatus {
        state: GithubConnectionState::Authenticated,
        diagnostic: None,
        gh_installed: true,
        authenticated: true,
        host: Some("github.com".to_string()),
        account: Some("reefagent".to_string()),
    });

    assert!(response.gh_installed);
    assert!(response.authenticated);
    assert_eq!(response.state, GithubConnectionState::Authenticated);
    assert!(response.diagnostic.is_none());
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
    assert_eq!(response.state, GithubConnectionState::CliUnavailable);
    assert_eq!(
        response.diagnostic,
        Some(GithubConnectionDiagnostic::CliLaunch)
    );
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

    assert!(unavailable.gh_installed);
    assert!(!unavailable.authenticated);
    assert_eq!(unavailable.state, GithubConnectionState::ProbeFailed);
    assert_eq!(
        unavailable.diagnostic,
        Some(GithubConnectionDiagnostic::ServiceFailure)
    );
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

#[tokio::test]
async fn get_github_branch_overview_lists_pr_rx_and_ticket_indicators() {
    let test_root = validate_absolute_non_root_path(
        &std::env::current_dir().expect("current checkout should be available"),
        "test checkout",
    )
    .expect("current checkout should be a safe path");
    let temp_dir = tempfile::Builder::new()
        .prefix("github-branch-overview-")
        .tempdir_in(test_root)
        .expect("tempdir should be created");
    let repo = validate_absolute_non_root_path(temp_dir.path(), "test git repository")
        .expect("temp repo should be a safe path");
    let readme_path =
        validate_absolute_non_root_path(&repo.join("README.md"), "test repository README")
            .expect("README path should be safe");
    Command::new(resolve_git_cli_path())
        .args(["init", "-b", "main"])
        .current_dir(&repo)
        .output()
        .expect("git init should run");
    Command::new(resolve_git_cli_path())
        .args(["config", "user.email", "test@example.com"])
        .current_dir(&repo)
        .output()
        .expect("git config should run");
    Command::new(resolve_git_cli_path())
        .args(["config", "user.name", "Test User"])
        .current_dir(&repo)
        .output()
        .expect("git config should run");
    std::fs::write(&readme_path, "base\n").expect("fixture should be written");
    Command::new(resolve_git_cli_path())
        .args(["add", "."])
        .current_dir(&repo)
        .output()
        .expect("git add should run");
    Command::new(resolve_git_cli_path())
        .args(["commit", "-m", "base"])
        .current_dir(&repo)
        .output()
        .expect("git commit should run");
    Command::new(resolve_git_cli_path())
        .args(["branch", "feature/alpha"])
        .current_dir(&repo)
        .output()
        .expect("git branch should run");
    Command::new(resolve_git_cli_path())
        .args(["branch", "feature/merged"])
        .current_dir(&repo)
        .output()
        .expect("git merged branch should run");
    Command::new(resolve_git_cli_path())
        .args(["branch", "ralphx/ticket/clickup-cu-1"])
        .current_dir(&repo)
        .output()
        .expect("git clickup ticket branch should run");
    Command::new(resolve_git_cli_path())
        .args(["branch", "ralphx/demo/agent-jira-PROJ-123-conversa"])
        .current_dir(&repo)
        .output()
        .expect("git jira agent ticket branch should run");

    let github = Arc::new(MockGithubService::new());
    github.will_return_pull_request_search(vec![
        PrSearchResult {
            number: 9,
            title: "Alpha PR".to_string(),
            url: "https://github.com/aigentive/ralphx.app/pull/9".to_string(),
            head_ref_name: "feature/alpha".to_string(),
            head_ref_oid: None,
            base_ref_name: "main".to_string(),
            is_draft: false,
            state: Some("OPEN".to_string()),
            merged_at: None,
            updated_at: Some("2026-06-28T08:00:00Z".to_string()),
            author_login: Some("reefagent".to_string()),
            assignee_logins: vec!["lazabogdan".to_string()],
            review_decision: Some("REVIEW_REQUIRED".to_string()),
            latest_review_author_logins: vec!["adriandemian".to_string()],
            review_request_logins: vec!["lazabogdan".to_string()],
            is_cross_repository: false,
        },
        PrSearchResult {
            number: 10,
            title: "Closed remote-only PR".to_string(),
            url: "https://github.com/aigentive/ralphx.app/pull/10".to_string(),
            head_ref_name: "feature/pr-only".to_string(),
            head_ref_oid: None,
            base_ref_name: "main".to_string(),
            is_draft: true,
            state: Some("CLOSED".to_string()),
            merged_at: None,
            updated_at: None,
            author_login: None,
            assignee_logins: Vec::new(),
            review_decision: None,
            latest_review_author_logins: Vec::new(),
            review_request_logins: Vec::new(),
            is_cross_repository: false,
        },
        PrSearchResult {
            number: 11,
            title: "Merged remote-only PR".to_string(),
            url: "https://github.com/aigentive/ralphx.app/pull/11".to_string(),
            head_ref_name: "feature/merged-pr-only".to_string(),
            head_ref_oid: None,
            base_ref_name: "main".to_string(),
            is_draft: false,
            state: Some("MERGED".to_string()),
            merged_at: Some("2026-06-27T08:00:00Z".to_string()),
            updated_at: None,
            author_login: None,
            assignee_logins: Vec::new(),
            review_decision: None,
            latest_review_author_logins: Vec::new(),
            review_request_logins: Vec::new(),
            is_cross_repository: false,
        },
    ]);
    github.will_return_pull_request_branch_matches(vec![
        PrBranchMatch {
            number: 7,
            url: "https://github.com/aigentive/ralphx.app/pull/7".to_string(),
            status: PrStatus::Closed,
            is_draft: false,
            head_ref_name: "feature/merged".to_string(),
            updated_at: Some("2026-06-26T08:00:00Z".to_string()),
            author_login: Some("olderauthor".to_string()),
        },
        PrBranchMatch {
            number: 11,
            url: "https://github.com/aigentive/ralphx.app/pull/11".to_string(),
            status: PrStatus::Merged {
                merge_commit_sha: Some("abc123".to_string()),
                merged_at: None,
            },
            is_draft: false,
            head_ref_name: "feature/merged".to_string(),
            updated_at: Some("2026-06-27T08:00:00Z".to_string()),
            author_login: Some("mergeauthor".to_string()),
        },
        PrBranchMatch {
            number: 14,
            url: "https://github.com/aigentive/ralphx.app/pull/14".to_string(),
            status: PrStatus::Closed,
            is_draft: false,
            head_ref_name: "feature/merged".to_string(),
            updated_at: Some("2026-06-27T08:00:00Z".to_string()),
            author_login: Some("closedauthor".to_string()),
        },
        PrBranchMatch {
            number: 12,
            url: "https://github.com/aigentive/ralphx.app/pull/12".to_string(),
            status: PrStatus::Open,
            is_draft: false,
            head_ref_name: "feature/not-local".to_string(),
            updated_at: Some("2026-06-28T08:00:00Z".to_string()),
            author_login: Some("nonlocalauthor".to_string()),
        },
        PrBranchMatch {
            number: 13,
            url: "https://github.com/aigentive/ralphx.app/pull/13".to_string(),
            status: PrStatus::Open,
            is_draft: false,
            head_ref_name: "main".to_string(),
            updated_at: Some("2026-06-28T09:00:00Z".to_string()),
            author_login: Some("mainauthor".to_string()),
        },
    ]);

    let mut state = AppState::new_test();
    state.github_service = Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>);
    let project = state
        .project_repo
        .create(Project::new(
            "Branch Overview".to_string(),
            repo.to_string_lossy().to_string(),
        ))
        .await
        .expect("project should seed");
    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.set_title("Alpha branch work");
    let conversation = state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("conversation should be created");
    let mut workspace = AgentConversationWorkspace::new(
        conversation.id,
        project.id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Current branch (main)".to_string()),
        None,
        "feature/alpha".to_string(),
        repo.join(".ralphx-test-worktree")
            .to_string_lossy()
            .to_string(),
    );
    workspace.publication_pr_number = Some(8);
    workspace.publication_pr_status = Some("closed".to_string());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should be created");
    state
        .agent_conversation_jira_issue_repo
        .upsert({
            let mut link = AgentConversationJiraIssueLink::new(
                conversation.id,
                project.id.clone(),
                "RX-77".to_string(),
                chrono::Utc::now(),
            );
            link.title = Some("Jira branch ticket".to_string());
            link.issue_url = Some("https://example.atlassian.net/browse/RX-77".to_string());
            link
        })
        .await
        .expect("jira link should be created");

    let app = test_app(state);
    let overview = get_github_branch_overview(
        GetGithubBranchOverviewInput {
            project_id: project.id.0,
        },
        app.state::<AppState>(),
    )
    .await
    .expect("overview should load");

    assert_eq!(overview.current_branch.as_deref(), Some("main"));
    assert!(overview.sources_unavailable.is_empty());
    assert_eq!(
        overview
            .branches
            .first()
            .map(|branch| branch.branch_name.as_str()),
        Some("main")
    );
    let alpha = overview
        .branches
        .iter()
        .find(|branch| branch.branch_name == "feature/alpha")
        .expect("feature branch row should exist");
    assert!(!alpha.is_current);
    assert_eq!(alpha.pr_number, Some(9));
    assert_eq!(alpha.pr_title.as_deref(), Some("Alpha PR"));
    assert_eq!(
        alpha.pr_url.as_deref(),
        Some("https://github.com/aigentive/ralphx.app/pull/9")
    );
    assert_eq!(alpha.pr_status.as_deref(), Some("open"));
    assert!(!alpha.pr_is_draft);
    assert_eq!(alpha.pr_updated_at.as_deref(), Some("2026-06-28T08:00:00Z"));
    assert_eq!(alpha.pr_author_login.as_deref(), Some("reefagent"));
    assert_eq!(alpha.pr_assignee_logins, vec!["lazabogdan"]);
    assert_eq!(alpha.pr_review_decision.as_deref(), Some("REVIEW_REQUIRED"));
    assert_eq!(alpha.pr_latest_review_author_logins, vec!["adriandemian"]);
    assert_eq!(alpha.pr_review_request_logins, vec!["lazabogdan"]);
    assert_eq!(alpha.pr_base_ref_name.as_deref(), Some("main"));
    assert_eq!(alpha.rx_conversation_count, 1);
    assert_eq!(alpha.rx_conversations.len(), 1);
    assert_eq!(
        alpha.rx_conversations[0].title.as_deref(),
        Some("Alpha branch work")
    );
    assert_eq!(
        alpha.rx_conversations[0].conversation_id,
        conversation.id.to_string()
    );
    assert_eq!(alpha.ticket_count, 1);
    assert_eq!(alpha.ticket_labels, vec!["Jira RX-77"]);
    assert_eq!(alpha.ticket_links.len(), 1);
    assert_eq!(alpha.ticket_links[0].provider, "jira");
    assert_eq!(
        alpha.ticket_links[0].url.as_deref(),
        Some("https://example.atlassian.net/browse/RX-77")
    );

    assert!(overview.branches.iter().all(|branch| {
        branch.branch_name != "feature/pr-only" && branch.branch_name != "feature/merged-pr-only"
    }));

    let merged = overview
        .branches
        .iter()
        .find(|branch| branch.branch_name == "feature/merged")
        .expect("merged local PR branch row should exist");
    assert_eq!(merged.pr_number, Some(14));
    assert_eq!(merged.pr_status.as_deref(), Some("closed"));
    assert_eq!(
        merged.pr_url.as_deref(),
        Some("https://github.com/aigentive/ralphx.app/pull/14")
    );
    assert_eq!(merged.pr_title, None);
    assert_eq!(merged.pr_author_login.as_deref(), Some("closedauthor"));
    assert_eq!(merged.pr_base_ref_name, None);
    assert!(overview
        .branches
        .iter()
        .all(|branch| branch.branch_name != "feature/not-local"));

    let main = overview
        .branches
        .iter()
        .find(|branch| branch.branch_name == "main")
        .expect("main branch row should exist");
    assert!(main.is_current);
    assert_eq!(main.pr_number, None);
    assert_eq!(main.rx_conversation_count, 0);
    assert_eq!(main.rx_conversations.len(), 0);
    assert_eq!(main.ticket_count, 0);
    assert_eq!(main.ticket_links.len(), 0);

    let clickup = overview
        .branches
        .iter()
        .find(|branch| branch.branch_name == "ralphx/ticket/clickup-cu-1")
        .expect("ClickUp canonical ticket branch row should exist");
    assert!(!clickup.is_current);
    assert_eq!(clickup.ticket_count, 1);
    assert_eq!(clickup.ticket_labels, vec!["ClickUp cu-1"]);
    assert_eq!(clickup.ticket_links[0].provider, "clickup");
    assert_eq!(clickup.ticket_links[0].title, None);
    assert!(clickup.ticket_links[0].url.is_none());

    let jira_agent_branch = overview
        .branches
        .iter()
        .find(|branch| branch.branch_name == "ralphx/demo/agent-jira-PROJ-123-conversa")
        .expect("Jira agent ticket branch row should exist");
    assert_eq!(jira_agent_branch.ticket_count, 1);
    assert_eq!(jira_agent_branch.ticket_labels, vec!["Jira PROJ-123"]);
    assert_eq!(jira_agent_branch.ticket_links[0].provider, "jira");
    assert_eq!(jira_agent_branch.ticket_links[0].label, "PROJ-123");
    assert!(jira_agent_branch.ticket_links[0].url.is_none());

    assert_eq!(
        github.state().last_search_pull_requests_args,
        Some((None, 50))
    );
    assert_eq!(github.state().list_pull_request_branch_matches_calls, 1);
    assert_eq!(
        github.state().last_list_pull_request_branch_matches_limit,
        Some(200)
    );
    assert_eq!(github.state().find_latest_pr_by_head_branch_calls, 0);
}
