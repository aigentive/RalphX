use chrono::Utc;
use serde_json::Value;

use super::*;

const ASSIGNMENT_MUTATION_LOCKED: &str =
    "agent task state, owner, and dependencies are controlled by an active delegate assignment";

pub(super) fn ensure_mutation_allowed(
    state: &MemoryAgentTaskState,
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
    if owns_assignment_fields
        && state.assignments.iter().any(|assignment| {
            assignment.task_list_id == *list_id
                && assignment.task_id == *task_id
                && assignment.state.is_unresolved()
        })
    {
        return Err(AppError::Conflict(ASSIGNMENT_MUTATION_LOCKED.to_string()));
    }
    Ok(())
}

pub(super) fn reserve(
    repo: &MemoryAgentTaskRepository,
    scope: &AgentTaskScope,
    task_ref: &str,
    delegated_session_id: &DelegatedSessionId,
    caller_agent_run_id: &AgentRunId,
    delegate_agent_name: &str,
) -> AppResult<Option<AgentTaskAssignmentReservation>> {
    let mut state = repo.state.write().unwrap();
    let Some(list) = find_list(&state, scope).cloned() else {
        return Ok(None);
    };
    let Some(task_index) = find_task_index(&state, &list.id, task_ref) else {
        return Ok(None);
    };
    let task = state.tasks[task_index].clone();
    if task.state != AgentTaskState::Open {
        return Err(AppError::Conflict(
            "agent task must be open before delegation".to_string(),
        ));
    }
    let detail = detail_for_row(&state, &task);
    if !detail.unresolved_blocked_by.is_empty() {
        return Err(AppError::Validation(format!(
            "agent task is blocked by unresolved tasks: {}",
            detail.unresolved_blocked_by.join(", ")
        )));
    }
    let meaningful_count = state
        .tasks
        .iter()
        .filter(|candidate| {
            candidate.task_list_id == list.id && candidate.state != AgentTaskState::Dropped
        })
        .count();
    if meaningful_count == 1 {
        return Err(AppError::Validation(
            "single-task agent task ledger cannot be delegated; decompose it first".to_string(),
        ));
    }
    if state.assignments.iter().any(|assignment| {
        assignment.state.is_unresolved()
            && (assignment.delegated_session_id == *delegated_session_id
                || (assignment.task_list_id == list.id && assignment.task_id == task.task_id))
    }) {
        return Err(AppError::Conflict(
            "agent task or delegated session already has an unresolved assignment".to_string(),
        ));
    }

    let now = Utc::now();
    let attempt_number = state
        .assignments
        .iter()
        .filter(|assignment| assignment.delegated_session_id == *delegated_session_id)
        .map(|assignment| assignment.attempt_number)
        .max()
        .unwrap_or(0)
        + 1;
    let assignment = AgentTaskAssignment {
        id: crate::domain::entities::AgentTaskAssignmentId::new(),
        delegated_session_id: delegated_session_id.clone(),
        attempt_number,
        caller_agent_run_id: caller_agent_run_id.clone(),
        planned_delegated_agent_run_id: None,
        delegated_agent_run_id: None,
        team_id: None,
        team_member_id: None,
        team_member_generation: None,
        task_list_id: list.id.clone(),
        task_id: task.task_id.clone(),
        delegate_agent_name: delegate_agent_name.to_string(),
        state: crate::domain::entities::AgentTaskAssignmentState::Reserved,
        prior_owner_agent: task.owner_agent.clone(),
        settlement_reason: None,
        completion_metadata: None,
        created_at: now,
        run_bound_at: None,
        settled_at: None,
        updated_at: now,
    };
    {
        let row = &mut state.tasks[task_index];
        row.state = AgentTaskState::Active;
        row.owner_agent = Some(delegate_agent_name.to_string());
        row.version += 1;
        row.updated_at = now;
    }
    state.assignments.push(assignment.clone());
    append_event(
        &mut state,
        &list.id,
        "agent_task.assignment_reserved",
        &task.task_id,
    );
    Ok(Some(AgentTaskAssignmentReservation {
        assignment: view(&state, &assignment)?,
    }))
}

