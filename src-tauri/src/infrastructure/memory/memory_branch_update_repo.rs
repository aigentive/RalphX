use async_trait::async_trait;
use chrono::Utc;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::domain::entities::{
    BranchUpdateOperation, BranchUpdateOperationId, BranchUpdatePhase, GitMutationClaim,
    GitTargetIdentity, GitTargetLease, GitTargetLeaseError, InternalStatus, TaskId,
};
use crate::domain::repositories::{
    BeginGitMutation, BindBranchUpdateRun, BlockBranchUpdate, BranchUpdateActivation,
    BranchUpdateActivationOutcome, BranchUpdateCasOutcome, BranchUpdateRepository,
    CheckpointBranchUpdateResult, ClaimBranchUpdateContinuation, CompleteBranchUpdateContinuation,
    CompleteGitMutation, GitAuthorityCasOutcome, MarkBranchUpdateResolving, PauseBranchUpdate,
    ResumeBranchUpdate, RetryBranchUpdate, SettleBranchUpdateProgrammatic, StopBranchUpdate,
    TaskRepository, TransferBranchUpdateTargetLease, UnbindBranchUpdateRun,
};
use crate::error::AppResult;

#[derive(Default)]
struct State {
    task_statuses: HashMap<TaskId, InternalStatus>,
    operations: HashMap<BranchUpdateOperationId, BranchUpdateOperation>,
    leases: HashMap<GitTargetIdentity, GitTargetLease>,
}

#[derive(Default)]
pub struct MemoryBranchUpdateRepository {
    state: Mutex<State>,
    task_repository: Option<Arc<dyn TaskRepository>>,
}

impl MemoryBranchUpdateRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_task_repository(mut self, task_repository: Arc<dyn TaskRepository>) -> Self {
        self.task_repository = Some(task_repository);
        self
    }

    pub async fn seed_task_status(&self, task_id: TaskId, status: InternalStatus) {
        self.state
            .lock()
            .await
            .task_statuses
            .insert(task_id, status);
    }

    async fn mirror_task_status(
        &self,
        task_id: &TaskId,
        status: InternalStatus,
        metadata: Option<String>,
    ) -> AppResult<()> {
        let Some(repository) = self.task_repository.as_ref() else {
            return Ok(());
        };
        let Some(mut task) = repository.get_by_id(task_id).await? else {
            return Err(crate::error::AppError::TaskNotFound(
                task_id.as_str().to_string(),
            ));
        };
        task.internal_status = status;
        if metadata.is_some() {
            task.metadata = metadata;
        }
        task.touch();
        repository.update(&task).await
    }
}

fn authority_outcome(error: GitTargetLeaseError) -> GitAuthorityCasOutcome {
    match error {
        GitTargetLeaseError::MutationInFlight => GitAuthorityCasOutcome::MutationInFlight,
        GitTargetLeaseError::StaleMutationClaim => GitAuthorityCasOutcome::StaleMutationClaim,
        GitTargetLeaseError::StaleAuthority => GitAuthorityCasOutcome::StaleAuthority,
    }
}

#[async_trait]
impl BranchUpdateRepository for MemoryBranchUpdateRepository {
    async fn get_operation(
        &self,
        operation_id: &BranchUpdateOperationId,
    ) -> AppResult<Option<BranchUpdateOperation>> {
        Ok(self
            .state
            .lock()
            .await
            .operations
            .get(operation_id)
            .cloned())
    }

    async fn get_active_operation(
        &self,
        task_id: &TaskId,
    ) -> AppResult<Option<BranchUpdateOperation>> {
        Ok(self
            .state
            .lock()
            .await
            .operations
            .values()
            .find(|operation| &operation.task_id == task_id && operation.settled_at.is_none())
            .cloned())
    }

    async fn list_active_operations(&self) -> AppResult<Vec<BranchUpdateOperation>> {
        let state = self.state.lock().await;
        let mut operations = state
            .operations
            .values()
            .filter(|operation| operation.settled_at.is_none())
            .cloned()
            .collect::<Vec<_>>();
        operations.sort_by_key(|operation| operation.created_at);
        Ok(operations)
    }

