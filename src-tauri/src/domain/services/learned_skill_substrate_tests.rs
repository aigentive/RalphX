use std::str::FromStr;
use std::sync::Arc;

use chrono::{Duration, Utc};
use serde_json::json;

use crate::domain::entities::types::ProjectId;
use crate::domain::entities::{
    MemoryBucket, MemoryEntry, MemoryEntryId, ProjectSkill, ProjectSkillId,
    ProjectSkillLifecycleStatus, SkillUsageEventId, SkillUsageInjectionKind, TaskOutcomeId,
    TaskOutcomeSource, TaskOutcomeStatus,
};
use crate::domain::repositories::{
    MemoryEntryRepository, ProjectSkillListOptions, ProjectSkillRepository,
    SkillUsageEventRepository, SkillUsageListOptions, TaskOutcomeRepository,
    UpsertTaskOutcomeInput,
};
use crate::domain::services::learned_skill_substrate::{
    new_c2_skill_usage_event, new_empty_task_outcome, new_skill_usage_event,
    MemoryToProjectSkillPromotionService, OutcomeLedgerService, ProjectSkillAgingStatus,
    ProjectSkillEvidenceLevel, ProjectSkillImportApplyInput, ProjectSkillImportCandidate,
    ProjectSkillImportDecision, ProjectSkillImportPreviewInput, ProjectSkillImportPreviewService,
    ProjectSkillReportOptions, ProjectSkillReportService, ProjectSkillService,
    PromoteMemoryToProjectSkillInput, SkillUsageAttribution, SkillUsageService,
    UpdateProjectSkillContentInput,
};
use crate::testing::{
    InMemoryMemoryEntryRepository, MemoryProjectSkillRepository, MemorySkillUsageEventRepository,
    MemoryTaskOutcomeRepository,
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
        content_hash: String::new(),
        evidence_hash: String::new(),
        created_by: crate::domain::entities::ProjectSkillCreatedBy::User,
        pipeline_role: None,
        created_at: now,
        updated_at: now,
    }
}

#[test]
fn c2_usage_policy_builds_deterministic_scoring_and_linkage_metadata() {
    let project_id = ProjectId::from_string("project-1".to_string());
    let skill_id = ProjectSkillId::from_string("skill-1");
    let exact = SkillUsageAttribution::ExactRun {
        conversation_id: "conversation-1".to_string(),
        agent_run_id: "run-1".to_string(),
        provider_harness: "codex".to_string(),
        stage: Some("execution".to_string()),
        bucket: Some("execution".to_string()),
    };
    let first = new_c2_skill_usage_event(
        project_id.clone(),
        skill_id.clone(),
        SkillUsageInjectionKind::CompactIndex,
        exact.clone(),
    )
    .unwrap();
    let retry = new_c2_skill_usage_event(
        project_id.clone(),
        skill_id.clone(),
        SkillUsageInjectionKind::CompactIndex,
        exact,
    )
    .unwrap();
    let composer = new_c2_skill_usage_event(
        project_id.clone(),
        skill_id.clone(),
        SkillUsageInjectionKind::ComposerDirective,
        SkillUsageAttribution::ExactRun {
            conversation_id: "conversation-1".to_string(),
            agent_run_id: "run-1".to_string(),
            provider_harness: "codex".to_string(),
            stage: Some("execution".to_string()),
            bucket: Some("execution".to_string()),
        },
    )
    .unwrap();

    assert_eq!(first.id, retry.id);
    assert_ne!(first.id, composer.id);
    assert_eq!(first.agent_run_id.as_deref(), Some("run-1"));
    assert_eq!(first.metadata_json["scoring_eligible"], true);
    assert_eq!(first.metadata_json["outcome_linkage_eligible"], true);
    assert_eq!(first.metadata_json["outcome_linkage_policy"], "exact_run");
    assert!(first.outcome_id.is_none());

    let bounded = new_c2_skill_usage_event(
        project_id.clone(),
        skill_id.clone(),
        SkillUsageInjectionKind::FullLoad,
        SkillUsageAttribution::BoundedConversation {
            conversation_id: "conversation-1".to_string(),
            reason: "agent_run_unavailable".to_string(),
            stage: Some("execution".to_string()),
            bucket: Some("execution".to_string()),
        },
    )
    .unwrap();
    assert!(bounded.agent_run_id.is_none());
    assert_eq!(
        bounded.metadata_json["outcome_linkage_policy"],
        "bounded_conversation"
    );
    assert_eq!(
        bounded.metadata_json["attribution_reason"],
        "agent_run_unavailable"
    );

    let stdin = new_c2_skill_usage_event(
        project_id,
        skill_id,
        SkillUsageInjectionKind::InteractiveStdinUnattributed,
        SkillUsageAttribution::InteractiveStdin {
            conversation_id: "conversation-1".to_string(),
            source_turn_id: "message-1".to_string(),
            stage: Some("execution".to_string()),
            bucket: Some("execution".to_string()),
        },
    )
    .unwrap();
    assert!(stdin.agent_run_id.is_none());
    assert_eq!(stdin.metadata_json["scoring_eligible"], false);
    assert_eq!(stdin.metadata_json["outcome_linkage_eligible"], false);
    assert_eq!(
        stdin.metadata_json["exclusion_reason"],
        "interactive_stdin_has_no_exact_agent_run"
    );
}