pub(super) fn bind_run(
    repo: &MemoryAgentTaskRepository,
    assignment_id: &AgentTaskAssignmentId,
    delegated_session_id: &DelegatedSessionId,
    delegated_agent_run_id: &AgentRunId,
) -> AppResult<Option<AgentTaskAssignmentView>> {
    let mut state = repo.state.write().unwrap();
    let Some(index) = state
        .assignments
        .iter()
        .position(|assignment| assignment.id == *assignment_id)
    else {
        return Ok(None);
    };
    let assignment = &mut state.assignments[index];
    if assignment.delegated_session_id != *delegated_session_id {
        return Err(AppError::Conflict(
            "delegate assignment does not belong to the requested session".to_string(),
        ));
    }
    if assignment.planned_delegated_agent_run_id.as_ref() != Some(delegated_agent_run_id) {
        return Err(AppError::Conflict(
            "delegate assignment was not planned for the requested run".to_string(),
        ));
    }
    if assignment.state == crate::domain::entities::AgentTaskAssignmentState::Active {
        if assignment.delegated_agent_run_id.as_ref() != Some(delegated_agent_run_id) {
            return Err(AppError::Conflict(
                "active delegate assignment is bound to a different run".to_string(),
            ));
        }
        let assignment = assignment.clone();
        return Ok(Some(view(&state, &assignment)?));
    }
    if assignment.state != crate::domain::entities::AgentTaskAssignmentState::Reserved {
        return Err(AppError::Conflict(
            "only the exact reserved delegate assignment can bind a run".to_string(),
        ));
    }
    let now = Utc::now();
    assignment.delegated_agent_run_id = Some(*delegated_agent_run_id);
    assignment.state = crate::domain::entities::AgentTaskAssignmentState::Active;
    assignment.run_bound_at = Some(now);
    assignment.updated_at = now;
    let assignment = state.assignments[index].clone();
    append_event(
        &mut state,
        &assignment.task_list_id,
        "agent_task.assignment_run_bound",
        &assignment.task_id,
    );
    Ok(Some(view(&state, &assignment)?))
}

pub(super) fn plan_run(
    repo: &MemoryAgentTaskRepository,
    assignment_id: &AgentTaskAssignmentId,
    delegated_session_id: &DelegatedSessionId,
    delegated_agent_run_id: &AgentRunId,
) -> AppResult<Option<AgentTaskAssignmentView>> {
    let mut state = repo.state.write().unwrap();
    let Some(index) = state
        .assignments
        .iter()
        .position(|assignment| assignment.id == *assignment_id)
    else {
        return Ok(None);
    };
    let assignment = &mut state.assignments[index];
    if assignment.delegated_session_id != *delegated_session_id {
        return Err(AppError::Conflict(
            "delegate assignment does not belong to the requested session".to_string(),
        ));
    }
    if assignment.state != crate::domain::entities::AgentTaskAssignmentState::Reserved {
        return Err(AppError::Conflict(
            "only a reserved delegate assignment can plan a run".to_string(),
        ));
    }
    if let Some(planned) = assignment.planned_delegated_agent_run_id.as_ref() {
        if planned != delegated_agent_run_id {
            return Err(AppError::Conflict(
                "delegate assignment is already planned for a different run".to_string(),
            ));
        }
        let assignment = assignment.clone();
        return Ok(Some(view(&state, &assignment)?));
    }
    assignment.planned_delegated_agent_run_id = Some(*delegated_agent_run_id);
    assignment.updated_at = Utc::now();
    let assignment = state.assignments[index].clone();
    append_event(
        &mut state,
        &assignment.task_list_id,
        "agent_task.assignment_run_planned",
        &assignment.task_id,
    );
    Ok(Some(view(&state, &assignment)?))
}