    async fn get_target_lease(
        &self,
        identity: &GitTargetIdentity,
    ) -> AppResult<Option<GitTargetLease>> {
        Ok(self.state.lock().await.leases.get(identity).cloned())
    }

    async fn acquire_target_lease(
        &self,
        request: crate::domain::repositories::AcquireGitTargetLease,
    ) -> AppResult<crate::domain::repositories::AcquireGitTargetLeaseOutcome> {
        use crate::domain::repositories::AcquireGitTargetLeaseOutcome;

        let mut state = self.state.lock().await;
        if let Some(lease) = state
            .leases
            .get(&request.identity)
            .filter(|lease| !lease.is_released())
        {
            if lease.owner() == &request.owner {
                return Ok(AcquireGitTargetLeaseOutcome::AlreadyOwned {
                    fencing_epoch: lease.fencing_epoch(),
                });
            }
            return Ok(AcquireGitTargetLeaseOutcome::TargetBusy {
                owner: lease.owner().clone(),
                fencing_epoch: lease.fencing_epoch(),
            });
        }
        let fencing_epoch = state
            .leases
            .get(&request.identity)
            .map(|lease| lease.fencing_epoch().saturating_add(1))
            .unwrap_or(1);
        state.leases.insert(
            request.identity.clone(),
            GitTargetLease::new(request.identity, request.owner, fencing_epoch, Utc::now()),
        );
        Ok(AcquireGitTargetLeaseOutcome::Acquired { fencing_epoch })
    }

    async fn activate(
        &self,
        request: BranchUpdateActivation,
    ) -> AppResult<BranchUpdateActivationOutcome> {
        let external_status = if let Some(repository) = self.task_repository.as_ref() {
            repository
                .get_by_id(&request.operation.task_id)
                .await?
                .map(|task| task.internal_status)
        } else {
            None
        };
        let mut state = self.state.lock().await;
        let current_status = external_status
            .as_ref()
            .or_else(|| state.task_statuses.get(&request.operation.task_id));
        if current_status != Some(&request.expected_status) {
            return Ok(BranchUpdateActivationOutcome::StaleTask);
        }
        if state.operations.values().any(|operation| {
            operation.task_id == request.operation.task_id && operation.settled_at.is_none()
        }) {
            return Ok(BranchUpdateActivationOutcome::ActiveOperationExists);
        }
        let identity = request.operation.target_identity.clone();
        if let Some(lease) = state
            .leases
            .get(&identity)
            .filter(|lease| !lease.is_released())
        {
            return Ok(BranchUpdateActivationOutcome::TargetBusy {
                owner: lease.owner().clone(),
                fencing_epoch: lease.fencing_epoch(),
            });
        }
        let epoch = state
            .leases
            .get(&identity)
            .map(|lease| lease.fencing_epoch().saturating_add(1))
            .unwrap_or(1);
        let owner = crate::domain::entities::GitTargetLeaseOwner::branch_update(
            request.operation.task_id.as_str(),
            request.operation.id.as_str(),
        );
        let mut operation = request.operation;
        operation.target_lease_epoch = epoch;
        state
            .task_statuses
            .insert(operation.task_id.clone(), request.update_status);
        state.leases.insert(
            identity.clone(),
            GitTargetLease::new(identity, owner, epoch, Utc::now()),
        );
        let operation_id = operation.id.clone();
        let history_id = operation.originating_history_id.clone();
        let task_id = operation.task_id.clone();
        state.operations.insert(operation_id.clone(), operation);
        drop(state);
        self.mirror_task_status(&task_id, request.update_status, None)
            .await?;
        Ok(BranchUpdateActivationOutcome::Applied {
            operation_id,
            history_id,
            fencing_epoch: epoch,
        })
    }

    async fn begin_git_mutation(
        &self,
        request: BeginGitMutation,
    ) -> AppResult<GitAuthorityCasOutcome> {
        let mut state = self.state.lock().await;
        let Some(lease) = state.leases.get_mut(&request.identity) else {
            return Ok(GitAuthorityCasOutcome::StaleAuthority);
        };
        Ok(
            match lease.begin_mutation(
                &request.owner,
                request.fencing_epoch,
                request.kind,
                request.claim_id,
                Utc::now(),
            ) {
                Ok(_) => GitAuthorityCasOutcome::Applied {
                    fencing_epoch: request.fencing_epoch,
                },
                Err(error) => authority_outcome(error),
            },
        )
    }

