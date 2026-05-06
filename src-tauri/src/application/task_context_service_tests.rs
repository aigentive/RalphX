use super::*;
use crate::domain::entities::{
    create_artifact_content_preview, Artifact, ArtifactId, ArtifactRelation,
    ArtifactRelationType, ArtifactType, InternalStatus, Priority, ProjectId, ProposalCategory,
    Task, TaskProposal, TaskProposalId, TaskStep, TaskStepId,
};
use crate::domain::repositories::{
    ArtifactRepository, StateHistoryMetadata, TaskDependencyRepository, TaskProposalRepository,
    TaskRepository, TaskStepRepository,
};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

// Mock repositories for testing
struct MockTaskRepository {
    task: Option<Task>,
    tasks: Vec<Task>,
}

impl MockTaskRepository {
    fn with_task(task: Task) -> Self {
        Self {
            task: Some(task.clone()),
            tasks: vec![task],
        }
    }

    fn with_tasks(task: Task, mut related: Vec<Task>) -> Self {
        let mut tasks = vec![task.clone()];
        tasks.append(&mut related);
        Self {
            task: Some(task),
            tasks,
        }
    }
}

#[async_trait]
impl TaskRepository for MockTaskRepository {
    async fn create(&self, task: Task) -> AppResult<Task> {
        Ok(task)
    }

    async fn get_by_id(&self, id: &TaskId) -> AppResult<Option<Task>> {
        Ok(self
            .tasks
            .iter()
            .find(|task| &task.id == id)
            .cloned()
            .or_else(|| self.task.clone()))
    }

    async fn get_by_project(&self, _project_id: &ProjectId) -> AppResult<Vec<Task>> {
        Ok(vec![])
    }

    async fn update(&self, _task: &Task) -> AppResult<()> {
        Ok(())
    }

    async fn update_with_expected_status(
        &self,
        _task: &Task,
        _expected_status: InternalStatus,
    ) -> AppResult<bool> {
        Ok(true)
    }

    async fn update_metadata(&self, _id: &TaskId, _metadata: Option<String>) -> AppResult<()> {
        Ok(())
    }

    async fn delete(&self, _id: &TaskId) -> AppResult<()> {
        Ok(())
    }

    async fn get_by_status(
        &self,
        _project_id: &ProjectId,
        _status: InternalStatus,
    ) -> AppResult<Vec<Task>> {
        Ok(vec![])
    }

