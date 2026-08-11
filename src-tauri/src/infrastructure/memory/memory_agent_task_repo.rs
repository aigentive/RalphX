use std::collections::HashSet;
use std::sync::RwLock;

use async_trait::async_trait;
use chrono::Utc;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::domain::entities::{
    merge_agent_task_metadata, AgentRunId, AgentTaskAssignment, AgentTaskAssignmentId,
    AgentTaskAssignmentReservation, AgentTaskAssignmentSettlement,
    AgentTaskAssignmentTerminalStatus, AgentTaskAssignmentView, AgentTaskCreate, AgentTaskDetail,
    AgentTaskId, AgentTaskList, AgentTaskListId, AgentTaskListSummary, AgentTaskMutationResult,
    AgentTaskPatch, AgentTaskScope, AgentTaskState, AgentTaskStateChange, AgentTaskSummary,
    DelegatedSessionId,
};
use crate::domain::repositories::{AgentTaskListOptions, AgentTaskRepository};
use crate::error::{AppError, AppResult};

mod assignments;

#[derive(Clone)]
struct AgentTaskRow {
    task_id: AgentTaskId,
    task_list_id: AgentTaskListId,
    task_number: i64,
    title: String,
    details: String,
    active_label: Option<String>,
    owner_agent: Option<String>,
    state: AgentTaskState,
    metadata: Option<Value>,
    version: i64,
    created_at: chrono::DateTime<Utc>,
    updated_at: chrono::DateTime<Utc>,
    completed_at: Option<chrono::DateTime<Utc>>,
}

#[derive(Clone)]
struct AgentTaskEventRow {
    task_list_id: AgentTaskListId,
    seq: i64,
}

#[derive(Default)]
struct MemoryAgentTaskState {
    lists: Vec<AgentTaskList>,
    tasks: Vec<AgentTaskRow>,
    dependencies: HashSet<(AgentTaskListId, AgentTaskId, AgentTaskId)>,
    events: Vec<AgentTaskEventRow>,
    assignments: Vec<AgentTaskAssignment>,
}

pub struct MemoryAgentTaskRepository {
    state: RwLock<MemoryAgentTaskState>,
}

impl MemoryAgentTaskRepository {
    pub fn new() -> Self {
        Self {
            state: RwLock::new(MemoryAgentTaskState::default()),
        }
    }
}

impl Default for MemoryAgentTaskRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AgentTaskRepository for MemoryAgentTaskRepository {
    async fn create_task(
        &self,
        scope: &AgentTaskScope,
        input: AgentTaskCreate,
    ) -> AppResult<AgentTaskMutationResult> {
        validate_title_and_details(&input.title, &input.details)?;
        let mut state = self.state.write().unwrap();
        let list_id = ensure_list(&mut state, scope);
        let now = Utc::now();
        let task_number = next_task_number(&mut state, &list_id);
        let row = AgentTaskRow {
            task_id: AgentTaskId::new(),
            task_list_id: list_id.clone(),
            task_number,
            title: input.title,
            details: input.details,
            active_label: input.active_label,
            owner_agent: input.owner_agent,
            state: AgentTaskState::Open,
            metadata: input.metadata,
            version: 1,
            created_at: now,
            updated_at: now,
            completed_at: None,
        };
        state.tasks.push(row.clone());

        for blocker_ref in input.blocked_by {
            let blocker = resolve_task_id(&state, &list_id, &blocker_ref)?;
            add_dependency(&mut state, &list_id, &blocker, &row.task_id)?;
        }
        for blocked_ref in input.blocks {
            let blocked = resolve_task_id(&state, &list_id, &blocked_ref)?;
            add_dependency(&mut state, &list_id, &row.task_id, &blocked)?;
        }
        append_event(&mut state, &list_id, "agent_task.created", &row.task_id);

        Ok(AgentTaskMutationResult {
            task: detail_for_row(&state, &row),
            changed_fields: vec!["created".to_string()],
            state_change: None,
        })
    }

