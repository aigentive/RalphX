use std::collections::HashMap;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use tracing::error;

use crate::application::proposal_generation_progress::{
    write_proposal_generation_progress, ProposalGenerationProgressTransition,
};
use crate::domain::entities::{IdeationSessionId, TaskProposalId};
use crate::http_server::types::{AnalyzeDependenciesResponse, HttpServerState};

/// Analyze dependencies for a session - returns full graph with critical path, cycles, etc.
/// Used by chat agent to provide intelligent dependency recommendations
pub async fn analyze_session_dependencies(
    State(state): State<HttpServerState>,
    Path(session_id): Path<String>,
) -> Result<Json<AnalyzeDependenciesResponse>, StatusCode> {
    use crate::domain::entities::{DependencyGraph, DependencyGraphEdge, DependencyGraphNode};
    use crate::http_server::types::{
        AnalyzeDependenciesResponse, DependencyAnalysisSummary, DependencyEdgeResponse,
        DependencyNodeResponse,
    };

    let session_id = IdeationSessionId::from_string(session_id.clone());

    // Get all proposals for the session
    let proposals = state
        .app_state
        .task_proposal_repo
        .get_by_session(&session_id)
        .await
        .map_err(|e| {
            error!(
                "Failed to get proposals for session {}: {}",
                session_id.as_str(),
                e
            );
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    // Get all dependencies for the session
    let dependencies = state
        .app_state
        .proposal_dependency_repo
        .get_all_for_session(&session_id)
        .await
        .map_err(|e| {
            error!(
                "Failed to get dependencies for session {}: {}",
                session_id.as_str(),
                e
            );
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    // Build adjacency lists
    let mut from_map: HashMap<TaskProposalId, Vec<TaskProposalId>> = HashMap::new();
    let mut to_map: HashMap<TaskProposalId, Vec<TaskProposalId>> = HashMap::new();

    for (from, to, _reason) in &dependencies {
        from_map.entry(from.clone()).or_default().push(to.clone());
        to_map.entry(to.clone()).or_default().push(from.clone());
    }

    // Build nodes with degree counts
    let mut nodes: Vec<DependencyGraphNode> = Vec::new();
    for proposal in &proposals {
        let in_degree = from_map.get(&proposal.id).map(|v| v.len()).unwrap_or(0);
        let out_degree = to_map.get(&proposal.id).map(|v| v.len()).unwrap_or(0);
        let node = DependencyGraphNode::new(proposal.id.clone(), &proposal.title)
            .with_in_degree(in_degree)
            .with_out_degree(out_degree);
        nodes.push(node);
    }

    // Build edges for internal graph operations (reason stored separately in response)
    let edges: Vec<DependencyGraphEdge> = dependencies
        .iter()
        .map(|(from, to, _reason)| DependencyGraphEdge::new(from.clone(), to.clone()))
        .collect();

    // Simple cycle detection using DFS
    let cycles = detect_cycles_simple(&proposals, &from_map);

    // Critical path calculation (longest path)
    let critical_path = if cycles.is_empty() {
        find_critical_path_simple(&proposals, &from_map, &to_map)
    } else {
        Vec::new()
    };

    let mut graph = DependencyGraph::with_nodes_and_edges(nodes.clone(), edges);
    graph.set_critical_path(critical_path.clone());
    graph.set_cycles(cycles.clone());

    // Calculate summary
    let root_count = nodes.iter().filter(|n| n.is_root()).count();
    let leaf_count = nodes.iter().filter(|n| n.is_leaf()).count();

    // Convert to response types
    let response_nodes: Vec<DependencyNodeResponse> = nodes
        .iter()
        .map(|n| DependencyNodeResponse {
            id: n.proposal_id.to_string(),
            title: n.title.clone(),
            in_degree: n.in_degree,
            out_degree: n.out_degree,
            is_root: n.is_root(),
            is_blocker: n.is_blocker(),
        })
        .collect();

    // Build response edges directly from dependencies to include reason
    let response_edges: Vec<DependencyEdgeResponse> = dependencies
        .iter()
        .map(|(from, to, reason)| DependencyEdgeResponse {
            from: from.to_string(),
            to: to.to_string(),
            reason: reason.clone(),
        })
        .collect();

    let response_critical_path: Vec<String> =
        critical_path.iter().map(|id| id.to_string()).collect();

    let response_cycles = if cycles.is_empty() {
        None
    } else {
        Some(
            cycles
                .iter()
                .map(|cycle| cycle.iter().map(|id| id.to_string()).collect())
                .collect(),
        )
    };

    let summary = DependencyAnalysisSummary {
        total_proposals: proposals.len(),
        root_count,
        leaf_count,
        max_depth: critical_path.len(),
    };

    // Mark dependencies as acknowledged: agent reviewed the graph, satisfying the finalize gate.
    // Best-effort — don't fail the response if the flag update fails.
    if let Err(e) = state
        .app_state
        .ideation_session_repo
        .set_dependencies_acknowledged(session_id.as_str())
        .await
    {
        error!(
            "Failed to set dependencies_acknowledged for session {}: {}",
            session_id.as_str(),
            e
        );
    }
    write_proposal_generation_progress(
        &state.app_state,
        &session_id,
        ProposalGenerationProgressTransition::AnalyzingDependencies,
    )
    .await
    .map_err(|e| {
        error!(
            "Failed to write dependency-analysis progress for session {}: {}",
            session_id.as_str(),
            e
        );
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(AnalyzeDependenciesResponse {
        nodes: response_nodes,
        edges: response_edges,
        critical_path: response_critical_path,
        critical_path_length: critical_path.len(),
        has_cycles: !cycles.is_empty(),
        cycles: response_cycles,
        message: None,
        summary,
    }))
}

/// Simple cycle detection using DFS
fn detect_cycles_simple(
    proposals: &[crate::domain::entities::TaskProposal],
    from_map: &HashMap<TaskProposalId, Vec<TaskProposalId>>,
) -> Vec<Vec<TaskProposalId>> {
    use std::collections::HashSet;

    let mut cycles = Vec::new();
    let mut visited = HashSet::new();
    let mut rec_stack = HashSet::new();
    let mut path = Vec::new();

    for proposal in proposals {
        if !visited.contains(&proposal.id) {
            dfs_detect_cycle_simple(
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

fn dfs_detect_cycle_simple(
    node: &TaskProposalId,
    from_map: &HashMap<TaskProposalId, Vec<TaskProposalId>>,
    visited: &mut std::collections::HashSet<TaskProposalId>,
    rec_stack: &mut std::collections::HashSet<TaskProposalId>,
    path: &mut Vec<TaskProposalId>,
    cycles: &mut Vec<Vec<TaskProposalId>>,
) {
    visited.insert(node.clone());
    rec_stack.insert(node.clone());
    path.push(node.clone());

    if let Some(neighbors) = from_map.get(node) {
        for neighbor in neighbors {
            if !visited.contains(neighbor) {
                dfs_detect_cycle_simple(neighbor, from_map, visited, rec_stack, path, cycles);
            } else if rec_stack.contains(neighbor) {
                // Found a cycle
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

/// Find critical path (longest path) using topological sort + DP
fn find_critical_path_simple(
    proposals: &[crate::domain::entities::TaskProposal],
    from_map: &HashMap<TaskProposalId, Vec<TaskProposalId>>,
    to_map: &HashMap<TaskProposalId, Vec<TaskProposalId>>,
) -> Vec<TaskProposalId> {
    use std::collections::{HashMap as StdHashMap, VecDeque};

    if proposals.is_empty() {
        return Vec::new();
    }

    // Calculate in-degrees
    // For topological sort, in_degree[X] = number of prerequisites for X
    // So in_degree[X] = from_map[X].len() (number of things X depends on)
    let mut in_degree: StdHashMap<TaskProposalId, usize> = StdHashMap::new();
    for proposal in proposals {
        let deps_count = from_map.get(&proposal.id).map(|v| v.len()).unwrap_or(0);
        in_degree.insert(proposal.id.clone(), deps_count);
    }

    // Kahn's algorithm for topological sort
    let mut queue: VecDeque<TaskProposalId> = VecDeque::new();
    for (id, &degree) in &in_degree {
        if degree == 0 {
            queue.push_back(id.clone());
        }
    }

    let mut topo_order = Vec::new();
    let mut remaining_degree = in_degree.clone();

    while let Some(node) = queue.pop_front() {
        topo_order.push(node.clone());

        // For each proposal that depends on this node, decrease its in_degree
        if let Some(dependents) = to_map.get(&node) {
            for dependent in dependents {
                if let Some(degree) = remaining_degree.get_mut(dependent) {
                    *degree -= 1;
                    if *degree == 0 {
                        queue.push_back(dependent.clone());
                    }
                }
            }
        }
    }

    // If not all nodes processed, there's a cycle - return empty
    if topo_order.len() != proposals.len() {
        return Vec::new();
    }

    // DP for longest path
    let mut dist: StdHashMap<TaskProposalId, i32> = StdHashMap::new();
    let mut prev: StdHashMap<TaskProposalId, Option<TaskProposalId>> = StdHashMap::new();

    for id in &topo_order {
        dist.insert(id.clone(), 0);
        prev.insert(id.clone(), None);
    }

    // Process in topological order
    for node in &topo_order {
        if let Some(dependents) = to_map.get(node) {
            for dependent in dependents {
                let new_dist = dist.get(node).unwrap_or(&0) + 1;
                if new_dist > *dist.get(dependent).unwrap_or(&0) {
                    dist.insert(dependent.clone(), new_dist);
                    prev.insert(dependent.clone(), Some(node.clone()));
                }
            }
        }
    }

    // Find end node (max distance)
    let mut max_dist = 0;
    let mut end_node: Option<TaskProposalId> = topo_order.first().cloned();

    for (id, &d) in &dist {
        if d > max_dist {
            max_dist = d;
            end_node = Some(id.clone());
        }
    }

    // Reconstruct path
    let mut path = Vec::new();
    let mut current = end_node;

    while let Some(node) = current {
        path.push(node.clone());
        current = prev.get(&node).and_then(|p| p.clone());
    }

    path.reverse();
    path
}
