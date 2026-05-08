use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::domain::entities::{Project, ProjectId};
use crate::domain::repositories::{AppStateRepository, ProjectRepository};
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

pub(crate) async fn run_startup_git_auth_preflight(
    project_repo: Arc<dyn ProjectRepository>,
    app_state_repo: Arc<dyn AppStateRepository>,
    app_handle: &AppHandle,
) -> StartupGitAuthPreflightSummary {
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

    let gh_authenticated = check_gh_auth_status().await;
    let mut issues = Vec::new();

    for project in projects {
        let active_project = active_project_id.as_ref() == Some(&project.id);
        if !should_preflight_project(&project, active_project) {
            continue;
        }

        let config_result = inspect_origin_auth_config(Path::new(&project.working_directory))
            .await
            .map_err(|error| error.to_string());

        if let Some(issue) = evaluate_project_git_auth_issue(
            &project,
            active_project,
            gh_authenticated,
            config_result,
        ) {
            tracing::warn!(
                project_id = issue.project_id,
                project_name = issue.project_name,
                reasons = ?issue.reasons,
                "Startup Git auth preflight blocked Git/GitHub startup work for project"
            );
            issues.push(issue);
        }
    }

    let summary = StartupGitAuthPreflightSummary { issues };
    if !summary.issues.is_empty() {
        let _ = app_handle.emit(STARTUP_GIT_AUTH_PREFLIGHT_EVENT, &summary);
    }

    summary
}

fn should_preflight_project(project: &Project, active_project: bool) -> bool {
    project.archived_at.is_none() && (active_project || project.github_pr_enabled)
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
                reasons.push("origin remote is not configured".to_string());
            }

            if mixed_auth_modes {
                reasons.push("origin fetch and push use different auth modes".to_string());
            }

            if project.github_pr_enabled && !gh_authenticated {
                reasons.push(
                    "GitHub PR mode is enabled but GitHub CLI is not authenticated".to_string(),
                );
            }

            if has_github_https_remote(&config) && !gh_authenticated {
                reasons.push(
                    "GitHub HTTPS origin needs non-interactive credentials for background git access"
                        .to_string(),
                );
            }
        }
        Err(error) => {
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
}
