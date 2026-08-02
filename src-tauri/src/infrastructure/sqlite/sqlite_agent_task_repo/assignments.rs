use chrono::{DateTime, Utc};
use rusqlite::{params, types::Type, Connection, OptionalExtension};
use serde_json::{json, Value};

use super::*;
use crate::domain::entities::{
    AgentTaskAssignment, AgentTaskAssignmentId, AgentTaskAssignmentState, TeamMemberId,
    TeamSessionId,
};

const UNRESOLVED_STATES_SQL: &str =
    "('reserved', 'active', 'completion_requested', 'release_requested')";
const ASSIGNMENT_MUTATION_LOCKED: &str =
    "agent task state, owner, and dependencies are controlled by an active delegate assignment";

pub(super) fn ensure_mutation_allowed(
    conn: &Connection,
    list_id: &AgentTaskListId,
    task_id: &AgentTaskId,
    patch: &AgentTaskPatch,
) -> AppResult<()> {
    let owns_assignment_fields = patch.state.is_some()
        || patch.owner_agent.is_some()
        || !patch.add_blocked_by.is_empty()
        || !patch.add_blocks.is_empty()
        || !patch.remove_blocked_by.is_empty()
        || !patch.remove_blocks.is_empty();
    if !owns_assignment_fields {
        return Ok(());
    }
    let exists: bool = conn.query_row(
        &format!(
            "SELECT EXISTS(
                SELECT 1
                FROM agent_task_delegate_assignments
                WHERE task_list_id = ?1
                  AND task_id = ?2
                  AND state IN {UNRESOLVED_STATES_SQL}
            )"
        ),
        params![list_id.as_str(), task_id.as_str()],
        |row| row.get(0),
    )?;
    if exists {
        return Err(AppError::Conflict(ASSIGNMENT_MUTATION_LOCKED.to_string()));
    }
    Ok(())
}

