//! Authoritative strict-ticket validation at Agent workspace publish boundaries.

use std::path::Path;

use serde::Serialize;

use super::git_service::GitService;
use super::publish_resilience::PublishFailureClass;
use super::AppState;
use crate::domain::entities::{
    AgentConversationWorkspace, TicketCanonicalBranchCycleState, TicketCanonicalBranchPolicyKind,
};

#[path = "ticket_git_publish_hook.rs"]
mod hook;
pub use hook::{install_resolved_ticket_git_commit_hook, install_ticket_git_commit_hook};
#[path = "ticket_git_publish_subject.rs"]
mod subject;
use subject::validate_frozen_commit_rule;
pub use subject::{frozen_commit_subject_matches, render_frozen_commit_subject};

const POLICY_ERROR_MARKER: &str = "[ralphx:ticket_git_publish]";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TicketGitPublishFailureKind {
    BranchMismatch,
    InvalidCycleState,
    CycleBaseUnavailable,
    InvalidCommitSubjects,
    PublishedCommitSubjects,
    InvalidFrozenPolicy,
    HookEnvironment,
    RepositoryFailure,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketGitOffendingCommit {
    pub sha: String,
    pub short_sha: String,
    pub subject: String,
    pub published: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketGitPublishFailure {
    pub kind: TicketGitPublishFailureKind,
    pub message: String,
    pub expected_branch: Option<String>,
    pub actual_branch: Option<String>,
    pub expected_commit_subject: Option<String>,
    pub offending_commits: Box<[TicketGitOffendingCommit]>,
}

impl TicketGitPublishFailure {
    pub fn class(&self) -> PublishFailureClass {
        match self.kind {
            TicketGitPublishFailureKind::InvalidCommitSubjects => PublishFailureClass::AgentFixable,
            TicketGitPublishFailureKind::BranchMismatch
            | TicketGitPublishFailureKind::InvalidCycleState
            | TicketGitPublishFailureKind::CycleBaseUnavailable
            | TicketGitPublishFailureKind::PublishedCommitSubjects
            | TicketGitPublishFailureKind::InvalidFrozenPolicy
            | TicketGitPublishFailureKind::HookEnvironment
            | TicketGitPublishFailureKind::RepositoryFailure => PublishFailureClass::Operational,
        }
    }

    fn new(kind: TicketGitPublishFailureKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            expected_branch: None,
            actual_branch: None,
            expected_commit_subject: None,
            offending_commits: Box::new([]),
        }
    }

    fn for_branch(mut self, expected: &str, actual: Option<&str>) -> Self {
        self.expected_branch = Some(expected.to_string());
        self.actual_branch = actual.map(str::to_string);
        self
    }

    fn for_commit_rule(mut self, rule: &str) -> Self {
        self.expected_commit_subject = Some(rule.to_string());
        self
    }

    fn with_offending_commits(mut self, commits: Vec<TicketGitOffendingCommit>) -> Self {
        self.offending_commits = commits.into_boxed_slice();
        self
    }
}

impl std::fmt::Display for TicketGitPublishFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let details = serde_json::to_string(self).unwrap_or_else(|_| self.message.clone());
        write!(formatter, "{POLICY_ERROR_MARKER} {details}")
    }
}

impl std::error::Error for TicketGitPublishFailure {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTicketGitPublishPolicy {
    pub expected_branch: String,
    pub cycle_base: String,
    pub commit_subject_rule: String,
    pub automatic_commit_subject: String,
    pub frozen_pr_title: String,
    pub policy_version: i64,
    pub cycle_generation: i64,
    pub validated_commit_count: usize,
}

/// Load strict policy by exact workspace branch and validate its current cycle range.
/// A legacy or unbound workspace returns `None` without changing existing publish behavior.
pub async fn load_ticket_git_publish_policy(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    worktree_path: &Path,
    automatic_summary: &str,
) -> Result<Option<ResolvedTicketGitPublishPolicy>, TicketGitPublishFailure> {
    let binding = state
        .ticket_canonical_branch_repo
        .get_by_branch_name(&workspace.project_id, &workspace.branch_name)
        .await
        .map_err(|error| repository_failure("load strict ticket binding", error))?;
    let Some(binding) = binding else {
        return Ok(None);
    };
    if binding.policy_kind != TicketCanonicalBranchPolicyKind::StrictGitConvention {
        return Ok(None);
    }
    binding.validate_policy().map_err(|error| {
        TicketGitPublishFailure::new(
            TicketGitPublishFailureKind::InvalidFrozenPolicy,
            format!("Strict ticket binding is invalid: {error}"),
        )
        .for_branch(&workspace.branch_name, None)
    })?;
    if binding.branch_name != workspace.branch_name {
        return Err(TicketGitPublishFailure::new(
            TicketGitPublishFailureKind::BranchMismatch,
            "Workspace branch does not match its frozen strict ticket binding",
        )
        .for_branch(&binding.branch_name, Some(&workspace.branch_name)));
    }
    if binding.cycle.state != TicketCanonicalBranchCycleState::Active {
        return Err(TicketGitPublishFailure::new(
            TicketGitPublishFailureKind::InvalidCycleState,
            format!(
                "Strict ticket branch '{}' cannot publish while cycle {} is {}",
                binding.branch_name, binding.cycle.generation, binding.cycle.state
            ),
        )
        .for_branch(&binding.branch_name, None));
    }
    let cycle_base = binding
        .cycle
        .effective_merge_base
        .as_deref()
        .or(binding.cycle.base_commit.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            TicketGitPublishFailure::new(
                TicketGitPublishFailureKind::CycleBaseUnavailable,
                "Strict ticket cycle has no recorded publish base",
            )
            .for_branch(&binding.branch_name, None)
        })?;
    let frozen = binding.strict_policy.as_ref().ok_or_else(|| {
        TicketGitPublishFailure::new(
            TicketGitPublishFailureKind::InvalidFrozenPolicy,
            "Strict ticket binding has no frozen Git convention",
        )
    })?;
    let automatic_commit_subject =
        render_frozen_commit_subject(&frozen.commit_subject_rule, automatic_summary)?;