    async fn bind_git_process_group(
        &self,
        identity: &GitTargetIdentity,
        owner: &crate::domain::entities::GitTargetLeaseOwner,
        fencing_epoch: u64,
        claim_id: &str,
        process_group_id: i64,
    ) -> AppResult<GitAuthorityCasOutcome> {
        let mut state = self.state.lock().await;
        let Some(lease) = state.leases.get_mut(identity) else {
            return Ok(GitAuthorityCasOutcome::StaleAuthority);
        };
        Ok(
            match lease.bind_process_group(owner, fencing_epoch, claim_id, process_group_id) {
                Ok(()) => GitAuthorityCasOutcome::Applied { fencing_epoch },
                Err(error) => authority_outcome(error),
            },
        )
    }

    async fn complete_git_mutation(
        &self,
        request: CompleteGitMutation,
    ) -> AppResult<GitAuthorityCasOutcome> {
        let mut state = self.state.lock().await;
        let Some(lease) = state.leases.get_mut(&request.identity) else {
            return Ok(GitAuthorityCasOutcome::StaleAuthority);
        };
        Ok(
            match lease.complete_mutation(&request.owner, request.fencing_epoch, &request.claim_id)
            {
                Ok(()) => GitAuthorityCasOutcome::Applied {
                    fencing_epoch: request.fencing_epoch,
                },
                Err(error) => authority_outcome(error),
            },
        )
    }

    async fn checkpoint_result(
        &self,
        request: CheckpointBranchUpdateResult,
    ) -> AppResult<BranchUpdateCasOutcome> {
        let mut state = self.state.lock().await;
        if state.task_statuses.get(&request.task_id) != Some(&request.update_status) {
            return Ok(BranchUpdateCasOutcome::Stale);
        }
        let Some(snapshot) = state.operations.get(&request.operation_id) else {
            return Ok(BranchUpdateCasOutcome::Stale);
        };
        if snapshot.task_id != request.task_id
            || snapshot.originating_history_id != request.originating_history_id
            || !matches!(
                snapshot.phase,
                BranchUpdatePhase::Programmatic | BranchUpdatePhase::Resolving
            )
            || snapshot
                .resulting_sha
                .as_deref()
                .is_some_and(|sha| sha != request.resulting_sha)
        {
            return Ok(BranchUpdateCasOutcome::Stale);
        }
        let identity = snapshot.target_identity.clone();
        let Some(lease) = state.leases.get(&identity) else {
            return Ok(BranchUpdateCasOutcome::Stale);
        };
        if lease.owner() != &request.owner || lease.fencing_epoch() != request.fencing_epoch {
            return Ok(BranchUpdateCasOutcome::Stale);
        }
        if lease.active_mutation().is_some() {
            return Ok(BranchUpdateCasOutcome::MutationInFlight);
        }
        let operation = state
            .operations
            .get_mut(&request.operation_id)
            .expect("operation checked above");
        operation.resulting_sha = Some(request.resulting_sha);
        operation.updated_at = Utc::now();
        Ok(BranchUpdateCasOutcome::Applied)
    }

