//! `git clone` with streamed progress, a bounded deadline, and cleanup on every
//! exit path.
//!
//! Runs on [`GitCommandLane::Clone`] rather than the background lane, because a
//! clone can take minutes and the background lane has a single global permit
//! shared by every workspace automation.
//!
//! Cancellation, timeout, and failure all remove exactly what RalphX created and
//! nothing else — a destination the user already populated is refused before the
//! process is ever spawned.

use serde::Serialize;
use std::path::{Component, Path, PathBuf};

use crate::error::{AppError, AppResult};
use crate::infrastructure::agents::claude::git_runtime_config;
use crate::infrastructure::git_auth::is_git_auth_failure_text;
use crate::utils::path_safety::{
    checked_read_dir, checked_remove_dir_all, require_under_root, validate_absolute_non_root_path,
};

use super::clone_progress::parse_clone_progress;
use super::git_cmd::{self, GitCommandLane, StreamedGitOutcome};
use super::GitService;

pub const CLONE_URL_INVALID: &str = "CLONE_URL_INVALID";
pub const CLONE_DEST_INVALID: &str = "CLONE_DEST_INVALID";
pub const CLONE_DEST_NOT_EMPTY: &str = "CLONE_DEST_NOT_EMPTY";
pub const CLONE_AUTH_FAILED: &str = "CLONE_AUTH_FAILED";
pub const CLONE_NOT_FOUND: &str = "CLONE_NOT_FOUND";
pub const CLONE_NETWORK: &str = "CLONE_NETWORK";
pub const CLONE_TIMEOUT: &str = "CLONE_TIMEOUT";
pub const CLONE_CANCELLED: &str = "CLONE_CANCELLED";
pub const CLONE_UNKNOWN: &str = "CLONE_UNKNOWN";

/// Phases reported by `git clone --progress`, in the order Git emits them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClonePhase {
    Connecting,
    Counting,
    Compressing,
    Receiving,
    Resolving,
    CheckingOut,
}

/// One parsed progress update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloneProgress {
    pub phase: ClonePhase,
    pub percent: Option<u8>,
    pub received: Option<u64>,
    pub total: Option<u64>,
    pub line: String,
}

/// What to clone and where.
///
/// `url` is expected to be already normalized by
/// [`super::clone_url::normalize_clone_url`], which is where the "remote URLs
/// only, no local paths" policy lives. Keeping that decision at the command
/// boundary lets this layer clone whatever it is handed — including the local
/// fixture repositories the integration tests need.
#[derive(Debug, Clone)]
pub struct GitCloneRequest {
    pub url: String,
    /// Full destination directory, including the repository folder itself.
    pub destination: PathBuf,
    pub branch: Option<String>,
    pub depth: Option<u32>,
    pub single_branch: bool,
    pub recurse_submodules: bool,
}

impl GitCloneRequest {
    pub fn new(url: String, destination: PathBuf) -> Self {
        Self {
            url,
            destination,
            branch: None,
            depth: None,
            single_branch: false,
            recurse_submodules: false,
        }
    }
}

/// A terminal clone failure with a stable code the UI can branch on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloneFailure {
    pub code: &'static str,
    pub message: String,
}

/// How a clone ended. Every variant is terminal, so callers can emit exactly one
/// terminal event per job without inspecting anything else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloneOutcome {
    Completed {
        destination: PathBuf,
        default_branch: Option<String>,
    },
    Failed {
        code: &'static str,
        message: String,
        cleaned_up: bool,
    },
    Cancelled {
        cleaned_up: bool,
    },
}

/// Whether RalphX may remove the destination if the clone does not finish.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DestinationOwnership {
    /// RalphX created the directory, so RalphX may remove it entirely.
    CreatedByUs,
    /// The directory already existed but was empty; empty it, never remove it.
    PreExistingEmpty,
}

impl GitService {
    /// Clone `request.url` into `request.destination`, reporting progress.
    ///
    /// Never returns `Err`: every failure is a terminal [`CloneOutcome`] so the
    /// caller has exactly one place to emit its terminal event and record state.
    pub async fn clone_repository<C, F>(
        request: GitCloneRequest,
        cancel: C,
        on_progress: F,
    ) -> CloneOutcome
    where
        C: std::future::Future<Output = ()>,
        F: FnMut(CloneProgress),
    {
        match clone_repository_inner(request, cancel, on_progress).await {
            Ok(outcome) => outcome,
            Err(failure) => CloneOutcome::Failed {
                code: failure.code,
                message: failure.message,
                cleaned_up: false,
            },
        }
    }
}

