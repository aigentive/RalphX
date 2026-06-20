use crate::application::agent_issue_report::{
    configured_support_issue_repository_from_yaml, submit_agent_issue_report_with_service,
    validate_github_repository,
};
use crate::tests::mock_github_service::MockGithubService;

#[test]
fn configured_destination_prefers_support_issue_github_repository() {
    let yaml = r#"
support_issue:
  github_repository: aigentive/support
"#;

    assert_eq!(
        configured_support_issue_repository_from_yaml(yaml).as_deref(),
        Some("aigentive/support")
    );
}

#[test]
fn configured_destination_accepts_compat_issue_reporting_repository() {
    let yaml = r#"
issue_reporting:
  repository: enterprise/private-support
"#;

    assert_eq!(
        configured_support_issue_repository_from_yaml(yaml).as_deref(),
        Some("enterprise/private-support")
    );
}

#[test]
fn github_repository_validation_rejects_urls_and_nested_paths() {
    assert!(validate_github_repository("owner/repo").is_ok());
    assert!(validate_github_repository("https://github.com/owner/repo").is_err());
    assert!(validate_github_repository("owner/repo/extra").is_err());
}

#[tokio::test]
async fn submit_issue_report_uses_edited_markdown_body_exactly() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let github = MockGithubService::new();
    github.will_create_issue("https://github.com/aigentive/support/issues/42");

    let edited_body = "# Edited Report\n\nThe user removed one log line.";
    let body_dir = temp_dir.path().join("bodies");
    let issue_url = submit_agent_issue_report_with_service(
        &github,
        temp_dir.path(),
        &body_dir,
        "aigentive/support",
        "Support report",
        edited_body,
    )
    .await
    .expect("issue submit should succeed");

    assert_eq!(issue_url, "https://github.com/aigentive/support/issues/42");
    let state = github.state();
    assert_eq!(state.create_issue_calls, 1);
    assert_eq!(
        state
            .last_create_issue_args
            .as_ref()
            .map(|(repo, title, _)| (repo.as_str(), title.as_str())),
        Some(("aigentive/support", "Support report"))
    );
    assert_eq!(state.last_create_issue_body.as_deref(), Some(edited_body));
}