pub(super) async fn reserve(
    db: &DbConnection,
    scope: &AgentTaskScope,
    task_ref: &str,
    delegated_session_id: &DelegatedSessionId,
    caller_agent_run_id: &AgentRunId,
    delegate_agent_name: &str,
) -> AppResult<Option<AgentTaskAssignmentReservation>> {
    let scope = scope.clone();
    let task_ref = task_ref.to_string();
    let delegated_session_id = delegated_session_id.clone();
    let caller_agent_run_id = caller_agent_run_id.clone();
    let delegate_agent_name = delegate_agent_name.to_string();
    db.run_transaction(move |conn| {
        let Some(list) = find_list(conn, &scope)? else {
            return Ok(None);
        };
        let Some(task) = find_task(conn, &list.id, &task_ref)? else {
            return Ok(None);
        };
        if task.state != AgentTaskState::Open {
            return Err(AppError::Conflict(
                "agent task must be open before delegation".to_string(),
            ));
        }
        let detail = detail_for_row(conn, &task)?;
        if !detail.unresolved_blocked_by.is_empty() {
            return Err(AppError::Validation(format!(
                "agent task is blocked by unresolved tasks: {}",
                detail.unresolved_blocked_by.join(", ")
            )));
        }
        let meaningful_count: i64 = conn.query_row(
            "SELECT COUNT(*)
             FROM agent_tasks
             WHERE task_list_id = ?1 AND state != 'dropped'",
            [list.id.as_str()],
            |row| row.get(0),
        )?;
        if meaningful_count == 1 {
            return Err(AppError::Validation(
                "single-task agent task ledger cannot be delegated; decompose it first".to_string(),
            ));
        }
        let conflict: bool = conn.query_row(
            &format!(
                "SELECT EXISTS(
                    SELECT 1
                    FROM agent_task_delegate_assignments
                    WHERE state IN {UNRESOLVED_STATES_SQL}
                      AND (
                        delegated_session_id = ?1
                        OR (task_list_id = ?2 AND task_id = ?3)
                      )
                )"
            ),
            params![
                delegated_session_id.as_str(),
                list.id.as_str(),
                task.task_id.as_str()
            ],
            |row| row.get(0),
        )?;
        if conflict {
            return Err(AppError::Conflict(
                "agent task or delegated session already has an unresolved assignment".to_string(),
            ));
        }

        let attempt_number: i64 = conn.query_row(
            "SELECT COALESCE(MAX(attempt_number), 0) + 1
             FROM agent_task_delegate_assignments
             WHERE delegated_session_id = ?1",
            [delegated_session_id.as_str()],
            |row| row.get(0),
        )?;
        let now = Utc::now();
        let assignment_id = AgentTaskAssignmentId::new();
        let updated = conn.execute(
            "UPDATE agent_tasks
             SET state = 'active',
                 owner_agent = ?1,
                 completed_at = NULL,
                 version = version + 1,
                 updated_at = ?2
             WHERE task_list_id = ?3 AND id = ?4 AND state = 'open'",
            params![
                delegate_agent_name,
                now.to_rfc3339(),
                list.id.as_str(),
                task.task_id.as_str()
            ],
        )?;
        if updated != 1 {
            return Err(AppError::Conflict(
                "agent task changed before assignment reservation".to_string(),
            ));
        }
        conn.execute(
            "INSERT INTO agent_task_delegate_assignments (
                id, delegated_session_id, attempt_number, caller_agent_run_id,
                delegated_agent_run_id, task_list_id, task_id, delegate_agent_name,
                state, prior_owner_agent, settlement_reason, completion_metadata_json,
                created_at, run_bound_at, settled_at, updated_at
             ) VALUES (
                ?1, ?2, ?3, ?4,
                NULL, ?5, ?6, ?7,
                'reserved', ?8, NULL, NULL,
                ?9, NULL, NULL, ?10
             )",
            params![
                assignment_id.as_str(),
                delegated_session_id.as_str(),
                attempt_number,
                caller_agent_run_id.as_str(),
                list.id.as_str(),
                task.task_id.as_str(),
                delegate_agent_name,
                task.owner_agent,
                now.to_rfc3339(),
                now.to_rfc3339()
            ],
        )?;
        append_event(
            conn,
            &list.id,
            "agent_task.assignment_reserved",
            scope.actor_agent.as_deref(),
            Some(&task.task_id),
            json!({
                "delegated_session_id": delegated_session_id.as_str(),
                "attempt_number": attempt_number,
                "delegate_agent_name": delegate_agent_name,
            }),
        )?;
        let assignment = load_by_id(conn, &assignment_id)?
            .ok_or_else(|| AppError::Database("reserved assignment disappeared".to_string()))?;
        Ok(Some(AgentTaskAssignmentReservation {
            assignment: view(conn, assignment)?,
        }))
    })
    .await
}

pub(super) async fn bind_run(
    db: &DbConnection,
    assignment_id: &AgentTaskAssignmentId,
    delegated_session_id: &DelegatedSessionId,
    delegated_agent_run_id: &AgentRunId,
) -> AppResult<Option<AgentTaskAssignmentView>> {
    let assignment_id = assignment_id.clone();
    let delegated_session_id = delegated_session_id.clone();
    let delegated_agent_run_id = delegated_agent_run_id.clone();
    db.run_transaction(move |conn| {
        let Some(assignment) = load_by_id(conn, &assignment_id)? else {
            return Ok(None);
        };
        if assignment.delegated_session_id != delegated_session_id {
            return Err(AppError::Conflict(
                "delegate assignment does not belong to the requested session".to_string(),
            ));
        }
        if assignment.planned_delegated_agent_run_id.as_ref() != Some(&delegated_agent_run_id) {
            return Err(AppError::Conflict(
                "delegate assignment was not planned for the requested run".to_string(),
            ));
        }
        if assignment.state == AgentTaskAssignmentState::Active {
            if assignment.delegated_agent_run_id.as_ref() != Some(&delegated_agent_run_id) {
                return Err(AppError::Conflict(
                    "active delegate assignment is bound to a different run".to_string(),
                ));
            }
            return Ok(Some(view(conn, assignment)?));
        }
        if assignment.state != AgentTaskAssignmentState::Reserved {
            return Err(AppError::Conflict(
                "only the exact reserved delegate assignment can bind a run".to_string(),
            ));
        }
        let now = Utc::now();
        let updated = conn.execute(
            "UPDATE agent_task_delegate_assignments
             SET delegated_agent_run_id = ?1,
                 state = 'active',
                 run_bound_at = ?2,
                 updated_at = ?3
             WHERE id = ?4
               AND delegated_session_id = ?5
               AND state = 'reserved'
               AND planned_delegated_agent_run_id = ?6
               AND delegated_agent_run_id IS NULL",
            params![
                delegated_agent_run_id.as_str(),
                now.to_rfc3339(),
                now.to_rfc3339(),
                assignment.id.as_str(),
                delegated_session_id.as_str(),
                delegated_agent_run_id.as_str()
            ],
        )?;
        if updated != 1 {
            return Err(AppError::Conflict(
                "delegate assignment changed before run binding".to_string(),
            ));
        }
        append_event(
            conn,
            &assignment.task_list_id,
            "agent_task.assignment_run_bound",
            None,
            Some(&assignment.task_id),
            assignment_lifecycle_payload(
                &assignment,
                json!({"delegated_agent_run_id": delegated_agent_run_id.as_str()}),
            ),
        )?;
        let bound = load_by_id(conn, &assignment.id)?
            .ok_or_else(|| AppError::Database("bound assignment disappeared".to_string()))?;
        Ok(Some(view(conn, bound)?))
    })
    .await
}

