use async_trait::async_trait;
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use std::sync::Arc;
use tokio::sync::Mutex;

use super::rows;
use crate::domain::entities::{
    BranchUpdateOperation, BranchUpdateOperationId, GitMutationClaim, GitTargetIdentity,
    GitTargetLease, GitTargetLeaseOwner, TaskId,
};
use crate::domain::ideation::TasksFeatureAction;
use crate::domain::repositories::{
    AcquireGitTargetLease, AcquireGitTargetLeaseOutcome, BeginGitMutation, BindBranchUpdateRun,
    BlockBranchUpdate, BranchUpdateActivation, BranchUpdateActivationOutcome,
    BranchUpdateCasOutcome, BranchUpdateRepository, CheckpointBranchUpdateResult,
    ClaimBranchUpdateContinuation, CompleteBranchUpdateContinuation, CompleteGitMutation,
    GitAuthorityCasOutcome, MarkBranchUpdateResolving, PauseBranchUpdate, ResumeBranchUpdate,
    RetryBranchUpdate, SettleBranchUpdateProgrammatic, StopBranchUpdate,
    TransferBranchUpdateTargetLease, UnbindBranchUpdateRun,
};
use crate::error::{AppError, AppResult};
use crate::infrastructure::sqlite::DbConnection;

pub struct SqliteBranchUpdateRepository {
    db: DbConnection,
    enforce_tasks_feature_policy: bool,
}

impl SqliteBranchUpdateRepository {
    pub fn new(conn: Connection) -> Self {
        Self {
            db: DbConnection::new(conn),
            enforce_tasks_feature_policy: false,
        }
    }

    pub fn from_shared(conn: Arc<Mutex<Connection>>) -> Self {
        Self {
            db: DbConnection::from_shared(conn),
            enforce_tasks_feature_policy: false,
        }
    }

    pub(crate) fn with_tasks_feature_policy(mut self) -> Self {
        self.enforce_tasks_feature_policy = true;
        self
    }
}

fn authorize_branch_update_progress(conn: &Connection, task_id: &TaskId) -> AppResult<()> {
    let exists = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM tasks WHERE id = ?1)",
        [task_id.as_str()],
        |row| row.get::<_, bool>(0),
    )?;
    if !exists {
        return Err(AppError::TaskNotFound(task_id.as_str().to_string()));
    }
    crate::infrastructure::sqlite::sqlite_ideation_settings_repo::authorize_tasks_session_sync(
        conn,
        None,
        TasksFeatureAction::Progress,
    )
}

#[derive(Debug)]
struct LeaseAuthorityRow {
    owner: GitTargetLeaseOwner,
    epoch: u64,
    claim_id: Option<String>,
    released: bool,
}

fn load_lease_authority(
    conn: &Connection,
    identity: &GitTargetIdentity,
) -> AppResult<Option<LeaseAuthorityRow>> {
    conn.query_row(
        "SELECT owner_kind, owner_task_id, owner_id, fencing_epoch,
                mutation_claim_id, released_at
         FROM git_target_leases WHERE git_common_dir = ?1 AND target_ref = ?2",
        params![
            identity.git_common_dir().to_string_lossy(),
            identity.full_ref()
        ],
        |row| {
            let owner_kind: String = row.get(0)?;
            let epoch: i64 = row.get(3)?;
            Ok((
                owner_kind,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
                epoch,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
            ))
        },
    )
    .optional()?
    .map(
        |(owner_kind, task_id, owner_id, epoch, claim_id, released_at)| {
            Ok(LeaseAuthorityRow {
                owner: GitTargetLeaseOwner {
                    kind: owner_kind.parse().map_err(
                        |error: crate::domain::entities::branch_update::StringEnumParseError| {
                            AppError::Database(error.to_string())
                        },
                    )?,
                    task_id,
                    owner_id,
                },
                epoch: epoch
                    .try_into()
                    .map_err(|_| AppError::Database("negative target lease epoch".to_string()))?,
                claim_id,
                released: released_at.is_some(),
            })
        },
    )
    .transpose()
}

fn write_branch_update_task_metadata(
    conn: &Connection,
    task_id: &TaskId,
    branch_update: Option<serde_json::Value>,
) -> AppResult<()> {
    let raw: Option<String> = conn.query_row(
        "SELECT metadata FROM tasks WHERE id = ?1",
        [task_id.as_str()],
        |row| row.get(0),
    )?;
    let mut metadata = raw
        .as_deref()
        .and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    let object = metadata
        .as_object_mut()
        .ok_or_else(|| AppError::Database("task metadata is not an object".to_string()))?;
    if let Some(mut branch_update) = branch_update {
        if let (Some(existing), Some(next)) = (
            object
                .get_mut("branch_update")
                .and_then(serde_json::Value::as_object_mut),
            branch_update.as_object_mut(),
        ) {
            existing.append(next);
        } else {
            object.insert("branch_update".to_string(), branch_update);
        }
    } else {
        object.remove("branch_update");
    }
    conn.execute(
        "UPDATE tasks SET metadata = ?1 WHERE id = ?2",
        params![metadata.to_string(), task_id.as_str()],
    )?;
    Ok(())
}

fn classify_authority(
    current: Option<&LeaseAuthorityRow>,
    owner: &GitTargetLeaseOwner,
    epoch: u64,
) -> Option<GitAuthorityCasOutcome> {
    match current {
        Some(row) if !row.released && &row.owner == owner && row.epoch == epoch => None,
        _ => Some(GitAuthorityCasOutcome::StaleAuthority),
    }
}

fn insert_operation(
    conn: &Connection,
    operation: &BranchUpdateOperation,
    fencing_epoch: u64,
) -> AppResult<()> {
    let conflict_files = serde_json::to_string(&operation.conflict_files)
        .map_err(|error| AppError::Database(error.to_string()))?;
    conn.execute(
        "INSERT INTO branch_update_operations (
            id, task_id, direction, phase, continuation, originating_history_id,
            attempt_id, source_branch, target_branch, observed_source_sha,
            observed_target_sha, resulting_sha, workspace_ownership, workspace_path,
            capacity_ownership, failure_kind, conflict_files_json, diagnostics,
            conversation_id, agent_run_id, continuation_claim_id,
            continuation_idempotency_key, continuation_receipt, git_common_dir,
            target_ref, target_identity_version, target_lease_epoch, retry_count,
            created_at, updated_at, settled_at
         ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, NULL, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
            ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, 1, ?25,
            ?26, ?27, ?28, ?29
         )",
        params![
            operation.id.as_str(),
            operation.task_id.as_str(),
            operation.direction.as_str(),
            operation.phase.as_str(),
            operation.continuation.as_str(),
            operation.originating_history_id,
            operation.source_branch,
            operation.target_branch,
            operation.observed_source_sha,
            operation.observed_target_sha,
            operation.resulting_sha,
            operation.workspace_ownership.as_str(),
            operation
                .workspace_path
                .as_ref()
                .map(|path| path.to_string_lossy()),
            operation.capacity_ownership.as_str(),
            operation.failure_kind.map(|kind| kind.as_str()),
            conflict_files,
            operation.diagnostics,
            operation.conversation_id,
            operation.agent_run_id,
            operation.continuation_claim_id,
            operation.continuation_idempotency_key,
            operation.continuation_receipt,
            operation.target_identity.git_common_dir().to_string_lossy(),
            operation.target_identity.full_ref(),
            i64::try_from(fencing_epoch)
                .map_err(|_| AppError::Database("target lease epoch overflow".to_string()))?,
            operation.retry_count,
            operation.created_at.to_rfc3339(),
            operation.updated_at.to_rfc3339(),
            operation.settled_at.map(|value| value.to_rfc3339()),
        ],
    )?;
    Ok(())
}