#[test]
fn c2_usage_policy_rejects_mismatched_or_empty_attribution() {
    let project_id = ProjectId::from_string("project-1".to_string());
    let skill_id = ProjectSkillId::from_string("skill-1");
    assert!(new_c2_skill_usage_event(
        project_id.clone(),
        skill_id.clone(),
        SkillUsageInjectionKind::CompactIndex,
        SkillUsageAttribution::BoundedConversation {
            conversation_id: "conversation-1".to_string(),
            reason: "missing".to_string(),
            stage: None,
            bucket: None,
        },
    )
    .is_err());
    assert!(new_c2_skill_usage_event(
        project_id,
        skill_id,
        SkillUsageInjectionKind::FullLoad,
        SkillUsageAttribution::ExactRun {
            conversation_id: String::new(),
            agent_run_id: "run-1".to_string(),
            provider_harness: "claude".to_string(),
            stage: None,
            bucket: None,
        },
    )
    .is_err());
}

fn import_candidate() -> ProjectSkillImportCandidate {
    ProjectSkillImportCandidate {
        external_id: Some("manifest-skill-1".to_string()),
        title: "Keep learned skills repository-backed".to_string(),
        bucket: "execution".to_string(),
        stage: "execution".to_string(),
        scope_paths: vec!["src-tauri/src/domain/services".to_string()],
        compact_guidance: "Read approved learned skills from the project skill service."
            .to_string(),
        body_markdown: "Detailed guidance".to_string(),
        predicted_effect: "Avoids adapter-only injection.".to_string(),
        provenance_json: json!({
            "source": "external_manifest",
            "source_ref": "manifest-skill-1"
        }),
        source_snapshot_json: json!({
            "kind": "project_skill_manifest",
            "captured_at": "2026-06-15T00:00:00Z",
            "sha256": "test-snapshot"
        }),
    }
}

#[test]
fn learned_skill_entity_ids_and_statuses_round_trip() {
    let task_outcome_id = TaskOutcomeId::from_string("outcome-1");
    let project_skill_id = ProjectSkillId::from_string("skill-1");
    let usage_event_id = SkillUsageEventId::from_string("usage-1");

    assert_eq!(task_outcome_id.as_str(), "outcome-1");
    assert_eq!(project_skill_id.as_str(), "skill-1");
    assert_eq!(usage_event_id.as_str(), "usage-1");
    assert!(!TaskOutcomeId::new().as_str().is_empty());
    assert!(!ProjectSkillId::new().as_str().is_empty());
    assert!(!SkillUsageEventId::new().as_str().is_empty());

    for (value, expected) in [
        ("unknown", TaskOutcomeStatus::Unknown),
        ("eligible", TaskOutcomeStatus::Eligible),
        ("ineligible", TaskOutcomeStatus::Ineligible),
        ("succeeded", TaskOutcomeStatus::Succeeded),
        ("failed", TaskOutcomeStatus::Failed),
    ] {
        assert_eq!(TaskOutcomeStatus::from_str(value).unwrap(), expected);
        assert_eq!(expected.to_string(), value);
    }
    assert!(TaskOutcomeStatus::from_str("pending").is_err());

    for (value, expected) in [
        ("staged", ProjectSkillLifecycleStatus::Staged),
        ("approved", ProjectSkillLifecycleStatus::Approved),
        ("rejected", ProjectSkillLifecycleStatus::Rejected),
        ("archived", ProjectSkillLifecycleStatus::Archived),
        ("retired", ProjectSkillLifecycleStatus::Retired),
    ] {
        assert_eq!(
            ProjectSkillLifecycleStatus::from_str(value).unwrap(),
            expected
        );
        assert_eq!(expected.to_string(), value);
    }
    assert!(ProjectSkillLifecycleStatus::from_str("draft").is_err());
}