pub(super) async fn plan_run(
    db: &DbConnection,
    assignment_id: &AgentTaskAssignmentId,
    delegated_session_id: &DelegatedSessionId,
    delegated_agent_run_id: &AgentRunId,
) -> AppResult<Option<AgentTaskAssignmentView>> {
    let assignment_id = assignment_id.clone();
    let delegated_session_id = delegated_session_id.clone();
    let delegated_agent_run_id = *delegated_agent_run_id;
    db.run_transaction(move |conn| {
        let Some(assignment) = load_by_id(conn, &assignment_id)? else {
            return Ok(None);
        };
        if assignment.delegated_session_id != delegated_session_id {
            return Err(AppError::Conflict(
                "delegate assignment does not belong to the requested session".to_string(),
            ));
        }
        if assignment.state != AgentTaskAssignmentState::Reserved {
            return Err(AppError::Conflict(
                "only a reserved delegate assignment can plan a run".to_string(),
            ));
        }
        if let Some(planned) = assignment.planned_delegated_agent_run_id.as_ref() {
            if planned != &delegated_agent_run_id {
                return Err(AppError::Conflict(
                    "delegate assignment is already planned for a different run".to_string(),
                ));
            }
            return Ok(Some(view(conn, assignment)?));
        }
        let now = Utc::now();
        let updated = conn.execute(
            "UPDATE agent_task_delegate_assignments
             SET planned_delegated_agent_run_id = ?1,
                 updated_at = ?2
             WHERE id = ?3
               AND delegated_session_id = ?4
               AND state = 'reserved'
               AND planned_delegated_agent_run_id IS NULL
               AND delegated_agent_run_id IS NULL",
            params![
                delegated_agent_run_id.as_str(),
                now.to_rfc3339(),
                assignment.id.as_str(),
                delegated_session_id.as_str()
            ],
        )?;
        if updated != 1 {
            return Err(AppError::Conflict(
                "delegate assignment changed before run planning".to_string(),
            ));
        }
        append_event(
            conn,
            &assignment.task_list_id,
            "agent_task.assignment_run_planned",
            None,
            Some(&assignment.task_id),
            assignment_lifecycle_payload(
                &assignment,
                json!({"delegated_agent_run_id": delegated_agent_run_id.as_str()}),
            ),
        )?;
        let planned = load_by_id(conn, &assignment.id)?
            .ok_or_else(|| AppError::Database("planned assignment disappeared".to_string()))?;
        Ok(Some(view(conn, planned)?))
    })
    .await
}