    async fn get_task(
        &self,
        scope: &AgentTaskScope,
        task_ref: &str,
    ) -> AppResult<Option<AgentTaskDetail>> {
        let state = self.state.read().unwrap();
        let Some(list) = find_list(&state, scope) else {
            return Ok(None);
        };
        Ok(find_task(&state, &list.id, task_ref).map(|row| detail_for_row(&state, row)))
    }

    async fn list_tasks(
        &self,
        scope: &AgentTaskScope,
        options: AgentTaskListOptions,
    ) -> AppResult<Vec<AgentTaskSummary>> {
        let state = self.state.read().unwrap();
        let Some(list) = find_list(&state, scope) else {
            return Ok(Vec::new());
        };
        Ok(list_task_summaries_for_list(&state, &list.id, options))
    }

    async fn list_task_lists(
        &self,
        scope: &AgentTaskScope,
    ) -> AppResult<Vec<AgentTaskListSummary>> {
        let state = self.state.read().unwrap();
        let mut lists = state
            .lists
            .iter()
            .filter(|list| list.scope_type == scope.scope_type && list.scope_id == scope.scope_id)
            .map(|list| list_summary_for_list(&state, list))
            .collect::<Vec<_>>();
        lists.sort_by_key(|list| std::cmp::Reverse(list.list_sequence));
        Ok(lists)
    }

    async fn list_tasks_for_list(
        &self,
        scope: &AgentTaskScope,
        list_id: &AgentTaskListId,
        options: AgentTaskListOptions,
    ) -> AppResult<Vec<AgentTaskSummary>> {
        let state = self.state.read().unwrap();
        let Some(list) = find_list_by_id(&state, scope, list_id) else {
            return Ok(Vec::new());
        };
        Ok(list_task_summaries_for_list(&state, &list.id, options))
    }

    async fn update_task(
        &self,
        scope: &AgentTaskScope,
        task_ref: &str,
        patch: AgentTaskPatch,
    ) -> AppResult<Option<AgentTaskMutationResult>> {
        let mut state = self.state.write().unwrap();
        let Some(list) = find_list(&state, scope).cloned() else {
            return Ok(None);
        };
        let Some(index) = find_task_index(&state, &list.id, task_ref) else {
            return Ok(None);
        };
        assignments::ensure_mutation_allowed(
            &state,
            &list.id,
            &state.tasks[index].task_id,
            &patch,
        )?;

        let mut changed_fields = Vec::new();
        let mut state_change = None;
        {
            let row = &mut state.tasks[index];
            if let Some(title) = patch.title {
                if title.trim().is_empty() {
                    return Err(AppError::Validation(
                        "agent task title is required".to_string(),
                    ));
                }
                if row.title != title {
                    row.title = title;
                    changed_fields.push("title".to_string());
                }
            }
            if let Some(details) = patch.details {
                if details.trim().is_empty() {
                    return Err(AppError::Validation(
                        "agent task details are required".to_string(),
                    ));
                }
                if row.details != details {
                    row.details = details;
                    changed_fields.push("details".to_string());
                }
            }
            if let Some(active_label) = patch.active_label {
                if row.active_label != active_label {
                    row.active_label = active_label;
                    changed_fields.push("active_label".to_string());
                }
            }
            if let Some(owner_agent) = patch.owner_agent {
                if row.owner_agent != owner_agent {
                    row.owner_agent = owner_agent;
                    changed_fields.push("owner_agent".to_string());
                }
            }
            if let Some(next_state) = patch.state {
                if row.state != next_state {
                    let previous = row.state;
                    row.state = next_state;
                    if next_state == AgentTaskState::Done {
                        row.completed_at = Some(Utc::now());
                    } else {
                        row.completed_at = None;
                    }
                    state_change = Some(AgentTaskStateChange {
                        from: previous,
                        to: next_state,
                    });
                    changed_fields.push("state".to_string());
                }
            }
            if let Some(metadata_patch) = patch.metadata_patch {
                row.metadata = merge_agent_task_metadata(row.metadata.clone(), metadata_patch);
                changed_fields.push("metadata".to_string());
            }
        }

        let row_id = state.tasks[index].task_id.clone();
        for blocker_ref in patch.add_blocked_by {
            let blocker = resolve_task_id(&state, &list.id, &blocker_ref)?;
            add_dependency(&mut state, &list.id, &blocker, &row_id)?;
            changed_fields.push("dependencies".to_string());
        }
        for blocked_ref in patch.add_blocks {
            let blocked = resolve_task_id(&state, &list.id, &blocked_ref)?;
            add_dependency(&mut state, &list.id, &row_id, &blocked)?;
            changed_fields.push("dependencies".to_string());
        }
        for blocker_ref in patch.remove_blocked_by {
            let blocker = resolve_task_id(&state, &list.id, &blocker_ref)?;
            remove_dependency(&mut state, &list.id, &blocker, &row_id);
            changed_fields.push("dependencies".to_string());
        }
        for blocked_ref in patch.remove_blocks {
            let blocked = resolve_task_id(&state, &list.id, &blocked_ref)?;
            remove_dependency(&mut state, &list.id, &row_id, &blocked);
            changed_fields.push("dependencies".to_string());
        }
        changed_fields.sort();
        changed_fields.dedup();
        if !changed_fields.is_empty() {
            let now = Utc::now();
            let row = &mut state.tasks[index];
            row.version += 1;
            row.updated_at = now;
            append_event(&mut state, &list.id, "agent_task.updated", &row_id);
        }
        let row = state.tasks[index].clone();
        Ok(Some(AgentTaskMutationResult {
            task: detail_for_row(&state, &row),
            changed_fields,
            state_change,
        }))
    }

