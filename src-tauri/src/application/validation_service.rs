use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;

use crate::application::git_service::GitService;
use crate::application::AppState;
use crate::domain::entities::{
    InternalStatus, Project, Task, TaskId, ValidationCacheData, ValidationCacheDecision,
    ValidationCacheMetadata, ValidationCommandCategory, ValidationCommandResult,
    ValidationCommandSource, ValidationCommandStatus, ValidationContextType, ValidationPurpose,
    ValidationRun, ValidationRunMode, ValidationRunStatus,
};
use crate::error::{AppError, AppResult};
use crate::infrastructure::tool_paths::{
    agent_subprocess_env_path, prepend_resolved_node_bin_to_path, resolve_shell_cli_path,
};
use crate::utils::path_safety::validate_absolute_non_root_path;
use crate::utils::truncate_str;

const DEFAULT_VALIDATION_TIMEOUT_SECS: u64 = 600;
const OUTPUT_SNIPPET_BYTES: usize = 4_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunTaskValidationRequest {
    pub task_id: String,
    #[serde(default)]
    pub purpose: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub context_type: Option<String>,
    #[serde(default)]
    pub caller_agent: Option<String>,
    #[serde(default)]
    pub analysis_fingerprint: Option<String>,
    #[serde(default)]
    pub commands: Vec<ValidationCommandRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationCommandRequest {
    pub command: String,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub related_files: Vec<String>,
    #[serde(default)]
    pub command_ref: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskValidationSummary {
    pub task_id: String,
    pub project_id: String,
    pub policy_enabled: bool,
    pub latest_run: Option<ValidationRunSummary>,
    pub commands: Vec<ValidationCommandSummary>,
    pub legacy_validation_cache: Option<ValidationCacheData>,
    pub disabled_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationRunSummary {
    pub id: String,
    pub purpose: String,
    pub context_type: String,
    pub requested_by_agent: Option<String>,
    pub status: String,
    pub mode: String,
    pub policy_enabled: bool,
    pub head_sha: Option<String>,
    pub head_short_sha: Option<String>,
    pub base_ref: Option<String>,
    pub started_at: String,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationCommandSummary {
    pub id: String,
    pub command_source: String,
    pub command_ref: Option<String>,
    pub command: String,
    pub cwd: String,
    pub label: Option<String>,
    pub category: String,
    pub reason: Option<String>,
    pub related_files: Vec<String>,
    pub cache_decision: String,
    pub status: String,
    pub exit_code: Option<i32>,
    pub duration_ms: Option<u64>,
    pub stdout_snippet: Option<String>,
    pub stderr_snippet: Option<String>,
    pub stdout_log_path: Option<String>,
    pub stderr_log_path: Option<String>,
    pub created_at: String,
}

pub struct TaskValidationService;

impl TaskValidationService {
    pub async fn run_task_validation(
        state: &AppState,
        request: RunTaskValidationRequest,
    ) -> AppResult<TaskValidationSummary> {
        let task_id = TaskId::from_string(request.task_id.clone());
        let task = state
            .task_repo
            .get_by_id(&task_id)
            .await?
            .ok_or_else(|| AppError::TaskNotFound(task_id.as_str().to_string()))?;
        let project = state
            .project_repo
            .get_by_id(&task.project_id)
            .await?
            .ok_or_else(|| AppError::ProjectNotFound(task.project_id.as_str().to_string()))?;
        let settings = state
            .review_settings_repo
            .get_settings()
            .await
            .map_err(|e| {
                AppError::Infrastructure(format!("failed to read review settings: {e}"))
            })?;

        reject_disallowed_runner(&request.caller_agent, settings.run_task_validations)?;

        let repo_path = resolve_validation_repo_path(&task, &project)?;
        let current_head_sha = GitService::get_head_sha(&repo_path).await.ok();
        let status_episode_entered_at = latest_execution_episode_entered_at(state, &task_id).await;
        let base_ref = resolve_validation_base_ref(state, &task, &project).await;
        let purpose = ValidationPurpose::parse(request.purpose.as_deref().unwrap_or("final"));
        let context_type =
            ValidationContextType::parse(request.context_type.as_deref().unwrap_or("execution"));
        let mode = ValidationRunMode::parse(request.mode.as_deref().unwrap_or("reuse_or_run"));
        let run_id = uuid::Uuid::new_v4().to_string();
        let started_at = Utc::now();

        let run = ValidationRun {
            id: run_id.clone(),
            task_id: task_id.clone(),
            project_id: task.project_id.clone(),
            purpose,
            context_type,
            requested_by_agent: request.caller_agent.clone(),
            status: ValidationRunStatus::Running,
            mode,
            policy_enabled: settings.run_task_validations,
            head_sha: current_head_sha.clone(),
            base_ref,
            analysis_fingerprint: request.analysis_fingerprint.clone(),
            status_episode_entered_at,
            started_at,
            completed_at: None,
        };
        state.validation_run_repo.create_run(&run).await?;

        let prior_results = state
            .validation_run_repo
            .list_command_results_for_task(&task_id)
            .await
            .unwrap_or_default();
        let mut summaries = Vec::new();

        for command in request.commands {
            let result = build_or_run_command(
                &run,
                &task,
                &project,
                &repo_path,
                current_head_sha.as_deref(),
                request.analysis_fingerprint.as_deref(),
                status_episode_entered_at,
                mode,
                command,
                &prior_results,
            )
            .await?;
            state
                .validation_run_repo
                .add_command_result(&result)
                .await?;
            summaries.push(ValidationCommandSummary::from(&result));
        }

        let completed_at = Utc::now();
        let status = aggregate_run_status(&summaries);
        state
            .validation_run_repo
            .update_run_status(&run_id, status, Some(completed_at))
            .await?;

        let mut completed_run = run;
        completed_run.status = status;
        completed_run.completed_at = Some(completed_at);

        Ok(TaskValidationSummary {
            task_id: task.id.as_str().to_string(),
            project_id: task.project_id.as_str().to_string(),
            policy_enabled: settings.run_task_validations,
            latest_run: Some(ValidationRunSummary::from(&completed_run)),
            commands: summaries,
            legacy_validation_cache: legacy_validation_cache(
                &task,
                current_head_sha.as_deref(),
                status_episode_entered_at,
            ),
            disabled_reason: None,
        })
    }

    pub async fn get_task_validation_summary(
        state: &AppState,
        task_id: &TaskId,
    ) -> AppResult<TaskValidationSummary> {
        let task = state
            .task_repo
            .get_by_id(task_id)
            .await?
            .ok_or_else(|| AppError::TaskNotFound(task_id.as_str().to_string()))?;
        let project_id = task.project_id.clone();
        let settings = state
            .review_settings_repo
            .get_settings()
            .await
            .map_err(|e| {
                AppError::Infrastructure(format!("failed to read review settings: {e}"))
            })?;
        let repo_path = state
            .project_repo
            .get_by_id(&project_id)
            .await?
            .and_then(|project| resolve_validation_repo_path(&task, &project).ok());
        let current_head_sha = match repo_path {
            Some(path) => GitService::get_head_sha(&path).await.ok(),
            None => None,
        };
        let status_episode_entered_at = latest_execution_episode_entered_at(state, task_id).await;
        let latest = state
            .validation_run_repo
            .latest_run_with_results_for_task(task_id)
            .await?;

        let (latest_run, commands) = match latest {
            Some(with_results) => (
                Some(ValidationRunSummary::from(&with_results.run)),
                with_results
                    .commands
                    .iter()
                    .map(ValidationCommandSummary::from)
                    .collect(),
            ),
            None => (None, Vec::new()),
        };

        Ok(TaskValidationSummary {
            task_id: task.id.as_str().to_string(),
            project_id: project_id.as_str().to_string(),
            policy_enabled: settings.run_task_validations,
            latest_run,
            commands,
            legacy_validation_cache: legacy_validation_cache(
                &task,
                current_head_sha.as_deref(),
                status_episode_entered_at,
            ),
            disabled_reason: (!settings.run_task_validations)
                .then(|| "Run Task Validations is disabled in Review Policy".to_string()),
        })
    }
}

async fn build_or_run_command(
    run: &ValidationRun,
    task: &Task,
    project: &Project,
    repo_path: &Path,
    head_sha: Option<&str>,
    analysis_fingerprint: Option<&str>,
    status_episode_entered_at: Option<DateTime<Utc>>,
    mode: ValidationRunMode,
    request: ValidationCommandRequest,
    prior_results: &[ValidationCommandResult],
) -> AppResult<ValidationCommandResult> {
    let command = normalize_command(&request.command)?;
    let cwd = resolve_command_cwd(repo_path, request.cwd.as_deref())?;
    let category =
        ValidationCommandCategory::parse(request.category.as_deref().unwrap_or("test"));
    let command_source = match request.source.as_deref() {
        Some("project_analysis_ref") => ValidationCommandSource::ProjectAnalysisRef,
        _ if request.command_ref.is_some() => ValidationCommandSource::ProjectAnalysisRef,
        _ => ValidationCommandSource::AgentSelected,
    };
    let cache_key = validation_cache_key(
        task,
        project,
        cwd.as_path(),
        &command,
        category,
        head_sha,
        analysis_fingerprint,
        status_episode_entered_at,
    );

    if mode == ValidationRunMode::ReuseOrRun && status_episode_entered_at.is_some() {
        if let Some(cached) = prior_results
            .iter()
            .find(|result| result.cache_key == cache_key && result.status.is_success_like())
        {
            return Ok(cached_as_command_result(run, cached));
        }
    }

    if mode == ValidationRunMode::DryRun {
        return Ok(skipped_command_result(
            run,
            task,
            &command,
            &cwd,
            command_source,
            request,
            category,
            cache_key,
            head_sha,
            analysis_fingerprint,
            status_episode_entered_at,
        ));
    }

    let cache_decision = if mode == ValidationRunMode::Force {
        ValidationCacheDecision::Forced
    } else if prior_results
        .iter()
        .any(|result| result.command == command && result.cwd == cwd.to_string_lossy())
    {
        ValidationCacheDecision::Stale
    } else {
        ValidationCacheDecision::Ran
    };

    let command_id = uuid::Uuid::new_v4().to_string();
    let started = Instant::now();
    let execution = execute_shell_command(&command, &cwd, DEFAULT_VALIDATION_TIMEOUT_SECS).await;
    let duration_ms = started.elapsed().as_millis() as u64;
    let created_at = Utc::now();
    let shell_path = resolve_shell_cli_path().to_string_lossy().to_string();

    let (status, exit_code, stdout, stderr) = match execution {
        Ok(output) => {
            let status = if output.status.success() {
                ValidationCommandStatus::Passed
            } else {
                ValidationCommandStatus::Failed
            };
            (
                status,
                output.status.code(),
                String::from_utf8_lossy(&output.stdout).to_string(),
                String::from_utf8_lossy(&output.stderr).to_string(),
            )
        }
        Err(error) => (
            ValidationCommandStatus::Error,
            None,
            String::new(),
            error.to_string(),
        ),
    };

    let (stdout_log_path, stderr_log_path) = write_command_logs(
        task.id.as_str(),
        run.id.as_str(),
        &command_id,
        &stdout,
        &stderr,
    );

    Ok(ValidationCommandResult {
        id: command_id,
        validation_run_id: run.id.clone(),
        task_id: task.id.clone(),
        project_id: task.project_id.clone(),
        command_source,
        command_ref: request.command_ref,
        command,
        cwd: cwd.to_string_lossy().to_string(),
        label: request.label,
        category,
        reason: request.reason,
        related_files: sanitize_related_files(request.related_files),
        cache_key,
        cache_decision,
        status,
        exit_code,
        duration_ms: Some(duration_ms),
        stdout_snippet: (!stdout.is_empty())
            .then(|| truncate_str(&stdout, OUTPUT_SNIPPET_BYTES).to_string()),
        stderr_snippet: (!stderr.is_empty())
            .then(|| truncate_str(&stderr, OUTPUT_SNIPPET_BYTES).to_string()),
        stdout_log_path,
        stderr_log_path,
        launcher_kind: Some("production_shell_resolver".to_string()),
        resolved_shell_path: Some(shell_path),
        head_sha: head_sha.map(ToString::to_string),
        analysis_fingerprint: analysis_fingerprint.map(ToString::to_string),
        status_episode_entered_at,
        created_at,
    })
}

async fn execute_shell_command(
    command_text: &str,
    cwd: &Path,
    timeout_secs: u64,
) -> AppResult<std::process::Output> {
    let mut command = tokio::process::Command::new(resolve_shell_cli_path());
    crate::infrastructure::login_shell_env::apply_to(&mut command);
    command.env("PATH", agent_subprocess_env_path());
    prepend_resolved_node_bin_to_path(command.as_std_mut());

    let mut child = command
        .arg("-c")
        .arg(command_text)
        .current_dir(cwd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| {
            AppError::Infrastructure(format!("failed to spawn validation command: {e}"))
        })?;
    let pid = child.id();
    let stdout_handle = child.stdout.take();
    let stderr_handle = child.stderr.take();

    let stdout_fut = async {
        let mut buf = Vec::new();
        if let Some(mut out) = stdout_handle {
            let _ = out.read_to_end(&mut buf).await;
        }
        buf
    };
    let stderr_fut = async {
        let mut buf = Vec::new();
        if let Some(mut err) = stderr_handle {
            let _ = err.read_to_end(&mut buf).await;
        }
        buf
    };

    tokio::select! {
        (status, stdout, stderr) = async { tokio::join!(child.wait(), stdout_fut, stderr_fut) } => {
            let status = status.map_err(|e| AppError::Infrastructure(format!("failed to wait for validation command: {e}")))?;
            Ok(std::process::Output { status, stdout, stderr })
        }
        _ = tokio::time::sleep(Duration::from_secs(timeout_secs)) => {
            if let Some(pid) = pid {
                crate::domain::services::kill_process(pid);
            }
            Err(AppError::Infrastructure(format!(
                "validation command timed out after {timeout_secs}s"
            )))
        }
    }
}

fn reject_disallowed_runner(caller_agent: &Option<String>, policy_enabled: bool) -> AppResult<()> {
    let caller = caller_agent.as_deref().unwrap_or("");
    if caller.contains("reviewer") {
        return Err(AppError::ExecutionBlocked(
            "Review agents cannot run task validation".to_string(),
        ));
    }
    if !policy_enabled {
        return Err(AppError::ExecutionBlocked(
            "Run Task Validations is disabled in Review Policy".to_string(),
        ));
    }
    Ok(())
}

fn resolve_validation_repo_path(task: &Task, project: &Project) -> AppResult<PathBuf> {
    let path = task
        .worktree_path
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(&project.working_directory));
    let path = validate_absolute_non_root_path(&path, "validation worktree")?;
    std::fs::canonicalize(&path).map_err(|e| {
        AppError::Validation(format!(
            "validation worktree path is not available: {} ({e})",
            path.display()
        ))
    })
}

fn resolve_command_cwd(repo_path: &Path, cwd: Option<&str>) -> AppResult<PathBuf> {
    let raw = cwd.unwrap_or(".").trim();
    if raw.is_empty() {
        return Err(AppError::Validation(
            "validation cwd must not be empty".to_string(),
        ));
    }
    let candidate = Path::new(raw);
    let joined = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        repo_path.join(candidate)
    };
    let resolved = std::fs::canonicalize(&joined).map_err(|e| {
        AppError::Validation(format!(
            "validation cwd is not available: {} ({e})",
            joined.display()
        ))
    })?;
    if !resolved.starts_with(repo_path) {
        return Err(AppError::Validation(format!(
            "validation cwd must stay inside task worktree: {}",
            resolved.display()
        )));
    }
    Ok(resolved)
}

async fn resolve_validation_base_ref(
    state: &AppState,
    task: &Task,
    project: &Project,
) -> Option<String> {
    if let Some(exec_plan_id) = &task.execution_plan_id {
        if let Ok(Some(plan_branch)) = state
            .plan_branch_repo
            .get_by_execution_plan_id(exec_plan_id)
            .await
        {
            return Some(plan_branch.branch_name);
        }
    }
    if let Some(session_id) = &task.ideation_session_id {
        if let Ok(Some(plan_branch)) = state.plan_branch_repo.get_by_session_id(session_id).await {
            return Some(plan_branch.branch_name);
        }
    }
    Some(project.base_branch_or_default().to_string())
}

async fn latest_execution_episode_entered_at(
    state: &AppState,
    task_id: &TaskId,
) -> Option<DateTime<Utc>> {
    let executing = state
        .task_repo
        .get_status_last_entered_at(task_id, InternalStatus::Executing)
        .await
        .ok()
        .flatten();
    let re_executing = state
        .task_repo
        .get_status_last_entered_at(task_id, InternalStatus::ReExecuting)
        .await
        .ok()
        .flatten();
    executing.into_iter().chain(re_executing).max()
}

fn normalize_command(command: &str) -> AppResult<String> {
    let command = command.trim();
    if command.is_empty() {
        return Err(AppError::Validation(
            "validation command must not be empty".to_string(),
        ));
    }
    Ok(command.to_string())
}

fn validation_cache_key(
    task: &Task,
    project: &Project,
    cwd: &Path,
    command: &str,
    category: ValidationCommandCategory,
    head_sha: Option<&str>,
    analysis_fingerprint: Option<&str>,
    status_episode_entered_at: Option<DateTime<Utc>>,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(task.id.as_str().as_bytes());
    hasher.update(project.id.as_str().as_bytes());
    hasher.update(head_sha.unwrap_or("unknown-head").as_bytes());
    hasher.update(cwd.to_string_lossy().as_bytes());
    hasher.update(command.as_bytes());
    hasher.update(category.as_str().as_bytes());
    hasher.update(analysis_fingerprint.unwrap_or("no-analysis").as_bytes());
    hasher.update(
        status_episode_entered_at
            .map(|dt| dt.to_rfc3339())
            .unwrap_or_else(|| "unknown-episode".to_string())
            .as_bytes(),
    );
    format!("{:x}", hasher.finalize())
}

fn cached_as_command_result(
    run: &ValidationRun,
    cached: &ValidationCommandResult,
) -> ValidationCommandResult {
    let mut result = cached.clone();
    result.id = uuid::Uuid::new_v4().to_string();
    result.validation_run_id = run.id.clone();
    result.cache_decision = ValidationCacheDecision::Cached;
    result.status = ValidationCommandStatus::Cached;
    result.created_at = Utc::now();
    result
}

#[allow(clippy::too_many_arguments)]
fn skipped_command_result(
    run: &ValidationRun,
    task: &Task,
    command: &str,
    cwd: &Path,
    command_source: ValidationCommandSource,
    request: ValidationCommandRequest,
    category: ValidationCommandCategory,
    cache_key: String,
    head_sha: Option<&str>,
    analysis_fingerprint: Option<&str>,
    status_episode_entered_at: Option<DateTime<Utc>>,
) -> ValidationCommandResult {
    ValidationCommandResult {
        id: uuid::Uuid::new_v4().to_string(),
        validation_run_id: run.id.clone(),
        task_id: task.id.clone(),
        project_id: task.project_id.clone(),
        command_source,
        command_ref: request.command_ref,
        command: command.to_string(),
        cwd: cwd.to_string_lossy().to_string(),
        label: request.label,
        category,
        reason: request.reason,
        related_files: sanitize_related_files(request.related_files),
        cache_key,
        cache_decision: ValidationCacheDecision::Skipped,
        status: ValidationCommandStatus::Skipped,
        exit_code: None,
        duration_ms: Some(0),
        stdout_snippet: None,
        stderr_snippet: None,
        stdout_log_path: None,
        stderr_log_path: None,
        launcher_kind: Some("production_shell_resolver".to_string()),
        resolved_shell_path: Some(resolve_shell_cli_path().to_string_lossy().to_string()),
        head_sha: head_sha.map(ToString::to_string),
        analysis_fingerprint: analysis_fingerprint.map(ToString::to_string),
        status_episode_entered_at,
        created_at: Utc::now(),
    }
}

fn sanitize_related_files(files: Vec<String>) -> Vec<String> {
    files
        .into_iter()
        .map(|file| file.trim().to_string())
        .filter(|file| {
            !file.is_empty()
                && !file.starts_with('/')
                && !file.split('/').any(|part| part == ".." || part.is_empty())
        })
        .take(100)
        .collect()
}

fn aggregate_run_status(commands: &[ValidationCommandSummary]) -> ValidationRunStatus {
    if commands.is_empty() {
        return ValidationRunStatus::Skipped;
    }
    if commands
        .iter()
        .any(|command| command.status == "failed" || command.status == "error")
    {
        return ValidationRunStatus::Failed;
    }
    if commands.iter().all(|command| command.status == "skipped") {
        return ValidationRunStatus::Skipped;
    }
    ValidationRunStatus::Passed
}

fn legacy_validation_cache(
    task: &Task,
    current_head_sha: Option<&str>,
    episode_entered_at: Option<DateTime<Utc>>,
) -> Option<ValidationCacheData> {
    let cache = ValidationCacheMetadata::from_task_metadata(task.metadata.as_deref())
        .ok()
        .flatten()?;
    let current_head_sha = current_head_sha?;
    let (validation_hint, hint_message) = crate::http_server::helpers::compute_validation_hint(
        &cache,
        current_head_sha,
        episode_entered_at,
    );
    Some(ValidationCacheData {
        commit_sha: cache.commit_sha,
        tests_ran: cache.tests_ran,
        tests_passed: cache.tests_passed,
        test_summary: cache.test_summary,
        captured_at: cache.captured_at,
        validation_hint,
        hint_message,
    })
}

fn write_command_logs(
    task_id: &str,
    run_id: &str,
    command_id: &str,
    stdout: &str,
    stderr: &str,
) -> (Option<String>, Option<String>) {
    let stdout_path = write_command_log(task_id, run_id, command_id, "stdout", stdout);
    let stderr_path = write_command_log(task_id, run_id, command_id, "stderr", stderr);
    (stdout_path, stderr_path)
}

fn write_command_log(
    task_id: &str,
    run_id: &str,
    command_id: &str,
    stream: &str,
    content: &str,
) -> Option<String> {
    if content.is_empty() {
        return None;
    }
    let path = crate::utils::runtime_log_paths::task_validation_command_log_file(
        task_id, run_id, command_id, stream,
    );
    if let Some(parent) = path.parent() {
        // The path is derived from fixed RalphX runtime log roots plus hashed IDs.
        // codeql[rust/path-injection]
        if let Err(error) = std::fs::create_dir_all(parent) {
            tracing::warn!(%error, "Failed to create task validation log directory");
            return None;
        }
    }
    // The path is derived from fixed RalphX runtime log roots plus hashed IDs.
    // codeql[rust/path-injection]
    match std::fs::write(&path, content) {
        Ok(()) => Some(path.to_string_lossy().to_string()),
        Err(error) => {
            tracing::warn!(%error, "Failed to write task validation command log");
            None
        }
    }
}

impl From<&ValidationRun> for ValidationRunSummary {
    fn from(run: &ValidationRun) -> Self {
        Self {
            id: run.id.clone(),
            purpose: run.purpose.as_str().to_string(),
            context_type: run.context_type.as_str().to_string(),
            requested_by_agent: run.requested_by_agent.clone(),
            status: run.status.as_str().to_string(),
            mode: run.mode.as_str().to_string(),
            policy_enabled: run.policy_enabled,
            head_sha: run.head_sha.clone(),
            head_short_sha: run
                .head_sha
                .as_ref()
                .map(|sha| sha.chars().take(8).collect::<String>()),
            base_ref: run.base_ref.clone(),
            started_at: run.started_at.to_rfc3339(),
            completed_at: run.completed_at.map(|dt| dt.to_rfc3339()),
        }
    }
}

impl From<&ValidationCommandResult> for ValidationCommandSummary {
    fn from(result: &ValidationCommandResult) -> Self {
        Self {
            id: result.id.clone(),
            command_source: result.command_source.as_str().to_string(),
            command_ref: result.command_ref.clone(),
            command: result.command.clone(),
            cwd: result.cwd.clone(),
            label: result.label.clone(),
            category: result.category.as_str().to_string(),
            reason: result.reason.clone(),
            related_files: result.related_files.clone(),
            cache_decision: result.cache_decision.as_str().to_string(),
            status: result.status.as_str().to_string(),
            exit_code: result.exit_code,
            duration_ms: result.duration_ms,
            stdout_snippet: result.stdout_snippet.clone(),
            stderr_snippet: result.stderr_snippet.clone(),
            stdout_log_path: result.stdout_log_path.clone(),
            stderr_log_path: result.stderr_log_path.clone(),
            created_at: result.created_at.to_rfc3339(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::review::ReviewSettings;

    async fn seeded_state() -> (AppState, tempfile::TempDir, TaskId) {
        let state = AppState::new_test();
        let temp_dir = tempfile::tempdir().expect("temp project dir");
        let project = Project::new(
            "Validation Test".to_string(),
            temp_dir.path().to_string_lossy().to_string(),
        );
        let project = state
            .project_repo
            .create(project)
            .await
            .expect("project should be created");
        let task = Task::new(project.id.clone(), "Validate runner".to_string());
        let task_id = task.id.clone();
        state
            .task_repo
            .create(task)
            .await
            .expect("task should be created");
        (state, temp_dir, task_id)
    }

    fn request(task_id: &TaskId, caller_agent: &str) -> RunTaskValidationRequest {
        RunTaskValidationRequest {
            task_id: task_id.as_str().to_string(),
            purpose: Some("final".to_string()),
            mode: Some("force".to_string()),
            context_type: Some("execution".to_string()),
            caller_agent: Some(caller_agent.to_string()),
            analysis_fingerprint: None,
            commands: vec![ValidationCommandRequest {
                command: "echo should-not-run".to_string(),
                cwd: None,
                label: Some("Should not run".to_string()),
                category: Some("test".to_string()),
                reason: Some("gate test".to_string()),
                related_files: Vec::new(),
                command_ref: None,
                source: None,
            }],
        }
    }

    #[tokio::test]
    async fn run_task_validation_rejects_when_policy_disabled_before_creating_run() {
        let (state, _temp_dir, task_id) = seeded_state().await;
        state
            .review_settings_repo
            .update_settings(&ReviewSettings {
                run_task_validations: false,
                ..ReviewSettings::default()
            })
            .await
            .expect("settings should update");

        let error = TaskValidationService::run_task_validation(
            &state,
            request(&task_id, "ralphx-execution-worker"),
        )
        .await
        .expect_err("disabled policy should reject validation");

        assert!(
            matches!(error, AppError::ExecutionBlocked(ref message) if message.contains("disabled")),
            "expected disabled policy block, got {error:?}"
        );
        assert!(state
            .validation_run_repo
            .latest_run_with_results_for_task(&task_id)
            .await
            .expect("validation run lookup should succeed")
            .is_none());
    }

    #[tokio::test]
    async fn run_task_validation_rejects_reviewers_before_creating_run() {
        let (state, _temp_dir, task_id) = seeded_state().await;

        let error = TaskValidationService::run_task_validation(
            &state,
            request(&task_id, "ralphx-execution-reviewer"),
        )
        .await
        .expect_err("reviewers should not run validation");

        assert!(
            matches!(error, AppError::ExecutionBlocked(ref message) if message.contains("Review agents")),
            "expected reviewer policy block, got {error:?}"
        );
        assert!(state
            .validation_run_repo
            .latest_run_with_results_for_task(&task_id)
            .await
            .expect("validation run lookup should succeed")
            .is_none());
    }
}
