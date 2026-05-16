// Production implementation of GithubServiceTrait using the `gh` CLI.
//
// Safety rules (NON-NEGOTIABLE):
//  - All subprocess calls: tokio::process::Command + .spawn() + kill_on_drop(true)
//  - NEVER .output() — kills the tokio runtime by blocking
//  - Pipe buffer safety: piped stdout/stderr consumed via BufReader to prevent deadlocks
//  - All calls wrapped in tokio::time::timeout(30s)
//  - Stderr sanitized: secrets filtered, token-embedded URLs scrubbed

use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::time::{timeout, Duration};
use tracing::{debug, warn};

use crate::domain::services::github_service::{
    GithubServiceTrait, PrAnnotationSourceUnavailable, PrBranchMatch, PrDiffAnnotation,
    PrDiffAnnotations, PrMergeStateStatus, PrMergeableState, PrReviewCommentFeedback,
    PrReviewFeedback, PrStatus, PrSyncState,
};
use crate::error::AppError;
use crate::infrastructure::agents::claude::git_runtime_config;
use crate::infrastructure::git_auth::{
    apply_git_subprocess_env, git_auth_error_from_failure, GitNetworkOperation,
};
use crate::infrastructure::tool_paths::{resolve_gh_cli_path, resolve_git_cli_path};
use crate::utils::secret_redactor::redact;
use crate::AppResult;

const SUBPROCESS_TIMEOUT: Duration = Duration::from_secs(30);
/// Secret keyword fragments to filter from stderr output (case-insensitive match)
const SECRET_KEYWORDS: &[&str] = &[
    "token",
    "bearer",
    "auth",
    "credential",
    "password",
    "secret",
    "ghp_",
    "gho_",
];

/// Known error message fragments from `gh pr create` when a PR already exists for this branch.
/// Used by `create_draft_pr` to detect duplicates and return `AppError::DuplicatePr`.
pub(crate) const DUPLICATE_PR_FRAGMENTS: [&str; 3] = [
    "already exists",
    "a pull request for",
    "already a pull request",
];

#[async_trait]
pub(crate) trait GhCliCommandRunner: Send + Sync {
    async fn run_gh(&self, working_dir: &Path, args: &[String]) -> AppResult<Vec<String>>;
    async fn run_git(&self, working_dir: &Path, args: &[String]) -> AppResult<()>;
}

struct RealGhCliCommandRunner;

#[async_trait]
impl GhCliCommandRunner for RealGhCliCommandRunner {
    async fn run_gh(&self, working_dir: &Path, args: &[String]) -> AppResult<Vec<String>> {
        GhCliGithubService::run_gh_process(working_dir, args).await
    }

    async fn run_git(&self, working_dir: &Path, args: &[String]) -> AppResult<()> {
        GhCliGithubService::run_git_process(working_dir, args).await
    }
}

/// Production GitHub service backed by the `gh` CLI
pub struct GhCliGithubService {
    runner: Arc<dyn GhCliCommandRunner>,
}

impl GhCliGithubService {
    pub fn new() -> Self {
        Self::with_runner(Arc::new(RealGhCliCommandRunner))
    }

    pub(crate) fn with_runner(runner: Arc<dyn GhCliCommandRunner>) -> Self {
        Self { runner }
    }

    /// Consume stdout + stderr from a spawned child in separate tasks.
    /// Returns (stdout_lines, sanitized_stderr_lines).
    async fn collect_output(
        child: &mut tokio::process::Child,
    ) -> AppResult<(Vec<String>, Vec<String>)> {
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| AppError::Infrastructure("Failed to capture stdout pipe".to_string()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| AppError::Infrastructure("Failed to capture stderr pipe".to_string()))?;

        let stdout_task = tokio::spawn(async move {
            let mut lines = Vec::new();
            let mut reader = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                lines.push(line);
            }
            lines
        });

        let stderr_task = tokio::spawn(async move {
            let mut lines = Vec::new();
            let mut reader = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                let sanitized = sanitize_stderr_line(&line);
                lines.push(sanitized);
            }
            lines
        });

        let stdout_lines = stdout_task
            .await
            .map_err(|e| AppError::Infrastructure(format!("stdout task panicked: {e}")))?;
        let stderr_lines = stderr_task
            .await
            .map_err(|e| AppError::Infrastructure(format!("stderr task panicked: {e}")))?;

        Ok((stdout_lines, stderr_lines))
    }

    /// Run a `gh` command, collect output, wait for exit, and return stdout lines.
    /// Errors if the process exits non-zero.
    async fn run_gh_process<I, S>(working_dir: &Path, args: I) -> AppResult<Vec<String>>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        let mut child = tokio::process::Command::new(resolve_gh_cli_path())
            .args(args)
            .current_dir(working_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| AppError::Infrastructure(format!("Failed to spawn gh: {e}")))?;

        let result = timeout(SUBPROCESS_TIMEOUT, async {
            let (stdout, stderr) = Self::collect_output(&mut child).await?;
            let status = child.wait().await.map_err(|e| {
                AppError::Infrastructure(format!("Failed to wait for gh process: {e}"))
            })?;
            Ok::<_, AppError>((stdout, stderr, status))
        })
        .await
        .map_err(|_| AppError::Infrastructure("gh command timed out after 30s".to_string()))??;

        let (stdout, stderr, status) = result;

        if !status.success() {
            let code = status.code().unwrap_or(-1);
            let err_msg = stderr.join("\n");
            debug!(code, %err_msg, "gh command failed");
            return Err(AppError::Infrastructure(format!(
                "gh exited with code {code}: {err_msg}"
            )));
        }

        if !stderr.is_empty() {
            debug!(lines = ?stderr, "gh stderr output");
        }

        Ok(stdout)
    }

    /// Run a git command (for operations not covered by `gh`, e.g. push, fetch).
    async fn run_git_process<I, S>(working_dir: &Path, args: I) -> AppResult<()>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        let args: Vec<OsString> = args
            .into_iter()
            .map(|arg| arg.as_ref().to_os_string())
            .collect();
        let arg_strings: Vec<String> = args
            .iter()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect();
        let operation = GitNetworkOperation::from_args(&arg_strings);
        let mut command = tokio::process::Command::new(resolve_git_cli_path());
        apply_git_subprocess_env(&mut command);
        let mut child = command
            .args(&args)
            .current_dir(working_dir)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| AppError::Infrastructure(format!("Failed to spawn git: {e}")))?;

        let result = timeout(SUBPROCESS_TIMEOUT, async {
            let stderr_handle = child.stderr.take();
            let stderr_task = tokio::spawn(async move {
                if let Some(stderr) = stderr_handle {
                    let mut lines = Vec::new();
                    let mut reader = BufReader::new(stderr).lines();
                    while let Ok(Some(line)) = reader.next_line().await {
                        lines.push(sanitize_stderr_line(&line));
                    }
                    lines
                } else {
                    Vec::new()
                }
            });
            let status = child.wait().await.map_err(|e| {
                AppError::Infrastructure(format!("Failed to wait for git process: {e}"))
            })?;
            let stderr = stderr_task.await.unwrap_or_default();
            Ok::<_, AppError>((status, stderr))
        })
        .await
        .map_err(|_| AppError::Infrastructure("git command timed out after 30s".to_string()))??;

        let (status, stderr) = result;

        if !status.success() {
            let code = status.code().unwrap_or(-1);
            let err_msg = stderr.join("\n");
            if let Some(operation) = operation {
                if let Some(error) =
                    git_auth_error_from_failure(operation, working_dir, &err_msg).await
                {
                    return Err(error);
                }
            }
            return Err(AppError::Infrastructure(format!(
                "git exited with code {code}: {err_msg}"
            )));
        }

        Ok(())
    }
}