    async fn reserve_assignment(
        &self,
        scope: &AgentTaskScope,
        task_ref: &str,
        delegated_session_id: &DelegatedSessionId,
        caller_agent_run_id: &AgentRunId,
        delegate_agent_name: &str,
    ) -> AppResult<Option<AgentTaskAssignmentReservation>> {
        assignments::reserve(
            self,
            scope,
            task_ref,
            delegated_session_id,
            caller_agent_run_id,
            delegate_agent_name,
        )
    }

    async fn bind_assignment_run(
        &self,
        assignment_id: &AgentTaskAssignmentId,
        delegated_session_id: &DelegatedSessionId,
        delegated_agent_run_id: &AgentRunId,
    ) -> AppResult<Option<AgentTaskAssignmentView>> {
        assignments::bind_run(
            self,
            assignment_id,
            delegated_session_id,
            delegated_agent_run_id,
        )
    }

    async fn plan_assignment_run(
        &self,
        assignment_id: &AgentTaskAssignmentId,
        delegated_session_id: &DelegatedSessionId,
        delegated_agent_run_id: &AgentRunId,
    ) -> AppResult<Option<AgentTaskAssignmentView>> {
        assignments::plan_run(
            self,
            assignment_id,
            delegated_session_id,
            delegated_agent_run_id,
        )
    }

    async fn set_assignment_team_identity(
        &self,
        assignment_id: &AgentTaskAssignmentId,
        delegated_session_id: &DelegatedSessionId,
        team_id: &crate::domain::entities::TeamSessionId,
        team_member_id: &crate::domain::entities::TeamMemberId,
        team_member_generation: i64,
    ) -> AppResult<Option<AgentTaskAssignmentView>> {
        assignments::set_team_identity(
            self,
            assignment_id,
            delegated_session_id,
            team_id,
            team_member_id,
            team_member_generation,
        )
    }

    async fn get_unresolved_assignment(
        &self,
        delegated_session_id: &DelegatedSessionId,
    ) -> AppResult<Option<AgentTaskAssignmentView>> {
        assignments::get_unresolved(self, delegated_session_id)
    }

    async fn request_assignment_completion(
        &self,
        delegated_session_id: &DelegatedSessionId,
        delegated_agent_run_id: &AgentRunId,
        local_scope: &AgentTaskScope,
        completion_metadata: Option<Value>,
    ) -> AppResult<Option<AgentTaskAssignmentView>> {
        assignments::request_completion(
            self,
            delegated_session_id,
            delegated_agent_run_id,
            local_scope,
            completion_metadata,
        )
    }

    async fn request_assignment_release(
        &self,
        delegated_session_id: &DelegatedSessionId,
        delegated_agent_run_id: &AgentRunId,
        reason: &str,
    ) -> AppResult<Option<AgentTaskAssignmentView>> {
        assignments::request_release(self, delegated_session_id, delegated_agent_run_id, reason)
    }

