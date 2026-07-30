use super::*;

use crate::domain::entities::{
    AgentWorkspaceRepairAttempt, AgentWorkspaceRepairAttemptId, AgentWorkspaceRepairContinuation,
    AgentWorkspaceRepairEffect, AgentWorkspaceRepairEffectId, AgentWorkspaceRepairEffectStatus,
    AgentWorkspaceRepairOutcome, AgentWorkspaceRepairPhase, AgentWorkspaceRepairSource,
};
use crate::domain::repositories::{
    AgentWorkspaceRepairAttemptTransition, AgentWorkspaceRepairAttemptTransitionOutcome,
    AgentWorkspaceRepairCompatibilityProjection, AgentWorkspaceRepairRepository,
    BindAgentWorkspaceRepairAttemptRun, CompleteAgentWorkspaceRepairEffect,
    CompleteAgentWorkspaceRepairEffectOutcome, CreateAgentWorkspaceRepairEffect,
    CreateAgentWorkspaceRepairEffectOutcome, ImportLegacyAgentWorkspaceRepairAttempt,
    ImportLegacyAgentWorkspaceRepairAttemptOutcome, SettleAgentWorkspaceRepairAttempt,
    SettleAgentWorkspaceRepairAttemptOutcome, SettleAndStartAgentWorkspaceRepairSuccessor,
    SettleAndStartAgentWorkspaceRepairSuccessorOutcome, StartOrJoinAgentWorkspaceRepairAttempt,
    StartOrJoinAgentWorkspaceRepairAttemptOutcome,
};

fn invalid_repair_row_value(
    column: usize,
    value_type: rusqlite::types::Type,
    message: impl std::fmt::Display,
) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        column,
        value_type,
        Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            message.to_string(),
        )),
    )
}

fn row_to_repair_attempt(row: &rusqlite::Row<'_>) -> rusqlite::Result<AgentWorkspaceRepairAttempt> {
    let source: String = row.get("source")?;
    let phase: String = row.get("phase")?;
    let continuation: String = row.get("continuation")?;
    let outcome: Option<String> = row.get("outcome")?;
    let pending_reasons_json: String = row.get("pending_reasons_json")?;
    let created_at: String = row.get("created_at")?;
    let updated_at: String = row.get("updated_at")?;

    Ok(AgentWorkspaceRepairAttempt {
        id: AgentWorkspaceRepairAttemptId::from_string(row.get::<_, String>("id")?),
        conversation_id: ChatConversationId::from_string(row.get::<_, String>("conversation_id")?),
        generation: u64::try_from(row.get::<_, i64>("generation")?)
            .map_err(|error| invalid_repair_row_value(2, rusqlite::types::Type::Integer, error))?,
        source: AgentWorkspaceRepairSource::from_str(&source)
            .map_err(|error| invalid_repair_row_value(3, rusqlite::types::Type::Text, error))?,
        phase: AgentWorkspaceRepairPhase::from_str(&phase)
            .map_err(|error| invalid_repair_row_value(4, rusqlite::types::Type::Text, error))?,
        continuation: AgentWorkspaceRepairContinuation::from_str(&continuation)
            .map_err(|error| invalid_repair_row_value(5, rusqlite::types::Type::Text, error))?,
        reserved_agent_run_id: row
            .get::<_, Option<String>>("reserved_agent_run_id")?
            .map(crate::domain::entities::AgentRunId::from_string),
        target_base_ref: row.get("target_base_ref")?,
        target_base_commit: row.get("target_base_commit")?,
        pending_reasons: serde_json::from_str(&pending_reasons_json)
            .map_err(|error| invalid_repair_row_value(9, rusqlite::types::Type::Text, error))?,
        review_required: row.get("review_required")?,
        auto_publish_enabled: row.get("auto_publish_enabled")?,
        auto_merge_desired: row.get("auto_merge_desired")?,
        auto_merge_method: row.get("auto_merge_method")?,
        dispatch_count: u32::try_from(row.get::<_, i64>("dispatch_count")?)
            .map_err(|error| invalid_repair_row_value(15, rusqlite::types::Type::Integer, error))?,
        next_dispatch_at: row
            .get::<_, Option<String>>("next_dispatch_at")?
            .map(|value| parse_datetime(&value)),
        ci_rerun_count: u32::try_from(row.get::<_, i64>("ci_rerun_count")?)
            .map_err(|error| invalid_repair_row_value(16, rusqlite::types::Type::Integer, error))?,
        ci_rerun_fingerprint: row.get("ci_rerun_fingerprint")?,
        repair_head_commit: row.get("repair_head_commit")?,
        summary: row.get("summary")?,
        blocker: row.get("blocker")?,
        git_common_dir: row.get("git_common_dir")?,
        target_ref: row.get("target_ref")?,
        target_identity_version: row
            .get::<_, Option<i64>>("target_identity_version")?
            .map(u64::try_from)
            .transpose()
            .map_err(|error| invalid_repair_row_value(21, rusqlite::types::Type::Integer, error))?,
        target_lease_epoch: row
            .get::<_, Option<i64>>("target_lease_epoch")?
            .map(u64::try_from)
            .transpose()
            .map_err(|error| invalid_repair_row_value(22, rusqlite::types::Type::Integer, error))?,
        outcome: outcome
            .as_deref()
            .map(AgentWorkspaceRepairOutcome::from_str)
            .transpose()
            .map_err(|error| invalid_repair_row_value(23, rusqlite::types::Type::Text, error))?,
        created_at: parse_datetime(&created_at),
        updated_at: parse_datetime(&updated_at),
        settled_at: row
            .get::<_, Option<String>>("settled_at")?
            .map(|value| parse_datetime(&value)),
    })
}