impl Default for GhCliGithubService {
    fn default() -> Self {
        Self::new()
    }
}

fn build_create_pr_args(
    base: &str,
    head: &str,
    title: &str,
    body_file: &str,
    include_json: bool,
) -> Vec<String> {
    let mut args = vec![
        "pr".to_string(),
        "create".to_string(),
        "--draft".to_string(),
        "--base".to_string(),
        base.to_string(),
        "--head".to_string(),
        head.to_string(),
        "--title".to_string(),
        title.to_string(),
        "--body-file".to_string(),
        body_file.to_string(),
    ];
    if include_json {
        args.push("--json".to_string());
        args.push("number,url".to_string());
    }
    args
}

fn build_update_pr_args(pr_number: i64, title: &str, body_file: &str) -> Vec<String> {
    vec![
        "pr".to_string(),
        "edit".to_string(),
        pr_number.to_string(),
        "--title".to_string(),
        title.to_string(),
        "--body-file".to_string(),
        body_file.to_string(),
    ]
}

fn build_update_pr_base_args(pr_number: i64, base: &str) -> Vec<String> {
    vec![
        "pr".to_string(),
        "edit".to_string(),
        pr_number.to_string(),
        "--base".to_string(),
        base.to_string(),
    ]
}

fn build_pr_review_decision_args(pr_number: i64) -> Vec<String> {
    vec![
        "pr".to_string(),
        "view".to_string(),
        pr_number.to_string(),
        "--json".to_string(),
        "reviewDecision".to_string(),
    ]
}

fn build_pr_sync_state_args(pr_number: i64) -> Vec<String> {
    vec![
        "pr".to_string(),
        "view".to_string(),
        pr_number.to_string(),
        "--json".to_string(),
        "state,mergeStateStatus,mergeable,isDraft,headRefName,baseRefName,headRefOid,baseRefOid,mergedAt,mergeCommit".to_string(),
    ]
}

fn build_pr_reviews_api_args(pr_number: i64) -> Vec<String> {
    vec![
        "api".to_string(),
        format!("repos/{{owner}}/{{repo}}/pulls/{pr_number}/reviews"),
        "--paginate".to_string(),
        "--slurp".to_string(),
    ]
}

fn build_pr_review_comments_api_args(pr_number: i64) -> Vec<String> {
    vec![
        "api".to_string(),
        format!("repos/{{owner}}/{{repo}}/pulls/{pr_number}/comments"),
        "--paginate".to_string(),
        "--slurp".to_string(),
    ]
}

fn build_pr_annotation_pr_view_args(pr_number: i64) -> Vec<String> {
    vec![
        "pr".to_string(),
        "view".to_string(),
        pr_number.to_string(),
        "--json".to_string(),
        "headRefOid".to_string(),
    ]
}

fn build_check_runs_for_ref_api_args(head_sha: &str) -> Vec<String> {
    vec![
        "api".to_string(),
        format!("repos/{{owner}}/{{repo}}/commits/{head_sha}/check-runs"),
        "--paginate".to_string(),
        "--slurp".to_string(),
    ]
}

fn build_check_run_annotations_api_args(check_run_id: i64) -> Vec<String> {
    vec![
        "api".to_string(),
        format!("repos/{{owner}}/{{repo}}/check-runs/{check_run_id}/annotations"),
        "--paginate".to_string(),
        "--slurp".to_string(),
    ]
}

fn build_code_scanning_alerts_api_args(pr_number: i64) -> Vec<String> {
    vec![
        "api".to_string(),
        format!(
            "repos/{{owner}}/{{repo}}/code-scanning/alerts?state=open&pr={pr_number}&per_page=100"
        ),
        "--paginate".to_string(),
        "--slurp".to_string(),
    ]
}

fn is_duplicate_pr_error(msg: &str) -> bool {
    let lower = msg.to_lowercase();
    DUPLICATE_PR_FRAGMENTS
        .iter()
        .any(|fragment| lower.contains(fragment))
}

fn pr_annotation_source_unavailable(
    source: &str,
    error: impl std::fmt::Display,
) -> PrAnnotationSourceUnavailable {
    PrAnnotationSourceUnavailable {
        source: source.to_string(),
        reason: error.to_string(),
    }
}

fn max_pr_check_run_annotation_fetches() -> usize {
    usize::try_from(git_runtime_config().workspace_pr_annotations_check_run_fetch_limit)
        .unwrap_or(usize::MAX)
}