fn promotion_input(
    project_id: ProjectId,
    memory_id: MemoryEntryId,
) -> PromoteMemoryToProjectSkillInput {
    PromoteMemoryToProjectSkillInput {
            project_id,
            memory_id,
            title: Some("Run branch checks before export".to_string()),
            bucket: "review".to_string(),
            stage: "review".to_string(),
            compact_guidance: "Before exporting learned skills, verify the checkout is on a clean review branch.".to_string(),
            body_markdown: "## Procedure\n\nCheck branch protection, worktree status, and preview output before export.".to_string(),
            predicted_effect: "Prevents unsafe direct writes during skill export.".to_string(),
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
async fn project_skill_service_rejects_unknown_bucket_and_stage() {
    let service = ProjectSkillService::new(Arc::new(MemoryProjectSkillRepository::new()));
    let project_id = ProjectId::from_string("project-1".to_string());
    let mut skill = staged_skill(project_id);
    skill.bucket = "reviewer".to_string();

    let error = service
        .stage_skill(skill)
        .await
        .expect_err("unknown bucket/stage should be rejected");

    assert!(error
        .to_string()
        .contains("project skill bucket must be one of"));
}

#[tokio::test]
async fn project_skill_service_rejects_unknown_stage() {
    let service = ProjectSkillService::new(Arc::new(MemoryProjectSkillRepository::new()));
    let project_id = ProjectId::from_string("project-1".to_string());
    let mut skill = staged_skill(project_id);
    skill.stage = "unsupported_stage".to_string();

    let error = service
        .stage_skill(skill)
        .await
        .expect_err("unknown stage should be rejected");

    assert!(error
        .to_string()
        .contains("project skill stage must be one of"));
}

#[tokio::test]
async fn project_skill_service_rejects_unknown_category_on_update() {
    let repo = Arc::new(MemoryProjectSkillRepository::new());
    let service = ProjectSkillService::new(repo.clone());
    let project_id = ProjectId::from_string("project-1".to_string());
    let skill = repo.create(staged_skill(project_id)).await.unwrap();

    let error = service
        .update_skill_content(UpdateProjectSkillContentInput {
            project_id: skill.project_id.clone(),
            project_skill_id: skill.id,
            title: "Updated skill".to_string(),
            bucket: "execution".to_string(),
            stage: "unsupported_stage".to_string(),
            scope_paths: Vec::new(),
            compact_guidance: "Updated compact guidance.".to_string(),
            body_markdown: "Updated body markdown.".to_string(),
            predicted_effect: "Keeps category validation centralized.".to_string(),
            source_sync_enabled: None,
        })
        .await
        .expect_err("unknown stage should be rejected during update");

    assert!(error
        .to_string()
        .contains("project skill stage must be one of"));
}

#[tokio::test]
async fn project_skill_service_lifecycle_and_usage_services_work_together() {
    let skill_repo: Arc<dyn ProjectSkillRepository> = Arc::new(MemoryProjectSkillRepository::new());
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

    let pinned = skill_service
        .pin_skill(&approved.id)
        .await
        .unwrap()
        .unwrap();
    assert!(pinned.pinned);

    let unpinned = skill_service
        .unpin_skill(&approved.id)
        .await
        .unwrap()
        .unwrap();
    assert!(!unpinned.pinned);

    usage_service
        .record_usage(new_skill_usage_event(
            project_id.clone(),
            approved.id,
            SkillUsageInjectionKind::CompactIndex,
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
async fn outcome_ledger_rejects_compatibility_source_without_writing() {
    let repo = Arc::new(MemoryTaskOutcomeRepository::new());
    let service = OutcomeLedgerService::new(repo.clone());
    let project_id = ProjectId::from_string("project-compat".to_string());

    let error = service
        .record_outcome(new_empty_task_outcome(
            project_id.clone(),
            TaskOutcomeSource::TaskPipeline,
            "task",
            "task-1",
        ))
        .await
        .expect_err("compatibility-only source must not be emitted");

    assert!(error.to_string().contains("read-only compatibility"));
    assert!(repo
        .list_by_project(&project_id, Default::default())
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn project_skill_import_preview_marks_valid_rows_eligible_without_writing() {
    let repo = Arc::new(MemoryProjectSkillRepository::new());
    let service = ProjectSkillImportPreviewService::new(repo.clone());
    let project_id = ProjectId::from_string("project-import".to_string());

    let preview = service
        .preview_import(ProjectSkillImportPreviewInput {
            project_id: project_id.clone(),
            candidates: vec![import_candidate()],
        })
        .await
        .unwrap();

    assert_eq!(preview.eligible_count, 1);
    assert_eq!(preview.invalid_count, 0);
    assert_eq!(preview.duplicate_count, 0);
    assert_eq!(
        preview.rows[0].decision,
        ProjectSkillImportDecision::Eligible
    );
    let written = repo
        .list_by_project(&project_id, ProjectSkillListOptions::default())
        .await
        .unwrap();
    assert!(written.is_empty(), "preview must not write imported skills");
}

#[tokio::test]
async fn project_skill_import_preview_fails_closed_for_invalid_manifest_and_paths() {
    let repo = Arc::new(MemoryProjectSkillRepository::new());
    let service = ProjectSkillImportPreviewService::new(repo);
    let project_id = ProjectId::from_string("project-import".to_string());
    let mut candidate = import_candidate();
    candidate.bucket = "reviewer".to_string();
    candidate.stage = "unsupported_stage".to_string();
    candidate.predicted_effect = " ".to_string();
    candidate.provenance_json = json!({});
    candidate.source_snapshot_json = json!(null);
    candidate.scope_paths = vec![
        "../outside".to_string(),
        "/absolute/path".to_string(),
        "src//bad".to_string(),
    ];

    let preview = service
        .preview_import(ProjectSkillImportPreviewInput {
            project_id,
            candidates: vec![candidate],
        })
        .await
        .unwrap();

    assert_eq!(preview.eligible_count, 0);
    assert_eq!(preview.invalid_count, 1);
    assert_eq!(
        preview.rows[0].decision,
        ProjectSkillImportDecision::Invalid
    );
    assert!(preview.rows[0]
        .reasons
        .iter()
        .any(|reason| reason == "predicted_effect is required"));
    assert!(preview.rows[0]
        .reasons
        .iter()
        .any(|reason| reason.starts_with("bucket must be one of")));
    assert!(preview.rows[0]
        .reasons
        .iter()
        .any(|reason| reason.starts_with("stage must be one of")));
    assert!(preview.rows[0]
        .reasons
        .iter()
        .any(|reason| reason == "provenance is required"));
    assert!(preview.rows[0]
        .reasons
        .iter()
        .any(|reason| reason == "source snapshot is required before import"));
    assert!(preview.rows[0]
        .reasons
        .iter()
        .any(|reason| reason.starts_with("invalid scope path: ../outside")));
}

#[tokio::test]
async fn project_skill_import_preview_detects_existing_duplicates() {
    let repo = Arc::new(MemoryProjectSkillRepository::new());
    let project_id = ProjectId::from_string("project-import".to_string());
    let existing = repo.create(staged_skill(project_id.clone())).await.unwrap();
    let service = ProjectSkillImportPreviewService::new(repo);

    let preview = service
        .preview_import(ProjectSkillImportPreviewInput {
            project_id,
            candidates: vec![import_candidate()],
        })
        .await
        .unwrap();

    assert_eq!(preview.eligible_count, 0);
    assert_eq!(preview.duplicate_count, 1);
    assert_eq!(
        preview.rows[0].decision,
        ProjectSkillImportDecision::Duplicate
    );
    assert_eq!(
        preview.rows[0].duplicate_project_skill_id,
        Some(existing.id)
    );
    assert!(preview.rows[0]
        .reasons
        .iter()
        .any(|reason| reason == "matching project skill already exists"));
}

#[tokio::test]
async fn project_skill_import_preview_dedupes_stable_source_id_before_title() {
    let repo = Arc::new(MemoryProjectSkillRepository::new());
    let project_id = ProjectId::from_string("project-import".to_string());
    let mut existing = staged_skill(project_id.clone());
    existing.title = "Old imported title".to_string();
    existing.provenance_json = json!({
        "source": "project_skill_import",
        "external_id": ".claude/skills/review/SKILL.md",
        "source_snapshot": {
            "relative_path": ".claude/skills/review/SKILL.md",
            "source_sync_enabled": true
        }
    });
    let existing = repo.create(existing).await.unwrap();
    let service = ProjectSkillImportPreviewService::new(repo);
    let mut candidate = import_candidate();
    candidate.external_id = Some(".claude/skills/review/SKILL.md".to_string());
    candidate.title = "New imported title".to_string();

    let preview = service
        .preview_import(ProjectSkillImportPreviewInput {
            project_id,
            candidates: vec![candidate],
        })
        .await
        .unwrap();

    assert_eq!(preview.eligible_count, 0);
    assert_eq!(preview.duplicate_count, 1);
    assert_eq!(
        preview.rows[0].decision,
        ProjectSkillImportDecision::Duplicate
    );
    assert_eq!(
        preview.rows[0].duplicate_project_skill_id,
        Some(existing.id)
    );
}

#[tokio::test]
async fn project_skill_import_apply_requires_confirmation() {
    let repo = Arc::new(MemoryProjectSkillRepository::new());
    let service = ProjectSkillImportPreviewService::new(repo);
    let project_id = ProjectId::from_string("project-import".to_string());

    let result = service
        .apply_import(ProjectSkillImportApplyInput {
            project_id,
            candidates: vec![import_candidate()],
            confirm_import: false,
        })
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn project_skill_import_apply_stages_only_eligible_rows_with_snapshot_provenance() {
    let repo = Arc::new(MemoryProjectSkillRepository::new());
    let service = ProjectSkillImportPreviewService::new(repo.clone());
    let project_id = ProjectId::from_string("project-import".to_string());
    let mut invalid = import_candidate();
    invalid.external_id = Some("invalid-skill".to_string());
    invalid.title = "Invalid imported skill".to_string();
    invalid.scope_paths = vec!["../outside".to_string()];

    let result = service
        .apply_import(ProjectSkillImportApplyInput {
            project_id: project_id.clone(),
            candidates: vec![import_candidate(), invalid],
            confirm_import: true,
        })
        .await
        .unwrap();

    assert_eq!(result.preview.eligible_count, 1);
    assert_eq!(result.preview.invalid_count, 1);
    assert_eq!(result.imported_skills.len(), 1);
    let imported = &result.imported_skills[0];
    assert_eq!(imported.status, ProjectSkillLifecycleStatus::Staged);
    assert_eq!(
        imported.created_by,
        crate::domain::entities::ProjectSkillCreatedBy::Imported
    );
    assert_eq!(
        imported
            .provenance_json
            .get("source")
            .and_then(serde_json::Value::as_str),
        Some("project_skill_import")
    );
    assert!(imported.provenance_json.get("source_snapshot").is_some());

    let written = repo
        .list_by_project(&project_id, ProjectSkillListOptions::default())
        .await
        .unwrap();
    assert_eq!(written.len(), 1);
}

#[tokio::test]
async fn memory_to_project_skill_promotion_requires_procedural_content() {
    let memory_repo = Arc::new(InMemoryMemoryEntryRepository::new());
    let skill_repo: Arc<dyn ProjectSkillRepository> = Arc::new(MemoryProjectSkillRepository::new());
    let project_id = ProjectId::from_string("project-memory".to_string());
    let memory = MemoryEntry::new(
        project_id.clone(),
        MemoryBucket::OperationalPlaybooks,
        "Export branch checks".to_string(),
        "Check the export branch before writing files.".to_string(),
        "Memory facts about the branch checks.".to_string(),
        vec!["src-tauri".to_string()],
        "hash-1".to_string(),
    );
    let memory = memory_repo.create(memory).await.unwrap();
    let service = MemoryToProjectSkillPromotionService::new(memory_repo, skill_repo);
    let mut input = promotion_input(project_id, memory.id);
    input.compact_guidance = "Check the export branch before writing files.".to_string();

    let result = service.promote_memory(input).await;

    assert!(result.is_err());
}

#[tokio::test]
async fn memory_to_project_skill_promotion_rejects_unknown_category() {
    let memory_repo = Arc::new(InMemoryMemoryEntryRepository::new());
    let skill_repo: Arc<dyn ProjectSkillRepository> = Arc::new(MemoryProjectSkillRepository::new());
    let project_id = ProjectId::from_string("project-memory".to_string());
    let memory = MemoryEntry::new(
        project_id.clone(),
        MemoryBucket::OperationalPlaybooks,
        "Export branch checks".to_string(),
        "Check the export branch before writing files.".to_string(),
        "Memory facts about the branch checks.".to_string(),
        vec!["src-tauri".to_string()],
        "hash-1".to_string(),
    );
    let memory = memory_repo.create(memory).await.unwrap();
    let service = MemoryToProjectSkillPromotionService::new(memory_repo, skill_repo);
    let mut input = promotion_input(project_id, memory.id);
    input.stage = "unsupported_stage".to_string();

    let error = service
        .promote_memory(input)
        .await
        .expect_err("unknown stage should be rejected");

    assert!(error
        .to_string()
        .contains("project skill stage must be one of"));
}

#[tokio::test]
async fn memory_to_project_skill_promotion_stages_skill_with_memory_provenance() {
    let memory_repo = Arc::new(InMemoryMemoryEntryRepository::new());
    let skill_repo: Arc<dyn ProjectSkillRepository> = Arc::new(MemoryProjectSkillRepository::new());
    let project_id = ProjectId::from_string("project-memory".to_string());
    let mut memory = MemoryEntry::new(
        project_id.clone(),
        MemoryBucket::OperationalPlaybooks,
        "Export branch checks".to_string(),
        "Check the export branch before writing files.".to_string(),
        "Memory facts about the branch checks.".to_string(),
        vec!["src-tauri".to_string()],
        "hash-1".to_string(),
    );
    memory.source_context_type = Some("agent_run".to_string());
    memory.source_context_id = Some("run-1".to_string());
    let memory = memory_repo.create(memory).await.unwrap();
    let service = MemoryToProjectSkillPromotionService::new(memory_repo, Arc::clone(&skill_repo));

    let result = service
        .promote_memory(promotion_input(project_id.clone(), memory.id.clone()))
        .await
        .unwrap();

    assert_eq!(result.skill.status, ProjectSkillLifecycleStatus::Staged);
    assert_eq!(
        result.skill.created_by,
        crate::domain::entities::ProjectSkillCreatedBy::User
    );
    assert_eq!(result.skill.scope_paths, vec!["src-tauri".to_string()]);
    assert_eq!(
        result
            .skill
            .provenance_json
            .get("source")
            .and_then(serde_json::Value::as_str),
        Some("memory_to_project_skill_promotion")
    );
    assert_eq!(
        result
            .skill
            .provenance_json
            .get("memory_id")
            .and_then(serde_json::Value::as_str),
        Some(memory.id.as_str())
    );

    let written = skill_repo
        .list_by_project(&project_id, ProjectSkillListOptions::default())
        .await
        .unwrap();
    assert_eq!(written.len(), 1);
}

#[tokio::test]
async fn project_skill_report_cards_are_descriptive_until_min_n_is_met() {
    let skill_repo = Arc::new(MemoryProjectSkillRepository::new());
    let usage_repo = Arc::new(MemorySkillUsageEventRepository::new());
    let outcome_repo = Arc::new(MemoryTaskOutcomeRepository::new());
    let project_id = ProjectId::from_string("project-1".to_string());
    let now = Utc::now();
    let mut skill = staged_skill(project_id.clone());
    skill.status = ProjectSkillLifecycleStatus::Approved;
    skill.created_at = now - Duration::days(10);
    let skill = skill_repo.create(skill).await.unwrap();

    let mut success = new_empty_task_outcome(
        project_id.clone(),
        TaskOutcomeSource::Review,
        "review_note",
        "review-1",
    );
    success.status = TaskOutcomeStatus::Succeeded;
    outcome_repo
        .upsert(UpsertTaskOutcomeInput {
            outcome: success.clone(),
        })
        .await
        .unwrap();
    let mut failure = new_empty_task_outcome(
        project_id.clone(),
        TaskOutcomeSource::Review,
        "review_note",
        "review-2",
    );
    failure.status = TaskOutcomeStatus::Failed;
    outcome_repo
        .upsert(UpsertTaskOutcomeInput {
            outcome: failure.clone(),
        })
        .await
        .unwrap();

    for (index, outcome_id) in [Some(success.id.clone()), Some(failure.id.clone()), None]
        .into_iter()
        .enumerate()
    {
        let mut event = new_skill_usage_event(
            project_id.clone(),
            skill.id.clone(),
            SkillUsageInjectionKind::CompactIndex,
        );
        event.outcome_id = outcome_id;
        event.created_at = now - Duration::days(index as i64);
        usage_repo.record(event).await.unwrap();
    }

    let service = ProjectSkillReportService::new(skill_repo, usage_repo, outcome_repo);
    let cards = service
        .list_report_cards(
            &project_id,
            ProjectSkillReportOptions {
                min_linked_outcomes: 3,
                stale_after_days: 30,
                now,
            },
        )
        .await
        .unwrap();

    assert_eq!(cards.len(), 1);
    let card = &cards[0];
    assert_eq!(card.usage_count, 3);
    assert_eq!(card.linked_outcome_count, 2);
    assert_eq!(card.succeeded_outcome_count, 1);
    assert_eq!(card.failed_outcome_count, 1);
    assert_eq!(
        card.evidence_level,
        ProjectSkillEvidenceLevel::InsufficientData
    );
    assert_eq!(card.aging_status, ProjectSkillAgingStatus::Active);
}

#[tokio::test]
async fn project_skill_report_cards_mark_unused_but_exempt_pinned_skills() {
    let skill_repo = Arc::new(MemoryProjectSkillRepository::new());
    let usage_repo = Arc::new(MemorySkillUsageEventRepository::new());
    let outcome_repo = Arc::new(MemoryTaskOutcomeRepository::new());
    let project_id = ProjectId::from_string("project-1".to_string());
    let now = Utc::now();

    let mut unused = staged_skill(project_id.clone());
    unused.title = "Unused approved skill".to_string();
    unused.status = ProjectSkillLifecycleStatus::Approved;
    unused.created_at = now - Duration::days(45);
    skill_repo.create(unused).await.unwrap();

    let mut pinned = staged_skill(project_id.clone());
    pinned.title = "Pinned approved skill".to_string();
    pinned.status = ProjectSkillLifecycleStatus::Approved;
    pinned.pinned = true;
    pinned.created_at = now - Duration::days(45);
    skill_repo.create(pinned).await.unwrap();

    let service = ProjectSkillReportService::new(skill_repo, usage_repo, outcome_repo);
    let cards = service
        .list_report_cards(
            &project_id,
            ProjectSkillReportOptions {
                min_linked_outcomes: 1,
                stale_after_days: 30,
                now,
            },
        )
        .await
        .unwrap();

    let unused = cards
        .iter()
        .find(|card| card.title == "Unused approved skill")
        .unwrap();
    assert_eq!(unused.aging_status, ProjectSkillAgingStatus::Unused);
    let pinned = cards
        .iter()
        .find(|card| card.title == "Pinned approved skill")
        .unwrap();
    assert_eq!(pinned.aging_status, ProjectSkillAgingStatus::Active);
}

#[tokio::test]
async fn project_skill_service_rejects_pinning_unapproved_skills() {
    let service = ProjectSkillService::new(Arc::new(MemoryProjectSkillRepository::new()));
    let project_id = ProjectId::from_string("project-1".to_string());
    let staged = service.stage_skill(staged_skill(project_id)).await.unwrap();

    let result = service.pin_skill(&staged.id).await;

    assert!(matches!(result, Err(crate::error::AppError::Validation(_))));
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
