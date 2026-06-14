use std::collections::BTreeSet;
use std::sync::Arc;

use chrono::Utc;
use serde_json::Value;

use crate::domain::entities::types::ProjectId;
use crate::domain::entities::{
    ProjectSkill, ProjectSkillId, ProjectSkillLifecycleStatus, SkillUsageEvent, SkillUsageEventId,
    TaskOutcome, TaskOutcomeId, TaskOutcomeStatus,
};
use crate::domain::repositories::{
    ProjectSkillListOptions, ProjectSkillRepository, SkillUsageEventRepository,
    SkillUsageListOptions, TaskOutcomeListOptions, TaskOutcomeRepository, UpsertTaskOutcomeInput,
};
use crate::domain::services::learned_skill_adapters::LearnedSkillConstraintCitation;
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

pub struct StageProjectSkillFromOutcomeInput {
    pub outcome: TaskOutcome,
    pub title: String,
    pub bucket: String,
    pub stage: String,
    pub scope_paths: Vec<String>,
    pub compact_guidance: String,
    pub body_markdown: String,
    pub predicted_effect: String,
    pub additional_provenance: Value,
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

    pub async fn stage_skill_from_outcome(
        &self,
        input: StageProjectSkillFromOutcomeInput,
    ) -> AppResult<ProjectSkill> {
        if input.outcome.status != TaskOutcomeStatus::Eligible {
            return Err(AppError::Validation(
                "project skill distillation requires an eligible task outcome".to_string(),
            ));
        }
        validate_non_empty("predicted_effect", &input.predicted_effect)?;

        let now = Utc::now();
        let skill = ProjectSkill {
            id: ProjectSkillId::new(),
            project_id: input.outcome.project_id.clone(),
            title: input.title,
            bucket: input.bucket,
            stage: input.stage,
            status: ProjectSkillLifecycleStatus::Staged,
            pinned: false,
            archived: false,
            scope_paths: input.scope_paths,
            compact_guidance: input.compact_guidance,
            body_markdown: input.body_markdown,
            predicted_effect: Some(input.predicted_effect),
            provenance_json: serde_json::json!({
                "source": "task_outcome",
                "outcome_id": input.outcome.id.as_str(),
                "outcome_source": input.outcome.source,
                "outcome_source_ref_kind": input.outcome.source_ref_kind,
                "outcome_source_ref_id": input.outcome.source_ref_id,
                "outcome_class": input.outcome.outcome_class,
                "task_id": input.outcome.task_id,
                "agent_run_id": input.outcome.agent_run_id,
                "review_id": input.outcome.review_id,
                "additional": input.additional_provenance,
            }),
            companion_of_skill_id: None,
            created_at: now,
            updated_at: now,
        };

        self.stage_skill(skill).await
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

    pub async fn prompt_selected_citations(
        &self,
        project_id: &ProjectId,
        prompt: &str,
    ) -> AppResult<Vec<LearnedSkillConstraintCitation>> {
        Ok(self
            .prompt_selected_skills(project_id, prompt)
            .await?
            .into_iter()
            .map(project_skill_to_constraint_citation)
            .collect())
    }

    pub async fn prompt_selected_skills(
        &self,
        project_id: &ProjectId,
        prompt: &str,
    ) -> AppResult<Vec<ProjectSkill>> {
        let mut skills = Vec::new();
        for skill_id in extract_project_skill_directives(prompt) {
            let Some(skill) = self
                .repo
                .get_by_id(&ProjectSkillId::from_string(skill_id))
                .await?
            else {
                continue;
            };
            if &skill.project_id != project_id
                || skill.status != ProjectSkillLifecycleStatus::Approved
                || skill.archived
            {
                continue;
            }
            skills.push(skill);
        }
        Ok(skills)
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

fn extract_project_skill_directives(text: &str) -> Vec<String> {
    let mut skill_ids = BTreeSet::new();
    for line in text.lines() {
        if let Some(index) = line.find("ralphx_project_skill=") {
            let raw = &line[index + "ralphx_project_skill=".len()..];
            if let Some(value) = raw.split_whitespace().next() {
                let skill_id = value
                    .trim_matches(|char| matches!(char, '<' | '>' | '-' | '"' | '\'' | ';' | ','));
                if is_safe_project_skill_id(skill_id) {
                    skill_ids.insert(skill_id.to_string());
                }
            }
        }
    }
    skill_ids.into_iter().collect()
}

fn is_safe_project_skill_id(skill_id: &str) -> bool {
    !skill_id.is_empty()
        && skill_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn project_skill_to_constraint_citation(skill: ProjectSkill) -> LearnedSkillConstraintCitation {
    LearnedSkillConstraintCitation {
        skill_id: skill.id.as_str().to_string(),
        title: skill.title,
        predicted_effect: skill.predicted_effect.unwrap_or_default(),
        compact_guidance: skill.compact_guidance,
        provenance_refs: provenance_refs_from_json(&skill.provenance_json),
    }
}

fn provenance_refs_from_json(value: &Value) -> Vec<String> {
    for key in ["provenance_refs", "refs"] {
        if let Some(refs) = value.get(key).and_then(Value::as_array) {
            return refs
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect();
        }
    }
    value
        .get("source")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| vec![value.to_string()])
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::Utc;
    use serde_json::json;

    use super::{
        new_empty_task_outcome, new_skill_usage_event, ProjectSkillService, SkillUsageService,
        StageProjectSkillFromOutcomeInput,
    };
    use crate::domain::entities::types::ProjectId;
    use crate::domain::entities::{
        ProjectSkill, ProjectSkillId, ProjectSkillLifecycleStatus, TaskOutcomeStatus,
    };
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

    #[tokio::test]
    async fn project_skill_distillation_requires_eligible_outcome() {
        let service = ProjectSkillService::new(Arc::new(MemoryProjectSkillRepository::new()));
        let project_id = ProjectId::from_string("project-1".to_string());
        let outcome = new_empty_task_outcome(project_id, "review", "review_note", "review-1");

        let result = service
            .stage_skill_from_outcome(StageProjectSkillFromOutcomeInput {
                outcome,
                title: "Use review feedback as regression guidance".to_string(),
                bucket: "review".to_string(),
                stage: "review".to_string(),
                scope_paths: Vec::new(),
                compact_guidance: "Check the same regression before approving.".to_string(),
                body_markdown: "Detailed guidance".to_string(),
                predicted_effect: "Reduces repeat review changes.".to_string(),
                additional_provenance: json!({ "test": true }),
            })
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn project_skill_distillation_stages_skill_with_outcome_provenance() {
        let service = ProjectSkillService::new(Arc::new(MemoryProjectSkillRepository::new()));
        let project_id = ProjectId::from_string("project-1".to_string());
        let mut outcome =
            new_empty_task_outcome(project_id.clone(), "merge_validation", "task", "task-1");
        outcome.status = TaskOutcomeStatus::Eligible;
        outcome.outcome_class = Some("merge_validation_failed".to_string());
        outcome.task_id = Some("task-1".to_string());

        let staged = service
            .stage_skill_from_outcome(StageProjectSkillFromOutcomeInput {
                outcome: outcome.clone(),
                title: "Run merge validation before marking complete".to_string(),
                bucket: "merge".to_string(),
                stage: "review".to_string(),
                scope_paths: vec!["src-tauri".to_string()],
                compact_guidance: "Before approving merge recovery, check the validation failure class.".to_string(),
                body_markdown: "Use the validation log as evidence before repeating a failed merge.".to_string(),
                predicted_effect: "Prevents repeated failed merge validation loops.".to_string(),
                additional_provenance: json!({ "distiller": "service-test" }),
            })
            .await
            .unwrap();

        assert_eq!(staged.project_id, project_id);
        assert_eq!(staged.status, ProjectSkillLifecycleStatus::Staged);
        assert_eq!(
            staged.predicted_effect.as_deref(),
            Some("Prevents repeated failed merge validation loops.")
        );
        assert_eq!(
            staged.provenance_json["outcome_id"].as_str(),
            Some(outcome.id.as_str())
        );
        assert_eq!(
            staged.provenance_json["outcome_source"].as_str(),
            Some("merge_validation")
        );
        assert_eq!(
            staged.provenance_json["additional"]["distiller"].as_str(),
            Some("service-test")
        );
    }

    #[tokio::test]
    async fn prompt_selected_citations_resolve_only_approved_same_project_skills() {
        let skill_repo = Arc::new(MemoryProjectSkillRepository::new());
        let skill_service = ProjectSkillService::new(skill_repo);
        let project_id = ProjectId::from_string("project-1".to_string());
        let other_project_id = ProjectId::from_string("project-2".to_string());

        let approved = skill_service
            .stage_skill(staged_skill(project_id.clone()))
            .await
            .unwrap();
        let approved = skill_service
            .approve_skill(&approved.id)
            .await
            .unwrap()
            .unwrap();

        let staged = skill_service
            .stage_skill(staged_skill(project_id.clone()))
            .await
            .unwrap();

        let archived = skill_service
            .stage_skill(staged_skill(project_id.clone()))
            .await
            .unwrap();
        let archived = skill_service
            .approve_skill(&archived.id)
            .await
            .unwrap()
            .unwrap();
        skill_service
            .archive_skill(&archived.id)
            .await
            .unwrap()
            .unwrap();

        let other_project = skill_service
            .stage_skill(staged_skill(other_project_id))
            .await
            .unwrap();
        let other_project = skill_service
            .approve_skill(&other_project.id)
            .await
            .unwrap()
            .unwrap();

        let prompt = format!(
            "Use these.\n\
             <!-- ralphx_project_skill={} -->\n\
             <!-- ralphx_project_skill={} -->\n\
             <!-- ralphx_project_skill={} -->\n\
             <!-- ralphx_project_skill={} -->\n\
             <!-- ralphx_project_skill=../bad -->\n\
             <!-- ralphx_project_skill={} -->",
            approved.id.as_str(),
            staged.id.as_str(),
            archived.id.as_str(),
            other_project.id.as_str(),
            approved.id.as_str()
        );

        let citations = skill_service
            .prompt_selected_citations(&project_id, &prompt)
            .await
            .unwrap();

        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0].skill_id, approved.id.as_str());
        assert_eq!(
            citations[0].predicted_effect,
            "Avoids adapter-only injection."
        );

        let selected_skills = skill_service
            .prompt_selected_skills(&project_id, &prompt)
            .await
            .unwrap();

        assert_eq!(selected_skills.len(), 1);
        assert_eq!(selected_skills[0].id, approved.id);
        assert_eq!(selected_skills[0].stage, "execution");
        assert_eq!(selected_skills[0].bucket, "execution");
    }
}