#[async_trait]
impl BranchUpdateRepository for SqliteBranchUpdateRepository {
    async fn get_operation(
        &self,
        operation_id: &BranchUpdateOperationId,
    ) -> AppResult<Option<BranchUpdateOperation>> {
        let operation_id = operation_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let sql = format!(
                    "SELECT {} FROM branch_update_operations WHERE id = ?1",
                    rows::OPERATION_COLUMNS
                );
                conn.query_row(&sql, [operation_id], |row| {
                    rows::operation_from_row(row).map_err(|error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            0,
                            rusqlite::types::Type::Text,
                            Box::new(error),
                        )
                    })
                })
                .optional()
                .map_err(AppError::from)
            })
            .await
    }

    async fn get_active_operation(
        &self,
        task_id: &TaskId,
    ) -> AppResult<Option<BranchUpdateOperation>> {
        let task_id = task_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let sql = format!(
                    "SELECT {} FROM branch_update_operations
                     WHERE task_id = ?1 AND settled_at IS NULL",
                    rows::OPERATION_COLUMNS
                );
                conn.query_row(&sql, [task_id], |row| {
                    rows::operation_from_row(row).map_err(|error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            0,
                            rusqlite::types::Type::Text,
                            Box::new(error),
                        )
                    })
                })
                .optional()
                .map_err(AppError::from)
            })
            .await
    }

    async fn list_active_operations(&self) -> AppResult<Vec<BranchUpdateOperation>> {
        self.db
            .run(move |conn| {
                let sql = format!(
                    "SELECT {} FROM branch_update_operations
                     WHERE settled_at IS NULL ORDER BY created_at ASC",
                    rows::OPERATION_COLUMNS
                );
                let mut statement = conn.prepare(&sql)?;
                let operations = statement
                    .query_map([], |row| {
                        rows::operation_from_row(row).map_err(|error| {
                            rusqlite::Error::FromSqlConversionFailure(
                                0,
                                rusqlite::types::Type::Text,
                                Box::new(error),
                            )
                        })
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(operations)
            })
            .await
    }

    async fn get_target_lease(
        &self,
        identity: &GitTargetIdentity,
    ) -> AppResult<Option<GitTargetLease>> {
        let common_dir = identity.git_common_dir().to_string_lossy().into_owned();
        let target_ref = identity.full_ref().to_string();
        self.db
            .run(move |conn| {
                let sql = format!(
                    "SELECT {} FROM git_target_leases
                     WHERE git_common_dir = ?1 AND target_ref = ?2",
                    rows::LEASE_COLUMNS
                );
                conn.query_row(&sql, params![common_dir, target_ref], |row| {
                    rows::lease_from_row(row).map_err(|error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            0,
                            rusqlite::types::Type::Text,
                            Box::new(error),
                        )
                    })
                })
                .optional()
                .map_err(AppError::from)
            })
            .await
    }

    async fn acquire_target_lease(
        &self,
        request: AcquireGitTargetLease,
    ) -> AppResult<AcquireGitTargetLeaseOutcome> {
        self.db
            .run_transaction(move |conn| {
                let current = load_lease_authority(conn, &request.identity)?;
                if let Some(current) = current.as_ref().filter(|lease| !lease.released) {
                    if current.owner == request.owner {
                        return Ok(AcquireGitTargetLeaseOutcome::AlreadyOwned {
                            fencing_epoch: current.epoch,
                        });
                    }
                    return Ok(AcquireGitTargetLeaseOutcome::TargetBusy {
                        owner: current.owner.clone(),
                        fencing_epoch: current.epoch,
                    });
                }
                let fencing_epoch = current
                    .as_ref()
                    .map(|lease| lease.epoch.saturating_add(1))
                    .unwrap_or(1);
                let now = Utc::now().to_rfc3339();
                conn.execute(
                    "INSERT INTO git_target_leases (
                        git_common_dir, target_ref, identity_version, owner_kind,
                        owner_task_id, owner_id, fencing_epoch, acquired_at, recovery_state
                     ) VALUES (?1, ?2, 1, ?3, ?4, ?5, ?6, ?7, 'ready')
                     ON CONFLICT(git_common_dir, target_ref) DO UPDATE SET
                        owner_kind = excluded.owner_kind,
                        owner_task_id = excluded.owner_task_id,
                        owner_id = excluded.owner_id,
                        fencing_epoch = excluded.fencing_epoch,
                        acquired_at = excluded.acquired_at,
                        recovery_state = 'ready',
                        mutation_claim_id = NULL,
                        mutation_kind = NULL,
                        mutation_process_group_id = NULL,
                        mutation_started_at = NULL,
                        released_at = NULL,
                        updated_at = excluded.acquired_at",
                    params![
                        request.identity.git_common_dir().to_string_lossy(),
                        request.identity.full_ref(),
                        request.owner.kind.as_str(),
                        request.owner.task_id,
                        request.owner.owner_id,
                        i64::try_from(fencing_epoch).map_err(|_| AppError::Database(
                            "target lease epoch overflow".to_string()
                        ))?,
                        now,
                    ],
                )?;
                Ok(AcquireGitTargetLeaseOutcome::Acquired { fencing_epoch })
            })
            .await
    }

    async fn activate(
        &self,
        request: BranchUpdateActivation,
    ) -> AppResult<BranchUpdateActivationOutcome> {
        let enforce_tasks_feature_policy = self.enforce_tasks_feature_policy;
        self.db
            .run_transaction(move |conn| {
                if enforce_tasks_feature_policy {
                    authorize_branch_update_progress(conn, &request.operation.task_id)?;
                }
                let current_status: Option<String> = conn
                    .query_row(
                        "SELECT internal_status FROM tasks WHERE id = ?1",
                        [request.operation.task_id.as_str()],
                        |row| row.get(0),
                    )
                    .optional()?;
                let Some(current_status) = current_status else {
                    return Err(AppError::TaskNotFound(
                        request.operation.task_id.as_str().to_string(),
                    ));
                };
                if current_status != request.expected_status.as_str() {
                    return Ok(BranchUpdateActivationOutcome::StaleTask);
                }

                let active_exists: bool = conn.query_row(
                    "SELECT EXISTS(
                        SELECT 1 FROM branch_update_operations
                        WHERE task_id = ?1 AND settled_at IS NULL
                    )",
                    [request.operation.task_id.as_str()],
                    |row| row.get(0),
                )?;
                if active_exists {
                    return Ok(BranchUpdateActivationOutcome::ActiveOperationExists);
                }

                let identity = &request.operation.target_identity;
                let current_lease = load_lease_authority(conn, identity)?;
                if let Some(lease) = current_lease.as_ref().filter(|lease| !lease.released) {
                    return Ok(BranchUpdateActivationOutcome::TargetBusy {
                        owner: lease.owner.clone(),
                        fencing_epoch: lease.epoch,
                    });
                }
                let fencing_epoch = current_lease
                    .as_ref()
                    .map(|lease| lease.epoch.saturating_add(1))
                    .unwrap_or(1);
                let owner = GitTargetLeaseOwner::branch_update(
                    request.operation.task_id.as_str(),
                    request.operation.id.as_str(),
                );
                conn.execute(
                    "INSERT INTO git_target_leases (
                        git_common_dir, target_ref, identity_version, owner_kind,
                        owner_task_id, owner_id, fencing_epoch, acquired_at, recovery_state
                     ) VALUES (?1, ?2, 1, ?3, ?4, ?5, ?6, ?7, 'ready')
                     ON CONFLICT(git_common_dir, target_ref) DO UPDATE SET
                        owner_kind = excluded.owner_kind,
                        owner_task_id = excluded.owner_task_id,
                        owner_id = excluded.owner_id,
                        fencing_epoch = excluded.fencing_epoch,
                        acquired_at = excluded.acquired_at,
                        recovery_state = 'ready',
                        mutation_claim_id = NULL,
                        mutation_kind = NULL,
                        mutation_process_group_id = NULL,
                        mutation_started_at = NULL,
                        released_at = NULL,
                        updated_at = excluded.acquired_at",
                    params![
                        identity.git_common_dir().to_string_lossy(),
                        identity.full_ref(),
                        owner.kind.as_str(),
                        owner.task_id,
                        owner.owner_id,
                        i64::try_from(fencing_epoch).map_err(|_| AppError::Database(
                            "target lease epoch overflow".to_string()
                        ))?,
                        Utc::now().to_rfc3339(),
                    ],
                )?;

                let changed = conn.execute(
                    "UPDATE tasks SET internal_status = ?1,
                        updated_at = strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')
                     WHERE id = ?2 AND internal_status = ?3",
                    params![
                        request.update_status.as_str(),
                        request.operation.task_id.as_str(),
                        request.expected_status.as_str(),
                    ],
                )?;
                if changed != 1 {
                    return Err(AppError::Database(
                        "task status changed during branch-update activation".to_string(),
                    ));
                }
                write_branch_update_task_metadata(
                    conn,
                    &request.operation.task_id,
                    Some(serde_json::json!({
                        "operation_id": request.operation.id.as_str(),
                        "direction": request.operation.direction.as_str(),
                        "phase": request.operation.phase.as_str(),
                        "source_branch": request.operation.source_branch.as_str(),
                        "target_branch": request.operation.target_branch.as_str(),
                    })),
                )?;
                conn.execute(
                    "INSERT INTO task_state_history (
                        id, task_id, from_status, to_status, changed_by, reason, metadata
                     ) VALUES (?1, ?2, ?3, ?4, 'system', ?5, '{}')",
                    params![
                        request.operation.originating_history_id,
                        request.operation.task_id.as_str(),
                        request.expected_status.as_str(),
                        request.update_status.as_str(),
                        request.trigger,
                    ],
                )?;
                insert_operation(conn, &request.operation, fencing_epoch)?;

                Ok(BranchUpdateActivationOutcome::Applied {
                    operation_id: request.operation.id.clone(),
                    history_id: request.operation.originating_history_id.clone(),
                    fencing_epoch,
                })
            })
            .await
    }

    async fn begin_git_mutation(
        &self,
        request: BeginGitMutation,
    ) -> AppResult<GitAuthorityCasOutcome> {
        let enforce_tasks_feature_policy = self.enforce_tasks_feature_policy;
        self.db
            .run_transaction(move |conn| {
                if enforce_tasks_feature_policy {
                    if let Some(task_id) = request.owner.task_id.as_ref() {
                        authorize_branch_update_progress(
                            conn,
                            &TaskId::from_string(task_id.clone()),
                        )?;
                    }
                }
                let current = load_lease_authority(conn, &request.identity)?;
                if let Some(outcome) =
                    classify_authority(current.as_ref(), &request.owner, request.fencing_epoch)
                {
                    return Ok(outcome);
                }
                if current
                    .as_ref()
                    .and_then(|row| row.claim_id.as_ref())
                    .is_some()
                {
                    return Ok(GitAuthorityCasOutcome::MutationInFlight);
                }
                conn.execute(
                    "UPDATE git_target_leases SET
                        mutation_claim_id = ?1, mutation_kind = ?2,
                        mutation_process_group_id = NULL, mutation_started_at = ?3,
                        recovery_state = 'mutation_in_flight', updated_at = ?3
                     WHERE git_common_dir = ?4 AND target_ref = ?5",
                    params![
                        request.claim_id,
                        request.kind.as_str(),
                        Utc::now().to_rfc3339(),
                        request.identity.git_common_dir().to_string_lossy(),
                        request.identity.full_ref(),
                    ],
                )?;
                Ok(GitAuthorityCasOutcome::Applied {
                    fencing_epoch: request.fencing_epoch,
                })
            })
            .await
    }

    async fn bind_git_process_group(
        &self,
        identity: &GitTargetIdentity,
        owner: &GitTargetLeaseOwner,
        fencing_epoch: u64,
        claim_id: &str,
        process_group_id: i64,
    ) -> AppResult<GitAuthorityCasOutcome> {
        let identity = identity.clone();
        let owner = owner.clone();
        let claim_id = claim_id.to_string();
        self.db
            .run_transaction(move |conn| {
                let current = load_lease_authority(conn, &identity)?;
                if let Some(outcome) = classify_authority(current.as_ref(), &owner, fencing_epoch) {
                    return Ok(outcome);
                }
                if current.as_ref().and_then(|row| row.claim_id.as_deref()) != Some(&claim_id) {
                    return Ok(GitAuthorityCasOutcome::StaleMutationClaim);
                }
                conn.execute(
                    "UPDATE git_target_leases SET mutation_process_group_id = ?1,
                        updated_at = strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')
                     WHERE git_common_dir = ?2 AND target_ref = ?3",
                    params![
                        process_group_id,
                        identity.git_common_dir().to_string_lossy(),
                        identity.full_ref(),
                    ],
                )?;
                Ok(GitAuthorityCasOutcome::Applied { fencing_epoch })
            })
            .await
    }

    async fn complete_git_mutation(
        &self,
        request: CompleteGitMutation,
    ) -> AppResult<GitAuthorityCasOutcome> {
        self.db
            .run_transaction(move |conn| {
                let current = load_lease_authority(conn, &request.identity)?;
                if let Some(outcome) =
                    classify_authority(current.as_ref(), &request.owner, request.fencing_epoch)
                {
                    return Ok(outcome);
                }
                if current.as_ref().and_then(|row| row.claim_id.as_deref())
                    != Some(request.claim_id.as_str())
                {
                    return Ok(GitAuthorityCasOutcome::StaleMutationClaim);
                }
                conn.execute(
                    "UPDATE git_target_leases SET mutation_claim_id = NULL,
                        mutation_kind = NULL, mutation_process_group_id = NULL,
                        mutation_started_at = NULL, recovery_state = 'ready',
                        updated_at = strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')
                     WHERE git_common_dir = ?1 AND target_ref = ?2",
                    params![
                        request.identity.git_common_dir().to_string_lossy(),
                        request.identity.full_ref(),
                    ],
                )?;
                Ok(GitAuthorityCasOutcome::Applied {
                    fencing_epoch: request.fencing_epoch,
                })
            })
            .await
    }

    async fn checkpoint_result(
        &self,
        request: CheckpointBranchUpdateResult,
    ) -> AppResult<BranchUpdateCasOutcome> {
        let enforce_tasks_feature_policy = self.enforce_tasks_feature_policy;
        self.db
            .run_transaction(move |conn| {
                if enforce_tasks_feature_policy {
                    authorize_branch_update_progress(conn, &request.task_id)?;
                }
                let epoch = i64::try_from(request.fencing_epoch)
                    .map_err(|_| AppError::Database("target lease epoch overflow".to_string()))?;
                let changed = conn.execute(
                    "UPDATE branch_update_operations SET resulting_sha = ?1, updated_at = ?2
                     WHERE id = ?3 AND task_id = ?4 AND originating_history_id = ?5
                       AND phase IN ('programmatic', 'resolving') AND settled_at IS NULL
                       AND (resulting_sha IS NULL OR resulting_sha = ?1)
                       AND target_lease_epoch = ?6
                       AND EXISTS (
                         SELECT 1 FROM git_target_leases lease
                         WHERE lease.git_common_dir = branch_update_operations.git_common_dir
                           AND lease.target_ref = branch_update_operations.target_ref
                           AND lease.owner_kind = ?7 AND lease.owner_id = ?8
                           AND lease.fencing_epoch = ?6 AND lease.released_at IS NULL
                           AND lease.mutation_claim_id IS NULL
                       )
                       AND EXISTS (SELECT 1 FROM tasks WHERE id = ?4 AND internal_status = ?9)",
                    params![
                        request.resulting_sha,
                        Utc::now().to_rfc3339(),
                        request.operation_id.as_str(),
                        request.task_id.as_str(),
                        request.originating_history_id,
                        epoch,
                        request.owner.kind.as_str(),
                        request.owner.owner_id,
                        request.update_status.as_str(),
                    ],
                )?;
                Ok(if changed == 1 {
                    BranchUpdateCasOutcome::Applied
                } else {
                    BranchUpdateCasOutcome::Stale
                })
            })
            .await
    }

    async fn settle_programmatic(
        &self,
        request: SettleBranchUpdateProgrammatic,
    ) -> AppResult<BranchUpdateCasOutcome> {
        let enforce_tasks_feature_policy = self.enforce_tasks_feature_policy;
        self.db
            .run_transaction(move |conn| {
                if enforce_tasks_feature_policy {
                    authorize_branch_update_progress(conn, &request.task_id)?;
                }
                let identity = conn
                    .query_row(
                        "SELECT git_common_dir, target_ref FROM branch_update_operations
                         WHERE id = ?1 AND task_id = ?2 AND originating_history_id = ?3
                           AND phase IN ('programmatic', 'resolving') AND settled_at IS NULL
                           AND resulting_sha = ?4",
                        params![
                            request.operation_id.as_str(),
                            request.task_id.as_str(),
                            request.originating_history_id,
                            request.resulting_sha,
                        ],
                        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                    )
                    .optional()?;
                let Some((common_dir, target_ref)) = identity else {
                    return Ok(BranchUpdateCasOutcome::Stale);
                };
                let identity = GitTargetIdentity::new(common_dir.into(), target_ref)
                    .map_err(|error| AppError::Database(error.to_string()))?;
                let lease = load_lease_authority(conn, &identity)?;
                if classify_authority(lease.as_ref(), &request.owner, request.fencing_epoch)
                    .is_some()
                {
                    return Ok(BranchUpdateCasOutcome::Stale);
                }
                if lease.as_ref().is_some_and(|lease| lease.claim_id.is_some()) {
                    return Ok(BranchUpdateCasOutcome::MutationInFlight);
                }
                let changed = conn.execute(
                    "UPDATE branch_update_operations SET phase = 'continuation_pending',
                        updated_at = ?1
                     WHERE id = ?2 AND task_id = ?3 AND originating_history_id = ?4
                       AND phase IN ('programmatic', 'resolving') AND settled_at IS NULL
                       AND resulting_sha = ?5
                       AND EXISTS (SELECT 1 FROM tasks WHERE id = ?3 AND internal_status = ?6)",
                    params![
                        Utc::now().to_rfc3339(),
                        request.operation_id.as_str(),
                        request.task_id.as_str(),
                        request.originating_history_id,
                        request.resulting_sha,
                        request.update_status.as_str(),
                    ],
                )?;
                Ok(if changed == 1 {
                    BranchUpdateCasOutcome::Applied
                } else {
                    BranchUpdateCasOutcome::Stale
                })
            })
            .await
    }

    async fn block_operation(
        &self,
        request: BlockBranchUpdate,
    ) -> AppResult<BranchUpdateCasOutcome> {
        let enforce_tasks_feature_policy = self.enforce_tasks_feature_policy;
        self.db
            .run_transaction(move |conn| {
                if enforce_tasks_feature_policy {
                    authorize_branch_update_progress(conn, &request.task_id)?;
                }
                let conflicts = serde_json::to_string(&request.conflict_files)
                    .map_err(|error| AppError::Database(error.to_string()))?;
                let changed = conn.execute(
                    "UPDATE branch_update_operations SET phase = 'blocked', failure_kind = ?1,
                        conflict_files_json = ?2, diagnostics = ?3, updated_at = ?4
                     WHERE id = ?5 AND task_id = ?6 AND originating_history_id = ?7
                       AND phase IN ('programmatic', 'resolving') AND settled_at IS NULL
                       AND target_lease_epoch = ?8
                       AND EXISTS (
                         SELECT 1 FROM git_target_leases lease
                         WHERE lease.git_common_dir = branch_update_operations.git_common_dir
                           AND lease.target_ref = branch_update_operations.target_ref
                           AND lease.owner_kind = ?9 AND lease.owner_id = ?10
                           AND lease.fencing_epoch = ?8 AND lease.released_at IS NULL
                           AND lease.mutation_claim_id IS NULL
                       )
                       AND EXISTS (SELECT 1 FROM tasks WHERE id = ?6 AND internal_status = ?11)",
                    params![
                        request.failure_kind.as_str(),
                        conflicts,
                        request.diagnostics,
                        Utc::now().to_rfc3339(),
                        request.operation_id.as_str(),
                        request.task_id.as_str(),
                        request.originating_history_id,
                        i64::try_from(request.fencing_epoch).map_err(|_| AppError::Database(
                            "target lease epoch overflow".to_string()
                        ))?,
                        request.owner.kind.as_str(),
                        request.owner.owner_id,
                        request.update_status.as_str(),
                    ],
                )?;
                if changed != 1 {
                    return Ok(BranchUpdateCasOutcome::Stale);
                }
                let task_changed = conn.execute(
                    "UPDATE tasks SET internal_status = 'branch_update_blocked', updated_at = ?1
                     WHERE id = ?2 AND internal_status = ?3",
                    params![
                        Utc::now().to_rfc3339(),
                        request.task_id.as_str(),
                        request.update_status.as_str(),
                    ],
                )?;
                if task_changed != 1 {
                    return Err(AppError::Database(
                        "task changed during branch-update blocking".to_string(),
                    ));
                }
                write_branch_update_task_metadata(
                    conn,
                    &request.task_id,
                    Some(serde_json::json!({
                        "operation_id": request.operation_id.as_str(),
                        "phase": "blocked",
                        "failure_kind": request.failure_kind.as_str(),
                        "diagnostics": request.diagnostics,
                        "conflict_files": request.conflict_files,
                    })),
                )?;
                conn.execute(
                    "INSERT INTO task_state_history (
                        id, task_id, from_status, to_status, changed_by, reason, metadata
                     ) VALUES (?1, ?2, ?3, 'branch_update_blocked', 'system',
                        'branch_update_blocked', '{}')",
                    params![
                        uuid::Uuid::new_v4().to_string(),
                        request.task_id.as_str(),
                        request.update_status.as_str(),
                    ],
                )?;
                Ok(BranchUpdateCasOutcome::Applied)
            })
            .await
    }

    async fn mark_resolving(
        &self,
        request: MarkBranchUpdateResolving,
    ) -> AppResult<BranchUpdateCasOutcome> {
        let enforce_tasks_feature_policy = self.enforce_tasks_feature_policy;
        self.db
            .run(move |conn| {
                if enforce_tasks_feature_policy {
                    authorize_branch_update_progress(conn, &request.task_id)?;
                }
                let conflicts = serde_json::to_string(&request.conflict_files)
                    .map_err(|error| AppError::Database(error.to_string()))?;
                let epoch = i64::try_from(request.fencing_epoch)
                    .map_err(|_| AppError::Database("target lease epoch overflow".to_string()))?;
                let changed = conn.execute(
                    "UPDATE branch_update_operations SET phase = 'resolving',
                        conflict_files_json = ?1, updated_at = ?2
                     WHERE id = ?3 AND task_id = ?4 AND originating_history_id = ?5
                       AND phase = 'programmatic' AND settled_at IS NULL
                       AND target_lease_epoch = ?6
                       AND EXISTS (
                         SELECT 1 FROM git_target_leases lease
                         WHERE lease.git_common_dir = branch_update_operations.git_common_dir
                           AND lease.target_ref = branch_update_operations.target_ref
                           AND lease.owner_kind = ?7 AND lease.owner_id = ?8
                           AND lease.fencing_epoch = ?6 AND lease.released_at IS NULL
                           AND lease.mutation_claim_id IS NULL
                       )
                       AND EXISTS (SELECT 1 FROM tasks WHERE id = ?4 AND internal_status = ?9)",
                    params![
                        conflicts,
                        Utc::now().to_rfc3339(),
                        request.operation_id.as_str(),
                        request.task_id.as_str(),
                        request.originating_history_id,
                        epoch,
                        request.owner.kind.as_str(),
                        request.owner.owner_id,
                        request.update_status.as_str(),
                    ],
                )?;
                Ok(if changed == 1 {
                    BranchUpdateCasOutcome::Applied
                } else {
                    BranchUpdateCasOutcome::Stale
                })
            })
            .await
    }

    async fn bind_agent_run(
        &self,
        request: BindBranchUpdateRun,
    ) -> AppResult<BranchUpdateCasOutcome> {
        self.db
            .run_transaction(move |conn| {
                let changed = conn.execute(
                    "UPDATE branch_update_operations SET conversation_id = ?1,
                        agent_run_id = ?2, updated_at = ?3
                     WHERE id = ?4 AND task_id = ?5 AND originating_history_id = ?6
                       AND phase = 'resolving' AND settled_at IS NULL
                       AND conversation_id IS NULL AND agent_run_id IS NULL
                       AND EXISTS (SELECT 1 FROM tasks WHERE id = ?5 AND internal_status = ?7)",
                    params![
                        request.conversation_id,
                        request.agent_run_id,
                        Utc::now().to_rfc3339(),
                        request.operation_id.as_str(),
                        request.task_id.as_str(),
                        request.originating_history_id,
                        request.update_status.as_str(),
                    ],
                )?;
                if changed != 1 {
                    return Ok(BranchUpdateCasOutcome::Stale);
                }
                let history_metadata: Option<String> = conn
                    .query_row(
                        "SELECT metadata FROM task_state_history WHERE id = ?1 AND task_id = ?2",
                        params![request.originating_history_id, request.task_id.as_str()],
                        |row| row.get(0),
                    )
                    .optional()?
                    .flatten();
                let mut metadata = history_metadata
                    .as_deref()
                    .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
                    .unwrap_or_else(|| serde_json::json!({}));
                let object = metadata.as_object_mut().ok_or_else(|| {
                    AppError::Database(
                        "branch-update history metadata is not an object".to_string(),
                    )
                })?;
                object.insert(
                    "conversation_id".to_string(),
                    serde_json::Value::String(request.conversation_id),
                );
                object.insert(
                    "agent_run_id".to_string(),
                    serde_json::Value::String(request.agent_run_id),
                );
                let history_changed = conn.execute(
                    "UPDATE task_state_history SET metadata = ?1 WHERE id = ?2 AND task_id = ?3",
                    params![
                        metadata.to_string(),
                        request.originating_history_id,
                        request.task_id.as_str()
                    ],
                )?;
                if history_changed != 1 {
                    return Err(AppError::Database(
                        "branch-update run binding history row is missing".to_string(),
                    ));
                }
                Ok(BranchUpdateCasOutcome::Applied)
            })
            .await
    }

    async fn unbind_agent_run(
        &self,
        request: UnbindBranchUpdateRun,
    ) -> AppResult<BranchUpdateCasOutcome> {
        self.db
            .run_transaction(move |conn| {
                let changed = conn.execute(
                    "UPDATE branch_update_operations SET conversation_id = NULL,
                        agent_run_id = NULL, updated_at = ?1
                     WHERE id = ?2 AND task_id = ?3 AND originating_history_id = ?4
                       AND phase = 'resolving' AND settled_at IS NULL
                       AND conversation_id = ?5 AND agent_run_id = ?6
                       AND EXISTS (SELECT 1 FROM tasks WHERE id = ?3 AND internal_status = ?7)",
                    params![
                        Utc::now().to_rfc3339(),
                        request.operation_id.as_str(),
                        request.task_id.as_str(),
                        request.originating_history_id,
                        request.conversation_id,
                        request.agent_run_id,
                        request.update_status.as_str(),
                    ],
                )?;
                if changed != 1 {
                    return Ok(BranchUpdateCasOutcome::Stale);
                }
                let history_metadata: Option<String> = conn
                    .query_row(
                        "SELECT metadata FROM task_state_history WHERE id = ?1 AND task_id = ?2",
                        params![request.originating_history_id, request.task_id.as_str()],
                        |row| row.get(0),
                    )
                    .optional()?
                    .flatten();
                let mut metadata = history_metadata
                    .as_deref()
                    .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
                    .unwrap_or_else(|| serde_json::json!({}));
                let object = metadata.as_object_mut().ok_or_else(|| {
                    AppError::Database(
                        "branch-update history metadata is not an object".to_string(),
                    )
                })?;
                object.remove("conversation_id");
                object.remove("agent_run_id");
                let history_changed = conn.execute(
                    "UPDATE task_state_history SET metadata = ?1 WHERE id = ?2 AND task_id = ?3",
                    params![
                        metadata.to_string(),
                        request.originating_history_id,
                        request.task_id.as_str()
                    ],
                )?;
                if history_changed != 1 {
                    return Err(AppError::Database(
                        "branch-update run binding history row is missing".to_string(),
                    ));
                }
                Ok(BranchUpdateCasOutcome::Applied)
            })
            .await
    }

    async fn claim_continuation(
        &self,
        request: ClaimBranchUpdateContinuation,
    ) -> AppResult<BranchUpdateCasOutcome> {
        let enforce_tasks_feature_policy = self.enforce_tasks_feature_policy;
        self.db
            .run(move |conn| {
                if enforce_tasks_feature_policy {
                    authorize_branch_update_progress(conn, &request.task_id)?;
                }
                let changed = conn.execute(
                    "UPDATE branch_update_operations SET phase = 'continuation_in_progress',
                        continuation_claim_id = ?1, continuation_idempotency_key = ?2,
                        updated_at = ?3
                     WHERE id = ?4 AND task_id = ?5 AND originating_history_id = ?6
                       AND phase = 'continuation_pending' AND settled_at IS NULL
                       AND continuation_claim_id IS NULL
                       AND EXISTS (SELECT 1 FROM tasks WHERE id = ?5 AND internal_status = ?7)",
                    params![
                        request.claim_id,
                        request.idempotency_key,
                        Utc::now().to_rfc3339(),
                        request.operation_id.as_str(),
                        request.task_id.as_str(),
                        request.originating_history_id,
                        request.update_status.as_str(),
                    ],
                )?;
                Ok(if changed == 1 {
                    BranchUpdateCasOutcome::Applied
                } else {
                    BranchUpdateCasOutcome::Stale
                })
            })
            .await
    }

    async fn complete_continuation(
        &self,
        request: CompleteBranchUpdateContinuation,
    ) -> AppResult<BranchUpdateCasOutcome> {
        let enforce_tasks_feature_policy = self.enforce_tasks_feature_policy;
        self.db
            .run_transaction(move |conn| {
                if enforce_tasks_feature_policy {
                    authorize_branch_update_progress(conn, &request.task_id)?;
                }
                let now = Utc::now().to_rfc3339();
                let epoch = i64::try_from(request.fencing_epoch)
                    .map_err(|_| AppError::Database("target lease epoch overflow".to_string()))?;
                let changed = conn.execute(
                    "UPDATE branch_update_operations SET phase = 'settled',
                        continuation_receipt = ?1, settled_at = ?2, updated_at = ?2
                     WHERE id = ?3 AND task_id = ?4 AND originating_history_id = ?5
                       AND phase = 'continuation_in_progress' AND settled_at IS NULL
                       AND continuation_claim_id = ?6 AND continuation_idempotency_key = ?7
                       AND target_lease_epoch = ?8
                       AND EXISTS (
                         SELECT 1 FROM git_target_leases lease
                         WHERE lease.git_common_dir = branch_update_operations.git_common_dir
                           AND lease.target_ref = branch_update_operations.target_ref
                           AND lease.owner_kind = ?9 AND lease.owner_id = ?10
                           AND lease.fencing_epoch = ?8 AND lease.released_at IS NULL
                           AND lease.mutation_claim_id IS NULL
                       )
                       AND EXISTS (SELECT 1 FROM tasks WHERE id = ?4 AND internal_status = ?11)",
                    params![
                        request.receipt,
                        now,
                        request.operation_id.as_str(),
                        request.task_id.as_str(),
                        request.originating_history_id,
                        request.claim_id,
                        request.idempotency_key,
                        epoch,
                        request.owner.kind.as_str(),
                        request.owner.owner_id,
                        request.update_status.as_str(),
                    ],
                )?;
                if changed != 1 {
                    return Ok(BranchUpdateCasOutcome::Stale);
                }
                let task_metadata: Option<String> = conn.query_row(
                    "SELECT metadata FROM tasks WHERE id = ?1 AND internal_status = ?2",
                    params![request.task_id.as_str(), request.update_status.as_str()],
                    |row| row.get(0),
                )?;
                let mut task_metadata = task_metadata
                    .as_deref()
                    .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
                    .unwrap_or_else(|| serde_json::json!({}));
                if request.destination_status == crate::domain::entities::InternalStatus::Merged {
                    let metadata = task_metadata.as_object_mut().ok_or_else(|| {
                        AppError::Database("task metadata is not an object".to_string())
                    })?;
                    metadata.insert("pending_cleanup".to_string(), serde_json::json!(true));
                    for key in [
                        "error",
                        "error_code",
                        "branch_freshness_conflict",
                        "plan_update_conflict",
                        "pr_branch_update_conflict",
                        "pr_branch_publication_conflict",
                        "publication_remote_ref",
                        "conflict_files",
                    ] {
                        metadata.remove(key);
                    }
                    let mut recovery = crate::domain::entities::task_metadata::MergeRecoveryMetadata::from_task_metadata(
                        Some(task_metadata.to_string().as_str()),
                    )
                    .map_err(|error| AppError::Database(error.to_string()))?
                    .unwrap_or_else(
                        crate::domain::entities::task_metadata::MergeRecoveryMetadata::new,
                    );
                    let attempt_count = recovery
                        .events
                        .iter()
                        .filter(|event| {
                            matches!(
                                event.kind,
                                crate::domain::entities::task_metadata::MergeRecoveryEventKind::AutoRetryTriggered
                            )
                        })
                        .count() as u32
                        + 1;
                    recovery.append_event_with_state(
                        crate::domain::entities::task_metadata::MergeRecoveryEvent::new(
                            crate::domain::entities::task_metadata::MergeRecoveryEventKind::AttemptSucceeded,
                            crate::domain::entities::task_metadata::MergeRecoverySource::System,
                            crate::domain::entities::task_metadata::MergeRecoveryReasonCode::Unknown,
                            "Merge completed after durable PR branch publication".to_string(),
                        )
                        .with_attempt(attempt_count),
                        crate::domain::entities::task_metadata::MergeRecoveryState::Succeeded,
                    );
                    task_metadata = serde_json::from_str(
                        &recovery
                            .update_task_metadata(Some(task_metadata.to_string().as_str()))
                            .map_err(|error| AppError::Database(error.to_string()))?,
                    )
                    .map_err(|error| AppError::Database(error.to_string()))?;
                }
                let task_changed = conn.execute(
                    "UPDATE tasks SET internal_status = ?1,
                        merge_commit_sha = CASE WHEN ?1 = 'merged' THEN (
                            SELECT resulting_sha FROM branch_update_operations WHERE id = ?5
                        ) ELSE merge_commit_sha END,
                        metadata = ?6, updated_at = ?2
                     WHERE id = ?3 AND internal_status = ?4",
                    params![
                        request.destination_status.as_str(),
                        now,
                        request.task_id.as_str(),
                        request.update_status.as_str(),
                        request.operation_id.as_str(),
                        task_metadata.to_string(),
                    ],
                )?;
                if task_changed != 1 {
                    return Err(AppError::Database(
                        "task changed during branch-update continuation".to_string(),
                    ));
                }
                conn.execute(
                    "INSERT INTO task_state_history (
                        id, task_id, from_status, to_status, changed_by, reason, metadata
                     ) VALUES (?1, ?2, ?3, ?4, 'system', 'branch_update_continuation', ?5)",
                    params![
                        request.history_id,
                        request.task_id.as_str(),
                        request.update_status.as_str(),
                        request.destination_status.as_str(),
                        serde_json::json!({
                            "branch_update_operation_id": request.operation_id.as_str(),
                            "continuation_receipt": request.receipt,
                        })
                        .to_string(),
                    ],
                )?;
                conn.execute(
                    "UPDATE git_target_leases SET released_at = ?1, updated_at = ?1
                     WHERE owner_kind = ?2 AND owner_id = ?3 AND fencing_epoch = ?4
                       AND mutation_claim_id IS NULL AND released_at IS NULL",
                    params![
                        now,
                        request.owner.kind.as_str(),
                        request.owner.owner_id,
                        epoch
                    ],
                )?;
                Ok(BranchUpdateCasOutcome::Applied)
            })
            .await
    }

    async fn transfer_target_lease(
        &self,
        identity: &GitTargetIdentity,
        owner: &GitTargetLeaseOwner,
        fencing_epoch: u64,
        next_owner: GitTargetLeaseOwner,
    ) -> AppResult<GitAuthorityCasOutcome> {
        let identity = identity.clone();
        let owner = owner.clone();
        self.db
            .run_transaction(move |conn| {
                let current = load_lease_authority(conn, &identity)?;
                if let Some(outcome) = classify_authority(current.as_ref(), &owner, fencing_epoch) {
                    return Ok(outcome);
                }
                if current
                    .as_ref()
                    .and_then(|row| row.claim_id.as_ref())
                    .is_some()
                {
                    return Ok(GitAuthorityCasOutcome::MutationInFlight);
                }
                let next_epoch = fencing_epoch.saturating_add(1);
                conn.execute(
                    "UPDATE git_target_leases SET owner_kind = ?1, owner_task_id = ?2,
                        owner_id = ?3, fencing_epoch = ?4, acquired_at = ?5,
                        recovery_state = 'ready', released_at = NULL, updated_at = ?5
                     WHERE git_common_dir = ?6 AND target_ref = ?7",
                    params![
                        next_owner.kind.as_str(),
                        next_owner.task_id,
                        next_owner.owner_id,
                        i64::try_from(next_epoch).map_err(|_| AppError::Database(
                            "target lease epoch overflow".to_string()
                        ))?,
                        Utc::now().to_rfc3339(),
                        identity.git_common_dir().to_string_lossy(),
                        identity.full_ref(),
                    ],
                )?;
                Ok(GitAuthorityCasOutcome::Applied {
                    fencing_epoch: next_epoch,
                })
            })
            .await
    }

    async fn transfer_operation_target_lease(
        &self,
        request: TransferBranchUpdateTargetLease,
    ) -> AppResult<GitAuthorityCasOutcome> {
        self.db
            .run_transaction(move |conn| {
                let operation_identity: Option<(String, String, i64)> = conn
                    .query_row(
                        "SELECT git_common_dir, target_ref, target_lease_epoch
                         FROM branch_update_operations
                         WHERE id = ?1 AND task_id = ?2 AND originating_history_id = ?3
                           AND settled_at IS NULL
                           AND EXISTS (
                             SELECT 1 FROM tasks WHERE id = ?2 AND internal_status = ?4
                           )",
                        params![
                            request.operation_id.as_str(),
                            request.task_id.as_str(),
                            request.originating_history_id,
                            request.update_status.as_str(),
                        ],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                    )
                    .optional()?;
                let Some((git_common_dir, target_ref, operation_epoch)) = operation_identity else {
                    return Ok(GitAuthorityCasOutcome::StaleAuthority);
                };
                let epoch = i64::try_from(request.fencing_epoch)
                    .map_err(|_| AppError::Database("target lease epoch overflow".to_string()))?;
                if operation_epoch != epoch {
                    return Ok(GitAuthorityCasOutcome::StaleAuthority);
                }
                let identity =
                    GitTargetIdentity::new(std::path::PathBuf::from(git_common_dir), target_ref)
                        .map_err(|error| AppError::Database(error.to_string()))?;
                let current = load_lease_authority(conn, &identity)?;
                if let Some(outcome) =
                    classify_authority(current.as_ref(), &request.owner, request.fencing_epoch)
                {
                    return Ok(outcome);
                }
                if current
                    .as_ref()
                    .and_then(|row| row.claim_id.as_ref())
                    .is_some()
                {
                    return Ok(GitAuthorityCasOutcome::MutationInFlight);
                }
                let next_epoch = request.fencing_epoch.saturating_add(1);
                let next_epoch_db = i64::try_from(next_epoch)
                    .map_err(|_| AppError::Database("target lease epoch overflow".to_string()))?;
                let now = Utc::now().to_rfc3339();
                let lease_changed = conn.execute(
                    "UPDATE git_target_leases SET owner_kind = ?1, owner_task_id = ?2,
                        owner_id = ?3, fencing_epoch = ?4, acquired_at = ?5,
                        recovery_state = 'ready', released_at = NULL, updated_at = ?5
                     WHERE git_common_dir = ?6 AND target_ref = ?7
                       AND owner_kind = ?8 AND owner_id = ?9 AND fencing_epoch = ?10
                       AND mutation_claim_id IS NULL AND released_at IS NULL",
                    params![
                        request.next_owner.kind.as_str(),
                        request.next_owner.task_id,
                        request.next_owner.owner_id,
                        next_epoch_db,
                        now,
                        identity.git_common_dir().to_string_lossy(),
                        identity.full_ref(),
                        request.owner.kind.as_str(),
                        request.owner.owner_id,
                        epoch,
                    ],
                )?;
                if lease_changed != 1 {
                    return Ok(GitAuthorityCasOutcome::StaleAuthority);
                }
                let operation_changed = conn.execute(
                    "UPDATE branch_update_operations SET target_lease_epoch = ?1, updated_at = ?2
                     WHERE id = ?3 AND task_id = ?4 AND originating_history_id = ?5
                       AND target_lease_epoch = ?6 AND settled_at IS NULL",
                    params![
                        next_epoch_db,
                        now,
                        request.operation_id.as_str(),
                        request.task_id.as_str(),
                        request.originating_history_id,
                        epoch,
                    ],
                )?;
                if operation_changed != 1 {
                    return Err(AppError::Database(
                        "branch-update operation changed during target lease handoff".to_string(),
                    ));
                }
                Ok(GitAuthorityCasOutcome::Applied {
                    fencing_epoch: next_epoch,
                })
            })
            .await
    }

    async fn pause_operation(
        &self,
        request: PauseBranchUpdate,
    ) -> AppResult<BranchUpdateCasOutcome> {
        self.db
            .run_transaction(move |conn| {
                let now = Utc::now().to_rfc3339();
                let epoch = i64::try_from(request.fencing_epoch)
                    .map_err(|_| AppError::Database("target lease epoch overflow".to_string()))?;
                let changed = conn.execute(
                    "UPDATE branch_update_operations
                     SET conversation_id = NULL, agent_run_id = NULL, updated_at = ?1
                     WHERE id = ?2 AND task_id = ?3 AND originating_history_id = ?4
                       AND settled_at IS NULL AND target_lease_epoch = ?5
                       AND EXISTS (
                         SELECT 1 FROM git_target_leases lease
                         WHERE lease.git_common_dir = branch_update_operations.git_common_dir
                           AND lease.target_ref = branch_update_operations.target_ref
                           AND lease.owner_kind = ?6 AND lease.owner_id = ?7
                           AND lease.fencing_epoch = ?5 AND lease.released_at IS NULL
                           AND lease.mutation_claim_id IS NULL
                       )
                       AND EXISTS (SELECT 1 FROM tasks WHERE id = ?3 AND internal_status = ?8)",
                    params![
                        now,
                        request.operation_id.as_str(),
                        request.task_id.as_str(),
                        request.originating_history_id,
                        epoch,
                        request.owner.kind.as_str(),
                        request.owner.owner_id,
                        request.update_status.as_str(),
                    ],
                )?;
                if changed != 1 {
                    return Ok(BranchUpdateCasOutcome::Stale);
                }
                if conn.execute(
                    "UPDATE tasks
                     SET internal_status = 'paused', metadata = COALESCE(?1, metadata), updated_at = ?2
                     WHERE id = ?3 AND internal_status = ?4",
                    params![
                        request.task_metadata,
                        now,
                        request.task_id.as_str(),
                        request.update_status.as_str()
                    ],
                )? != 1
                {
                    return Err(AppError::Database(
                        "task changed during branch-update pause".to_string(),
                    ));
                }
                conn.execute(
                    "INSERT INTO task_state_history (
                        id, task_id, from_status, to_status, changed_by, reason, metadata
                     ) VALUES (?1, ?2, ?3, 'paused', 'user',
                        'branch_update_paused', ?4)",
                    params![
                        request.history_id,
                        request.task_id.as_str(),
                        request.update_status.as_str(),
                        serde_json::json!({
                            "branch_update_operation_id": request.operation_id.as_str(),
                        })
                        .to_string(),
                    ],
                )?;
                Ok(BranchUpdateCasOutcome::Applied)
            })
            .await
    }

    async fn resume_operation(
        &self,
        request: ResumeBranchUpdate,
    ) -> AppResult<BranchUpdateCasOutcome> {
        let enforce_tasks_feature_policy = self.enforce_tasks_feature_policy;
        self.db
            .run_transaction(move |conn| {
                if enforce_tasks_feature_policy {
                    authorize_branch_update_progress(conn, &request.task_id)?;
                }
                let now = Utc::now().to_rfc3339();
                let epoch = i64::try_from(request.fencing_epoch)
                    .map_err(|_| AppError::Database("target lease epoch overflow".to_string()))?;
                let changed = conn.execute(
                    "UPDATE branch_update_operations SET updated_at = ?1
                     WHERE id = ?2 AND task_id = ?3 AND originating_history_id = ?4
                       AND settled_at IS NULL AND target_lease_epoch = ?5
                       AND EXISTS (
                         SELECT 1 FROM git_target_leases lease
                         WHERE lease.git_common_dir = branch_update_operations.git_common_dir
                           AND lease.target_ref = branch_update_operations.target_ref
                           AND lease.owner_kind = ?6 AND lease.owner_id = ?7
                           AND lease.fencing_epoch = ?5 AND lease.released_at IS NULL
                           AND lease.mutation_claim_id IS NULL
                       )
                       AND EXISTS (SELECT 1 FROM tasks WHERE id = ?3 AND internal_status = 'paused')",
                    params![
                        now,
                        request.operation_id.as_str(),
                        request.task_id.as_str(),
                        request.originating_history_id,
                        epoch,
                        request.owner.kind.as_str(),
                        request.owner.owner_id,
                    ],
                )?;
                if changed != 1 {
                    return Ok(BranchUpdateCasOutcome::Stale);
                }
                if conn.execute(
                    "UPDATE tasks SET internal_status = ?1, updated_at = ?2
                     WHERE id = ?3 AND internal_status = 'paused'",
                    params![request.update_status.as_str(), now, request.task_id.as_str()],
                )? != 1
                {
                    return Err(AppError::Database(
                        "task changed during branch-update resume".to_string(),
                    ));
                }
                conn.execute(
                    "INSERT INTO task_state_history (
                        id, task_id, from_status, to_status, changed_by, reason, metadata
                     ) VALUES (?1, ?2, 'paused', ?3, 'user',
                        'branch_update_resumed', ?4)",
                    params![
                        request.history_id,
                        request.task_id.as_str(),
                        request.update_status.as_str(),
                        serde_json::json!({
                            "branch_update_operation_id": request.operation_id.as_str(),
                        })
                        .to_string(),
                    ],
                )?;
                Ok(BranchUpdateCasOutcome::Applied)
            })
            .await
    }

    async fn stop_operation(&self, request: StopBranchUpdate) -> AppResult<BranchUpdateCasOutcome> {
        self.db
            .run_transaction(move |conn| {
                let now = Utc::now().to_rfc3339();
                let epoch = i64::try_from(request.fencing_epoch)
                    .map_err(|_| AppError::Database("target lease epoch overflow".to_string()))?;
                let changed = conn.execute(
                    "UPDATE branch_update_operations
                     SET phase = 'settled', capacity_ownership = 'released',
                         conversation_id = NULL, agent_run_id = NULL,
                         diagnostics = COALESCE(?1, diagnostics), settled_at = ?2, updated_at = ?2
                     WHERE id = ?3 AND task_id = ?4 AND originating_history_id = ?5
                       AND settled_at IS NULL AND target_lease_epoch = ?6
                       AND EXISTS (
                         SELECT 1 FROM git_target_leases lease
                         WHERE lease.git_common_dir = branch_update_operations.git_common_dir
                           AND lease.target_ref = branch_update_operations.target_ref
                           AND lease.owner_kind = ?7 AND lease.owner_id = ?8
                           AND lease.fencing_epoch = ?6 AND lease.released_at IS NULL
                           AND lease.mutation_claim_id IS NULL
                       )
                       AND EXISTS (SELECT 1 FROM tasks WHERE id = ?4 AND internal_status = ?9)",
                    params![
                        request.reason,
                        now,
                        request.operation_id.as_str(),
                        request.task_id.as_str(),
                        request.originating_history_id,
                        epoch,
                        request.owner.kind.as_str(),
                        request.owner.owner_id,
                        request.update_status.as_str(),
                    ],
                )?;
                if changed != 1 {
                    return Ok(BranchUpdateCasOutcome::Stale);
                }
                if conn.execute(
                    "UPDATE tasks SET internal_status = 'stopped', updated_at = ?1
                     WHERE id = ?2 AND internal_status = ?3",
                    params![
                        now,
                        request.task_id.as_str(),
                        request.update_status.as_str()
                    ],
                )? != 1
                {
                    return Err(AppError::Database(
                        "task changed during branch-update stop".to_string(),
                    ));
                }
                if conn.execute(
                    "UPDATE git_target_leases SET released_at = ?1, updated_at = ?1
                     WHERE git_common_dir = (
                         SELECT git_common_dir FROM branch_update_operations WHERE id = ?2
                     ) AND target_ref = (
                         SELECT target_ref FROM branch_update_operations WHERE id = ?2
                     ) AND owner_kind = ?3 AND owner_id = ?4
                       AND fencing_epoch = ?5 AND released_at IS NULL
                       AND mutation_claim_id IS NULL",
                    params![
                        now,
                        request.operation_id.as_str(),
                        request.owner.kind.as_str(),
                        request.owner.owner_id,
                        epoch,
                    ],
                )? != 1
                {
                    return Err(AppError::Database(
                        "target lease changed during branch-update stop".to_string(),
                    ));
                }
                conn.execute(
                    "INSERT INTO task_state_history (
                        id, task_id, from_status, to_status, changed_by, reason, metadata
                     ) VALUES (?1, ?2, ?3, 'stopped', 'user',
                        'branch_update_stopped', ?4)",
                    params![
                        request.history_id,
                        request.task_id.as_str(),
                        request.update_status.as_str(),
                        serde_json::json!({
                            "branch_update_operation_id": request.operation_id.as_str(),
                            "stop_reason": request.reason,
                        })
                        .to_string(),
                    ],
                )?;
                Ok(BranchUpdateCasOutcome::Applied)
            })
            .await
    }

    async fn retry_operation(
        &self,
        request: RetryBranchUpdate,
    ) -> AppResult<BranchUpdateCasOutcome> {
        let enforce_tasks_feature_policy = self.enforce_tasks_feature_policy;
        self.db
            .run_transaction(move |conn| {
                if enforce_tasks_feature_policy {
                    authorize_branch_update_progress(conn, &request.task_id)?;
                }
                let now = Utc::now().to_rfc3339();
                let epoch = i64::try_from(request.fencing_epoch)
                    .map_err(|_| AppError::Database("target lease epoch overflow".to_string()))?;
                let next_epoch = request.fencing_epoch.saturating_add(1);
                let next_epoch_db = i64::try_from(next_epoch)
                    .map_err(|_| AppError::Database("target lease epoch overflow".to_string()))?;
                let old_changed = conn.execute(
                    "UPDATE branch_update_operations SET phase = 'settled', settled_at = ?1,
                        updated_at = ?1
                     WHERE id = ?2 AND task_id = ?3 AND originating_history_id = ?4
                       AND phase = 'blocked' AND settled_at IS NULL AND target_lease_epoch = ?5
                       AND EXISTS (
                         SELECT 1 FROM git_target_leases lease
                         WHERE lease.git_common_dir = branch_update_operations.git_common_dir
                           AND lease.target_ref = branch_update_operations.target_ref
                           AND lease.owner_kind = ?6 AND lease.owner_id = ?7
                           AND lease.fencing_epoch = ?5 AND lease.released_at IS NULL
                           AND lease.mutation_claim_id IS NULL
                       )
                       AND EXISTS (
                         SELECT 1 FROM tasks WHERE id = ?3
                           AND internal_status = 'branch_update_blocked'
                       )",
                    params![
                        now,
                        request.operation_id.as_str(),
                        request.task_id.as_str(),
                        request.originating_history_id,
                        epoch,
                        request.owner.kind.as_str(),
                        request.owner.owner_id,
                    ],
                )?;
                if old_changed != 1 {
                    return Ok(BranchUpdateCasOutcome::Stale);
                }
                conn.execute(
                    "INSERT INTO task_state_history (
                        id, task_id, from_status, to_status, changed_by, reason, metadata
                     ) VALUES (?1, ?2, 'branch_update_blocked', ?3, 'user',
                        'branch_update_retry', ?4)",
                    params![
                        request.history_id,
                        request.task_id.as_str(),
                        request.update_status.as_str(),
                        serde_json::json!({
                            "previous_branch_update_operation_id": request.operation_id.as_str(),
                            "branch_update_operation_id": request.new_operation_id.as_str(),
                        })
                        .to_string(),
                    ],
                )?;
                let inserted = conn.execute(
                    "INSERT INTO branch_update_operations (
                        id, task_id, direction, phase, continuation, originating_history_id,
                        attempt_id, source_branch, target_branch, observed_source_sha,
                        observed_target_sha, resulting_sha, workspace_ownership, workspace_path,
                        capacity_ownership, failure_kind, conflict_files_json, diagnostics,
                        conversation_id, agent_run_id, continuation_claim_id,
                        continuation_idempotency_key, continuation_receipt, git_common_dir,
                        target_ref, target_identity_version, target_lease_epoch, retry_count,
                        created_at, updated_at, settled_at
                     ) SELECT
                        ?1, task_id, direction,
                        CASE WHEN conflict_files_json = '[]' THEN 'programmatic' ELSE 'resolving' END,
                        continuation, ?2, attempt_id, source_branch, target_branch,
                        observed_source_sha, observed_target_sha, NULL, workspace_ownership,
                        workspace_path, capacity_ownership, NULL, conflict_files_json, NULL,
                        NULL, NULL, NULL, NULL, NULL, git_common_dir, target_ref,
                        target_identity_version, ?3, retry_count + 1, ?4, ?4, NULL
                     FROM branch_update_operations WHERE id = ?5 AND task_id = ?6
                       AND phase = 'settled' AND settled_at = ?4",
                    params![
                        request.new_operation_id.as_str(),
                        request.history_id,
                        next_epoch_db,
                        now,
                        request.operation_id.as_str(),
                        request.task_id.as_str(),
                    ],
                )?;
                if inserted != 1 {
                    return Err(AppError::Database(
                        "failed to create immutable branch-update retry operation".to_string(),
                    ));
                }
                let next_owner = GitTargetLeaseOwner::branch_update(
                    request.task_id.as_str(),
                    request.new_operation_id.as_str(),
                );
                if conn.execute(
                    "UPDATE git_target_leases SET owner_kind = ?1, owner_task_id = ?2,
                        owner_id = ?3, fencing_epoch = ?4, acquired_at = ?5,
                        recovery_state = 'ready', updated_at = ?5
                     WHERE git_common_dir = (
                         SELECT git_common_dir FROM branch_update_operations WHERE id = ?6
                     ) AND target_ref = (
                         SELECT target_ref FROM branch_update_operations WHERE id = ?6
                     ) AND owner_kind = ?7 AND owner_id = ?8 AND fencing_epoch = ?9
                       AND released_at IS NULL AND mutation_claim_id IS NULL",
                    params![
                        next_owner.kind.as_str(),
                        next_owner.task_id,
                        next_owner.owner_id,
                        next_epoch_db,
                        now,
                        request.new_operation_id.as_str(),
                        request.owner.kind.as_str(),
                        request.owner.owner_id,
                        epoch,
                    ],
                )? != 1
                {
                    return Err(AppError::Database(
                        "target lease changed during branch-update retry".to_string(),
                    ));
                }
                if conn.execute(
                    "UPDATE tasks SET internal_status = ?1, updated_at = ?2
                     WHERE id = ?3 AND internal_status = 'branch_update_blocked'",
                    params![request.update_status.as_str(), now, request.task_id.as_str()],
                )? != 1
                {
                    return Err(AppError::Database(
                        "task changed during branch-update retry".to_string(),
                    ));
                }
                write_branch_update_task_metadata(
                    conn,
                    &request.task_id,
                    Some(serde_json::json!({
                        "operation_id": request.new_operation_id.as_str(),
                        "phase": "retrying",
                    })),
                )?;
                Ok(BranchUpdateCasOutcome::Applied)
            })
            .await
    }

    async fn release_target_lease(
        &self,
        identity: &GitTargetIdentity,
        owner: &GitTargetLeaseOwner,
        fencing_epoch: u64,
    ) -> AppResult<GitAuthorityCasOutcome> {
        let identity = identity.clone();
        let owner = owner.clone();
        self.db
            .run_transaction(move |conn| {
                let current = load_lease_authority(conn, &identity)?;
                if let Some(outcome) = classify_authority(current.as_ref(), &owner, fencing_epoch) {
                    return Ok(outcome);
                }
                if current
                    .as_ref()
                    .and_then(|row| row.claim_id.as_ref())
                    .is_some()
                {
                    return Ok(GitAuthorityCasOutcome::MutationInFlight);
                }
                conn.execute(
                    "UPDATE git_target_leases SET released_at = ?1,
                        updated_at = ?1 WHERE git_common_dir = ?2 AND target_ref = ?3",
                    params![
                        Utc::now().to_rfc3339(),
                        identity.git_common_dir().to_string_lossy(),
                        identity.full_ref(),
                    ],
                )?;
                Ok(GitAuthorityCasOutcome::Applied { fencing_epoch })
            })
            .await
    }

    async fn list_in_flight_mutations(&self) -> AppResult<Vec<GitMutationClaim>> {
        self.db
            .run(move |conn| {
                let mut statement = conn.prepare(
                    "SELECT git_common_dir, target_ref, owner_kind, owner_task_id, owner_id,
                            mutation_claim_id, mutation_kind, fencing_epoch,
                            mutation_process_group_id, mutation_started_at
                     FROM git_target_leases WHERE mutation_claim_id IS NOT NULL",
                )?;
                let rows = statement.query_and_then([], rows::mutation_claim_from_row)?;
                rows.collect()
            })
            .await
    }
}