    async fn settle_programmatic(
        &self,
        request: SettleBranchUpdateProgrammatic,
    ) -> AppResult<BranchUpdateCasOutcome> {
        let mut state = self.state.lock().await;
        if state.task_statuses.get(&request.task_id) != Some(&request.update_status) {
            return Ok(BranchUpdateCasOutcome::Stale);
        }
        let Some(snapshot) = state.operations.get(&request.operation_id) else {
            return Ok(BranchUpdateCasOutcome::Stale);
        };
        if snapshot.task_id != request.task_id
            || snapshot.originating_history_id != request.originating_history_id
            || !matches!(
                snapshot.phase,
                BranchUpdatePhase::Programmatic | BranchUpdatePhase::Resolving
            )
            || snapshot.resulting_sha.as_deref() != Some(request.resulting_sha.as_str())
        {
            return Ok(BranchUpdateCasOutcome::Stale);
        }
        let identity = snapshot.target_identity.clone();
        let Some(lease) = state.leases.get(&identity) else {
            return Ok(BranchUpdateCasOutcome::Stale);
        };
        if lease.owner() != &request.owner || lease.fencing_epoch() != request.fencing_epoch {
            return Ok(BranchUpdateCasOutcome::Stale);
        }
        if lease.active_mutation().is_some() {
            return Ok(BranchUpdateCasOutcome::MutationInFlight);
        }
        let operation = state
            .operations
            .get_mut(&request.operation_id)
            .expect("operation checked above");
        operation.phase = BranchUpdatePhase::ContinuationPending;
        operation.updated_at = Utc::now();
        Ok(BranchUpdateCasOutcome::Applied)
    }

    async fn block_operation(
        &self,
        request: BlockBranchUpdate,
    ) -> AppResult<BranchUpdateCasOutcome> {
        let mut state = self.state.lock().await;
        if state.task_statuses.get(&request.task_id) != Some(&request.update_status) {
            return Ok(BranchUpdateCasOutcome::Stale);
        }
        let Some(snapshot) = state.operations.get(&request.operation_id) else {
            return Ok(BranchUpdateCasOutcome::Stale);
        };
        if snapshot.task_id != request.task_id
            || snapshot.originating_history_id != request.originating_history_id
            || !matches!(
                snapshot.phase,
                BranchUpdatePhase::Programmatic | BranchUpdatePhase::Resolving
            )
        {
            return Ok(BranchUpdateCasOutcome::Stale);
        }
        let identity = snapshot.target_identity.clone();
        let Some(lease) = state.leases.get(&identity) else {
            return Ok(BranchUpdateCasOutcome::Stale);
        };
        if lease.owner() != &request.owner || lease.fencing_epoch() != request.fencing_epoch {
            return Ok(BranchUpdateCasOutcome::Stale);
        }
        if lease.active_mutation().is_some() {
            return Ok(BranchUpdateCasOutcome::MutationInFlight);
        }
        let operation = state
            .operations
            .get_mut(&request.operation_id)
            .expect("operation checked above");
        operation.phase = BranchUpdatePhase::Blocked;
        operation.failure_kind = Some(request.failure_kind);
        operation.diagnostics = Some(request.diagnostics);
        operation.conflict_files = request.conflict_files;
        state
            .task_statuses
            .insert(request.task_id.clone(), InternalStatus::BranchUpdateBlocked);
        drop(state);
        self.mirror_task_status(&request.task_id, InternalStatus::BranchUpdateBlocked, None)
            .await?;
        Ok(BranchUpdateCasOutcome::Applied)
    }

    async fn mark_resolving(
        &self,
        request: MarkBranchUpdateResolving,
    ) -> AppResult<BranchUpdateCasOutcome> {
        let mut state = self.state.lock().await;
        if state.task_statuses.get(&request.task_id) != Some(&request.update_status) {
            return Ok(BranchUpdateCasOutcome::Stale);
        }
        let Some(snapshot) = state.operations.get(&request.operation_id) else {
            return Ok(BranchUpdateCasOutcome::Stale);
        };
        if snapshot.task_id != request.task_id
            || snapshot.originating_history_id != request.originating_history_id
            || snapshot.phase != BranchUpdatePhase::Programmatic
        {
            return Ok(BranchUpdateCasOutcome::Stale);
        }
        let identity = snapshot.target_identity.clone();
        let Some(lease) = state.leases.get(&identity) else {
            return Ok(BranchUpdateCasOutcome::Stale);
        };
        if lease.owner() != &request.owner || lease.fencing_epoch() != request.fencing_epoch {
            return Ok(BranchUpdateCasOutcome::Stale);
        }
        if lease.active_mutation().is_some() {
            return Ok(BranchUpdateCasOutcome::MutationInFlight);
        }
        let operation = state
            .operations
            .get_mut(&request.operation_id)
            .expect("operation checked above");
        operation.phase = BranchUpdatePhase::Resolving;
        operation.conflict_files = request.conflict_files;
        Ok(BranchUpdateCasOutcome::Applied)
    }