/// Sanitize a single stderr line:
/// 1. Filter lines containing secret keywords (case-insensitive) — full-line suppression
/// 2. Scrub token-embedded URLs: `https://<token>@github.com` → `https://***@github.com`
/// 3. Apply `redact()` as a second pass for any remaining regex-pattern secrets
pub(crate) fn sanitize_stderr_line(line: &str) -> String {
    let lower = line.to_lowercase();
    for keyword in SECRET_KEYWORDS {
        if lower.contains(keyword) {
            return "[REDACTED: potential secret in stderr]".to_string();
        }
    }
    let url_scrubbed = scrub_token_urls(line);
    redact(&url_scrubbed)
}

/// Replace `https://<anything>@github.com` with `https://***@github.com`
pub(crate) fn scrub_token_urls(s: &str) -> String {
    // Simple state-machine scan — avoids pulling in the regex crate
    let prefix = "https://";
    let separator = "@github.com";

    let mut result = String::with_capacity(s.len());
    let mut remaining = s;

    while let Some(start) = remaining.find(prefix) {
        result.push_str(&remaining[..start]);
        let after_prefix = &remaining[start + prefix.len()..];

        if let Some(at_pos) = after_prefix.find(separator) {
            // Check there's an actual token (non-empty) before the @
            if at_pos > 0 {
                result.push_str(prefix);
                result.push_str("***");
                result.push_str(separator);
                remaining = &after_prefix[at_pos + separator.len()..];
            } else {
                // No token — keep as-is
                result.push_str(prefix);
                remaining = after_prefix;
            }
        } else {
            // No @github.com after this https:// — keep as-is
            result.push_str(prefix);
            remaining = after_prefix;
        }
    }

    result.push_str(remaining);
    result
}

#[async_trait]
impl GithubServiceTrait for GhCliGithubService {
    async fn create_draft_pr(
        &self,
        working_dir: &Path,
        base: &str,
        head: &str,
        title: &str,
        body_file: &Path,
    ) -> AppResult<(i64, String)> {
        // gh pr create --draft --base <base> --head <head> --title <title> --body-file <file>
        let body_file_str = body_file
            .to_str()
            .ok_or_else(|| {
                AppError::Infrastructure("body_file path is not valid UTF-8".to_string())
            })?
            .to_string();

        let args = build_create_pr_args(base, head, title, &body_file_str, false);
        let result = self.runner.run_gh(working_dir, &args).await;

        match result {
            Ok(stdout) => {
                let plain_output = stdout.join("\n");
                parse_pr_create_plain_output(&plain_output)
            }
            Err(AppError::Infrastructure(msg)) if is_duplicate_pr_error(&msg) => {
                Err(AppError::DuplicatePr)
            }
            Err(other) => Err(other),
        }
    }

    async fn mark_pr_ready(&self, working_dir: &Path, pr_number: i64) -> AppResult<()> {
        // gh pr ready <number>
        let args = vec!["pr".to_string(), "ready".to_string(), pr_number.to_string()];
        self.runner.run_gh(working_dir, &args).await?;
        Ok(())
    }

    async fn update_pr_details(
        &self,
        working_dir: &Path,
        pr_number: i64,
        title: &str,
        body_file: &Path,
    ) -> AppResult<()> {
        let body_file_str = body_file
            .to_str()
            .ok_or_else(|| {
                AppError::Infrastructure("body_file path is not valid UTF-8".to_string())
            })?
            .to_string();
        let args = build_update_pr_args(pr_number, title, &body_file_str);
        self.runner.run_gh(working_dir, &args).await?;
        Ok(())
    }

    async fn update_pr_base(
        &self,
        working_dir: &Path,
        pr_number: i64,
        base: &str,
    ) -> AppResult<()> {
        let args = build_update_pr_base_args(pr_number, base);
        self.runner.run_gh(working_dir, &args).await?;
        Ok(())
    }

    async fn check_pr_status(&self, working_dir: &Path, pr_number: i64) -> AppResult<PrStatus> {
        // gh pr view <number> --json state,mergedAt,mergeCommit
        let args = vec![
            "pr".to_string(),
            "view".to_string(),
            pr_number.to_string(),
            "--json".to_string(),
            "state,mergedAt,mergeCommit".to_string(),
        ];
        let stdout = self.runner.run_gh(working_dir, &args).await?;

        let json_str = stdout.join("\n");
        parse_pr_status_output(&json_str)
    }

    async fn check_pr_sync_state(
        &self,
        working_dir: &Path,
        pr_number: i64,
    ) -> AppResult<PrSyncState> {
        let stdout = self
            .runner
            .run_gh(working_dir, &build_pr_sync_state_args(pr_number))
            .await?;
        parse_pr_sync_state_output(&stdout.join("\n"))
    }

    async fn check_pr_review_feedback(
        &self,
        working_dir: &Path,
        pr_number: i64,
    ) -> AppResult<Option<PrReviewFeedback>> {
        let decision_stdout = self
            .runner
            .run_gh(working_dir, &build_pr_review_decision_args(pr_number))
            .await?;
        if !parse_pr_review_decision_output(&decision_stdout.join("\n"))? {
            return Ok(None);
        }

        let reviews_stdout = self
            .runner
            .run_gh(working_dir, &build_pr_reviews_api_args(pr_number))
            .await?;
        let comments_stdout = self
            .runner
            .run_gh(working_dir, &build_pr_review_comments_api_args(pr_number))
            .await?;

        parse_pr_review_feedback_output(&reviews_stdout.join("\n"), &comments_stdout.join("\n"))
    }

