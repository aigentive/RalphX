//! Git Service - Branch, worktree, and merge operations for task isolation
//!
//! Provides git operations for per-task branch isolation:
//! - Branch creation, checkout, and deletion (both modes)
//! - Worktree management (Worktree mode only)
//! - Commit operations with configurable messages
//! - Rebase and merge operations for the two-phase merge workflow
//! - Checkout-free merge operations (git plumbing, no working tree mutation)
//! - Query operations for commits and diff stats

mod branch;
mod bootstrap;
pub mod checkout_free;
pub mod clone;
mod clone_progress;
pub(crate) mod clone_url;
mod commit;
pub(crate) mod git_cmd;
mod identity;
mod merge;
mod query;
mod rebase;
mod repository_capability;
mod state_query;
mod worktree;

pub(crate) use rebase::FetchOriginOutcome;
pub use bootstrap::{GitBootstrapRequest, ProjectGitBootstrap};
/// Read-only repository shape helpers shared by bootstrap and the
/// project-candidate probe.
pub(crate) use bootstrap::{repository_top_level, symbolic_branch_opt};

#[cfg(test)]
mod tests;

use crate::error::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use tracing::{debug, warn};

/// Result of a merge operation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MergeResult {
    /// Merge succeeded with a new merge commit
    Success { commit_sha: String },
    /// Merge resulted in conflicts
    Conflict { files: Vec<PathBuf> },
    /// Fast-forward merge (no merge commit needed)
    FastForward { commit_sha: String },
}

/// Result of a rebase operation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RebaseResult {
    /// Rebase completed successfully
    Success,
    /// Rebase resulted in conflicts
    Conflict { files: Vec<PathBuf> },
}

/// Result of the combined rebase + merge attempt (Phase 1 of merge workflow)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MergeAttemptResult {
    /// Rebase + merge succeeded (fast path)
    Success { commit_sha: String },
    /// Conflict detected, needs agent resolution (Phase 2)
    NeedsAgent { conflict_files: Vec<PathBuf> },
    /// Source or target branch does not exist
    BranchNotFound { branch: String },
}

/// Result of attempting to complete a stale rebase
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StaleRebaseResult {
    /// No rebase in progress
    NoRebase,
    /// Rebase was completed successfully
    Completed,
    /// Rebase has real conflicts that need resolution
    HasConflicts { files: Vec<PathBuf> },
    /// Rebase completion failed with an error
    Failed { reason: String },
}

/// Information about a single commit
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitInfo {
    /// Commit SHA (full 40 characters)
    pub sha: String,
    /// Short SHA (7 characters)
    pub short_sha: String,
    /// Commit message (first line)
    pub message: String,
    /// Author name
    pub author: String,
    /// Commit timestamp (RFC3339)
    pub timestamp: String,
}

/// Statistics about changes between branches
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffStats {
    /// Number of files changed
    pub files_changed: u32,
    /// Number of lines added
    pub insertions: u32,
    /// Number of lines deleted
    pub deletions: u32,
    /// List of changed file paths
    pub changed_files: Vec<String>,
}

/// Information about a git worktree, parsed from `git worktree list --porcelain`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeInfo {
    /// Absolute path to the worktree directory
    pub path: String,
    /// Branch checked out in this worktree (None for detached HEAD or bare repos)
    pub branch: Option<String>,
    /// HEAD commit SHA (None for bare repos)
    pub head: Option<String>,
}

/// Git Service for branch, worktree, and merge operations
pub struct GitService;