    async fn settle_assignment_for_run(
        &self,
        delegated_agent_run_id: &AgentRunId,
        terminal_status: AgentTaskAssignmentTerminalStatus,
        reason: Option<&str>,
    ) -> AppResult<Option<AgentTaskAssignmentSettlement>> {
        assignments::settle(self, delegated_agent_run_id, terminal_status, reason)
    }

    async fn get_assignment_for_run(
        &self,
        delegated_agent_run_id: &AgentRunId,
    ) -> AppResult<Option<AgentTaskAssignmentView>> {
        assignments::get_for_run(self, delegated_agent_run_id)
    }

    async fn fail_reserved_assignment(
        &self,
        delegated_session_id: &DelegatedSessionId,
        reason: &str,
    ) -> AppResult<Option<AgentTaskAssignmentSettlement>> {
        assignments::fail_reserved(self, delegated_session_id, reason)
    }

    async fn list_unresolved_assignments(&self) -> AppResult<Vec<AgentTaskAssignmentView>> {
        assignments::list_unresolved(self)
    }
}

fn validate_title_and_details(title: &str, details: &str) -> AppResult<()> {
    if title.trim().is_empty() {
        return Err(AppError::Validation(
            "agent task title is required".to_string(),
        ));
    }
    if details.trim().is_empty() {
        return Err(AppError::Validation(
            "agent task details are required".to_string(),
        ));
    }
    Ok(())
}

fn ensure_list(state: &mut MemoryAgentTaskState, scope: &AgentTaskScope) -> AgentTaskListId {
    if let Some(list) = find_list(state, scope) {
        let list_id = list.id.clone();
        let next_sequence = list.list_sequence + 1;
        if list_should_roll_over(state, &list_id) {
            return create_list(state, scope, next_sequence);
        }
        return list_id;
    }
    create_list(state, scope, 1)
}

fn create_list(
    state: &mut MemoryAgentTaskState,
    scope: &AgentTaskScope,
    list_sequence: i64,
) -> AgentTaskListId {
    let now = Utc::now();
    let list = AgentTaskList {
        id: AgentTaskListId::new(),
        project_id: scope.project_id.clone(),
        scope_type: scope.scope_type.clone(),
        scope_id: scope.scope_id.clone(),
        list_sequence,
        name: None,
        created_by_agent: scope.actor_agent.clone(),
        next_task_number: 1,
        created_at: now,
        updated_at: now,
    };
    let id = list.id.clone();
    state.lists.push(list);
    id
}

fn find_list<'a>(
    state: &'a MemoryAgentTaskState,
    scope: &AgentTaskScope,
) -> Option<&'a AgentTaskList> {
    state
        .lists
        .iter()
        .filter(|list| list.scope_type == scope.scope_type && list.scope_id == scope.scope_id)
        .max_by_key(|list| list.list_sequence)
}

fn find_list_by_id<'a>(
    state: &'a MemoryAgentTaskState,
    scope: &AgentTaskScope,
    list_id: &AgentTaskListId,
) -> Option<&'a AgentTaskList> {
    state.lists.iter().find(|list| {
        list.id == *list_id
            && list.scope_type == scope.scope_type
            && list.scope_id == scope.scope_id
    })
}

fn list_should_roll_over(state: &MemoryAgentTaskState, list_id: &AgentTaskListId) -> bool {
    let mut has_tasks = false;
    let mut has_actionable = false;
    for task in state
        .tasks
        .iter()
        .filter(|task| task.task_list_id == *list_id)
    {
        has_tasks = true;
        if !task.state.is_resolved() {
            has_actionable = true;
            break;
        }
    }
    has_tasks && !has_actionable
}

