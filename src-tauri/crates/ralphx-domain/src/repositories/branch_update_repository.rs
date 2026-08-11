use async_trait::async_trait;

use crate::domain::entities::{
    BranchUpdateFailureKind, BranchUpdateOperation, BranchUpdateOperationId, GitMutationClaim,
    GitMutationKind, GitTargetIdentity, GitTargetLease, GitTargetLeaseOwner, InternalStatus,
    TaskId,
};
use crate::error::AppResult;

#[derive(Debug, Clone)]
pub struct BranchUpdateActivation {
    pub operation: BranchUpdateOperation,
    pub expected_status: InternalStatus,
    pub update_status: InternalStatus,
    pub trigger: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchUpdateActivationOutcome {
    Applied {
        operation_id: BranchUpdateOperationId,
        history_id: String,
        fencing_epoch: u64,
    },
    StaleTask,
    ActiveOperationExists,
    TargetBusy {
        owner: GitTargetLeaseOwner,
        fencing_epoch: u64,
    },
}

#[derive(Debug, Clone)]
pub struct BeginGitMutation {
    pub identity: GitTargetIdentity,
    pub owner: GitTargetLeaseOwner,
    pub fencing_epoch: u64,
    pub claim_id: String,
    pub kind: GitMutationKind,
}

#[derive(Debug, Clone)]
pub struct CompleteGitMutation {
    pub identity: GitTargetIdentity,
    pub owner: GitTargetLeaseOwner,
    pub fencing_epoch: u64,
    pub claim_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitAuthorityCasOutcome {
    Applied { fencing_epoch: u64 },
    StaleAuthority,
    MutationInFlight,
    StaleMutationClaim,
}

#[derive(Debug, Clone)]
pub struct AcquireGitTargetLease {
    pub identity: GitTargetIdentity,
    pub owner: GitTargetLeaseOwner,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcquireGitTargetLeaseOutcome {
    Acquired {
        fencing_epoch: u64,
    },
    AlreadyOwned {
        fencing_epoch: u64,
    },
    TargetBusy {
        owner: GitTargetLeaseOwner,
        fencing_epoch: u64,
    },
}

#[derive(Debug, Clone)]
pub struct SettleBranchUpdateProgrammatic {
    pub operation_id: BranchUpdateOperationId,
    pub task_id: TaskId,
    pub originating_history_id: String,
    pub update_status: InternalStatus,
    pub owner: GitTargetLeaseOwner,
    pub fencing_epoch: u64,
    pub resulting_sha: String,
}

#[derive(Debug, Clone)]
pub struct CheckpointBranchUpdateResult {
    pub operation_id: BranchUpdateOperationId,
    pub task_id: TaskId,
    pub originating_history_id: String,
    pub update_status: InternalStatus,
    pub owner: GitTargetLeaseOwner,
    pub fencing_epoch: u64,
    pub resulting_sha: String,
}

#[derive(Debug, Clone)]
pub struct BlockBranchUpdate {
    pub operation_id: BranchUpdateOperationId,
    pub task_id: TaskId,
    pub originating_history_id: String,
    pub update_status: InternalStatus,
    pub owner: GitTargetLeaseOwner,
    pub fencing_epoch: u64,
    pub failure_kind: BranchUpdateFailureKind,
    pub diagnostics: String,
    pub conflict_files: Vec<std::path::PathBuf>,
}

#[derive(Debug, Clone)]
pub struct MarkBranchUpdateResolving {
    pub operation_id: BranchUpdateOperationId,
    pub task_id: TaskId,
    pub originating_history_id: String,
    pub update_status: InternalStatus,
    pub owner: GitTargetLeaseOwner,
    pub fencing_epoch: u64,
    pub conflict_files: Vec<std::path::PathBuf>,
}

#[derive(Debug, Clone)]
pub struct BindBranchUpdateRun {
    pub operation_id: BranchUpdateOperationId,
    pub task_id: TaskId,
    pub originating_history_id: String,
    pub update_status: InternalStatus,
    pub conversation_id: String,
    pub agent_run_id: String,
}

#[derive(Debug, Clone)]
pub struct UnbindBranchUpdateRun {
    pub operation_id: BranchUpdateOperationId,
    pub task_id: TaskId,
    pub originating_history_id: String,
    pub update_status: InternalStatus,
    pub conversation_id: String,
    pub agent_run_id: String,
}

#[derive(Debug, Clone)]
pub struct ClaimBranchUpdateContinuation {
    pub operation_id: BranchUpdateOperationId,
    pub task_id: TaskId,
    pub originating_history_id: String,
    pub update_status: InternalStatus,
    pub claim_id: String,
    pub idempotency_key: String,
}

#[derive(Debug, Clone)]
pub struct CompleteBranchUpdateContinuation {
    pub operation_id: BranchUpdateOperationId,
    pub task_id: TaskId,
    pub originating_history_id: String,
    pub update_status: InternalStatus,
    pub destination_status: InternalStatus,
    pub owner: GitTargetLeaseOwner,
    pub fencing_epoch: u64,
    pub claim_id: String,
    pub idempotency_key: String,
    pub receipt: String,
    pub history_id: String,
}

#[derive(Debug, Clone)]
pub struct TransferBranchUpdateTargetLease {
    pub operation_id: BranchUpdateOperationId,
    pub task_id: TaskId,
    pub originating_history_id: String,
    pub update_status: InternalStatus,
    pub owner: GitTargetLeaseOwner,
    pub fencing_epoch: u64,
    pub next_owner: GitTargetLeaseOwner,
}

#[derive(Debug, Clone)]
pub struct PauseBranchUpdate {
    pub operation_id: BranchUpdateOperationId,
    pub task_id: TaskId,
    pub originating_history_id: String,
    pub update_status: InternalStatus,
    pub owner: GitTargetLeaseOwner,
    pub fencing_epoch: u64,
    pub history_id: String,
    /// Optional task metadata that must be persisted atomically with the pause.
    pub task_metadata: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ResumeBranchUpdate {
    pub operation_id: BranchUpdateOperationId,
    pub task_id: TaskId,
    pub originating_history_id: String,
    pub update_status: InternalStatus,
    pub owner: GitTargetLeaseOwner,
    pub fencing_epoch: u64,
    pub history_id: String,
}

#[derive(Debug, Clone)]
pub struct StopBranchUpdate {
    pub operation_id: BranchUpdateOperationId,
    pub task_id: TaskId,
    pub originating_history_id: String,
    pub update_status: InternalStatus,
    pub owner: GitTargetLeaseOwner,
    pub fencing_epoch: u64,
    pub history_id: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RetryBranchUpdate {
    pub operation_id: BranchUpdateOperationId,
    pub new_operation_id: BranchUpdateOperationId,
    pub task_id: TaskId,
    pub originating_history_id: String,
    pub update_status: InternalStatus,
    pub owner: GitTargetLeaseOwner,
    pub fencing_epoch: u64,
    pub history_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchUpdateCasOutcome {
    Applied,
    Stale,
    MutationInFlight,
}

#[async_trait]
pub trait BranchUpdateRepository: Send + Sync {
    async fn get_operation(
        &self,
        operation_id: &BranchUpdateOperationId,
    ) -> AppResult<Option<BranchUpdateOperation>>;

    async fn get_active_operation(
        &self,
        task_id: &TaskId,
    ) -> AppResult<Option<BranchUpdateOperation>>;

    async fn list_active_operations(&self) -> AppResult<Vec<BranchUpdateOperation>>;

    async fn get_target_lease(
        &self,
        identity: &GitTargetIdentity,
    ) -> AppResult<Option<GitTargetLease>>;

    async fn acquire_target_lease(
        &self,
        request: AcquireGitTargetLease,
    ) -> AppResult<AcquireGitTargetLeaseOutcome>;

    async fn activate(
        &self,
        request: BranchUpdateActivation,
    ) -> AppResult<BranchUpdateActivationOutcome>;

    async fn begin_git_mutation(
        &self,
        request: BeginGitMutation,
    ) -> AppResult<GitAuthorityCasOutcome>;

    async fn bind_git_process_group(
        &self,
        identity: &GitTargetIdentity,
        owner: &GitTargetLeaseOwner,
        fencing_epoch: u64,
        claim_id: &str,
        process_group_id: i64,
    ) -> AppResult<GitAuthorityCasOutcome>;

    async fn complete_git_mutation(
        &self,
        request: CompleteGitMutation,
    ) -> AppResult<GitAuthorityCasOutcome>;

    /// Durably records the exact commit produced by the operation before the target
    /// ref or operation worktree is mutated. Repeating the same checkpoint is
    /// idempotent; a different result is stale.
    async fn checkpoint_result(
        &self,
        request: CheckpointBranchUpdateResult,
    ) -> AppResult<BranchUpdateCasOutcome>;

    async fn settle_programmatic(
        &self,
        request: SettleBranchUpdateProgrammatic,
    ) -> AppResult<BranchUpdateCasOutcome>;

    async fn block_operation(
        &self,
        request: BlockBranchUpdate,
    ) -> AppResult<BranchUpdateCasOutcome>;

    async fn mark_resolving(
        &self,
        request: MarkBranchUpdateResolving,
    ) -> AppResult<BranchUpdateCasOutcome>;

    async fn bind_agent_run(
        &self,
        request: BindBranchUpdateRun,
    ) -> AppResult<BranchUpdateCasOutcome>;

    async fn unbind_agent_run(
        &self,
        request: UnbindBranchUpdateRun,
    ) -> AppResult<BranchUpdateCasOutcome>;

    async fn claim_continuation(
        &self,
        request: ClaimBranchUpdateContinuation,
    ) -> AppResult<BranchUpdateCasOutcome>;

    async fn complete_continuation(
        &self,
        request: CompleteBranchUpdateContinuation,
    ) -> AppResult<BranchUpdateCasOutcome>;

    async fn transfer_target_lease(
        &self,
        identity: &GitTargetIdentity,
        owner: &GitTargetLeaseOwner,
        fencing_epoch: u64,
        next_owner: GitTargetLeaseOwner,
    ) -> AppResult<GitAuthorityCasOutcome>;

    /// Atomically hand an active operation's target lease to its next effect owner and
    /// advance the operation epoch. This is the only transfer suitable for a later
    /// operation continuation CAS; the generic lease transfer intentionally does not
    /// mutate operation state.
    async fn transfer_operation_target_lease(
        &self,
        request: TransferBranchUpdateTargetLease,
    ) -> AppResult<GitAuthorityCasOutcome>;

    async fn pause_operation(
        &self,
        request: PauseBranchUpdate,
    ) -> AppResult<BranchUpdateCasOutcome>;

    async fn resume_operation(
        &self,
        request: ResumeBranchUpdate,
    ) -> AppResult<BranchUpdateCasOutcome>;

    async fn stop_operation(&self, request: StopBranchUpdate) -> AppResult<BranchUpdateCasOutcome>;

    async fn retry_operation(
        &self,
        request: RetryBranchUpdate,
    ) -> AppResult<BranchUpdateCasOutcome>;

    async fn release_target_lease(
        &self,
        identity: &GitTargetIdentity,
        owner: &GitTargetLeaseOwner,
        fencing_epoch: u64,
    ) -> AppResult<GitAuthorityCasOutcome>;

    async fn list_in_flight_mutations(&self) -> AppResult<Vec<GitMutationClaim>>;
}
