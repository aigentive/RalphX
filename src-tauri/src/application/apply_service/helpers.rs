// Helper functions for ApplyService

use crate::domain::entities::{
    InternalStatus, ProjectId, Task, TaskCategory, TaskProposal, TaskProposalId,
};
use std::collections::{HashMap, HashSet};

use super::types::SelectionValidation;

/// Detect cycles in the dependency graph
pub fn detect_cycles(
    nodes: &HashSet<String>,
    adj: &HashMap<String, Vec<String>>,
) -> Vec<Vec<TaskProposalId>> {
    let mut cycles = Vec::new();
    let mut visited = HashSet::new();
    let mut rec_stack = HashSet::new();
    let mut path = Vec::new();

    for node in nodes {
        if !visited.contains(node) {
            dfs_cycle_detect(
                node,
                adj,
                &mut visited,
                &mut rec_stack,
                &mut path,
                &mut cycles,
            );
        }
    }

    cycles
}

fn dfs_cycle_detect(
    node: &str,
    adj: &HashMap<String, Vec<String>>,
    visited: &mut HashSet<String>,
    rec_stack: &mut HashSet<String>,
    path: &mut Vec<String>,
    cycles: &mut Vec<Vec<TaskProposalId>>,
) {
    visited.insert(node.to_string());
    rec_stack.insert(node.to_string());
    path.push(node.to_string());

    if let Some(neighbors) = adj.get(node) {
        for neighbor in neighbors {
            if !visited.contains(neighbor) {
                dfs_cycle_detect(neighbor, adj, visited, rec_stack, path, cycles);
            } else if rec_stack.contains(neighbor) {
                // Found a cycle - extract it from path
                let cycle_start = path.iter().position(|n| n == neighbor).unwrap();
                let cycle: Vec<TaskProposalId> = path[cycle_start..]
                    .iter()
                    .map(|id| TaskProposalId::from_string(id.clone()))
                    .collect();
                if !cycle.is_empty() {
                    cycles.push(cycle);
                }
            }
        }
    }

    path.pop();
    rec_stack.remove(node);
}

/// Build validation result from cycle detection
pub fn build_validation_result(
    cycles: Vec<Vec<TaskProposalId>>,
    all_deps: &[(TaskProposalId, TaskProposalId, Option<String>)],
    selected_set: &HashSet<String>,
) -> SelectionValidation {
    let mut warnings = Vec::new();

    // Check for missing dependencies (deps outside selection)
    for (from, to, _reason) in all_deps {
        if selected_set.contains(&from.to_string()) && !selected_set.contains(&to.to_string()) {
            warnings.push(format!(
                "Proposal {} depends on {} which is not selected",
                from, to
            ));
        }
    }

    SelectionValidation {
        is_valid: cycles.is_empty(),
        cycles,
        warnings,
    }
}

/// Create a Task from a TaskProposal
pub fn create_task_from_proposal(
    proposal: &TaskProposal,
    project_id: &ProjectId,
    status: InternalStatus,
) -> Task {
    // All tasks from proposals are Regular (not system-managed PlanMerge tasks)
    let mut task = Task::new_with_category(
        project_id.clone(),
        proposal.title.clone(),
        TaskCategory::Regular,
    );

    task.description = proposal.description.clone();
    task.priority = proposal.priority_score;
    task.internal_status = status;

    // Copy traceability references for worker context access
    task.source_proposal_id = Some(proposal.id.clone());
    task.plan_artifact_id = proposal.plan_artifact_id.clone();
    task.plan_blueprint_artifact_id = proposal.blueprint_artifact_id.clone();

    task
}