fn row_to_repair_effect(row: &rusqlite::Row<'_>) -> rusqlite::Result<AgentWorkspaceRepairEffect> {
    let kind: String = row.get("kind")?;
    let status: String = row.get("status")?;
    let created_at: String = row.get("created_at")?;
    let updated_at: String = row.get("updated_at")?;

    Ok(AgentWorkspaceRepairEffect {
        id: AgentWorkspaceRepairEffectId::from_string(row.get::<_, String>("id")?),
        attempt_id: AgentWorkspaceRepairAttemptId::from_string(row.get::<_, String>("attempt_id")?),
        kind: crate::domain::entities::AgentWorkspaceRepairEffectKind::from_str(&kind)
            .map_err(|error| invalid_repair_row_value(2, rusqlite::types::Type::Text, error))?,
        status: AgentWorkspaceRepairEffectStatus::from_str(&status)
            .map_err(|error| invalid_repair_row_value(3, rusqlite::types::Type::Text, error))?,
        idempotency_key: row.get("idempotency_key")?,
        intended_head_oid: row.get("intended_head_oid")?,
        expected_remote_oid: row.get("expected_remote_oid")?,
        expected_pr_number: row.get("expected_pr_number")?,
        expected_remote_absent: row.get("expected_remote_absent")?,
        receipt_json: row.get("receipt_json")?,
        last_error: row.get("last_error")?,
        created_at: parse_datetime(&created_at),
        updated_at: parse_datetime(&updated_at),
        completed_at: row
            .get::<_, Option<String>>("completed_at")?
            .map(|value| parse_datetime(&value)),
    })
}

fn validate_repair_events(
    conversation_id: &ChatConversationId,
    events: &[AgentConversationWorkspacePublicationEvent],
) -> AppResult<()> {
    if events
        .iter()
        .any(|event| event.conversation_id != *conversation_id)
    {
        return Err(AppError::Validation(
            "repair attempt events must belong to the repaired workspace".to_string(),
        ));
    }
    Ok(())
}

fn load_repair_attempt(
    conn: &Connection,
    attempt_id: &str,
) -> AppResult<Option<AgentWorkspaceRepairAttempt>> {
    conn.query_row(
        "SELECT * FROM agent_workspace_repair_attempts WHERE id = ?1",
        rusqlite::params![attempt_id],
        row_to_repair_attempt,
    )
    .optional()
    .map_err(Into::into)
}

fn load_current_repair_attempt(
    conn: &Connection,
    conversation_id: &str,
) -> AppResult<Option<AgentWorkspaceRepairAttempt>> {
    conn.query_row(
        "SELECT * FROM agent_workspace_repair_attempts
         WHERE conversation_id = ?1 AND settled_at IS NULL
         LIMIT 1",
        rusqlite::params![conversation_id],
        row_to_repair_attempt,
    )
    .optional()
    .map_err(Into::into)
}

fn load_latest_repair_attempt(
    conn: &Connection,
    conversation_id: &str,
) -> AppResult<Option<AgentWorkspaceRepairAttempt>> {
    conn.query_row(
        "SELECT * FROM agent_workspace_repair_attempts
         WHERE conversation_id = ?1
         ORDER BY generation DESC
         LIMIT 1",
        rusqlite::params![conversation_id],
        row_to_repair_attempt,
    )
    .optional()
    .map_err(Into::into)
}

fn write_repair_attempt(conn: &Connection, attempt: &AgentWorkspaceRepairAttempt) -> AppResult<()> {
    conn.execute(
        "INSERT INTO agent_workspace_repair_attempts (
            id, conversation_id, generation, source, phase, continuation,
            reserved_agent_run_id, target_base_ref, target_base_commit, pending_reasons_json,
            review_required, auto_publish_enabled, auto_merge_desired, auto_merge_method,
            dispatch_count, next_dispatch_at, ci_rerun_count, ci_rerun_fingerprint,
            repair_head_commit, summary, blocker,
            git_common_dir, target_ref, target_identity_version, target_lease_epoch, outcome,
            created_at, updated_at, settled_at
         ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
            ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29
         )",
        rusqlite::params![
            attempt.id.as_str(),
            attempt.conversation_id.as_str(),
            i64::try_from(attempt.generation).map_err(|_| {
                AppError::Validation("repair attempt generation exceeds SQLite range".to_string())
            })?,
            attempt.source.to_string(),
            attempt.phase.to_string(),
            attempt.continuation.to_string(),
            attempt.reserved_agent_run_id.as_ref().map(|id| id.as_str()),
            attempt.target_base_ref,
            attempt.target_base_commit,
            serde_json::to_string(&attempt.pending_reasons)
                .map_err(|error| AppError::Validation(error.to_string()))?,
            attempt.review_required,
            attempt.auto_publish_enabled,
            attempt.auto_merge_desired,
            attempt.auto_merge_method,
            i64::from(attempt.dispatch_count),
            attempt.next_dispatch_at.map(|value| value.to_rfc3339()),
            i64::from(attempt.ci_rerun_count),
            attempt.ci_rerun_fingerprint,
            attempt.repair_head_commit,
            attempt.summary,
            attempt.blocker,
            attempt.git_common_dir,
            attempt.target_ref,
            attempt
                .target_identity_version
                .map(i64::try_from)
                .transpose()
                .map_err(|_| {
                    AppError::Validation(
                        "repair target identity version exceeds SQLite range".to_string(),
                    )
                })?,
            attempt
                .target_lease_epoch
                .map(i64::try_from)
                .transpose()
                .map_err(|_| {
                    AppError::Validation(
                        "repair target lease epoch exceeds SQLite range".to_string(),
                    )
                })?,
            attempt.outcome.map(|outcome| outcome.to_string()),
            attempt.created_at.to_rfc3339(),
            attempt.updated_at.to_rfc3339(),
            attempt.settled_at.map(|value| value.to_rfc3339()),
        ],
    )?;
    Ok(())
}