    async fn fetch_pr_diff_annotations(
        &self,
        working_dir: &Path,
        pr_number: i64,
    ) -> AppResult<PrDiffAnnotations> {
        let mut payload = PrDiffAnnotations::empty(pr_number);

        match self
            .runner
            .run_gh(working_dir, &build_pr_review_comments_api_args(pr_number))
            .await
        {
            Ok(stdout) => {
                match parse_pr_review_comment_annotations_output(pr_number, &stdout.join("\n")) {
                    Ok(mut annotations) => payload.annotations.append(&mut annotations),
                    Err(error) => payload
                        .sources_unavailable
                        .push(pr_annotation_source_unavailable("review_comments", error)),
                }
            }
            Err(error) => payload
                .sources_unavailable
                .push(pr_annotation_source_unavailable("review_comments", error)),
        }

        match self
            .runner
            .run_gh(working_dir, &build_code_scanning_alerts_api_args(pr_number))
            .await
        {
            Ok(stdout) => match parse_code_scanning_alert_annotations_output(&stdout.join("\n")) {
                Ok(mut annotations) => payload.annotations.append(&mut annotations),
                Err(error) => payload
                    .sources_unavailable
                    .push(pr_annotation_source_unavailable("code_scanning", error)),
            },
            Err(error) => payload
                .sources_unavailable
                .push(pr_annotation_source_unavailable("code_scanning", error)),
        }

        match self
            .runner
            .run_gh(working_dir, &build_pr_annotation_pr_view_args(pr_number))
            .await
        {
            Ok(stdout) => match parse_pr_annotation_head_sha_output(&stdout.join("\n")) {
                Ok(Some(head_sha)) => {
                    payload.head_sha = Some(head_sha.clone());
                    match self
                        .runner
                        .run_gh(working_dir, &build_check_runs_for_ref_api_args(&head_sha))
                        .await
                    {
                        Ok(check_runs_stdout) => {
                            match parse_check_runs_output(&check_runs_stdout.join("\n")) {
                                Ok(check_runs) => {
                                    let annotated_check_runs = check_runs
                                        .into_iter()
                                        .filter(|run| run.annotations_count > 0)
                                        .collect::<Vec<_>>();
                                    let fetch_limit = max_pr_check_run_annotation_fetches();
                                    let skipped_count =
                                        annotated_check_runs.len().saturating_sub(fetch_limit);
                                    for check_run in
                                        annotated_check_runs.into_iter().take(fetch_limit)
                                    {
                                        match self
                                            .runner
                                            .run_gh(
                                                working_dir,
                                                &build_check_run_annotations_api_args(check_run.id),
                                            )
                                            .await
                                        {
                                            Ok(annotation_stdout) => {
                                                match parse_check_run_annotations_output(
                                                    &check_run,
                                                    &annotation_stdout.join("\n"),
                                                ) {
                                                    Ok(mut annotations) => {
                                                        payload.annotations.append(&mut annotations)
                                                    }
                                                    Err(error) => payload.sources_unavailable.push(
                                                        pr_annotation_source_unavailable(
                                                            "check_run_annotations",
                                                            error,
                                                        ),
                                                    ),
                                                }
                                            }
                                            Err(error) => payload.sources_unavailable.push(
                                                pr_annotation_source_unavailable(
                                                    "check_run_annotations",
                                                    error,
                                                ),
                                            ),
                                        }
                                    }
                                    if skipped_count > 0 {
                                        payload.sources_unavailable.push(
                                            PrAnnotationSourceUnavailable {
                                                source: "check_run_annotations".to_string(),
                                                reason: format!(
                                                    "Skipped annotations for {skipped_count} additional check runs after limit of {fetch_limit}"
                                                ),
                                            },
                                        );
                                    }
                                }
                                Err(error) => payload
                                    .sources_unavailable
                                    .push(pr_annotation_source_unavailable("check_runs", error)),
                            }
                        }
                        Err(error) => payload
                            .sources_unavailable
                            .push(pr_annotation_source_unavailable("check_runs", error)),
                    }
                }
                Ok(None) => payload
                    .sources_unavailable
                    .push(PrAnnotationSourceUnavailable {
                        source: "check_runs".to_string(),
                        reason: "Pull request head SHA was unavailable".to_string(),
                    }),
                Err(error) => payload
                    .sources_unavailable
                    .push(pr_annotation_source_unavailable("check_runs", error)),
            },
            Err(error) => payload
                .sources_unavailable
                .push(pr_annotation_source_unavailable("check_runs", error)),
        }

        payload.annotations.sort_by(|left, right| {
            left.path
                .cmp(&right.path)
                .then(left.start_line.cmp(&right.start_line))
                .then(left.source.cmp(&right.source))
                .then(left.id.cmp(&right.id))
        });
        Ok(payload)
    }

    async fn push_branch(&self, working_dir: &Path, branch: &str) -> AppResult<()> {
        // git push origin <branch> — fire-and-forget style (stdout null, stderr piped for safety)
        let args = vec!["push".to_string(), "origin".to_string(), branch.to_string()];
        self.runner.run_git(working_dir, &args).await
    }

    async fn close_pr(&self, working_dir: &Path, pr_number: i64) -> AppResult<()> {
        // gh pr close <number>
        let args = vec!["pr".to_string(), "close".to_string(), pr_number.to_string()];
        self.runner.run_gh(working_dir, &args).await?;
        Ok(())
    }

    async fn delete_remote_branch(&self, working_dir: &Path, branch: &str) -> AppResult<()> {
        // git push origin --delete <branch>
        // Already-deleted → "remote ref does not exist" → treat as no-op
        let mut command = tokio::process::Command::new(resolve_git_cli_path());
        apply_git_subprocess_env(&mut command);
        let mut child = command
            .args(["push", "origin", "--delete", branch])
            .current_dir(working_dir)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| AppError::Infrastructure(format!("Failed to spawn git: {e}")))?;

        let result = timeout(SUBPROCESS_TIMEOUT, async {
            let stderr_handle = child.stderr.take();
            let stderr_task = tokio::spawn(async move {
                if let Some(stderr) = stderr_handle {
                    let mut lines = Vec::new();
                    let mut reader = BufReader::new(stderr).lines();
                    while let Ok(Some(line)) = reader.next_line().await {
                        lines.push(sanitize_stderr_line(&line));
                    }
                    lines
                } else {
                    Vec::new()
                }
            });
            let status = child
                .wait()
                .await
                .map_err(|e| AppError::Infrastructure(format!("git wait failed: {e}")))?;
            let stderr = stderr_task.await.unwrap_or_default();
            Ok::<_, AppError>((status, stderr))
        })
        .await
        .map_err(|_| AppError::Infrastructure("git push --delete timed out".to_string()))??;

        let (status, stderr) = result;

        if status.success() {
            return Ok(());
        }

