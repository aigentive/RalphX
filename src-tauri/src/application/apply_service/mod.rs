// ApplyService
// Application service for converting task proposals to real tasks
//
// This service handles the "apply" flow:
// - Validating selected proposals have no circular dependencies
// - Creating Task entities from TaskProposal entities
// - Copying proposal dependencies to task dependencies
// - Updating proposal status and linking to created tasks
// - Optionally marking the session as "converted"

mod helpers;
mod types;

pub use types::{ApplyProposalsOptions, ApplyProposalsResult, SelectionValidation, TargetColumn};

use crate::domain::entities::ideation::PLAN_CONTRACT_V2;
use crate::domain::entities::{
    IdeationSessionId, IdeationSessionStatus, ProposalStatus, TaskProposal, TaskProposalId,
    TaskStep,
};
use crate::domain::repositories::{
    IdeationSessionRepository, ProposalDependencyRepository, TaskDependencyRepository,
    TaskProposalRepository, TaskRepository, TaskStepRepository,
};
use crate::error::{AppError, AppResult};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Service for converting proposals to tasks
pub struct ApplyService<
    S: IdeationSessionRepository,
    P: TaskProposalRepository,
    PD: ProposalDependencyRepository,
    T: TaskRepository,
    TD: TaskDependencyRepository,
    TS: TaskStepRepository,
> {
    session_repo: Arc<S>,
    proposal_repo: Arc<P>,
    proposal_dep_repo: Arc<PD>,
    task_repo: Arc<T>,
    task_dep_repo: Arc<TD>,
    task_step_repo: Arc<TS>,
}