fn update_repair_attempt(
    conn: &Connection,
    attempt: &AgentWorkspaceRepairAttempt,
    expected_phase: AgentWorkspaceRepairPhase,
    expected_updated_at: Option<DateTime<Utc>>,
    require_unsettled: bool,
) -> AppResult<bool> {
    let rows = conn.execute(
        "UPDATE agent_workspace_repair_attempts
         SET phase = ?4,
             continuation = ?5,
             reserved_agent_run_id = ?6,
             target_base_ref = ?7,
             target_base_commit = ?8,
             pending_reasons_json = ?9,
             review_required = ?10,
             auto_publish_enabled = ?11,
             auto_merge_desired = ?12,
             auto_merge_method = ?13,
             dispatch_count = ?14,
             next_dispatch_at = ?15,
             ci_rerun_count = ?16,
             ci_rerun_fingerprint = ?17,
             repair_head_commit = ?18,
             summary = ?19,
             blocker = ?20,
             git_common_dir = ?21,
             target_ref = ?22,
             target_identity_version = ?23,
             target_lease_epoch = ?24,
             outcome = ?25,
             updated_at = ?26,
             settled_at = ?27
         WHERE id = ?1 AND generation = ?2 AND phase = ?3
           AND (?28 IS NULL OR updated_at = ?28)
           AND (?29 = 0 OR settled_at IS NULL)",
        rusqlite::params![
            attempt.id.as_str(),
            i64::try_from(attempt.generation).map_err(|_| {
                AppError::Validation("repair attempt generation exceeds SQLite range".to_string())
            })?,
            expected_phase.to_string(),
            attempt.phase.to_string(),
            attempt.continuation.to_string(),
            attempt.reserved_agent_run_id.as_ref().map(|id| id.as_str()),
            attempt.target_base_ref,
            attempt.target_base_commit,
            serde_json::to_string(&attempt.pending_reasons)
                .map_err(|error| AppError::Validation(error.to_string()))?,
            attempt.review_required,
            attempt.auto_publish_enabled,
            attempt.auto_merge_desired,
            attempt.auto_merge_method,
            i64::from(attempt.dispatch_count),
            attempt.next_dispatch_at.map(|value| value.to_rfc3339()),
            i64::from(attempt.ci_rerun_count),
            attempt.ci_rerun_fingerprint,
            attempt.repair_head_commit,
            attempt.summary,
            attempt.blocker,
            attempt.git_common_dir,
            attempt.target_ref,
            attempt
                .target_identity_version
                .map(i64::try_from)
                .transpose()
                .map_err(|_| {
                    AppError::Validation(
                        "repair target identity version exceeds SQLite range".to_string(),
                    )
                })?,
            attempt
                .target_lease_epoch
                .map(i64::try_from)
                .transpose()
                .map_err(|_| {
                    AppError::Validation(
                        "repair target lease epoch exceeds SQLite range".to_string(),
                    )
                })?,
            attempt.outcome.map(|outcome| outcome.to_string()),
            attempt.updated_at.to_rfc3339(),
            attempt.settled_at.map(|value| value.to_rfc3339()),
            expected_updated_at.map(|value| value.to_rfc3339()),
            if require_unsettled { 1_i64 } else { 0_i64 },
        ],
    )?;
    Ok(rows == 1)
}

fn apply_compatibility_projection(
    conn: &Connection,
    conversation_id: &ChatConversationId,
    projection: Option<&AgentWorkspaceRepairCompatibilityProjection>,
    updated_at: DateTime<Utc>,
) -> AppResult<()> {
    let Some(projection) = projection else {
        return Ok(());
    };

    let rows = conn.execute(
        "UPDATE agent_conversation_workspaces
         SET publication_push_status = ?2,
             pr_supervision_status = ?3,
             pr_supervision_summary = ?4,
             pr_supervision_updated_at = ?5,
             pr_auto_merge_current = ?6,
             base_commit = ?7,
             updated_at = ?8
         WHERE conversation_id = ?1",
        rusqlite::params![
            conversation_id.as_str(),
            projection.publication_push_status,
            projection.pr_supervision_status,
            projection.pr_supervision_summary,
            projection
                .pr_supervision_updated_at
                .map(|value| value.to_rfc3339()),
            projection.pr_auto_merge_current,
            projection.base_commit,
            updated_at.to_rfc3339(),
        ],
    )?;
    if rows != 1 {
        return Err(AppError::NotFound(format!(
            "workspace {} for repair attempt",
            conversation_id.as_str()
        )));
    }
    Ok(())
}