    async fn bind_agent_run(
        &self,
        request: BindBranchUpdateRun,
    ) -> AppResult<BranchUpdateCasOutcome> {
        let mut state = self.state.lock().await;
        if state.task_statuses.get(&request.task_id) != Some(&request.update_status) {
            return Ok(BranchUpdateCasOutcome::Stale);
        }
        let Some(operation) = state.operations.get_mut(&request.operation_id) else {
            return Ok(BranchUpdateCasOutcome::Stale);
        };
        if operation.task_id != request.task_id
            || operation.originating_history_id != request.originating_history_id
            || operation.phase != BranchUpdatePhase::Resolving
            || operation.conversation_id.is_some()
            || operation.agent_run_id.is_some()
        {
            return Ok(BranchUpdateCasOutcome::Stale);
        }
        operation.conversation_id = Some(request.conversation_id);
        operation.agent_run_id = Some(request.agent_run_id);
        operation.updated_at = Utc::now();
        Ok(BranchUpdateCasOutcome::Applied)
    }

    async fn unbind_agent_run(
        &self,
        request: UnbindBranchUpdateRun,
    ) -> AppResult<BranchUpdateCasOutcome> {
        let mut state = self.state.lock().await;
        if state.task_statuses.get(&request.task_id) != Some(&request.update_status) {
            return Ok(BranchUpdateCasOutcome::Stale);
        }
        let Some(operation) = state.operations.get_mut(&request.operation_id) else {
            return Ok(BranchUpdateCasOutcome::Stale);
        };
        if operation.task_id != request.task_id
            || operation.originating_history_id != request.originating_history_id
            || operation.phase != BranchUpdatePhase::Resolving
            || operation.conversation_id.as_deref() != Some(request.conversation_id.as_str())
            || operation.agent_run_id.as_deref() != Some(request.agent_run_id.as_str())
        {
            return Ok(BranchUpdateCasOutcome::Stale);
        }
        operation.conversation_id = None;
        operation.agent_run_id = None;
        operation.updated_at = Utc::now();
        Ok(BranchUpdateCasOutcome::Applied)
    }

    async fn claim_continuation(
        &self,
        request: ClaimBranchUpdateContinuation,
    ) -> AppResult<BranchUpdateCasOutcome> {
        let mut state = self.state.lock().await;
        if state.task_statuses.get(&request.task_id) != Some(&request.update_status) {
            return Ok(BranchUpdateCasOutcome::Stale);
        }
        let Some(operation) = state.operations.get_mut(&request.operation_id) else {
            return Ok(BranchUpdateCasOutcome::Stale);
        };
        if operation.task_id != request.task_id
            || operation.originating_history_id != request.originating_history_id
            || operation.phase != BranchUpdatePhase::ContinuationPending
            || operation.continuation_claim_id.is_some()
        {
            return Ok(BranchUpdateCasOutcome::Stale);
        }
        operation.phase = BranchUpdatePhase::ContinuationInProgress;
        operation.continuation_claim_id = Some(request.claim_id);
        operation.continuation_idempotency_key = Some(request.idempotency_key);
        Ok(BranchUpdateCasOutcome::Applied)
    }

