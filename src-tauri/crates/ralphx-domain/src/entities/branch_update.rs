use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use thiserror::Error;
use uuid::Uuid;

use super::TaskId;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BranchUpdateOperationId(String);

impl BranchUpdateOperationId {
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    pub fn from_string(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for BranchUpdateOperationId {
    fn default() -> Self {
        Self::new()
    }
}

macro_rules! string_enum {
    ($name:ident { $($variant:ident => $wire:literal),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name { $($variant),+ }

        impl $name {
            pub fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $wire),+ }
            }
        }

        impl FromStr for $name {
            type Err = StringEnumParseError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                match value {
                    $($wire => Ok(Self::$variant),)+
                    _ => Err(StringEnumParseError {
                        type_name: stringify!($name),
                        value: value.to_string(),
                    }),
                }
            }
        }
    };
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("unknown {type_name} value: '{value}'")]
pub struct StringEnumParseError {
    type_name: &'static str,
    value: String,
}

string_enum!(BranchUpdateDirection {
    PlanBranch => "plan_branch",
    TaskBranch => "task_branch",
});

string_enum!(BranchUpdatePhase {
    Programmatic => "programmatic",
    Resolving => "resolving",
    Blocked => "blocked",
    ContinuationPending => "continuation_pending",
    ContinuationInProgress => "continuation_in_progress",
    Settled => "settled",
});

string_enum!(BranchUpdateContinuation {
    ResumeExecution => "resume_execution",
    ResumeReExecution => "resume_re_execution",
    ResumeReview => "resume_review",
    RetryPendingMerge => "retry_pending_merge",
    ResumeWaitingOnPr => "resume_waiting_on_pr",
    FinalizePostMergePrPublication => "finalize_post_merge_pr_publication",
});

string_enum!(BranchUpdateCapacityOwnership {
    Inherited => "inherited",
    Acquired => "acquired",
    Released => "released",
});

string_enum!(BranchUpdateWorkspaceOwnership {
    OperationWorktree => "operation_worktree",
    BorrowedTaskWorktree => "borrowed_task_worktree",
    BorrowedLocalCheckout => "borrowed_local_checkout",
});

string_enum!(GitTargetLeaseOwnerKind {
    BranchUpdateOperation => "branch_update_operation",
    MergeAttempt => "merge_attempt",
    PublicationRecovery => "publication_recovery",
    AgentWorkspaceRepair => "agent_workspace_repair",
    Manual => "manual",
});

string_enum!(GitMutationKind {
    Fetch => "fetch",
    Merge => "merge",
    Rebase => "rebase",
    Push => "push",
    WorktreeCreate => "worktree_create",
    WorktreeDelete => "worktree_delete",
    Abort => "abort",
    Cleanup => "cleanup",
});

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BranchUpdateFailureKind {
    Conflict,
    Incomplete,
    Timeout,
    BranchMissing,
    DirtyWorkspace,
    CheckoutBusy,
    WorkspaceOwnershipInvalid,
    EnvironmentFailure,
    ContextCorrupt,
}

impl BranchUpdateFailureKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Conflict => "conflict",
            Self::Incomplete => "incomplete",
            Self::Timeout => "timeout",
            Self::BranchMissing => "branch_missing",
            Self::DirtyWorkspace => "dirty_workspace",
            Self::CheckoutBusy => "checkout_busy",
            Self::WorkspaceOwnershipInvalid => "workspace_ownership_invalid",
            Self::EnvironmentFailure => "environment_failure",
            Self::ContextCorrupt => "context_corrupt",
        }
    }

    pub fn policy(self) -> BranchUpdateFailurePolicy {
        match self {
            Self::Conflict | Self::Incomplete | Self::Timeout | Self::CheckoutBusy => {
                BranchUpdateFailurePolicy::Retryable
            }
            Self::BranchMissing
            | Self::DirtyWorkspace
            | Self::WorkspaceOwnershipInvalid
            | Self::EnvironmentFailure => BranchUpdateFailurePolicy::OperatorActionRequired,
            Self::ContextCorrupt => BranchUpdateFailurePolicy::TerminalForOperation,
        }
    }
}