pub(super) fn set_team_identity(
    repo: &MemoryAgentTaskRepository,
    assignment_id: &AgentTaskAssignmentId,
    delegated_session_id: &DelegatedSessionId,
    team_id: &crate::domain::entities::TeamSessionId,
    team_member_id: &crate::domain::entities::TeamMemberId,
    team_member_generation: i64,
) -> AppResult<Option<AgentTaskAssignmentView>> {
    let mut state = repo.state.write().unwrap();
    let Some(index) = state
        .assignments
        .iter()
        .position(|assignment| assignment.id == *assignment_id)
    else {
        return Ok(None);
    };
    let assignment = &mut state.assignments[index];
    if assignment.delegated_session_id != *delegated_session_id {
        return Err(AppError::Conflict(
            "delegate assignment does not belong to the requested session".to_string(),
        ));
    }
    if assignment.state != crate::domain::entities::AgentTaskAssignmentState::Reserved {
        return Err(AppError::Conflict(
            "only a reserved delegate assignment can receive Team authority".to_string(),
        ));
    }
    if assignment.team_id.is_some()
        && (assignment.team_id.as_ref() != Some(team_id)
            || assignment.team_member_id.as_ref() != Some(team_member_id)
            || assignment.team_member_generation != Some(team_member_generation))
    {
        return Err(AppError::Conflict(
            "delegate assignment already belongs to a different Team member generation".to_string(),
        ));
    }
    assignment.team_id = Some(team_id.clone());
    assignment.team_member_id = Some(team_member_id.clone());
    assignment.team_member_generation = Some(team_member_generation);
    assignment.updated_at = Utc::now();
    let assignment = state.assignments[index].clone();
    append_event(
        &mut state,
        &assignment.task_list_id,
        "agent_task.assignment_team_identity_bound",
        &assignment.task_id,
    );
    Ok(Some(view(&state, &assignment)?))
}

pub(super) fn get_unresolved(
    repo: &MemoryAgentTaskRepository,
    delegated_session_id: &DelegatedSessionId,
) -> AppResult<Option<AgentTaskAssignmentView>> {
    let state = repo.state.read().unwrap();
    unresolved_index_for_session(&state, delegated_session_id)
        .map(|index| view(&state, &state.assignments[index]))
        .transpose()
}

pub(super) fn request_completion(
    repo: &MemoryAgentTaskRepository,
    delegated_session_id: &DelegatedSessionId,
    delegated_agent_run_id: &AgentRunId,
    local_scope: &AgentTaskScope,
    completion_metadata: Option<Value>,
) -> AppResult<Option<AgentTaskAssignmentView>> {
    request_intent(
        repo,
        delegated_session_id,
        delegated_agent_run_id,
        crate::domain::entities::AgentTaskAssignmentState::CompletionRequested,
        Some(local_scope),
        completion_metadata,
        None,
    )
}

fn ensure_local_ledger_resolved(
    state: &MemoryAgentTaskState,
    local_scope: &AgentTaskScope,
) -> AppResult<()> {
    let Some(list) = find_list(state, local_scope) else {
        return Ok(());
    };
    let unfinished = state.tasks.iter().filter(|task| {
        task.task_list_id == list.id
            && matches!(task.state, AgentTaskState::Open | AgentTaskState::Active)
    });
    let task_refs = unfinished
        .map(|task| task.task_number.to_string())
        .collect::<Vec<_>>();
    if task_refs.is_empty() {
        Ok(())
    } else {
        Err(AppError::Validation(format!(
            "delegate-local tasks must be resolved before requesting assignment completion: {}",
            task_refs.join(", ")
        )))
    }
}

pub(super) fn request_release(
    repo: &MemoryAgentTaskRepository,
    delegated_session_id: &DelegatedSessionId,
    delegated_agent_run_id: &AgentRunId,
    reason: &str,
) -> AppResult<Option<AgentTaskAssignmentView>> {
    request_intent(
        repo,
        delegated_session_id,
        delegated_agent_run_id,
        crate::domain::entities::AgentTaskAssignmentState::ReleaseRequested,
        None,
        None,
        Some(reason),
    )
}