fn append_repair_events(
    conn: &Connection,
    events: &[AgentConversationWorkspacePublicationEvent],
) -> AppResult<()> {
    for event in events {
        conn.execute(
            "INSERT INTO agent_conversation_workspace_publication_events (
                id, conversation_id, step, status, summary, classification, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                event.id,
                event.conversation_id.as_str(),
                event.step,
                event.status,
                event.summary,
                event.classification,
                event.created_at.to_rfc3339(),
            ],
        )?;
    }
    Ok(())
}

fn next_repair_generation(
    conn: &Connection,
    conversation_id: &ChatConversationId,
) -> AppResult<u64> {
    let max_generation = conn.query_row(
        "SELECT COALESCE(MAX(generation), 0) FROM agent_workspace_repair_attempts
         WHERE conversation_id = ?1",
        rusqlite::params![conversation_id.as_str()],
        |row| row.get::<_, i64>(0),
    )?;
    u64::try_from(max_generation)
        .ok()
        .and_then(|generation| generation.checked_add(1))
        .ok_or_else(|| AppError::Validation("repair attempt generation overflow".to_string()))
}

fn validate_repair_effect_shape(effect: &AgentWorkspaceRepairEffect) -> AppResult<()> {
    if effect.expected_remote_absent && effect.expected_remote_oid.is_some() {
        return Err(AppError::Validation(
            "repair effects cannot expect both a missing and concrete remote ref".to_string(),
        ));
    }
    match effect.status {
        AgentWorkspaceRepairEffectStatus::Observed => {
            let completed_at = effect.completed_at.ok_or_else(|| {
                AppError::Validation(
                    "observed repair effects require a completion time".to_string(),
                )
            })?;
            effect
                .can_complete_observed(effect.receipt_json.as_deref(), completed_at)
                .map_err(AppError::Validation)
        }
        AgentWorkspaceRepairEffectStatus::Pending
        | AgentWorkspaceRepairEffectStatus::InFlight
        | AgentWorkspaceRepairEffectStatus::Failed => {
            if effect.completed_at.is_some() {
                return Err(AppError::Validation(
                    "only observed repair effects may have a completion time".to_string(),
                ));
            }
            Ok(())
        }
    }
}

fn write_repair_effect(conn: &Connection, effect: &AgentWorkspaceRepairEffect) -> AppResult<()> {
    conn.execute(
        "INSERT INTO agent_workspace_repair_effects (
            id, attempt_id, kind, status, idempotency_key, intended_head_oid,
            expected_remote_oid, expected_pr_number, expected_remote_absent, receipt_json,
            last_error, created_at, updated_at, completed_at
         ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14
         )",
        rusqlite::params![
            effect.id.as_str(),
            effect.attempt_id.as_str(),
            effect.kind.to_string(),
            effect.status.to_string(),
            effect.idempotency_key,
            effect.intended_head_oid,
            effect.expected_remote_oid,
            effect.expected_pr_number,
            effect.expected_remote_absent,
            effect.receipt_json,
            effect.last_error,
            effect.created_at.to_rfc3339(),
            effect.updated_at.to_rfc3339(),
            effect.completed_at.map(|value| value.to_rfc3339()),
        ],
    )?;
    Ok(())
}

fn update_repair_effect(
    conn: &Connection,
    effect: &AgentWorkspaceRepairEffect,
    expected_updated_at: DateTime<Utc>,
    expected_status: AgentWorkspaceRepairEffectStatus,
) -> AppResult<bool> {
    let rows = conn.execute(
        "UPDATE agent_workspace_repair_effects
         SET kind = ?3,
             status = ?4,
             idempotency_key = ?5,
             intended_head_oid = ?6,
             expected_remote_oid = ?7,
             expected_pr_number = ?8,
             expected_remote_absent = ?9,
             receipt_json = ?10,
             last_error = ?11,
             updated_at = ?12,
             completed_at = ?13
         WHERE id = ?1 AND attempt_id = ?2 AND completed_at IS NULL
           AND updated_at = ?14 AND status = ?15",
        rusqlite::params![
            effect.id.as_str(),
            effect.attempt_id.as_str(),
            effect.kind.to_string(),
            effect.status.to_string(),
            effect.idempotency_key,
            effect.intended_head_oid,
            effect.expected_remote_oid,
            effect.expected_pr_number,
            effect.expected_remote_absent,
            effect.receipt_json,
            effect.last_error,
            effect.updated_at.to_rfc3339(),
            effect.completed_at.map(|value| value.to_rfc3339()),
            expected_updated_at.to_rfc3339(),
            expected_status.to_string(),
        ],
    )?;
    Ok(rows == 1)
}