fn list_summary_for_list(
    state: &MemoryAgentTaskState,
    list: &AgentTaskList,
) -> AgentTaskListSummary {
    let mut summary = AgentTaskListSummary {
        list_id: list.id.clone(),
        list_sequence: list.list_sequence,
        task_count: 0,
        open_count: 0,
        active_count: 0,
        done_count: 0,
        dropped_count: 0,
        created_at: list.created_at,
        updated_at: list.updated_at,
    };
    for task in state
        .tasks
        .iter()
        .filter(|task| task.task_list_id == list.id)
    {
        summary.task_count += 1;
        match task.state {
            AgentTaskState::Open => summary.open_count += 1,
            AgentTaskState::Active => summary.active_count += 1,
            AgentTaskState::Done => summary.done_count += 1,
            AgentTaskState::Dropped => summary.dropped_count += 1,
        }
    }
    summary
}

fn list_task_summaries_for_list(
    state: &MemoryAgentTaskState,
    list_id: &AgentTaskListId,
    options: AgentTaskListOptions,
) -> Vec<AgentTaskSummary> {
    let mut rows = state
        .tasks
        .iter()
        .filter(|row| row.task_list_id == *list_id)
        .filter(|row| options.include_done || !row.state.is_resolved())
        .collect::<Vec<_>>();
    rows.sort_by_key(|row| row.task_number);
    rows.into_iter()
        .map(|row| summary_for_row(state, row))
        .collect()
}

fn next_task_number(state: &mut MemoryAgentTaskState, list_id: &AgentTaskListId) -> i64 {
    let list = state
        .lists
        .iter_mut()
        .find(|list| list.id == *list_id)
        .expect("agent task list should exist");
    let next = list.next_task_number;
    list.next_task_number += 1;
    list.updated_at = Utc::now();
    next
}

fn find_task_index(
    state: &MemoryAgentTaskState,
    list_id: &AgentTaskListId,
    task_ref: &str,
) -> Option<usize> {
    state.tasks.iter().position(|row| {
        row.task_list_id == *list_id
            && (row.task_id.as_str() == task_ref || row.task_number.to_string() == task_ref)
    })
}

fn find_task<'a>(
    state: &'a MemoryAgentTaskState,
    list_id: &AgentTaskListId,
    task_ref: &str,
) -> Option<&'a AgentTaskRow> {
    find_task_index(state, list_id, task_ref).map(|index| &state.tasks[index])
}

fn resolve_task_id(
    state: &MemoryAgentTaskState,
    list_id: &AgentTaskListId,
    task_ref: &str,
) -> AppResult<AgentTaskId> {
    find_task(state, list_id, task_ref)
        .map(|row| row.task_id.clone())
        .ok_or_else(|| AppError::Validation(format!("agent task dependency not found: {task_ref}")))
}

fn add_dependency(
    state: &mut MemoryAgentTaskState,
    list_id: &AgentTaskListId,
    blocker_id: &AgentTaskId,
    blocked_id: &AgentTaskId,
) -> AppResult<()> {
    if blocker_id == blocked_id {
        return Err(AppError::Validation(
            "agent task dependency cannot reference itself".to_string(),
        ));
    }
    if has_path(state, list_id, blocked_id, blocker_id) {
        return Err(AppError::Validation(
            "agent task dependency would create a cycle".to_string(),
        ));
    }
    state
        .dependencies
        .insert((list_id.clone(), blocker_id.clone(), blocked_id.clone()));
    Ok(())
}

fn remove_dependency(
    state: &mut MemoryAgentTaskState,
    list_id: &AgentTaskListId,
    blocker_id: &AgentTaskId,
    blocked_id: &AgentTaskId,
) {
    state
        .dependencies
        .remove(&(list_id.clone(), blocker_id.clone(), blocked_id.clone()));
}

fn has_path(
    state: &MemoryAgentTaskState,
    list_id: &AgentTaskListId,
    start: &AgentTaskId,
    target: &AgentTaskId,
) -> bool {
    let mut visited = HashSet::new();
    let mut stack = vec![start.clone()];
    while let Some(current) = stack.pop() {
        if current == *target {
            return true;
        }
        if !visited.insert(current.clone()) {
            continue;
        }
        for (dep_list_id, blocker, blocked) in &state.dependencies {
            if dep_list_id == list_id && blocker == &current {
                stack.push(blocked.clone());
            }
        }
    }
    false
}