    async fn complete_continuation(
        &self,
        request: CompleteBranchUpdateContinuation,
    ) -> AppResult<BranchUpdateCasOutcome> {
        let mut state = self.state.lock().await;
        if state.task_statuses.get(&request.task_id) != Some(&request.update_status) {
            return Ok(BranchUpdateCasOutcome::Stale);
        }
        let Some(snapshot) = state.operations.get(&request.operation_id) else {
            return Ok(BranchUpdateCasOutcome::Stale);
        };
        if snapshot.task_id != request.task_id
            || snapshot.originating_history_id != request.originating_history_id
            || snapshot.phase != BranchUpdatePhase::ContinuationInProgress
            || snapshot.continuation_claim_id.as_deref() != Some(&request.claim_id)
            || snapshot.continuation_idempotency_key.as_deref() != Some(&request.idempotency_key)
        {
            return Ok(BranchUpdateCasOutcome::Stale);
        }
        let identity = snapshot.target_identity.clone();
        let Some(lease) = state.leases.get_mut(&identity) else {
            return Ok(BranchUpdateCasOutcome::Stale);
        };
        if let Err(error) = lease.release(&request.owner, request.fencing_epoch) {
            return Ok(match authority_outcome(error) {
                GitAuthorityCasOutcome::MutationInFlight => {
                    BranchUpdateCasOutcome::MutationInFlight
                }
                _ => BranchUpdateCasOutcome::Stale,
            });
        }
        let operation = state
            .operations
            .get_mut(&request.operation_id)
            .expect("operation checked above");
        operation.phase = BranchUpdatePhase::Settled;
        operation.continuation_receipt = Some(request.receipt);
        operation.settled_at = Some(Utc::now());
        state
            .task_statuses
            .insert(request.task_id.clone(), request.destination_status);
        drop(state);
        self.mirror_task_status(&request.task_id, request.destination_status, None)
            .await?;
        Ok(BranchUpdateCasOutcome::Applied)
    }

    async fn transfer_target_lease(
        &self,
        identity: &GitTargetIdentity,
        owner: &crate::domain::entities::GitTargetLeaseOwner,
        fencing_epoch: u64,
        next_owner: crate::domain::entities::GitTargetLeaseOwner,
    ) -> AppResult<GitAuthorityCasOutcome> {
        let mut state = self.state.lock().await;
        let Some(lease) = state.leases.get_mut(identity) else {
            return Ok(GitAuthorityCasOutcome::StaleAuthority);
        };
        Ok(
            match lease.transfer(owner, fencing_epoch, next_owner, Utc::now()) {
                Ok(()) => GitAuthorityCasOutcome::Applied {
                    fencing_epoch: lease.fencing_epoch(),
                },
                Err(error) => authority_outcome(error),
            },
        )
    }

    async fn transfer_operation_target_lease(
        &self,
        request: TransferBranchUpdateTargetLease,
    ) -> AppResult<GitAuthorityCasOutcome> {
        let mut state = self.state.lock().await;
        if state.task_statuses.get(&request.task_id) != Some(&request.update_status) {
            return Ok(GitAuthorityCasOutcome::StaleAuthority);
        }
        let Some(snapshot) = state.operations.get(&request.operation_id) else {
            return Ok(GitAuthorityCasOutcome::StaleAuthority);
        };
        if snapshot.task_id != request.task_id
            || snapshot.originating_history_id != request.originating_history_id
            || snapshot.target_lease_epoch != request.fencing_epoch
            || snapshot.settled_at.is_some()
        {
            return Ok(GitAuthorityCasOutcome::StaleAuthority);
        }
        let identity = snapshot.target_identity.clone();
        let Some(lease) = state.leases.get_mut(&identity) else {
            return Ok(GitAuthorityCasOutcome::StaleAuthority);
        };
        if let Err(error) = lease.transfer(
            &request.owner,
            request.fencing_epoch,
            request.next_owner,
            Utc::now(),
        ) {
            return Ok(authority_outcome(error));
        }
        let next_epoch = lease.fencing_epoch();
        state
            .operations
            .get_mut(&request.operation_id)
            .expect("operation checked above")
            .target_lease_epoch = next_epoch;
        Ok(GitAuthorityCasOutcome::Applied {
            fencing_epoch: next_epoch,
        })
    }

