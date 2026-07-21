// Proposal CRUD and management commands

use std::collections::{HashMap, HashSet, VecDeque};
use tauri::{Emitter, State};

use crate::application::{AppState, CreateProposalOptions, UpdateProposalOptions, UpdateSource};
use crate::domain::entities::{
    BusinessValueFactor, ComplexityFactor, CriticalPathFactor, DependencyFactor, DependencyGraph,
    DependencyGraphEdge, DependencyGraphNode, IdeationSessionId, Priority, PriorityAssessment,
    PriorityAssessmentFactors, ProposalCategory, TaskProposal, TaskProposalId, UserHintFactor,
};
use crate::http_server::helpers::{
    archive_proposal_impl, assert_session_mutable, create_proposal_impl, update_proposal_impl,
};

use super::ideation_commands_types::{
    CreateProposalInput, PriorityAssessmentResponse, TaskProposalResponse, UpdateProposalInput,
};

// ============================================================================
// Proposal Management Commands
// ============================================================================

/// Create a new task proposal
#[tauri::command]
pub async fn create_task_proposal(
    input: CreateProposalInput,
    state: State<'_, AppState>,
) -> Result<TaskProposalResponse, String> {
    let session_id = IdeationSessionId::from_string(input.session_id);

    let category: ProposalCategory = input
        .category
        .parse()
        .map_err(|_| format!("Invalid category: {}", input.category))?;
    let priority: Priority = input
        .priority
        .map(|p| p.parse().unwrap_or(Priority::Medium))
        .unwrap_or(Priority::Medium);
    let steps = input
        .steps
        .map(|s| serde_json::to_string(&s).unwrap_or_default());
    let acceptance_criteria = input
        .acceptance_criteria
        .map(|ac| serde_json::to_string(&ac).unwrap_or_default());
    let affected_paths = input
        .affected_paths
        .map(|paths| serde_json::to_string(&paths).unwrap_or_default());

    let options = CreateProposalOptions {
        title: input.title,
        description: input.description,
        category,
        suggested_priority: priority,
        steps,
        acceptance_criteria,
        affected_paths,
        estimated_complexity: input.complexity,
        target_project: input.target_project,
        depends_on: input.depends_on,
        expected_proposal_count: input.expected_proposal_count,
    };

    // Delegates all checks, INSERT, event emission, and dep analysis to shared impl
    create_proposal_impl(state.inner(), session_id, options)
        .await
        .map(|(proposal, dep_errors, auto_accept_triggered)| {
            let mut response = TaskProposalResponse::from(proposal);
            response.dependency_errors = dep_errors;
            response.auto_accept_triggered = auto_accept_triggered;
            response
        })
        .map_err(|e| e.to_string())
}

/// Get a task proposal by ID
#[tauri::command]
pub async fn get_task_proposal(
    id: String,
    state: State<'_, AppState>,
) -> Result<Option<TaskProposalResponse>, String> {
    let proposal_id = TaskProposalId::from_string(id);
    state
        .task_proposal_repo
        .get_by_id(&proposal_id)
        .await
        .map(|opt| opt.map(TaskProposalResponse::from))
        .map_err(|e| e.to_string())
}