fn request_intent(
    repo: &MemoryAgentTaskRepository,
    delegated_session_id: &DelegatedSessionId,
    delegated_agent_run_id: &AgentRunId,
    requested_state: crate::domain::entities::AgentTaskAssignmentState,
    local_scope: Option<&AgentTaskScope>,
    completion_metadata: Option<Value>,
    reason: Option<&str>,
) -> AppResult<Option<AgentTaskAssignmentView>> {
    let mut state = repo.state.write().unwrap();
    let Some(index) = unresolved_index_for_session(&state, delegated_session_id) else {
        return Ok(None);
    };
    let assignment = &state.assignments[index];
    if assignment.delegated_agent_run_id.as_ref() != Some(delegated_agent_run_id) {
        return Err(AppError::Conflict(
            "delegate assignment does not belong to the current run".to_string(),
        ));
    }
    if assignment.state == requested_state {
        return Ok(Some(view(&state, assignment)?));
    }
    if assignment.state != crate::domain::entities::AgentTaskAssignmentState::Active {
        return Err(AppError::Conflict(
            "delegate assignment cannot accept that request in its current state".to_string(),
        ));
    }
    if let Some(local_scope) = local_scope {
        ensure_local_ledger_resolved(&state, local_scope)?;
    }
    let assignment = &mut state.assignments[index];
    assignment.state = requested_state;
    assignment.completion_metadata = completion_metadata;
    assignment.settlement_reason = reason.map(str::to_string);
    assignment.updated_at = Utc::now();
    let assignment = assignment.clone();
    let event_type = if requested_state
        == crate::domain::entities::AgentTaskAssignmentState::CompletionRequested
    {
        "agent_task.assignment_completion_requested"
    } else {
        "agent_task.assignment_release_requested"
    };
    append_event(
        &mut state,
        &assignment.task_list_id,
        event_type,
        &assignment.task_id,
    );
    Ok(Some(view(&state, &assignment)?))
}

pub(super) fn settle(
    repo: &MemoryAgentTaskRepository,
    delegated_agent_run_id: &AgentRunId,
    terminal_status: AgentTaskAssignmentTerminalStatus,
    reason: Option<&str>,
) -> AppResult<Option<AgentTaskAssignmentSettlement>> {
    let mut state = repo.state.write().unwrap();
    let Some(index) = state.assignments.iter().position(|assignment| {
        assignment.state.is_unresolved()
            && assignment.delegated_agent_run_id.as_ref() == Some(delegated_agent_run_id)
    }) else {
        return Ok(None);
    };
    settle_index(&mut state, index, terminal_status, reason)
}

pub(super) fn get_for_run(
    repo: &MemoryAgentTaskRepository,
    delegated_agent_run_id: &AgentRunId,
) -> AppResult<Option<AgentTaskAssignmentView>> {
    let state = repo.state.read().unwrap();
    state
        .assignments
        .iter()
        .find(|assignment| {
            assignment.delegated_agent_run_id.as_ref() == Some(delegated_agent_run_id)
        })
        .map(|assignment| view(&state, assignment))
        .transpose()
}

pub(super) fn fail_reserved(
    repo: &MemoryAgentTaskRepository,
    delegated_session_id: &DelegatedSessionId,
    reason: &str,
) -> AppResult<Option<AgentTaskAssignmentSettlement>> {
    let mut state = repo.state.write().unwrap();
    let Some(index) = unresolved_index_for_session(&state, delegated_session_id) else {
        return Ok(None);
    };
    if state.assignments[index].state != crate::domain::entities::AgentTaskAssignmentState::Reserved
    {
        return Err(AppError::Conflict(
            "only a reserved delegate assignment can be failed before launch".to_string(),
        ));
    }
    settle_index(
        &mut state,
        index,
        AgentTaskAssignmentTerminalStatus::Failed,
        Some(reason),
    )
}

pub(super) fn list_unresolved(
    repo: &MemoryAgentTaskRepository,
) -> AppResult<Vec<AgentTaskAssignmentView>> {
    let state = repo.state.read().unwrap();
    state
        .assignments
        .iter()
        .filter(|assignment| assignment.state.is_unresolved())
        .map(|assignment| view(&state, assignment))
        .collect()
}

