use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::AppHandle;

use crate::application::git_service::GitService;
use crate::application::task_diff_base::resolve_task_diff_base;
use crate::application::validation_events::{
    emit_task_validation_event, read_stream_with_events, TaskValidationEventPayload,
    ValidationCommandEventContext,
};
use crate::application::AppState;
use crate::domain::entities::{
    InternalStatus, Project, Task, TaskId, ValidationCacheData, ValidationCacheDecision,
    ValidationCacheMetadata, ValidationCommandCategory, ValidationCommandResult,
    ValidationCommandSource, ValidationCommandStatus, ValidationContextType, ValidationPurpose,
    ValidationRun, ValidationRunMode, ValidationRunStatus,
};
use crate::error::{AppError, AppResult};
use crate::infrastructure::tool_paths::{
    agent_subprocess_env_path, ensure_resolved_node_bin_in_path, resolve_shell_cli_path,
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
    pub current_for_head: bool,
    pub current_for_execution_episode: bool,
    pub review_evidence_eligible: bool,
    pub ineligible_reason: Option<String>,
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

#[derive(Debug, Clone)]
struct ValidationEvidenceClassification {
    current_for_head: bool,
    current_for_execution_episode: bool,
    review_evidence_eligible: bool,
    ineligible_reason: Option<&'static str>,
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
        let start_content_fingerprint = GitService::working_tree_fingerprint(&repo_path).await.ok();
        let status_episode_entered_at = latest_execution_episode_entered_at(state, &task_id).await;
        let task_diff_base = resolve_task_diff_base(state, &task, &project).await;
        let base_ref = Some(task_diff_base.effective_base_ref.clone());
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
            start_content_fingerprint,
            validated_content_fingerprint: None,
            promoted_commit_sha: None,
            base_ref: base_ref.clone(),
            analysis_fingerprint: request.analysis_fingerprint.clone(),
            status_episode_entered_at,
            started_at,
            completed_at: None,
        };
        state.validation_run_repo.create_run(&run).await?;
        emit_task_validation_event(state, &TaskValidationEventPayload::run_started(&run));

        let prior_results = state
            .validation_run_repo
            .list_command_results_for_task(&task_id)
            .await
            .unwrap_or_default();
        let mut summaries = Vec::new();

        for command in request.commands {
            let result = match build_or_run_command(
                state,
                &run,
                &task,
                &project,
                &repo_path,
                current_head_sha.as_deref(),
                base_ref.as_deref(),
                request.analysis_fingerprint.as_deref(),
                status_episode_entered_at,
                mode,
                command,
                &prior_results,
            )
            .await
            {
                Ok(result) => result,
                Err(error) => {
                    settle_validation_run_after_error(state, &run).await;
                    return Err(error);
                }
            };
            if let Err(error) = state.validation_run_repo.add_command_result(&result).await {
                settle_validation_run_after_error(state, &run).await;
                return Err(error);
            }
            emit_task_validation_event(
                state,
                &TaskValidationEventPayload::command_completed(&run, &result),
            );
            summaries.push(ValidationCommandSummary::from(&result));
        }

        let completed_at = Utc::now();
        let status = aggregate_run_status(&summaries);
        let validated_content_fingerprint = if status == ValidationRunStatus::Passed {
            GitService::working_tree_fingerprint(&repo_path).await.ok()
        } else {
            None
        };
        state
            .validation_run_repo
            .update_run_status(&run_id, status, Some(completed_at))
            .await?;
        state
            .validation_run_repo
            .record_validated_content_fingerprint(&run_id, validated_content_fingerprint.clone())
            .await?;

        let mut completed_run = run;
        completed_run.status = status;
        completed_run.completed_at = Some(completed_at);
        completed_run.validated_content_fingerprint = validated_content_fingerprint;
        emit_task_validation_event(
            state,
            &TaskValidationEventPayload::run_completed(&completed_run),
        );

        Ok(TaskValidationSummary {
            task_id: task.id.as_str().to_string(),
            project_id: task.project_id.as_str().to_string(),
            policy_enabled: settings.run_task_validations,
            latest_run: Some(ValidationRunSummary::from_run_with_evidence(
                &completed_run,
                current_head_sha.as_deref(),
                status_episode_entered_at,
                &summaries,
            )),
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
            Some(with_results) => {
                let commands = with_results
                    .commands
                    .iter()
                    .map(ValidationCommandSummary::from)
                    .collect::<Vec<_>>();
                let latest_run = Some(ValidationRunSummary::from_run_with_evidence(
                    &with_results.run,
                    current_head_sha.as_deref(),
                    status_episode_entered_at,
                    &commands,
                ));
                (latest_run, commands)
            }
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

    pub async fn promote_matching_validation_to_commit(
        state: &AppState,
        task_id: &TaskId,
        repo_path: &Path,
        commit_sha: &str,
    ) -> AppResult<bool> {
        let Some(with_results) = state
            .validation_run_repo
            .latest_non_baseline_run_with_results_for_task(task_id)
            .await?
        else {
            return Ok(false);
        };
        let fingerprint = GitService::working_tree_fingerprint(repo_path).await?;
        let commands = with_results
            .commands
            .iter()
            .map(ValidationCommandSummary::from)
            .collect::<Vec<_>>();
        let passed_test_run = with_results.run.status == ValidationRunStatus::Passed
            && commands.iter().any(|command| {
                command.category == ValidationCommandCategory::Test.as_str()
                    && validation_command_status_success_like(&command.status)
            });
        if !passed_test_run
            || with_results.run.validated_content_fingerprint.as_deref()
                != Some(fingerprint.as_str())
        {
            return Ok(false);
        }
        state
            .validation_run_repo
            .promote_run_to_commit(&with_results.run.id, commit_sha)
            .await?;
        Ok(true)
    }
}

async fn settle_validation_run_after_error(state: &AppState, run: &ValidationRun) {
    let completed_at = Utc::now();
    if let Err(error) = state
        .validation_run_repo
        .update_run_status(&run.id, ValidationRunStatus::Error, Some(completed_at))
        .await
    {
        tracing::warn!(
            validation_run_id = %run.id,
            error = %error,
            "Failed to settle validation run after infrastructure error"
        );
        return;
    }

    let mut completed_run = run.clone();
    completed_run.status = ValidationRunStatus::Error;
    completed_run.completed_at = Some(completed_at);
    emit_task_validation_event(
        state,
        &TaskValidationEventPayload::run_completed(&completed_run),
    );
}

async fn build_or_run_command(
    state: &AppState,
    run: &ValidationRun,
    task: &Task,
    project: &Project,
    repo_path: &Path,
    head_sha: Option<&str>,
    base_ref: Option<&str>,
    analysis_fingerprint: Option<&str>,
    status_episode_entered_at: Option<DateTime<Utc>>,
    mode: ValidationRunMode,
    request: ValidationCommandRequest,
    prior_results: &[ValidationCommandResult],
) -> AppResult<ValidationCommandResult> {
    let command = normalize_command(&request.command)?;
    let cwd = resolve_command_cwd(repo_path, request.cwd.as_deref())?;
    let category = ValidationCommandCategory::parse(request.category.as_deref().unwrap_or("test"));
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
        base_ref,
        analysis_fingerprint,
        status_episode_entered_at,
    );
    let command_id = uuid::Uuid::new_v4().to_string();

    if mode == ValidationRunMode::ReuseOrRun && status_episode_entered_at.is_some() {
        if let Some(cached) = prior_results
            .iter()
            .find(|result| result.cache_key == cache_key && result.status.is_success_like())
        {
            return Ok(cached_as_command_result(run, cached, command_id));
        }
    }

    if mode == ValidationRunMode::DryRun {
        return Ok(skipped_command_result(
            run,
            task,
            command_id,
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

    let started = Instant::now();
    let command_started_at = Utc::now();
    let event_context = ValidationCommandEventContext::from_request(
        run,
        &command_id,
        command_source,
        &request,
        &command,
        &cwd,
        category,
        cache_decision,
        command_started_at,
    );
    emit_task_validation_event(
        state,
        &TaskValidationEventPayload::command_started(&event_context),
    );
    let execution = execute_shell_command(
        &command,
        &cwd,
        DEFAULT_VALIDATION_TIMEOUT_SECS,
        state.app_handle.clone(),
        Some(event_context),
    )
    .await;
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
    app_handle: Option<AppHandle>,
    event_context: Option<ValidationCommandEventContext>,
) -> AppResult<std::process::Output> {
    let mut command = tokio::process::Command::new(resolve_shell_cli_path());
    configure_validation_shell_command(&mut command);

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

    let stdout_fut = read_stream_with_events(
        stdout_handle,
        app_handle.clone(),
        event_context.clone(),
        "stdout",
    );
    let stderr_fut = read_stream_with_events(stderr_handle, app_handle, event_context, "stderr");

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

fn configure_validation_shell_command(command: &mut tokio::process::Command) {
    crate::infrastructure::login_shell_env::apply_to(command);
    command.env("PATH", agent_subprocess_env_path());
    ensure_resolved_node_bin_in_path(command.as_std_mut());
}

#[cfg(test)]
pub(crate) fn configure_validation_shell_command_for_test(command: &mut tokio::process::Command) {
    configure_validation_shell_command(command);
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
    base_ref: Option<&str>,
    analysis_fingerprint: Option<&str>,
    status_episode_entered_at: Option<DateTime<Utc>>,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(task.id.as_str().as_bytes());
    hasher.update(project.id.as_str().as_bytes());
    hasher.update(head_sha.unwrap_or("unknown-head").as_bytes());
    hasher.update(base_ref.unwrap_or("unknown-base").as_bytes());
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
    command_id: String,
) -> ValidationCommandResult {
    let mut result = cached.clone();
    result.id = command_id;
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
    command_id: String,
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
        id: command_id,
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

fn classify_validation_evidence(
    run: &ValidationRun,
    current_head_sha: Option<&str>,
    episode_entered_at: Option<DateTime<Utc>>,
    commands: &[ValidationCommandSummary],
) -> ValidationEvidenceClassification {
    let current_for_head = current_head_sha
        .zip(
            run.promoted_commit_sha
                .as_deref()
                .or(run.head_sha.as_deref()),
        )
        .map(|(current, captured)| current == captured)
        .unwrap_or(false);
    let current_for_execution_episode = match (
        run.status_episode_entered_at.as_ref(),
        episode_entered_at.as_ref(),
    ) {
        (Some(captured), Some(current)) => captured >= current,
        _ => false,
    };
    let has_test_commands = commands
        .iter()
        .any(|command| command.category == ValidationCommandCategory::Test.as_str());
    let commands_successful = !commands.is_empty()
        && commands
            .iter()
            .all(|command| validation_command_status_success_like(&command.status));

    let ineligible_reason = if run.purpose == ValidationPurpose::Baseline {
        Some("baseline_only")
    } else if !current_for_head {
        Some("stale_head")
    } else if !current_for_execution_episode {
        Some("stale_episode")
    } else if run.status != ValidationRunStatus::Passed || !commands_successful {
        Some("failed")
    } else if !has_test_commands {
        Some("no_test_commands")
    } else {
        None
    };

    ValidationEvidenceClassification {
        current_for_head,
        current_for_execution_episode,
        review_evidence_eligible: ineligible_reason.is_none(),
        ineligible_reason,
    }
}

pub(crate) fn validation_run_proves_current_completion(
    evidence: &crate::domain::entities::ValidationRunWithResults,
    current_head_sha: &str,
    episode_entered_at: DateTime<Utc>,
) -> bool {
    let run = &evidence.run;
    run.purpose != ValidationPurpose::Baseline
        && run.status == ValidationRunStatus::Passed
        && run.promoted_commit_sha.as_deref() == Some(current_head_sha)
        && run
            .status_episode_entered_at
            .is_some_and(|captured| captured >= episode_entered_at)
        && !evidence.commands.is_empty()
        && evidence
            .commands
            .iter()
            .all(|command| command.status.is_success_like())
        && evidence
            .commands
            .iter()
            .any(|command| command.category == ValidationCommandCategory::Test)
}

fn validation_command_status_success_like(status: &str) -> bool {
    matches!(
        ValidationCommandStatus::parse(status),
        ValidationCommandStatus::Passed | ValidationCommandStatus::Cached
    )
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

impl ValidationRunSummary {
    fn from_run_with_evidence(
        run: &ValidationRun,
        current_head_sha: Option<&str>,
        episode_entered_at: Option<DateTime<Utc>>,
        commands: &[ValidationCommandSummary],
    ) -> Self {
        let classification =
            classify_validation_evidence(run, current_head_sha, episode_entered_at, commands);
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
            current_for_head: classification.current_for_head,
            current_for_execution_episode: classification.current_for_execution_episode,
            review_evidence_eligible: classification.review_evidence_eligible,
            ineligible_reason: classification.ineligible_reason.map(str::to_string),
        }
    }
}

impl From<&ValidationRun> for ValidationRunSummary {
    fn from(run: &ValidationRun) -> Self {
        Self::from_run_with_evidence(run, None, None, &[])
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
    use crate::application::validation_events::emit_task_validation_event_to_handle;
    use crate::domain::review::ReviewSettings;

    async fn seeded_state() -> (AppState, tempfile::TempDir, TaskId) {
        let state = AppState::new_test();
        let temp_dir = tempfile::tempdir().expect("temp project dir");
        let git = |args: &[&str]| {
            let output = std::process::Command::new("git")
                .args(args)
                .current_dir(temp_dir.path())
                .env("GIT_AUTHOR_NAME", "Validation Test")
                .env("GIT_AUTHOR_EMAIL", "validation@example.test")
                .env("GIT_COMMITTER_NAME", "Validation Test")
                .env("GIT_COMMITTER_EMAIL", "validation@example.test")
                .output()
                .expect("git should run");
            assert!(
                output.status.success(),
                "git {:?} failed: {}",
                args,
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&["init", "-b", "main"]);
        std::fs::write(temp_dir.path().join("README.md"), "validation fixture")
            .expect("fixture readme should be written");
        git(&["add", "README.md"]);
        git(&["commit", "-m", "initial"]);
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

    fn validation_run_fixture(
        purpose: ValidationPurpose,
        status: ValidationRunStatus,
        head_sha: Option<&str>,
        episode_entered_at: Option<DateTime<Utc>>,
    ) -> ValidationRun {
        let project = Project::new(
            "Evidence project".to_string(),
            "/tmp/evidence-project".to_string(),
        );
        let task = Task::new(project.id.clone(), "Evidence task".to_string());
        ValidationRun {
            id: "run-evidence".to_string(),
            task_id: task.id,
            project_id: project.id,
            purpose,
            context_type: ValidationContextType::Execution,
            requested_by_agent: Some("ralphx-execution-worker".to_string()),
            status,
            mode: ValidationRunMode::Force,
            policy_enabled: true,
            head_sha: head_sha.map(ToString::to_string),
            start_content_fingerprint: None,
            validated_content_fingerprint: None,
            promoted_commit_sha: None,
            base_ref: Some("main".to_string()),
            analysis_fingerprint: None,
            status_episode_entered_at: episode_entered_at,
            started_at: Utc::now(),
            completed_at: Some(Utc::now()),
        }
    }

    fn command_summary(category: &str, status: &str) -> ValidationCommandSummary {
        ValidationCommandSummary {
            id: "command-evidence".to_string(),
            command_source: "agent_selected".to_string(),
            command_ref: None,
            command: "cargo test validation".to_string(),
            cwd: "/tmp/evidence-project".to_string(),
            label: None,
            category: category.to_string(),
            reason: None,
            related_files: Vec::new(),
            cache_decision: "ran".to_string(),
            status: status.to_string(),
            exit_code: Some(0),
            duration_ms: Some(1),
            stdout_snippet: None,
            stderr_snippet: None,
            stdout_log_path: None,
            stderr_log_path: None,
            created_at: Utc::now().to_rfc3339(),
        }
    }

    #[test]
    fn validation_run_summary_marks_baseline_passed_evidence_non_eligible() {
        let episode = Utc::now();
        let run = validation_run_fixture(
            ValidationPurpose::Baseline,
            ValidationRunStatus::Passed,
            Some("abcdef1234567890"),
            Some(episode),
        );
        let commands = vec![command_summary("test", "passed")];

        let summary = ValidationRunSummary::from_run_with_evidence(
            &run,
            Some("abcdef1234567890"),
            Some(episode),
            &commands,
        );

        assert!(summary.current_for_head);
        assert!(summary.current_for_execution_episode);
        assert!(!summary.review_evidence_eligible);
        assert_eq!(summary.ineligible_reason.as_deref(), Some("baseline_only"));
    }

    #[test]
    fn validation_run_summary_marks_current_final_test_evidence_eligible() {
        let episode = Utc::now();
        let run = validation_run_fixture(
            ValidationPurpose::Final,
            ValidationRunStatus::Passed,
            Some("abcdef1234567890"),
            Some(episode),
        );
        let commands = vec![command_summary("test", "passed")];

        let summary = ValidationRunSummary::from_run_with_evidence(
            &run,
            Some("abcdef1234567890"),
            Some(episode),
            &commands,
        );

        assert!(summary.current_for_head);
        assert!(summary.current_for_execution_episode);
        assert!(summary.review_evidence_eligible);
        assert!(summary.ineligible_reason.is_none());
    }

    #[test]
    fn validation_run_summary_marks_stale_head_ineligible() {
        let episode = Utc::now();
        let run = validation_run_fixture(
            ValidationPurpose::Final,
            ValidationRunStatus::Passed,
            Some("oldhead123456789"),
            Some(episode),
        );
        let commands = vec![command_summary("test", "passed")];

        let summary = ValidationRunSummary::from_run_with_evidence(
            &run,
            Some("newhead123456789"),
            Some(episode),
            &commands,
        );

        assert!(!summary.current_for_head);
        assert!(!summary.review_evidence_eligible);
        assert_eq!(summary.ineligible_reason.as_deref(), Some("stale_head"));
    }

    #[test]
    fn validation_run_summary_accepts_matching_promoted_commit() {
        let episode = Utc::now();
        let mut run = validation_run_fixture(
            ValidationPurpose::Final,
            ValidationRunStatus::Passed,
            Some("validation-start-head"),
            Some(episode),
        );
        run.promoted_commit_sha = Some("committed-validated-tree".to_string());
        let commands = vec![command_summary("test", "passed")];

        let summary = ValidationRunSummary::from_run_with_evidence(
            &run,
            Some("committed-validated-tree"),
            Some(episode),
            &commands,
        );

        assert!(summary.current_for_head);
        assert!(summary.review_evidence_eligible);
        assert!(summary.ineligible_reason.is_none());
    }

    #[test]
    fn validation_run_summary_marks_stale_episode_ineligible() {
        let previous_episode = Utc::now();
        let current_episode = previous_episode + chrono::Duration::seconds(1);
        let run = validation_run_fixture(
            ValidationPurpose::Final,
            ValidationRunStatus::Passed,
            Some("abcdef1234567890"),
            Some(previous_episode),
        );
        let commands = vec![command_summary("test", "passed")];

        let summary = ValidationRunSummary::from_run_with_evidence(
            &run,
            Some("abcdef1234567890"),
            Some(current_episode),
            &commands,
        );

        assert!(summary.current_for_head);
        assert!(!summary.current_for_execution_episode);
        assert!(!summary.review_evidence_eligible);
        assert_eq!(summary.ineligible_reason.as_deref(), Some("stale_episode"));
    }

    #[test]
    fn validation_run_summary_marks_failed_command_ineligible() {
        let episode = Utc::now();
        let run = validation_run_fixture(
            ValidationPurpose::Final,
            ValidationRunStatus::Failed,
            Some("abcdef1234567890"),
            Some(episode),
        );
        let commands = vec![command_summary("test", "failed")];

        let summary = ValidationRunSummary::from_run_with_evidence(
            &run,
            Some("abcdef1234567890"),
            Some(episode),
            &commands,
        );

        assert!(!summary.review_evidence_eligible);
        assert_eq!(summary.ineligible_reason.as_deref(), Some("failed"));
    }

    #[test]
    fn validation_run_summary_marks_no_test_commands_ineligible() {
        let episode = Utc::now();
        let run = validation_run_fixture(
            ValidationPurpose::Final,
            ValidationRunStatus::Passed,
            Some("abcdef1234567890"),
            Some(episode),
        );
        let commands = vec![command_summary("lint", "passed")];

        let summary = ValidationRunSummary::from_run_with_evidence(
            &run,
            Some("abcdef1234567890"),
            Some(episode),
            &commands,
        );

        assert!(!summary.review_evidence_eligible);
        assert_eq!(
            summary.ineligible_reason.as_deref(),
            Some("no_test_commands")
        );
    }

    fn event_payload_run() -> (ValidationRun, Task) {
        let project = Project::new(
            "Event project".to_string(),
            "/tmp/event-project".to_string(),
        );
        let task = Task::new(project.id.clone(), "Event task".to_string());
        let run = ValidationRun {
            id: "run-123".to_string(),
            task_id: task.id.clone(),
            project_id: project.id.clone(),
            purpose: ValidationPurpose::Final,
            context_type: ValidationContextType::Execution,
            requested_by_agent: Some("ralphx-execution-worker".to_string()),
            status: ValidationRunStatus::Running,
            mode: ValidationRunMode::Force,
            policy_enabled: true,
            head_sha: Some("abcdef1234567890".to_string()),
            start_content_fingerprint: None,
            validated_content_fingerprint: None,
            promoted_commit_sha: None,
            base_ref: Some("main".to_string()),
            analysis_fingerprint: Some("analysis".to_string()),
            status_episode_entered_at: None,
            started_at: Utc::now(),
            completed_at: None,
        };
        (run, task)
    }

    #[test]
    fn task_validation_command_output_event_serializes_current_command_ids() {
        let (run, _task) = event_payload_run();
        let request = ValidationCommandRequest {
            command: "npm test".to_string(),
            cwd: None,
            label: Some("Unit tests".to_string()),
            category: Some("test".to_string()),
            reason: Some("current run proof".to_string()),
            related_files: Vec::new(),
            command_ref: Some("unit-tests".to_string()),
            source: Some("project_analysis_ref".to_string()),
        };
        let context = ValidationCommandEventContext::from_request(
            &run,
            "command-123",
            ValidationCommandSource::ProjectAnalysisRef,
            &request,
            "npm test",
            Path::new("/tmp/event-project"),
            ValidationCommandCategory::Test,
            ValidationCacheDecision::Ran,
            Utc::now(),
        );

        let payload =
            TaskValidationEventPayload::command_output(&context, "stderr", "failure\n".to_string());
        emit_task_validation_event_to_handle(None, &payload);
        let value = serde_json::to_value(payload).expect("payload should serialize");

        assert_eq!(value["type"], "command_output");
        assert_eq!(value["task_id"], run.task_id.as_str());
        assert_eq!(value["run_id"], "run-123");
        assert_eq!(value["command_id"], "command-123");
        assert_eq!(value["command_ref"], "unit-tests");
        assert_eq!(value["cache_decision"], "ran");
        assert_eq!(value["stream"], "stderr");
        assert_eq!(value["stderr_delta"], "failure\n");
        assert!(value.get("stdout_delta").is_none());
    }

    #[test]
    fn task_validation_command_completed_event_carries_persisted_evidence() {
        let (run, task) = event_payload_run();
        let result = ValidationCommandResult {
            id: "command-456".to_string(),
            validation_run_id: run.id.clone(),
            task_id: task.id.clone(),
            project_id: task.project_id.clone(),
            command_source: ValidationCommandSource::AgentSelected,
            command_ref: None,
            command: "cargo test validation".to_string(),
            cwd: "/tmp/event-project".to_string(),
            label: Some("Rust validation".to_string()),
            category: ValidationCommandCategory::Test,
            reason: None,
            related_files: Vec::new(),
            cache_key: "cache-key".to_string(),
            cache_decision: ValidationCacheDecision::Forced,
            status: ValidationCommandStatus::Failed,
            exit_code: Some(101),
            duration_ms: Some(42),
            stdout_snippet: Some("stdout".to_string()),
            stderr_snippet: Some("stderr".to_string()),
            stdout_log_path: Some("/logs/stdout.log".to_string()),
            stderr_log_path: Some("/logs/stderr.log".to_string()),
            launcher_kind: Some("production_shell_resolver".to_string()),
            resolved_shell_path: Some("/bin/sh".to_string()),
            head_sha: run.head_sha.clone(),
            analysis_fingerprint: run.analysis_fingerprint.clone(),
            status_episode_entered_at: None,
            created_at: Utc::now(),
        };

        let value =
            serde_json::to_value(TaskValidationEventPayload::command_completed(&run, &result))
                .expect("payload should serialize");

        assert_eq!(value["type"], "command_completed");
        assert_eq!(value["status"], "failed");
        assert_eq!(value["run_id"], "run-123");
        assert_eq!(value["command_id"], "command-456");
        assert_eq!(value["exit_code"], 101);
        assert_eq!(value["duration_ms"], 42);
        assert_eq!(value["stdout_snippet"], "stdout");
        assert_eq!(value["stderr_log_path"], "/logs/stderr.log");
        assert_eq!(value["head_short_sha"], "abcdef12");
    }

    #[tokio::test]
    async fn run_task_validation_runs_command_and_reuses_cached_success_for_same_episode() {
        let (state, temp_dir, task_id) = seeded_state().await;
        let package_dir = temp_dir.path().join("package");
        std::fs::create_dir(&package_dir).expect("package dir should be created");
        state
            .task_repo
            .persist_status_change(
                &task_id,
                InternalStatus::Ready,
                InternalStatus::Executing,
                "agent-started",
            )
            .await
            .expect("execution episode should be recorded");

        let mut first_request = request(&task_id, "ralphx-execution-worker");
        first_request.analysis_fingerprint = Some("analysis-fingerprint".to_string());
        first_request.commands = vec![ValidationCommandRequest {
            command: "printf validation-pass".to_string(),
            cwd: Some("package".to_string()),
            label: Some("Unit tests".to_string()),
            category: Some("test".to_string()),
            reason: Some("exercise managed validation runner".to_string()),
            related_files: vec![
                "src/lib.rs".to_string(),
                "/absolute.rs".to_string(),
                "../escape.rs".to_string(),
                "nested//bad.rs".to_string(),
                "  ".to_string(),
            ],
            command_ref: Some("test-command".to_string()),
            source: Some("project_analysis_ref".to_string()),
        }];

        let first = TaskValidationService::run_task_validation(&state, first_request.clone())
            .await
            .expect("first validation should run");

        assert!(first.policy_enabled);
        let first_run = first.latest_run.expect("run summary should be present");
        assert_eq!(first_run.status, "passed");
        assert_eq!(first_run.mode, "force");
        assert_eq!(
            first_run.requested_by_agent.as_deref(),
            Some("ralphx-execution-worker")
        );
        assert_eq!(first_run.base_ref.as_deref(), Some("main"));
        assert_eq!(first.commands.len(), 1);
        let first_command = &first.commands[0];
        assert_eq!(first_command.command_source, "project_analysis_ref");
        assert_eq!(first_command.command_ref.as_deref(), Some("test-command"));
        assert_eq!(first_command.category, "test");
        assert_eq!(first_command.cache_decision, "forced");
        assert_eq!(first_command.status, "passed");
        assert_eq!(first_command.exit_code, Some(0));
        assert_eq!(
            first_command.stdout_snippet.as_deref(),
            Some("validation-pass")
        );
        assert!(first_command.stdout_log_path.is_some());
        assert!(first_command.stderr_log_path.is_none());
        assert_eq!(first_command.related_files, vec!["src/lib.rs"]);

        let mut second_request = first_request;
        second_request.mode = Some("reuse_or_run".to_string());
        let second = TaskValidationService::run_task_validation(&state, second_request)
            .await
            .expect("second validation should reuse cached success");

        assert_eq!(second.commands.len(), 1);
        let cached = &second.commands[0];
        assert_eq!(cached.cache_decision, "cached");
        assert_eq!(cached.status, "cached");
        assert_eq!(cached.exit_code, Some(0));
        assert_eq!(cached.stdout_snippet.as_deref(), Some("validation-pass"));
        assert_ne!(cached.id, first_command.id);

        let latest = state
            .validation_run_repo
            .latest_run_with_results_for_task(&task_id)
            .await
            .expect("latest run lookup should succeed")
            .expect("latest run should exist");
        assert_eq!(latest.run.status, ValidationRunStatus::Passed);
        assert_eq!(latest.run.mode, ValidationRunMode::ReuseOrRun);
        assert_eq!(latest.commands.len(), 1);
        assert_eq!(
            latest.commands[0].cache_decision,
            ValidationCacheDecision::Cached
        );
        assert_eq!(latest.commands[0].status, ValidationCommandStatus::Cached);

        let summary = TaskValidationService::get_task_validation_summary(&state, &task_id)
            .await
            .expect("summary should load latest cached validation");
        assert_eq!(summary.latest_run.expect("latest run").status, "passed");
        assert_eq!(summary.commands[0].cache_decision, "cached");
        assert!(summary.disabled_reason.is_none());
    }

    #[tokio::test]
    async fn matching_committed_tree_promotes_persisted_validation_evidence() {
        let (state, temp_dir, task_id) = seeded_state().await;
        state
            .task_repo
            .persist_status_change(
                &task_id,
                InternalStatus::Ready,
                InternalStatus::Executing,
                "agent-started",
            )
            .await
            .expect("execution episode should be recorded");
        let source = temp_dir.path().join("validated.rs");
        std::fs::write(&source, "pub fn validated() {}").expect("source should be written");

        let validation = TaskValidationService::run_task_validation(
            &state,
            request(&task_id, "ralphx-execution-worker"),
        )
        .await
        .expect("validation should run against dirty worktree");
        assert_eq!(validation.latest_run.expect("run").status, "passed");

        let commit = std::process::Command::new("git")
            .args(["add", "validated.rs"])
            .current_dir(temp_dir.path())
            .output()
            .expect("git add should run");
        assert!(commit.status.success());
        let commit = std::process::Command::new("git")
            .args(["commit", "-m", "validated work"])
            .current_dir(temp_dir.path())
            .env("GIT_AUTHOR_NAME", "Validation Test")
            .env("GIT_AUTHOR_EMAIL", "validation@example.test")
            .env("GIT_COMMITTER_NAME", "Validation Test")
            .env("GIT_COMMITTER_EMAIL", "validation@example.test")
            .output()
            .expect("git commit should run");
        assert!(commit.status.success());
        let commit_sha = GitService::get_head_sha(temp_dir.path())
            .await
            .expect("head should resolve");

        assert!(
            TaskValidationService::promote_matching_validation_to_commit(
                &state,
                &task_id,
                temp_dir.path(),
                &commit_sha,
            )
            .await
            .expect("promotion should succeed")
        );
        let latest = state
            .validation_run_repo
            .latest_non_baseline_run_with_results_for_task(&task_id)
            .await
            .expect("validation run lookup should succeed")
            .expect("validation run should exist");
        assert_eq!(
            latest.run.promoted_commit_sha.as_deref(),
            Some(commit_sha.as_str())
        );
    }

    #[tokio::test]
    async fn changed_worktree_cannot_promote_persisted_validation_evidence() {
        let (state, temp_dir, task_id) = seeded_state().await;
        state
            .task_repo
            .persist_status_change(
                &task_id,
                InternalStatus::Ready,
                InternalStatus::Executing,
                "agent-started",
            )
            .await
            .expect("execution episode should be recorded");
        let source = temp_dir.path().join("validated.rs");
        std::fs::write(&source, "pub fn validated() {}").expect("source should be written");
        TaskValidationService::run_task_validation(
            &state,
            request(&task_id, "ralphx-execution-worker"),
        )
        .await
        .expect("validation should run against dirty worktree");

        std::fs::write(&source, "pub fn changed_after_validation() {}")
            .expect("source should be changed after validation");
        let commit_sha = GitService::get_head_sha(temp_dir.path())
            .await
            .expect("head should resolve");
        assert!(
            !TaskValidationService::promote_matching_validation_to_commit(
                &state,
                &task_id,
                temp_dir.path(),
                &commit_sha,
            )
            .await
            .expect("mismatch should be a normal rejected promotion")
        );
    }

    #[tokio::test]
    async fn run_task_validation_dry_run_records_skipped_commands_and_disabled_summary_reason() {
        let (state, _temp_dir, task_id) = seeded_state().await;
        let mut request = request(&task_id, "ralphx-execution-worker");
        request.mode = Some("dry-run".to_string());
        request.purpose = Some("re-execution".to_string());
        request.context_type = Some("agent-conversation".to_string());
        request.commands = vec![ValidationCommandRequest {
            command: "echo dry-run".to_string(),
            cwd: Some(".".to_string()),
            label: Some("Dry validation".to_string()),
            category: Some("type_check".to_string()),
            reason: Some("preview only".to_string()),
            related_files: vec![
                "frontend/src/App.tsx".to_string(),
                "bad/../path".to_string(),
            ],
            command_ref: Some("typecheck".to_string()),
            source: None,
        }];

        let summary = TaskValidationService::run_task_validation(&state, request)
            .await
            .expect("dry run should record skipped command");

        let run = summary.latest_run.expect("run summary should be present");
        assert_eq!(run.purpose, "re_execution");
        assert_eq!(run.context_type, "agent_conversation");
        assert_eq!(run.status, "skipped");
        assert_eq!(run.mode, "dry_run");
        assert_eq!(summary.commands.len(), 1);
        let command = &summary.commands[0];
        assert_eq!(command.command_source, "project_analysis_ref");
        assert_eq!(command.category, "typecheck");
        assert_eq!(command.cache_decision, "skipped");
        assert_eq!(command.status, "skipped");
        assert_eq!(command.duration_ms, Some(0));
        assert_eq!(command.related_files, vec!["frontend/src/App.tsx"]);
        assert!(command.stdout_log_path.is_none());
        assert!(command.stderr_log_path.is_none());

        state
            .review_settings_repo
            .update_settings(&ReviewSettings {
                run_task_validations: false,
                ..ReviewSettings::default()
            })
            .await
            .expect("settings should update");

        let disabled = TaskValidationService::get_task_validation_summary(&state, &task_id)
            .await
            .expect("disabled summary should still load");
        assert!(!disabled.policy_enabled);
        assert_eq!(
            disabled.disabled_reason.as_deref(),
            Some("Run Task Validations is disabled in Review Policy")
        );
        assert_eq!(disabled.commands[0].status, "skipped");
    }

    #[tokio::test]
    async fn run_task_validation_marks_repeated_failed_command_stale_without_execution_episode() {
        let (state, _temp_dir, task_id) = seeded_state().await;
        let mut first_request = request(&task_id, "ralphx-execution-worker");
        first_request.commands = vec![ValidationCommandRequest {
            command: "printf validation-fail >&2; exit 7".to_string(),
            cwd: None,
            label: Some("Failing validation".to_string()),
            category: Some("lint".to_string()),
            reason: Some("exercise failure result shaping".to_string()),
            related_files: Vec::new(),
            command_ref: None,
            source: None,
        }];

        let first = TaskValidationService::run_task_validation(&state, first_request.clone())
            .await
            .expect("first validation should run and fail");

        assert_eq!(first.latest_run.expect("run summary").status, "failed");
        assert_eq!(first.commands.len(), 1);
        let failed = &first.commands[0];
        assert_eq!(failed.command_source, "agent_selected");
        assert_eq!(failed.category, "lint");
        assert_eq!(failed.cache_decision, "forced");
        assert_eq!(failed.status, "failed");
        assert_eq!(failed.exit_code, Some(7));
        assert_eq!(failed.stderr_snippet.as_deref(), Some("validation-fail"));
        assert!(failed.stdout_snippet.is_none());
        assert!(failed.stdout_log_path.is_none());
        assert!(failed.stderr_log_path.is_some());

        let mut second_request = first_request;
        second_request.mode = Some("reuse_or_run".to_string());
        let second = TaskValidationService::run_task_validation(&state, second_request)
            .await
            .expect("second validation should rerun stale failed command");

        assert_eq!(
            second.latest_run.expect("second run summary").status,
            "failed"
        );
        assert_eq!(second.commands[0].cache_decision, "stale");
        assert_eq!(second.commands[0].status, "failed");
        assert_eq!(second.commands[0].exit_code, Some(7));
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