#[async_trait]
impl AgentWorkspaceRepairRepository for SqliteAgentConversationWorkspaceRepository {
    async fn get_current_repair_attempt(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentWorkspaceRepairAttempt>> {
        let conversation_id = conversation_id.as_str().to_string();
        self.db
            .run(move |conn| load_current_repair_attempt(conn, &conversation_id))
            .await
    }

    async fn get_latest_repair_attempt_for_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentWorkspaceRepairAttempt>> {
        let conversation_id = conversation_id.as_str().to_string();
        self.db
            .run(move |conn| load_latest_repair_attempt(conn, &conversation_id))
            .await
    }

    async fn get_repair_attempt(
        &self,
        attempt_id: &AgentWorkspaceRepairAttemptId,
    ) -> AppResult<Option<AgentWorkspaceRepairAttempt>> {
        let attempt_id = attempt_id.as_str().to_string();
        self.db
            .run(move |conn| load_repair_attempt(conn, &attempt_id))
            .await
    }

    async fn get_repair_attempt_for_run(
        &self,
        conversation_id: &ChatConversationId,
        run_id: &crate::domain::entities::AgentRunId,
    ) -> AppResult<Option<AgentWorkspaceRepairAttempt>> {
        let conversation_id = conversation_id.as_str().to_string();
        let run_id = run_id.as_str().to_string();
        self.db
            .run(move |conn| {
                conn.query_row(
                    "SELECT * FROM agent_workspace_repair_attempts
                     WHERE conversation_id = ?1 AND reserved_agent_run_id = ?2",
                    rusqlite::params![conversation_id, run_id],
                    row_to_repair_attempt,
                )
                .optional()
                .map_err(Into::into)
            })
            .await
    }

    async fn list_recoverable_repair_attempts(
        &self,
    ) -> AppResult<Vec<AgentWorkspaceRepairAttempt>> {
        self.db
            .run(move |conn| {
                let mut statement = conn.prepare(
                    "SELECT * FROM agent_workspace_repair_attempts
                     WHERE settled_at IS NULL
                     ORDER BY updated_at ASC, rowid ASC",
                )?;
                let rows = statement.query_map([], row_to_repair_attempt)?;
                let mut attempts = Vec::new();
                for row in rows {
                    attempts.push(row?);
                }
                Ok(attempts)
            })
            .await
    }

    async fn start_or_join_repair_attempt(
        &self,
        request: StartOrJoinAgentWorkspaceRepairAttempt,
    ) -> AppResult<StartOrJoinAgentWorkspaceRepairAttemptOutcome> {
        validate_repair_events(&request.attempt.conversation_id, &request.events)?;
        let conversation_id = request.attempt.conversation_id.clone();
        let attempt = request.attempt;
        let reason = request.reason;
        let verified_newer_base = request.verified_newer_base;
        let compatibility_projection = request.compatibility_projection;
        let events = request.events;

        self.db
            .run_transaction(move |conn| {
                if let Some(mut current) =
                    load_current_repair_attempt(conn, &conversation_id.as_str())?
                {
                    if current.target_base_ref != attempt.target_base_ref {
                        return Ok(
                            StartOrJoinAgentWorkspaceRepairAttemptOutcome::BlockedByCurrent(
                                current,
                            ),
                        );
                    }

                    if !reason.trim().is_empty() && !current.pending_reasons.contains(&reason) {
                        current.pending_reasons.push(reason);
                    }
                    if attempt.continuation.priority() > current.continuation.priority() {
                        current.continuation = attempt.continuation;
                    }
                    if verified_newer_base
                        && attempt.target_base_commit.is_some()
                        && attempt.target_base_commit != current.target_base_commit
                    {
                        current.target_base_commit = attempt.target_base_commit.clone();
                    }
                    current.auto_publish_enabled = attempt.auto_publish_enabled;
                    current.auto_merge_desired = attempt.auto_merge_desired;
                    current.auto_merge_method = attempt.auto_merge_method.clone();
                    current.updated_at = attempt.updated_at;
                    let updated =
                        update_repair_attempt(conn, &current, current.phase, None, false)?;
                    debug_assert!(
                        updated,
                        "current repair attempt disappeared inside transaction"
                    );
                    return Ok(StartOrJoinAgentWorkspaceRepairAttemptOutcome::Joined(
                        current,
                    ));
                }

                let mut started = attempt;
                started.generation = next_repair_generation(conn, &conversation_id)?;
                if !reason.trim().is_empty() && !started.pending_reasons.contains(&reason) {
                    started.pending_reasons.push(reason);
                }
                write_repair_attempt(conn, &started)?;
                apply_compatibility_projection(
                    conn,
                    &conversation_id,
                    compatibility_projection.as_ref(),
                    started.updated_at,
                )?;
                append_repair_events(conn, &events)?;
                Ok(StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(
                    started,
                ))
            })
            .await
    }

    async fn bind_repair_attempt_run(
        &self,
        request: BindAgentWorkspaceRepairAttemptRun,
    ) -> AppResult<AgentWorkspaceRepairAttemptTransitionOutcome> {
        let attempt_id = request.attempt_id.clone();
        self.db
            .run_transaction(move |conn| {
                let Some(mut current) = load_repair_attempt(conn, attempt_id.as_str())? else {
                    return Ok(AgentWorkspaceRepairAttemptTransitionOutcome::Missing);
                };
                if current.settled_at.is_some()
                    || current.generation != request.generation
                    || current.phase != request.expected_phase
                    || current.updated_at != request.expected_updated_at
                {
                    return Ok(AgentWorkspaceRepairAttemptTransitionOutcome::Stale(current));
                }
                if current.reserved_agent_run_id.as_ref() == Some(&request.run_id) {
                    return Ok(AgentWorkspaceRepairAttemptTransitionOutcome::Applied(
                        current,
                    ));
                }
                if current.reserved_agent_run_id.is_some() {
                    return Ok(AgentWorkspaceRepairAttemptTransitionOutcome::Stale(current));
                }

                let rows = conn.execute(
                    "UPDATE agent_workspace_repair_attempts
                     SET reserved_agent_run_id = ?4, updated_at = ?5
                     WHERE id = ?1 AND generation = ?2 AND phase = ?3
                       AND updated_at = ?6 AND reserved_agent_run_id IS NULL
                       AND settled_at IS NULL",
                    rusqlite::params![
                        current.id.as_str(),
                        i64::try_from(request.generation).map_err(|_| {
                            AppError::Validation(
                                "repair attempt generation exceeds SQLite range".to_string(),
                            )
                        })?,
                        request.expected_phase.to_string(),
                        request.run_id.as_str(),
                        request.updated_at.to_rfc3339(),
                        request.expected_updated_at.to_rfc3339(),
                    ],
                )?;
                if rows != 1 {
                    let latest =
                        load_repair_attempt(conn, attempt_id.as_str())?.ok_or_else(|| {
                            AppError::NotFound(format!("repair attempt {attempt_id}"))
                        })?;
                    return Ok(AgentWorkspaceRepairAttemptTransitionOutcome::Stale(latest));
                }
                current.reserved_agent_run_id = Some(request.run_id);
                current.updated_at = request.updated_at;
                Ok(AgentWorkspaceRepairAttemptTransitionOutcome::Applied(
                    current,
                ))
            })
            .await
    }

    async fn transition_repair_attempt(
        &self,
        request: AgentWorkspaceRepairAttemptTransition,
    ) -> AppResult<AgentWorkspaceRepairAttemptTransitionOutcome> {
        validate_repair_events(&request.attempt.conversation_id, &request.events)?;
        let attempt_id = request.attempt.id.clone();
        self.db
            .run_transaction(move |conn| {
                let Some(current) = load_repair_attempt(conn, attempt_id.as_str())? else {
                    return Ok(AgentWorkspaceRepairAttemptTransitionOutcome::Missing);
                };
                if !request.matches_attempt(&current) {
                    return Ok(AgentWorkspaceRepairAttemptTransitionOutcome::Stale(current));
                }
                if !update_repair_attempt(
                    conn,
                    &request.attempt,
                    request.expected_phase,
                    Some(request.expected_updated_at),
                    true,
                )? {
                    let latest =
                        load_repair_attempt(conn, attempt_id.as_str())?.ok_or_else(|| {
                            AppError::NotFound(format!("repair attempt {attempt_id}"))
                        })?;
                    return Ok(AgentWorkspaceRepairAttemptTransitionOutcome::Stale(latest));
                }
                apply_compatibility_projection(
                    conn,
                    &request.attempt.conversation_id,
                    request.compatibility_projection.as_ref(),
                    request.attempt.updated_at,
                )?;
                append_repair_events(conn, &request.events)?;
                Ok(AgentWorkspaceRepairAttemptTransitionOutcome::Applied(
                    request.attempt,
                ))
            })
            .await
    }

    async fn settle_repair_attempt(
        &self,
        request: SettleAgentWorkspaceRepairAttempt,
    ) -> AppResult<SettleAgentWorkspaceRepairAttemptOutcome> {
        let attempt_id = request.attempt_id.clone();
        self.db
            .run_transaction(move |conn| {
                let Some(mut current) = load_repair_attempt(conn, attempt_id.as_str())? else {
                    return Ok(SettleAgentWorkspaceRepairAttemptOutcome::Missing);
                };
                if current.generation != request.generation
                    || current.phase != request.expected_phase
                    || current.updated_at != request.expected_updated_at
                    || current.settled_at.is_some()
                {
                    return Ok(SettleAgentWorkspaceRepairAttemptOutcome::Stale(current));
                }
                validate_repair_events(&current.conversation_id, &request.events)?;
                current.outcome = Some(request.outcome);
                current.settled_at = Some(request.settled_at);
                current.updated_at = request.settled_at;
                if !update_repair_attempt(
                    conn,
                    &current,
                    request.expected_phase,
                    Some(request.expected_updated_at),
                    true,
                )? {
                    let latest =
                        load_repair_attempt(conn, attempt_id.as_str())?.ok_or_else(|| {
                            AppError::NotFound(format!("repair attempt {attempt_id}"))
                        })?;
                    return Ok(SettleAgentWorkspaceRepairAttemptOutcome::Stale(latest));
                }
                apply_compatibility_projection(
                    conn,
                    &current.conversation_id,
                    request.compatibility_projection.as_ref(),
                    request.settled_at,
                )?;
                append_repair_events(conn, &request.events)?;
                Ok(SettleAgentWorkspaceRepairAttemptOutcome::Applied(current))
            })
            .await
    }

    async fn settle_and_start_repair_successor(
        &self,
        request: SettleAndStartAgentWorkspaceRepairSuccessor,
    ) -> AppResult<SettleAndStartAgentWorkspaceRepairSuccessorOutcome> {
        validate_repair_events(&request.successor.attempt.conversation_id, &request.events)?;
        validate_repair_events(
            &request.successor.attempt.conversation_id,
            &request.successor.events,
        )?;
        let attempt_id = request.attempt_id.clone();
        self.db
            .run_transaction(move |conn| {
                let Some(mut current) = load_repair_attempt(conn, attempt_id.as_str())? else {
                    return Ok(SettleAndStartAgentWorkspaceRepairSuccessorOutcome::Missing);
                };
                if current.generation != request.generation
                    || current.phase != request.expected_phase
                    || current.updated_at != request.expected_updated_at
                    || current.settled_at.is_some()
                {
                    return Ok(SettleAndStartAgentWorkspaceRepairSuccessorOutcome::Stale(
                        current,
                    ));
                }
                if current.conversation_id != request.successor.attempt.conversation_id {
                    return Err(AppError::Validation(
                        "repair successor must belong to the settled workspace".to_string(),
                    ));
                }

                current.outcome = Some(request.outcome);
                current.settled_at = Some(request.settled_at);
                current.updated_at = request.settled_at;
                if !update_repair_attempt(
                    conn,
                    &current,
                    request.expected_phase,
                    Some(request.expected_updated_at),
                    true,
                )? {
                    let latest =
                        load_repair_attempt(conn, attempt_id.as_str())?.ok_or_else(|| {
                            AppError::NotFound(format!("repair attempt {attempt_id}"))
                        })?;
                    return Ok(SettleAndStartAgentWorkspaceRepairSuccessorOutcome::Stale(
                        latest,
                    ));
                }

                let mut successor = request.successor.attempt;
                successor.generation = next_repair_generation(conn, &current.conversation_id)?;
                if !request.successor.reason.trim().is_empty()
                    && !successor
                        .pending_reasons
                        .contains(&request.successor.reason)
                {
                    successor.pending_reasons.push(request.successor.reason);
                }
                write_repair_attempt(conn, &successor)?;
                apply_compatibility_projection(
                    conn,
                    &current.conversation_id,
                    request.compatibility_projection.as_ref(),
                    request.settled_at,
                )?;
                apply_compatibility_projection(
                    conn,
                    &current.conversation_id,
                    request.successor.compatibility_projection.as_ref(),
                    successor.updated_at,
                )?;
                append_repair_events(conn, &request.events)?;
                append_repair_events(conn, &request.successor.events)?;
                Ok(SettleAndStartAgentWorkspaceRepairSuccessorOutcome::Started(
                    successor,
                ))
            })
            .await
    }

    async fn create_repair_effect(
        &self,
        request: CreateAgentWorkspaceRepairEffect,
    ) -> AppResult<CreateAgentWorkspaceRepairEffectOutcome> {
        validate_repair_effect_shape(&request.effect)?;
        let attempt_id = request.attempt_id.clone();
        self.db
            .run_transaction(move |conn| {
                let Some(current) = load_repair_attempt(conn, attempt_id.as_str())? else {
                    return Ok(CreateAgentWorkspaceRepairEffectOutcome::Missing);
                };
                validate_repair_events(&current.conversation_id, &request.events)?;
                if current.generation != request.generation
                    || current.phase != request.expected_phase
                    || current.updated_at != request.expected_attempt_updated_at
                    || current.settled_at.is_some()
                {
                    return Ok(CreateAgentWorkspaceRepairEffectOutcome::Stale(current));
                }
                if request.effect.attempt_id != current.id {
                    return Err(AppError::Validation(
                        "repair effect does not belong to its requested attempt".to_string(),
                    ));
                }
                let existing_open = conn
                    .query_row(
                        "SELECT * FROM agent_workspace_repair_effects
                         WHERE attempt_id = ?1 AND completed_at IS NULL
                         LIMIT 1",
                        rusqlite::params![current.id.as_str()],
                        row_to_repair_effect,
                    )
                    .optional()?;
                if let Some(effect) = existing_open {
                    return Ok(CreateAgentWorkspaceRepairEffectOutcome::OpenEffectExists(
                        effect,
                    ));
                }
                if request.effect.completed_at.is_some()
                    || matches!(
                        request.effect.status,
                        AgentWorkspaceRepairEffectStatus::Observed
                    )
                {
                    return Err(AppError::Validation(
                        "new repair effects must start incomplete".to_string(),
                    ));
                }
                write_repair_effect(conn, &request.effect)?;
                apply_compatibility_projection(
                    conn,
                    &current.conversation_id,
                    request.compatibility_projection.as_ref(),
                    request.effect.updated_at,
                )?;
                append_repair_events(conn, &request.events)?;
                Ok(CreateAgentWorkspaceRepairEffectOutcome::Created(
                    request.effect,
                ))
            })
            .await
    }

    async fn get_repair_effect_by_idempotency_key(
        &self,
        idempotency_key: &str,
    ) -> AppResult<Option<AgentWorkspaceRepairEffect>> {
        let idempotency_key = idempotency_key.to_string();
        self.db
            .run(move |conn| {
                conn.query_row(
                    "SELECT * FROM agent_workspace_repair_effects WHERE idempotency_key = ?1",
                    rusqlite::params![idempotency_key],
                    row_to_repair_effect,
                )
                .optional()
                .map_err(Into::into)
            })
            .await
    }

    async fn get_open_repair_effect(
        &self,
        attempt_id: &AgentWorkspaceRepairAttemptId,
    ) -> AppResult<Option<AgentWorkspaceRepairEffect>> {
        let attempt_id = attempt_id.as_str().to_string();
        self.db
            .run(move |conn| {
                conn.query_row(
                    "SELECT * FROM agent_workspace_repair_effects
                     WHERE attempt_id = ?1 AND completed_at IS NULL AND status != 'failed'
                     ORDER BY created_at ASC, rowid ASC
                     LIMIT 1",
                    rusqlite::params![attempt_id],
                    row_to_repair_effect,
                )
                .optional()
                .map_err(Into::into)
            })
            .await
    }

    async fn complete_repair_effect(
        &self,
        request: CompleteAgentWorkspaceRepairEffect,
    ) -> AppResult<CompleteAgentWorkspaceRepairEffectOutcome> {
        validate_repair_effect_shape(&request.effect)?;
        let attempt_id = request.attempt_id.clone();
        self.db
            .run_transaction(move |conn| {
                let Some(current) = load_repair_attempt(conn, attempt_id.as_str())? else {
                    return Ok(CompleteAgentWorkspaceRepairEffectOutcome::Missing);
                };
                validate_repair_events(&current.conversation_id, &request.events)?;
                if request.effect.attempt_id != current.id {
                    return Err(AppError::Validation(
                        "repair effect does not belong to its requested attempt".to_string(),
                    ));
                }
                let Some(existing) = conn
                    .query_row(
                        "SELECT * FROM agent_workspace_repair_effects WHERE id = ?1",
                        rusqlite::params![request.effect.id.as_str()],
                        row_to_repair_effect,
                    )
                    .optional()?
                else {
                    return Ok(CompleteAgentWorkspaceRepairEffectOutcome::Missing);
                };
                if existing.attempt_id != current.id {
                    return Err(AppError::Validation(
                        "repair effect belongs to another attempt".to_string(),
                    ));
                }
                if existing.completed_at.is_some() && existing == request.effect {
                    return Ok(CompleteAgentWorkspaceRepairEffectOutcome::Applied(existing));
                }
                if current.generation != request.generation
                    || current.phase != request.expected_phase
                    || current.updated_at != request.expected_attempt_updated_at
                    || current.settled_at.is_some()
                    || existing.updated_at != request.expected_effect_updated_at
                    || existing.status != request.expected_effect_status
                {
                    return Ok(CompleteAgentWorkspaceRepairEffectOutcome::Stale(current));
                }
                if !update_repair_effect(
                    conn,
                    &request.effect,
                    request.expected_effect_updated_at,
                    request.expected_effect_status,
                )? {
                    let latest = conn
                        .query_row(
                            "SELECT * FROM agent_workspace_repair_effects WHERE id = ?1",
                            rusqlite::params![request.effect.id.as_str()],
                            row_to_repair_effect,
                        )
                        .optional()?
                        .ok_or_else(|| {
                            AppError::NotFound(format!("repair effect {}", request.effect.id))
                        })?;
                    if latest.completed_at.is_some() && latest == request.effect {
                        return Ok(CompleteAgentWorkspaceRepairEffectOutcome::Applied(latest));
                    }
                    return Ok(CompleteAgentWorkspaceRepairEffectOutcome::Stale(current));
                }
                apply_compatibility_projection(
                    conn,
                    &current.conversation_id,
                    request.compatibility_projection.as_ref(),
                    request.effect.updated_at,
                )?;
                append_repair_events(conn, &request.events)?;
                Ok(CompleteAgentWorkspaceRepairEffectOutcome::Applied(
                    request.effect,
                ))
            })
            .await
    }

    async fn import_legacy_repair_attempt(
        &self,
        request: ImportLegacyAgentWorkspaceRepairAttempt,
    ) -> AppResult<ImportLegacyAgentWorkspaceRepairAttemptOutcome> {
        validate_repair_events(&request.attempt.conversation_id, &request.events)?;
        if request.attempt.generation == 0 {
            return Err(AppError::Validation(
                "legacy repair attempts require a positive generation".to_string(),
            ));
        }
        let attempt_id = request.attempt.id.clone();
        self.db
            .run_transaction(move |conn| {
                if let Some(existing) =
                    load_latest_repair_attempt(conn, &request.attempt.conversation_id.as_str())?
                {
                    return Ok(
                        ImportLegacyAgentWorkspaceRepairAttemptOutcome::ExistingDurable(existing),
                    );
                }
                if let Some(existing) = load_repair_attempt(conn, attempt_id.as_str())? {
                    return Err(AppError::Conflict(format!(
                        "legacy repair attempt id {} belongs to a different conversation {}",
                        existing.id, existing.conversation_id
                    )));
                }
                write_repair_attempt(conn, &request.attempt)?;
                apply_compatibility_projection(
                    conn,
                    &request.attempt.conversation_id,
                    request.compatibility_projection.as_ref(),
                    request.attempt.updated_at,
                )?;
                append_repair_events(conn, &request.events)?;
                Ok(ImportLegacyAgentWorkspaceRepairAttemptOutcome::Imported(
                    request.attempt,
                ))
            })
            .await
    }
}
