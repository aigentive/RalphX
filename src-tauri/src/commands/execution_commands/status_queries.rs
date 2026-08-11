use super::*;
use crate::commands::execution_task_navigation::resolve_agent_workspace_target_for_task;
use std::collections::HashMap;

/// Get current execution status
/// Phase 82: Optional project_id for per-project scoping.
/// If project_id is None, falls back to active project or aggregates across all projects.
#[tauri::command]
pub async fn get_execution_status(
    project_id: Option<String>,
    active_project_state: State<'_, Arc<ActiveProjectState>>,
    execution_state: State<'_, Arc<ExecutionState>>,
    app_state: State<'_, AppState>,
) -> Result<ExecutionStatusResponse, String> {
    // Sync runtime quota with persisted project settings before returning status
    let project_id = project_id.map(|id| ProjectId::from_string(id));
    let (effective_project_id, _max_concurrent) = sync_quota_from_project(
        project_id,
        &active_project_state,
        &execution_state,
        &app_state,
    )
    .await?;

    // Count queued tasks (tasks in Ready status)
    let mut queued_count = 0u32;

    if let Some(pid) = &effective_project_id {
        // Scoped to single project
        let tasks = app_state
            .task_repo
            .get_by_project(pid)
            .await
            .map_err(|e| e.to_string())?;

        queued_count = tasks
            .iter()
            .filter(|t| t.internal_status == InternalStatus::Ready)
            .count() as u32;
    } else {
        // Aggregate across all projects
        let all_projects = app_state
            .project_repo
            .get_all()
            .await
            .map_err(|e| e.to_string())?;

        for project in &all_projects {
            let tasks = app_state
                .task_repo
                .get_by_project(&project.id)
                .await
                .map_err(|e| e.to_string())?;

            queued_count += tasks
                .iter()
                .filter(|t| t.internal_status == InternalStatus::Ready)
                .count() as u32;
        }
    }

    let queued_message_count =
        count_slot_consuming_queued_messages(effective_project_id.as_ref(), &app_state).await?;

    // Runtime GC pass to prune stale rows on every status poll.
    prune_stale_execution_registry_entries(&app_state, &execution_state).await;

    let registry_entries = app_state.running_agent_registry.list_all().await;

    // Keep execution state synchronized to global execution contexts.
    // Subtract idle interactive slots (processes alive between turns that already
    // freed their execution slot via TurnComplete) to avoid re-inflating the counter.
    let mut total_with_slot = 0usize;
    for (key, _) in &registry_entries {
        let context_type = match ChatContextType::from_str(&key.context_type) {
            Ok(value) => value,
            Err(_) => continue,
        };

        if !uses_execution_slot(context_type) {
            continue;
        }

        if matches!(context_type, ChatContextType::Ideation) {
            total_with_slot += 1;
            continue;
        }

        let task_id = TaskId::from_string(key.context_id.clone());
        let task = match app_state.task_repo.get_by_id(&task_id).await {
            Ok(Some(task)) => task,
            _ => continue,
        };

        if task.archived_at.is_some()
            || !context_matches_running_status_for_gc(context_type, task.internal_status)
        {
            continue;
        }

        total_with_slot += 1;
    }
    let active_count =
        (total_with_slot.saturating_sub(execution_state.interactive_idle_count())) as u32;
    execution_state.set_running_count(active_count);
    let global_running_count = active_count;

    let mut scoped_subjects = Vec::new();
    for (key, _) in registry_entries {
        let context_type = match ChatContextType::from_str(&key.context_type) {
            Ok(value) => value,
            Err(_) => continue,
        };

        if !uses_execution_slot(context_type) {
            continue;
        }

        // Ideation uses session IDs (not task IDs) — look up session for project filtering.
        // Track active (generating) and idle (waiting_for_input) separately.
        if matches!(context_type, ChatContextType::Ideation) {
            let session_id = IdeationSessionId::from_string(key.context_id.clone());
            let session = match app_state.ideation_session_repo.get_by_id(&session_id).await {
                Ok(Some(s)) => s,
                _ => continue, // orphaned registry entry — skip
            };
            if let Some(pid) = &effective_project_id {
                if session.project_id != *pid {
                    continue;
                }
            }
            let slot_key = format!("{}/{}", key.context_type, key.context_id);
            scoped_subjects.push(ScopedExecutionSubject::Ideation {
                project_id: session.project_id,
                is_idle: execution_state.is_interactive_idle(&slot_key),
            });
            continue;
        }

        let task_id = TaskId::from_string(key.context_id);
        let task = match app_state.task_repo.get_by_id(&task_id).await {
            Ok(Some(task)) => task,
            _ => continue,
        };

        if task.archived_at.is_some() {
            continue;
        }

        scoped_subjects.push(ScopedExecutionSubject::Task {
            context_type,
            project_id: task.project_id,
            status: task.internal_status,
        });
    }
    let counts = count_execution_status(scoped_subjects, effective_project_id.as_ref());

    // Count sessions waiting for ideation capacity (have pending_initial_prompt set).
    let ideation_waiting = match &effective_project_id {
        Some(pid) => app_state
            .ideation_session_repo
            .count_pending_sessions_for_project(pid)
            .await
            .unwrap_or(0),
        None => 0,
    };

    let max_concurrent = execution_state.max_concurrent();
    let global_max = execution_state.global_max_concurrent();
    let halt_mode = load_execution_halt_mode(&app_state).await?;

    Ok(build_execution_status_response(ExecutionStatusInput {
        is_paused: execution_state.is_paused(),
        halt_mode: execution_halt_mode_str(halt_mode).to_string(),
        running_count: counts.running_count,
        max_concurrent,
        global_max_concurrent: global_max,
        queued_count,
        queued_message_count,
        provider_blocked: execution_state.is_provider_blocked(),
        provider_blocked_until_epoch: execution_state.provider_blocked_until_epoch(),
        total_project_active: counts.total_project_active,
        global_running_count,
        ideation_active: counts.ideation_active,
        ideation_idle: counts.ideation_idle,
        ideation_waiting,
        ideation_max_project: execution_state.project_ideation_max(),
        ideation_max_global: execution_state.global_ideation_max(),
    }))
}
// ========================================
// Running Processes Query
// ========================================