pub(super) async fn set_team_identity(
    db: &DbConnection,
    assignment_id: &AgentTaskAssignmentId,
    delegated_session_id: &DelegatedSessionId,
    team_id: &TeamSessionId,
    team_member_id: &TeamMemberId,
    team_member_generation: i64,
) -> AppResult<Option<AgentTaskAssignmentView>> {
    let assignment_id = assignment_id.clone();
    let delegated_session_id = delegated_session_id.clone();
    let team_id = team_id.clone();
    let team_member_id = team_member_id.clone();
    db.run_transaction(move |conn| {
        let Some(assignment) = load_by_id(conn, &assignment_id)? else {
            return Ok(None);
        };
        if assignment.delegated_session_id != delegated_session_id {
            return Err(AppError::Conflict(
                "delegate assignment does not belong to the requested session".to_string(),
            ));
        }
        if assignment.state != AgentTaskAssignmentState::Reserved {
            return Err(AppError::Conflict(
                "only a reserved delegate assignment can receive Team authority".to_string(),
            ));
        }
        if assignment.team_id.is_some()
            && (assignment.team_id.as_ref() != Some(&team_id)
                || assignment.team_member_id.as_ref() != Some(&team_member_id)
                || assignment.team_member_generation != Some(team_member_generation))
        {
            return Err(AppError::Conflict(
                "delegate assignment already belongs to a different Team member generation"
                    .to_string(),
            ));
        }
        let updated = conn.execute(
            "UPDATE agent_task_delegate_assignments
             SET team_id = ?1, team_member_id = ?2, team_member_generation = ?3,
                 updated_at = ?4
             WHERE id = ?5 AND delegated_session_id = ?6 AND state = 'reserved'
               AND (team_id IS NULL OR (team_id = ?1 AND team_member_id = ?2
                    AND team_member_generation = ?3))",
            params![
                team_id.as_str(),
                team_member_id.as_str(),
                team_member_generation,
                Utc::now().to_rfc3339(),
                assignment_id.as_str(),
                delegated_session_id.as_str(),
            ],
        )?;
        if updated != 1 {
            return Err(AppError::Conflict(
                "delegate assignment changed before Team authority was attached".to_string(),
            ));
        }
        let assignment = load_by_id(conn, &assignment_id)?
            .ok_or_else(|| AppError::Database("Team-linked assignment disappeared".to_string()))?;
        append_event(
            conn,
            &assignment.task_list_id,
            "agent_task.assignment_team_identity_bound",
            None,
            Some(&assignment.task_id),
            assignment_lifecycle_payload(&assignment, json!({})),
        )?;
        Ok(Some(view(conn, assignment)?))
    })
    .await
}

pub(super) async fn get_unresolved(
    db: &DbConnection,
    delegated_session_id: &DelegatedSessionId,
) -> AppResult<Option<AgentTaskAssignmentView>> {
    let delegated_session_id = delegated_session_id.clone();
    db.run(move |conn| {
        load_unresolved_for_session(conn, &delegated_session_id)?
            .map(|assignment| view(conn, assignment))
            .transpose()
    })
    .await
}

pub(super) async fn request_completion(
    db: &DbConnection,
    delegated_session_id: &DelegatedSessionId,
    delegated_agent_run_id: &AgentRunId,
    local_scope: &AgentTaskScope,
    completion_metadata: Option<Value>,
) -> AppResult<Option<AgentTaskAssignmentView>> {
    let delegated_session_id = delegated_session_id.clone();
    let delegated_agent_run_id = delegated_agent_run_id.clone();
    let local_scope = local_scope.clone();
    db.run_transaction(move |conn| {
        request_intent_in_transaction(
            conn,
            &delegated_session_id,
            &delegated_agent_run_id,
            AgentTaskAssignmentState::CompletionRequested,
            Some(&local_scope),
            completion_metadata,
            None,
        )
    })
    .await
}

pub(super) async fn request_release(
    db: &DbConnection,
    delegated_session_id: &DelegatedSessionId,
    delegated_agent_run_id: &AgentRunId,
    reason: &str,
) -> AppResult<Option<AgentTaskAssignmentView>> {
    request_intent(
        db,
        delegated_session_id,
        delegated_agent_run_id,
        AgentTaskAssignmentState::ReleaseRequested,
        None,
        Some(reason.to_string()),
    )
    .await
}

async fn request_intent(
    db: &DbConnection,
    delegated_session_id: &DelegatedSessionId,
    delegated_agent_run_id: &AgentRunId,
    requested_state: AgentTaskAssignmentState,
    completion_metadata: Option<Value>,
    reason: Option<String>,
) -> AppResult<Option<AgentTaskAssignmentView>> {
    let delegated_session_id = delegated_session_id.clone();
    let delegated_agent_run_id = delegated_agent_run_id.clone();
    db.run_transaction(move |conn| {
        request_intent_in_transaction(
            conn,
            &delegated_session_id,
            &delegated_agent_run_id,
            requested_state,
            None,
            completion_metadata,
            reason,
        )
    })
    .await
}