async fn clone_repository_inner<C, F>(
    request: GitCloneRequest,
    cancel: C,
    mut on_progress: F,
) -> Result<CloneOutcome, CloneFailure>
where
    C: std::future::Future<Output = ()>,
    F: FnMut(CloneProgress),
{
    let (destination, parent, ownership) = prepare_destination(&request.destination)?;

    let branch = request.branch.clone();
    let destination_arg = destination.display().to_string();
    let args = build_clone_args(&request, &request.url, &destination_arg, branch.as_deref());
    let args: Vec<&str> = args.iter().map(String::as_str).collect();

    let mut stderr_tail: Vec<String> = Vec::new();
    let outcome = git_cmd::with_git_command_lane(
        GitCommandLane::Clone,
        git_cmd::spawn_streaming(
            &args,
            &parent,
            git_runtime_config().clone_timeout_secs,
            &clone_subprocess_env(),
            cancel,
            |line| {
                if stderr_tail.len() < STDERR_TAIL_LINES {
                    stderr_tail.push(line.to_string());
                }
                if let Some(progress) = parse_clone_progress(line) {
                    on_progress(progress);
                }
            },
        ),
    )
    .await;

    let outcome = match outcome {
        Ok(outcome) => outcome,
        Err(error) => {
            let cleaned_up = cleanup(&destination, ownership).await;
            return Ok(CloneOutcome::Failed {
                code: CLONE_UNKNOWN,
                message: error.to_string(),
                cleaned_up,
            });
        }
    };

    match outcome {
        StreamedGitOutcome::Cancelled => {
            let cleaned_up = cleanup(&destination, ownership).await;
            Ok(CloneOutcome::Cancelled { cleaned_up })
        }
        StreamedGitOutcome::TimedOut => {
            let cleaned_up = cleanup(&destination, ownership).await;
            Ok(CloneOutcome::Failed {
                code: CLONE_TIMEOUT,
                message: "The clone took too long and was stopped.".to_string(),
                cleaned_up,
            })
        }
        StreamedGitOutcome::Completed(status) if status.success() => Ok(CloneOutcome::Completed {
            default_branch: GitService::detect_default_branch(&destination).await.ok(),
            destination,
        }),
        StreamedGitOutcome::Completed(_) => {
            let stderr = stderr_tail.join("\n");
            let cleaned_up = cleanup(&destination, ownership).await;
            let (code, message) = classify_clone_failure(&stderr);
            Ok(CloneOutcome::Failed {
                code,
                message,
                cleaned_up,
            })
        }
    }
}

/// Keep enough stderr for classification without letting a chatty clone grow
/// unbounded in memory.
const STDERR_TAIL_LINES: usize = 200;

pub(crate) fn build_clone_args(
    request: &GitCloneRequest,
    url: &str,
    destination: &str,
    branch: Option<&str>,
) -> Vec<String> {
    let mut args = vec!["clone".to_string(), "--progress".to_string()];
    if let Some(branch) = branch {
        args.push("--branch".to_string());
        args.push(branch.to_string());
    }
    if let Some(depth) = request.depth {
        args.push("--depth".to_string());
        args.push(depth.to_string());
    }
    if request.single_branch {
        args.push("--single-branch".to_string());
    }
    if request.recurse_submodules {
        args.push("--recurse-submodules".to_string());
    }
    // `--` keeps a URL that starts with a dash from being read as an option.
    args.push("--".to_string());
    args.push(url.to_string());
    args.push(destination.to_string());
    args
}

/// Environment additions specific to cloning.
///
/// `GIT_TERMINAL_PROMPT=0` is already applied to every Git subprocess by
/// `apply_git_subprocess_env`, so it is deliberately not repeated here. These
/// three are: without them an HTTPS clone can fall back to a GUI askpass helper
/// and an SSH clone can block forever on a passphrase or host-key prompt with no
/// terminal attached.
pub(crate) fn clone_subprocess_env() -> Vec<(String, String)> {
    vec![
        ("GIT_ASKPASS".to_string(), String::new()),
        ("SSH_ASKPASS".to_string(), String::new()),
        (
            "GIT_SSH_COMMAND".to_string(),
            "ssh -o BatchMode=yes -o StrictHostKeyChecking=accept-new".to_string(),
        ),
    ]
}

/// Validate the destination and take ownership of it before anything is spawned.
///
/// Returns the containment-checked destination, the directory to run `git` from,
/// and whether RalphX created the destination.
fn prepare_destination(
    destination: &Path,
) -> Result<(PathBuf, PathBuf, DestinationOwnership), CloneFailure> {
    let destination = validate_absolute_non_root_path(destination, "clone destination")
        .map_err(|error| destination_error(CLONE_DEST_INVALID, &error))?;

    let Some(parent) = destination.parent() else {
        return Err(CloneFailure {
            code: CLONE_DEST_INVALID,
            message: "Pick a folder inside another folder".to_string(),
        });
    };
    let Some(Component::Normal(folder)) = destination.components().next_back() else {
        return Err(CloneFailure {
            code: CLONE_DEST_INVALID,
            message: "That destination folder name isn't usable".to_string(),
        });
    };

    let canonical_parent = parent.canonicalize().map_err(|error| CloneFailure {
        code: CLONE_DEST_INVALID,
        message: format!("The folder {} isn't available: {error}", parent.display()),
    })?;
    let canonical_parent = validate_absolute_non_root_path(&canonical_parent, "clone parent")
        .map_err(|error| destination_error(CLONE_DEST_INVALID, &error))?;
    if !canonical_parent.is_dir() {
        return Err(CloneFailure {
            code: CLONE_DEST_INVALID,
            message: format!("{} is not a folder", canonical_parent.display()),
        });
    }

    // Re-derive the destination under the *canonical* parent so a symlinked
    // parent cannot smuggle the clone outside the folder the user chose.
    let resolved = canonical_parent.join(folder);
    let resolved = validate_absolute_non_root_path(&resolved, "clone destination")
        .map_err(|error| destination_error(CLONE_DEST_INVALID, &error))?;
    require_under_root(&resolved, &canonical_parent, "clone destination")
        .map_err(|error| destination_error(CLONE_DEST_INVALID, &error))?;

    let ownership = claim_destination(&resolved)?;
    Ok((resolved, canonical_parent, ownership))
}