    async fn persist_status_change(
        &self,
        _id: &TaskId,
        _from: InternalStatus,
        _to: InternalStatus,
        _trigger: &str,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn get_status_history(
        &self,
        _id: &TaskId,
    ) -> AppResult<Vec<crate::domain::repositories::StatusTransition>> {
        Ok(vec![])
    }

    async fn get_status_entered_at(
        &self,
        _task_id: &TaskId,
        _status: InternalStatus,
    ) -> AppResult<Option<chrono::DateTime<chrono::Utc>>> {
        Ok(None)
    }

    async fn get_next_executable(&self, _project_id: &ProjectId) -> AppResult<Option<Task>> {
        Ok(None)
    }

    async fn get_by_ideation_session(
        &self,
        _session_id: &crate::domain::entities::IdeationSessionId,
    ) -> AppResult<Vec<Task>> {
        Ok(vec![])
    }

    async fn get_by_project_filtered(
        &self,
        _project_id: &ProjectId,
        _include_archived: bool,
    ) -> AppResult<Vec<Task>> {
        Ok(vec![])
    }

    async fn archive(&self, task_id: &TaskId) -> AppResult<Task> {
        if let Some(task) = &self.task {
            let mut archived = task.clone();
            archived.archived_at = Some(chrono::Utc::now());
            Ok(archived)
        } else {
            Err(crate::error::AppError::NotFound(format!(
                "Task {} not found",
                task_id.as_str()
            )))
        }
    }

    async fn restore(&self, task_id: &TaskId) -> AppResult<Task> {
        if let Some(task) = &self.task {
            let mut restored = task.clone();
            restored.archived_at = None;
            Ok(restored)
        } else {
            Err(crate::error::AppError::NotFound(format!(
                "Task {} not found",
                task_id.as_str()
            )))
        }
    }

    async fn get_archived_count(
        &self,
        _project_id: &ProjectId,
        _ideation_session_id: Option<&str>,
    ) -> AppResult<u32> {
        Ok(0)
    }

    async fn list_paginated(
        &self,
        _project_id: &ProjectId,
        _statuses: Option<Vec<InternalStatus>>,
        _offset: u32,
        _limit: u32,
        _include_archived: bool,
        _ideation_session_id: Option<&str>,
        _execution_plan_id: Option<&str>,
        _categories: Option<&[String]>,
    ) -> AppResult<Vec<Task>> {
        if let Some(task) = &self.task {
            Ok(vec![task.clone()])
        } else {
            Ok(vec![])
        }
    }

    async fn count_tasks(
        &self,
        _project_id: &ProjectId,
        _include_archived: bool,
        _ideation_session_id: Option<&str>,
        _execution_plan_id: Option<&str>,
    ) -> AppResult<u32> {
        Ok(if self.task.is_some() { 1 } else { 0 })
    }

    async fn search(
        &self,
        _project_id: &ProjectId,
        _query: &str,
        _include_archived: bool,
    ) -> AppResult<Vec<Task>> {
        Ok(vec![])
    }

    async fn get_oldest_ready_task(&self) -> AppResult<Option<Task>> {
        Ok(None)
    }

    async fn get_oldest_ready_tasks(&self, _limit: u32) -> AppResult<Vec<Task>> {
        Ok(vec![])
    }

    async fn get_stale_ready_tasks(&self, _threshold_secs: u64) -> AppResult<Vec<Task>> {
        Ok(vec![])
    }

    async fn update_latest_state_history_metadata(
        &self,
        _task_id: &TaskId,
        _metadata: &StateHistoryMetadata,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn has_task_in_states(
        &self,
        _project_id: &ProjectId,
        _statuses: &[InternalStatus],
    ) -> AppResult<bool> {
        Ok(false)
    }

    async fn get_status_history_batch(
        &self,
        _task_ids: &[crate::domain::entities::TaskId],
    ) -> AppResult<HashMap<crate::domain::entities::TaskId, Vec<crate::domain::repositories::StatusTransition>>> {
        Ok(HashMap::new())
    }
}

struct MockTaskDependencyRepository {
    blockers: HashMap<String, Vec<TaskId>>,
    blocked_by: HashMap<String, Vec<TaskId>>,
}

impl MockTaskDependencyRepository {
    fn empty() -> Self {
        Self {
            blockers: HashMap::new(),
            blocked_by: HashMap::new(),
        }
    }

    fn with_blockers(task_id: &TaskId, blockers: Vec<TaskId>) -> Self {
        let mut repo = Self::empty();
        repo.blockers.insert(task_id.as_str().to_string(), blockers);
        repo
    }
}

#[async_trait]
impl TaskDependencyRepository for MockTaskDependencyRepository {
    async fn add_dependency(&self, _task_id: &TaskId, _depends_on: &TaskId) -> AppResult<()> {
        Ok(())
    }
    async fn remove_dependency(&self, _task_id: &TaskId, _depends_on: &TaskId) -> AppResult<()> {
        Ok(())
    }
    async fn get_blockers(&self, task_id: &TaskId) -> AppResult<Vec<TaskId>> {
        Ok(self
            .blockers
            .get(task_id.as_str())
            .cloned()
            .unwrap_or_default())
    }
    async fn get_blocked_by(&self, task_id: &TaskId) -> AppResult<Vec<TaskId>> {
        Ok(self
            .blocked_by
            .get(task_id.as_str())
            .cloned()
            .unwrap_or_default())
    }
    async fn has_circular_dependency(
        &self,
        _task_id: &TaskId,
        _potential_dep: &TaskId,
    ) -> AppResult<bool> {
        Ok(false)
    }
    async fn clear_dependencies(&self, _task_id: &TaskId) -> AppResult<()> {
        Ok(())
    }
    async fn count_blockers(&self, _task_id: &TaskId) -> AppResult<u32> {
        Ok(0)
    }
    async fn count_blocked_by(&self, _task_id: &TaskId) -> AppResult<u32> {
        Ok(0)
    }
    async fn has_dependency(&self, _task_id: &TaskId, _depends_on: &TaskId) -> AppResult<bool> {
        Ok(false)
    }

    async fn get_blockers_batch(
        &self,
        _task_ids: &[TaskId],
    ) -> AppResult<HashMap<TaskId, Vec<TaskId>>> {
        Ok(HashMap::new())
    }
}

struct MockTaskProposalRepository {
    proposal: Option<TaskProposal>,
}

impl MockTaskProposalRepository {
    fn empty() -> Self {
        Self { proposal: None }
    }

    fn with_proposal(proposal: TaskProposal) -> Self {
        Self {
            proposal: Some(proposal),
        }
    }
}

#[async_trait]
impl TaskProposalRepository for MockTaskProposalRepository {
    async fn create(&self, proposal: TaskProposal) -> AppResult<TaskProposal> {
        Ok(proposal)
    }

    async fn get_by_id(&self, _id: &TaskProposalId) -> AppResult<Option<TaskProposal>> {
        Ok(self.proposal.clone())
    }

    async fn get_by_session(
        &self,
        _session_id: &crate::domain::entities::IdeationSessionId,
    ) -> AppResult<Vec<TaskProposal>> {
        Ok(vec![])
    }

    async fn update(&self, _proposal: &TaskProposal) -> AppResult<()> {
        Ok(())
    }

    async fn update_priority(
        &self,
        _id: &TaskProposalId,
        _assessment: &crate::domain::entities::PriorityAssessment,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn update_selection(&self, _id: &TaskProposalId, _selected: bool) -> AppResult<()> {
        Ok(())
    }

    async fn set_created_task_id(&self, _id: &TaskProposalId, _task_id: &TaskId) -> AppResult<()> {
        Ok(())
    }

    async fn delete(&self, _id: &TaskProposalId) -> AppResult<()> {
        Ok(())
    }

    async fn reorder(
        &self,
        _session_id: &crate::domain::entities::IdeationSessionId,
        _proposal_ids: Vec<TaskProposalId>,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn get_selected_by_session(
        &self,
        _session_id: &crate::domain::entities::IdeationSessionId,
    ) -> AppResult<Vec<TaskProposal>> {
        Ok(vec![])
    }

    async fn count_by_session(
        &self,
        _session_id: &crate::domain::entities::IdeationSessionId,
    ) -> AppResult<u32> {
        Ok(0)
    }

    async fn count_selected_by_session(
        &self,
        _session_id: &crate::domain::entities::IdeationSessionId,
    ) -> AppResult<u32> {
        Ok(0)
    }

    async fn get_by_plan_artifact_id(
        &self,
        _artifact_id: &ArtifactId,
    ) -> AppResult<Vec<TaskProposal>> {
        Ok(vec![])
    }

    async fn clear_created_task_ids_by_session(
        &self,
        _session_id: &crate::domain::entities::IdeationSessionId,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn archive(&self, _id: &TaskProposalId) -> AppResult<TaskProposal> {
        unimplemented!()
    }
}

struct MockArtifactRepository {
    artifact: Option<Artifact>,
    related: Vec<Artifact>,
}

impl MockArtifactRepository {
    fn empty() -> Self {
        Self {
            artifact: None,
            related: vec![],
        }
    }

    fn with_artifact(artifact: Artifact) -> Self {
        Self {
            artifact: Some(artifact),
            related: vec![],
        }
    }

    fn with_related(artifact: Artifact, related: Vec<Artifact>) -> Self {
        Self {
            artifact: Some(artifact),
            related,
        }
    }
}

#[async_trait]
impl ArtifactRepository for MockArtifactRepository {
    async fn create(&self, artifact: Artifact) -> AppResult<Artifact> {
        Ok(artifact)
    }

    async fn get_by_id(&self, _id: &ArtifactId) -> AppResult<Option<Artifact>> {
        Ok(self.artifact.clone())
    }

    async fn get_by_id_at_version(
        &self,
        _id: &ArtifactId,
        _version: u32,
    ) -> AppResult<Option<Artifact>> {
        Ok(self.artifact.clone())
    }

    async fn get_by_bucket(
        &self,
        _bucket_id: &crate::domain::entities::ArtifactBucketId,
    ) -> AppResult<Vec<Artifact>> {
        Ok(vec![])
    }

    async fn get_by_type(&self, _artifact_type: ArtifactType) -> AppResult<Vec<Artifact>> {
        Ok(vec![])
    }

    async fn get_by_task(&self, _task_id: &TaskId) -> AppResult<Vec<Artifact>> {
        Ok(vec![])
    }

    async fn get_by_process(
        &self,
        _process_id: &crate::domain::entities::ProcessId,
    ) -> AppResult<Vec<Artifact>> {
        Ok(vec![])
    }

    async fn update(&self, _artifact: &Artifact) -> AppResult<()> {
        Ok(())
    }

    async fn delete(&self, _id: &ArtifactId) -> AppResult<()> {
        Ok(())
    }

    async fn get_derived_from(&self, _artifact_id: &ArtifactId) -> AppResult<Vec<Artifact>> {
        Ok(vec![])
    }

    async fn get_related(&self, _artifact_id: &ArtifactId) -> AppResult<Vec<Artifact>> {
        Ok(self.related.clone())
    }

    async fn add_relation(&self, relation: ArtifactRelation) -> AppResult<ArtifactRelation> {
        Ok(relation)
    }

    async fn get_relations(&self, _artifact_id: &ArtifactId) -> AppResult<Vec<ArtifactRelation>> {
        Ok(vec![])
    }

    async fn get_relations_by_type(
        &self,
        _artifact_id: &ArtifactId,
        _relation_type: ArtifactRelationType,
    ) -> AppResult<Vec<ArtifactRelation>> {
        Ok(vec![])
    }

    async fn delete_relation(&self, _from_id: &ArtifactId, _to_id: &ArtifactId) -> AppResult<()> {
        Ok(())
    }

    async fn create_with_previous_version(
        &self,
        artifact: Artifact,
        _previous_version_id: ArtifactId,
    ) -> AppResult<Artifact> {
        Ok(artifact)
    }

    async fn get_version_history(
        &self,
        _id: &ArtifactId,
    ) -> AppResult<Vec<crate::domain::repositories::ArtifactVersionSummary>> {
        Ok(vec![])
    }

    async fn resolve_latest_artifact_id(&self, id: &ArtifactId) -> AppResult<ArtifactId> {
        Ok(id.clone())
    }

    async fn archive(&self, _id: &ArtifactId) -> AppResult<Artifact> {
        unimplemented!()
    }
}

struct MockTaskStepRepository {
    steps: Mutex<HashMap<String, TaskStep>>,
}

impl MockTaskStepRepository {
    fn empty() -> Self {
        Self {
            steps: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl TaskStepRepository for MockTaskStepRepository {
    async fn create(&self, step: TaskStep) -> AppResult<TaskStep> {
        self.steps
            .lock()
            .unwrap()
            .insert(step.id.to_string(), step.clone());
        Ok(step)
    }

    async fn get_by_id(&self, id: &TaskStepId) -> AppResult<Option<TaskStep>> {
        Ok(self.steps.lock().unwrap().get(&id.to_string()).cloned())
    }

    async fn get_by_task(&self, task_id: &TaskId) -> AppResult<Vec<TaskStep>> {
        let mut steps: Vec<_> = self
            .steps
            .lock()
            .unwrap()
            .values()
            .filter(|s| &s.task_id == task_id)
            .cloned()
            .collect();
        steps.sort_by_key(|s| s.sort_order);
        Ok(steps)
    }

    async fn get_by_task_and_status(
        &self,
        task_id: &TaskId,
        status: crate::domain::entities::TaskStepStatus,
    ) -> AppResult<Vec<TaskStep>> {
        let mut steps: Vec<_> = self
            .steps
            .lock()
            .unwrap()
            .values()
            .filter(|s| &s.task_id == task_id && s.status == status)
            .cloned()
            .collect();
        steps.sort_by_key(|s| s.sort_order);
        Ok(steps)
    }

    async fn update(&self, step: &TaskStep) -> AppResult<()> {
        self.steps
            .lock()
            .unwrap()
            .insert(step.id.to_string(), step.clone());
        Ok(())
    }

    async fn delete(&self, id: &TaskStepId) -> AppResult<()> {
        self.steps.lock().unwrap().remove(&id.to_string());
        Ok(())
    }

    async fn delete_by_task(&self, task_id: &TaskId) -> AppResult<()> {
        self.steps
            .lock()
            .unwrap()
            .retain(|_, s| &s.task_id != task_id);
        Ok(())
    }

    async fn count_by_status(
        &self,
        task_id: &TaskId,
    ) -> AppResult<HashMap<crate::domain::entities::TaskStepStatus, u32>> {
        let steps = self.get_by_task(task_id).await?;
        let mut counts = HashMap::new();
        for step in steps {
            *counts.entry(step.status).or_insert(0) += 1;
        }
        Ok(counts)
    }

    async fn bulk_create(&self, steps: Vec<TaskStep>) -> AppResult<Vec<TaskStep>> {
        for step in &steps {
            self.steps
                .lock()
                .unwrap()
                .insert(step.id.to_string(), step.clone());
        }
        Ok(steps)
    }

    async fn reorder(&self, _task_id: &TaskId, _step_ids: Vec<TaskStepId>) -> AppResult<()> {
        Ok(())
    }

    async fn reset_all_to_pending(&self, _task_id: &TaskId) -> AppResult<u32> {
        Ok(0)
    }
}

#[tokio::test]
async fn test_get_task_context_basic() {
    let task = Task::new(ProjectId::new(), "Test Task".to_string());
    let task_id = task.id.clone();

    let service = TaskContextService::new(
        Arc::new(MockTaskRepository::with_task(task.clone())),
        Arc::new(MockTaskDependencyRepository::empty()),
        Arc::new(MockTaskProposalRepository::empty()),
        Arc::new(MockArtifactRepository::empty()),
        Arc::new(MockTaskStepRepository::empty()),
    );

    let context = service.get_task_context(&task_id).await.unwrap();

    assert_eq!(context.task.id, task_id);
    assert!(context.source_proposal.is_none());
    assert!(context.plan_artifact.is_none());
    assert_eq!(context.related_artifacts.len(), 0);
    assert_eq!(context.steps.len(), 0);
    assert!(context.step_progress.is_none());
    assert!(!context.context_hints.is_empty());
    // New dependency fields
    assert!(context.blocked_by.is_empty());
    assert!(context.blocks.is_empty());
    assert_eq!(context.tier, Some(1)); // No blockers = tier 1
}

#[tokio::test]
async fn test_get_task_context_with_proposal() {
    let mut task = Task::new(ProjectId::new(), "Test Task".to_string());
    let proposal_id = TaskProposalId::new();
    task.source_proposal_id = Some(proposal_id.clone());
    let task_id = task.id.clone();

    let mut proposal = TaskProposal::new(
        crate::domain::entities::IdeationSessionId::new(),
        "Test Proposal",
        ProposalCategory::Feature,
        Priority::Medium,
    );
    proposal.id = proposal_id;
    proposal.description = Some("Proposal description".to_string());
    // acceptance_criteria is stored as JSON string
    proposal.acceptance_criteria = Some(serde_json::to_string(&vec!["AC1"]).unwrap());

    let service = TaskContextService::new(
        Arc::new(MockTaskRepository::with_task(task.clone())),
        Arc::new(MockTaskDependencyRepository::empty()),
        Arc::new(MockTaskProposalRepository::with_proposal(proposal)),
        Arc::new(MockArtifactRepository::empty()),
        Arc::new(MockTaskStepRepository::empty()),
    );

    let context = service.get_task_context(&task_id).await.unwrap();

    assert_eq!(context.task.id, task_id);
    assert!(context.source_proposal.is_some());
    let proposal_summary = context.source_proposal.unwrap();
    assert_eq!(proposal_summary.title, "Test Proposal");
    assert_eq!(proposal_summary.acceptance_criteria.len(), 1);
    assert_eq!(proposal_summary.priority_score, 50); // Default priority score
    assert!(context.context_hints.iter().any(|h| h.contains("ideation")));
}

#[tokio::test]
async fn test_get_task_context_with_plan_artifact() {
    let mut task = Task::new(ProjectId::new(), "Test Task".to_string());
    let artifact_id = ArtifactId::new();
    task.plan_artifact_id = Some(artifact_id.clone());
    let task_id = task.id.clone();

    let artifact = Artifact::new_inline(
        "Implementation Plan",
        ArtifactType::Specification,
        "# Plan\n\nDetailed implementation instructions...".repeat(20),
        "user",
    );

    let service = TaskContextService::new(
        Arc::new(MockTaskRepository::with_task(task.clone())),
        Arc::new(MockTaskDependencyRepository::empty()),
        Arc::new(MockTaskProposalRepository::empty()),
        Arc::new(MockArtifactRepository::with_artifact(artifact)),
        Arc::new(MockTaskStepRepository::empty()),
    );

    let context = service.get_task_context(&task_id).await.unwrap();

    assert_eq!(context.task.id, task_id);
    assert!(context.plan_artifact.is_some());
    let plan = context.plan_artifact.unwrap();
    assert_eq!(plan.title, "Implementation Plan");
    assert!(plan.content_preview.len() <= 503); // 500 + "..."
    assert!(context.context_hints.iter().any(|h| h.contains("plan")));
}

#[tokio::test]
async fn test_get_task_context_with_related_artifacts() {
    let mut task = Task::new(ProjectId::new(), "Test Task".to_string());
    let artifact_id = ArtifactId::new();
    task.plan_artifact_id = Some(artifact_id.clone());
    let task_id = task.id.clone();

    let artifact = Artifact::new_inline(
        "Implementation Plan",
        ArtifactType::Specification,
        "Plan content",
        "user",
    );

    let related1 = Artifact::new_inline(
        "Research Doc",
        ArtifactType::ResearchDocument,
        "Research",
        "user",
    );
    let related2 = Artifact::new_inline("Design Doc", ArtifactType::DesignDoc, "Design", "user");

    let service = TaskContextService::new(
        Arc::new(MockTaskRepository::with_task(task.clone())),
        Arc::new(MockTaskDependencyRepository::empty()),
        Arc::new(MockTaskProposalRepository::empty()),
        Arc::new(MockArtifactRepository::with_related(
            artifact,
            vec![related1, related2],
        )),
        Arc::new(MockTaskStepRepository::empty()),
    );

    let context = service.get_task_context(&task_id).await.unwrap();

    assert_eq!(context.related_artifacts.len(), 2);
    assert!(context
        .context_hints
        .iter()
        .any(|h| h.contains("2 related artifacts")));
}

#[tokio::test]
async fn test_content_preview_truncation() {
    let short_content = "Short content";
    let artifact = Artifact::new_inline("Test", ArtifactType::Specification, short_content, "user");
    let preview = create_artifact_content_preview(&artifact);
    assert_eq!(preview, short_content);

    let long_content = "x".repeat(600);
    let artifact = Artifact::new_inline("Test", ArtifactType::Specification, long_content, "user");
    let preview = create_artifact_content_preview(&artifact);
    assert_eq!(preview.len(), 503); // 500 + "..."
    assert!(preview.ends_with("..."));
}

#[tokio::test]
async fn test_task_not_found() {
    let service = TaskContextService::new(
        Arc::new(MockTaskRepository {
            task: None,
            tasks: Vec::new(),
        }),
        Arc::new(MockTaskDependencyRepository::empty()),
        Arc::new(MockTaskProposalRepository::empty()),
        Arc::new(MockArtifactRepository::empty()),
        Arc::new(MockTaskStepRepository::empty()),
    );

    let result = service.get_task_context(&TaskId::new()).await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), AppError::NotFound(_)));
}

#[tokio::test]
async fn test_get_task_context_dependency_fields() {
    // Test that the new dependency fields are populated correctly
    let task = Task::new(ProjectId::new(), "Test Task".to_string());
    let task_id = task.id.clone();

    let service = TaskContextService::new(
        Arc::new(MockTaskRepository::with_task(task.clone())),
        Arc::new(MockTaskDependencyRepository::empty()),
        Arc::new(MockTaskProposalRepository::empty()),
        Arc::new(MockArtifactRepository::empty()),
        Arc::new(MockTaskStepRepository::empty()),
    );

    let context = service.get_task_context(&task_id).await.unwrap();

    // With mock returning empty blockers/dependents
    assert!(context.blocked_by.is_empty());
    assert!(context.blocks.is_empty());
    // Tier should be 1 when no blockers exist
    assert_eq!(context.tier, Some(1));
}

#[tokio::test]
async fn test_get_task_context_omits_merged_dependency_from_blocked_by() {
    let task = Task::new(ProjectId::new(), "Dependent Task".to_string());
    let task_id = task.id.clone();

    let mut blocker = Task::new(task.project_id.clone(), "Merged Blocker".to_string());
    blocker.internal_status = InternalStatus::Merged;
    let blocker_id = blocker.id.clone();

    let service = TaskContextService::new(
        Arc::new(MockTaskRepository::with_tasks(task.clone(), vec![blocker])),
        Arc::new(MockTaskDependencyRepository::with_blockers(
            &task_id,
            vec![blocker_id],
        )),
        Arc::new(MockTaskProposalRepository::empty()),
        Arc::new(MockArtifactRepository::empty()),
        Arc::new(MockTaskStepRepository::empty()),
    );

    let context = service.get_task_context(&task_id).await.unwrap();

    assert!(
        context.blocked_by.is_empty(),
        "merged dependencies are resolved and must not appear as active blockers"
    );
    assert_eq!(context.tier, Some(1));
    assert!(
        !context
            .context_hints
            .iter()
            .any(|hint| hint.starts_with("BLOCKED:")),
        "resolved blockers must not produce a worker stop hint"
    );
}
