use std::sync::Arc;

use crate::domain::entities::{
    ProjectId, ProjectSkillCreatedBy, ProjectSkillId, ProjectSkillLifecycleStatus,
};
use crate::domain::repositories::{
    ProjectSkillListOptions, ProjectSkillRepository, ProjectSkillResolutionOutcome,
};
use crate::domain::services::project_skill_pipeline::{
    ProjectSkillPipelineContext, ProjectSkillPipelineInput, ProjectSkillPipelineService,
    PROJECT_SKILL_BODY_MAX_CHARS, PROJECT_SKILL_COMPACT_GUIDANCE_MAX_CHARS,
    PROJECT_SKILL_PREDICTED_EFFECT_MAX_CHARS, PROJECT_SKILL_TITLE_MAX_CHARS,
};
use crate::error::AppError;
use crate::testing::MemoryProjectSkillRepository;

fn context(role: &str) -> ProjectSkillPipelineContext {
    ProjectSkillPipelineContext {
        agent_name: match role {
            "memory_capture" => "ralphx-memory-capture",
            "memory_maintainer" => "ralphx-memory-maintainer",
            _ => "ralphx-memory-capture",
        }
        .to_string(),
        pipeline_role: role.to_string(),
        project_id: ProjectId::from_string("project-1".to_string()),
        context_type: "project".to_string(),
        context_id: "project-1".to_string(),
        conversation_id: "conversation-1".to_string(),
        agent_run_id: Some("run-1".to_string()),
        task_id: None,
    }
}

fn input(title: &str) -> ProjectSkillPipelineInput {
    ProjectSkillPipelineInput {
        project_id: ProjectId::from_string("project-1".to_string()),
        title: title.to_string(),
        bucket: "execution".to_string(),
        stage: "execution".to_string(),
        scope_paths: vec!["src-tauri/src/domain/**".to_string()],
        compact_guidance: "Route project-skill writes through the owning service.".to_string(),
        body_markdown: "## Procedure\n\n1. Reuse the project-skill resolution seam.".to_string(),
        predicted_effect: "Prevents competing project-skill writers.".to_string(),
    }
}

fn setup() -> (
    Arc<MemoryProjectSkillRepository>,
    ProjectSkillPipelineService,
) {
    let repo = Arc::new(MemoryProjectSkillRepository::new());
    let repository: Arc<dyn ProjectSkillRepository> = repo.clone();
    let service = ProjectSkillPipelineService::new(repository);
    (repo, service)
}

#[tokio::test]
async fn pipeline_upsert_and_patch_derive_attribution_and_preserve_versions() {
    let (repo, service) = setup();
    let runtime = context("memory_capture");
    let created = service
        .upsert(runtime.clone(), input("Resolution ownership"))
        .await
        .expect("create pipeline skill");

    assert_eq!(created.outcome, ProjectSkillResolutionOutcome::CreateNew);
    assert_eq!(created.skill.created_by, ProjectSkillCreatedBy::Agent);
    assert_eq!(
        created.skill.pipeline_role.as_deref(),
        Some("memory_capture")
    );
    assert_eq!(
        created.skill.provenance_json["source"].as_str(),
        Some("skill_pipeline_mcp")
    );
    assert_eq!(
        created.skill.provenance_json["additional"]["agent_name"].as_str(),
        Some("ralphx-memory-capture")
    );
    assert_eq!(
        created.skill.provenance_json["additional"]["context_id"].as_str(),
        Some("project-1")
    );
    assert_eq!(
        repo.list_versions(&created.skill.id)
            .await
            .expect("versions")
            .len(),
        1
    );

    let duplicate = service
        .upsert(runtime.clone(), input("Resolution ownership"))
        .await
        .expect("duplicate retry");
    assert_eq!(duplicate.outcome, ProjectSkillResolutionOutcome::Duplicate);
    assert!(duplicate.version.is_none());

    let mut revised = input("Resolution ownership");
    revised.body_markdown.push_str("\n2. Verify one writer.");
    let patched = service
        .patch(runtime, created.skill.id.clone(), revised)
        .await
        .expect("patch pipeline skill");
    assert_eq!(
        patched.outcome,
        ProjectSkillResolutionOutcome::PatchExisting
    );
    assert_eq!(patched.skill.id, created.skill.id);
    assert_eq!(
        repo.list_versions(&created.skill.id)
            .await
            .expect("versions")
            .len(),
        2
    );
}

