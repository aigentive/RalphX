use std::sync::Arc;

use chrono::Utc;

use crate::domain::entities::types::ProjectId;
use crate::domain::entities::{
    ProjectSkill, ProjectSkillId, ProjectSkillLifecycleStatus, SkillUsageEvent, SkillUsageEventId,
    TaskOutcome, TaskOutcomeId, TaskOutcomeStatus,
};
use crate::domain::repositories::{
    ProjectSkillListOptions, ProjectSkillRepository, SkillUsageEventRepository,
    SkillUsageListOptions, TaskOutcomeListOptions, TaskOutcomeRepository, UpsertTaskOutcomeInput,
};
use crate::error::{AppError, AppResult};

pub struct OutcomeLedgerService {
    repo: Arc<dyn TaskOutcomeRepository>,
}

impl OutcomeLedgerService {
    pub fn new(repo: Arc<dyn TaskOutcomeRepository>) -> Self {
        Self { repo }
    }

    pub async fn record_outcome(&self, outcome: TaskOutcome) -> AppResult<TaskOutcome> {
        validate_non_empty("outcome source", &outcome.source)?;
        validate_non_empty("outcome source_ref_kind", &outcome.source_ref_kind)?;
        validate_non_empty("outcome source_ref_id", &outcome.source_ref_id)?;
        self.repo.upsert(UpsertTaskOutcomeInput { outcome }).await
    }

    pub async fn list_project_outcomes(
        &self,
        project_id: &ProjectId,
        options: TaskOutcomeListOptions,
    ) -> AppResult<Vec<TaskOutcome>> {
        self.repo.list_by_project(project_id, options).await
    }
}

pub struct ProjectSkillService {
    repo: Arc<dyn ProjectSkillRepository>,
}

impl ProjectSkillService {
    pub fn new(repo: Arc<dyn ProjectSkillRepository>) -> Self {
        Self { repo }
    }

    pub async fn stage_skill(&self, skill: ProjectSkill) -> AppResult<ProjectSkill> {
        validate_project_skill(&skill)?;
        if skill
            .predicted_effect
            .as_deref()
            .unwrap_or("")
            .trim()
            .is_empty()
        {
            return Err(AppError::Validation(
                "staged project skills require predicted_effect".to_string(),
            ));
        }
        self.repo.create(skill).await
    }

    pub async fn get_skill(&self, id: &ProjectSkillId) -> AppResult<Option<ProjectSkill>> {
        self.repo.get_by_id(id).await
    }

    pub async fn list_project_skills(
        &self,
        project_id: &ProjectId,
        options: ProjectSkillListOptions,
    ) -> AppResult<Vec<ProjectSkill>> {
        self.repo.list_by_project(project_id, options).await
    }

    pub async fn approve_skill(&self, id: &ProjectSkillId) -> AppResult<Option<ProjectSkill>> {
        self.repo
            .update_lifecycle_status(id, ProjectSkillLifecycleStatus::Approved)
            .await
    }

    pub async fn reject_skill(&self, id: &ProjectSkillId) -> AppResult<Option<ProjectSkill>> {
        self.repo
            .update_lifecycle_status(id, ProjectSkillLifecycleStatus::Rejected)
            .await
    }

    pub async fn archive_skill(&self, id: &ProjectSkillId) -> AppResult<Option<ProjectSkill>> {
        self.repo
            .update_lifecycle_status(id, ProjectSkillLifecycleStatus::Archived)
            .await
    }
}

pub struct SkillUsageService {
    repo: Arc<dyn SkillUsageEventRepository>,
}

impl SkillUsageService {
    pub fn new(repo: Arc<dyn SkillUsageEventRepository>) -> Self {
        Self { repo }
    }

    pub async fn record_usage(&self, event: SkillUsageEvent) -> AppResult<SkillUsageEvent> {
        validate_non_empty("skill usage injection_kind", &event.injection_kind)?;
        self.repo.record(event).await
    }

    pub async fn list_project_usage(
        &self,
        project_id: &ProjectId,
        options: SkillUsageListOptions,
    ) -> AppResult<Vec<SkillUsageEvent>> {
        self.repo.list_by_project(project_id, options).await
    }
}

