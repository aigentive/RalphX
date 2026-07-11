use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use chrono::Utc;
use futures::{stream, StreamExt};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime};

use crate::application::NotificationService;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceStatus, NewNotification,
    NotificationCategory, NotificationSeverity, NotificationTarget, PlanBranch, PlanBranchStatus,
    Project, ProjectId,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, AppStateRepository, PlanBranchRepository,
    ProjectRepository,
};
use crate::infrastructure::git_auth::{
    check_gh_auth_token_available, git_remote_url_kind_label, inspect_origin_auth_config,
    suggested_github_ssh_origin, GitRemoteAuthConfig,
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

pub(crate) async fn run_startup_git_auth_preflight_with_notifications<R: Runtime>(
    project_repo: Arc<dyn ProjectRepository>,
    app_state_repo: Arc<dyn AppStateRepository>,
    plan_branch_repo: Option<Arc<dyn PlanBranchRepository>>,
    workspace_repo: Option<Arc<dyn AgentConversationWorkspaceRepository>>,
    app_handle: &AppHandle<R>,
    notification_service: Option<Arc<NotificationService>>,
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

    let inspected = stream::iter(candidates)
        .map(|(project, active_project)| async move {
            let project_started_at = Instant::now();
            let config_result = inspect_origin_auth_config(Path::new(&project.working_directory))
                .await
                .map_err(|error| error.to_string());
            let project_elapsed_ms = project_started_at.elapsed().as_millis();
            (project, active_project, config_result, project_elapsed_ms)
        })
        .buffer_unordered(4)
        .collect::<Vec<_>>()
        .await;

    let gh_auth_required = inspected
        .iter()
        .any(|(project, _, config_result, _)| project_needs_gh_auth_check(project, config_result));
    let gh_started_at = Instant::now();
    let gh_authenticated = if gh_auth_required {
        check_gh_auth_token_available().await
    } else {
        false
    };
    tracing::info!(
        gh_authenticated,
        auth_required = gh_auth_required,
        method = if gh_auth_required { "token" } else { "skipped" },
        elapsed_ms = gh_started_at.elapsed().as_millis(),
        "Startup Git auth preflight: GitHub CLI auth check completed"
    );

    let projects_considered = inspected.len();
    let mut issues = Vec::new();
    for (project, active_project, config_result, project_elapsed_ms) in inspected {
        let issue = evaluate_project_git_auth_issue(
            &project,
            active_project,
            gh_authenticated,
            config_result,
        );
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
        if let Some(notification_service) = notification_service {
            notification_service
                .record(NewNotification {
                    project_id: None,
                    category: NotificationCategory::GitAuthPreflight,
                    severity: NotificationSeverity::Warning,
                    title: "Git authentication needs attention".to_string(),
                    body: Some(format!(
                        "{} project{} blocked by Git or GitHub authentication",
                        summary.issues.len(),
                        if summary.issues.len() == 1 {
                            " is"
                        } else {
                            "s are"
                        }
                    )),
                    target: NotificationTarget::none(),
                    dedupe_key: Some(format!("git-auth-preflight:{}", Utc::now().to_rfc3339())),
                })
                .await;
        }
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
        match plan_branch_repo
            .get_startup_pr_recovery_candidates_by_project_id(&project.id)
            .await
        {
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

fn project_needs_gh_auth_check(
    project: &Project,
    config_result: &Result<GitRemoteAuthConfig, String>,
) -> bool {
    let Ok(config) = config_result else {
        return false;
    };
    config.fetch_url.is_some() && project.github_pr_enabled || has_github_https_remote(config)
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

                let has_github_https_remote = config.has_github_https_remote();
                if has_github_https_remote && !config.github_https_credential_helper_configured {
                    issue_kind = "auth_blocked".to_string();
                    reasons.push(
                        "GitHub HTTPS origin needs a non-interactive credential helper for background git access"
                            .to_string(),
                    );
                }

                if has_github_https_remote && !gh_authenticated {
                    issue_kind = "auth_blocked".to_string();
                    reasons.push(
                        "GitHub HTTPS origin needs GitHub CLI authentication for background git access"
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
    config.has_github_https_remote()
}

#[cfg(test)]
#[path = "startup_git_auth_preflight_tests.rs"]
mod tests;
