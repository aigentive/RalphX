use chrono::{DateTime, Utc};
use rusqlite::Row;
use std::path::PathBuf;
use std::str::FromStr;

use crate::domain::entities::{
    BranchUpdateOperation, BranchUpdateOperationId, GitMutationClaim, GitMutationKind,
    GitTargetIdentity, GitTargetLease, GitTargetLeaseOwner, GitTargetLeaseOwnerKind, TaskId,
};
use crate::error::{AppError, AppResult};

pub(super) const OPERATION_COLUMNS: &str = "
    id, task_id, direction, phase, continuation, originating_history_id,
    source_branch, target_branch, observed_source_sha, observed_target_sha,
    resulting_sha, workspace_ownership, workspace_path, capacity_ownership,
    failure_kind, conflict_files_json, diagnostics, conversation_id, agent_run_id,
    continuation_claim_id, continuation_idempotency_key, continuation_receipt,
    git_common_dir, target_ref, target_lease_epoch, retry_count, created_at,
    updated_at, settled_at";

pub(super) const LEASE_COLUMNS: &str = "
    git_common_dir, target_ref, owner_kind, owner_task_id, owner_id, fencing_epoch,
    acquired_at, mutation_claim_id, mutation_kind, mutation_process_group_id,
    mutation_started_at, released_at";

fn parse_time(value: String, field: &str) -> AppResult<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(&value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|error| AppError::Database(format!("invalid {field}: {error}")))
}

fn parse_optional_time(value: Option<String>, field: &str) -> AppResult<Option<DateTime<Utc>>> {
    value.map(|value| parse_time(value, field)).transpose()
}

fn parse_enum<T>(value: String, field: &str) -> AppResult<T>
where
    T: FromStr,
    T::Err: std::fmt::Display,
{
    value
        .parse()
        .map_err(|error| AppError::Database(format!("invalid {field}: {error}")))
}

pub(super) fn operation_from_row(row: &Row<'_>) -> AppResult<BranchUpdateOperation> {
    let failure_kind = row
        .get::<_, Option<String>>(14)?
        .map(|value| parse_enum(value, "branch update failure kind"))
        .transpose()?;
    let conflict_files_json: String = row.get(15)?;
    let conflict_files: Vec<PathBuf> = serde_json::from_str(&conflict_files_json)
        .map_err(|error| AppError::Database(format!("invalid conflict files: {error}")))?;
    let common_dir: String = row.get(22)?;
    let target_ref: String = row.get(23)?;

    Ok(BranchUpdateOperation {
        id: BranchUpdateOperationId::from_string(row.get::<_, String>(0)?),
        task_id: TaskId::from_string(row.get(1)?),
        direction: parse_enum(row.get(2)?, "branch update direction")?,
        phase: parse_enum(row.get(3)?, "branch update phase")?,
        continuation: parse_enum(row.get(4)?, "branch update continuation")?,
        originating_history_id: row.get(5)?,
        source_branch: row.get(6)?,
        target_branch: row.get(7)?,
        observed_source_sha: row.get(8)?,
        observed_target_sha: row.get(9)?,
        resulting_sha: row.get(10)?,
        workspace_ownership: parse_enum(row.get(11)?, "workspace ownership")?,
        workspace_path: row.get::<_, Option<String>>(12)?.map(PathBuf::from),
        capacity_ownership: parse_enum(row.get(13)?, "capacity ownership")?,
        failure_kind,
        conflict_files,
        diagnostics: row.get(16)?,
        conversation_id: row.get(17)?,
        agent_run_id: row.get(18)?,
        continuation_claim_id: row.get(19)?,
        continuation_idempotency_key: row.get(20)?,
        continuation_receipt: row.get(21)?,
        target_identity: GitTargetIdentity::new(PathBuf::from(common_dir), target_ref)
            .map_err(|error| AppError::Database(error.to_string()))?,
        target_lease_epoch: row
            .get::<_, i64>(24)?
            .try_into()
            .map_err(|_| AppError::Database("negative target lease fencing epoch".to_string()))?,
        retry_count: row
            .get::<_, i64>(25)?
            .try_into()
            .map_err(|_| AppError::Database("invalid retry count".to_string()))?,
        created_at: parse_time(row.get(26)?, "branch update created_at")?,
        updated_at: parse_time(row.get(27)?, "branch update updated_at")?,
        settled_at: parse_optional_time(row.get(28)?, "branch update settled_at")?,
    })
}

pub(super) fn lease_from_row(row: &Row<'_>) -> AppResult<GitTargetLease> {
    let identity = GitTargetIdentity::new(
        PathBuf::from(row.get::<_, String>(0)?),
        row.get::<_, String>(1)?,
    )
    .map_err(|error| AppError::Database(error.to_string()))?;
    let owner = GitTargetLeaseOwner {
        kind: parse_enum(row.get(2)?, "target lease owner kind")?,
        task_id: row.get(3)?,
        owner_id: row.get(4)?,
    };
    let fencing_epoch: u64 = row
        .get::<_, i64>(5)?
        .try_into()
        .map_err(|_| AppError::Database("negative target lease fencing epoch".to_string()))?;
    let acquired_at = parse_time(row.get(6)?, "target lease acquired_at")?;
    let mutation_claim_id: Option<String> = row.get(7)?;
    let active_mutation = match mutation_claim_id {
        Some(claim_id) => Some(GitMutationClaim {
            identity: identity.clone(),
            claim_id,
            kind: parse_enum(row.get::<_, String>(8)?, "git mutation kind")?,
            owner: owner.clone(),
            fencing_epoch,
            process_group_id: row.get(9)?,
            started_at: parse_time(row.get(10)?, "mutation started_at")?,
        }),
        None => None,
    };
    let released_at = parse_optional_time(row.get(11)?, "target lease released_at")?;
    Ok(GitTargetLease::from_persisted(
        identity,
        owner,
        fencing_epoch,
        acquired_at,
        active_mutation,
        released_at,
    ))
}

pub(super) fn mutation_claim_from_row(row: &Row<'_>) -> AppResult<GitMutationClaim> {
    let identity = GitTargetIdentity::new(
        PathBuf::from(row.get::<_, String>(0)?),
        row.get::<_, String>(1)?,
    )
    .map_err(|error| AppError::Database(error.to_string()))?;
    let owner = GitTargetLeaseOwner {
        kind: parse_enum::<GitTargetLeaseOwnerKind>(row.get(2)?, "target lease owner kind")?,
        task_id: row.get(3)?,
        owner_id: row.get(4)?,
    };
    Ok(GitMutationClaim {
        identity,
        claim_id: row.get(5)?,
        kind: parse_enum::<GitMutationKind>(row.get(6)?, "git mutation kind")?,
        owner,
        fencing_epoch: row
            .get::<_, i64>(7)?
            .try_into()
            .map_err(|_| AppError::Database("negative target lease fencing epoch".to_string()))?,
        process_group_id: row.get(8)?,
        started_at: parse_time(row.get(9)?, "mutation started_at")?,
    })
}
