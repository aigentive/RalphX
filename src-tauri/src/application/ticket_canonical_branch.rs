//! Ticket canonical-branch resolution.
//!
//! When a user starts a RalphX agent conversation from a ticket, all work for
//! that ticket must converge on a single "canonical branch" so that each
//! conversation's publication PR targets it (commit-back happens via the
//! workspace base ref). The canonical branch is created once in the project's
//! main checkout (never worktree-checked-out) and pushed to origin; every
//! subsequent conversation reuses it and forges its own per-conversation branch
//! off it.

use std::path::Path;

use chrono::Utc;

use crate::application::git_service::GitService;
use crate::application::AppState;
use crate::domain::entities::{ProjectId, TicketCanonicalBranch};
use crate::error::{AppError, AppResult};

/// Prefix for all ticket canonical branches.
const CANONICAL_BRANCH_PREFIX: &str = "ralphx/ticket";

/// Build a readable, git-safe slug from a raw provider issue key.
///
/// Lowercases, maps every run of non-`[a-z0-9]` characters to a single `-`,
/// and trims leading/trailing `-`. The result is purely `[a-z0-9-]`, so it can
/// never introduce a path separator or `..` traversal component. Returns `None`
/// when nothing usable remains (empty input or all-separator input).
pub(crate) fn sanitize_issue_key_slug(issue_key: &str) -> Option<String> {
    let mut slug = String::with_capacity(issue_key.len());
    let mut last_was_dash = false;
    for ch in issue_key.chars() {
        let lower = ch.to_ascii_lowercase();
        if lower.is_ascii_lowercase() || lower.is_ascii_digit() {
            slug.push(lower);
            last_was_dash = false;
        } else if !last_was_dash {
            slug.push('-');
            last_was_dash = true;
        }
    }
    let trimmed = slug.trim_matches('-');
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Compute the canonical branch name for a ticket:
/// `ralphx/ticket/<provider>-<sanitized-issue-key>`.
///
/// The provider is sanitized with the same slug rules as the issue key so the
/// whole component stays `[a-z0-9-]`.
pub(crate) fn canonical_branch_name(provider: &str, issue_key: &str) -> Option<String> {
    let provider_slug = sanitize_issue_key_slug(provider)?;
    let issue_slug = sanitize_issue_key_slug(issue_key)?;
    Some(format!(
        "{CANONICAL_BRANCH_PREFIX}/{provider_slug}-{issue_slug}"
    ))
}

/// Resolve (and lazily establish) the single canonical branch for a ticket.
///
/// - If a row exists and is `terminal`, returns a typed `ExecutionBlocked` error
///   — a merged canonical branch must never be resurrected.
/// - If a row exists and was pushed to origin, returns it (idempotent reuse by
///   concurrent conversations).
/// - If a row exists but was never confirmed pushed, re-attempts the push and
///   marks it pushed before returning (recovers a partially-established branch).
/// - If no row exists, this is the first conversation: it forges the branch at
///   the project default tip in the main checkout, pushes it to origin, and
///   persists the row (only marking `origin_pushed` after a confirmed push).
///
/// On any failure (project not found, invalid ref, branch create, push) this
/// returns a typed error so the caller can surface it; it never silently falls
/// back to the project default branch.
pub async fn ensure_ticket_canonical_branch(
    state: &AppState,
    project_id: &ProjectId,
    provider: &str,
    issue_key: &str,
) -> AppResult<TicketCanonicalBranch> {
    if let Some(existing) = state
        .ticket_canonical_branch_repo
        .get(project_id, provider, issue_key)
        .await?
    {
        if existing.terminal {
            return Err(AppError::ExecutionBlocked(format!(
                "Ticket {issue_key} canonical branch '{}' is terminal (already merged); cannot start new work on it",
                existing.branch_name
            )));
        }
        if existing.origin_pushed {
            return Ok(existing);
        }
        // Row exists but the push was never confirmed (e.g. a prior attempt that
        // created the row then failed at push). Complete it idempotently.
        return complete_unpushed_canonical_branch(state, project_id, provider, issue_key, existing)
            .await;
    }

    establish_canonical_branch(state, project_id, provider, issue_key).await
}

/// Forge a brand-new canonical branch for the first conversation on a ticket.
async fn establish_canonical_branch(
    state: &AppState,
    project_id: &ProjectId,
    provider: &str,
    issue_key: &str,
) -> AppResult<TicketCanonicalBranch> {
    let project = state
        .project_repo
        .get_by_id(project_id)
        .await?
        .ok_or_else(|| AppError::ProjectNotFound(project_id.to_string()))?;

    let repo = Path::new(&project.working_directory);

    let branch_name = canonical_branch_name(provider, issue_key).ok_or_else(|| {
        AppError::Validation(format!(
            "Ticket issue key '{issue_key}' (provider '{provider}') does not yield a valid branch name"
        ))
    })?;

    // CodeQL path-safety: validate the derived ref with git before using it as a
    // git argument. Reject anything git would not accept as a branch ref.
    if !GitService::check_ref_format(repo, &branch_name).await? {
        return Err(AppError::Validation(format!(
            "Computed canonical branch name '{branch_name}' is not a valid git ref"
        )));
    }

    let base_branch =
        GitService::resolve_project_default_branch(repo, project.base_branch.as_deref()).await;

    // Create the canonical branch at the project-default tip in the MAIN checkout
    // without checking it out (a worktree must never hold this branch).
    if !GitService::branch_exists(repo, &branch_name).await? {
        GitService::create_branch(repo, &branch_name, &base_branch).await?;
    }

    let base_commit = GitService::get_branch_sha(repo, &branch_name).await.ok();

    let mut branch = TicketCanonicalBranch::new(
        project_id.clone(),
        provider,
        issue_key,
        branch_name.clone(),
        base_branch,
        base_commit,
        Utc::now(),
    );

    // Push to origin; only flag origin_pushed after a confirmed successful push.
    let pushed = push_canonical_branch(state, repo, &branch_name).await?;
    branch.origin_pushed = pushed;

    state.ticket_canonical_branch_repo.upsert(branch).await
}

/// Re-attempt the push for a canonical branch whose row exists but was never
/// confirmed pushed, then mark it pushed.
async fn complete_unpushed_canonical_branch(
    state: &AppState,
    project_id: &ProjectId,
    provider: &str,
    issue_key: &str,
    mut existing: TicketCanonicalBranch,
) -> AppResult<TicketCanonicalBranch> {
    let project = state
        .project_repo
        .get_by_id(project_id)
        .await?
        .ok_or_else(|| AppError::ProjectNotFound(project_id.to_string()))?;

    let repo = Path::new(&project.working_directory);

    // The branch name was already decided and persisted; re-validate defensively
    // before reusing it as a git argument.
    if !GitService::check_ref_format(repo, &existing.branch_name).await? {
        return Err(AppError::Validation(format!(
            "Persisted canonical branch name '{}' is not a valid git ref",
            existing.branch_name
        )));
    }

    // Ensure the local branch still exists before pushing (it lives only in the
    // main checkout). Recreate from its recorded base branch if it went missing.
    if !GitService::branch_exists(repo, &existing.branch_name).await? {
        GitService::create_branch(repo, &existing.branch_name, &existing.base_branch).await?;
    }

    let pushed = push_canonical_branch(state, repo, &existing.branch_name).await?;
    if pushed {
        state
            .ticket_canonical_branch_repo
            .mark_origin_pushed(project_id, provider, issue_key)
            .await?;
        existing.origin_pushed = true;
    }

    Ok(existing)
}

/// Push the canonical branch to origin through the GitHub service.
///
/// Returns `Ok(true)` only on a confirmed successful push. A missing GitHub
/// service is a hard error: the canonical-branch contract requires the branch
/// to exist on origin so each conversation's publication PR can target it.
async fn push_canonical_branch(
    state: &AppState,
    repo: &Path,
    branch_name: &str,
) -> AppResult<bool> {
    let github = state.github_service.as_ref().ok_or_else(|| {
        AppError::GitOperation(
            "GitHub integration is not available; cannot push ticket canonical branch".to_string(),
        )
    })?;

    github.push_branch(repo, branch_name).await?;
    Ok(true)
}

#[cfg(test)]
#[path = "ticket_canonical_branch_tests.rs"]
mod tests;
