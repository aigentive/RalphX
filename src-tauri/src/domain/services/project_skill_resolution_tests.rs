use std::sync::Arc;

use chrono::Utc;

use super::project_skill_resolution::ProjectSkillResolutionService;
use crate::domain::entities::{
    ProjectId, ProjectSkill, ProjectSkillCreatedBy, ProjectSkillId, ProjectSkillLifecycleStatus,
};
use crate::domain::repositories::{
    ProjectSkillMatchedMutation, ProjectSkillRepository, ProjectSkillResolutionCommand,
    ProjectSkillResolutionIdentity, ProjectSkillResolutionIdentityKind,
    ProjectSkillResolutionIntent, ProjectSkillResolutionOutcome,
};
use crate::error::AppError;
use crate::testing::MemoryProjectSkillRepository;

fn skill(
    project_id: &ProjectId,
    id: &str,
    status: ProjectSkillLifecycleStatus,
    body: &str,
    provenance: serde_json::Value,
) -> ProjectSkill {
    let now = Utc::now();
    ProjectSkill {
        id: ProjectSkillId::from_string(id),
        project_id: project_id.clone(),
        title: "Review procedure".to_string(),
        bucket: "review".to_string(),
        stage: "review".to_string(),
        status,
        pinned: status == ProjectSkillLifecycleStatus::Approved,
        archived: false,
        scope_paths: vec!["src".to_string()],
        compact_guidance: "Review the bounded change.".to_string(),
        body_markdown: body.to_string(),
        predicted_effect: Some("Fewer review regressions.".to_string()),
        provenance_json: provenance,
        companion_of_skill_id: None,
        content_hash: String::new(),
        evidence_hash: String::new(),
        created_by: ProjectSkillCreatedBy::Agent,
        pipeline_role: None,
        created_at: now,
        updated_at: now,
    }
}

fn outcome_identity(value: &str) -> ProjectSkillResolutionIdentity {
    ProjectSkillResolutionIdentity {
        kind: ProjectSkillResolutionIdentityKind::Outcome,
        value: value.to_string(),
    }
}

fn recurrence_identity(value: &str) -> ProjectSkillResolutionIdentity {
    ProjectSkillResolutionIdentity {
        kind: ProjectSkillResolutionIdentityKind::Recurrence,
        value: value.to_string(),
    }
}

fn upsert_command(
    candidate: ProjectSkill,
    mutation: ProjectSkillMatchedMutation,
) -> ProjectSkillResolutionCommand {
    ProjectSkillResolutionCommand {
        candidate,
        intent: ProjectSkillResolutionIntent::Upsert {
            identities: vec![outcome_identity("outcome-1")],
            matched_mutation: mutation,
        },
        evidence_markdown: None,
        staging_policy: None,
    }
}

#[tokio::test]
async fn create_and_duplicate_share_one_versioned_resolution_path() {
    let project_id = ProjectId::from_string("project-1".to_string());
    let repo: Arc<dyn ProjectSkillRepository> = Arc::new(MemoryProjectSkillRepository::new());
    let service = ProjectSkillResolutionService::new(Arc::clone(&repo));
    let candidate = skill(
        &project_id,
        "candidate-1",
        ProjectSkillLifecycleStatus::Staged,
        "First body",
        serde_json::json!({"outcome_id": "outcome-1"}),
    );

    let created = service
        .resolve(upsert_command(
            candidate.clone(),
            ProjectSkillMatchedMutation::PatchExisting,
        ))
        .await
        .unwrap();
    assert_eq!(created.outcome, ProjectSkillResolutionOutcome::CreateNew);
    assert_eq!(
        created.version.as_ref().map(|version| version.version),
        Some(1)
    );

    let duplicate = service
        .resolve(upsert_command(
            candidate,
            ProjectSkillMatchedMutation::PatchExisting,
        ))
        .await
        .unwrap();
    assert_eq!(duplicate.outcome, ProjectSkillResolutionOutcome::Duplicate);
    assert!(duplicate.version.is_none());
    assert_eq!(
        repo.list_versions(&created.skill.id).await.unwrap().len(),
        1
    );
}

#[tokio::test]
async fn recurrence_identity_appends_evidence_through_the_existing_resolution_path() {
    let project_id = ProjectId::from_string("project-1".to_string());
    let key = format!("token-set-v1:{}", "d".repeat(64));
    let repo: Arc<dyn ProjectSkillRepository> = Arc::new(MemoryProjectSkillRepository::new());
    let service = ProjectSkillResolutionService::new(Arc::clone(&repo));
    let existing = skill(
        &project_id,
        "recurrence-skill",
        ProjectSkillLifecycleStatus::Staged,
        "Existing body",
        serde_json::json!({ "additional": { "recurrence_key": key } }),
    );
    repo.seed_for_test(existing.clone()).await.unwrap();
    let candidate = skill(
        &project_id,
        "candidate",
        ProjectSkillLifecycleStatus::Staged,
        "Ignored replacement",
        serde_json::json!({
            "additional": { "recurrence_key": key },
            "outcome_id": "outcome-2",
        }),
    );

    let result = service
        .resolve(ProjectSkillResolutionCommand {
            candidate,
            intent: ProjectSkillResolutionIntent::Upsert {
                identities: vec![recurrence_identity(&key)],
                matched_mutation: ProjectSkillMatchedMutation::AppendEvidence,
            },
            evidence_markdown: Some("## Additional evidence\n\n- session two".to_string()),
            staging_policy: None,
        })
        .await
        .expect("append recurrence evidence");

    assert_eq!(
        result.outcome,
        ProjectSkillResolutionOutcome::AppendEvidence
    );
    assert_eq!(result.skill.id, existing.id);
    assert!(result.skill.body_markdown.contains("session two"));
}