/// List all proposals for a session
#[tauri::command]
pub async fn list_session_proposals(
    session_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<TaskProposalResponse>, String> {
    let session_id = IdeationSessionId::from_string(session_id);
    state
        .task_proposal_repo
        .get_by_session(&session_id)
        .await
        .map(|proposals| {
            proposals
                .into_iter()
                .map(TaskProposalResponse::from)
                .collect()
        })
        .map_err(|e| e.to_string())
}

/// Update a task proposal
#[tauri::command]
pub async fn update_task_proposal(
    id: String,
    input: UpdateProposalInput,
    state: State<'_, AppState>,
) -> Result<TaskProposalResponse, String> {
    let proposal_id = TaskProposalId::from_string(id);

    let category = input
        .category
        .map(|s| s.parse::<ProposalCategory>())
        .transpose()
        .map_err(|_| "Invalid category".to_string())?;
    let user_priority = input
        .user_priority
        .map(|s| s.parse::<Priority>())
        .transpose()
        .map_err(|_| "Invalid priority".to_string())?;
    let steps = input
        .steps
        .map(|s| Some(serde_json::to_string(&s).unwrap_or_default()));
    let acceptance_criteria = input
        .acceptance_criteria
        .map(|ac| Some(serde_json::to_string(&ac).unwrap_or_default()));
    let affected_paths = input
        .affected_paths
        .map(|paths| Some(serde_json::to_string(&paths).unwrap_or_default()));

    let options = UpdateProposalOptions {
        title: input.title,
        description: input.description.map(Some),
        category,
        steps,
        acceptance_criteria,
        affected_paths,
        user_priority,
        estimated_complexity: input.complexity,
        target_project: input.target_project.map(Some),
        source: UpdateSource::TauriIpc,
        add_depends_on: input.add_depends_on,
        add_blocks: input.add_blocks,
    };

    // Delegates all checks (including assert_session_mutable), UPDATE, event emission,
    // user_modified tracking, and dep analysis to shared impl
    update_proposal_impl(state.inner(), &proposal_id, options)
        .await
        .map(|(updated, dep_errors)| {
            let mut response = TaskProposalResponse::from(updated);
            response.dependency_errors = dep_errors;
            response
        })
        .map_err(|e| e.to_string())
}

/// Delete a task proposal
#[tauri::command]
pub async fn delete_task_proposal(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let proposal_id = TaskProposalId::from_string(id);

    // Delegates all checks (including assert_session_mutable), ARCHIVE, event emission,
    // and dep analysis to shared impl
    archive_proposal_impl(state.inner(), proposal_id)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Toggle proposal selection state
#[tauri::command]
pub async fn toggle_proposal_selection(
    id: String,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    let proposal_id = TaskProposalId::from_string(id);

    // Get current state
    let proposal = state
        .task_proposal_repo
        .get_by_id(&proposal_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Proposal not found".to_string())?;

    // Guard: reject mutations on Archived/Accepted sessions
    let session = state
        .ideation_session_repo
        .get_by_id(&proposal.session_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Session {} not found", proposal.session_id))?;
    assert_session_mutable(&session).map_err(|e| e.to_string())?;

    let new_selected = !proposal.selected;

    state
        .task_proposal_repo
        .update_selection(&proposal_id, new_selected)
        .await
        .map_err(|e| e.to_string())?;

    // Fetch updated proposal and emit event
    if let Some(app_handle) = &state.app_handle {
        if let Ok(Some(updated_proposal)) = state.task_proposal_repo.get_by_id(&proposal_id).await {
            let response = TaskProposalResponse::from(updated_proposal);
            let _ = app_handle.emit(
                "proposal:updated",
                serde_json::json!({
                    "proposal": response
                }),
            );
        }
    }

    Ok(new_selected)
}

/// Set proposal selection state
#[tauri::command]
pub async fn set_proposal_selection(
    id: String,
    selected: bool,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let proposal_id = TaskProposalId::from_string(id);

    // Guard: reject mutations on Archived/Accepted sessions
    let proposal = state
        .task_proposal_repo
        .get_by_id(&proposal_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Proposal not found".to_string())?;
    let session = state
        .ideation_session_repo
        .get_by_id(&proposal.session_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Session {} not found", proposal.session_id))?;
    assert_session_mutable(&session).map_err(|e| e.to_string())?;

    state
        .task_proposal_repo
        .update_selection(&proposal_id, selected)
        .await
        .map_err(|e| e.to_string())?;

    // Fetch updated proposal and emit event
    if let Some(app_handle) = &state.app_handle {
        if let Ok(Some(updated_proposal)) = state.task_proposal_repo.get_by_id(&proposal_id).await {
            let response = TaskProposalResponse::from(updated_proposal);
            let _ = app_handle.emit(
                "proposal:updated",
                serde_json::json!({
                    "proposal": response
                }),
            );
        }
    }

    Ok(())
}

/// Reorder proposals within a session
#[tauri::command]
pub async fn reorder_proposals(
    session_id: String,
    proposal_ids: Vec<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let session_id = IdeationSessionId::from_string(session_id.clone());

    // Guard: reject mutations on Archived/Accepted sessions
    let session = state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Session {} not found", session_id))?;
    assert_session_mutable(&session).map_err(|e| e.to_string())?;

    let proposal_ids: Vec<TaskProposalId> = proposal_ids
        .into_iter()
        .map(TaskProposalId::from_string)
        .collect();

    state
        .task_proposal_repo
        .reorder(&session_id, proposal_ids)
        .await
        .map_err(|e| e.to_string())?;

    // Emit event with updated proposals so frontend can update its state
    if let Some(app_handle) = &state.app_handle {
        // Fetch all proposals for this session with their new order
        if let Ok(proposals) = state.task_proposal_repo.get_by_session(&session_id).await {
            let responses: Vec<TaskProposalResponse> = proposals
                .into_iter()
                .map(TaskProposalResponse::from)
                .collect();
            let _ = app_handle.emit(
                "proposals:reordered",
                serde_json::json!({
                    "sessionId": session_id.as_str(),
                    "proposals": responses
                }),
            );
        }
    }

    Ok(())
}

/// Assess priority for a single proposal
#[tauri::command]
pub async fn assess_proposal_priority(
    id: String,
    state: State<'_, AppState>,
) -> Result<PriorityAssessmentResponse, String> {
    let proposal_id = TaskProposalId::from_string(id);

    let proposal = state
        .task_proposal_repo
        .get_by_id(&proposal_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Proposal not found".to_string())?;

    // Build the dependency graph for the session
    let graph = build_dependency_graph(&proposal.session_id, &state)
        .await
        .map_err(|e| e.to_string())?;

    // Calculate the assessment
    let assessment = calculate_proposal_assessment(&proposal, &graph, &state)
        .await
        .map_err(|e| e.to_string())?;

    // Store the result
    state
        .task_proposal_repo
        .update_priority(&proposal_id, &assessment)
        .await
        .map_err(|e| e.to_string())?;

    // Emit event to frontend
    if let Some(app_handle) = &state.app_handle {
        let _ = app_handle.emit(
            "proposal:priority_assessed",
            serde_json::json!({
                "proposalId": assessment.proposal_id.as_str(),
                "priority": assessment.suggested_priority.to_string(),
                "score": assessment.priority_score,
                "reason": assessment.priority_reason
            }),
        );
    }

    Ok(PriorityAssessmentResponse {
        proposal_id: assessment.proposal_id.as_str().to_string(),
        priority: assessment.suggested_priority.to_string(),
        score: assessment.priority_score,
        reason: assessment.priority_reason,
    })
}

/// Assess priority for all proposals in a session
#[tauri::command]
pub async fn assess_all_priorities(
    session_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<PriorityAssessmentResponse>, String> {
    let session_id = IdeationSessionId::from_string(session_id.clone());

    // Build the dependency graph once for the session
    let graph = build_dependency_graph(&session_id, &state)
        .await
        .map_err(|e| e.to_string())?;

    // Get all proposals
    let proposals = state
        .task_proposal_repo
        .get_by_session(&session_id)
        .await
        .map_err(|e| e.to_string())?;

    // Assess each proposal and update the database
    let mut assessments = Vec::with_capacity(proposals.len());
    for proposal in &proposals {
        let assessment = calculate_proposal_assessment(proposal, &graph, &state)
            .await
            .map_err(|e| e.to_string())?;

        // Store the result
        state
            .task_proposal_repo
            .update_priority(&proposal.id, &assessment)
            .await
            .map_err(|e| e.to_string())?;

        assessments.push(PriorityAssessmentResponse {
            proposal_id: assessment.proposal_id.as_str().to_string(),
            priority: assessment.suggested_priority.to_string(),
            score: assessment.priority_score,
            reason: assessment.priority_reason,
        });
    }

    // Emit event to frontend for batch assessment
    if let Some(app_handle) = &state.app_handle {
        let _ = app_handle.emit(
            "session:priorities_assessed",
            serde_json::json!({
                "sessionId": session_id.as_str(),
                "count": assessments.len()
            }),
        );
    }

    Ok(assessments)
}

// ============================================================================
// Helper Functions for Priority Assessment
// ============================================================================

/// Build a dependency graph for a session
async fn build_dependency_graph(
    session_id: &IdeationSessionId,
    state: &State<'_, AppState>,
) -> Result<DependencyGraph, crate::error::AppError> {
    // Get all proposals for the session
    let proposals = state.task_proposal_repo.get_by_session(session_id).await?;

    // Get all dependencies for the session
    let dependencies = state
        .proposal_dependency_repo
        .get_all_for_session(session_id)
        .await?;

    // Build adjacency lists
    // from_map: proposal_id -> list of proposals it depends on
    // to_map: proposal_id -> list of proposals that depend on it (dependents)
    let mut from_map: HashMap<TaskProposalId, Vec<TaskProposalId>> = HashMap::new();
    let mut to_map: HashMap<TaskProposalId, Vec<TaskProposalId>> = HashMap::new();

    for (from, to, _reason) in &dependencies {
        from_map.entry(from.clone()).or_default().push(to.clone());
        to_map.entry(to.clone()).or_default().push(from.clone());
    }

    // Build nodes with degree counts
    let mut nodes = Vec::new();
    for proposal in &proposals {
        let in_degree = from_map.get(&proposal.id).map(|v| v.len()).unwrap_or(0);
        let out_degree = to_map.get(&proposal.id).map(|v| v.len()).unwrap_or(0);
        let node = DependencyGraphNode::new(proposal.id.clone(), &proposal.title)
            .with_in_degree(in_degree)
            .with_out_degree(out_degree);
        nodes.push(node);
    }

    // Build edges
    // TODO: Task 5 will add reason to DependencyGraphEdge
    let edges: Vec<DependencyGraphEdge> = dependencies
        .iter()
        .map(|(from, to, _reason)| DependencyGraphEdge::new(from.clone(), to.clone()))
        .collect();

    // Detect cycles using DFS
    let cycles = detect_cycles(&proposals, &from_map);

    // Find critical path (longest path through the DAG)
    let critical_path = if cycles.is_empty() {
        find_critical_path(&proposals, &from_map)
    } else {
        Vec::new() // Can't compute critical path with cycles
    };

    let mut graph = DependencyGraph::with_nodes_and_edges(nodes, edges);
    graph.set_critical_path(critical_path);
    graph.set_cycles(cycles);

    Ok(graph)
}

/// Calculate priority assessment for a single proposal
async fn calculate_proposal_assessment(
    proposal: &TaskProposal,
    graph: &DependencyGraph,
    state: &State<'_, AppState>,
) -> Result<PriorityAssessment, crate::error::AppError> {
    // Get the number of dependents (tasks that this proposal blocks)
    let blocks_count = state
        .proposal_dependency_repo
        .count_dependents(&proposal.id)
        .await? as i32;

    // Calculate dependency factor
    let dependency_factor = DependencyFactor::calculate(blocks_count);

    // Calculate critical path factor
    let is_on_critical_path = graph.is_on_critical_path(&proposal.id);
    let path_length = if is_on_critical_path {
        graph.critical_path_length() as i32
    } else {
        0
    };
    let critical_path_factor = CriticalPathFactor::calculate(is_on_critical_path, path_length);

    // Calculate business value factor from title and description
    let text = format!(
        "{} {}",
        proposal.title,
        proposal.description.as_deref().unwrap_or("")
    );
    let business_value_factor = BusinessValueFactor::calculate(&text);

    // Calculate complexity factor
    let complexity_factor = ComplexityFactor::calculate(proposal.estimated_complexity);

    // Calculate user hint factor from title and description
    let user_hint_factor = UserHintFactor::calculate(&text);

    // Build factors container
    let factors = PriorityAssessmentFactors {
        dependency_factor,
        critical_path_factor,
        business_value_factor,
        complexity_factor,
        user_hint_factor,
    };

    // Create assessment (this calculates total score and suggested priority)
    let assessment = PriorityAssessment::new(proposal.id.clone(), factors);

    Ok(assessment)
}

/// Detect cycles in the dependency graph using DFS
fn detect_cycles(
    proposals: &[TaskProposal],
    from_map: &HashMap<TaskProposalId, Vec<TaskProposalId>>,
) -> Vec<Vec<TaskProposalId>> {
    let mut cycles = Vec::new();
    let mut visited = HashSet::new();
    let mut rec_stack = HashSet::new();
    let mut path = Vec::new();

    for proposal in proposals {
        if !visited.contains(&proposal.id) {
            dfs_detect_cycle(
                &proposal.id,
                from_map,
                &mut visited,
                &mut rec_stack,
                &mut path,
                &mut cycles,
            );
        }
    }

    cycles
}

/// DFS helper for cycle detection
fn dfs_detect_cycle(
    node: &TaskProposalId,
    from_map: &HashMap<TaskProposalId, Vec<TaskProposalId>>,
    visited: &mut HashSet<TaskProposalId>,
    rec_stack: &mut HashSet<TaskProposalId>,
    path: &mut Vec<TaskProposalId>,
    cycles: &mut Vec<Vec<TaskProposalId>>,
) {
    visited.insert(node.clone());
    rec_stack.insert(node.clone());
    path.push(node.clone());

    if let Some(neighbors) = from_map.get(node) {
        for neighbor in neighbors {
            if !visited.contains(neighbor) {
                dfs_detect_cycle(neighbor, from_map, visited, rec_stack, path, cycles);
            } else if rec_stack.contains(neighbor) {
                // Found a cycle - extract it from the path
                if let Some(start_idx) = path.iter().position(|n| n == neighbor) {
                    let cycle: Vec<TaskProposalId> = path[start_idx..].to_vec();
                    cycles.push(cycle);
                }
            }
        }
    }

    path.pop();
    rec_stack.remove(node);
}

/// Find the critical path (longest path) in the DAG using topological sort + DP
fn find_critical_path(
    proposals: &[TaskProposal],
    from_map: &HashMap<TaskProposalId, Vec<TaskProposalId>>,
) -> Vec<TaskProposalId> {
    if proposals.is_empty() {
        return Vec::new();
    }

    // Build reverse map (to_map) for topological sort
    // to_map: proposal_id -> list of proposals that depend on this (get unblocked when this completes)
    let mut to_map: HashMap<TaskProposalId, Vec<TaskProposalId>> = HashMap::new();
    let mut in_degree: HashMap<TaskProposalId, usize> = HashMap::new();

    // Initialize all nodes with zero in-degree
    for proposal in proposals {
        in_degree.insert(proposal.id.clone(), 0);
    }

    // Build reverse adjacency and count in-degrees
    for (from, deps) in from_map {
        for to in deps {
            to_map.entry(to.clone()).or_default().push(from.clone());
            *in_degree.entry(from.clone()).or_default() += 1;
        }
    }

    // Topological sort using Kahn's algorithm
    let mut queue: VecDeque<TaskProposalId> = VecDeque::new();
    for (id, &degree) in &in_degree {
        if degree == 0 {
            queue.push_back(id.clone());
        }
    }

    let mut topo_order = Vec::new();
    while let Some(node) = queue.pop_front() {
        topo_order.push(node.clone());

        if let Some(neighbors) = to_map.get(&node) {
            for neighbor in neighbors {
                if let Some(degree) = in_degree.get_mut(neighbor) {
                    *degree -= 1;
                    if *degree == 0 {
                        queue.push_back(neighbor.clone());
                    }
                }
            }
        }
    }

    // If we couldn't process all nodes, there's a cycle
    if topo_order.len() != proposals.len() {
        return Vec::new();
    }

    // DP to find longest path
    let mut dist: HashMap<TaskProposalId, i32> = HashMap::new();
    let mut prev: HashMap<TaskProposalId, Option<TaskProposalId>> = HashMap::new();

    for id in &topo_order {
        dist.insert(id.clone(), 0);
        prev.insert(id.clone(), None);
    }

    // Process nodes in topological order
    for node in &topo_order {
        if let Some(neighbors) = to_map.get(node) {
            for neighbor in neighbors {
                let new_dist = dist.get(node).unwrap_or(&0) + 1;
                if new_dist > *dist.get(neighbor).unwrap_or(&0) {
                    dist.insert(neighbor.clone(), new_dist);
                    prev.insert(neighbor.clone(), Some(node.clone()));
                }
            }
        }
    }

    // Find the node with maximum distance (end of critical path)
    let mut max_dist = 0;
    let mut end_node: Option<TaskProposalId> = topo_order.first().cloned();

    for (id, &d) in &dist {
        if d > max_dist {
            max_dist = d;
            end_node = Some(id.clone());
        }
    }

    // Reconstruct the path from end to start
    let mut path = Vec::new();
    let mut current = end_node;

    while let Some(node) = current {
        path.push(node.clone());
        current = prev.get(&node).and_then(|p| p.clone());
    }

    path.reverse();
    path
}