fn request_intent_in_transaction(
    conn: &Connection,
    delegated_session_id: &DelegatedSessionId,
    delegated_agent_run_id: &AgentRunId,
    requested_state: AgentTaskAssignmentState,
    local_scope: Option<&AgentTaskScope>,
    completion_metadata: Option<Value>,
    reason: Option<String>,
) -> AppResult<Option<AgentTaskAssignmentView>> {
    let Some(assignment) = load_unresolved_for_session(conn, delegated_session_id)? else {
        return Ok(None);
    };
    if assignment.delegated_agent_run_id.as_ref() != Some(delegated_agent_run_id) {
        return Err(AppError::Conflict(
            "delegate assignment does not belong to the current run".to_string(),
        ));
    }
    if assignment.state == requested_state {
        return Ok(Some(view(conn, assignment)?));
    }
    if assignment.state != AgentTaskAssignmentState::Active {
        return Err(AppError::Conflict(
            "delegate assignment cannot accept that request in its current state".to_string(),
        ));
    }
    if let Some(local_scope) = local_scope {
        ensure_local_ledger_resolved(conn, local_scope)?;
    }
    let now = Utc::now();
    let updated = conn.execute(
        "UPDATE agent_task_delegate_assignments
         SET state = ?1,
             completion_metadata_json = ?2,
             settlement_reason = ?3,
             updated_at = ?4
         WHERE id = ?5 AND state = 'active'",
        params![
            requested_state.as_str(),
            value_to_json_text(&completion_metadata)?,
            reason,
            now.to_rfc3339(),
            assignment.id.as_str()
        ],
    )?;
    if updated != 1 {
        return Err(AppError::Conflict(
            "delegate assignment changed before intent request".to_string(),
        ));
    }
    append_event(
        conn,
        &assignment.task_list_id,
        if requested_state == AgentTaskAssignmentState::CompletionRequested {
            "agent_task.assignment_completion_requested"
        } else {
            "agent_task.assignment_release_requested"
        },
        None,
        Some(&assignment.task_id),
        assignment_lifecycle_payload(
            &assignment,
            json!({"delegated_agent_run_id": delegated_agent_run_id.as_str()}),
        ),
    )?;
    let requested = load_by_id(conn, &assignment.id)?
        .ok_or_else(|| AppError::Database("requested assignment disappeared".to_string()))?;
    Ok(Some(view(conn, requested)?))
}

