// TaskContextService - aggregates task context with related artifacts and proposals
//
// Provides rich context for workers executing tasks by fetching:
// - Task details
// - Source proposal (if task was created from ideation)
// - Implementation plan artifact summary
// - Related artifacts
// - Context hints for workers

use std::sync::Arc;

use crate::domain::entities::{
    create_artifact_content_preview, generate_task_context_hints, ArtifactId, ArtifactSummary,
    ScopeDriftStatus, StepProgressSummary, Task, TaskContext, TaskDependencySummary, TaskId,
    TaskProposal, TaskProposalSummary,
};
use crate::domain::repositories::{
    ArtifactRepository, TaskDependencyRepository, TaskProposalRepository, TaskRepository,
    TaskStepRepository,
};
use crate::error::{AppError, AppResult};

pub(crate) fn resolve_task_blueprint_artifact_id(
    task: &Task,
    source_proposal: Option<&TaskProposal>,
) -> AppResult<Option<ArtifactId>> {
    if let Some(blueprint_id) = task.plan_blueprint_artifact_id.as_ref() {
        return Ok(Some(blueprint_id.clone()));
    }

    let Some(proposal) = source_proposal else {
        return Ok(None);
    };
    if proposal.blueprint_version_at_creation.is_some() {
        return Err(AppError::Validation(
            "v2 task is missing immutable Blueprint lineage".to_string(),
        ));
    }

    Ok(proposal.blueprint_artifact_id.clone())
}

/// Service for aggregating task context for worker execution
pub struct TaskContextService {
    task_repo: Arc<dyn TaskRepository>,
    task_dependency_repo: Arc<dyn TaskDependencyRepository>,
    proposal_repo: Arc<dyn TaskProposalRepository>,
    artifact_repo: Arc<dyn ArtifactRepository>,
    step_repo: Arc<dyn TaskStepRepository>,
}

impl TaskContextService {
    /// Create a new TaskContextService with the given repositories
    pub fn new(
        task_repo: Arc<dyn TaskRepository>,
        task_dependency_repo: Arc<dyn TaskDependencyRepository>,
        proposal_repo: Arc<dyn TaskProposalRepository>,
        artifact_repo: Arc<dyn ArtifactRepository>,
        step_repo: Arc<dyn TaskStepRepository>,
    ) -> Self {
        Self {
            task_repo,
            task_dependency_repo,
            proposal_repo,
            artifact_repo,
            step_repo,
        }
    }

    /// Get rich context for a task including linked artifacts and proposals
    ///
    /// Returns TaskContext with:
    /// - The task being executed
    /// - Source proposal summary (if exists)
    /// - Plan artifact summary with 500-char preview (if exists)
    /// - Related artifacts
    /// - Context hints for worker
    pub async fn get_task_context(&self, task_id: &TaskId) -> AppResult<TaskContext> {
        // 1. Fetch task by ID
        let task = self
            .task_repo
            .get_by_id(task_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("Task not found: {}", task_id)))?;

        // 2. If source_proposal_id present, fetch proposal and create TaskProposalSummary
        let source_proposal_entity = if let Some(proposal_id) = &task.source_proposal_id {
            self.proposal_repo.get_by_id(proposal_id).await?
        } else {
            None
        };
        let source_proposal = match source_proposal_entity.as_ref() {
            Some(proposal) => {
                // Parse acceptance_criteria from JSON string to Vec<String>
                let acceptance_criteria: Vec<String> = proposal
                    .acceptance_criteria
                    .as_ref()
                    .and_then(|json_str| serde_json::from_str(json_str).ok())
                    .unwrap_or_default();

                Some(TaskProposalSummary {
                    id: proposal.id.clone(),
                    title: proposal.title.clone(),
                    description: proposal.description.clone().unwrap_or_default(),
                    acceptance_criteria,
                    implementation_notes: None, // TaskProposal doesn't have implementation_notes field
                    plan_version_at_creation: proposal.plan_version_at_creation,
                    priority_score: proposal.priority_score,
                    affected_paths: proposal
                        .affected_paths
                        .as_ref()
                        .and_then(|json_str| serde_json::from_str(json_str).ok())
                        .unwrap_or_default(),
                })
            }
            None => None,
        };

        // 3. If plan_artifact_id present, fetch artifact and create ArtifactSummary (500-char preview)
        let plan_artifact = if let Some(artifact_id) = &task.plan_artifact_id {
            match self.artifact_repo.get_by_id(artifact_id).await? {
                Some(artifact) => {
                    let content_preview = create_artifact_content_preview(&artifact);
                    Some(ArtifactSummary {
                        id: artifact.id.clone(),
                        title: artifact.name.clone(),
                        artifact_type: artifact.artifact_type,
                        current_version: artifact.metadata.version,
                        content_preview,
                    })
                }
                None => None,
            }
        } else {
            None
        };