    async fn pause_operation(
        &self,
        request: PauseBranchUpdate,
    ) -> AppResult<BranchUpdateCasOutcome> {
        let mut state = self.state.lock().await;
        if state.task_statuses.get(&request.task_id) != Some(&request.update_status) {
            return Ok(BranchUpdateCasOutcome::Stale);
        }
        let Some(snapshot) = state.operations.get(&request.operation_id) else {
            return Ok(BranchUpdateCasOutcome::Stale);
        };
        if snapshot.task_id != request.task_id
            || snapshot.originating_history_id != request.originating_history_id
            || snapshot.settled_at.is_some()
        {
            return Ok(BranchUpdateCasOutcome::Stale);
        }
        let Some(lease) = state.leases.get(&snapshot.target_identity) else {
            return Ok(BranchUpdateCasOutcome::Stale);
        };
        if lease.owner() != &request.owner || lease.fencing_epoch() != request.fencing_epoch {
            return Ok(BranchUpdateCasOutcome::Stale);
        }
        if lease.active_mutation().is_some() {
            return Ok(BranchUpdateCasOutcome::MutationInFlight);
        }
        let operation = state
            .operations
            .get_mut(&request.operation_id)
            .expect("operation checked above");
        operation.conversation_id = None;
        operation.agent_run_id = None;
        operation.updated_at = Utc::now();
        state
            .task_statuses
            .insert(request.task_id.clone(), InternalStatus::Paused);
        drop(state);
        self.mirror_task_status(
            &request.task_id,
            InternalStatus::Paused,
            request.task_metadata,
        )
        .await?;
        Ok(BranchUpdateCasOutcome::Applied)
    }

    async fn resume_operation(
        &self,
        request: ResumeBranchUpdate,
    ) -> AppResult<BranchUpdateCasOutcome> {
        let mut state = self.state.lock().await;
        if state.task_statuses.get(&request.task_id) != Some(&InternalStatus::Paused) {
            return Ok(BranchUpdateCasOutcome::Stale);
        }
        let Some(snapshot) = state.operations.get(&request.operation_id) else {
            return Ok(BranchUpdateCasOutcome::Stale);
        };
        if snapshot.task_id != request.task_id
            || snapshot.originating_history_id != request.originating_history_id
            || snapshot.settled_at.is_some()
        {
            return Ok(BranchUpdateCasOutcome::Stale);
        }
        let Some(lease) = state.leases.get(&snapshot.target_identity) else {
            return Ok(BranchUpdateCasOutcome::Stale);
        };
        if lease.owner() != &request.owner || lease.fencing_epoch() != request.fencing_epoch {
            return Ok(BranchUpdateCasOutcome::Stale);
        }
        if lease.active_mutation().is_some() {
            return Ok(BranchUpdateCasOutcome::MutationInFlight);
        }
        state
            .task_statuses
            .insert(request.task_id.clone(), request.update_status);
        drop(state);
        self.mirror_task_status(&request.task_id, request.update_status, None)
            .await?;
        Ok(BranchUpdateCasOutcome::Applied)
    }

    async fn stop_operation(&self, request: StopBranchUpdate) -> AppResult<BranchUpdateCasOutcome> {
        let mut state = self.state.lock().await;
        if state.task_statuses.get(&request.task_id) != Some(&request.update_status) {
            return Ok(BranchUpdateCasOutcome::Stale);
        }
        let Some(snapshot) = state.operations.get(&request.operation_id) else {
            return Ok(BranchUpdateCasOutcome::Stale);
        };
        if snapshot.task_id != request.task_id
            || snapshot.originating_history_id != request.originating_history_id
            || snapshot.settled_at.is_some()
        {
            return Ok(BranchUpdateCasOutcome::Stale);
        }
        let identity = snapshot.target_identity.clone();
        let Some(lease) = state.leases.get_mut(&identity) else {
            return Ok(BranchUpdateCasOutcome::Stale);
        };
        if let Err(error) = lease.release(&request.owner, request.fencing_epoch) {
            return Ok(match authority_outcome(error) {
                GitAuthorityCasOutcome::MutationInFlight => {
                    BranchUpdateCasOutcome::MutationInFlight
                }
                _ => BranchUpdateCasOutcome::Stale,
            });
        }
        let operation = state
            .operations
            .get_mut(&request.operation_id)
            .expect("operation checked above");
        operation.phase = BranchUpdatePhase::Settled;
        operation.capacity_ownership =
            crate::domain::entities::BranchUpdateCapacityOwnership::Released;
        operation.conversation_id = None;
        operation.agent_run_id = None;
        operation.diagnostics = request.reason;
        operation.settled_at = Some(Utc::now());
        operation.updated_at = Utc::now();
        state
            .task_statuses
            .insert(request.task_id.clone(), InternalStatus::Stopped);
        drop(state);
        self.mirror_task_status(&request.task_id, InternalStatus::Stopped, None)
            .await?;
        Ok(BranchUpdateCasOutcome::Applied)
    }