fn ensure_local_ledger_resolved(conn: &Connection, local_scope: &AgentTaskScope) -> AppResult<()> {
    let Some(list) = find_list(conn, local_scope)? else {
        return Ok(());
    };
    let mut statement = conn.prepare(
        "SELECT task_number
         FROM agent_tasks
         WHERE task_list_id = ?1
           AND state IN ('open', 'active')
         ORDER BY task_number",
    )?;
    let task_refs = statement
        .query_map([list.id.as_str()], |row| row.get::<_, i64>(0))?
        .map(|result| result.map(|task_number| task_number.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    if task_refs.is_empty() {
        Ok(())
    } else {
        Err(AppError::Validation(format!(
            "delegate-local tasks must be resolved before requesting assignment completion: {}",
            task_refs.join(", ")
        )))
    }
}

pub(super) async fn settle(
    db: &DbConnection,
    delegated_agent_run_id: &AgentRunId,
    terminal_status: AgentTaskAssignmentTerminalStatus,
    reason: Option<&str>,
) -> AppResult<Option<AgentTaskAssignmentSettlement>> {
    let delegated_agent_run_id = delegated_agent_run_id.clone();
    let reason = reason.map(str::to_string);
    db.run_transaction(move |conn| {
        let Some(assignment) = load_unresolved_for_run(conn, &delegated_agent_run_id)? else {
            return Ok(None);
        };
        settle_loaded(conn, assignment, terminal_status, reason.as_deref())
    })
    .await
}

pub(super) async fn get_for_run(
    db: &DbConnection,
    delegated_agent_run_id: &AgentRunId,
) -> AppResult<Option<AgentTaskAssignmentView>> {
    let delegated_agent_run_id = delegated_agent_run_id.clone();
    db.run(move |conn| {
        load_for_run(conn, &delegated_agent_run_id)?
            .map(|assignment| view(conn, assignment))
            .transpose()
    })
    .await
}

pub(super) async fn fail_reserved(
    db: &DbConnection,
    delegated_session_id: &DelegatedSessionId,
    reason: &str,
) -> AppResult<Option<AgentTaskAssignmentSettlement>> {
    let delegated_session_id = delegated_session_id.clone();
    let reason = reason.to_string();
    db.run_transaction(move |conn| {
        let Some(assignment) = load_unresolved_for_session(conn, &delegated_session_id)? else {
            return Ok(None);
        };
        if assignment.state != AgentTaskAssignmentState::Reserved {
            return Err(AppError::Conflict(
                "only a reserved delegate assignment can be failed before launch".to_string(),
            ));
        }
        settle_loaded(
            conn,
            assignment,
            AgentTaskAssignmentTerminalStatus::Failed,
            Some(&reason),
        )
    })
    .await
}

pub(super) async fn list_unresolved(db: &DbConnection) -> AppResult<Vec<AgentTaskAssignmentView>> {
    db.run(move |conn| {
        let mut statement = conn.prepare(&format!(
            "{ASSIGNMENT_SELECT}
             WHERE a.state IN {UNRESOLVED_STATES_SQL}
             ORDER BY a.created_at"
        ))?;
        let assignments = statement
            .query_map([], row_to_assignment)?
            .collect::<Result<Vec<_>, _>>()?;
        assignments
            .into_iter()
            .map(|assignment| view(conn, assignment))
            .collect()
    })
    .await
}

fn settle_loaded(
    conn: &Connection,
    assignment: AgentTaskAssignment,
    terminal_status: AgentTaskAssignmentTerminalStatus,
    reason: Option<&str>,
) -> AppResult<Option<AgentTaskAssignmentSettlement>> {
    let completion_authorized = terminal_status == AgentTaskAssignmentTerminalStatus::Completed
        && assignment.state == AgentTaskAssignmentState::CompletionRequested;
    let now = Utc::now();
    let (assignment_state, settlement_reason) = if completion_authorized {
        (
            AgentTaskAssignmentState::Completed,
            reason.map(str::to_string),
        )
    } else {
        let state = match terminal_status {
            AgentTaskAssignmentTerminalStatus::Completed => AgentTaskAssignmentState::Released,
            AgentTaskAssignmentTerminalStatus::Failed => AgentTaskAssignmentState::Failed,
            AgentTaskAssignmentTerminalStatus::Cancelled => AgentTaskAssignmentState::Cancelled,
        };
        let default_reason = match terminal_status {
            AgentTaskAssignmentTerminalStatus::Completed => "ended_without_assignment_completion",
            AgentTaskAssignmentTerminalStatus::Failed => "delegated_run_failed",
            AgentTaskAssignmentTerminalStatus::Cancelled => "delegated_run_cancelled",
        };
        (state, Some(reason.unwrap_or(default_reason).to_string()))
    };

    let task_updated = if completion_authorized {
        conn.execute(
            "UPDATE agent_tasks
             SET state = 'done',
                 metadata_json = ?1,
                 completed_at = ?2,
                 version = version + 1,
                 updated_at = ?3
             WHERE task_list_id = ?4
               AND id = ?5
               AND state = 'active'
               AND owner_agent = ?6",
            params![
                value_to_json_text(&merge_agent_task_metadata(
                    detail_for_task_id(conn, &assignment.task_list_id, &assignment.task_id)?
                        .metadata,
                    assignment
                        .completion_metadata
                        .clone()
                        .unwrap_or_else(|| json!({}))
                ))?,
                now.to_rfc3339(),
                now.to_rfc3339(),
                assignment.task_list_id.as_str(),
                assignment.task_id.as_str(),
                assignment.delegate_agent_name
            ],
        )?
    } else {
        conn.execute(
            "UPDATE agent_tasks
             SET state = 'open',
                 owner_agent = ?1,
                 completed_at = NULL,
                 version = version + 1,
                 updated_at = ?2
             WHERE task_list_id = ?3
               AND id = ?4
               AND state = 'active'
               AND owner_agent = ?5",
            params![
                assignment.prior_owner_agent,
                now.to_rfc3339(),
                assignment.task_list_id.as_str(),
                assignment.task_id.as_str(),
                assignment.delegate_agent_name
            ],
        )?
    };
    if task_updated != 1 {
        return Err(AppError::Conflict(
            "assigned task fields changed before terminal settlement".to_string(),
        ));
    }
    let assignment_updated = conn.execute(
        &format!(
            "UPDATE agent_task_delegate_assignments
             SET state = ?1,
                 settlement_reason = ?2,
                 settled_at = ?3,
                 updated_at = ?4
             WHERE id = ?5
               AND state IN {UNRESOLVED_STATES_SQL}"
        ),
        params![
            assignment_state.as_str(),
            settlement_reason,
            now.to_rfc3339(),
            now.to_rfc3339(),
            assignment.id.as_str()
        ],
    )?;
    if assignment_updated != 1 {
        return Err(AppError::Conflict(
            "delegate assignment changed before terminal settlement".to_string(),
        ));
    }
    append_event(
        conn,
        &assignment.task_list_id,
        if completion_authorized {
            "agent_task.assignment_completed"
        } else {
            "agent_task.assignment_reopened"
        },
        None,
        Some(&assignment.task_id),
        assignment_lifecycle_payload(
            &assignment,
            json!({
                "assignment_state": assignment_state.as_str(),
                "settlement_reason": settlement_reason,
            }),
        ),
    )?;
    let settled = load_by_id(conn, &assignment.id)?
        .ok_or_else(|| AppError::Database("settled assignment disappeared".to_string()))?;
    Ok(Some(AgentTaskAssignmentSettlement {
        assignment: view(conn, settled)?,
        task_reopened: !completion_authorized,
        task_completed: completion_authorized,
    }))
}

/// Enrich Team assignment lifecycle payloads without changing the historical
/// Solo event shape. The dedicated identity-bound event and later run/terminal
/// events therefore remain generation-fenced for diagnostics and recovery.
fn assignment_lifecycle_payload(assignment: &AgentTaskAssignment, mut payload: Value) -> Value {
    let (Some(team_id), Some(member_id), Some(member_generation)) = (
        assignment.team_id.as_ref(),
        assignment.team_member_id.as_ref(),
        assignment.team_member_generation,
    ) else {
        return payload;
    };
    if let Some(fields) = payload.as_object_mut() {
        fields.insert("team_id".to_string(), json!(team_id.as_str()));
        fields.insert("team_member_id".to_string(), json!(member_id.as_str()));
        fields.insert(
            "team_member_generation".to_string(),
            json!(member_generation),
        );
    }
    payload
}

const ASSIGNMENT_SELECT: &str = "SELECT
    a.id,
    a.delegated_session_id,
    a.attempt_number,
    a.caller_agent_run_id,
    a.planned_delegated_agent_run_id,
    a.delegated_agent_run_id,
    a.team_id,
    a.team_member_id,
    a.team_member_generation,
    a.task_list_id,
    a.task_id,
    a.delegate_agent_name,
    a.state,
    a.prior_owner_agent,
    a.settlement_reason,
    a.completion_metadata_json,
    a.created_at,
    a.run_bound_at,
    a.settled_at,
    a.updated_at
 FROM agent_task_delegate_assignments a";

fn load_by_id(
    conn: &Connection,
    assignment_id: &AgentTaskAssignmentId,
) -> AppResult<Option<AgentTaskAssignment>> {
    Ok(conn
        .query_row(
            &format!("{ASSIGNMENT_SELECT} WHERE a.id = ?1"),
            [assignment_id.as_str()],
            row_to_assignment,
        )
        .optional()?)
}

fn load_unresolved_for_session(
    conn: &Connection,
    delegated_session_id: &DelegatedSessionId,
) -> AppResult<Option<AgentTaskAssignment>> {
    Ok(conn
        .query_row(
            &format!(
                "{ASSIGNMENT_SELECT}
                 WHERE a.delegated_session_id = ?1
                   AND a.state IN {UNRESOLVED_STATES_SQL}"
            ),
            [delegated_session_id.as_str()],
            row_to_assignment,
        )
        .optional()?)
}

fn load_unresolved_for_run(
    conn: &Connection,
    delegated_agent_run_id: &AgentRunId,
) -> AppResult<Option<AgentTaskAssignment>> {
    Ok(conn
        .query_row(
            &format!(
                "{ASSIGNMENT_SELECT}
                 WHERE a.delegated_agent_run_id = ?1
                   AND a.state IN {UNRESOLVED_STATES_SQL}"
            ),
            [delegated_agent_run_id.as_str()],
            row_to_assignment,
        )
        .optional()?)
}

fn load_for_run(
    conn: &Connection,
    delegated_agent_run_id: &AgentRunId,
) -> AppResult<Option<AgentTaskAssignment>> {
    Ok(conn
        .query_row(
            &format!("{ASSIGNMENT_SELECT} WHERE a.delegated_agent_run_id = ?1"),
            [delegated_agent_run_id.as_str()],
            row_to_assignment,
        )
        .optional()?)
}

fn row_to_assignment(row: &rusqlite::Row<'_>) -> rusqlite::Result<AgentTaskAssignment> {
    let state = row.get::<_, String>("state")?;
    let completion_metadata_json = row.get::<_, Option<String>>("completion_metadata_json")?;
    Ok(AgentTaskAssignment {
        id: AgentTaskAssignmentId::from_string(row.get::<_, String>("id")?),
        delegated_session_id: DelegatedSessionId::from_string(
            row.get::<_, String>("delegated_session_id")?,
        ),
        attempt_number: row.get("attempt_number")?,
        caller_agent_run_id: AgentRunId::from_string(row.get::<_, String>("caller_agent_run_id")?),
        planned_delegated_agent_run_id: row
            .get::<_, Option<String>>("planned_delegated_agent_run_id")?
            .map(AgentRunId::from_string),
        delegated_agent_run_id: row
            .get::<_, Option<String>>("delegated_agent_run_id")?
            .map(AgentRunId::from_string),
        team_id: row
            .get::<_, Option<String>>("team_id")?
            .map(TeamSessionId::from_string),
        team_member_id: row
            .get::<_, Option<String>>("team_member_id")?
            .map(TeamMemberId::from_string),
        team_member_generation: row.get("team_member_generation")?,
        task_list_id: AgentTaskListId::from_string(row.get::<_, String>("task_list_id")?),
        task_id: AgentTaskId::from_string(row.get::<_, String>("task_id")?),
        delegate_agent_name: row.get("delegate_agent_name")?,
        state: state.parse().map_err(|error: String| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                Type::Text,
                Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, error)),
            )
        })?,
        prior_owner_agent: row.get("prior_owner_agent")?,
        settlement_reason: row.get("settlement_reason")?,
        completion_metadata: completion_metadata_json
            .map(|raw| serde_json::from_str(&raw))
            .transpose()
            .map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(0, Type::Text, Box::new(error))
            })?,
        created_at: parse_required_datetime(row.get::<_, String>("created_at")?),
        run_bound_at: row
            .get::<_, Option<String>>("run_bound_at")?
            .as_deref()
            .map(parse_required_datetime),
        settled_at: row
            .get::<_, Option<String>>("settled_at")?
            .as_deref()
            .map(parse_required_datetime),
        updated_at: parse_required_datetime(row.get::<_, String>("updated_at")?),
    })
}

fn parse_required_datetime(value: impl AsRef<str>) -> DateTime<Utc> {
    parse_datetime(value.as_ref())
}

fn view(conn: &Connection, assignment: AgentTaskAssignment) -> AppResult<AgentTaskAssignmentView> {
    let (scope_type, scope_id): (String, String) = conn.query_row(
        "SELECT scope_type, scope_id
         FROM agent_task_lists
         WHERE id = ?1",
        [assignment.task_list_id.as_str()],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let task = detail_for_task_id(conn, &assignment.task_list_id, &assignment.task_id)?;
    Ok(AgentTaskAssignmentView {
        assignment,
        caller_scope_type: scope_type,
        caller_scope_id: scope_id,
        task,
    })
}