#[tokio::test]
async fn pipeline_validation_rejects_boundaries_templates_and_unsafe_paths_without_writes() {
    let (repo, service) = setup();
    let runtime = context("memory_capture");
    let invalid = [
        {
            let value = input(&"t".repeat(PROJECT_SKILL_TITLE_MAX_CHARS + 1));
            value
        },
        {
            let mut value = input("Compact overflow");
            value.compact_guidance = "g".repeat(PROJECT_SKILL_COMPACT_GUIDANCE_MAX_CHARS + 1);
            value
        },
        {
            let mut value = input("Body overflow");
            value.body_markdown = "b".repeat(PROJECT_SKILL_BODY_MAX_CHARS + 1);
            value
        },
        {
            let mut value = input("Effect overflow");
            value.predicted_effect = "e".repeat(PROJECT_SKILL_PREDICTED_EFFECT_MAX_CHARS + 1);
            value
        },
        input("Draft procedure for legacy output"),
        input("Draft implementation procedure from legacy output"),
        input("Draft procedure from PR #42"),
        {
            let mut value = input("Legacy body marker");
            value.body_markdown = "## Authoring required\n\nRewrite me.".to_string();
            value
        },
        {
            let mut value = input("Unsafe scope");
            value.scope_paths = vec!["../outside".to_string()];
            value
        },
        {
            let mut value = input("Invalid bucket");
            value.bucket = "memory".to_string();
            value
        },
    ];

    for value in invalid {
        assert!(matches!(
            service.upsert(runtime.clone(), value).await,
            Err(AppError::Validation(_))
        ));
    }
    let mut mismatched_authority = context("memory_capture");
    mismatched_authority.agent_name = "ralphx-memory-maintainer".to_string();
    assert!(matches!(
        service
            .upsert(mismatched_authority, input("Invalid authority"))
            .await,
        Err(AppError::Validation(_))
    ));

    let project_id = ProjectId::from_string("project-1".to_string());
    assert!(repo
        .list_by_project(&project_id, ProjectSkillListOptions::default())
        .await
        .expect("rows")
        .is_empty());
}

#[tokio::test]
async fn pipeline_validation_accepts_exact_character_limits() {
    let (_repo, service) = setup();
    let mut exact = input(&"t".repeat(PROJECT_SKILL_TITLE_MAX_CHARS));
    exact.compact_guidance = "g".repeat(PROJECT_SKILL_COMPACT_GUIDANCE_MAX_CHARS);
    exact.body_markdown = "b".repeat(PROJECT_SKILL_BODY_MAX_CHARS);
    exact.predicted_effect = "e".repeat(PROJECT_SKILL_PREDICTED_EFFECT_MAX_CHARS);

    let result = service
        .upsert(context("memory_capture"), exact)
        .await
        .expect("exact limits are valid");
    assert_eq!(result.outcome, ProjectSkillResolutionOutcome::CreateNew);
}

#[tokio::test]
async fn pipeline_retire_is_scoped_guarded_and_idempotent_without_versions() {
    let (repo, service) = setup();
    let runtime = context("memory_maintainer");
    let created = service
        .upsert(runtime.clone(), input("Retirement guard"))
        .await
        .expect("create skill");
    let version_count = repo
        .list_versions(&created.skill.id)
        .await
        .expect("versions")
        .len();

    let retired = service
        .retire(
            runtime.clone(),
            &created.skill.project_id,
            &created.skill.id,
        )
        .await
        .expect("retire skill");
    assert!(retired.changed);
    assert_eq!(retired.skill.status, ProjectSkillLifecycleStatus::Retired);

    let retried = service
        .retire(runtime, &created.skill.project_id, &created.skill.id)
        .await
        .expect("idempotent retire");
    assert!(!retried.changed);
    assert_eq!(retried.skill.updated_at, retired.skill.updated_at);
    assert_eq!(
        repo.list_versions(&created.skill.id)
            .await
            .expect("versions")
            .len(),
        version_count
    );
}