impl FromStr for BranchUpdateFailureKind {
    type Err = StringEnumParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "conflict" => Ok(Self::Conflict),
            "incomplete" => Ok(Self::Incomplete),
            "timeout" => Ok(Self::Timeout),
            "branch_missing" => Ok(Self::BranchMissing),
            "dirty_workspace" => Ok(Self::DirtyWorkspace),
            "checkout_busy" => Ok(Self::CheckoutBusy),
            "workspace_ownership_invalid" => Ok(Self::WorkspaceOwnershipInvalid),
            "environment_failure" => Ok(Self::EnvironmentFailure),
            "context_corrupt" => Ok(Self::ContextCorrupt),
            _ => Err(StringEnumParseError {
                type_name: "BranchUpdateFailureKind",
                value: value.to_string(),
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchUpdateFailurePolicy {
    Retryable,
    OperatorActionRequired,
    TerminalForOperation,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GitTargetIdentity {
    git_common_dir: PathBuf,
    full_ref: String,
}

impl GitTargetIdentity {
    pub fn new(
        git_common_dir: PathBuf,
        full_ref: impl Into<String>,
    ) -> Result<Self, GitTargetIdentityError> {
        if !git_common_dir.is_absolute() {
            return Err(GitTargetIdentityError::CommonDirectoryNotAbsolute);
        }
        let full_ref = full_ref.into();
        validate_local_branch_ref(&full_ref)?;
        Ok(Self {
            git_common_dir,
            full_ref,
        })
    }

    pub fn git_common_dir(&self) -> &Path {
        &self.git_common_dir
    }

    pub fn full_ref(&self) -> &str {
        &self.full_ref
    }
}

fn validate_local_branch_ref(full_ref: &str) -> Result<(), GitTargetIdentityError> {
    let Some(branch) = full_ref.strip_prefix("refs/heads/") else {
        return Err(GitTargetIdentityError::NotLocalBranchRef);
    };
    let invalid = branch.is_empty()
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
    if invalid {
        return Err(GitTargetIdentityError::InvalidLocalBranchRef);
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum GitTargetIdentityError {
    #[error("git common directory must be an absolute canonical path")]
    CommonDirectoryNotAbsolute,
    #[error("target ref must be a full local branch ref under refs/heads/")]
    NotLocalBranchRef,
    #[error("target local branch ref is invalid")]
    InvalidLocalBranchRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitTargetLeaseOwner {
    pub kind: GitTargetLeaseOwnerKind,
    pub task_id: Option<String>,
    pub owner_id: String,
}

impl GitTargetLeaseOwner {
    pub fn branch_update(task_id: impl Into<String>, operation_id: impl Into<String>) -> Self {
        Self {
            kind: GitTargetLeaseOwnerKind::BranchUpdateOperation,
            task_id: Some(task_id.into()),
            owner_id: operation_id.into(),
        }
    }

    pub fn merge_attempt(task_id: impl Into<String>, attempt_id: impl Into<String>) -> Self {
        Self {
            kind: GitTargetLeaseOwnerKind::MergeAttempt,
            task_id: Some(task_id.into()),
            owner_id: attempt_id.into(),
        }
    }

    pub fn publication_recovery(
        task_id: impl Into<String>,
        operation_id: impl Into<String>,
    ) -> Self {
        Self {
            kind: GitTargetLeaseOwnerKind::PublicationRecovery,
            task_id: Some(task_id.into()),
            owner_id: operation_id.into(),
        }
    }

    pub fn agent_workspace_repair(attempt_id: impl Into<String>) -> Self {
        Self {
            kind: GitTargetLeaseOwnerKind::AgentWorkspaceRepair,
            task_id: None,
            owner_id: attempt_id.into(),
        }
    }

    pub fn manual(owner_id: impl Into<String>) -> Self {
        Self {
            kind: GitTargetLeaseOwnerKind::Manual,
            task_id: None,
            owner_id: owner_id.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitMutationClaim {
    pub identity: GitTargetIdentity,
    pub claim_id: String,
    pub kind: GitMutationKind,
    pub owner: GitTargetLeaseOwner,
    pub fencing_epoch: u64,
    pub process_group_id: Option<i64>,
    pub started_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitTargetLease {
    identity: GitTargetIdentity,
    owner: GitTargetLeaseOwner,
    fencing_epoch: u64,
    acquired_at: DateTime<Utc>,
    active_mutation: Option<GitMutationClaim>,
    released_at: Option<DateTime<Utc>>,
}

impl GitTargetLease {
    pub fn new(
        identity: GitTargetIdentity,
        owner: GitTargetLeaseOwner,
        fencing_epoch: u64,
        acquired_at: DateTime<Utc>,
    ) -> Self {
        Self {
            identity,
            owner,
            fencing_epoch,
            acquired_at,
            active_mutation: None,
            released_at: None,
        }
    }

    pub fn from_persisted(
        identity: GitTargetIdentity,
        owner: GitTargetLeaseOwner,
        fencing_epoch: u64,
        acquired_at: DateTime<Utc>,
        active_mutation: Option<GitMutationClaim>,
        released_at: Option<DateTime<Utc>>,
    ) -> Self {
        Self {
            identity,
            owner,
            fencing_epoch,
            acquired_at,
            active_mutation,
            released_at,
        }
    }

    pub fn identity(&self) -> &GitTargetIdentity {
        &self.identity
    }

    pub fn owner(&self) -> &GitTargetLeaseOwner {
        &self.owner
    }

    pub fn fencing_epoch(&self) -> u64 {
        self.fencing_epoch
    }

    pub fn acquired_at(&self) -> DateTime<Utc> {
        self.acquired_at
    }

    pub fn active_mutation(&self) -> Option<&GitMutationClaim> {
        self.active_mutation.as_ref()
    }

    pub fn is_released(&self) -> bool {
        self.released_at.is_some()
    }

    pub fn released_at(&self) -> Option<DateTime<Utc>> {
        self.released_at
    }

    pub fn begin_mutation(
        &mut self,
        owner: &GitTargetLeaseOwner,
        epoch: u64,
        kind: GitMutationKind,
        claim_id: impl Into<String>,
        started_at: DateTime<Utc>,
    ) -> Result<GitMutationClaim, GitTargetLeaseError> {
        self.verify_authority(owner, epoch)?;
        if self.active_mutation.is_some() {
            return Err(GitTargetLeaseError::MutationInFlight);
        }
        let claim = GitMutationClaim {
            identity: self.identity.clone(),
            claim_id: claim_id.into(),
            kind,
            owner: owner.clone(),
            fencing_epoch: epoch,
            process_group_id: None,
            started_at,
        };
        self.active_mutation = Some(claim.clone());
        Ok(claim)
    }

    pub fn complete_mutation(
        &mut self,
        owner: &GitTargetLeaseOwner,
        epoch: u64,
        claim_id: &str,
    ) -> Result<(), GitTargetLeaseError> {
        self.verify_authority(owner, epoch)?;
        match self.active_mutation.as_ref() {
            Some(claim) if claim.claim_id == claim_id => {
                self.active_mutation = None;
                Ok(())
            }
            _ => Err(GitTargetLeaseError::StaleMutationClaim),
        }
    }

    pub fn bind_process_group(
        &mut self,
        owner: &GitTargetLeaseOwner,
        epoch: u64,
        claim_id: &str,
        process_group_id: i64,
    ) -> Result<(), GitTargetLeaseError> {
        self.verify_authority(owner, epoch)?;
        match self.active_mutation.as_mut() {
            Some(claim) if claim.claim_id == claim_id => {
                claim.process_group_id = Some(process_group_id);
                Ok(())
            }
            _ => Err(GitTargetLeaseError::StaleMutationClaim),
        }
    }

    pub fn transfer(
        &mut self,
        owner: &GitTargetLeaseOwner,
        epoch: u64,
        next_owner: GitTargetLeaseOwner,
        transferred_at: DateTime<Utc>,
    ) -> Result<(), GitTargetLeaseError> {
        self.verify_authority(owner, epoch)?;
        if self.active_mutation.is_some() {
            return Err(GitTargetLeaseError::MutationInFlight);
        }
        self.owner = next_owner;
        self.fencing_epoch = self.fencing_epoch.saturating_add(1);
        self.acquired_at = transferred_at;
        Ok(())
    }

    pub fn release(
        &mut self,
        owner: &GitTargetLeaseOwner,
        epoch: u64,
    ) -> Result<(), GitTargetLeaseError> {
        self.verify_authority(owner, epoch)?;
        if self.active_mutation.is_some() {
            return Err(GitTargetLeaseError::MutationInFlight);
        }
        self.released_at = Some(Utc::now());
        Ok(())
    }

    fn verify_authority(
        &self,
        owner: &GitTargetLeaseOwner,
        epoch: u64,
    ) -> Result<(), GitTargetLeaseError> {
        if self.released_at.is_some() || &self.owner != owner || self.fencing_epoch != epoch {
            return Err(GitTargetLeaseError::StaleAuthority);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum GitTargetLeaseError {
    #[error("stale target-lease authority")]
    StaleAuthority,
    #[error("a protected git mutation is already in flight")]
    MutationInFlight,
    #[error("mutation claim does not match current authority")]
    StaleMutationClaim,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchUpdateOperation {
    pub id: BranchUpdateOperationId,
    pub task_id: TaskId,
    pub direction: BranchUpdateDirection,
    pub phase: BranchUpdatePhase,
    pub continuation: BranchUpdateContinuation,
    pub originating_history_id: String,
    pub source_branch: String,
    pub target_branch: String,
    pub observed_source_sha: Option<String>,
    pub observed_target_sha: Option<String>,
    pub resulting_sha: Option<String>,
    pub workspace_ownership: BranchUpdateWorkspaceOwnership,
    pub workspace_path: Option<PathBuf>,
    pub capacity_ownership: BranchUpdateCapacityOwnership,
    pub failure_kind: Option<BranchUpdateFailureKind>,
    pub conflict_files: Vec<PathBuf>,
    pub diagnostics: Option<String>,
    pub conversation_id: Option<String>,
    pub agent_run_id: Option<String>,
    pub continuation_claim_id: Option<String>,
    pub continuation_idempotency_key: Option<String>,
    pub continuation_receipt: Option<String>,
    pub target_identity: GitTargetIdentity,
    pub target_lease_epoch: u64,
    pub retry_count: u32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub settled_at: Option<DateTime<Utc>>,
}

impl BranchUpdateOperation {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        task_id: TaskId,
        direction: BranchUpdateDirection,
        continuation: BranchUpdateContinuation,
        originating_history_id: impl Into<String>,
        source_branch: impl Into<String>,
        target_branch: impl Into<String>,
        workspace_ownership: BranchUpdateWorkspaceOwnership,
        capacity_ownership: BranchUpdateCapacityOwnership,
        target_identity: GitTargetIdentity,
        now: DateTime<Utc>,
    ) -> Self {
        Self {
            id: BranchUpdateOperationId::new(),
            task_id,
            direction,
            phase: BranchUpdatePhase::Programmatic,
            continuation,
            originating_history_id: originating_history_id.into(),
            source_branch: source_branch.into(),
            target_branch: target_branch.into(),
            observed_source_sha: None,
            observed_target_sha: None,
            resulting_sha: None,
            workspace_ownership,
            workspace_path: None,
            capacity_ownership,
            failure_kind: None,
            conflict_files: Vec::new(),
            diagnostics: None,
            conversation_id: None,
            agent_run_id: None,
            continuation_claim_id: None,
            continuation_idempotency_key: None,
            continuation_receipt: None,
            target_identity,
            target_lease_epoch: 0,
            retry_count: 0,
            created_at: now,
            updated_at: now,
            settled_at: None,
        }
    }
}
