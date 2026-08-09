// Dependency analysis commands and graph building utilities

use ralphx_events::emit_serialized;
use std::collections::{HashMap, HashSet, VecDeque};
use tauri::State;
use tracing::warn;

use crate::application::AppState;
use crate::domain::entities::{
    DependencyGraph, DependencyGraphEdge, DependencyGraphNode, IdeationSessionId, TaskProposal,
    TaskProposalId,
};

use super::ideation_commands_types::DependencyGraphResponse;

// ============================================================================
// Dependency Commands
// ============================================================================

/// Remove a dependency between proposals
#[tauri::command]
pub async fn remove_proposal_dependency(
    proposal_id: String,
    depends_on_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let from_id = TaskProposalId::from_string(proposal_id.clone());
    let to_id = TaskProposalId::from_string(depends_on_id.clone());

    state
        .proposal_dependency_repo
        .remove_dependency(&from_id, &to_id)
        .await
        .map_err(|e| e.to_string())?;

    // Emit event to frontend
    let _ = emit_serialized(
        state.events.as_ref(),
        "dependency:removed",
        &serde_json::json!({
            "proposalId": proposal_id,
            "dependsOnId": depends_on_id
        }),
    );

    Ok(())
}

/// Get dependencies for a proposal
#[tauri::command]
pub async fn get_proposal_dependencies(
    proposal_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<String>, String> {
    let proposal_id = TaskProposalId::from_string(proposal_id);

    state
        .proposal_dependency_repo
        .get_dependencies(&proposal_id)
        .await
        .map(|deps| deps.into_iter().map(|id| id.as_str().to_string()).collect())
        .map_err(|e| e.to_string())
}

/// Get dependents for a proposal (proposals that depend on this one)
#[tauri::command]
pub async fn get_proposal_dependents(
    proposal_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<String>, String> {
    let proposal_id = TaskProposalId::from_string(proposal_id);

    state
        .proposal_dependency_repo
        .get_dependents(&proposal_id)
        .await
        .map(|deps| deps.into_iter().map(|id| id.as_str().to_string()).collect())
        .map_err(|e| e.to_string())
}

/// Analyze dependencies for a session and return the dependency graph
#[tauri::command]
pub async fn analyze_dependencies(
    session_id: String,
    state: State<'_, AppState>,
) -> Result<DependencyGraphResponse, String> {
    let session_id = IdeationSessionId::from_string(session_id);

    analyze_dependencies_for_session(state.inner(), &session_id).await
}

/// Analyze dependencies for a session and mark the session as dependency-reviewed.
#[doc(hidden)]
pub async fn analyze_dependencies_for_session(
    state: &AppState,
    session_id: &IdeationSessionId,
) -> Result<DependencyGraphResponse, String> {
    // Get proposals for this session
    let proposals = state
        .task_proposal_repo
        .get_by_session(session_id)
        .await
        .map_err(|e| e.to_string())?;

    // Get all dependencies for this session
    let deps = state
        .proposal_dependency_repo
        .get_all_for_session(session_id)
        .await
        .map_err(|e| e.to_string())?;

    // Build the graph directly (inline version of DependencyService logic)
    let graph = build_dependency_graph(&proposals, &deps);

    // Mirror the HTTP dependency-analysis contract: viewing the graph counts as
    // reviewing dependency ordering for the accept gate.
    if let Err(error) = state
        .ideation_session_repo
        .set_dependencies_acknowledged(session_id.as_str())
        .await
    {
        warn!(
            session_id = session_id.as_str(),
            error = %error,
            "Failed to mark dependency analysis as acknowledged"
        );
    }

    Ok(DependencyGraphResponse::from(graph))
}

// ============================================================================
// Graph Building Helpers
// ============================================================================

/// Build a dependency graph from proposals and their dependencies
pub fn build_dependency_graph(
    proposals: &[TaskProposal],
    dependencies: &[(TaskProposalId, TaskProposalId, Option<String>)],
) -> DependencyGraph {
    // Build adjacency lists
    let mut from_map: HashMap<TaskProposalId, Vec<TaskProposalId>> = HashMap::new();
    let mut to_map: HashMap<TaskProposalId, Vec<TaskProposalId>> = HashMap::new();

    for (from, to, _reason) in dependencies {
        from_map.entry(from.clone()).or_default().push(to.clone());
        to_map.entry(to.clone()).or_default().push(from.clone());
    }

    // Build nodes with degree counts
    let mut nodes = Vec::new();
    for proposal in proposals {
        let in_degree = from_map.get(&proposal.id).map(|v| v.len()).unwrap_or(0);
        let out_degree = to_map.get(&proposal.id).map(|v| v.len()).unwrap_or(0);

        let mut node = DependencyGraphNode::new(proposal.id.clone(), &proposal.title);
        node.in_degree = in_degree;
        node.out_degree = out_degree;
        nodes.push(node);
    }

    // Build edges
    // TODO: Task 5 will add reason to DependencyGraphEdge
    let edges: Vec<DependencyGraphEdge> = dependencies
        .iter()
        .map(|(from, to, _reason)| DependencyGraphEdge::new(from.clone(), to.clone()))
        .collect();

    // Detect cycles using DFS
    let (has_cycles, cycles) = detect_cycles(&from_map, proposals);

    // Find critical path if no cycles
    let critical_path = if !has_cycles {
        find_critical_path(&from_map, proposals)
    } else {
        Vec::new()
    };

    DependencyGraph {
        nodes,
        edges,
        critical_path,
        has_cycles,
        cycles: if has_cycles { Some(cycles) } else { None },
    }
}

/// Detect cycles in the dependency graph using DFS
fn detect_cycles(
    from_map: &HashMap<TaskProposalId, Vec<TaskProposalId>>,
    proposals: &[TaskProposal],
) -> (bool, Vec<Vec<TaskProposalId>>) {
    let proposal_ids: HashSet<TaskProposalId> = proposals.iter().map(|p| p.id.clone()).collect();
    let mut visited = HashSet::new();
    let mut rec_stack = HashSet::new();
    let mut cycles = Vec::new();

    for proposal in proposals {
        if !visited.contains(&proposal.id) {
            let mut path = Vec::new();
            if dfs_detect_cycle(
                &proposal.id,
                from_map,
                &proposal_ids,
                &mut visited,
                &mut rec_stack,
                &mut path,
                &mut cycles,
            ) {
                // Continue to find all cycles
            }
        }
    }

    (!cycles.is_empty(), cycles)
}

fn dfs_detect_cycle(
    node: &TaskProposalId,
    from_map: &HashMap<TaskProposalId, Vec<TaskProposalId>>,
    proposal_ids: &HashSet<TaskProposalId>,
    visited: &mut HashSet<TaskProposalId>,
    rec_stack: &mut HashSet<TaskProposalId>,
    path: &mut Vec<TaskProposalId>,
    cycles: &mut Vec<Vec<TaskProposalId>>,
) -> bool {
    visited.insert(node.clone());
    rec_stack.insert(node.clone());
    path.push(node.clone());

    if let Some(deps) = from_map.get(node) {
        for dep in deps {
            if !proposal_ids.contains(dep) {
                continue;
            }
            if !visited.contains(dep) {
                if dfs_detect_cycle(
                    dep,
                    from_map,
                    proposal_ids,
                    visited,
                    rec_stack,
                    path,
                    cycles,
                ) {
                    // Found a cycle
                }
            } else if rec_stack.contains(dep) {
                // Found a cycle - extract it
                let cycle_start = path.iter().position(|p| p == dep).unwrap_or(0);
                let cycle: Vec<TaskProposalId> = path[cycle_start..].to_vec();
                cycles.push(cycle);
            }
        }
    }

    path.pop();
    rec_stack.remove(node);

    !cycles.is_empty()
}

/// Find the critical path (longest path through the graph)
fn find_critical_path(
    from_map: &HashMap<TaskProposalId, Vec<TaskProposalId>>,
    proposals: &[TaskProposal],
) -> Vec<TaskProposalId> {
    if proposals.is_empty() {
        return Vec::new();
    }

    // Build reverse map for topological sort
    let mut in_degree: HashMap<TaskProposalId, usize> = HashMap::new();
    for proposal in proposals {
        in_degree.insert(proposal.id.clone(), 0);
    }

    for deps in from_map.values() {
        for dep in deps {
            if in_degree.contains_key(dep) {
                *in_degree
                    .get_mut(dep)
                    .expect("Dependency should exist in in_degree map") += 1;
            }
        }
    }

    // Topological sort using Kahn's algorithm
    let mut queue: VecDeque<TaskProposalId> = in_degree
        .iter()
        .filter(|(_, &deg)| deg == 0)
        .map(|(id, _)| id.clone())
        .collect();

    let mut sorted = Vec::new();
    while let Some(node) = queue.pop_front() {
        sorted.push(node.clone());
        if let Some(deps) = from_map.get(&node) {
            for dep in deps {
                if let Some(deg) = in_degree.get_mut(dep) {
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push_back(dep.clone());
                    }
                }
            }
        }
    }

    // Find longest path using dynamic programming
    let mut dist: HashMap<TaskProposalId, usize> = HashMap::new();
    let mut prev: HashMap<TaskProposalId, Option<TaskProposalId>> = HashMap::new();

    for id in &sorted {
        dist.insert(id.clone(), 0);
        prev.insert(id.clone(), None);
    }

    for node in &sorted {
        if let Some(deps) = from_map.get(node) {
            for dep in deps {
                if let Some(&node_dist) = dist.get(node) {
                    if let Some(dep_dist) = dist.get_mut(dep) {
                        if node_dist + 1 > *dep_dist {
                            *dep_dist = node_dist + 1;
                            prev.insert(dep.clone(), Some(node.clone()));
                        }
                    }
                }
            }
        }
    }

    // Find the end of the longest path
    let (end_node, _) = dist
        .iter()
        .max_by_key(|(_, &d)| d)
        .unwrap_or((&sorted[0], &0));

    // Reconstruct path
    let mut path = vec![end_node.clone()];
    let mut current = end_node.clone();
    while let Some(Some(prev_node)) = prev.get(&current) {
        path.push(prev_node.clone());
        current = prev_node.clone();
    }

    path.reverse();
    path
}
