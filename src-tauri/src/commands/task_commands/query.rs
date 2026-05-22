// Query (read-only) handlers for task_commands module

use super::helpers::status_to_label;
use super::types::{
    PlanGroupInfo, StateTransitionResponse, StatusSummary, StatusTransition,
    TaskDependencyGraphResponse, TaskGraphEdge, TaskGraphNode, TaskListResponse, TaskResponse,
    TimelineEvent, TimelineEventType, TimelineEventsResponse,
};
use crate::application::AppState;
use crate::domain::entities::{InternalStatus, ProjectId, TaskId};
use tauri::State;

/// List tasks for a project with pagination support
///
/// # Arguments
/// * `project_id` - The project ID
/// * `statuses` - Optional status filter (array of status strings)
/// * `offset` - Pagination offset (default 0)
/// * `limit` - Page size (default 20)
/// * `include_archived` - Whether to include archived tasks (default false)
/// * `ideation_session_id` - Optional ideation session ID to filter tasks
/// * `execution_plan_id` - Optional execution plan ID to filter tasks (mutually exclusive with ideation_session_id)
/// * `categories` - Optional category filter (array of category strings, OR logic)
///
/// # Returns
/// * `TaskListResponse` - Contains tasks, total count, has_more flag, and offset
#[tauri::command]
pub async fn list_tasks(
    project_id: String,
    statuses: Option<Vec<String>>,
    offset: Option<u32>,
    limit: Option<u32>,
    include_archived: Option<bool>,
    ideation_session_id: Option<String>,
    execution_plan_id: Option<String>,
    categories: Option<Vec<String>>,
    state: State<'_, AppState>,
) -> Result<TaskListResponse, String> {
    let project_id = ProjectId::from_string(project_id);
    let offset = offset.unwrap_or(0);
    let limit = limit.unwrap_or(20);
    let include_archived = include_archived.unwrap_or(false);

    // Parse statuses if provided
    let internal_statuses = if let Some(status_vec) = statuses {
        let mut parsed = Vec::new();
        for status_str in status_vec {
            let status = status_str
                .parse::<InternalStatus>()
                .map_err(|_| format!("Invalid status: {}", status_str))?;
            parsed.push(status);
        }
        if parsed.is_empty() {
            None
        } else {
            Some(parsed)
        }
    } else {
        None
    };

    // Get paginated tasks with session/plan/category filters passed to repository
    let tasks = state
        .task_repo
        .list_paginated(
            &project_id,
            internal_statuses,
            offset,
            limit,
            include_archived,
            ideation_session_id.as_deref(),
            execution_plan_id.as_deref(),
            categories.as_deref(),
        )
        .await
        .map_err(|e| e.to_string())?;

    // Get total count with session/plan filter passed to repository
    let total = state
        .task_repo
        .count_tasks(
            &project_id,
            include_archived,
            ideation_session_id.as_deref(),
            execution_plan_id.as_deref(),
        )
        .await
        .map_err(|e| e.to_string())?;

    // Calculate has_more
    let has_more = (offset + tasks.len() as u32) < total;

    // Convert to response
    let task_responses: Vec<TaskResponse> = tasks.into_iter().map(TaskResponse::from).collect();

    Ok(TaskListResponse {
        tasks: task_responses,
        total,
        has_more,
        offset,
    })
}

/// Get a single task by ID
#[tauri::command]
pub async fn get_task(
    id: String,
    state: State<'_, AppState>,
) -> Result<Option<TaskResponse>, String> {
    let task_id = TaskId::from_string(id);
    state
        .task_repo
        .get_by_id(&task_id)
        .await
        .map(|opt| opt.map(TaskResponse::from))
        .map_err(|e| e.to_string())
}

