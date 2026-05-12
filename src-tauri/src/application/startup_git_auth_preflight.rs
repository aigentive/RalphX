use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use futures::{stream, StreamExt};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime};

use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceStatus, PlanBranch, PlanBranchStatus,
    Project, ProjectId,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, AppStateRepository, PlanBranchRepository,
    ProjectRepository,
};
use crate::infrastructure::git_auth::{
    check_gh_auth_status, git_remote_url_kind_label, inspect_origin_auth_config,
    suggested_github_ssh_origin, GitRemoteAuthConfig, GitRemoteUrlKind,
};

pub(crate) const STARTUP_GIT_AUTH_PREFLIGHT_EVENT: &str = "git-auth:startup_preflight";

#[derive(Debug, Default)]
pub(crate) struct StartupGitAuthRecoveryState {
    pending: AtomicBool,
    resuming: AtomicBool,
}

impl StartupGitAuthRecoveryState {
    pub(crate) fn mark_pending(&self) {
        self.pending.store(true, Ordering::SeqCst);
    }

    pub(crate) fn clear_pending(&self) {
        self.pending.store(false, Ordering::SeqCst);
    }

    pub(crate) fn is_pending(&self) -> bool {
        self.pending.load(Ordering::SeqCst)
    }

    pub(crate) fn try_begin_resume(&self) -> bool {
        self.is_pending()
            && self
                .resuming
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
    }