#[tokio::test]
async fn recurrence_identity_keeps_approved_guidance_immutable_and_creates_a_companion() {
    let project_id = ProjectId::from_string("project-1".to_string());
    let key = format!("token-set-v1:{}", "e".repeat(64));
    let repo: Arc<dyn ProjectSkillRepository> = Arc::new(MemoryProjectSkillRepository::new());
    let service = ProjectSkillResolutionService::new(Arc::clone(&repo));
    let approved = skill(
        &project_id,
        "approved-recurrence",
        ProjectSkillLifecycleStatus::Approved,
        "Approved body",
        serde_json::json!({ "additional": { "recurrence_key": key } }),
    );
    repo.seed_for_test(approved.clone()).await.unwrap();
    let candidate = skill(
        &project_id,
        "candidate",
        ProjectSkillLifecycleStatus::Staged,
        "Agent-authored recurrence revision",
        serde_json::json!({
            "additional": { "recurrence_key": key },
            "outcome_id": "outcome-2",
        }),
    );

    let result = service
        .resolve(ProjectSkillResolutionCommand {
            candidate,
            intent: ProjectSkillResolutionIntent::Upsert {
                identities: vec![recurrence_identity(&key)],
                matched_mutation: ProjectSkillMatchedMutation::AppendEvidence,
            },
            evidence_markdown: Some("## Additional evidence\n\n- second session".to_string()),
            staging_policy: None,
        })
        .await
        .expect("create recurrence companion");

    assert_eq!(result.outcome, ProjectSkillResolutionOutcome::CreateNew);
    assert_eq!(
        result.skill.companion_of_skill_id,
        Some(approved.id.clone())
    );
    assert_eq!(
        repo.get_by_id(&approved.id)
            .await
            .unwrap()
            .unwrap()
            .body_markdown,
        "Approved body"
    );
}

#[tokio::test]
async fn patch_and_append_evidence_create_monotonic_matching_snapshots() {
    let project_id = ProjectId::from_string("project-1".to_string());
    let repo: Arc<dyn ProjectSkillRepository> = Arc::new(MemoryProjectSkillRepository::new());
    let service = ProjectSkillResolutionService::new(Arc::clone(&repo));
    let initial = skill(
        &project_id,
        "candidate-1",
        ProjectSkillLifecycleStatus::Staged,
        "First body",
        serde_json::json!({"outcome_id": "outcome-1", "evidence": "first"}),
    );
    let created = service
        .resolve(upsert_command(
            initial,
            ProjectSkillMatchedMutation::PatchExisting,
        ))
        .await
        .unwrap();

    let patched_candidate = skill(
        &project_id,
        "ignored-candidate-id",
        ProjectSkillLifecycleStatus::Staged,
        "Revised body",
        serde_json::json!({"outcome_id": "outcome-1", "evidence": "first"}),
    );
    let patched = service
        .resolve(upsert_command(
            patched_candidate,
            ProjectSkillMatchedMutation::PatchExisting,
        ))
        .await
        .unwrap();
    assert_eq!(
        patched.outcome,
        ProjectSkillResolutionOutcome::PatchExisting
    );
    assert_eq!(patched.skill.id, created.skill.id);
    assert_eq!(
        patched.version.as_ref().map(|version| version.version),
        Some(2)
    );

    let evidence_candidate = skill(
        &project_id,
        "another-ignored-id",
        ProjectSkillLifecycleStatus::Staged,
        "Caller-authored replacement is not used for evidence append",
        serde_json::json!({"outcome_id": "outcome-1", "evidence": "second"}),
    );
    let appended = service
        .resolve(ProjectSkillResolutionCommand {
            candidate: evidence_candidate.clone(),
            intent: ProjectSkillResolutionIntent::Upsert {
                identities: vec![outcome_identity("outcome-1")],
                matched_mutation: ProjectSkillMatchedMutation::AppendEvidence,
            },
            evidence_markdown: Some("## Additional evidence\n\n- second".to_string()),
            staging_policy: None,
        })
        .await
        .unwrap();
    assert_eq!(
        appended.outcome,
        ProjectSkillResolutionOutcome::AppendEvidence
    );
    assert!(appended.skill.body_markdown.contains("Revised body"));
    assert!(appended.skill.body_markdown.contains("- second"));
    assert_eq!(
        appended.version.as_ref().map(|version| version.version),
        Some(3)
    );

    let duplicate = service
        .resolve(ProjectSkillResolutionCommand {
            candidate: evidence_candidate,
            intent: ProjectSkillResolutionIntent::Upsert {
                identities: vec![outcome_identity("outcome-1")],
                matched_mutation: ProjectSkillMatchedMutation::AppendEvidence,
            },
            evidence_markdown: Some("## Additional evidence\n\n- second".to_string()),
            staging_policy: None,
        })
        .await
        .unwrap();
    assert_eq!(duplicate.outcome, ProjectSkillResolutionOutcome::Duplicate);
    assert_eq!(
        repo.list_versions(&created.skill.id).await.unwrap().len(),
        3
    );
}

