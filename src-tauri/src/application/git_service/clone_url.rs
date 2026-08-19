//! Clone URL normalization.
//!
//! Accepts the shapes a person actually pastes — HTTPS, SSH, scp-like,
//! `owner/repo` shorthand, and a GitHub branch page URL — and rejects everything
//! else, including local paths and `file://`. Local clones are out of scope
//! because "add this folder" is already its own intent in the wizard.

use crate::infrastructure::git_auth::{classify_git_remote_url, GitRemoteUrlKind};

use super::clone::{CloneFailure, CLONE_URL_INVALID};

const GITHUB_HTTPS_PREFIX: &str = "https://github.com/";

/// A clone URL Git can be handed, plus what it implies for the destination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NormalizedCloneUrl {
    /// The URL passed to `git clone`.
    pub url: String,
    /// Folder name derived from the repository name.
    pub folder_name: String,
    /// Branch implied by the input (only a GitHub `/tree/<branch>` page sets this).
    pub branch: Option<String>,
}

/// Normalize a user-entered clone target.
///
/// # Errors
/// Returns `CLONE_URL_INVALID` for local paths, `file://`, unrecognized shapes,
/// and anything whose repository name would not be a safe folder component.
pub(crate) fn normalize_clone_url(raw: &str) -> Result<NormalizedCloneUrl, CloneFailure> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(invalid("Enter a repository URL"));
    }

    match classify_git_remote_url(trimmed) {
        GitRemoteUrlKind::File => Err(invalid(
            "Local folders can't be cloned. Use Add Existing Repository instead.",
        )),
        GitRemoteUrlKind::Https => normalize_https(trimmed),
        GitRemoteUrlKind::Ssh => normalize_ssh(trimmed),
        GitRemoteUrlKind::Other => normalize_shorthand(trimmed),
    }
}

fn normalize_https(url: &str) -> Result<NormalizedCloneUrl, CloneFailure> {
    if url.contains('?') || url.contains('#') {
        return Err(invalid("That doesn't look like a repository address"));
    }

    // A bare host is not a repository: without this, `https://host/` would
    // happily derive the folder name "host".
    let path = url
        .trim_end_matches('/')
        .strip_prefix("https://")
        .or_else(|| url.trim_end_matches('/').strip_prefix("http://"))
        .unwrap_or_default();
    if !path.contains('/') {
        return Err(invalid("Include the repository path, not just the host"));
    }

    // A pasted GitHub branch page carries the branch the user is looking at, so
    // honor it instead of silently cloning the default branch.
    if let Some(path) = url.trim_end_matches('/').strip_prefix(GITHUB_HTTPS_PREFIX) {
        let mut parts = path.splitn(4, '/');
        let (Some(owner), Some(repo), Some("tree"), Some(branch)) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return finish(url.to_string(), url, None);
        };
        let repo = repo.strip_suffix(".git").unwrap_or(repo);
        if !is_safe_component(owner) || !is_safe_component(repo) || branch.is_empty() {
            return Err(invalid("That doesn't look like a repository address"));
        }
        return Ok(NormalizedCloneUrl {
            url: format!("{GITHUB_HTTPS_PREFIX}{owner}/{repo}.git"),
            folder_name: repo.to_string(),
            branch: Some(branch.to_string()),
        });
    }

    finish(url.to_string(), url, None)
}

fn normalize_ssh(url: &str) -> Result<NormalizedCloneUrl, CloneFailure> {
    // scp-like `git@host:owner/repo.git` has no `/` before the colon, so derive
    // the repository name from whichever separator comes last.
    let tail = url
        .trim_end_matches('/')
        .rsplit(['/', ':'])
        .next()
        .unwrap_or_default();
    finish(url.to_string(), tail, None)
}

fn normalize_shorthand(input: &str) -> Result<NormalizedCloneUrl, CloneFailure> {
    let trimmed = input.trim_end_matches('/');
    let mut parts = trimmed.split('/');
    let (Some(owner), Some(repo), None) = (parts.next(), parts.next(), parts.next()) else {
        return Err(invalid(
            "Enter a repository URL, or an owner/repository name",
        ));
    };
    let repo = repo.strip_suffix(".git").unwrap_or(repo);
    if !is_safe_component(owner) || !is_safe_component(repo) {
        return Err(invalid(
            "Enter a repository URL, or an owner/repository name",
        ));
    }

    Ok(NormalizedCloneUrl {
        url: format!("{GITHUB_HTTPS_PREFIX}{owner}/{repo}.git"),
        folder_name: repo.to_string(),
        branch: None,
    })
}

/// Derive the folder name from a URL's last path segment and validate it.
fn finish(
    url: String,
    source_for_name: &str,
    branch: Option<String>,
) -> Result<NormalizedCloneUrl, CloneFailure> {
    let tail = source_for_name
        .trim_end_matches('/')
        .rsplit(['/', ':'])
        .next()
        .unwrap_or_default();
    let folder_name = tail.strip_suffix(".git").unwrap_or(tail);
    if !is_safe_component(folder_name) {
        return Err(invalid("That doesn't look like a repository address"));
    }

    Ok(NormalizedCloneUrl {
        url,
        folder_name: folder_name.to_string(),
        branch,
    })
}

/// A component safe to use both in a GitHub path and as a directory name.
fn is_safe_component(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && !value.starts_with('-')
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

fn invalid(message: &str) -> CloneFailure {
    CloneFailure {
        code: CLONE_URL_INVALID,
        message: message.to_string(),
    }
}