fn task_number_for_id(
    state: &MemoryAgentTaskState,
    list_id: &AgentTaskListId,
    task_id: &AgentTaskId,
) -> String {
    state
        .tasks
        .iter()
        .find(|row| row.task_list_id == *list_id && row.task_id == *task_id)
        .map(|row| row.task_number.to_string())
        .unwrap_or_else(|| task_id.to_string())
}

fn blockers_for_row(state: &MemoryAgentTaskState, row: &AgentTaskRow) -> Vec<AgentTaskId> {
    state
        .dependencies
        .iter()
        .filter(|(list_id, _, blocked)| list_id == &row.task_list_id && blocked == &row.task_id)
        .map(|(_, blocker, _)| blocker.clone())
        .collect()
}

fn blocked_by_row(state: &MemoryAgentTaskState, row: &AgentTaskRow) -> Vec<AgentTaskId> {
    state
        .dependencies
        .iter()
        .filter(|(list_id, blocker, _)| list_id == &row.task_list_id && blocker == &row.task_id)
        .map(|(_, _, blocked)| blocked.clone())
        .collect()
}

fn unresolved_blockers_for_row(
    state: &MemoryAgentTaskState,
    row: &AgentTaskRow,
) -> Vec<AgentTaskId> {
    blockers_for_row(state, row)
        .into_iter()
        .filter(|task_id| {
            state
                .tasks
                .iter()
                .find(|candidate| {
                    candidate.task_list_id == row.task_list_id && candidate.task_id == *task_id
                })
                .map(|candidate| !candidate.state.is_resolved())
                .unwrap_or(false)
        })
        .collect()
}

fn detail_for_row(state: &MemoryAgentTaskState, row: &AgentTaskRow) -> AgentTaskDetail {
    let blocked_by = blockers_for_row(state, row)
        .iter()
        .map(|task_id| task_number_for_id(state, &row.task_list_id, task_id))
        .collect::<Vec<_>>();
    let unresolved_blocked_by = unresolved_blockers_for_row(state, row)
        .iter()
        .map(|task_id| task_number_for_id(state, &row.task_list_id, task_id))
        .collect::<Vec<_>>();
    let blocks = blocked_by_row(state, row)
        .iter()
        .map(|task_id| task_number_for_id(state, &row.task_list_id, task_id))
        .collect::<Vec<_>>();
    AgentTaskDetail {
        task_id: row.task_id.clone(),
        task_number: row.task_number,
        title: row.title.clone(),
        details: row.details.clone(),
        active_label: row.active_label.clone(),
        owner_agent: row.owner_agent.clone(),
        state: row.state,
        metadata: row.metadata.clone(),
        blocked_by,
        unresolved_blocked_by,
        blocks,
        version: row.version,
        created_at: row.created_at,
        updated_at: row.updated_at,
        completed_at: row.completed_at,
    }
}

fn summary_for_row(state: &MemoryAgentTaskState, row: &AgentTaskRow) -> AgentTaskSummary {
    let detail = detail_for_row(state, row);
    let availability = detail.availability().to_string();
    AgentTaskSummary {
        task_id: detail.task_id,
        task_number: detail.task_number,
        title: detail.title,
        state: detail.state,
        owner_agent: detail.owner_agent,
        blocked_by: detail.unresolved_blocked_by,
        blocks: detail.blocks,
        availability,
        updated_at: detail.updated_at,
    }
}

fn append_event(
    state: &mut MemoryAgentTaskState,
    list_id: &AgentTaskListId,
    event_type: &str,
    task_id: &AgentTaskId,
) {
    let seq = state
        .events
        .iter()
        .filter(|event| event.task_list_id == *list_id)
        .map(|event| event.seq)
        .max()
        .unwrap_or(0)
        + 1;
    let _payload = json!({
        "event_type": event_type,
        "task_id": task_id.as_str(),
        "event_id": Uuid::new_v4().to_string(),
    });
    state.events.push(AgentTaskEventRow {
        task_list_id: list_id.clone(),
        seq,
    });
}

#[cfg(test)]
#[path = "memory_agent_task_assignment_repo_tests.rs"]
mod assignment_tests;
#[cfg(test)]
#[path = "memory_agent_task_repo_tests.rs"]
mod tests;