    GitService::fetch_origin(worktree_path)
        .await
        .map_err(|error| repository_failure("refresh strict ticket remote history", error))?;

    validate_branch_and_commits(
        worktree_path,
        &binding.branch_name,
        cycle_base,
        &frozen.commit_subject_rule,
    )
    .await
    .map(|validated_commit_count| {
        Some(ResolvedTicketGitPublishPolicy {
            expected_branch: binding.branch_name,
            cycle_base: cycle_base.to_string(),
            commit_subject_rule: frozen.commit_subject_rule.clone(),
            automatic_commit_subject,
            frozen_pr_title: frozen.pr_title.clone(),
            policy_version: frozen.policy_version,
            cycle_generation: binding.cycle.generation,
            validated_commit_count,
        })
    })
}

/// Persist the effective base selected by freshness handling before push.
///
/// The CAS revalidates the exact active cycle so a stale publish attempt cannot
/// authorize a later generation or changed frozen policy.
pub async fn refresh_ticket_git_publish_cycle_base(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    worktree_path: &Path,
    policy: &mut ResolvedTicketGitPublishPolicy,
    refreshed_base: &str,
) -> Result<(), TicketGitPublishFailure> {
    let refreshed_base = refreshed_base.trim();
    if refreshed_base.is_empty() {
        return Err(TicketGitPublishFailure::new(
            TicketGitPublishFailureKind::CycleBaseUnavailable,
            "Publish freshness returned an empty strict ticket cycle base",
        ));
    }
    let refreshed_base_is_reachable =
        GitService::is_commit_on_branch(worktree_path, refreshed_base, "HEAD")
            .await
            .map_err(|error| repository_failure("verify refreshed strict ticket base", error))?;
    if !refreshed_base_is_reachable {
        return Err(TicketGitPublishFailure::new(
            TicketGitPublishFailureKind::CycleBaseUnavailable,
            format!("Refreshed strict ticket base '{refreshed_base}' is not reachable from HEAD"),
        )
        .for_branch(&policy.expected_branch, Some(&workspace.branch_name)));
    }
    let binding = state
        .ticket_canonical_branch_repo
        .get_by_branch_name(&workspace.project_id, &workspace.branch_name)
        .await
        .map_err(|error| repository_failure("reload strict ticket cycle", error))?
        .ok_or_else(|| {
            TicketGitPublishFailure::new(
                TicketGitPublishFailureKind::InvalidCycleState,
                "Strict ticket binding disappeared during publish freshness",
            )
        })?;
    let frozen = binding.strict_policy.as_ref().ok_or_else(|| {
        TicketGitPublishFailure::new(
            TicketGitPublishFailureKind::InvalidFrozenPolicy,
            "Strict ticket binding lost its frozen policy during publish freshness",
        )
    })?;
    if binding.policy_kind != TicketCanonicalBranchPolicyKind::StrictGitConvention
        || binding.branch_name != policy.expected_branch
        || binding.cycle.generation != policy.cycle_generation
        || binding.cycle.state != TicketCanonicalBranchCycleState::Active
        || frozen.policy_version != policy.policy_version
        || frozen.commit_subject_rule != policy.commit_subject_rule
        || frozen.pr_title != policy.frozen_pr_title
    {
        return Err(TicketGitPublishFailure::new(
            TicketGitPublishFailureKind::InvalidCycleState,
            "Strict ticket binding changed while publish freshness was running",
        )
        .for_branch(&policy.expected_branch, Some(&workspace.branch_name)));
    }
    if binding.cycle.effective_merge_base.as_deref() != Some(refreshed_base) {
        let mut replacement = binding.cycle.clone();
        replacement.effective_merge_base = Some(refreshed_base.to_string());
        let swapped = state
            .ticket_canonical_branch_repo
            .compare_and_swap_cycle(
                &binding.project_id,
                &binding.provider,
                &binding.issue_key,
                binding.cycle.generation,
                TicketCanonicalBranchCycleState::Active,
                replacement,
            )
            .await
            .map_err(|error| repository_failure("persist strict ticket publish base", error))?;
        if !swapped {
            return Err(TicketGitPublishFailure::new(
                TicketGitPublishFailureKind::InvalidCycleState,
                "Strict ticket cycle changed before its refreshed publish base could persist",
            )
            .for_branch(&policy.expected_branch, Some(&workspace.branch_name)));
        }
    }
    policy.cycle_base = refreshed_base.to_string();
    Ok(())
}