/// Get the count of archived tasks for a project
///
/// This count is used by the frontend to show an archive access button
/// when archived tasks exist.
///
/// # Arguments
/// * `project_id` - The project ID
/// * `ideation_session_id` - Optional ideation session ID to filter tasks
///
/// # Returns
/// * `u32` - The count of archived tasks
#[tauri::command]
pub async fn get_archived_count(
    project_id: String,
    ideation_session_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<u32, String> {
    let project_id_obj = ProjectId::from_string(project_id);

    state
        .task_repo
        .get_archived_count(&project_id_obj, ideation_session_id.as_deref())
        .await
        .map_err(|e| e.to_string())
}

/// Search tasks by title and description (case-insensitive)
///
/// Searches in both title AND description fields for the query string.
/// Uses server-side search for reliable results across all tasks.
///
/// # Arguments
/// * `project_id` - The project ID to search within
/// * `query` - The search query string
/// * `include_archived` - Whether to include archived tasks in search results (default: false)
/// * `ideation_session_id` - Optional ideation session ID to filter tasks
///
/// # Returns
/// * `Vec<TaskResponse>` - All matching tasks (no pagination - results should be small)
///
/// # Examples
/// ```ignore
/// // Search for "authentication" in title or description
/// search_tasks("proj-123", "authentication", None, None)
///
/// // Search including archived tasks
/// search_tasks("proj-123", "old feature", Some(true), None)
/// ```
#[tauri::command]
pub async fn search_tasks(
    project_id: String,
    query: String,
    include_archived: Option<bool>,
    ideation_session_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<TaskResponse>, String> {
    let project_id_obj = ProjectId::from_string(project_id);
    let include_archived = include_archived.unwrap_or(false);

    // Call repository search method
    let mut tasks = state
        .task_repo
        .search(&project_id_obj, &query, include_archived)
        .await
        .map_err(|e| e.to_string())?;

    // Filter by session_id if provided
    if let Some(ref sid) = ideation_session_id {
        tasks.retain(|t| {
            t.ideation_session_id
                .as_ref()
                .is_some_and(|id| id.as_str() == sid)
        });
    }

    // Convert to response
    let task_responses: Vec<TaskResponse> = tasks.into_iter().map(TaskResponse::from).collect();

    Ok(task_responses)
}

/// Get state transition history for a task
///
/// Returns a chronological list of all status transitions a task has gone through.
/// Used by the StateTimelineNav component for displaying task state history.
///
/// # Arguments
/// * `task_id` - The task ID to get state history for
///
/// # Returns
/// * `Vec<StateTransitionResponse>` - Chronologically ordered list of state transitions
///
/// # Examples
/// ```ignore
/// // Get state history for a completed task
/// // Returns transitions like:
/// // [
/// //   { from_status: null, to_status: "backlog", trigger: "user", timestamp: "..." },
/// //   { from_status: "backlog", to_status: "ready", trigger: "user", timestamp: "..." },
/// //   { from_status: "ready", to_status: "executing", trigger: "agent", timestamp: "..." },
/// //   { from_status: "executing", to_status: "approved", trigger: "reviewer", timestamp: "..." }
/// // ]
/// get_task_state_transitions("task-123")
/// ```
#[tauri::command]
pub async fn get_task_state_transitions(
    task_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<StateTransitionResponse>, String> {
    let task_id_obj = TaskId::from_string(task_id);

    // Get status history from repository
    let transitions = state
        .task_repo
        .get_status_history(&task_id_obj)
        .await
        .map_err(|e| e.to_string())?;

    // Convert domain StatusTransition to StateTransitionResponse
    let responses: Vec<StateTransitionResponse> = transitions
        .into_iter()
        .map(|t| StateTransitionResponse {
            from_status: Some(t.from.as_str().to_string()),
            to_status: t.to.as_str().to_string(),
            trigger: t.trigger,
            timestamp: t.timestamp.to_rfc3339(),
            conversation_id: t.conversation_id,
            agent_run_id: t.agent_run_id,
        })
        .collect();

    Ok(responses)
}

/// Get valid status transitions for a task
///
/// Queries the state machine for valid transitions from the task's current status
/// and maps them to user-friendly labels for display in the status dropdown.
///
/// # Arguments
/// * `task_id` - The task ID to get valid transitions for
///
/// # Returns
/// * `Vec<StatusTransition>` - List of valid transitions with status string and label
///
/// # Examples
/// ```ignore
/// // Get valid transitions for a task in "backlog" status
/// // Returns: [
/// //   { status: "ready", label: "Ready for Work" },
/// //   { status: "cancelled", label: "Cancel" }
/// // ]
/// get_valid_transitions("task-123")
/// ```
#[tauri::command]
pub async fn get_valid_transitions(
    task_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<StatusTransition>, String> {
    // Get the task to check its current status
    let task_id_obj = TaskId::from_string(task_id);
    let task = state
        .task_repo
        .get_by_id(&task_id_obj)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Task not found".to_string())?;

    // Get valid transitions from the state machine
    let valid_transitions = task.internal_status.valid_transitions();

    // Map to user-friendly labels
    let transitions = valid_transitions
        .iter()
        .map(|status| {
            let status_str = status.as_str().to_string();
            let label = status_to_label(*status);
            StatusTransition {
                status: status_str,
                label,
            }
        })
        .collect();

    Ok(transitions)
}

/// Get tasks awaiting review for a project
///
/// Returns tasks in review-related statuses that are awaiting either
/// AI review or human review decision.
///
/// # Arguments
/// * `project_id` - The project ID
///
/// # Returns
/// * `Vec<TaskResponse>` - Tasks in pending_review, reviewing, review_passed, or escalated states
///
/// # Review Status Meanings
/// - `pending_review`: Queued for AI review
/// - `reviewing`: AI review in progress
/// - `review_passed`: AI approved, awaiting human approval
/// - `escalated`: AI escalated, awaiting human decision
#[tauri::command]
pub async fn get_tasks_awaiting_review(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<TaskResponse>, String> {
    let project_id = ProjectId::from_string(project_id);

    // Define the review-related statuses
    let review_statuses = vec![
        InternalStatus::PendingReview,
        InternalStatus::Reviewing,
        InternalStatus::ReviewPassed,
        InternalStatus::Escalated,
    ];

    // Get tasks in review statuses using the existing list_paginated method
    // Use a high limit to get all tasks (no pagination needed for this view)
    let tasks = state
        .task_repo
        .list_paginated(&project_id, Some(review_statuses), 0, 1000, false, None, None, None)
        .await
        .map_err(|e| e.to_string())?;

    // Convert to response
    let task_responses: Vec<TaskResponse> = tasks.into_iter().map(TaskResponse::from).collect();

    Ok(task_responses)
}

/// Get the task dependency graph for a project
///
/// Returns a graph representation of all tasks and their dependencies,
/// including plan groupings, critical path computation, and cycle detection.
///
/// # Arguments
/// * `project_id` - The project ID
/// * `include_archived` - Whether to include archived tasks (default: false)
/// * `session_id` - Optional ideation session ID to filter tasks
/// * `execution_plan_id` - Optional execution plan ID to filter tasks (mutually exclusive with session_id)
///
/// # Returns
/// * `TaskDependencyGraphResponse` - Contains nodes, edges, plan groups, critical path, and cycle info
///
/// # Graph Structure
/// - **Nodes**: One per task with status, tier, and plan info
/// - **Edges**: Dependencies (source blocks target)
/// - **Plan Groups**: Tasks grouped by their originating plan artifact
/// - **Critical Path**: Longest dependency chain (affects project completion time)
#[tauri::command]
pub async fn get_task_dependency_graph(
    project_id: String,
    include_archived: Option<bool>,
    session_id: Option<String>,
    execution_plan_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<TaskDependencyGraphResponse, String> {
    use std::collections::{HashMap, HashSet, VecDeque};

    let project_id_obj = ProjectId::from_string(project_id);
    let include_archived = include_archived.unwrap_or(false);

    // 1. Get all tasks for the project
    let tasks = state
        .task_repo
        .get_by_project(&project_id_obj)
        .await
        .map_err(|e| e.to_string())?;

    // Filter out archived tasks if not requested
    let mut tasks: Vec<_> = if include_archived {
        tasks
    } else {
        tasks
            .into_iter()
            .filter(|t| t.archived_at.is_none())
            .collect()
    };

    // Filter by session_id if provided
    if let Some(ref sid) = session_id {
        tasks.retain(|t| {
            t.ideation_session_id
                .as_ref()
                .is_some_and(|id| id.as_str() == sid)
        });
    }

    // Filter by execution_plan_id if provided
    if let Some(ref ep_id) = execution_plan_id {
        tasks.retain(|t| {
            t.execution_plan_id
                .as_ref()
                .is_some_and(|id| id.as_str() == ep_id)
        });
    }

    // Build task lookup map
    let task_map: HashMap<String, &crate::domain::entities::Task> = tasks
        .iter()
        .map(|t| (t.id.as_str().to_string(), t))
        .collect();
    let task_ids: HashSet<String> = task_map.keys().cloned().collect();

    // 2. Build edges — fetch all blockers in a single batch query
    let task_id_list: Vec<crate::domain::entities::TaskId> =
        tasks.iter().map(|t| t.id.clone()).collect();
    let all_blockers = state
        .task_dependency_repo
        .get_blockers_batch(&task_id_list)
        .await
        .map_err(|e| e.to_string())?;

    let mut edges: Vec<TaskGraphEdge> = Vec::new();
    let mut in_degree: HashMap<String, u32> = HashMap::new();
    let mut out_degree: HashMap<String, u32> = HashMap::new();
    let mut adjacency: HashMap<String, Vec<String>> = HashMap::new(); // source -> targets
    let mut reverse_adjacency: HashMap<String, Vec<String>> = HashMap::new(); // target -> sources

    for task in &tasks {
        let task_id_str = task.id.as_str().to_string();
        in_degree.entry(task_id_str.clone()).or_insert(0);
        out_degree.entry(task_id_str.clone()).or_insert(0);
        adjacency.entry(task_id_str.clone()).or_default();
        reverse_adjacency.entry(task_id_str.clone()).or_default();

        // Get tasks this task depends on (blockers) from the pre-fetched batch
        let blockers = all_blockers
            .get(&task.id)
            .map(|v| v.as_slice())
            .unwrap_or(&[]);

        for blocker_id in blockers {
            let blocker_str = blocker_id.as_str().to_string();
            // Only include edges where both tasks exist in our filtered set
            if task_ids.contains(&blocker_str) {
                edges.push(TaskGraphEdge {
                    source: blocker_str.clone(),
                    target: task_id_str.clone(),
                    is_critical_path: false, // Will be set later
                });
                *in_degree.entry(task_id_str.clone()).or_insert(0) += 1;
                *out_degree.entry(blocker_str.clone()).or_insert(0) += 1;
                adjacency
                    .entry(blocker_str.clone())
                    .or_default()
                    .push(task_id_str.clone());
                reverse_adjacency
                    .entry(task_id_str.clone())
                    .or_default()
                    .push(blocker_str.clone());
            }
        }
    }

    // 3. Compute tiers using Kahn's algorithm (topological sort)
    let mut tier_map: HashMap<String, u32> = HashMap::new();
    let mut remaining_in_degree = in_degree.clone();
    let mut queue: VecDeque<String> = VecDeque::new();
    let mut has_cycles = false;

    // Start with nodes that have no incoming edges
    for (id, &deg) in &remaining_in_degree {
        if deg == 0 {
            queue.push_back(id.clone());
            tier_map.insert(id.clone(), 0);
        }
    }

    let mut processed = 0;
    while let Some(current) = queue.pop_front() {
        processed += 1;
        let current_tier = *tier_map.get(&current).unwrap_or(&0);

        if let Some(targets) = adjacency.get(&current) {
            for target in targets {
                if let Some(deg) = remaining_in_degree.get_mut(target) {
                    *deg -= 1;
                    // Target tier is max of its current tier and current_tier + 1
                    let new_tier = current_tier + 1;
                    tier_map
                        .entry(target.clone())
                        .and_modify(|t| *t = (*t).max(new_tier))
                        .or_insert(new_tier);
                    if *deg == 0 {
                        queue.push_back(target.clone());
                    }
                }
            }
        }
    }

    // If we couldn't process all nodes, there's a cycle
    if processed < tasks.len() {
        has_cycles = true;
        // Assign tier 0 to remaining unprocessed nodes
        for task in &tasks {
            let id = task.id.as_str().to_string();
            tier_map.entry(id).or_insert(0);
        }
    }

    // 4. Compute critical path using DP on longest path
    // critical_path[node] = length of longest path ending at node
    let mut critical_path_length: HashMap<String, u32> = HashMap::new();
    let mut critical_path_parent: HashMap<String, Option<String>> = HashMap::new();

    // Process nodes in topological order (by tier)
    let mut nodes_by_tier: Vec<(String, u32)> =
        tier_map.iter().map(|(k, v)| (k.clone(), *v)).collect();
    nodes_by_tier.sort_by_key(|(_, tier)| *tier);

    for (node, _) in &nodes_by_tier {
        let sources = reverse_adjacency.get(node).cloned().unwrap_or_default();
        if sources.is_empty() {
            // Starting node
            critical_path_length.insert(node.clone(), 1);
            critical_path_parent.insert(node.clone(), None);
        } else {
            // Find longest path from any source
            let mut max_length = 0u32;
            let mut best_parent: Option<String> = None;
            for source in &sources {
                let source_length = *critical_path_length.get(source).unwrap_or(&0);
                if source_length >= max_length {
                    max_length = source_length;
                    best_parent = Some(source.clone());
                }
            }
            critical_path_length.insert(node.clone(), max_length + 1);
            critical_path_parent.insert(node.clone(), best_parent);
        }
    }

    // Find the endpoint of the critical path (node with max length)
    let critical_endpoint = critical_path_length
        .iter()
        .max_by_key(|(_, &len)| len)
        .map(|(id, _)| id.clone());

    // Trace back to build critical path
    let mut critical_path_ids: Vec<String> = Vec::new();
    let mut critical_path_set: HashSet<String> = HashSet::new();
    if let Some(mut current) = critical_endpoint {
        while {
            critical_path_ids.push(current.clone());
            critical_path_set.insert(current.clone());
            if let Some(Some(parent)) = critical_path_parent.get(&current) {
                current = parent.clone();
                true
            } else {
                false
            }
        } {}
    }
    critical_path_ids.reverse();

    // Mark edges on the critical path
    for edge in &mut edges {
        if critical_path_set.contains(&edge.source) && critical_path_set.contains(&edge.target) {
            // Check if this is an adjacent pair in the critical path
            let source_idx = critical_path_ids.iter().position(|x| x == &edge.source);
            let target_idx = critical_path_ids.iter().position(|x| x == &edge.target);
            if let (Some(si), Some(ti)) = (source_idx, target_idx) {
                if ti == si + 1 {
                    edge.is_critical_path = true;
                }
            }
        }
    }

    // 5. Build plan groups by finding proposals that created these tasks
    // Trace: session → proposals → proposal.created_task_id
    // This works even when task.source_proposal_id isn't set.
    let mut plan_groups: Vec<PlanGroupInfo> = Vec::new();

    // Get all sessions for this project
    let sessions = state
        .ideation_session_repo
        .get_by_project(&project_id_obj)
        .await
        .map_err(|e| e.to_string())?;

    for session in sessions {
        // Get all proposals for this session
        let proposals = state
            .task_proposal_repo
            .get_by_session(&session.id)
            .await
            .map_err(|e| e.to_string())?;

        // Collect task IDs from proposals that have created tasks
        let task_ids_in_plan: Vec<String> = proposals
            .iter()
            .filter_map(|p| p.created_task_id.as_ref())
            .filter(|tid| task_ids.contains(tid.as_str())) // Only include tasks in our set
            .map(|tid| tid.as_str().to_string())
            .collect();

        // Skip sessions with no tasks
        if task_ids_in_plan.is_empty() {
            continue;
        }

        let session_id_str = session.id.as_str().to_string();
        let session_title = session.title.clone();
        let plan_artifact_id = session
            .plan_artifact_id
            .as_ref()
            .map(|id| id.as_str().to_string())
            .unwrap_or_else(|| session_id_str.clone());

        // Compute status summary
        let mut summary = StatusSummary::default();
        for task_id in &task_ids_in_plan {
            if let Some(task) = task_map.get(task_id) {
                categorize_status(&task.internal_status, &mut summary);
            }
        }

        // Find execution_plan_id from any task in this group
        let group_execution_plan_id = task_ids_in_plan.iter().find_map(|tid| {
            task_map
                .get(tid)
                .and_then(|t| t.execution_plan_id.as_ref())
                .map(|id| id.as_str().to_string())
        });

        plan_groups.push(PlanGroupInfo {
            plan_artifact_id,
            session_id: session_id_str,
            session_title,
            task_ids: task_ids_in_plan,
            status_summary: summary,
            execution_plan_id: group_execution_plan_id,
        });
    }

    // 5b. Second pass: catch tasks with plan_artifact_id not found via proposal tracing
    // (e.g. merge tasks created directly via task_repo.create())
    {
        let grouped_task_ids: HashSet<String> = plan_groups
            .iter()
            .flat_map(|g| g.task_ids.iter().cloned())
            .collect();

        // Build a lookup from plan_artifact_id -> plan_group index
        let plan_group_index: HashMap<String, usize> = plan_groups
            .iter()
            .enumerate()
            .map(|(i, g)| (g.plan_artifact_id.clone(), i))
            .collect();

        for task in &tasks {
            let task_id_str = task.id.as_str().to_string();
            if grouped_task_ids.contains(&task_id_str) {
                continue;
            }
            if let Some(plan_id) = task
                .plan_artifact_id
                .as_ref()
                .map(|id| id.as_str().to_string())
            {
                if let Some(&group_idx) = plan_group_index.get(&plan_id) {
                    plan_groups[group_idx].task_ids.push(task_id_str);
                    categorize_status(
                        &task.internal_status,
                        &mut plan_groups[group_idx].status_summary,
                    );
                }
            }
        }
    }

    // 5c. Third pass: catch tasks with ideation_session_id but no plan_artifact_id
    // (tasks from sessions without a plan artifact)
    {
        let grouped_task_ids: HashSet<String> = plan_groups
            .iter()
            .flat_map(|g| g.task_ids.iter().cloned())
            .collect();

        let session_group_index: HashMap<String, usize> = plan_groups
            .iter()
            .enumerate()
            .map(|(i, g)| (g.session_id.clone(), i))
            .collect();

        for task in &tasks {
            let task_id_str = task.id.as_str().to_string();
            if grouped_task_ids.contains(&task_id_str) {
                continue;
            }
            if let Some(sid) = task
                .ideation_session_id
                .as_ref()
                .map(|id| id.as_str().to_string())
            {
                if let Some(&idx) = session_group_index.get(&sid) {
                    plan_groups[idx].task_ids.push(task_id_str);
                    categorize_status(&task.internal_status, &mut plan_groups[idx].status_summary);
                }
            }
        }
    }

    // 6. Build nodes
    let nodes: Vec<TaskGraphNode> = tasks
        .iter()
        .map(|task| {
            let task_id_str = task.id.as_str().to_string();
            TaskGraphNode {
                task_id: task_id_str.clone(),
                title: task.title.clone(),
                description: task.description.clone(),
                category: task.category.to_string(),
                internal_status: task.internal_status.as_str().to_string(),
                priority: task.priority,
                in_degree: *in_degree.get(&task_id_str).unwrap_or(&0),
                out_degree: *out_degree.get(&task_id_str).unwrap_or(&0),
                tier: *tier_map.get(&task_id_str).unwrap_or(&0),
                plan_artifact_id: task
                    .plan_artifact_id
                    .as_ref()
                    .map(|id| id.as_str().to_string()),
                source_proposal_id: task
                    .source_proposal_id
                    .as_ref()
                    .map(|id| id.as_str().to_string()),
                execution_plan_id: task
                    .execution_plan_id
                    .as_ref()
                    .map(|id| id.as_str().to_string()),
            }
        })
        .collect();

    Ok(TaskDependencyGraphResponse {
        nodes,
        edges,
        plan_groups,
        critical_path: critical_path_ids,
        has_cycles,
    })
}

/// Helper to categorize a status into the summary buckets
fn categorize_status(status: &InternalStatus, summary: &mut StatusSummary) {
    match status {
        InternalStatus::Backlog => summary.backlog += 1,
        InternalStatus::Ready => summary.ready += 1,
        InternalStatus::Blocked => summary.blocked += 1,
        InternalStatus::Executing | InternalStatus::ReExecuting => summary.executing += 1,
        InternalStatus::QaRefining
        | InternalStatus::QaTesting
        | InternalStatus::QaPassed
        | InternalStatus::QaFailed => summary.qa += 1,
        InternalStatus::PendingReview
        | InternalStatus::Reviewing
        | InternalStatus::ReviewPassed
        | InternalStatus::Escalated
        | InternalStatus::RevisionNeeded => summary.review += 1,
        InternalStatus::PendingMerge
        | InternalStatus::Merging
        | InternalStatus::WaitingOnPr
        | InternalStatus::MergeIncomplete
        | InternalStatus::MergeConflict => summary.merge += 1,
        InternalStatus::Approved | InternalStatus::Merged => summary.completed += 1,
        InternalStatus::Failed | InternalStatus::Cancelled | InternalStatus::Stopped => {
            summary.terminal += 1
        }
        // Paused tasks are in a suspended state (not terminal, can resume)
        InternalStatus::Paused => summary.blocked += 1,
    }
}

/// Get execution timeline events for the task graph view
///
/// Returns chronological timeline of task status changes and plan-level events.
/// Used by the ExecutionTimeline panel in the Task Graph View.
///
/// # Arguments
/// * `project_id` - The project ID
/// * `offset` - Pagination offset (default 0)
/// * `limit` - Page size (default 50)
/// * `session_id` - Optional ideation session ID to filter events
///
/// # Returns
/// * `TimelineEventsResponse` - Events in reverse chronological order (newest first)
///
/// # Event Types
/// - `status_change`: Task transitioned to a new status
/// - `plan_accepted`: A plan was accepted and tasks were created
/// - `plan_completed`: All tasks in a plan reached terminal state
#[tauri::command]
pub async fn get_task_timeline_events(
    project_id: String,
    offset: Option<u32>,
    limit: Option<u32>,
    session_id: Option<String>,
    execution_plan_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<TimelineEventsResponse, String> {
    let project_id_obj = ProjectId::from_string(project_id.clone());
    let offset = offset.unwrap_or(0) as usize;
    let limit = limit.unwrap_or(50) as usize;

    // 1. Get all tasks for the project
    let mut tasks = state
        .task_repo
        .get_by_project(&project_id_obj)
        .await
        .map_err(|e| e.to_string())?;

    // Filter by session_id if provided
    if let Some(ref sid) = session_id {
        tasks.retain(|t| {
            t.ideation_session_id
                .as_ref()
                .is_some_and(|id| id.as_str() == sid)
        });
    }

    // Filter by execution_plan_id if provided
    if let Some(ref ep_id) = execution_plan_id {
        tasks.retain(|t| {
            t.execution_plan_id
                .as_ref()
                .is_some_and(|id| id.as_str() == ep_id)
        });
    }

    // 2. Collect status change events — fetch all histories in a single batch query
    let task_id_list: Vec<crate::domain::entities::TaskId> =
        tasks.iter().map(|t| t.id.clone()).collect();
    let all_history = state
        .task_repo
        .get_status_history_batch(&task_id_list)
        .await
        .map_err(|e| e.to_string())?;

    let mut all_events: Vec<TimelineEvent> = Vec::new();

    for task in &tasks {
        let transitions = all_history
            .get(&task.id)
            .map(|v| v.as_slice())
            .unwrap_or(&[]);

        for (idx, transition) in transitions.iter().enumerate() {
            let description = format_status_change_description(&task.title, transition.to.as_str());

            all_events.push(TimelineEvent {
                id: format!("{}-{}", task.id.as_str(), idx),
                timestamp: transition.timestamp.to_rfc3339(),
                task_id: Some(task.id.as_str().to_string()),
                task_title: Some(task.title.clone()),
                event_type: TimelineEventType::StatusChange,
                from_status: Some(transition.from.as_str().to_string()),
                to_status: Some(transition.to.as_str().to_string()),
                description,
                trigger: Some(transition.trigger.clone()),
                plan_artifact_id: task
                    .plan_artifact_id
                    .as_ref()
                    .map(|id| id.as_str().to_string()),
                session_title: None,
            });
        }
    }

    // 3. Get plan-level events from ideation sessions
    // Find sessions with accepted plans that created tasks
    let plan_artifact_ids: Vec<String> = tasks
        .iter()
        .filter_map(|t| {
            t.plan_artifact_id
                .as_ref()
                .map(|id| id.as_str().to_string())
        })
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    for plan_artifact_id in &plan_artifact_ids {
        // Get the session for this plan
        let sessions = state
            .ideation_session_repo
            .get_by_plan_artifact_id(plan_artifact_id)
            .await
            .map_err(|e| e.to_string())?;

        if let Some(session) = sessions.first() {
            // Find the earliest task created from this plan for the "plan accepted" timestamp
            let plan_tasks: Vec<_> = tasks
                .iter()
                .filter(|t| {
                    t.plan_artifact_id
                        .as_ref()
                        .is_some_and(|id| id.as_str() == plan_artifact_id)
                })
                .collect();

            if let Some(earliest) = plan_tasks.iter().min_by_key(|t| t.created_at) {
                let session_title = session
                    .title
                    .clone()
                    .unwrap_or_else(|| "Untitled Plan".to_string());
                let task_count = plan_tasks.len();

                all_events.push(TimelineEvent {
                    id: format!("plan-accepted-{}", plan_artifact_id),
                    timestamp: earliest.created_at.to_rfc3339(),
                    task_id: None,
                    task_title: None,
                    event_type: TimelineEventType::PlanAccepted,
                    from_status: None,
                    to_status: None,
                    description: format!(
                        "Plan \"{}\" accepted - created {} task{}",
                        session_title,
                        task_count,
                        if task_count == 1 { "" } else { "s" }
                    ),
                    trigger: Some("user".to_string()),
                    plan_artifact_id: Some(plan_artifact_id.clone()),
                    session_title: Some(session_title.clone()),
                });

                // Check if all tasks in this plan are complete (approved, merged, cancelled, or failed)
                let all_complete = plan_tasks.iter().all(|t| {
                    matches!(
                        t.internal_status,
                        InternalStatus::Approved
                            | InternalStatus::Merged
                            | InternalStatus::Failed
                            | InternalStatus::Cancelled
                    )
                });

                if all_complete && !plan_tasks.is_empty() {
                    // Find the latest completion timestamp
                    if let Some(latest) = plan_tasks.iter().filter_map(|t| t.completed_at).max() {
                        let completed_count = plan_tasks
                            .iter()
                            .filter(|t| {
                                matches!(
                                    t.internal_status,
                                    InternalStatus::Approved | InternalStatus::Merged
                                )
                            })
                            .count();

                        all_events.push(TimelineEvent {
                            id: format!("plan-completed-{}", plan_artifact_id),
                            timestamp: latest.to_rfc3339(),
                            task_id: None,
                            task_title: None,
                            event_type: TimelineEventType::PlanCompleted,
                            from_status: None,
                            to_status: None,
                            description: format!(
                                "Plan \"{}\" completed - {} of {} task{} succeeded",
                                session_title,
                                completed_count,
                                task_count,
                                if task_count == 1 { "" } else { "s" }
                            ),
                            trigger: Some("system".to_string()),
                            plan_artifact_id: Some(plan_artifact_id.clone()),
                            session_title: Some(session_title),
                        });
                    }
                }
            }
        }
    }

    // 4. Sort by timestamp descending (newest first)
    all_events.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

    // 5. Apply pagination
    let total = all_events.len() as u32;
    let paginated: Vec<TimelineEvent> = all_events.into_iter().skip(offset).take(limit).collect();
    let has_more = (offset + paginated.len()) < total as usize;

    Ok(TimelineEventsResponse {
        events: paginated,
        total,
        has_more,
    })
}

/// Generate a human-readable description for a status change
fn format_status_change_description(task_title: &str, to_status: &str) -> String {
    let action = match to_status {
        "ready" => "marked ready",
        "blocked" => "blocked",
        "executing" => "started execution",
        "re_executing" => "restarted execution",
        "qa_refining" => "entered QA refinement",
        "qa_testing" => "entered QA testing",
        "qa_passed" => "passed QA",
        "qa_failed" => "failed QA",
        "pending_review" => "queued for review",
        "reviewing" => "review started",
        "review_passed" => "passed AI review",
        "escalated" => "escalated to human",
        "revision_needed" => "needs revision",
        "pending_merge" => "queued for merge",
        "merging" => "merging started",
        "merge_conflict" => "has merge conflict",
        "merged" => "merged",
        "approved" => "approved",
        "failed" => "failed",
        "cancelled" => "cancelled",
        _ => "status changed",
    };

    // Truncate title if too long (char-boundary safe)
    let title = if task_title.chars().count() > 40 {
        let truncated: String = task_title.chars().take(37).collect();
        format!("{truncated}...")
    } else {
        task_title.to_string()
    };

    format!("\"{}\" {}", title, action)
}