fn claim_destination(destination: &Path) -> Result<DestinationOwnership, CloneFailure> {
    if !destination.exists() {
        // codeql[rust/path-injection]
        std::fs::create_dir(destination).map_err(|error| CloneFailure {
            code: CLONE_DEST_INVALID,
            message: format!("Couldn't create {}: {error}", destination.display()),
        })?;
        return Ok(DestinationOwnership::CreatedByUs);
    }

    if !destination.is_dir() {
        return Err(CloneFailure {
            code: CLONE_DEST_NOT_EMPTY,
            message: format!("A file already exists at {}", destination.display()),
        });
    }

    let mut entries = std::fs::read_dir(destination).map_err(|error| CloneFailure {
        code: CLONE_DEST_INVALID,
        message: format!("Couldn't read {}: {error}", destination.display()),
    })?;
    if entries.next().is_some() {
        return Err(CloneFailure {
            code: CLONE_DEST_NOT_EMPTY,
            message: format!(
                "{} already has contents. Pick an empty folder.",
                destination.display()
            ),
        });
    }

    Ok(DestinationOwnership::PreExistingEmpty)
}

/// Undo a partial clone. Returns whether the destination was actually cleaned.
async fn cleanup(destination: &Path, ownership: DestinationOwnership) -> bool {
    if let Err(error) = checked_remove_dir_all(destination, "clone destination").await {
        tracing::warn!(
            destination = %destination.display(),
            error = %error,
            "Failed to clean up an unfinished clone"
        );
        return false;
    }

    if ownership == DestinationOwnership::PreExistingEmpty {
        // The user's own (empty) folder must survive; only its contents go.
        if let Err(error) = std::fs::create_dir(destination) {
            tracing::warn!(
                destination = %destination.display(),
                error = %error,
                "Failed to restore a pre-existing clone destination"
            );
            return false;
        }
    }

    true
}

/// Classify clone stderr.
///
/// Auth is matched ahead of the broad not-found and network patterns, mirroring
/// `classify_git_failure_text`, because GitHub reports a private repository the
/// caller cannot see as "Repository not found" — that is a credential problem,
/// not a missing repository.
///
/// The one signal checked *before* auth is "does not appear to be a git
/// repository", which is definitive: git also emits a generic "could not read
/// from remote repository" alongside it, and that phrase is an auth marker.
pub(crate) fn classify_clone_failure(stderr: &str) -> (&'static str, String) {
    let normalized = stderr.to_lowercase();

    if normalized.contains("does not appear to be a git repository") {
        return (
            CLONE_NOT_FOUND,
            "That address isn't a Git repository.".to_string(),
        );
    }
    if is_git_auth_failure_text(&normalized) {
        return (
            CLONE_AUTH_FAILED,
            "RalphX couldn't sign in to that repository.".to_string(),
        );
    }
    if normalized.contains("repository not found")
        || normalized.contains("does not exist")
        || normalized.contains("not found")
        || normalized.contains("remote branch")
    {
        return (
            CLONE_NOT_FOUND,
            "That repository or branch couldn't be found.".to_string(),
        );
    }
    if normalized.contains("could not resolve host")
        || normalized.contains("connection timed out")
        || normalized.contains("connection refused")
        || normalized.contains("network is unreachable")
        || normalized.contains("early eof")
        || normalized.contains("rpc failed")
        || normalized.contains("operation timed out")
    {
        return (
            CLONE_NETWORK,
            "RalphX couldn't reach that repository.".to_string(),
        );
    }

    (
        CLONE_UNKNOWN,
        "The clone didn't finish. Check the details below and try again.".to_string(),
    )
}

fn destination_error(code: &'static str, error: &AppError) -> CloneFailure {
    CloneFailure {
        code,
        message: error.to_string(),
    }
}

/// Whether a directory has no entries, for callers validating a target before a
/// clone is ever started.
///
/// # Errors
/// Returns `Err` when the path is unsafe or cannot be read.
pub(crate) async fn is_empty_directory(path: &Path) -> AppResult<bool> {
    let mut entries = checked_read_dir(path, "clone destination").await?;
    Ok(entries
        .next_entry()
        .await
        .map_err(|error| {
            AppError::Infrastructure(format!("Failed to read {}: {error}", path.display()))
        })?
        .is_none())
}