pub fn new_empty_task_outcome(
    project_id: ProjectId,
    source: impl Into<String>,
    source_ref_kind: impl Into<String>,
    source_ref_id: impl Into<String>,
) -> TaskOutcome {
    let now = Utc::now();
    TaskOutcome {
        id: TaskOutcomeId::new(),
        project_id,
        source: source.into(),
        source_ref_kind: source_ref_kind.into(),
        source_ref_id: source_ref_id.into(),
        task_id: None,
        conversation_id: None,
        agent_run_id: None,
        pull_request_id: None,
        proposal_id: None,
        verification_id: None,
        review_id: None,
        outcome_class: None,
        status: TaskOutcomeStatus::Unknown,
        evidence_json: serde_json::json!({}),
        provider_harness: None,
        provider_session_id: None,
        created_at: now,
        updated_at: now,
    }
}

pub fn new_skill_usage_event(
    project_id: ProjectId,
    project_skill_id: ProjectSkillId,
    injection_kind: impl Into<String>,
) -> SkillUsageEvent {
    SkillUsageEvent {
        id: SkillUsageEventId::new(),
        project_id,
        project_skill_id,
        conversation_id: None,
        agent_run_id: None,
        provider_harness: None,
        stage: None,
        bucket: None,
        injection_kind: injection_kind.into(),
        outcome_id: None,
        metadata_json: serde_json::json!({}),
        created_at: Utc::now(),
    }
}

fn validate_project_skill(skill: &ProjectSkill) -> AppResult<()> {
    validate_non_empty("project skill title", &skill.title)?;
    validate_non_empty("project skill bucket", &skill.bucket)?;
    validate_non_empty("project skill stage", &skill.stage)?;
    validate_non_empty("project skill compact_guidance", &skill.compact_guidance)
}

fn validate_non_empty(label: &str, value: &str) -> AppResult<()> {
    if value.trim().is_empty() {
        return Err(AppError::Validation(format!("{label} is required")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::Utc;
    use serde_json::json;

    use super::{new_skill_usage_event, ProjectSkillService, SkillUsageService};
    use crate::domain::entities::types::ProjectId;
    use crate::domain::entities::{ProjectSkill, ProjectSkillId, ProjectSkillLifecycleStatus};
    use crate::domain::repositories::{ProjectSkillListOptions, SkillUsageListOptions};
    use crate::infrastructure::memory::{
        MemoryProjectSkillRepository, MemorySkillUsageEventRepository,
    };

    fn staged_skill(project_id: ProjectId) -> ProjectSkill {
        let now = Utc::now();
        ProjectSkill {
            id: ProjectSkillId::new(),
            project_id,
            title: "Keep learned skills repository-backed".to_string(),
            bucket: "execution".to_string(),
            stage: "execution".to_string(),
            status: ProjectSkillLifecycleStatus::Staged,
            pinned: false,
            archived: false,
            scope_paths: Vec::new(),
            compact_guidance: "Read approved learned skills from the project skill service."
                .to_string(),
            body_markdown: "Detailed guidance".to_string(),
            predicted_effect: Some("Avoids adapter-only injection.".to_string()),
            provenance_json: json!({ "test": true }),
            companion_of_skill_id: None,
            created_at: now,
            updated_at: now,
        }
    }

    #[tokio::test]
    async fn project_skill_service_enforces_predicted_effect_before_staging() {
        let service = ProjectSkillService::new(Arc::new(MemoryProjectSkillRepository::new()));
        let project_id = ProjectId::from_string("project-1".to_string());
        let mut skill = staged_skill(project_id);
        skill.predicted_effect = None;

        let result = service.stage_skill(skill).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn project_skill_service_lifecycle_and_usage_services_work_together() {
        let skill_repo = Arc::new(MemoryProjectSkillRepository::new());
        let usage_repo = Arc::new(MemorySkillUsageEventRepository::new());
        let skill_service = ProjectSkillService::new(skill_repo);
        let usage_service = SkillUsageService::new(usage_repo);
        let project_id = ProjectId::from_string("project-1".to_string());

        let staged = skill_service
            .stage_skill(staged_skill(project_id.clone()))
            .await
            .unwrap();
        let approved = skill_service
            .approve_skill(&staged.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(approved.status, ProjectSkillLifecycleStatus::Approved);

        let listed = skill_service
            .list_project_skills(
                &project_id,
                ProjectSkillListOptions {
                    status: Some(ProjectSkillLifecycleStatus::Approved),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(listed.len(), 1);

        usage_service
            .record_usage(new_skill_usage_event(
                project_id.clone(),
                approved.id,
                "compact_index",
            ))
            .await
            .unwrap();
        let usage = usage_service
            .list_project_usage(&project_id, SkillUsageListOptions::default())
            .await
            .unwrap();
        assert_eq!(usage.len(), 1);
    }
}