    pub(crate) fn finish_resume(&self) {
        self.resuming.store(false, Ordering::SeqCst);
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StartupGitAuthIssue {
    pub project_id: String,
    pub project_name: String,
    pub active_project: bool,
    pub github_pr_enabled: bool,
    pub fetch_kind: Option<String>,
    pub push_kind: Option<String>,
    pub mixed_auth_modes: bool,
    pub gh_authenticated: bool,
    pub issue_kind: String,
    pub can_switch_to_ssh: bool,
    pub suggested_ssh_url: Option<String>,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StartupGitAuthPreflightSummary {
    pub issues: Vec<StartupGitAuthIssue>,
}

impl StartupGitAuthPreflightSummary {
    pub(crate) fn blocked_project_ids(&self) -> HashSet<ProjectId> {
        self.issues
            .iter()
            .map(|issue| ProjectId::from_string(issue.project_id.clone()))
            .collect()
    }

    pub(crate) fn active_project_blocked(&self) -> bool {
        self.issues.iter().any(|issue| issue.active_project)
    }

    pub(crate) fn has_blocked_projects(&self) -> bool {
        !self.issues.is_empty()
    }
}

pub(crate) async fn run_startup_git_auth_preflight<R: Runtime>(
    project_repo: Arc<dyn ProjectRepository>,
    app_state_repo: Arc<dyn AppStateRepository>,
    plan_branch_repo: Option<Arc<dyn PlanBranchRepository>>,
    workspace_repo: Option<Arc<dyn AgentConversationWorkspaceRepository>>,
    app_handle: &AppHandle<R>,
) -> StartupGitAuthPreflightSummary {
    let started_at = Instant::now();
    let active_project_id = app_state_repo
        .get()
        .await
        .ok()
        .and_then(|settings| settings.active_project_id);

    let projects = match project_repo.get_all().await {
        Ok(projects) => projects,
        Err(error) => {
            tracing::warn!(
                error = %error,
                "Startup Git auth preflight: failed to load projects"
            );
            return StartupGitAuthPreflightSummary::default();
        }
    };

    let mut projects_seen = 0usize;
    let mut projects_skipped_no_work = 0usize;
    let mut projects_skipped_archived = 0usize;
    let mut candidates = Vec::new();

    for project in projects {
        projects_seen += 1;
        let active_project = active_project_id.as_ref() == Some(&project.id);
        if project.archived_at.is_some() {
            projects_skipped_archived += 1;
            continue;
        }

        let has_startup_git_work = active_project
            || project_has_startup_git_work(
                &project,
                plan_branch_repo.as_ref(),
                workspace_repo.as_ref(),
            )
            .await;
        if !should_preflight_project(&project, active_project, has_startup_git_work) {
            projects_skipped_no_work += 1;
            continue;
        }
        candidates.push((project, active_project));
    }

    if candidates.is_empty() {
        tracing::info!(
            projects_seen,
            projects_considered = 0usize,
            projects_skipped_no_work,
            projects_skipped_archived,
            issues = 0usize,
            elapsed_ms = started_at.elapsed().as_millis(),
            "Startup Git auth preflight completed"
        );
        return StartupGitAuthPreflightSummary::default();
    }

    let gh_started_at = Instant::now();
    let gh_authenticated = check_gh_auth_status().await;
    tracing::info!(
        gh_authenticated,
        elapsed_ms = gh_started_at.elapsed().as_millis(),
        "Startup Git auth preflight: GitHub CLI auth check completed"
    );

    let inspected = stream::iter(candidates)
        .map(|(project, active_project)| async move {
            let project_started_at = Instant::now();
            let config_result = inspect_origin_auth_config(Path::new(&project.working_directory))
                .await
                .map_err(|error| error.to_string());
            let project_elapsed_ms = project_started_at.elapsed().as_millis();
            let issue = evaluate_project_git_auth_issue(
                &project,
                active_project,
                gh_authenticated,
                config_result,
            );
            (project, active_project, issue, project_elapsed_ms)
        })
        .buffer_unordered(4)
        .collect::<Vec<_>>()
        .await;

    let projects_considered = inspected.len();
    let mut issues = Vec::new();
    for (project, active_project, issue, project_elapsed_ms) in inspected {
        if let Some(issue) = issue {
            tracing::warn!(
                project_id = issue.project_id.as_str(),
                project_name = issue.project_name.as_str(),
                issue_kind = issue.issue_kind.as_str(),
                active_project = issue.active_project,
                github_pr_enabled = issue.github_pr_enabled,
                elapsed_ms = project_elapsed_ms,
                reasons = ?issue.reasons,
                "Startup Git auth preflight blocked Git/GitHub startup work for project"
            );
            issues.push(issue);
        } else if project_elapsed_ms >= 1_000 {
            tracing::info!(
                project_id = project.id.as_str(),
                project_name = project.name.as_str(),
                active_project,
                github_pr_enabled = project.github_pr_enabled,
                elapsed_ms = project_elapsed_ms,
                "Startup Git auth preflight: project passed slowly"
            );
        }
    }

    let summary = StartupGitAuthPreflightSummary { issues };
    tracing::info!(
        projects_seen,
        projects_considered,
        projects_skipped_no_work,
        projects_skipped_archived,
        issues = summary.issues.len(),
        elapsed_ms = started_at.elapsed().as_millis(),
        "Startup Git auth preflight completed"
    );
    if !summary.issues.is_empty() {
        let _ = app_handle.emit(STARTUP_GIT_AUTH_PREFLIGHT_EVENT, &summary);
    }

    summary
}

fn should_preflight_project(
    project: &Project,
    active_project: bool,
    has_startup_git_work: bool,
) -> bool {
    project.archived_at.is_none()
        && (active_project || (project.github_pr_enabled && has_startup_git_work))
}

async fn project_has_startup_git_work(
    project: &Project,
    plan_branch_repo: Option<&Arc<dyn PlanBranchRepository>>,
    workspace_repo: Option<&Arc<dyn AgentConversationWorkspaceRepository>>,
) -> bool {
    if let Some(plan_branch_repo) = plan_branch_repo {
        match plan_branch_repo.get_by_project_id(&project.id).await {
            Ok(plan_branches) if plan_branches.iter().any(plan_branch_has_startup_git_work) => {
                return true;
            }
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(
                    project_id = project.id.as_str(),
                    error = %error,
                    "Startup Git auth preflight: failed to inspect plan branches; keeping project in preflight scope"
                );
                return true;
            }
        }
    }

    if let Some(workspace_repo) = workspace_repo {
        match workspace_repo.get_by_project_id(&project.id).await {
            Ok(workspaces) if workspaces.iter().any(workspace_has_startup_git_work) => return true,
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(
                    project_id = project.id.as_str(),
                    error = %error,
                    "Startup Git auth preflight: failed to inspect agent workspaces; keeping project in preflight scope"
                );
                return true;
            }
        }
    }

    false
}

fn plan_branch_has_startup_git_work(plan_branch: &PlanBranch) -> bool {
    plan_branch.pr_polling_active
        || (plan_branch.pr_eligible && plan_branch.status == PlanBranchStatus::Active)
        || (plan_branch.pr_number.is_some() && plan_branch.status == PlanBranchStatus::Active)
}

fn workspace_has_startup_git_work(workspace: &AgentConversationWorkspace) -> bool {
    workspace.status == AgentConversationWorkspaceStatus::Active
        && workspace.publication_pr_number.is_some()
        && !workspace
            .publication_pr_status
            .as_deref()
            .is_some_and(|status| matches!(status, "merged" | "closed"))
}

pub(crate) fn evaluate_project_git_auth_issue(
    project: &Project,
    active_project: bool,
    gh_authenticated: bool,
    config_result: Result<GitRemoteAuthConfig, String>,
) -> Option<StartupGitAuthIssue> {
    let mut reasons = Vec::new();
    let mut fetch_kind = None;
    let mut push_kind = None;
    let mut mixed_auth_modes = false;
    let mut suggested_ssh_url = None;
    let mut issue_kind = "auth_blocked".to_string();

    match config_result {
        Ok(config) => {
            let fetch_kind_value = config.fetch_kind();
            let push_kind_value = config.push_kind();
            fetch_kind =
                fetch_kind_value.map(|kind| git_remote_url_kind_label(Some(kind)).to_string());
            push_kind =
                push_kind_value.map(|kind| git_remote_url_kind_label(Some(kind)).to_string());
            mixed_auth_modes = config.has_mixed_auth_modes();
            suggested_ssh_url = suggested_github_ssh_origin(&config);

            if config.fetch_url.is_none() {
                issue_kind = "repo_remote_missing".to_string();
                reasons.push("origin remote is not configured".to_string());
            } else if mixed_auth_modes {
                issue_kind = "auth_blocked".to_string();
                reasons.push("origin fetch and push use different auth modes".to_string());
            } else {
                if project.github_pr_enabled && !gh_authenticated {
                    issue_kind = "auth_blocked".to_string();
                    reasons.push(
                        "GitHub PR mode is enabled but GitHub CLI is not authenticated".to_string(),
                    );
                }

                if has_github_https_remote(&config) && !gh_authenticated {
                    issue_kind = "auth_blocked".to_string();
                    reasons.push(
                        "GitHub HTTPS origin needs non-interactive credentials for background git access"
                            .to_string(),
                    );
                }
            }
        }
        Err(error) => {
            issue_kind = "repo_unavailable".to_string();
            reasons.push(format!("could not inspect origin remote: {error}"));
        }
    }

    if reasons.is_empty() {
        return None;
    }

    Some(StartupGitAuthIssue {
        project_id: project.id.as_str().to_string(),
        project_name: project.name.clone(),
        active_project,
        github_pr_enabled: project.github_pr_enabled,
        fetch_kind,
        push_kind,
        mixed_auth_modes,
        gh_authenticated,
        issue_kind,
        can_switch_to_ssh: suggested_ssh_url.is_some(),
        suggested_ssh_url,
        reasons,
    })
}

fn has_github_https_remote(config: &GitRemoteAuthConfig) -> bool {
    [config.fetch_url.as_deref(), config.push_url.as_deref()]
        .into_iter()
        .flatten()
        .any(|url| {
            url.trim().starts_with("https://github.com/")
                && matches!(
                    crate::infrastructure::git_auth::classify_git_remote_url(url),
                    GitRemoteUrlKind::Https
                )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(github_pr_enabled: bool) -> Project {
        let mut project = Project::new("RalphX".to_string(), "/repo".to_string());
        project.id = ProjectId::from_string("project-1".to_string());
        project.github_pr_enabled = github_pr_enabled;
        project
    }

    #[test]
    fn mixed_https_fetch_ssh_push_blocks_startup_git_work() {
        let issue = evaluate_project_git_auth_issue(
            &project(true),
            true,
            true,
            Ok(GitRemoteAuthConfig {
                fetch_url: Some("https://github.com/owner/repo.git".to_string()),
                push_url: Some("git@github.com:owner/repo.git".to_string()),
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
            false,
            Ok(GitRemoteAuthConfig {
                fetch_url: Some("git@github.com:owner/repo.git".to_string()),
                push_url: Some("git@github.com:owner/repo.git".to_string()),
            }),
        )
        .expect("gh auth should be required for PR mode");

        assert!(issue.github_pr_enabled);
        assert!(issue
            .reasons
            .iter()
            .any(|reason| reason.contains("GitHub CLI is not authenticated")));
        assert_eq!(issue.issue_kind, "auth_blocked");
    }

    #[test]
    fn github_https_origin_blocks_background_git_when_gh_is_missing() {
        let issue = evaluate_project_git_auth_issue(
            &project(false),
            true,
            false,
            Ok(GitRemoteAuthConfig {
                fetch_url: Some("https://github.com/owner/repo.git".to_string()),
                push_url: Some("https://github.com/owner/repo.git".to_string()),
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
            .any(|reason| reason.contains("non-interactive credentials")));
    }

    #[test]
    fn git_config_inspection_error_reports_repo_unavailable() {
        let issue = evaluate_project_git_auth_issue(
            &project(false),
            false,
            true,
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
            false,
            Ok(GitRemoteAuthConfig {
                fetch_url: Some("git@github.com:owner/repo.git".to_string()),
                push_url: Some("git@github.com:owner/repo.git".to_string()),
            }),
        );

        assert!(issue.is_none());
    }

    #[test]
    fn missing_origin_is_repo_config_issue_not_auth_issue() {
        let issue = evaluate_project_git_auth_issue(
            &project(true),
            false,
            false,
            Ok(GitRemoteAuthConfig {
                fetch_url: None,
                push_url: None,
            }),
        )
        .expect("missing origin should be reported");

        assert_eq!(issue.issue_kind, "repo_remote_missing");
        assert_eq!(issue.reasons, vec!["origin remote is not configured"]);
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

        let summary = run_startup_git_auth_preflight(
            Arc::clone(&app_state.project_repo),
            Arc::clone(&app_state.app_state_repo),
            Some(Arc::clone(&app_state.plan_branch_repo)),
            Some(Arc::clone(&app_state.agent_conversation_workspace_repo)),
            app.handle(),
        )
        .await;

        assert!(summary.issues.is_empty());
        assert!(summary.blocked_project_ids().is_empty());
        assert!(!summary.active_project_blocked());
        assert!(!summary.has_blocked_projects());
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

        let summary = run_startup_git_auth_preflight(
            Arc::clone(&app_state.project_repo),
            Arc::clone(&app_state.app_state_repo),
            Some(Arc::clone(&app_state.plan_branch_repo)),
            Some(Arc::clone(&app_state.agent_conversation_workspace_repo)),
            app.handle(),
        )
        .await;

        assert!(summary.active_project_blocked());
        assert_eq!(summary.issues.len(), 1);
        assert_eq!(summary.issues[0].issue_kind, "repo_remote_missing");
        assert_eq!(
            summary.issues[0].reasons,
            vec!["origin remote is not configured"]
        );
    }
}