        // Treat "remote ref does not exist" as success (already deleted)
        let stderr_combined = stderr.join("\n").to_lowercase();
        if stderr_combined.contains("remote ref does not exist")
            || (stderr_combined.contains("error: unable to delete")
                && stderr_combined.contains("does not exist"))
            || stderr_combined.contains("no such ref")
        {
            warn!(branch, "Remote branch already deleted — treating as no-op");
            return Ok(());
        }

        let stderr_text = stderr.join("\n");
        if let Some(error) = git_auth_error_from_failure(
            GitNetworkOperation::DeleteRemoteBranch,
            working_dir,
            &stderr_text,
        )
        .await
        {
            return Err(error);
        }

        Err(AppError::Infrastructure(format!(
            "git push --delete failed: {}",
            stderr_text
        )))
    }

    async fn fetch_remote(&self, working_dir: &Path, branch: &str) -> AppResult<()> {
        // git fetch origin <branch>
        let args = vec![
            "fetch".to_string(),
            "origin".to_string(),
            branch.to_string(),
        ];
        self.runner.run_git(working_dir, &args).await
    }

    async fn find_pr_by_head_branch(
        &self,
        working_dir: &Path,
        head: &str,
    ) -> AppResult<Option<(i64, String)>> {
        // gh pr list --head <head> --json number,url --state open
        let args = vec![
            "pr".to_string(),
            "list".to_string(),
            "--head".to_string(),
            head.to_string(),
            "--json".to_string(),
            "number,url".to_string(),
            "--state".to_string(),
            "open".to_string(),
        ];
        let stdout = self.runner.run_gh(working_dir, &args).await?;

        let json_str = stdout.join("\n");
        parse_pr_list_output(&json_str)
    }

    async fn find_latest_pr_by_head_branch(
        &self,
        working_dir: &Path,
        head: &str,
    ) -> AppResult<Option<PrBranchMatch>> {
        // gh pr list --head <head> --state all --limit 20 --json number,url,state,isDraft,headRefName,updatedAt
        let args = vec![
            "pr".to_string(),
            "list".to_string(),
            "--head".to_string(),
            head.to_string(),
            "--state".to_string(),
            "all".to_string(),
            "--limit".to_string(),
            "20".to_string(),
            "--json".to_string(),
            "number,url,state,isDraft,headRefName,updatedAt".to_string(),
        ];
        let stdout = self.runner.run_gh(working_dir, &args).await?;

        let json_str = stdout.join("\n");
        parse_pr_branch_match_output(&json_str, head)
    }
}

// ── Output parsers ────────────────────────────────────────────────────────────

#[cfg(test)]
pub(crate) fn parse_pr_create_output(json_str: &str) -> AppResult<(i64, String)> {
    let v: serde_json::Value = serde_json::from_str(json_str).map_err(|e| {
        AppError::Infrastructure(format!(
            "Failed to parse gh pr create JSON: {e}\nRaw: {json_str}"
        ))
    })?;

    let number = v["number"].as_i64().ok_or_else(|| {
        AppError::Infrastructure("gh pr create: missing 'number' field".to_string())
    })?;
    let url = v["url"]
        .as_str()
        .ok_or_else(|| AppError::Infrastructure("gh pr create: missing 'url' field".to_string()))?
        .to_string();

    Ok((number, url))
}

pub(crate) fn parse_pr_create_plain_output(stdout_str: &str) -> AppResult<(i64, String)> {
    let url = stdout_str
        .split_whitespace()
        .map(|token| token.trim_matches(|c: char| "()[]<>{},'\"".contains(c)))
        .find(|token| {
            token.starts_with("https://")
                && token.contains("github.com/")
                && token.contains("/pull/")
        })
        .ok_or_else(|| {
            AppError::Infrastructure(format!(
                "gh pr create fallback: could not find PR URL in output: {stdout_str}"
            ))
        })?
        .to_string();

    let pr_number = url
        .split("/pull/")
        .nth(1)
        .and_then(|tail| tail.split(['/', '?', '#']).next())
        .ok_or_else(|| {
            AppError::Infrastructure(format!(
                "gh pr create fallback: could not extract PR number from URL: {url}"
            ))
        })?
        .parse::<i64>()
        .map_err(|e| {
            AppError::Infrastructure(format!(
                "gh pr create fallback: invalid PR number in URL {url}: {e}"
            ))
        })?;

    Ok((pr_number, url))
}

pub(crate) fn parse_pr_list_output(json_str: &str) -> AppResult<Option<(i64, String)>> {
    let arr: serde_json::Value = serde_json::from_str(json_str).map_err(|e| {
        AppError::Infrastructure(format!(
            "Failed to parse gh pr list JSON: {e}\nRaw: {json_str}"
        ))
    })?;

    let items = arr.as_array().ok_or_else(|| {
        AppError::Infrastructure(format!("gh pr list: expected JSON array, got: {json_str}"))
    })?;

    if items.is_empty() {
        return Ok(None);
    }

    let first = &items[0];
    let number = first["number"].as_i64().ok_or_else(|| {
        AppError::Infrastructure("gh pr list: missing 'number' field".to_string())
    })?;
    let url = first["url"]
        .as_str()
        .ok_or_else(|| AppError::Infrastructure("gh pr list: missing 'url' field".to_string()))?
        .to_string();

    Ok(Some((number, url)))
}

pub(crate) fn parse_pr_branch_match_output(
    json_str: &str,
    expected_head: &str,
) -> AppResult<Option<PrBranchMatch>> {
    let arr: Value = serde_json::from_str(json_str).map_err(|e| {
        AppError::Infrastructure(format!(
            "Failed to parse gh pr list branch JSON: {e}\nRaw: {json_str}"
        ))
    })?;

    let items = arr.as_array().ok_or_else(|| {
        AppError::Infrastructure(format!(
            "gh pr list branch lookup: expected JSON array, got: {json_str}"
        ))
    })?;

    let mut matches = items
        .iter()
        .filter(|item| {
            item.get("headRefName")
                .and_then(Value::as_str)
                .is_none_or(|head| head == expected_head)
        })
        .map(parse_pr_branch_match_item)
        .collect::<AppResult<Vec<_>>>()?;

    matches.sort_by(|left, right| {
        right
            .updated_at
            .cmp(&left.updated_at)
            .then_with(|| right.number.cmp(&left.number))
    });

    Ok(matches.into_iter().next())
}