impl<S, P, PD, T, TD, TS> ApplyService<S, P, PD, T, TD, TS>
where
    S: IdeationSessionRepository,
    P: TaskProposalRepository,
    PD: ProposalDependencyRepository,
    T: TaskRepository,
    TD: TaskDependencyRepository,
    TS: TaskStepRepository,
{
    /// Create a new apply service
    pub fn new(
        session_repo: Arc<S>,
        proposal_repo: Arc<P>,
        proposal_dep_repo: Arc<PD>,
        task_repo: Arc<T>,
        task_dep_repo: Arc<TD>,
        task_step_repo: Arc<TS>,
    ) -> Self {
        Self {
            session_repo,
            proposal_repo,
            proposal_dep_repo,
            task_repo,
            task_dep_repo,
            task_step_repo,
        }
    }

    #[doc(hidden)]
    pub fn task_step_repo_for_test(&self) -> &Arc<TS> {
        &self.task_step_repo
    }

    /// Validate that the selected proposals have no circular dependencies
    pub async fn validate_selection(
        &self,
        session_id: &IdeationSessionId,
        proposal_ids: &[TaskProposalId],
    ) -> AppResult<SelectionValidation> {
        if proposal_ids.is_empty() {
            return Ok(SelectionValidation {
                is_valid: true,
                cycles: Vec::new(),
                warnings: vec!["No proposals selected".to_string()],
            });
        }

        // Get all dependencies for the session
        let all_deps = self
            .proposal_dep_repo
            .get_all_for_session(session_id)
            .await?;

        // Build a set of selected proposal IDs for quick lookup
        let selected_set: HashSet<_> = proposal_ids.iter().map(|id| id.to_string()).collect();

        // Filter dependencies to only those between selected proposals
        let relevant_deps: Vec<_> = all_deps
            .iter()
            .filter(|(from, to, _reason)| {
                selected_set.contains(&from.to_string()) && selected_set.contains(&to.to_string())
            })
            .collect();

        // Build adjacency list for cycle detection
        let mut adj: HashMap<String, Vec<String>> = HashMap::new();
        for (from, to, _reason) in &relevant_deps {
            adj.entry(from.to_string())
                .or_default()
                .push(to.to_string());
        }

        // Detect cycles using DFS
        let cycles = helpers::detect_cycles(&selected_set, &adj);

        Ok(helpers::build_validation_result(
            cycles,
            &all_deps,
            &selected_set,
        ))
    }

    /// Apply selected proposals to the Kanban board, creating real tasks
    pub async fn apply_proposals(
        &self,
        session_id: &IdeationSessionId,
        options: ApplyProposalsOptions,
    ) -> AppResult<ApplyProposalsResult> {
        // Get the session to know the project_id
        let session = self
            .session_repo
            .get_by_id(session_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("Session {} not found", session_id)))?;

        if session.status != IdeationSessionStatus::Active {
            return Err(AppError::Validation(format!(
                "Cannot apply proposals from {} session",
                session.status
            )));
        }

        // Validate selection
        let validation = self
            .validate_selection(session_id, &options.proposal_ids)
            .await?;

        if !validation.is_valid {
            return Err(AppError::Validation(format!(
                "Selection has circular dependencies: {:?}",
                validation.cycles
            )));
        }

        // Get all selected proposals
        let mut proposals_map: HashMap<String, TaskProposal> = HashMap::new();
        for proposal_id in &options.proposal_ids {
            if let Some(proposal) = self.proposal_repo.get_by_id(proposal_id).await? {
                proposals_map.insert(proposal_id.to_string(), proposal);
            }
        }

        if proposals_map.is_empty() {
            return Ok(ApplyProposalsResult {
                created_tasks: Vec::new(),
                dependencies_created: 0,
                warnings: vec!["No valid proposals found".to_string()],
                session_converted: false,
            });
        }

        if session.plan_contract_version >= PLAN_CONTRACT_V2 {
            if let Some(proposal) = proposals_map
                .values()
                .find(|proposal| proposal.blueprint_artifact_id.is_none())
            {
                return Err(AppError::Validation(format!(
                    "Cannot apply proposal {} because this v2 plan requires blueprint lineage",
                    proposal.id
                )));
            }
        }

        // Create tasks and track proposal->task mapping
        let mut proposal_to_task: HashMap<String, _> = HashMap::new();
        let mut created_tasks = Vec::new();
        let target_status = options.target_column.to_status();

        for proposal in proposals_map.values() {
            let task =
                helpers::create_task_from_proposal(proposal, &session.project_id, target_status);
            let created_task = self.task_repo.create(task).await?;

            // Import steps from proposal if they exist
            if let Some(steps_json) = &proposal.steps {
                if let Ok(step_titles) = serde_json::from_str::<Vec<String>>(steps_json) {
                    if !step_titles.is_empty() {
                        let task_steps: Vec<TaskStep> = step_titles
                            .into_iter()
                            .enumerate()
                            .map(|(idx, title)| {
                                TaskStep::new(
                                    created_task.id.clone(),
                                    title,
                                    idx as i32,
                                    "proposal".to_string(),
                                )
                            })
                            .collect();

                        // Use bulk_create to insert all steps
                        let _ = self.task_step_repo.bulk_create(task_steps).await?;
                    }
                }
            }

            // Update proposal with created task ID and status
            let mut updated_proposal = proposal.clone();
            updated_proposal.created_task_id = Some(created_task.id.clone());
            updated_proposal.status = ProposalStatus::Accepted;
            updated_proposal.touch();
            self.proposal_repo.update(&updated_proposal).await?;

            proposal_to_task.insert(proposal.id.to_string(), created_task.id.clone());
            created_tasks.push(created_task);
        }

        // Create task dependencies from proposal dependencies
        let mut dependencies_created = 0;
        let all_deps = self
            .proposal_dep_repo
            .get_all_for_session(session_id)
            .await?;

        for (from_proposal, to_proposal, _reason) in all_deps {
            // Only create dependency if both proposals were converted
            if let (Some(from_task), Some(to_task)) = (
                proposal_to_task.get(&from_proposal.to_string()),
                proposal_to_task.get(&to_proposal.to_string()),
            ) {
                self.task_dep_repo
                    .add_dependency(from_task, to_task)
                    .await?;
                dependencies_created += 1;
            }
        }

        // Set initial Blocked/Ready status based on dependency graph
        // Tasks with blockers start as Blocked; tasks without blockers start as Ready
        for task in &created_tasks {
            let blockers = self.task_dep_repo.get_blockers(&task.id).await?;
            if !blockers.is_empty() {
                // Get blocker names for the blocked_reason
                let blocker_names: Vec<String> = blockers
                    .iter()
                    .filter_map(|blocker_id| {
                        created_tasks
                            .iter()
                            .find(|t| t.id == *blocker_id)
                            .map(|t| t.title.clone())
                    })
                    .collect();

                // Update task to Blocked status with reason
                let mut blocked_task = task.clone();
                blocked_task.internal_status = crate::domain::entities::InternalStatus::Blocked;
                blocked_task.blocked_reason =
                    Some(format!("Waiting for: {}", blocker_names.join(", ")));
                blocked_task.touch();
                self.task_repo.update(&blocked_task).await?;
            }
        }

        // Check if all proposals in session are now converted
        let session_converted = self.check_and_update_session_status(session_id).await?;

        Ok(ApplyProposalsResult {
            created_tasks,
            dependencies_created,
            warnings: validation.warnings,
            session_converted,
        })
    }

    /// Check if session should be marked as converted and update if so
    async fn check_and_update_session_status(
        &self,
        session_id: &IdeationSessionId,
    ) -> AppResult<bool> {
        // Get all proposals in the session
        let proposals = self.proposal_repo.get_by_session(session_id).await?;

        // Check if all proposals have been converted (have a created_task_id)
        let all_converted = proposals.iter().all(|p| p.created_task_id.is_some());

        if all_converted && !proposals.is_empty() {
            self.session_repo
                .update_status(session_id, IdeationSessionStatus::Accepted)
                .await?;
            return Ok(true);
        }

        Ok(false)
    }

    /// Apply all selected proposals from a session
    pub async fn apply_selected_proposals(
        &self,
        session_id: &IdeationSessionId,
        target_column: TargetColumn,
    ) -> AppResult<ApplyProposalsResult> {
        // Get all selected proposals
        let selected = self
            .proposal_repo
            .get_selected_by_session(session_id)
            .await?;

        let proposal_ids: Vec<TaskProposalId> = selected.iter().map(|p| p.id.clone()).collect();

        self.apply_proposals(
            session_id,
            ApplyProposalsOptions {
                proposal_ids,
                target_column,
            },
        )
        .await
    }
}