#[tokio::test]
async fn approved_patch_creates_and_then_reuses_a_staged_pending_revision() {
    let project_id = ProjectId::from_string("project-1".to_string());
    let repo: Arc<dyn ProjectSkillRepository> = Arc::new(MemoryProjectSkillRepository::new());
    let service = ProjectSkillResolutionService::new(Arc::clone(&repo));
    let approved = skill(
        &project_id,
        "approved-1",
        ProjectSkillLifecycleStatus::Approved,
        "Approved body",
        serde_json::json!({"outcome_id": "outcome-1", "source": "manual"}),
    );
    repo.seed_for_test(approved.clone()).await.unwrap();

    let proposed = skill(
        &project_id,
        "pending-1",
        ProjectSkillLifecycleStatus::Staged,
        "Proposed body",
        serde_json::json!({}),
    );
    let created = service
        .resolve(ProjectSkillResolutionCommand {
            candidate: proposed.clone(),
            intent: ProjectSkillResolutionIntent::ExplicitPatch {
                target_id: approved.id.clone(),
            },
            evidence_markdown: None,
            staging_policy: None,
        })
        .await
        .unwrap();
    assert_eq!(created.outcome, ProjectSkillResolutionOutcome::CreateNew);
    assert_eq!(
        created.skill.companion_of_skill_id,
        Some(approved.id.clone())
    );
    assert_eq!(
        repo.get_by_id(&approved.id)
            .await
            .unwrap()
            .unwrap()
            .body_markdown,
        "Approved body"
    );

    let duplicate = service
        .resolve(ProjectSkillResolutionCommand {
            candidate: proposed,
            intent: ProjectSkillResolutionIntent::ExplicitPatch {
                target_id: approved.id.clone(),
            },
            evidence_markdown: None,
            staging_policy: None,
        })
        .await
        .unwrap();
    assert_eq!(duplicate.outcome, ProjectSkillResolutionOutcome::Duplicate);
    assert_eq!(
        repo.list_versions(&created.skill.id).await.unwrap().len(),
        1
    );
}

#[tokio::test]
async fn ambiguous_active_matches_fail_without_creating_a_masking_row() {
    let project_id = ProjectId::from_string("project-1".to_string());
    let repo: Arc<dyn ProjectSkillRepository> = Arc::new(MemoryProjectSkillRepository::new());
    let service = ProjectSkillResolutionService::new(Arc::clone(&repo));
    for id in ["existing-1", "existing-2"] {
        repo.seed_for_test(skill(
            &project_id,
            id,
            ProjectSkillLifecycleStatus::Staged,
            "Existing body",
            serde_json::json!({"outcome_id": "outcome-1"}),
        ))
        .await
        .unwrap();
    }

    let error = service
        .resolve(upsert_command(
            skill(
                &project_id,
                "candidate",
                ProjectSkillLifecycleStatus::Staged,
                "New body",
                serde_json::json!({"outcome_id": "outcome-1"}),
            ),
            ProjectSkillMatchedMutation::PatchExisting,
        ))
        .await
        .unwrap_err();
    assert!(matches!(error, AppError::Conflict(_)));
    assert_eq!(
        repo.list_by_project(&project_id, Default::default())
            .await
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn rejected_and_archived_matches_do_not_block_create_new() {
    let project_id = ProjectId::from_string("project-1".to_string());
    let repo: Arc<dyn ProjectSkillRepository> = Arc::new(MemoryProjectSkillRepository::new());
    let service = ProjectSkillResolutionService::new(Arc::clone(&repo));
    let mut archived = skill(
        &project_id,
        "archived-1",
        ProjectSkillLifecycleStatus::Staged,
        "Archived body",
        serde_json::json!({"outcome_id": "outcome-1"}),
    );
    archived.archived = true;
    repo.seed_for_test(archived).await.unwrap();
    repo.seed_for_test(skill(
        &project_id,
        "rejected-1",
        ProjectSkillLifecycleStatus::Rejected,
        "Rejected body",
        serde_json::json!({"outcome_id": "outcome-1"}),
    ))
    .await
    .unwrap();

    let created = service
        .resolve(upsert_command(
            skill(
                &project_id,
                "candidate",
                ProjectSkillLifecycleStatus::Staged,
                "New body",
                serde_json::json!({"outcome_id": "outcome-1"}),
            ),
            ProjectSkillMatchedMutation::PatchExisting,
        ))
        .await
        .unwrap();
    assert_eq!(created.outcome, ProjectSkillResolutionOutcome::CreateNew);
    assert_eq!(created.skill.id.as_str(), "candidate");
}
