use std::sync::Arc;

use chrono::{Duration, Utc};

use crate::domain::entities::{
    ProjectId, ProjectSkill, ProjectSkillCreatedBy, ProjectSkillId, ProjectSkillLifecycleStatus,
};
use crate::domain::repositories::{
    ProjectSkillMatchedMutation, ProjectSkillRepository, ProjectSkillResolutionCommand,
    ProjectSkillResolutionIntent, ProjectSkillResolutionResult, ProjectSkillStagingPolicy,
};
use crate::domain::services::project_skill_resolution::{
    import_title_resolution_identity, ProjectSkillResolutionService,
};
use crate::error::{AppError, AppResult};

pub const PROJECT_SKILL_TITLE_MAX_CHARS: usize = 120;
pub const PROJECT_SKILL_COMPACT_GUIDANCE_MAX_CHARS: usize = 400;
pub const PROJECT_SKILL_BODY_MAX_CHARS: usize = 32_000;
pub const PROJECT_SKILL_PREDICTED_EFFECT_MAX_CHARS: usize = 600;
pub const PROJECT_SKILL_PIPELINE_PROJECT_SCOPE_ERROR: &str =
    "project skill target belongs to a different project";

const PIPELINE_STAGED_LIMIT: usize = 2;
const PIPELINE_WINDOW_HOURS: i64 = 24;
const PIPELINE_SOURCE: &str = "skill_pipeline_mcp";
const PROJECT_SKILL_VALUES: &[&str] =
    &["planning", "verification", "review", "execution", "merge"];
const LEGACY_TITLE_PREFIXES: &[&str] = &[
    "Draft procedure for ",
    "Draft implementation procedure from ",
    "Draft procedure from PR #",
];
const LEGACY_BODY_MARKER: &str = "## Authoring required";

#[derive(Debug, Clone)]
pub struct ProjectSkillPipelineContext {
    pub agent_name: String,
    pub pipeline_role: String,
    pub project_id: ProjectId,
    pub context_type: String,
    pub context_id: String,
    pub conversation_id: String,
    pub agent_run_id: Option<String>,
    pub task_id: Option<String>,
}

