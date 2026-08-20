//! Tauri commands for cloning a repository into a new project.
//!
//! The wizard validates a target, starts a job, watches progress, and can cancel.
//! Job state is also re-readable by id, because events alone cannot be trusted to
//! deliver a terminal outcome (see `clone_job_registry`).

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tauri::State;

use crate::application::clone_job_registry::CloneJobStatus;
use crate::application::clone_job_runner::run_clone_job;
use crate::application::git_service::clone::{is_empty_directory, GitCloneRequest};
use crate::application::git_service::clone_url::normalize_clone_url;
use crate::application::AppState;
use crate::commands::project_probe_commands::resolve_new_project_directory;
use crate::error::AppError;
use crate::infrastructure::agents::claude::git_runtime_config;
use crate::infrastructure::git_auth::{apply_git_subprocess_env, github_https_remote_to_ssh};
use crate::infrastructure::tool_paths::resolve_gh_cli_path;

/// Everything the wizard needs to decide whether Clone can proceed.
///
/// Deliberately total rather than fallible: a person typing a URL should see an
/// inline explanation, not an exception per keystroke.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CloneTargetPlan {
    /// The URL Git will actually be given, once the entry is usable.
    pub normalized_url: Option<String>,
    /// Folder name derived from the repository name.
    pub folder_name: Option<String>,
    /// Branch implied by the entry (a pasted GitHub branch page sets this).
    pub branch: Option<String>,
    /// SSH form of a convertible GitHub HTTPS URL, offered when sign-in fails.
    pub suggested_ssh_url: Option<String>,
    /// Full destination path, once a parent folder has been chosen.
    pub destination: Option<String>,
    pub ready: bool,
    /// Plain-language reason this target is not ready yet.
    pub problem: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloneTargetInput {
    pub url: String,
    pub parent_directory: Option<String>,
    /// Overrides the folder name derived from the URL.
    pub folder_name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartProjectCloneInput {
    pub url: String,
    pub parent_directory: String,
    pub folder_name: Option<String>,
    pub branch: Option<String>,
    #[serde(default)]
    pub depth: Option<u32>,
    #[serde(default)]
    pub single_branch: bool,
    #[serde(default)]
    pub recurse_submodules: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StartProjectCloneResponse {
    pub job_id: String,
}

/// Check a clone target without touching the network or the filesystem.
///
/// # Errors
/// Never returns `Err`; unusable input is reported through `problem`.
#[tauri::command]
pub async fn validate_clone_target(
    input: CloneTargetInput,
    state: State<'_, AppState>,
) -> Result<CloneTargetPlan, String> {
    Ok(build_clone_target_plan(&input, &state).await)
}

/// Start cloning and return the job id to watch.
///
/// # Errors
/// Returns `Err` when the URL or destination is unusable.
#[tauri::command]
pub async fn start_project_clone(
    input: StartProjectCloneInput,
    state: State<'_, AppState>,
) -> Result<StartProjectCloneResponse, String> {
    let normalized = normalize_clone_url(&input.url).map_err(|failure| failure.message)?;
    let folder_name = input
        .folder_name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or(&normalized.folder_name);
    let destination = resolve_new_project_directory(&input.parent_directory, folder_name)
        .map_err(|error: AppError| error.to_string())?;

    let registration = state.clone_jobs.start(
        uuid::Uuid::new_v4().to_string(),
        destination.clone(),
        retention(),
    );
    if registration.deduplicated {
        // A double-click must join the running clone, not start a rival one.
        return Ok(StartProjectCloneResponse {
            job_id: registration.job_id,
        });
    }

    let request = GitCloneRequest {
        url: normalized.url,
        destination,
        branch: input.branch.or(normalized.branch),
        depth: input.depth,
        single_branch: input.single_branch,
        recurse_submodules: input.recurse_submodules,
    };
    let job_id = registration.job_id.clone();
    let registry = Arc::clone(&state.clone_jobs);
    let events = Arc::clone(&state.events);
    // Spawned from an async command, so a Tokio runtime is guaranteed.
    tokio::spawn(run_clone_job(
        job_id.clone(),
        request,
        registration.cancel,
        registry,
        events,
    ));

    Ok(StartProjectCloneResponse { job_id })
}

/// Stop a running clone. Returns `false` when the job already finished.
///
/// # Errors
/// Never returns `Err`.
#[tauri::command]
pub async fn cancel_project_clone(
    job_id: String,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    Ok(state.clone_jobs.cancel(&job_id))
}

/// Read a clone job's live or retained status.
///
/// An unknown or expired id answers `unknown`, so the UI fails closed with a
/// retry instead of spinning forever on an event it never received.
///
/// # Errors
/// Never returns `Err`.
#[tauri::command]
pub async fn get_clone_job_status(
    job_id: String,
    state: State<'_, AppState>,
) -> Result<CloneJobStatus, String> {
    Ok(state.clone_jobs.status(&job_id, retention()))
}

/// One repository from `gh repo list`.
///
/// `#[serde(default)]` throughout plus unknown-field tolerance, because the `gh`
/// JSON shape drifts between versions and a picker is not worth a hard failure.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GitHubRepoSummary {
    #[serde(default)]
    pub name_with_owner: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub is_private: bool,
    #[serde(default)]
    pub updated_at: Option<String>,
}

/// List the signed-in user's GitHub repositories for the clone picker.
///
/// Repo-less by construction: there is no project yet, so this follows the
/// `run_gh_command_with_timeout` precedent (resolved CLI path, GUI-safe env, no
/// working directory) rather than the working-directory-bound `gh` service.
///
/// # Errors
/// Returns `Err` when `gh` is unavailable, unauthenticated, times out, or emits
/// output that is not a JSON array.
#[tauri::command]
pub async fn list_github_repositories() -> Result<Vec<GitHubRepoSummary>, String> {
    let stdout = run_gh_json(&[
        "repo",
        "list",
        "--json",
        "nameWithOwner,description,isPrivate,updatedAt",
        "--limit",
        "50",
    ])
    .await?;

    parse_github_repo_list(&stdout)
}

async fn run_gh_json(args: &[&str]) -> Result<String, String> {
    let mut command = tokio::process::Command::new(resolve_gh_cli_path());
    apply_git_subprocess_env(&mut command);
    let child = command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| format!("Failed to spawn gh: {error}"))?;

    let deadline = Duration::from_secs(git_runtime_config().cmd_timeout_secs);
    let output = tokio::time::timeout(deadline, child.wait_with_output())
        .await
        .map_err(|_| "gh command timed out".to_string())?
        .map_err(|error| format!("Failed to wait for gh: {error}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            format!("gh {args:?} failed")
        } else {
            stderr
        });
    }

    String::from_utf8(output.stdout)
        .map_err(|error| format!("gh returned non-UTF-8 output: {error}"))
}

