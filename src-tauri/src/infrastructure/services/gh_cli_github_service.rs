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
use std::collections::{BTreeSet, HashMap};
use std::ffi::OsString;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::time::{timeout, Duration};
use tracing::{debug, warn};

use crate::domain::services::github_service::{
    validate_pr_metadata_patch, GithubConnectionStatus, GithubServiceTrait,
    PrAnnotationSourceUnavailable, PrAutoMergeRequest, PrBranchMatch, PrDetail, PrDiffAnnotation,
    PrDiffAnnotations, PrHealth, PrHealthCheck, PrIssueCommentSummary, PrMergeStateStatus,
    PrMergeableState, PrReviewCommentFeedback, PrReviewFeedback, PrReviewSubmissionEvent,
    PrReviewThread, PrReviewThreadComment, PrSearchResult, PrStatus, PrSubmittedReview,
    PrSyncState,
};
use crate::error::AppError;
use crate::infrastructure::agents::claude::git_runtime_config;
use crate::infrastructure::git_auth::{
    apply_git_subprocess_env, git_auth_error_from_failure, probe_github_connection_status,
    GitNetworkOperation,
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
    async fn run_gh_connection_probe(&self) -> GithubConnectionStatus;
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

    async fn run_gh_connection_probe(&self) -> GithubConnectionStatus {
        probe_github_connection_status().await
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
        let mut command = tokio::process::Command::new(resolve_gh_cli_path());
        apply_git_subprocess_env(&mut command);
        let mut child = command
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

fn build_exact_force_with_lease_push_args(
    local_ref: &str,
    expected_remote_oid: &str,
) -> AppResult<Vec<String>> {
    let Some(branch) = local_ref.strip_prefix("refs/heads/") else {
        return Err(AppError::Validation(
            "exact force-with-lease requires a fully-qualified local branch ref".to_string(),
        ));
    };
    let invalid_branch = branch.is_empty()
        || branch.starts_with('/')
        || branch.ends_with('/')
        || branch.ends_with('.')
        || branch.ends_with(".lock")
        || branch.contains("..")
        || branch.contains("@{")
        || branch.contains("//")
        || branch
            .chars()
            .any(|ch| ch.is_control() || ch.is_whitespace() || "~^:?*[\\".contains(ch));
    if invalid_branch {
        return Err(AppError::Validation(
            "exact force-with-lease requires a valid fully-qualified local branch ref".to_string(),
        ));
    }
    if expected_remote_oid.len() != 40
        || !expected_remote_oid
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        return Err(AppError::Validation(
            "exact force-with-lease requires a full expected remote OID".to_string(),
        ));
    }

    Ok(vec![
        "push".to_string(),
        "origin".to_string(),
        format!("--force-with-lease={local_ref}:{expected_remote_oid}"),
        format!("{local_ref}:{local_ref}"),
    ])
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

fn build_create_issue_args(repository: &str, title: &str, body_file: &str) -> Vec<String> {
    vec![
        "issue".to_string(),
        "create".to_string(),
        "--repo".to_string(),
        repository.to_string(),
        "--title".to_string(),
        title.to_string(),
        "--body-file".to_string(),
        body_file.to_string(),
    ]
}

fn build_update_pr_args(
    pr_number: i64,
    title: Option<&str>,
    body_file: Option<&str>,
) -> AppResult<Vec<String>> {
    validate_pr_metadata_patch(title, body_file.map(Path::new))?;
    let mut args = vec!["pr".to_string(), "edit".to_string(), pr_number.to_string()];
    if let Some(title) = title {
        args.extend(["--title".to_string(), title.to_string()]);
    }
    if let Some(body_file) = body_file {
        args.extend(["--body-file".to_string(), body_file.to_string()]);
    }
    Ok(args)
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

fn build_pr_health_view_args(pr_number: i64) -> Vec<String> {
    vec![
        "pr".to_string(),
        "view".to_string(),
        pr_number.to_string(),
        "--json".to_string(),
        "state,mergeStateStatus,mergeable,isDraft,headRefName,baseRefName,headRefOid,baseRefOid,mergedAt,mergeCommit,reviewDecision,statusCheckRollup,autoMergeRequest".to_string(),
    ]
}

fn build_pr_detail_view_args(pr_number: i64) -> Vec<String> {
    vec![
        "pr".to_string(),
        "view".to_string(),
        pr_number.to_string(),
        "--json".to_string(),
        "number,title,body,author,createdAt,url,state,isDraft,headRefName,baseRefName,mergeCommit"
            .to_string(),
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

fn build_submit_pr_review_api_args(
    pr_number: i64,
    event: PrReviewSubmissionEvent,
    body: &str,
) -> Vec<String> {
    vec![
        "api".to_string(),
        format!("repos/{{owner}}/{{repo}}/pulls/{pr_number}/reviews"),
        "-X".to_string(),
        "POST".to_string(),
        "-f".to_string(),
        format!("event={event}"),
        "-f".to_string(),
        format!("body={body}"),
    ]
}

fn build_pr_issue_comments_api_args(pr_number: i64) -> Vec<String> {
    vec![
        "api".to_string(),
        format!("repos/{{owner}}/{{repo}}/issues/{pr_number}/comments"),
        "--paginate".to_string(),
        "--slurp".to_string(),
    ]
}

fn build_pr_enable_auto_merge_args(pr_number: i64, method: &str) -> Vec<String> {
    let method_flag = match method {
        "merge" => "--merge",
        "rebase" => "--rebase",
        _ => "--squash",
    };
    vec![
        "pr".to_string(),
        "merge".to_string(),
        pr_number.to_string(),
        "--auto".to_string(),
        method_flag.to_string(),
    ]
}

fn build_pr_disable_auto_merge_args(pr_number: i64) -> Vec<String> {
    vec![
        "pr".to_string(),
        "merge".to_string(),
        pr_number.to_string(),
        "--disable-auto".to_string(),
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

fn build_pr_diff_patch_args(pr_number: i64, pr_url: Option<&str>) -> Vec<String> {
    let selector = pr_url
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| pr_number.to_string());
    vec![
        "pr".to_string(),
        "diff".to_string(),
        selector,
        "--patch".to_string(),
        "--color".to_string(),
        "never".to_string(),
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
    async fn create_issue(
        &self,
        working_dir: &Path,
        repository: &str,
        title: &str,
        body_file: &Path,
    ) -> AppResult<String> {
        let body_file_str = body_file
            .to_str()
            .ok_or_else(|| {
                AppError::Infrastructure("body_file path is not valid UTF-8".to_string())
            })?
            .to_string();

        let stdout = self
            .runner
            .run_gh(
                working_dir,
                &build_create_issue_args(repository, title, &body_file_str),
            )
            .await?;

        parse_issue_create_plain_output(&stdout.join("\n"))
    }

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
        let args = build_update_pr_args(pr_number, Some(title), Some(&body_file_str))?;
        self.runner.run_gh(working_dir, &args).await?;
        Ok(())
    }

    async fn patch_pr_metadata(
        &self,
        working_dir: &Path,
        pr_number: i64,
        title: Option<&str>,
        body_file: Option<&Path>,
    ) -> AppResult<()> {
        let has_title = title.is_some();
        let has_body_file = body_file.is_some();
        let body_file = body_file
            .map(|path| {
                path.to_str().ok_or_else(|| {
                    AppError::Infrastructure("body_file path is not valid UTF-8".to_string())
                })
            })
            .transpose()?;
        let args = build_update_pr_args(pr_number, title, body_file)?;
        debug!(
            pr_number,
            has_title,
            has_body_file,
            result_class = "attempt",
            "Patching pull-request metadata"
        );
        let result = self.runner.run_gh(working_dir, &args).await;
        if result.is_ok() {
            debug!(
                pr_number,
                has_title,
                has_body_file,
                result_class = "success",
                "Patching pull-request metadata"
            );
        } else {
            warn!(
                pr_number,
                has_title,
                has_body_file,
                result_class = "error",
                "Patching pull-request metadata"
            );
        }
        result.map(|_| ())
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

    async fn fetch_pr_detail(&self, working_dir: &Path, pr_number: i64) -> AppResult<PrDetail> {
        let stdout = self
            .runner
            .run_gh(working_dir, &build_pr_detail_view_args(pr_number))
            .await?;
        parse_pr_detail_output(pr_number, &stdout.join("\n"))
    }

    async fn fetch_pr_review_thread(
        &self,
        working_dir: &Path,
        pr_number: i64,
    ) -> AppResult<PrReviewThread> {
        // Reuses the same review-comments source as `fetch_pr_diff_annotations`,
        // but returns the conversation thread directly (live, transient) so the
        // detail view never triggers the opt-in check-run/code-scanning fan-out.
        let stdout = self
            .runner
            .run_gh(working_dir, &build_pr_review_comments_api_args(pr_number))
            .await?;
        parse_pr_review_thread_output(pr_number, &stdout.join("\n"))
    }

    async fn submit_pr_review(
        &self,
        working_dir: &Path,
        pr_number: i64,
        event: PrReviewSubmissionEvent,
        body: &str,
    ) -> AppResult<PrSubmittedReview> {
        let stdout = self
            .runner
            .run_gh(
                working_dir,
                &build_submit_pr_review_api_args(pr_number, event, body),
            )
            .await?;
        parse_submit_pr_review_output(&stdout.join("\n"))
    }

    async fn fetch_pr_health(&self, working_dir: &Path, pr_number: i64) -> AppResult<PrHealth> {
        let view_stdout = self
            .runner
            .run_gh(working_dir, &build_pr_health_view_args(pr_number))
            .await?;
        let comments_stdout = self
            .runner
            .run_gh(working_dir, &build_pr_issue_comments_api_args(pr_number))
            .await?;
        parse_pr_health_output(&view_stdout.join("\n"), &comments_stdout.join("\n"))
    }

    async fn list_branch_check_conclusions(
        &self,
        working_dir: &Path,
        branch_ref: &str,
    ) -> AppResult<Option<Vec<PrHealthCheck>>> {
        let branch_ref = branch_ref.trim();
        if branch_ref.is_empty() {
            return Ok(None);
        }
        // The latest completed workflow runs on the branch tip. `gh run list` is the only surface
        // that reports checks for a branch with no pull request of its own, which is exactly the
        // base-branch case this exists for.
        let stdout = self
            .runner
            .run_gh(
                working_dir,
                &[
                    "run".to_string(),
                    "list".to_string(),
                    "--branch".to_string(),
                    branch_ref.to_string(),
                    "--limit".to_string(),
                    "40".to_string(),
                    "--json".to_string(),
                    "name,workflowName,status,conclusion,url,headSha".to_string(),
                ],
            )
            .await?;

        Ok(Some(parse_branch_check_conclusions(&stdout.join("\n"))))
    }

    async fn rerun_failed_workflow(&self, working_dir: &Path, run_id: i64) -> AppResult<()> {
        if run_id <= 0 {
            return Err(AppError::Validation(
                "GitHub Actions run id must be positive".to_string(),
            ));
        }
        self.runner
            .run_gh(
                working_dir,
                &[
                    "run".to_string(),
                    "rerun".to_string(),
                    run_id.to_string(),
                    "--failed".to_string(),
                ],
            )
            .await
            .map(|_| ())
    }

    async fn enable_pr_auto_merge(
        &self,
        working_dir: &Path,
        pr_number: i64,
        method: &str,
    ) -> AppResult<()> {
        self.runner
            .run_gh(
                working_dir,
                &build_pr_enable_auto_merge_args(pr_number, method),
            )
            .await?;
        Ok(())
    }

    async fn disable_pr_auto_merge(&self, working_dir: &Path, pr_number: i64) -> AppResult<()> {
        self.runner
            .run_gh(working_dir, &build_pr_disable_auto_merge_args(pr_number))
            .await?;
        Ok(())
    }

    async fn push_branch(&self, working_dir: &Path, branch: &str) -> AppResult<()> {
        // git push origin <branch> — fire-and-forget style (stdout null, stderr piped for safety)
        let args = vec!["push".to_string(), "origin".to_string(), branch.to_string()];
        self.runner.run_git(working_dir, &args).await
    }

    async fn push_branch_with_expected_remote_oid_lease(
        &self,
        working_dir: &Path,
        local_ref: &str,
        expected_remote_oid: &str,
    ) -> AppResult<()> {
        let args = build_exact_force_with_lease_push_args(local_ref, expected_remote_oid)?;
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

    async fn get_pr_diff_patch(
        &self,
        working_dir: &Path,
        pr_number: i64,
        pr_url: Option<&str>,
    ) -> AppResult<String> {
        let stdout = self
            .runner
            .run_gh(working_dir, &build_pr_diff_patch_args(pr_number, pr_url))
            .await?;
        Ok(stdout.join("\n"))
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

    async fn search_pull_requests(
        &self,
        working_dir: &Path,
        query: Option<&str>,
        limit: usize,
    ) -> AppResult<Vec<PrSearchResult>> {
        let limit = limit.clamp(1, 50);
        let mut args = vec![
            "pr".to_string(),
            "list".to_string(),
            "--state".to_string(),
            "open".to_string(),
            "--limit".to_string(),
            limit.to_string(),
            "--json".to_string(),
            "number,title,url,headRefName,headRefOid,baseRefName,isDraft,updatedAt,author,assignees,reviewDecision,latestReviews,reviewRequests,isCrossRepository".to_string(),
        ];
        if let Some(query) = query.map(str::trim).filter(|value| !value.is_empty()) {
            args.push("--search".to_string());
            args.push(query.to_string());
        }
        let stdout = self.runner.run_gh(working_dir, &args).await?;

        let json_str = stdout.join("\n");
        parse_pr_search_output(&json_str)
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

    async fn list_pull_request_branch_matches(
        &self,
        working_dir: &Path,
        limit: usize,
    ) -> AppResult<Vec<PrBranchMatch>> {
        let limit = limit.clamp(1, 200);
        let args = vec![
            "pr".to_string(),
            "list".to_string(),
            "--state".to_string(),
            "all".to_string(),
            "--limit".to_string(),
            limit.to_string(),
            "--json".to_string(),
            "number,url,state,isDraft,headRefName,updatedAt,author".to_string(),
        ];
        let stdout = self.runner.run_gh(working_dir, &args).await?;

        let json_str = stdout.join("\n");
        parse_pr_branch_matches_output(&json_str)
    }

    async fn fetch_github_connection_status(&self) -> AppResult<GithubConnectionStatus> {
        Ok(self.runner.run_gh_connection_probe().await)
    }
}

// ── Output parsers ────────────────────────────────────────────────────────────

/// Parse `gh auth status` output lines into `(authenticated, host, account)`.
///
/// Prefers the account block marked `Active account: true`; falls back to the
/// first authenticated block (older `gh` has no active-account marker). Token
/// lines are ignored — only the `Logged in to <host> account <account>` lines
/// carry the host/account we surface.
#[cfg(test)]
pub(crate) fn parse_gh_auth_status_lines(
    lines: &[String],
) -> (bool, Option<String>, Option<String>) {
    let mut entries: Vec<(String, String)> = Vec::new();
    let mut active_index: Option<usize> = None;

    for line in lines {
        if let Some((host, account)) = parse_logged_in_line(line) {
            entries.push((host, account));
        } else if is_active_account_true(line) {
            if let Some(last) = entries.len().checked_sub(1) {
                active_index = Some(last);
            }
        }
    }

    let Some((host, account)) = active_index
        .or(if entries.is_empty() { None } else { Some(0) })
        .map(|index| entries[index].clone())
    else {
        return (false, None, None);
    };

    (true, Some(host), Some(account))
}

/// Extract `(host, account)` from a `✓ Logged in to <host> account <account> (...)` line.
#[cfg(test)]
fn parse_logged_in_line(line: &str) -> Option<(String, String)> {
    const LOGGED_IN: &str = "Logged in to ";
    const ACCOUNT: &str = " account ";
    let start = line.find(LOGGED_IN)?;
    let rest = &line[start + LOGGED_IN.len()..];
    let account_at = rest.find(ACCOUNT)?;
    let host = rest[..account_at].trim().to_string();
    let after = rest[account_at + ACCOUNT.len()..].trim();
    let account = after.split_whitespace().next()?.to_string();
    if host.is_empty() || account.is_empty() {
        return None;
    }
    Some((host, account))
}

/// True for the `- Active account: true` marker line.
#[cfg(test)]
fn is_active_account_true(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.contains("active account:") && lower.contains("true")
}

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

pub(crate) fn parse_issue_create_plain_output(stdout_str: &str) -> AppResult<String> {
    stdout_str
        .split_whitespace()
        .map(|token| token.trim_matches(|c: char| "()[]<>{},'\"".contains(c)))
        .find(|token| token.starts_with("https://") && token.contains("/issues/"))
        .map(str::to_string)
        .ok_or_else(|| {
            AppError::Infrastructure(format!(
                "gh issue create: could not find issue URL in output: {stdout_str}"
            ))
        })
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

pub(crate) fn parse_pr_branch_matches_output(json_str: &str) -> AppResult<Vec<PrBranchMatch>> {
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

    items
        .iter()
        .filter(|item| {
            item.get("headRefName")
                .and_then(Value::as_str)
                .is_some_and(|head| !head.trim().is_empty())
        })
        .map(parse_pr_branch_match_item)
        .collect()
}

pub(crate) fn parse_pr_search_output(json_str: &str) -> AppResult<Vec<PrSearchResult>> {
    let arr: Value = serde_json::from_str(json_str).map_err(|e| {
        AppError::Infrastructure(format!(
            "Failed to parse gh pr list search JSON: {e}\nRaw: {json_str}"
        ))
    })?;

    let items = arr.as_array().ok_or_else(|| {
        AppError::Infrastructure(format!(
            "gh pr list search: expected JSON array, got: {json_str}"
        ))
    })?;

    items.iter().map(parse_pr_search_item).collect()
}

fn parse_pr_search_item(item: &Value) -> AppResult<PrSearchResult> {
    let context = "gh pr list search";
    let number = item["number"].as_i64().ok_or_else(|| {
        AppError::Infrastructure("gh pr list search: missing 'number' field".to_string())
    })?;

    Ok(PrSearchResult {
        number,
        title: required_string(item, "title", context)?,
        url: required_string(item, "url", context)?,
        head_ref_name: required_string(item, "headRefName", context)?,
        head_ref_oid: item
            .get("headRefOid")
            .and_then(Value::as_str)
            .map(str::to_string),
        base_ref_name: required_string(item, "baseRefName", context)?,
        is_draft: item
            .get("isDraft")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        updated_at: item
            .get("updatedAt")
            .and_then(Value::as_str)
            .map(str::to_string),
        author_login: item
            .get("author")
            .and_then(|author| author.get("login"))
            .and_then(Value::as_str)
            .map(str::to_string),
        assignee_logins: parse_login_array(item.get("assignees")),
        review_decision: item
            .get("reviewDecision")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        latest_review_author_logins: parse_latest_review_author_logins(item.get("latestReviews")),
        review_request_logins: parse_review_request_logins(item.get("reviewRequests")),
        is_cross_repository: item
            .get("isCrossRepository")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

fn parse_login_array(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("login").and_then(Value::as_str))
                .map(str::trim)
                .filter(|login| !login.is_empty())
                .map(str::to_string)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect()
        })
        .unwrap_or_default()
}

fn parse_latest_review_author_logins(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    item.get("author")
                        .and_then(|author| author.get("login"))
                        .and_then(Value::as_str)
                })
                .map(str::trim)
                .filter(|login| !login.is_empty())
                .map(str::to_string)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect()
        })
        .unwrap_or_default()
}

fn parse_review_request_logins(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    item.get("login")
                        .or_else(|| item.get("slug"))
                        .or_else(|| item.get("name"))
                        .and_then(Value::as_str)
                })
                .map(str::trim)
                .filter(|login| !login.is_empty())
                .map(str::to_string)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect()
        })
        .unwrap_or_default()
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
        author_login: item
            .get("author")
            .and_then(|author| author.get("login"))
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
            let merged_at = v["mergedAt"].as_str().map(str::to_string);
            Ok(PrStatus::Merged {
                merge_commit_sha: sha,
                merged_at,
            })
        }
        other => Err(AppError::Infrastructure(format!(
            "gh pr view: unknown state '{other}'"
        ))),
    }
}

/// Parse `gh pr view <n> --json title,body,author,...` into a [`PrDetail`].
pub(crate) fn parse_pr_detail_output(pr_number: i64, json_str: &str) -> AppResult<PrDetail> {
    let v: Value = serde_json::from_str(json_str).map_err(|e| {
        AppError::Infrastructure(format!(
            "Failed to parse gh pr view detail JSON: {e}\nRaw: {json_str}"
        ))
    })?;

    let context = "gh pr view detail";
    let state = parse_pr_status_value(&v)?;
    let head_ref_name = required_string(&v, "headRefName", context)?;
    let base_ref_name = required_string(&v, "baseRefName", context)?;

    Ok(PrDetail {
        number: v.get("number").and_then(Value::as_i64).unwrap_or(pr_number),
        title: v
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        body: v
            .get("body")
            .and_then(Value::as_str)
            .filter(|body| !body.is_empty())
            .map(str::to_string),
        author: v
            .get("author")
            .and_then(|author| author.get("login"))
            .and_then(Value::as_str)
            .map(str::to_string),
        created_at: v
            .get("createdAt")
            .and_then(Value::as_str)
            .map(str::to_string),
        url: v.get("url").and_then(Value::as_str).map(str::to_string),
        state,
        is_draft: v.get("isDraft").and_then(Value::as_bool).unwrap_or(false),
        head_ref_name,
        base_ref_name,
    })
}

pub(crate) fn parse_pr_sync_state_output(json_str: &str) -> AppResult<PrSyncState> {
    let v: Value = serde_json::from_str(json_str).map_err(|e| {
        AppError::Infrastructure(format!(
            "Failed to parse gh pr view sync-state JSON: {e}\nRaw: {json_str}"
        ))
    })?;

    parse_pr_sync_state_value(&v, "gh pr view sync-state")
}

fn parse_pr_sync_state_value(v: &Value, context: &str) -> AppResult<PrSyncState> {
    let status = parse_pr_status_value(v)?;
    let head_ref_name = required_string(v, "headRefName", context)?;
    let base_ref_name = required_string(v, "baseRefName", context)?;

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

pub(crate) fn parse_pr_health_output(view_json: &str, comments_json: &str) -> AppResult<PrHealth> {
    let view_value: Value = serde_json::from_str(view_json).map_err(|e| {
        AppError::Infrastructure(format!(
            "Failed to parse gh pr view health JSON: {e}\nRaw: {view_json}"
        ))
    })?;
    let comments_value: Value = serde_json::from_str(comments_json).map_err(|e| {
        AppError::Infrastructure(format!(
            "Failed to parse gh PR comments JSON: {e}\nRaw: {comments_json}"
        ))
    })?;

    Ok(PrHealth {
        sync_state: parse_pr_sync_state_value(&view_value, "gh pr view health")?,
        review_decision: view_value
            .get("reviewDecision")
            .and_then(Value::as_str)
            .map(str::to_string),
        checks: parse_status_check_rollup(&view_value),
        issue_comments: parse_pr_issue_comments_value(&comments_value)?,
        auto_merge_request: parse_auto_merge_request(&view_value),
    })
}

fn parse_status_check_rollup(view_value: &Value) -> Vec<PrHealthCheck> {
    view_value
        .get("statusCheckRollup")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|check| {
            let name = check
                .get("name")
                .or_else(|| check.get("context"))
                .or_else(|| check.get("workflowName"))
                .and_then(Value::as_str)
                .unwrap_or("GitHub check")
                .trim()
                .to_string();
            if name.is_empty() {
                return None;
            }
            Some(PrHealthCheck {
                name,
                status: check
                    .get("status")
                    .or_else(|| check.get("state"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
                conclusion: check
                    .get("conclusion")
                    .or_else(|| check.get("state"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
                details_url: check
                    .get("detailsUrl")
                    .or_else(|| check.get("targetUrl"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
            })
        })
        .collect()
}

/// Reduces `gh run list --json ...` output to the newest completed conclusion per check name.
/// Older runs for the same check are historical noise; only the current state of the branch tip
/// can tell us whether a failure already exists on the base.
fn parse_branch_check_conclusions(json_str: &str) -> Vec<PrHealthCheck> {
    let Ok(runs) = serde_json::from_str::<Value>(json_str) else {
        return Vec::new();
    };
    let mut newest_by_name: std::collections::BTreeMap<String, PrHealthCheck> =
        std::collections::BTreeMap::new();
    for run in runs.as_array().into_iter().flatten() {
        let name = run
            .get("name")
            .or_else(|| run.get("workflowName"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();
        if name.is_empty() {
            continue;
        }
        let status = run.get("status").and_then(Value::as_str).map(str::to_string);
        // An in-progress run proves nothing about the base yet.
        if !status
            .as_deref()
            .is_some_and(|value| value.eq_ignore_ascii_case("completed"))
        {
            continue;
        }
        newest_by_name.entry(name.clone()).or_insert(PrHealthCheck {
            name,
            status,
            conclusion: run
                .get("conclusion")
                .and_then(Value::as_str)
                .map(str::to_string),
            details_url: run.get("url").and_then(Value::as_str).map(str::to_string),
        });
    }
    newest_by_name.into_values().collect()
}

fn parse_auto_merge_request(view_value: &Value) -> Option<PrAutoMergeRequest> {
    let request = view_value.get("autoMergeRequest")?;
    if request.is_null() {
        return None;
    }
    Some(PrAutoMergeRequest {
        enabled_by: request
            .get("enabledBy")
            .and_then(|user| user.get("login"))
            .and_then(Value::as_str)
            .map(str::to_string),
        merge_method: request
            .get("mergeMethod")
            .and_then(Value::as_str)
            .map(|method| method.to_ascii_lowercase()),
    })
}

fn parse_pr_issue_comments_value(value: &Value) -> AppResult<Vec<PrIssueCommentSummary>> {
    let comments = flatten_paginated_array(value).ok_or_else(|| {
        AppError::Infrastructure("gh PR comments: expected JSON array/pages".to_string())
    })?;
    Ok(comments
        .into_iter()
        .rev()
        .take(20)
        .map(|comment| {
            let author = comment
                .get("user")
                .and_then(|user| user.get("login"))
                .and_then(Value::as_str)
                .map(str::to_string);
            let is_bot = comment
                .get("user")
                .and_then(|user| user.get("type"))
                .and_then(Value::as_str)
                .is_some_and(|kind| kind.eq_ignore_ascii_case("bot"));
            let body = comment
                .get("body")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let source = format!(
                "{}\n{}",
                author.as_deref().unwrap_or_default().to_ascii_lowercase(),
                body.to_ascii_lowercase()
            );
            PrIssueCommentSummary {
                id: json_id_to_string(comment.get("id")).unwrap_or_default(),
                author,
                body,
                url: comment
                    .get("html_url")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                created_at: comment
                    .get("created_at")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                updated_at: comment
                    .get("updated_at")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                is_bot,
                is_codecov: source.contains("codecov"),
            }
        })
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect())
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
            let merged_at = v["mergedAt"].as_str().map(str::to_string);
            Ok(PrStatus::Merged {
                merge_commit_sha: sha,
                merged_at,
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

pub(crate) fn parse_submit_pr_review_output(json_str: &str) -> AppResult<PrSubmittedReview> {
    let value: Value = serde_json::from_str(json_str).map_err(|e| {
        AppError::Infrastructure(format!(
            "Failed to parse submitted PR review JSON: {e}\nRaw: {json_str}"
        ))
    })?;
    let id = json_id_to_string(value.get("id")).ok_or_else(|| {
        AppError::Infrastructure("submitted PR review response missing id".to_string())
    })?;
    let url = value
        .get("html_url")
        .and_then(Value::as_str)
        .map(str::to_string);
    Ok(PrSubmittedReview { id, url })
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

/// Parse the PR review-comments API payload into a live [`PrReviewThread`].
///
/// Shares the review-comments source with `parse_pr_review_comment_annotations_output`
/// but preserves the conversation shape (author/body/reply linkage) rather than
/// projecting onto diff annotations.
pub(crate) fn parse_pr_review_thread_output(
    pr_number: i64,
    comments_json: &str,
) -> AppResult<PrReviewThread> {
    let comments_value: Value = serde_json::from_str(comments_json).map_err(|e| {
        AppError::Infrastructure(format!(
            "Failed to parse gh review thread JSON: {e}\nRaw: {comments_json}"
        ))
    })?;
    let comments = flatten_paginated_array(&comments_value).ok_or_else(|| {
        AppError::Infrastructure(format!(
            "gh review thread: expected JSON array/pages, got: {comments_json}"
        ))
    })?;

    let comments = comments
        .into_iter()
        .map(|comment| {
            let id = json_id_to_string(comment.get("id"))
                .unwrap_or_else(|| format!("pr-{pr_number}-review-comment"));
            let line = comment.get("line").and_then(Value::as_i64);
            let original_line = comment.get("original_line").and_then(Value::as_i64);
            PrReviewThreadComment {
                id,
                author: comment
                    .get("user")
                    .and_then(|user| user.get("login"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
                body: comment
                    .get("body")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                path: comment
                    .get("path")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                side: comment
                    .get("side")
                    .and_then(Value::as_str)
                    .or_else(|| comment.get("diff_side").and_then(Value::as_str))
                    .map(|side| side.to_ascii_lowercase()),
                line: line.or(original_line),
                url: comment
                    .get("html_url")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                created_at: comment
                    .get("created_at")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                in_reply_to_id: json_id_to_string(comment.get("in_reply_to_id")),
                is_outdated: line.is_none() && original_line.is_some(),
            }
        })
        .collect();

    Ok(PrReviewThread {
        pr_number,
        comments,
    })
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