/// Get all currently running processes (tasks with active execution contexts)
///
/// Returns tasks found in the running agent registry (task_execution/review/merge)
/// with enriched data:
/// - Step progress via StepProgressSummary::from_steps()
/// - Elapsed time from task_state_history
/// - Trigger origin from metadata
/// - Branch name
#[tauri::command]
pub async fn get_running_processes(
    project_id: Option<String>,
    active_project_state: State<'_, Arc<ActiveProjectState>>,
    execution_state: State<'_, Arc<ExecutionState>>,
    state: State<'_, AppState>,
) -> Result<RunningProcessesResponse, String> {
    let (effective_project_id, project_max_concurrent) = sync_quota_from_project(
        project_id.map(ProjectId::from_string),
        &active_project_state,
        &execution_state,
        &state,
    )
    .await?;

    // Keep the registry clean so process rows reflect truly running agents.
    prune_stale_execution_registry_entries(&state, &execution_state).await;

    let mut processes = Vec::new();
    let mut ideation_sessions = Vec::new();
    let mut workspace_sessions = Vec::new();
    let mut seen_task_ids = std::collections::HashSet::new();
    let mut seen_session_ids = std::collections::HashSet::new();
    let mut seen_conversation_ids = std::collections::HashSet::new();
    let mut plan_branch_cache: HashMap<String, Vec<crate::domain::entities::PlanBranch>> =
        HashMap::new();
    let mut agent_workspace_cache: HashMap<
        String,
        Vec<crate::domain::entities::AgentConversationWorkspace>,
    > = HashMap::new();
    let registry_entries = state.running_agent_registry.list_all().await;
    let now = chrono::Utc::now();

    for (key, info) in registry_entries {
        let context_type = match ChatContextType::from_str(&key.context_type) {
            Ok(value) => value,
            Err(_) => continue,
        };

        if context_type == ChatContextType::Project {
            let conversation_id_str = key.context_id.clone();
            if !seen_conversation_ids.insert(conversation_id_str.clone()) {
                continue;
            }
            let conversation_id = ChatConversationId::from_string(conversation_id_str);
            let conversation = match state
                .chat_conversation_repo
                .get_by_id(&conversation_id)
                .await
            {
                Ok(Some(conversation)) => conversation,
                _ => continue,
            };
            if conversation.context_type != ChatContextType::Project {
                continue;
            }
            if let Some(pid) = &effective_project_id {
                if conversation.context_id != pid.as_str() {
                    continue;
                }
            }
            workspace_sessions.push(build_running_workspace_session(
                &conversation,
                info.started_at,
                info.model.clone(),
                now,
            ));
            continue;
        }

        // Collect ideation sessions separately
        if context_type == ChatContextType::Ideation {
            let session_id_str = key.context_id.clone();
            if !seen_session_ids.insert(session_id_str.clone()) {
                continue;
            }
            let session_id = IdeationSessionId(session_id_str.clone());
            if let Ok(Some(session)) = state.ideation_session_repo.get_by_id(&session_id).await {
                if let Some(pid) = &effective_project_id {
                    if session.project_id != *pid {
                        continue;
                    }
                }
                let now = chrono::Utc::now();
                let slot_key = format!("ideation/{}", session_id_str);
                let is_generating = !execution_state.is_interactive_idle(&slot_key);
                ideation_sessions.push(build_running_ideation_session(
                    session_id_str,
                    &session,
                    is_generating,
                    now,
                ));
            }
            continue;
        }

        // Only include task-based execution contexts in the process list
        if !matches!(
            context_type,
            ChatContextType::TaskExecution | ChatContextType::Review | ChatContextType::Merge
        ) {
            continue;
        }

        let task_id = TaskId::from_string(key.context_id);
        let task = match state.task_repo.get_by_id(&task_id).await {
            Ok(Some(task)) => task,
            _ => continue,
        };

        if let Some(pid) = &effective_project_id {
            if task.project_id != *pid {
                continue;
            }
        }

        // Extra guard against races between status transitions and registry updates.
        if !context_matches_running_status_for_gc(context_type, task.internal_status) {
            continue;
        }

        let task_id_str = task.id.as_str().to_string();
        if !seen_task_ids.insert(task_id_str.clone()) {
            continue;
        }

        // Get step progress
        let steps = state
            .task_step_repo
            .get_by_task(&task_id)
            .await
            .map_err(|e| e.to_string())?;

        let step_progress = if !steps.is_empty() {
            Some(StepProgressSummary::from_steps(&task_id, &steps))
        } else {
            None
        };

        // Get elapsed time from status history
        let history = state
            .task_repo
            .get_status_history(&task_id)
            .await
            .map_err(|e| e.to_string())?;

        let elapsed_seconds =
            elapsed_seconds_for_status(&history, task.internal_status, chrono::Utc::now());

        // Get trigger origin
        let trigger_origin = get_trigger_origin(&task);
        let project_key = task.project_id.as_str().to_string();
        if !plan_branch_cache.contains_key(&project_key) {
            let plan_branches = state
                .plan_branch_repo
                .get_by_project_id(&task.project_id)
                .await
                .map_err(|e| e.to_string())?;
            plan_branch_cache.insert(project_key.clone(), plan_branches);
        }
        if !agent_workspace_cache.contains_key(&project_key) {
            let workspaces = state
                .agent_conversation_workspace_repo
                .get_by_project_id(&task.project_id)
                .await
                .map_err(|e| e.to_string())?;
            agent_workspace_cache.insert(project_key.clone(), workspaces);
        }
        let plan_branches = plan_branch_cache
            .get(&project_key)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let agent_workspaces = agent_workspace_cache
            .get(&project_key)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let agent_workspace =
            resolve_agent_workspace_target_for_task(&state, &task, plan_branches, agent_workspaces)
                .await?;

        processes.push(build_running_process_with_agent_workspace(
            &task,
            step_progress,
            elapsed_seconds,
            trigger_origin,
            agent_workspace,
        ));
    }

    let queued_ready_tasks =
        count_ready_tasks_for_lane(effective_project_id.as_ref(), &state).await?;
    let task_queued_messages = count_queued_messages_for_context_types(
        &[
            ChatContextType::TaskExecution,
            ChatContextType::Review,
            ChatContextType::Merge,
        ],
        effective_project_id.as_ref(),
        &state,
    )
    .await?;
    let workspace_waiting =
        crate::application::workspace_capacity::count_queued_workspace_messages(
            &state.message_queue,
            &state.project_repo,
            &state.chat_conversation_repo,
            effective_project_id.as_ref(),
        )
        .await?;
    let ideation_queued_messages = count_queued_messages_for_context_types(
        &[ChatContextType::Ideation],
        effective_project_id.as_ref(),
        &state,
    )
    .await?;
    let pending_ideation_sessions = match &effective_project_id {
        Some(pid) => state
            .ideation_session_repo
            .count_pending_sessions_for_project(pid)
            .await
            .unwrap_or(0),
        None => 0,
    };
    let ideation_waiting = pending_ideation_sessions + ideation_queued_messages;

    let workspace_active =
        count_active_workspace_sessions(&state, effective_project_id.as_ref()).await?;
    let workspace_max = execution_state.workspace_max_concurrent();
    let task_active = processes.len() as u32;
    let ideation_active = ideation_sessions
        .iter()
        .filter(|session| session.is_generating)
        .count() as u32;
    let ideation_idle = (ideation_sessions.len() as u32).saturating_sub(ideation_active);

    let lanes = vec![
        ExecutionLaneUsage {
            lane: "workspaces".to_string(),
            active: workspace_active,
            idle: 0,
            waiting: workspace_waiting,
            max: workspace_max,
            borrowed: workspace_active.saturating_sub(workspace_max),
            priority_rank: 1,
        },
        ExecutionLaneUsage {
            lane: "tasks".to_string(),
            active: task_active,
            idle: 0,
            waiting: queued_ready_tasks + task_queued_messages,
            max: project_max_concurrent,
            borrowed: task_active.saturating_sub(project_max_concurrent),
            priority_rank: 2,
        },
        ExecutionLaneUsage {
            lane: "ideation".to_string(),
            active: ideation_active,
            idle: ideation_idle,
            waiting: ideation_waiting,
            max: execution_state.project_ideation_max(),
            borrowed: ideation_active.saturating_sub(execution_state.project_ideation_max()),
            priority_rank: 3,
        },
    ];

    let capacity = ExecutionCapacitySummary {
        total_active: workspace_active + task_active + ideation_active,
        global_max_concurrent: execution_state.global_max_concurrent(),
        borrowing_enabled: execution_state.allow_ideation_borrow_idle_execution(),
        priority: vec![
            "workspaces".to_string(),
            "tasks".to_string(),
            "ideation".to_string(),
        ],
    };

    Ok(RunningProcessesResponse {
        processes,
        ideation_sessions,
        workspace_sessions,
        lanes,
        capacity,
    })
}

async fn count_ready_tasks_for_lane(
    project_filter: Option<&ProjectId>,
    app_state: &AppState,
) -> Result<u32, String> {
    if let Some(pid) = project_filter {
        let tasks = app_state
            .task_repo
            .get_by_project(pid)
            .await
            .map_err(|e| e.to_string())?;
        return Ok(tasks
            .iter()
            .filter(|task| task.internal_status == InternalStatus::Ready)
            .count() as u32);
    }

    let projects = app_state
        .project_repo
        .get_all()
        .await
        .map_err(|e| e.to_string())?;
    let mut count = 0u32;
    for project in projects {
        let tasks = app_state
            .task_repo
            .get_by_project(&project.id)
            .await
            .map_err(|e| e.to_string())?;
        count += tasks
            .iter()
            .filter(|task| task.internal_status == InternalStatus::Ready)
            .count() as u32;
    }
    Ok(count)
}