pub(crate) fn parse_github_repo_list(stdout: &str) -> Result<Vec<GitHubRepoSummary>, String> {
    serde_json::from_str::<Vec<GitHubRepoSummary>>(stdout.trim())
        .map_err(|error| format!("Could not read the repository list: {error}"))
}

pub(crate) async fn build_clone_target_plan(
    input: &CloneTargetInput,
    state: &AppState,
) -> CloneTargetPlan {
    let normalized = match normalize_clone_url(&input.url) {
        Ok(normalized) => normalized,
        Err(failure) => return unusable(None, failure.message),
    };
    let suggested_ssh_url = github_https_remote_to_ssh(&normalized.url);
    let folder_name = input
        .folder_name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or(&normalized.folder_name)
        .to_string();

    let mut plan = CloneTargetPlan {
        normalized_url: Some(normalized.url.clone()),
        folder_name: Some(folder_name.clone()),
        branch: normalized.branch.clone(),
        suggested_ssh_url,
        destination: None,
        ready: false,
        problem: None,
    };

    let Some(parent) = input
        .parent_directory
        .as_deref()
        .map(str::trim)
        .filter(|parent| !parent.is_empty())
    else {
        plan.problem = Some("Choose where to put the repository".to_string());
        return plan;
    };

    let destination = match resolve_new_project_directory(parent, &folder_name) {
        Ok(destination) => destination,
        Err(error) => {
            plan.problem = Some(error.to_string());
            return plan;
        }
    };
    plan.destination = Some(destination.display().to_string());
    plan.problem = destination_problem(&destination, state).await;
    plan.ready = plan.problem.is_none();
    plan
}

async fn destination_problem(destination: &Path, state: &AppState) -> Option<String> {
    if state.clone_jobs.has_live_job_for(destination) {
        return Some("That folder is already being cloned into".to_string());
    }
    if !destination.exists() {
        return None;
    }
    if !destination.is_dir() {
        return Some(format!(
            "A file already exists at {}",
            destination.display()
        ));
    }
    match is_empty_directory(destination).await {
        Ok(true) => None,
        Ok(false) => Some(format!(
            "{} already has contents. Pick an empty folder.",
            destination.display()
        )),
        Err(error) => Some(error.to_string()),
    }
}

fn unusable(normalized_url: Option<String>, problem: String) -> CloneTargetPlan {
    CloneTargetPlan {
        normalized_url,
        folder_name: None,
        branch: None,
        suggested_ssh_url: None,
        destination: None,
        ready: false,
        problem: Some(problem),
    }
}

/// Terminal outcomes stay readable for as long as a clone could have run, which
/// is the same budget the clone itself gets.
fn retention() -> Duration {
    Duration::from_secs(git_runtime_config().clone_timeout_secs)
}