fn settle_index(
    state: &mut MemoryAgentTaskState,
    assignment_index: usize,
    terminal_status: AgentTaskAssignmentTerminalStatus,
    reason: Option<&str>,
) -> AppResult<Option<AgentTaskAssignmentSettlement>> {
    let assignment = state.assignments[assignment_index].clone();
    let completion_authorized = terminal_status == AgentTaskAssignmentTerminalStatus::Completed
        && assignment.state
            == crate::domain::entities::AgentTaskAssignmentState::CompletionRequested;
    let task_index = state
        .tasks
        .iter()
        .position(|task| {
            task.task_list_id == assignment.task_list_id && task.task_id == assignment.task_id
        })
        .ok_or_else(|| AppError::NotFound("assigned agent task no longer exists".to_string()))?;
    let task = &state.tasks[task_index];
    if task.state != AgentTaskState::Active
        || task.owner_agent.as_deref() != Some(assignment.delegate_agent_name.as_str())
    {
        return Err(AppError::Conflict(
            "assigned task fields changed before terminal settlement".to_string(),
        ));
    }

    let now = Utc::now();
    let (assignment_state, settlement_reason) = if completion_authorized {
        (
            crate::domain::entities::AgentTaskAssignmentState::Completed,
            reason.map(str::to_string),
        )
    } else {
        let state = match terminal_status {
            AgentTaskAssignmentTerminalStatus::Completed => {
                crate::domain::entities::AgentTaskAssignmentState::Released
            }
            AgentTaskAssignmentTerminalStatus::Failed => {
                crate::domain::entities::AgentTaskAssignmentState::Failed
            }
            AgentTaskAssignmentTerminalStatus::Cancelled => {
                crate::domain::entities::AgentTaskAssignmentState::Cancelled
            }
        };
        let default_reason = match terminal_status {
            AgentTaskAssignmentTerminalStatus::Completed => "ended_without_assignment_completion",
            AgentTaskAssignmentTerminalStatus::Failed => "delegated_run_failed",
            AgentTaskAssignmentTerminalStatus::Cancelled => "delegated_run_cancelled",
        };
        (state, Some(reason.unwrap_or(default_reason).to_string()))
    };
    {
        let task = &mut state.tasks[task_index];
        if completion_authorized {
            task.state = AgentTaskState::Done;
            task.completed_at = Some(now);
            if let Some(metadata) = assignment.completion_metadata.clone() {
                task.metadata = merge_agent_task_metadata(task.metadata.clone(), metadata);
            }
        } else {
            task.state = AgentTaskState::Open;
            task.owner_agent = assignment.prior_owner_agent.clone();
            task.completed_at = None;
        }
        task.version += 1;
        task.updated_at = now;
    }
    {
        let stored = &mut state.assignments[assignment_index];
        stored.state = assignment_state;
        stored.settlement_reason = settlement_reason;
        stored.settled_at = Some(now);
        stored.updated_at = now;
    }
    let settled = state.assignments[assignment_index].clone();
    append_event(
        state,
        &settled.task_list_id,
        if completion_authorized {
            "agent_task.assignment_completed"
        } else {
            "agent_task.assignment_reopened"
        },
        &settled.task_id,
    );
    Ok(Some(AgentTaskAssignmentSettlement {
        assignment: view(state, &settled)?,
        task_reopened: !completion_authorized,
        task_completed: completion_authorized,
    }))
}

fn unresolved_index_for_session(
    state: &MemoryAgentTaskState,
    delegated_session_id: &DelegatedSessionId,
) -> Option<usize> {
    state.assignments.iter().position(|assignment| {
        assignment.delegated_session_id == *delegated_session_id && assignment.state.is_unresolved()
    })
}

fn view(
    state: &MemoryAgentTaskState,
    assignment: &AgentTaskAssignment,
) -> AppResult<AgentTaskAssignmentView> {
    let list = state
        .lists
        .iter()
        .find(|list| list.id == assignment.task_list_id)
        .ok_or_else(|| {
            AppError::NotFound("assigned agent task list no longer exists".to_string())
        })?;
    let task = state
        .tasks
        .iter()
        .find(|task| {
            task.task_list_id == assignment.task_list_id && task.task_id == assignment.task_id
        })
        .ok_or_else(|| AppError::NotFound("assigned agent task no longer exists".to_string()))?;
    Ok(AgentTaskAssignmentView {
        assignment: assignment.clone(),
        caller_scope_type: list.scope_type.clone(),
        caller_scope_id: list.scope_id.clone(),
        task: detail_for_row(state, task),
    })
}