        let blueprint_artifact = if let Some(blueprint_id) =
            resolve_task_blueprint_artifact_id(&task, source_proposal_entity.as_ref())?
        {
            Some(
                self.artifact_repo
                    .get_by_id(&blueprint_id)
                    .await?
                    .ok_or_else(|| {
                        AppError::NotFound(format!(
                            "Task immutable Blueprint artifact was not found: {}",
                            blueprint_id.as_str()
                        ))
                    })
                    .map(|artifact| ArtifactSummary {
                        content_preview: create_artifact_content_preview(&artifact),
                        id: artifact.id,
                        title: artifact.name,
                        artifact_type: artifact.artifact_type,
                        current_version: artifact.metadata.version,
                    })?,
            )
        } else {
            None
        };

        // 4. Fetch related artifacts via ArtifactRelation
        let related_artifacts = if let Some(artifact_id) = &task.plan_artifact_id {
            let related = self.artifact_repo.get_related(artifact_id).await?;
            related
                .into_iter()
                .map(|artifact| {
                    let content_preview = create_artifact_content_preview(&artifact);
                    ArtifactSummary {
                        id: artifact.id.clone(),
                        title: artifact.name.clone(),
                        artifact_type: artifact.artifact_type,
                        current_version: artifact.metadata.version,
                        content_preview,
                    }
                })
                .collect()
        } else {
            vec![]
        };

        // 5. Fetch steps for the task
        let steps = self.step_repo.get_by_task(task_id).await?;

        // 6. Calculate step progress summary if steps exist
        let step_progress = if !steps.is_empty() {
            Some(StepProgressSummary::from_steps(task_id, &steps))
        } else {
            None
        };

        // 7. Fetch task dependencies (blockers and dependents) via TaskDependencyRepository
        let blocker_ids = self.task_dependency_repo.get_blockers(task_id).await?;
        let mut blocked_by: Vec<TaskDependencySummary> = Vec::new();
        for blocker_id in &blocker_ids {
            if let Some(blocker_task) = self.task_repo.get_by_id(blocker_id).await? {
                if !blocker_task.internal_status.is_active_dependency_blocker() {
                    continue;
                }
                blocked_by.push(TaskDependencySummary {
                    id: blocker_task.id.clone(),
                    title: blocker_task.title.clone(),
                    internal_status: blocker_task.internal_status,
                });
            }
        }

        let dependent_ids = self.task_dependency_repo.get_blocked_by(task_id).await?;
        let mut blocks: Vec<TaskDependencySummary> = Vec::new();
        for dep_id in &dependent_ids {
            if let Some(dep_task) = self.task_repo.get_by_id(dep_id).await? {
                blocks.push(TaskDependencySummary {
                    id: dep_task.id.clone(),
                    title: dep_task.title.clone(),
                    internal_status: dep_task.internal_status,
                });
            }
        }

        // 8. Compute tier from dependency depth
        // Tier 1 = no blockers, Tier N = depends on tasks in tier N-1
        // For now, use simple heuristic: tier = number of incomplete blockers + 1
        let tier = if blocked_by.is_empty() {
            Some(1)
        } else {
            Some((blocked_by.len() as u32) + 1)
        };

        // 9. Generate context_hints based on what's available
        let context_hints = generate_task_context_hints(
            &task,
            source_proposal.is_some(),
            plan_artifact.is_some(),
            related_artifacts.len(),
            steps.len(),
            &blocked_by,
            &blocks,
        );

        // 10. Return TaskContext
        let task_branch = task.task_branch.clone();
        let worktree_path = task.worktree_path.clone();
        Ok(TaskContext {
            task,
            source_proposal,
            plan_artifact,
            blueprint_artifact,
            related_artifacts,
            steps,
            step_progress,
            context_hints,
            blocked_by,
            blocks,
            tier,
            task_branch,
            worktree_path,
            validation_cache: None,
            actual_changed_files: Vec::new(),
            scope_drift_status: ScopeDriftStatus::Unbounded,
            out_of_scope_files: Vec::new(),
            out_of_scope_blocker_fingerprint: None,
            followup_sessions: Vec::new(),
        })
    }
}

#[cfg(test)]
#[path = "task_context_service_tests.rs"]
mod tests;