impl ProjectSkillPipelineContext {
    /// Validate backend-owned pipeline authority before a write.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Validation`] when required runtime context is missing,
    /// malformed, or does not bind a canonical memory agent to an allowed role.
    pub fn validate(&self) -> AppResult<()> {
        validate_required("pipeline agent name", &self.agent_name)?;
        validate_required("pipeline role", &self.pipeline_role)?;
        validate_required("pipeline project ID", self.project_id.as_str())?;
        validate_required("pipeline context type", &self.context_type)?;
        validate_required("pipeline context ID", &self.context_id)?;
        validate_required("pipeline conversation ID", &self.conversation_id)?;
        validate_optional("pipeline agent run ID", self.agent_run_id.as_deref())?;
        validate_optional("pipeline task ID", self.task_id.as_deref())?;

        let role_allowed = match self.agent_name.as_str() {
            "ralphx-memory-capture" => {
                matches!(self.pipeline_role.as_str(), "memory_capture" | "skill_distiller")
            }
            "ralphx-memory-maintainer" => {
                matches!(
                    self.pipeline_role.as_str(),
                    "memory_maintainer" | "skill_distiller"
                )
            }
            _ => false,
        };
        if !role_allowed {
            return Err(AppError::Validation(
                "project skill pipeline runtime authority is invalid".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct ProjectSkillPipelineInput {
    pub project_id: ProjectId,
    pub title: String,
    pub bucket: String,
    pub stage: String,
    pub scope_paths: Vec<String>,
    pub compact_guidance: String,
    pub body_markdown: String,
    pub predicted_effect: String,
}

#[derive(Debug, Clone)]
pub struct ProjectSkillPipelineRetireResult {
    pub skill: ProjectSkill,
    pub changed: bool,
}

pub struct ProjectSkillPipelineService {
    repo: Arc<dyn ProjectSkillRepository>,
}

impl ProjectSkillPipelineService {
    pub fn new(repo: Arc<dyn ProjectSkillRepository>) -> Self {
        Self { repo }
    }

    /// Create or update a staged project skill through the canonical resolver.
    ///
    /// # Errors
    ///
    /// Returns validation, conflict, or repository errors when authority, content,
    /// staging limits, resolution, or persistence checks fail.
    pub async fn upsert(
        &self,
        context: ProjectSkillPipelineContext,
        input: ProjectSkillPipelineInput,
    ) -> AppResult<ProjectSkillResolutionResult> {
        context.validate()?;
        assert_context_project(&context, &input.project_id)?;
        validate_input(&input)?;
        let candidate = build_candidate(&context, input);
        let identity = import_title_resolution_identity(
            &candidate.title,
            &candidate.bucket,
            &candidate.stage,
        );
        ProjectSkillResolutionService::new(Arc::clone(&self.repo))
            .resolve(ProjectSkillResolutionCommand {
                staging_policy: Some(staging_policy(&context)),
                candidate,
                intent: ProjectSkillResolutionIntent::Upsert {
                    identities: vec![identity],
                    matched_mutation: ProjectSkillMatchedMutation::PatchExisting,
                },
                evidence_markdown: None,
            })
            .await
    }

    /// Patch an existing scoped project skill through the canonical resolver.
    ///
    /// # Errors
    ///
    /// Returns validation, not-found, conflict, or repository errors when authority,
    /// project ownership, lifecycle, staging limits, resolution, or persistence fail.
    pub async fn patch(
        &self,
        context: ProjectSkillPipelineContext,
        target_id: ProjectSkillId,
        input: ProjectSkillPipelineInput,
    ) -> AppResult<ProjectSkillResolutionResult> {
        context.validate()?;
        assert_context_project(&context, &input.project_id)?;
        validate_input(&input)?;
        let target = self
            .repo
            .get_by_id(&target_id)
            .await?
            .ok_or_else(|| AppError::NotFound("project skill was not found".to_string()))?;
        assert_same_project(&target.project_id, &input.project_id)?;
        let candidate = build_candidate(&context, input);
        ProjectSkillResolutionService::new(Arc::clone(&self.repo))
            .resolve(ProjectSkillResolutionCommand {
                staging_policy: Some(staging_policy(&context)),
                candidate,
                intent: ProjectSkillResolutionIntent::ExplicitPatch { target_id },
                evidence_markdown: None,
            })
            .await
    }

    /// Retire an unpinned active project skill without creating a content version.
    ///
    /// # Errors
    ///
    /// Returns validation, not-found, conflict, or repository errors when authority,
    /// project ownership, pin state, lifecycle, or persistence checks fail.
    pub async fn retire(
        &self,
        context: ProjectSkillPipelineContext,
        project_id: &ProjectId,
        target_id: &ProjectSkillId,
    ) -> AppResult<ProjectSkillPipelineRetireResult> {
        context.validate()?;
        assert_context_project(&context, project_id)?;
        let target = self
            .repo
            .get_by_id(target_id)
            .await?
            .ok_or_else(|| AppError::NotFound("project skill was not found".to_string()))?;
        assert_same_project(&target.project_id, project_id)?;
        if target.pinned {
            return Err(AppError::Conflict(
                "pinned project skills cannot be retired".to_string(),
            ));
        }
        match target.status {
            ProjectSkillLifecycleStatus::Retired => {
                return Ok(ProjectSkillPipelineRetireResult {
                    skill: target,
                    changed: false,
                });
            }
            ProjectSkillLifecycleStatus::Staged
            | ProjectSkillLifecycleStatus::Approved
            | ProjectSkillLifecycleStatus::Stale => {}
            ProjectSkillLifecycleStatus::Rejected | ProjectSkillLifecycleStatus::Archived => {
                return Err(AppError::Conflict(
                    "rejected or archived project skills cannot be retired".to_string(),
                ));
            }
        }
        let skill = self
            .repo
            .update_lifecycle_status(target_id, ProjectSkillLifecycleStatus::Retired)
            .await?
            .ok_or_else(|| {
                AppError::Conflict("project skill changed during retirement".to_string())
            })?;
        Ok(ProjectSkillPipelineRetireResult {
            skill,
            changed: true,
        })
    }
}

fn build_candidate(
    context: &ProjectSkillPipelineContext,
    input: ProjectSkillPipelineInput,
) -> ProjectSkill {
    let now = Utc::now();
    ProjectSkill {
        id: ProjectSkillId::new(),
        project_id: input.project_id,
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
            "source": PIPELINE_SOURCE,
            "additional": {
                "agent_name": context.agent_name,
                "pipeline_role": context.pipeline_role,
                "context_type": context.context_type,
                "context_id": context.context_id,
                "conversation_id": context.conversation_id,
                "agent_run_id": context.agent_run_id,
                "task_id": context.task_id,
            }
        }),
        companion_of_skill_id: None,
        content_hash: String::new(),
        evidence_hash: String::new(),
        created_by: ProjectSkillCreatedBy::Agent,
        pipeline_role: Some(context.pipeline_role.clone()),
        created_at: now,
        updated_at: now,
    }
}

fn staging_policy(context: &ProjectSkillPipelineContext) -> ProjectSkillStagingPolicy {
    ProjectSkillStagingPolicy {
        pipeline_role: context.pipeline_role.clone(),
        max_staged: PIPELINE_STAGED_LIMIT,
        window_start: Utc::now() - Duration::hours(PIPELINE_WINDOW_HOURS),
    }
}

fn validate_input(input: &ProjectSkillPipelineInput) -> AppResult<()> {
    validate_bounded("project skill title", &input.title, PROJECT_SKILL_TITLE_MAX_CHARS)?;
    validate_enum("project skill bucket", &input.bucket)?;
    validate_enum("project skill stage", &input.stage)?;
    validate_bounded(
        "project skill compact_guidance",
        &input.compact_guidance,
        PROJECT_SKILL_COMPACT_GUIDANCE_MAX_CHARS,
    )?;
    validate_bounded(
        "project skill body_markdown",
        &input.body_markdown,
        PROJECT_SKILL_BODY_MAX_CHARS,
    )?;
    validate_bounded(
        "project skill predicted_effect",
        &input.predicted_effect,
        PROJECT_SKILL_PREDICTED_EFFECT_MAX_CHARS,
    )?;
    if LEGACY_TITLE_PREFIXES
        .iter()
        .any(|prefix| input.title.starts_with(prefix))
        || input.body_markdown.contains(LEGACY_BODY_MARKER)
    {
        return Err(AppError::Validation(
            "deterministic draft templates cannot be submitted to the skill pipeline".to_string(),
        ));
    }
    for path in &input.scope_paths {
        if !is_safe_scope_path(path) {
            return Err(AppError::Validation(format!(
                "invalid project skill scope path: {path}"
            )));
        }
    }
    Ok(())
}

fn validate_bounded(label: &str, value: &str, max_chars: usize) -> AppResult<()> {
    validate_required(label, value)?;
    if value.chars().count() > max_chars {
        return Err(AppError::Validation(format!(
            "{label} must be at most {max_chars} characters"
        )));
    }
    Ok(())
}

fn validate_enum(label: &str, value: &str) -> AppResult<()> {
    validate_required(label, value)?;
    if !PROJECT_SKILL_VALUES.contains(&value) {
        return Err(AppError::Validation(format!(
            "{label} must be one of {}",
            PROJECT_SKILL_VALUES.join(", ")
        )));
    }
    Ok(())
}

fn validate_required(label: &str, value: &str) -> AppResult<()> {
    if value.trim().is_empty() || value != value.trim() {
        return Err(AppError::Validation(format!(
            "{label} must be trimmed and non-empty"
        )));
    }
    Ok(())
}

fn validate_optional(label: &str, value: Option<&str>) -> AppResult<()> {
    if let Some(value) = value {
        validate_required(label, value)?;
    }
    Ok(())
}

fn assert_context_project(
    context: &ProjectSkillPipelineContext,
    project_id: &ProjectId,
) -> AppResult<()> {
    assert_same_project(&context.project_id, project_id)
}

fn assert_same_project(left: &ProjectId, right: &ProjectId) -> AppResult<()> {
    if left != right {
        return Err(AppError::Validation(
            PROJECT_SKILL_PIPELINE_PROJECT_SCOPE_ERROR.to_string(),
        ));
    }
    Ok(())
}

fn is_safe_scope_path(path: &str) -> bool {
    let path = path.trim();
    !path.is_empty()
        && !path.starts_with('/')
        && !path.starts_with('~')
        && !path.contains('\\')
        && !path.contains('\0')
        && path
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}