#[tokio::test]
async fn pipeline_patch_of_approved_skill_creates_staged_companion_with_trusted_attribution() {
    let (repo, service) = setup();
    let runtime = context("memory_capture");
    let created = service
        .upsert(runtime.clone(), input("Approved revision"))
        .await
        .expect("create skill");
    repo.update_lifecycle_status(&created.skill.id, ProjectSkillLifecycleStatus::Approved)
        .await
        .expect("approve")
        .expect("skill");

    let mut revised = input("Approved revision");
    revised
        .body_markdown
        .push_str("\n2. Stage a companion revision.");
    let patched = service
        .patch(runtime, created.skill.id.clone(), revised)
        .await
        .expect("stage approved companion");

    assert_eq!(patched.outcome, ProjectSkillResolutionOutcome::CreateNew);
    assert_ne!(patched.skill.id, created.skill.id);
    assert_eq!(
        patched.skill.companion_of_skill_id.as_ref(),
        Some(&created.skill.id)
    );
    assert_eq!(patched.skill.status, ProjectSkillLifecycleStatus::Staged);
    assert_eq!(patched.skill.created_by, ProjectSkillCreatedBy::Agent);
    assert_eq!(
        patched.skill.pipeline_role.as_deref(),
        Some("memory_capture")
    );
    assert_eq!(
        repo.list_versions(&created.skill.id)
            .await
            .expect("approved versions")
            .len(),
        1
    );
    assert_eq!(
        repo.list_versions(&patched.skill.id)
            .await
            .expect("companion versions")
            .len(),
        1
    );
}

#[tokio::test]
async fn pipeline_retire_rejects_excluded_lifecycle_without_mutation() {
    let (repo, service) = setup();
    let runtime = context("memory_maintainer");
    let created = service
        .upsert(runtime.clone(), input("Excluded retirement"))
        .await
        .expect("create skill");
    let rejected = repo
        .update_lifecycle_status(&created.skill.id, ProjectSkillLifecycleStatus::Rejected)
        .await
        .expect("reject")
        .expect("skill");
    let version_count = repo
        .list_versions(&created.skill.id)
        .await
        .expect("versions")
        .len();

    assert!(matches!(
        service
            .retire(runtime, &created.skill.project_id, &created.skill.id)
            .await,
        Err(AppError::Conflict(_))
    ));

    let stored = repo
        .get_by_id(&created.skill.id)
        .await
        .expect("read")
        .expect("skill");
    assert_eq!(stored.status, ProjectSkillLifecycleStatus::Rejected);
    assert_eq!(stored.updated_at, rejected.updated_at);
    assert_eq!(
        repo.list_versions(&created.skill.id)
            .await
            .expect("versions after rejection")
            .len(),
        version_count
    );
}

#[tokio::test]
async fn pipeline_patch_and_retire_reject_cross_project_and_pinned_targets_unchanged() {
    let (repo, service) = setup();
    let runtime = context("memory_maintainer");
    let created = service
        .upsert(runtime.clone(), input("Scoped target"))
        .await
        .expect("create skill");
    repo.update_pinned(&created.skill.id, true)
        .await
        .expect("pin")
        .expect("skill");

    let other_project = ProjectId::from_string("project-2".to_string());
    let mut cross_project_input = input("Cross-project patch");
    cross_project_input.project_id = other_project.clone();
    assert!(matches!(
        service
            .patch(
                runtime.clone(),
                created.skill.id.clone(),
                cross_project_input
            )
            .await,
        Err(AppError::Validation(_))
    ));
    assert!(matches!(
        service
            .retire(runtime, &other_project, &created.skill.id)
            .await,
        Err(AppError::Validation(_))
    ));
    assert!(matches!(
        service
            .retire(
                context("memory_maintainer"),
                &created.skill.project_id,
                &created.skill.id,
            )
            .await,
        Err(AppError::Conflict(_))
    ));

    let stored = repo
        .get_by_id(&ProjectSkillId::from_string(
            created.skill.id.as_str().to_string(),
        ))
        .await
        .expect("read")
        .expect("skill");
    assert!(stored.pinned);
    assert_eq!(stored.status, ProjectSkillLifecycleStatus::Staged);
    assert_eq!(
        repo.list_versions(&stored.id)
            .await
            .expect("versions")
            .len(),
        1
    );
}
