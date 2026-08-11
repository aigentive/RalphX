use super::*;
use async_trait::async_trait;

struct ReadFailingAppStateRepository;

#[async_trait]
impl AppStateRepository for ReadFailingAppStateRepository {
    async fn get(
        &self,
    ) -> Result<crate::domain::entities::app_state::AppSettings, Box<dyn std::error::Error>> {
        Err(std::io::Error::other("injected app state read failure").into())
    }

    async fn set_active_project(
        &self,
        _project_id: Option<&ProjectId>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    async fn set_execution_halt_mode(
        &self,
        _halt_mode: crate::domain::entities::app_state::ExecutionHaltMode,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    async fn set_update_channel(
        &self,
        _update_channel: crate::domain::entities::app_state::UpdateChannel,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    async fn set_last_seen_release_notes_version(
        &self,
        _version: Option<&str>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    async fn set_remove_inherited_github_cli_tokens(
        &self,
        _enabled: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
}

struct ReadFailingProjectRepository;

#[async_trait]
impl ProjectRepository for ReadFailingProjectRepository {
    async fn create(&self, _project: Project) -> crate::error::AppResult<Project> {
        Err(crate::error::AppError::Database(
            "injected project write failure".to_string(),
        ))
    }

    async fn get_by_id(&self, _id: &ProjectId) -> crate::error::AppResult<Option<Project>> {
        Err(crate::error::AppError::Database(
            "injected project read failure".to_string(),
        ))
    }

    async fn get_all(&self) -> crate::error::AppResult<Vec<Project>> {
        Err(crate::error::AppError::Database(
            "injected project list failure".to_string(),
        ))
    }

    async fn update(&self, _project: &Project) -> crate::error::AppResult<()> {
        Err(crate::error::AppError::Database(
            "injected project write failure".to_string(),
        ))
    }

    async fn delete(&self, _id: &ProjectId) -> crate::error::AppResult<()> {
        Err(crate::error::AppError::Database(
            "injected project write failure".to_string(),
        ))
    }

    async fn get_by_working_directory(
        &self,
        _path: &str,
    ) -> crate::error::AppResult<Option<Project>> {
        Err(crate::error::AppError::Database(
            "injected project read failure".to_string(),
        ))
    }

    async fn archive(&self, _id: &ProjectId) -> crate::error::AppResult<Project> {
        Err(crate::error::AppError::Database(
            "injected project write failure".to_string(),
        ))
    }
}

fn project(github_pr_enabled: bool) -> Project {
    let mut project = Project::new("RalphX".to_string(), "/repo".to_string());
    project.id = ProjectId::from_string("project-1".to_string());
    project.github_pr_enabled = github_pr_enabled;
    project
}

fn gh_state(authenticated: bool) -> GithubConnectionState {
    if authenticated {
        GithubConnectionState::Authenticated
    } else {
        GithubConnectionState::Unauthenticated
    }
}

#[test]
fn mixed_https_fetch_ssh_push_blocks_startup_git_work() {
    let issue = evaluate_project_git_auth_issue(
        &project(true),
        true,
        gh_state(true),
        Ok(GitRemoteAuthConfig {
            fetch_url: Some("https://github.com/owner/repo.git".to_string()),
            push_url: Some("git@github.com:owner/repo.git".to_string()),
            github_https_credential_helper_configured: false,
        }),
    )
    .expect("mixed auth modes should block");

    assert!(issue.active_project);
    assert!(issue.mixed_auth_modes);
    assert_eq!(issue.fetch_kind.as_deref(), Some("HTTPS"));
    assert_eq!(issue.push_kind.as_deref(), Some("SSH"));
    assert!(issue.can_switch_to_ssh);
    assert!(issue
        .reasons
        .iter()
        .any(|reason| reason.contains("different auth modes")));
}

#[test]
fn github_pr_mode_blocks_when_gh_is_not_authenticated() {
    let issue = evaluate_project_git_auth_issue(
        &project(true),
        false,
        gh_state(false),
        Ok(GitRemoteAuthConfig {
            fetch_url: Some("git@github.com:owner/repo.git".to_string()),
            push_url: Some("git@github.com:owner/repo.git".to_string()),
            github_https_credential_helper_configured: false,
        }),
    )
    .expect("gh auth should be required for PR mode");

    assert!(issue.github_pr_enabled);
    assert!(issue
        .reasons
        .iter()
        .any(|reason| reason.contains("needs GitHub CLI authentication")));
    assert_eq!(issue.issue_kind, "auth_blocked");
}

#[test]
fn github_https_origin_blocks_background_git_when_gh_is_missing() {
    let issue = evaluate_project_git_auth_issue(
        &project(false),
        true,
        gh_state(false),
        Ok(GitRemoteAuthConfig {
            fetch_url: Some("https://github.com/owner/repo.git".to_string()),
            push_url: Some("https://github.com/owner/repo.git".to_string()),
            github_https_credential_helper_configured: false,
        }),
    )
    .expect("GitHub HTTPS origin should require non-interactive credentials");

    assert_eq!(issue.issue_kind, "auth_blocked");
    assert_eq!(issue.fetch_kind.as_deref(), Some("HTTPS"));
    assert_eq!(issue.push_kind.as_deref(), Some("HTTPS"));
    assert!(issue.can_switch_to_ssh);
    assert_eq!(
        issue.suggested_ssh_url.as_deref(),
        Some("git@github.com:owner/repo.git")
    );
    assert!(issue
        .reasons
        .iter()
        .any(|reason| reason.contains("non-interactive credential")));
}

#[test]
fn github_https_origin_blocks_when_helper_missing_even_if_gh_is_authenticated() {
    let issue = evaluate_project_git_auth_issue(
        &project(false),
        true,
        gh_state(true),
        Ok(GitRemoteAuthConfig {
            fetch_url: Some("https://github.com/owner/repo.git".to_string()),
            push_url: Some("https://github.com/owner/repo.git".to_string()),
            github_https_credential_helper_configured: false,
        }),
    )
    .expect("GitHub HTTPS origin should require a configured credential helper");

    assert_eq!(issue.issue_kind, "auth_blocked");
    assert!(issue
        .reasons
        .iter()
        .any(|reason| reason.contains("credential helper")));
}

#[test]
fn github_https_helper_repair_is_not_hidden_by_provider_outage() {
    let issue = evaluate_project_git_auth_issue(
        &project(false),
        true,
        GithubConnectionState::ProviderUnavailable,
        Ok(GitRemoteAuthConfig {
            fetch_url: Some("https://github.com/owner/repo.git".to_string()),
            push_url: Some("https://github.com/owner/repo.git".to_string()),
            github_https_credential_helper_configured: false,
        }),
    )
    .expect("missing helper should remain actionable during a provider outage");

    assert_eq!(issue.issue_kind, "auth_blocked");
    assert!(issue
        .reasons
        .iter()
        .any(|reason| reason.contains("credential helper")));
    assert!(!issue
        .reasons
        .iter()
        .any(|reason| reason.contains("temporarily unavailable")));
}

#[test]
fn github_https_origin_with_helper_and_gh_auth_does_not_block() {
    let issue = evaluate_project_git_auth_issue(
        &project(false),
        true,
        gh_state(true),
        Ok(GitRemoteAuthConfig {
            fetch_url: Some("https://github.com/owner/repo.git".to_string()),
            push_url: Some("https://github.com/owner/repo.git".to_string()),
            github_https_credential_helper_configured: true,
        }),
    );

    assert!(issue.is_none());
}

#[test]
fn git_config_inspection_error_reports_repo_unavailable() {
    let issue = evaluate_project_git_auth_issue(
        &project(false),
        false,
        gh_state(true),
        Err("permission denied".to_string()),
    )
    .expect("repo inspection failures should be visible");

    assert_eq!(issue.issue_kind, "repo_unavailable");
    assert_eq!(issue.fetch_kind, None);
    assert_eq!(issue.push_kind, None);
    assert!(issue
        .reasons
        .iter()
        .any(|reason| reason.contains("could not inspect origin remote")));
}

#[test]
fn ssh_project_without_pr_mode_does_not_block_when_gh_is_missing() {
    let issue = evaluate_project_git_auth_issue(
        &project(false),
        true,
        gh_state(false),
        Ok(GitRemoteAuthConfig {
            fetch_url: Some("git@github.com:owner/repo.git".to_string()),
            push_url: Some("git@github.com:owner/repo.git".to_string()),
            github_https_credential_helper_configured: false,
        }),
    );

    assert!(issue.is_none());
}

#[test]
fn missing_origin_is_repo_config_issue_not_auth_issue() {
    let issue = evaluate_project_git_auth_issue(
        &project(true),
        false,
        gh_state(false),
        Ok(GitRemoteAuthConfig {
            fetch_url: None,
            push_url: None,
            github_https_credential_helper_configured: false,
        }),
    )
    .expect("missing origin should be reported");

    assert_eq!(issue.issue_kind, "repo_remote_missing");
    assert_eq!(issue.reasons, vec!["origin remote is not configured"]);
}

#[test]
fn provider_outage_blocks_startup_without_requesting_auth_repair() {
    let issue = evaluate_project_git_auth_issue(
        &project(true),
        true,
        GithubConnectionState::ProviderUnavailable,
        Ok(GitRemoteAuthConfig {
            fetch_url: Some("git@github.com:owner/repo.git".to_string()),
            push_url: Some("git@github.com:owner/repo.git".to_string()),
            github_https_credential_helper_configured: false,
        }),
    )
    .expect("unreadable provider state must fail closed");

    assert!(!issue.gh_authenticated);
    assert_eq!(issue.gh_state, GithubConnectionState::ProviderUnavailable);
    assert_eq!(issue.issue_kind, "github_unavailable");
    assert!(issue
        .reasons
        .iter()
        .any(|reason| reason.contains("temporarily unavailable")));
    assert!(!issue
        .reasons
        .iter()
        .any(|reason| reason.contains("needs GitHub CLI authentication")));
}

#[test]
fn rejected_credential_requests_replacement_not_login() {
    let issue = evaluate_project_git_auth_issue(
        &project(true),
        true,
        GithubConnectionState::CredentialRejected,
        Ok(GitRemoteAuthConfig {
            fetch_url: Some("git@github.com:owner/repo.git".to_string()),
            push_url: Some("git@github.com:owner/repo.git".to_string()),
            github_https_credential_helper_configured: false,
        }),
    )
    .expect("rejected credentials must remain actionable");

    assert_eq!(issue.issue_kind, "auth_blocked");
    assert_eq!(issue.gh_state, GithubConnectionState::CredentialRejected);
    assert!(issue
        .reasons
        .iter()
        .any(|reason| reason.contains("rejected GitHub CLI credential replaced")));
    assert!(!issue
        .reasons
        .iter()
        .any(|reason| reason.contains("temporarily unavailable")));
}

#[test]
fn missing_gh_cli_gets_dedicated_startup_issue_kind() {
    let issue = evaluate_project_git_auth_issue(
        &project(true),
        true,
        GithubConnectionState::CliUnavailable,
        Ok(GitRemoteAuthConfig {
            fetch_url: Some("git@github.com:owner/repo.git".to_string()),
            push_url: Some("git@github.com:owner/repo.git".to_string()),
            github_https_credential_helper_configured: false,
        }),
    )
    .expect("missing gh CLI should block without claiming credential loss");

    assert_eq!(issue.issue_kind, "gh_cli_unavailable");
    assert_eq!(issue.gh_state, GithubConnectionState::CliUnavailable);
    assert!(issue
        .reasons
        .iter()
        .any(|reason| reason.contains("available GitHub CLI")));
}

#[test]
fn probe_failure_defers_startup_without_requesting_auth_repair() {
    let issue = evaluate_project_git_auth_issue(
        &project(true),
        true,
        GithubConnectionState::ProbeFailed,
        Ok(GitRemoteAuthConfig {
            fetch_url: Some("git@github.com:owner/repo.git".to_string()),
            push_url: Some("git@github.com:owner/repo.git".to_string()),
            github_https_credential_helper_configured: false,
        }),
    )
    .expect("failed verification should fail closed");

    assert_eq!(issue.issue_kind, "github_unavailable");
    assert_eq!(issue.gh_state, GithubConnectionState::ProbeFailed);
    assert!(issue
        .reasons
        .iter()
        .any(|reason| reason.contains("could not be verified")));
    assert!(!issue
        .reasons
        .iter()
        .any(|reason| reason.contains("needs GitHub CLI authentication")));
}

#[test]
fn terminal_only_records_do_not_force_preflight_scope() {
    let active_project = project(true);
    let mut plan_branch = PlanBranch::new(
        crate::domain::entities::ArtifactId::from_string("artifact-1".to_string()),
        crate::domain::entities::IdeationSessionId::from_string("session-1".to_string()),
        active_project.id.clone(),
        "ralphx/demo/plan-old".to_string(),
        "main".to_string(),
    );
    plan_branch.status = PlanBranchStatus::Merged;

    let mut workspace = AgentConversationWorkspace::new(
        crate::domain::entities::ChatConversationId::from_string("conversation-1".to_string()),
        active_project.id.clone(),
        crate::domain::entities::AgentConversationWorkspaceMode::Edit,
        crate::domain::entities::IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        None,
        None,
        "ralphx/demo/agent-old".to_string(),
        "/tmp/ralphx-demo-agent-old".to_string(),
    );
    workspace.publication_pr_number = Some(101);
    workspace.publication_pr_status = Some("merged".to_string());

    assert!(!plan_branch_has_startup_git_work(&plan_branch));
    assert!(!workspace_has_startup_git_work(&workspace));
    assert!(!should_preflight_project(&active_project, false, false));
}

#[tokio::test]
async fn startup_git_work_scope_detects_active_pr_and_workspace_records() {
    let app_state = crate::application::AppState::new_test();
    let active_project = project(true);

    let mut plan_branch = PlanBranch::new(
        crate::domain::entities::ArtifactId::from_string("artifact-active".to_string()),
        crate::domain::entities::IdeationSessionId::from_string("session-active".to_string()),
        active_project.id.clone(),
        "ralphx/demo/plan-active".to_string(),
        "main".to_string(),
    );
    plan_branch.status = PlanBranchStatus::Active;
    plan_branch.pr_eligible = true;
    plan_branch.merge_task_id = Some(crate::domain::entities::TaskId::from_string(
        "merge-task-active".to_string(),
    ));
    app_state
        .plan_branch_repo
        .create(plan_branch)
        .await
        .expect("plan branch should persist");

    assert!(
        project_has_startup_git_work(
            &active_project,
            Some(&app_state.plan_branch_repo),
            Some(&app_state.agent_conversation_workspace_repo),
        )
        .await
    );

    let mut workspace_project = project(false);
    workspace_project.id = ProjectId::from_string("project-2".to_string());
    let mut workspace = AgentConversationWorkspace::new(
        crate::domain::entities::ChatConversationId::from_string(
            "conversation-active-workspace".to_string(),
        ),
        workspace_project.id.clone(),
        crate::domain::entities::AgentConversationWorkspaceMode::Edit,
        crate::domain::entities::IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        None,
        None,
        "ralphx/demo/agent-active".to_string(),
        "/tmp/ralphx-demo-agent-active".to_string(),
    );
    workspace.publication_pr_number = Some(88);
    workspace.publication_pr_status = Some("open".to_string());
    app_state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");

    assert!(
        project_has_startup_git_work(
            &workspace_project,
            Some(&app_state.plan_branch_repo),
            Some(&app_state.agent_conversation_workspace_repo),
        )
        .await
    );
}

#[test]
fn gh_auth_check_is_needed_only_for_pr_mode_or_github_https() {
    assert!(project_needs_gh_auth_check(
        &project(true),
        &Ok(GitRemoteAuthConfig {
            fetch_url: Some("git@github.com:owner/repo.git".to_string()),
            push_url: Some("git@github.com:owner/repo.git".to_string()),
            github_https_credential_helper_configured: false,
        }),
    ));
    assert!(project_needs_gh_auth_check(
        &project(false),
        &Ok(GitRemoteAuthConfig {
            fetch_url: Some("https://github.com/owner/repo.git".to_string()),
            push_url: Some("https://github.com/owner/repo.git".to_string()),
            github_https_credential_helper_configured: false,
        }),
    ));
    assert!(!project_needs_gh_auth_check(
        &project(false),
        &Ok(GitRemoteAuthConfig {
            fetch_url: Some("git@github.com:owner/repo.git".to_string()),
            push_url: Some("git@github.com:owner/repo.git".to_string()),
            github_https_credential_helper_configured: false,
        }),
    ));
    assert!(!project_needs_gh_auth_check(
        &project(true),
        &Ok(GitRemoteAuthConfig {
            fetch_url: None,
            push_url: None,
            github_https_credential_helper_configured: false,
        }),
    ));
    assert!(!project_needs_gh_auth_check(
        &project(true),
        &Err("repo unavailable".to_string()),
    ));
}

#[tokio::test]
async fn startup_git_auth_preflight_skips_projects_without_startup_git_work() {
    let app_state = crate::application::AppState::new_test();
    let app = crate::testing::create_mock_app();
    let inactive_project = app_state
        .project_repo
        .create(project(false))
        .await
        .expect("project should persist");
    assert!(!inactive_project.github_pr_enabled);

    let summary = run_startup_git_auth_preflight_with_notifications(
        Arc::clone(&app_state.project_repo),
        Arc::clone(&app_state.app_state_repo),
        Some(Arc::clone(&app_state.plan_branch_repo)),
        Some(Arc::clone(&app_state.agent_conversation_workspace_repo)),
        app.handle(),
        Some(app_state.notification_service()),
    )
    .await;

    assert!(summary.issues.is_empty());
    assert!(summary.blocked_project_ids().is_empty());
    assert!(!summary.active_project_blocked());
    assert!(!summary.has_blocked_projects());
    assert!(app_state
        .notification_repo
        .list(None, None, 50)
        .await
        .expect("notifications should list")
        .notifications
        .is_empty());
}

#[tokio::test]
async fn startup_git_auth_preflight_reports_active_project_missing_origin() {
    let app_state = crate::application::AppState::new_test();
    let app = crate::testing::create_mock_app();
    let repo = tempfile::tempdir().expect("repo tempdir");
    std::fs::create_dir(repo.path().join(".git")).expect("git dir should exist");
    std::fs::write(
        repo.path().join(".git").join("config"),
        "[core]\n\trepositoryformatversion = 0\n",
    )
    .expect("git config should be written");
    let mut active_project = project(false);
    active_project.working_directory = repo.path().to_string_lossy().to_string();
    let active_project = app_state
        .project_repo
        .create(active_project)
        .await
        .expect("project should persist");
    app_state
        .app_state_repo
        .set_active_project(Some(&active_project.id))
        .await
        .expect("active project should persist");

    let summary = run_startup_git_auth_preflight_with_notifications(
        Arc::clone(&app_state.project_repo),
        Arc::clone(&app_state.app_state_repo),
        Some(Arc::clone(&app_state.plan_branch_repo)),
        Some(Arc::clone(&app_state.agent_conversation_workspace_repo)),
        app.handle(),
        Some(app_state.notification_service()),
    )
    .await;

    assert!(summary.active_project_blocked());
    assert_eq!(summary.issues.len(), 1);
    assert_eq!(summary.issues[0].issue_kind, "repo_remote_missing");
    assert_eq!(
        summary.issues[0].reasons,
        vec!["origin remote is not configured"]
    );
    let notifications = app_state
        .notification_repo
        .list(None, None, 50)
        .await
        .expect("notifications should list")
        .notifications;
    assert_eq!(notifications.len(), 1);
    assert_eq!(
        notifications[0].category,
        NotificationCategory::GitAuthPreflight
    );
}

#[tokio::test]
async fn startup_git_auth_preflight_fails_closed_on_app_state_read_error() {
    let app_state = crate::application::AppState::new_test();
    let app = crate::testing::create_mock_app();

    let summary = run_startup_git_auth_preflight_with_notifications(
        Arc::clone(&app_state.project_repo),
        Arc::new(ReadFailingAppStateRepository),
        None,
        None,
        app.handle(),
        None,
    )
    .await;

    assert_eq!(
        summary.failure_code.as_deref(),
        Some("startup_app_state_read_failed")
    );
    assert!(summary.has_blocked_projects());
    assert!(summary.active_project_blocked());
    assert!(summary.issues.is_empty());
}

#[tokio::test]
async fn startup_git_auth_preflight_fails_closed_on_project_list_error() {
    let app_state = crate::application::AppState::new_test();
    let app = crate::testing::create_mock_app();

    let summary = run_startup_git_auth_preflight_with_notifications(
        Arc::new(ReadFailingProjectRepository),
        Arc::clone(&app_state.app_state_repo),
        None,
        None,
        app.handle(),
        None,
    )
    .await;

    assert_eq!(
        summary.failure_code.as_deref(),
        Some("startup_project_list_read_failed")
    );
    assert!(summary.has_blocked_projects());
    assert!(summary.active_project_blocked());
    assert!(summary.issues.is_empty());
}