    async fn retry_operation(
        &self,
        request: RetryBranchUpdate,
    ) -> AppResult<BranchUpdateCasOutcome> {
        let mut state = self.state.lock().await;
        if state.task_statuses.get(&request.task_id) != Some(&InternalStatus::BranchUpdateBlocked) {
            return Ok(BranchUpdateCasOutcome::Stale);
        }
        let Some(snapshot) = state.operations.get(&request.operation_id).cloned() else {
            return Ok(BranchUpdateCasOutcome::Stale);
        };
        if snapshot.task_id != request.task_id
            || snapshot.originating_history_id != request.originating_history_id
            || snapshot.phase != BranchUpdatePhase::Blocked
            || snapshot.settled_at.is_some()
        {
            return Ok(BranchUpdateCasOutcome::Stale);
        }
        let Some(lease) = state.leases.get_mut(&snapshot.target_identity) else {
            return Ok(BranchUpdateCasOutcome::Stale);
        };
        let next_owner = crate::domain::entities::GitTargetLeaseOwner::branch_update(
            request.task_id.as_str(),
            request.new_operation_id.as_str(),
        );
        if let Err(error) = lease.transfer(
            &request.owner,
            request.fencing_epoch,
            next_owner,
            Utc::now(),
        ) {
            return Ok(match authority_outcome(error) {
                GitAuthorityCasOutcome::MutationInFlight => {
                    BranchUpdateCasOutcome::MutationInFlight
                }
                _ => BranchUpdateCasOutcome::Stale,
            });
        }
        let next_epoch = lease.fencing_epoch();
        let now = Utc::now();
        let previous = state
            .operations
            .get_mut(&request.operation_id)
            .expect("operation checked above");
        previous.phase = BranchUpdatePhase::Settled;
        previous.settled_at = Some(now);
        previous.updated_at = now;

        let mut retry = snapshot;
        retry.id = request.new_operation_id;
        retry.phase = if retry.conflict_files.is_empty() {
            BranchUpdatePhase::Programmatic
        } else {
            BranchUpdatePhase::Resolving
        };
        retry.originating_history_id = request.history_id;
        retry.resulting_sha = None;
        retry.failure_kind = None;
        retry.diagnostics = None;
        retry.conversation_id = None;
        retry.agent_run_id = None;
        retry.continuation_claim_id = None;
        retry.continuation_idempotency_key = None;
        retry.continuation_receipt = None;
        retry.target_lease_epoch = next_epoch;
        retry.retry_count = retry.retry_count.saturating_add(1);
        retry.created_at = now;
        retry.updated_at = now;
        retry.settled_at = None;
        state.operations.insert(retry.id.clone(), retry);
        state
            .task_statuses
            .insert(request.task_id.clone(), request.update_status);
        drop(state);
        self.mirror_task_status(&request.task_id, request.update_status, None)
            .await?;
        Ok(BranchUpdateCasOutcome::Applied)
    }

    async fn release_target_lease(
        &self,
        identity: &GitTargetIdentity,
        owner: &crate::domain::entities::GitTargetLeaseOwner,
        fencing_epoch: u64,
    ) -> AppResult<GitAuthorityCasOutcome> {
        let mut state = self.state.lock().await;
        let Some(lease) = state.leases.get_mut(identity) else {
            return Ok(GitAuthorityCasOutcome::StaleAuthority);
        };
        Ok(match lease.release(owner, fencing_epoch) {
            Ok(()) => GitAuthorityCasOutcome::Applied { fencing_epoch },
            Err(error) => authority_outcome(error),
        })
    }

    async fn list_in_flight_mutations(&self) -> AppResult<Vec<GitMutationClaim>> {
        Ok(self
            .state
            .lock()
            .await
            .leases
            .values()
            .filter_map(|lease| lease.active_mutation().cloned())
            .collect())
    }
}