fn parse_pr_branch_match_item(item: &Value) -> AppResult<PrBranchMatch> {
    let number = item["number"].as_i64().ok_or_else(|| {
        AppError::Infrastructure("gh pr list branch lookup: missing 'number' field".to_string())
    })?;
    let url = item["url"]
        .as_str()
        .ok_or_else(|| {
            AppError::Infrastructure("gh pr list branch lookup: missing 'url' field".to_string())
        })?
        .to_string();
    let head_ref_name = item
        .get("headRefName")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let status = parse_pr_status_value(item)?;

    Ok(PrBranchMatch {
        number,
        url,
        status,
        is_draft: item
            .get("isDraft")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        head_ref_name,
        updated_at: item
            .get("updatedAt")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

pub(crate) fn parse_pr_status_output(json_str: &str) -> AppResult<PrStatus> {
    let v: serde_json::Value = serde_json::from_str(json_str).map_err(|e| {
        AppError::Infrastructure(format!(
            "Failed to parse gh pr view JSON: {e}\nRaw: {json_str}"
        ))
    })?;

    let state = v["state"]
        .as_str()
        .ok_or_else(|| AppError::Infrastructure("gh pr view: missing 'state' field".to_string()))?;

    match state {
        "OPEN" => Ok(PrStatus::Open),
        "CLOSED" => Ok(PrStatus::Closed),
        "MERGED" => {
            // mergeCommit is an object with "oid" when merged, null otherwise
            let sha = v["mergeCommit"]["oid"].as_str().map(str::to_string);
            Ok(PrStatus::Merged {
                merge_commit_sha: sha,
            })
        }
        other => Err(AppError::Infrastructure(format!(
            "gh pr view: unknown state '{other}'"
        ))),
    }
}

pub(crate) fn parse_pr_sync_state_output(json_str: &str) -> AppResult<PrSyncState> {
    let v: Value = serde_json::from_str(json_str).map_err(|e| {
        AppError::Infrastructure(format!(
            "Failed to parse gh pr view sync-state JSON: {e}\nRaw: {json_str}"
        ))
    })?;

    let status = parse_pr_status_value(&v)?;
    let head_ref_name = required_string(&v, "headRefName", "gh pr view sync-state")?;
    let base_ref_name = required_string(&v, "baseRefName", "gh pr view sync-state")?;

    Ok(PrSyncState {
        status,
        merge_state_status: v
            .get("mergeStateStatus")
            .and_then(Value::as_str)
            .map(parse_merge_state_status),
        mergeable: v
            .get("mergeable")
            .and_then(Value::as_str)
            .map(parse_mergeable_state),
        is_draft: v.get("isDraft").and_then(Value::as_bool).unwrap_or(false),
        head_ref_name,
        base_ref_name,
        head_ref_oid: v
            .get("headRefOid")
            .and_then(Value::as_str)
            .map(str::to_string),
        base_ref_oid: v
            .get("baseRefOid")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

fn parse_pr_status_value(v: &Value) -> AppResult<PrStatus> {
    let state = v["state"]
        .as_str()
        .ok_or_else(|| AppError::Infrastructure("gh pr view: missing 'state' field".to_string()))?;

    match state {
        "OPEN" => Ok(PrStatus::Open),
        "CLOSED" => Ok(PrStatus::Closed),
        "MERGED" => {
            let sha = v["mergeCommit"]["oid"].as_str().map(str::to_string);
            Ok(PrStatus::Merged {
                merge_commit_sha: sha,
            })
        }
        other => Err(AppError::Infrastructure(format!(
            "gh pr view: unknown state '{other}'"
        ))),
    }
}

fn parse_merge_state_status(value: &str) -> PrMergeStateStatus {
    match value {
        "CLEAN" => PrMergeStateStatus::Clean,
        "BEHIND" => PrMergeStateStatus::Behind,
        "DIRTY" => PrMergeStateStatus::Dirty,
        "BLOCKED" => PrMergeStateStatus::Blocked,
        "DRAFT" => PrMergeStateStatus::Draft,
        "UNKNOWN" => PrMergeStateStatus::Unknown,
        "UNSTABLE" => PrMergeStateStatus::Unstable,
        "HAS_HOOKS" => PrMergeStateStatus::HasHooks,
        other => PrMergeStateStatus::Other(other.to_string()),
    }
}

fn parse_mergeable_state(value: &str) -> PrMergeableState {
    match value {
        "MERGEABLE" => PrMergeableState::Mergeable,
        "CONFLICTING" => PrMergeableState::Conflicting,
        "UNKNOWN" => PrMergeableState::Unknown,
        other => PrMergeableState::Other(other.to_string()),
    }
}

fn required_string(v: &Value, field: &str, context: &str) -> AppResult<String> {
    v.get(field)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| AppError::Infrastructure(format!("{context}: missing '{field}' field")))
}

#[derive(Debug, Clone)]
pub(crate) struct CheckRunAnnotationSource {
    pub id: i64,
    pub name: String,
    pub conclusion: Option<String>,
    pub status: Option<String>,
    pub html_url: Option<String>,
    pub annotations_count: i64,
}

pub(crate) fn parse_pr_review_decision_output(json_str: &str) -> AppResult<bool> {
    let v: Value = serde_json::from_str(json_str).map_err(|e| {
        AppError::Infrastructure(format!(
            "Failed to parse gh pr view reviewDecision JSON: {e}\nRaw: {json_str}"
        ))
    })?;

    Ok(v["reviewDecision"].as_str() == Some("CHANGES_REQUESTED"))
}

pub(crate) fn parse_pr_review_feedback_output(
    reviews_json: &str,
    comments_json: &str,
) -> AppResult<Option<PrReviewFeedback>> {
    let reviews_value: Value = serde_json::from_str(reviews_json).map_err(|e| {
        AppError::Infrastructure(format!(
            "Failed to parse gh reviews JSON: {e}\nRaw: {reviews_json}"
        ))
    })?;
    let comments_value: Value = serde_json::from_str(comments_json).map_err(|e| {
        AppError::Infrastructure(format!(
            "Failed to parse gh review comments JSON: {e}\nRaw: {comments_json}"
        ))
    })?;

    let reviews = flatten_paginated_array(&reviews_value).ok_or_else(|| {
        AppError::Infrastructure(format!(
            "gh reviews: expected JSON array/pages, got: {reviews_json}"
        ))
    })?;
    let comments = flatten_paginated_array(&comments_value).ok_or_else(|| {
        AppError::Infrastructure(format!(
            "gh review comments: expected JSON array/pages, got: {comments_json}"
        ))
    })?;

    let mut latest_by_author: HashMap<String, &Value> = HashMap::new();
    for review in reviews {
        let author = review
            .get("user")
            .and_then(|user| user.get("login"))
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        let replace = latest_by_author
            .get(&author)
            .map(|existing| review_sort_key(review) > review_sort_key(existing))
            .unwrap_or(true);
        if replace {
            latest_by_author.insert(author, review);
        }
    }

    let Some(review) = latest_by_author
        .values()
        .filter(|review| review.get("state").and_then(Value::as_str) == Some("CHANGES_REQUESTED"))
        .max_by_key(|review| review_sort_key(review))
        .copied()
    else {
        return Ok(None);
    };

    let review_id = json_id_to_string(review.get("id")).ok_or_else(|| {
        AppError::Infrastructure("gh reviews: requested-changes review missing id".to_string())
    })?;
    let author = review
        .get("user")
        .and_then(|user| user.get("login"))
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let submitted_at = review
        .get("submitted_at")
        .and_then(Value::as_str)
        .map(str::to_string);
    let body = review
        .get("body")
        .and_then(Value::as_str)
        .filter(|body| !body.trim().is_empty())
        .map(str::to_string);

    let review_comments = comments
        .into_iter()
        .filter(|comment| {
            json_id_to_string(comment.get("pull_request_review_id")).as_deref()
                == Some(review_id.as_str())
        })
        .map(|comment| PrReviewCommentFeedback {
            id: json_id_to_string(comment.get("id")).unwrap_or_default(),
            author: comment
                .get("user")
                .and_then(|user| user.get("login"))
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string(),
            path: comment
                .get("path")
                .and_then(Value::as_str)
                .map(str::to_string),
            line: comment
                .get("line")
                .and_then(Value::as_i64)
                .or_else(|| comment.get("original_line").and_then(Value::as_i64)),
            body: comment
                .get("body")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        })
        .collect();

    Ok(Some(PrReviewFeedback {
        review_id,
        author,
        submitted_at,
        body,
        comments: review_comments,
    }))
}

pub(crate) fn parse_pr_annotation_head_sha_output(json_str: &str) -> AppResult<Option<String>> {
    let value: Value = serde_json::from_str(json_str).map_err(|e| {
        AppError::Infrastructure(format!(
            "Failed to parse gh pr view headRefOid JSON: {e}\nRaw: {json_str}"
        ))
    })?;
    Ok(value
        .get("headRefOid")
        .and_then(Value::as_str)
        .filter(|sha| !sha.trim().is_empty())
        .map(str::to_string))
}

pub(crate) fn parse_pr_review_comment_annotations_output(
    pr_number: i64,
    comments_json: &str,
) -> AppResult<Vec<PrDiffAnnotation>> {
    let comments_value: Value = serde_json::from_str(comments_json).map_err(|e| {
        AppError::Infrastructure(format!(
            "Failed to parse gh review comments JSON: {e}\nRaw: {comments_json}"
        ))
    })?;
    let comments = flatten_paginated_array(&comments_value).ok_or_else(|| {
        AppError::Infrastructure(format!(
            "gh review comments: expected JSON array/pages, got: {comments_json}"
        ))
    })?;

    Ok(comments
        .into_iter()
        .map(|comment| {
            let id = json_id_to_string(comment.get("id"))
                .unwrap_or_else(|| format!("pr-{pr_number}-review-comment"));
            let line = comment.get("line").and_then(Value::as_i64);
            let original_line = comment.get("original_line").and_then(Value::as_i64);
            let start_line = comment
                .get("start_line")
                .and_then(Value::as_i64)
                .or_else(|| comment.get("original_start_line").and_then(Value::as_i64))
                .or(line)
                .or(original_line);
            let end_line = line.or(original_line).or(start_line);
            let side = comment
                .get("side")
                .and_then(Value::as_str)
                .or_else(|| comment.get("diff_side").and_then(Value::as_str))
                .map(|side| side.to_ascii_lowercase());
            PrDiffAnnotation {
                id: format!("review-comment:{id}"),
                source: "review_comment".to_string(),
                path: comment
                    .get("path")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                side,
                start_line,
                end_line,
                start_column: None,
                end_column: None,
                level: "comment".to_string(),
                status: None,
                title: None,
                message: comment
                    .get("body")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                author: comment
                    .get("user")
                    .and_then(|user| user.get("login"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
                check_name: None,
                url: comment
                    .get("html_url")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                is_outdated: line.is_none() && original_line.is_some(),
                created_at: comment
                    .get("created_at")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            }
        })
        .collect())
}

pub(crate) fn parse_check_runs_output(json_str: &str) -> AppResult<Vec<CheckRunAnnotationSource>> {
    let value: Value = serde_json::from_str(json_str).map_err(|e| {
        AppError::Infrastructure(format!(
            "Failed to parse gh check runs JSON: {e}\nRaw: {json_str}"
        ))
    })?;

    let pages: Vec<&Value> = match value.as_array() {
        Some(array) if array.iter().all(Value::is_object) => array.iter().collect(),
        _ if value.is_object() => vec![&value],
        _ => {
            return Err(AppError::Infrastructure(format!(
                "gh check runs: expected JSON object/pages, got: {json_str}"
            )));
        }
    };

    let mut check_runs = Vec::new();
    for page in pages {
        let Some(runs) = page.get("check_runs").and_then(Value::as_array) else {
            continue;
        };
        for run in runs {
            let Some(id) = run.get("id").and_then(Value::as_i64) else {
                continue;
            };
            check_runs.push(CheckRunAnnotationSource {
                id,
                name: run
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("GitHub check")
                    .to_string(),
                conclusion: run
                    .get("conclusion")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                status: run
                    .get("status")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                html_url: run
                    .get("html_url")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                annotations_count: run
                    .get("annotations_count")
                    .and_then(Value::as_i64)
                    .unwrap_or(0),
            });
        }
    }
    Ok(check_runs)
}

pub(crate) fn parse_check_run_annotations_output(
    check_run: &CheckRunAnnotationSource,
    annotations_json: &str,
) -> AppResult<Vec<PrDiffAnnotation>> {
    let annotations_value: Value = serde_json::from_str(annotations_json).map_err(|e| {
        AppError::Infrastructure(format!(
            "Failed to parse gh check run annotations JSON: {e}\nRaw: {annotations_json}"
        ))
    })?;
    let annotations = flatten_paginated_array(&annotations_value).ok_or_else(|| {
        AppError::Infrastructure(format!(
            "gh check run annotations: expected JSON array/pages, got: {annotations_json}"
        ))
    })?;

    Ok(annotations
        .into_iter()
        .enumerate()
        .map(|(idx, annotation)| {
            let path = annotation
                .get("path")
                .and_then(Value::as_str)
                .map(str::to_string);
            let start_line = annotation.get("start_line").and_then(Value::as_i64);
            let end_line = annotation
                .get("end_line")
                .and_then(Value::as_i64)
                .or(start_line);
            let title = annotation
                .get("title")
                .and_then(Value::as_str)
                .filter(|title| !title.trim().is_empty())
                .map(str::to_string);
            let message = annotation
                .get("message")
                .and_then(Value::as_str)
                .or_else(|| annotation.get("raw_details").and_then(Value::as_str))
                .unwrap_or_default()
                .to_string();
            PrDiffAnnotation {
                id: format!("check-run:{}:{idx}", check_run.id),
                source: "check_run".to_string(),
                path,
                side: Some("right".to_string()),
                start_line,
                end_line,
                start_column: annotation.get("start_column").and_then(Value::as_i64),
                end_column: annotation.get("end_column").and_then(Value::as_i64),
                level: annotation
                    .get("annotation_level")
                    .and_then(Value::as_str)
                    .unwrap_or("warning")
                    .to_string(),
                status: check_run
                    .conclusion
                    .clone()
                    .or_else(|| check_run.status.clone()),
                title,
                message,
                author: None,
                check_name: Some(check_run.name.clone()),
                url: annotation
                    .get("blob_href")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or_else(|| check_run.html_url.clone()),
                is_outdated: false,
                created_at: None,
            }
        })
        .collect())
}

pub(crate) fn parse_code_scanning_alert_annotations_output(
    alerts_json: &str,
) -> AppResult<Vec<PrDiffAnnotation>> {
    let alerts_value: Value = serde_json::from_str(alerts_json).map_err(|e| {
        AppError::Infrastructure(format!(
            "Failed to parse gh code scanning alerts JSON: {e}\nRaw: {alerts_json}"
        ))
    })?;
    let alerts = flatten_paginated_array(&alerts_value).ok_or_else(|| {
        AppError::Infrastructure(format!(
            "gh code scanning alerts: expected JSON array/pages, got: {alerts_json}"
        ))
    })?;

    Ok(alerts
        .into_iter()
        .filter_map(|alert| {
            let alert_number = json_id_to_string(alert.get("number")).unwrap_or_default();
            let instance = alert.get("most_recent_instance")?;
            let location = instance.get("location")?;
            let path = location
                .get("path")
                .and_then(Value::as_str)
                .map(str::to_string);
            let rule = alert.get("rule");
            let tool_name = alert
                .get("tool")
                .and_then(|tool| tool.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("Code scanning")
                .to_string();
            let title = rule
                .and_then(|rule| rule.get("description"))
                .and_then(Value::as_str)
                .or_else(|| {
                    rule.and_then(|rule| rule.get("name"))
                        .and_then(Value::as_str)
                })
                .or_else(|| rule.and_then(|rule| rule.get("id")).and_then(Value::as_str))
                .map(str::to_string);
            let message = instance
                .get("message")
                .and_then(|message| message.get("text"))
                .and_then(Value::as_str)
                .or(title.as_deref())
                .unwrap_or_default()
                .to_string();
            let level = rule
                .and_then(|rule| rule.get("security_severity_level"))
                .and_then(Value::as_str)
                .or_else(|| {
                    rule.and_then(|rule| rule.get("severity"))
                        .and_then(Value::as_str)
                })
                .unwrap_or("warning")
                .to_string();
            Some(PrDiffAnnotation {
                id: format!("code-scanning:{alert_number}"),
                source: "code_scanning".to_string(),
                path,
                side: Some("right".to_string()),
                start_line: location.get("start_line").and_then(Value::as_i64),
                end_line: location
                    .get("end_line")
                    .and_then(Value::as_i64)
                    .or_else(|| location.get("start_line").and_then(Value::as_i64)),
                start_column: location.get("start_column").and_then(Value::as_i64),
                end_column: location.get("end_column").and_then(Value::as_i64),
                level,
                status: alert
                    .get("state")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                title,
                message,
                author: None,
                check_name: Some(tool_name),
                url: alert
                    .get("html_url")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                is_outdated: false,
                created_at: alert
                    .get("created_at")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            })
        })
        .collect())
}

fn flatten_paginated_array(value: &Value) -> Option<Vec<&Value>> {
    let array = value.as_array()?;
    if array.iter().all(Value::is_array) {
        Some(
            array
                .iter()
                .flat_map(|page| page.as_array().into_iter().flatten())
                .collect(),
        )
    } else {
        Some(array.iter().collect())
    }
}

fn json_id_to_string(value: Option<&Value>) -> Option<String> {
    let value = value?;
    if let Some(id) = value.as_i64() {
        return Some(id.to_string());
    }
    value.as_str().map(str::to_string)
}

fn review_sort_key(review: &Value) -> String {
    review
        .get("submitted_at")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| json_id_to_string(review.get("id")))
        .unwrap_or_default()
}