pub async fn validate_resolved_ticket_git_publish_policy(
    worktree_path: &Path,
    policy: &ResolvedTicketGitPublishPolicy,
) -> Result<usize, TicketGitPublishFailure> {
    validate_branch_and_commits(
        worktree_path,
        &policy.expected_branch,
        &policy.cycle_base,
        &policy.commit_subject_rule,
    )
    .await
}

async fn validate_branch_and_commits(
    worktree_path: &Path,
    expected_branch: &str,
    cycle_base: &str,
    commit_subject_rule: &str,
) -> Result<usize, TicketGitPublishFailure> {
    validate_frozen_commit_rule(commit_subject_rule)?;
    let actual_branch = GitService::get_current_branch(worktree_path)
        .await
        .map_err(|error| repository_failure("read checked-out branch", error))?;
    if actual_branch != expected_branch {
        return Err(TicketGitPublishFailure::new(
            TicketGitPublishFailureKind::BranchMismatch,
            format!(
                "Strict ticket publish expected branch '{expected_branch}' but found '{actual_branch}'"
            ),
        )
        .for_branch(expected_branch, Some(&actual_branch)));
    }
    let base_is_reachable = GitService::is_commit_on_branch(worktree_path, cycle_base, "HEAD")
        .await
        .map_err(|error| repository_failure("verify strict ticket cycle base", error))?;
    if !base_is_reachable {
        return Err(TicketGitPublishFailure::new(
            TicketGitPublishFailureKind::CycleBaseUnavailable,
            format!(
                "Strict ticket cycle base '{cycle_base}' is not reachable from branch '{expected_branch}'"
            ),
        )
        .for_branch(expected_branch, Some(&actual_branch)));
    }
    let commits = GitService::get_commits_between(worktree_path, cycle_base, "HEAD")
        .await
        .map_err(|error| repository_failure("read strict ticket commit range", error))?;
    let remote_ref = format!("origin/{expected_branch}");
    let remote_exists = GitService::ref_exists(worktree_path, &remote_ref)
        .await
        .map_err(|error| repository_failure("inspect strict ticket remote branch", error))?;
    let mut offending = Vec::new();
    for commit in &commits {
        if frozen_commit_subject_matches(commit_subject_rule, &commit.message)? {
            continue;
        }
        let published = if remote_exists {
            GitService::is_commit_on_branch(worktree_path, &commit.sha, &remote_ref)
                .await
                .map_err(|error| repository_failure("inspect published ticket history", error))?
        } else {
            false
        };
        offending.push(TicketGitOffendingCommit {
            sha: commit.sha.clone(),
            short_sha: commit.short_sha.clone(),
            subject: commit.message.clone(),
            published,
        });
    }
    if offending.is_empty() {
        return Ok(commits.len());
    }
    let published = offending.iter().any(|commit| commit.published);
    let kind = if published {
        TicketGitPublishFailureKind::PublishedCommitSubjects
    } else {
        TicketGitPublishFailureKind::InvalidCommitSubjects
    };
    let message = if published {
        "Strict ticket history contains a nonconforming commit already published remotely; RalphX will not rewrite or force-push it"
    } else {
        "Strict ticket history contains local commits that do not match the frozen commit subject rule"
    };
    Err(TicketGitPublishFailure::new(kind, message)
        .for_branch(expected_branch, Some(&actual_branch))
        .for_commit_rule(commit_subject_rule)
        .with_offending_commits(offending))
}

fn repository_failure(operation: &str, error: impl std::fmt::Display) -> TicketGitPublishFailure {
    TicketGitPublishFailure::new(
        TicketGitPublishFailureKind::RepositoryFailure,
        format!("Failed to {operation}: {error}"),
    )
}

fn hook_failure(operation: &str, error: impl std::fmt::Display) -> TicketGitPublishFailure {
    TicketGitPublishFailure::new(
        TicketGitPublishFailureKind::HookEnvironment,
        format!("Failed to {operation}: {error}"),
    )
}
