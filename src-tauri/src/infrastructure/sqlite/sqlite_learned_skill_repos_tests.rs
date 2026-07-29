use std::sync::Arc;

use chrono::{Duration, Utc};
use rusqlite::Connection;
use serde_json::json;
use tokio::sync::Mutex;

use super::{
    SqliteProjectSkillRepository, SqliteSkillUsageEventRepository, SqliteTaskOutcomeRepository,
};
use crate::domain::entities::types::ProjectId;
use crate::domain::entities::{
    ProjectSkill, ProjectSkillCreatedBy, ProjectSkillId, ProjectSkillLifecycleStatus,
    ProjectSkillVersion, SkillUsageEvent, SkillUsageEventId, SkillUsageInjectionKind, TaskOutcome,
    TaskOutcomeClass, TaskOutcomeId, TaskOutcomeSource, TaskOutcomeStatus,
};
use crate::domain::repositories::{
    ProjectSkillListOptions, ProjectSkillMatchedMutation, ProjectSkillRepository,
    ProjectSkillResolutionCommand, ProjectSkillResolutionIdentity,
    ProjectSkillResolutionIdentityKind, ProjectSkillResolutionIntent,
    ProjectSkillResolutionOutcome, ProjectSkillStagingPolicy, SkillUsageEventRepository,
    SkillUsageListOptions, TaskOutcomeListOptions, TaskOutcomeRepository, UpsertTaskOutcomeInput,
};
use crate::domain::services::learned_skill_substrate::{
    new_c2_skill_usage_event, SkillUsageAttribution,
};
use crate::domain::services::project_skill_resolution::import_title_resolution_identity;
use crate::infrastructure::sqlite::run_migrations;

fn shared_test_connection() -> Arc<Mutex<Connection>> {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON").unwrap();
    run_migrations(&conn).unwrap();
    conn.execute(
        "INSERT INTO projects (id, name, working_directory)
             VALUES ('project-1', 'Project 1', '/tmp/project-1')",
        [],
    )
    .unwrap();
    Arc::new(Mutex::new(conn))
}

fn task_outcome(status: TaskOutcomeStatus, outcome_class: Option<&str>) -> TaskOutcome {
    let now = Utc::now();
    TaskOutcome {
        id: TaskOutcomeId::new(),
        project_id: ProjectId::from_string("project-1".to_string()),
        source: TaskOutcomeSource::TaskPipeline,
        source_ref_kind: "task".to_string(),
        source_ref_id: "task-1".to_string(),
        task_id: Some("task-1".to_string()),
        conversation_id: None,
        agent_run_id: None,
        pull_request_id: None,
        proposal_id: None,
        verification_id: None,
        review_id: None,
        outcome_class: outcome_class.map(TaskOutcomeClass::from),
        status,
        evidence_json: json!({ "summary": "evidence" }),
        failure_fingerprint: None,
        provider_harness: Some("codex".to_string()),
        provider_session_id: Some("session-1".to_string()),
        created_at: now,
        updated_at: now,
    }
}

fn project_skill(
    title: &str,
    bucket: &str,
    stage: &str,
    status: ProjectSkillLifecycleStatus,
    scope_paths: Vec<String>,
) -> ProjectSkill {
    let now = Utc::now();
    ProjectSkill {
        id: ProjectSkillId::new(),
        project_id: ProjectId::from_string("project-1".to_string()),
        title: title.to_string(),
        bucket: bucket.to_string(),
        stage: stage.to_string(),
        status,
        pinned: false,
        archived: false,
        scope_paths,
        compact_guidance: format!("Use {title} when it matches the project context."),
        body_markdown: format!("Detailed guidance for {title}."),
        predicted_effect: Some(format!("{title} reduces repeated work.")),
        provenance_json: json!({ "source": "sqlite-test" }),
        companion_of_skill_id: None,
        content_hash: String::new(),
        evidence_hash: String::new(),
        created_by: crate::domain::entities::ProjectSkillCreatedBy::User,
        pipeline_role: None,
        created_at: now,
        updated_at: now,
    }
}

fn outcome_resolution_command(
    mut skill: ProjectSkill,
    outcome_id: &str,
) -> ProjectSkillResolutionCommand {
    skill.provenance_json = json!({
        "source": "task_outcome",
        "outcome_id": outcome_id,
    });
    ProjectSkillResolutionCommand {
        candidate: skill,
        intent: ProjectSkillResolutionIntent::Upsert {
            identities: vec![ProjectSkillResolutionIdentity {
                kind: ProjectSkillResolutionIdentityKind::Outcome,
                value: outcome_id.to_string(),
            }],
            matched_mutation: ProjectSkillMatchedMutation::PatchExisting,
        },
        evidence_markdown: None,
        staging_policy: None,
    }
}

fn pipeline_resolution_command(
    mut skill: ProjectSkill,
    _identity: &str,
    role: &str,
) -> ProjectSkillResolutionCommand {
    skill.created_by = ProjectSkillCreatedBy::Agent;
    skill.pipeline_role = Some(role.to_string());
    skill.provenance_json = json!({
        "source": "skill_pipeline_mcp",
        "additional": {"pipeline_role": role},
    });
    let resolution_identity =
        import_title_resolution_identity(&skill.title, &skill.bucket, &skill.stage);
    ProjectSkillResolutionCommand {
        candidate: skill,
        intent: ProjectSkillResolutionIntent::Upsert {
            identities: vec![resolution_identity],
            matched_mutation: ProjectSkillMatchedMutation::PatchExisting,
        },
        evidence_markdown: None,
        staging_policy: Some(ProjectSkillStagingPolicy {
            pipeline_role: role.to_string(),
            max_staged: 2,
            window_start: Utc::now() - Duration::hours(24),
        }),
    }
}

#[tokio::test]
async fn concurrent_sqlite_resolution_converges_to_one_skill_and_snapshot() {
    let conn = shared_test_connection();
    let first_repo = SqliteProjectSkillRepository::from_shared(Arc::clone(&conn));
    let second_repo = SqliteProjectSkillRepository::from_shared(conn);
    let first = outcome_resolution_command(
        project_skill(
            "Concurrent skill",
            "execution",
            "execution",
            ProjectSkillLifecycleStatus::Staged,
            Vec::new(),
        ),
        "outcome-concurrent",
    );
    let mut second = first.clone();
    second.candidate.id = ProjectSkillId::new();

    let (first, second) = tokio::join!(first_repo.resolve(first), second_repo.resolve(second));
    let first = first.unwrap();
    let second = second.unwrap();
    assert_eq!(
        [first.outcome, second.outcome]
            .into_iter()
            .filter(|outcome| *outcome == ProjectSkillResolutionOutcome::CreateNew)
            .count(),
        1
    );
    assert_eq!(
        [first.outcome, second.outcome]
            .into_iter()
            .filter(|outcome| *outcome == ProjectSkillResolutionOutcome::Duplicate)
            .count(),
        1
    );
    let created = if first.outcome == ProjectSkillResolutionOutcome::CreateNew {
        first
    } else {
        second
    };
    assert_eq!(
        first_repo
            .list_by_project(
                &ProjectId::from_string("project-1".to_string()),
                ProjectSkillListOptions::default(),
            )
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        first_repo
            .list_versions(&created.skill.id)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn sqlite_pipeline_cap_runs_after_duplicate_and_isolated_by_role_and_bucket() {
    let repo = SqliteProjectSkillRepository::from_shared(shared_test_connection());
    let first = pipeline_resolution_command(
        project_skill(
            "Pipeline one",
            "execution",
            "execution",
            ProjectSkillLifecycleStatus::Staged,
            Vec::new(),
        ),
        "pipeline-1",
        "memory_capture",
    );
    let second = pipeline_resolution_command(
        project_skill(
            "Pipeline two",
            "execution",
            "execution",
            ProjectSkillLifecycleStatus::Staged,
            Vec::new(),
        ),
        "pipeline-2",
        "memory_capture",
    );
    repo.resolve(first.clone()).await.expect("first create");
    repo.resolve(second).await.expect("second create");

    let duplicate = repo.resolve(first).await.expect("duplicate at cap");
    assert_eq!(duplicate.outcome, ProjectSkillResolutionOutcome::Duplicate);
    assert!(duplicate.version.is_none());

    let third = pipeline_resolution_command(
        project_skill(
            "Pipeline three",
            "execution",
            "execution",
            ProjectSkillLifecycleStatus::Staged,
            Vec::new(),
        ),
        "pipeline-3",
        "memory_capture",
    );
    assert!(matches!(
        repo.resolve(third).await,
        Err(crate::error::AppError::Conflict(_))
    ));

    let other_role = pipeline_resolution_command(
        project_skill(
            "Maintainer role",
            "execution",
            "execution",
            ProjectSkillLifecycleStatus::Staged,
            Vec::new(),
        ),
        "pipeline-maintainer",
        "memory_maintainer",
    );
    assert_eq!(
        repo.resolve(other_role).await.expect("other role").outcome,
        ProjectSkillResolutionOutcome::CreateNew
    );

    let other_bucket = pipeline_resolution_command(
        project_skill(
            "Planning bucket",
            "planning",
            "planning",
            ProjectSkillLifecycleStatus::Staged,
            Vec::new(),
        ),
        "pipeline-planning",
        "memory_capture",
    );
    assert_eq!(
        repo.resolve(other_bucket)
            .await
            .expect("other bucket")
            .outcome,
        ProjectSkillResolutionOutcome::CreateNew
    );
}

#[tokio::test]
async fn sqlite_pipeline_cap_counts_recent_null_role_rows_for_every_role() {
    let repo = SqliteProjectSkillRepository::from_shared(shared_test_connection());
    repo.seed_for_test(project_skill(
        "Legacy NULL role",
        "review",
        "review",
        ProjectSkillLifecycleStatus::Staged,
        Vec::new(),
    ))
    .await
    .expect("seed null role");

    let first = pipeline_resolution_command(
        project_skill(
            "Role-specific row",
            "review",
            "review",
            ProjectSkillLifecycleStatus::Staged,
            Vec::new(),
        ),
        "pipeline-review-1",
        "memory_capture",
    );
    repo.resolve(first).await.expect("one role row after null");

    let blocked = pipeline_resolution_command(
        project_skill(
            "Blocked by NULL",
            "review",
            "review",
            ProjectSkillLifecycleStatus::Staged,
            Vec::new(),
        ),
        "pipeline-review-2",
        "memory_capture",
    );
    assert!(matches!(
        repo.resolve(blocked).await,
        Err(crate::error::AppError::Conflict(_))
    ));
}

#[tokio::test]
async fn sqlite_resolution_rolls_back_current_row_when_snapshot_insert_fails() {
    let conn = shared_test_connection();
    conn.lock()
        .await
        .execute_batch(
            "CREATE TRIGGER fail_project_skill_version_insert
             BEFORE INSERT ON project_skill_versions
             BEGIN
                 SELECT RAISE(ABORT, 'injected version failure');
             END;",
        )
        .unwrap();
    let repo = SqliteProjectSkillRepository::from_shared(conn);
    let command = outcome_resolution_command(
        project_skill(
            "Rollback skill",
            "execution",
            "execution",
            ProjectSkillLifecycleStatus::Staged,
            Vec::new(),
        ),
        "outcome-rollback",
    );

    assert!(repo.resolve(command).await.is_err());
    assert!(repo
        .list_by_project(
            &ProjectId::from_string("project-1".to_string()),
            ProjectSkillListOptions::default(),
        )
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn task_outcome_upsert_upgrades_class_without_duplicate() {
    let repo = SqliteTaskOutcomeRepository::from_shared(shared_test_connection());

    repo.upsert(UpsertTaskOutcomeInput {
        outcome: task_outcome(TaskOutcomeStatus::Unknown, None),
    })
    .await
    .unwrap();
    let updated = repo
        .upsert(UpsertTaskOutcomeInput {
            outcome: task_outcome(TaskOutcomeStatus::Eligible, Some("merge_passed")),
        })
        .await
        .unwrap();

    assert_eq!(updated.status, TaskOutcomeStatus::Eligible);
    assert_eq!(
        updated.outcome_class.as_ref().map(TaskOutcomeClass::as_str),
        Some("merge_passed")
    );

    let rows = repo
        .list_by_project(
            &ProjectId::from_string("project-1".to_string()),
            TaskOutcomeListOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
}

#[tokio::test]
async fn task_outcomes_filter_by_source_and_status_and_get_missing() {
    let repo = SqliteTaskOutcomeRepository::from_shared(shared_test_connection());
    let mut failed = task_outcome(TaskOutcomeStatus::Failed, Some("review_failed"));
    failed.source = TaskOutcomeSource::GithubPrReview;
    failed.source_ref_kind = "pull_request".to_string();
    failed.source_ref_id = "42:review".to_string();
    let failed_id = failed.id.clone();
    repo.upsert(UpsertTaskOutcomeInput { outcome: failed })
        .await
        .unwrap();

    let mut succeeded = task_outcome(TaskOutcomeStatus::Succeeded, Some("merge_passed"));
    succeeded.source = TaskOutcomeSource::AgentWorkspacePr;
    succeeded.source_ref_kind = "pull_request".to_string();
    succeeded.source_ref_id = "42:terminal:merged".to_string();
    repo.upsert(UpsertTaskOutcomeInput { outcome: succeeded })
        .await
        .unwrap();

    let failed_rows = repo
        .list_by_project(
            &ProjectId::from_string("project-1".to_string()),
            TaskOutcomeListOptions {
                source: Some(TaskOutcomeSource::GithubPrReview),
                status: Some(TaskOutcomeStatus::Failed),
            },
        )
        .await
        .unwrap();
    assert_eq!(failed_rows.len(), 1);
    assert_eq!(failed_rows[0].id.as_str(), failed_id.as_str());

    assert!(repo
        .get_by_id(&TaskOutcomeId::from_string("missing-outcome".to_string()))
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn project_skill_lifecycle_and_usage_round_trip() {
    let conn = shared_test_connection();
    let skill_repo = SqliteProjectSkillRepository::from_shared(Arc::clone(&conn));
    let usage_repo = SqliteSkillUsageEventRepository::from_shared(conn);
    let now = Utc::now();
    let skill = ProjectSkill {
        id: ProjectSkillId::new(),
        project_id: ProjectId::from_string("project-1".to_string()),
        title: "Prefer repository-backed learned skills".to_string(),
        bucket: "execution".to_string(),
        stage: "execution".to_string(),
        status: ProjectSkillLifecycleStatus::Staged,
        pinned: false,
        archived: false,
        scope_paths: vec!["src-tauri/".to_string()],
        compact_guidance: "Use repository-backed skill records.".to_string(),
        body_markdown: "Detailed guidance".to_string(),
        predicted_effect: Some("Prevents adapter-only learned skill injection.".to_string()),
        provenance_json: json!({ "source": "test" }),
        companion_of_skill_id: None,
        content_hash: String::new(),
        evidence_hash: String::new(),
        created_by: crate::domain::entities::ProjectSkillCreatedBy::User,
        pipeline_role: None,
        created_at: now,
        updated_at: now,
    };
    let skill_id = skill.id.clone();

    skill_repo.create(skill).await.unwrap();
    let approved = skill_repo
        .update_lifecycle_status(&skill_id, ProjectSkillLifecycleStatus::Approved)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(approved.status, ProjectSkillLifecycleStatus::Approved);

    let listed = skill_repo
        .list_by_project(
            &ProjectId::from_string("project-1".to_string()),
            ProjectSkillListOptions {
                status: Some(ProjectSkillLifecycleStatus::Approved),
                scope_path: Some("src-tauri/src/lib.rs".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(listed.len(), 1);

    let event = SkillUsageEvent {
        id: SkillUsageEventId::new(),
        project_id: ProjectId::from_string("project-1".to_string()),
        project_skill_id: skill_id.clone(),
        conversation_id: Some("conversation-1".to_string()),
        agent_run_id: Some("run-1".to_string()),
        provider_harness: Some("claude".to_string()),
        stage: Some("execution".to_string()),
        bucket: Some("execution".to_string()),
        injection_kind: SkillUsageInjectionKind::CompactIndex,
        outcome_id: None,
        metadata_json: json!({ "selected": true }),
        created_at: Utc::now(),
    };
    usage_repo.record(event).await.unwrap();
    let usage = usage_repo
        .list_by_project(
            &ProjectId::from_string("project-1".to_string()),
            SkillUsageListOptions {
                project_skill_id: Some(skill_id),
                agent_run_id: Some("run-1".to_string()),
            },
        )
        .await
        .unwrap();
    assert_eq!(usage.len(), 1);
}

#[tokio::test]
async fn c2_sqlite_usage_batch_dedupes_and_rolls_back_on_failure() {
    let conn = shared_test_connection();
    let skill_repo = SqliteProjectSkillRepository::from_shared(Arc::clone(&conn));
    let usage_repo = SqliteSkillUsageEventRepository::from_shared(conn);
    let project_id = ProjectId::from_string("project-1".to_string());
    let skill = project_skill(
        "C2 Usage Skill",
        "execution",
        "execution",
        ProjectSkillLifecycleStatus::Approved,
        Vec::new(),
    );
    let skill_id = skill.id.clone();
    skill_repo.create(skill).await.unwrap();
    let event = new_c2_skill_usage_event(
        project_id.clone(),
        skill_id.clone(),
        SkillUsageInjectionKind::CompactIndex,
        SkillUsageAttribution::ExactRun {
            conversation_id: "conversation-1".to_string(),
            agent_run_id: "run-1".to_string(),
            provider_harness: "claude".to_string(),
            stage: Some("execution".to_string()),
            bucket: Some("execution".to_string()),
        },
    )
    .unwrap();

    usage_repo
        .record_batch(vec![event.clone(), event])
        .await
        .unwrap();
    assert_eq!(
        usage_repo
            .list_by_project(&project_id, SkillUsageListOptions::default())
            .await
            .unwrap()
            .len(),
        1
    );

    let valid = new_c2_skill_usage_event(
        project_id.clone(),
        skill_id,
        SkillUsageInjectionKind::ComposerDirective,
        SkillUsageAttribution::ExactRun {
            conversation_id: "conversation-1".to_string(),
            agent_run_id: "run-2".to_string(),
            provider_harness: "codex".to_string(),
            stage: Some("execution".to_string()),
            bucket: Some("execution".to_string()),
        },
    )
    .unwrap();
    let invalid = new_c2_skill_usage_event(
        project_id.clone(),
        ProjectSkillId::from_string("missing-skill"),
        SkillUsageInjectionKind::ComposerDirective,
        SkillUsageAttribution::ExactRun {
            conversation_id: "conversation-1".to_string(),
            agent_run_id: "run-3".to_string(),
            provider_harness: "codex".to_string(),
            stage: Some("execution".to_string()),
            bucket: Some("execution".to_string()),
        },
    )
    .unwrap();
    assert!(usage_repo.record_batch(vec![valid, invalid]).await.is_err());
    assert_eq!(
        usage_repo
            .list_by_project(&project_id, SkillUsageListOptions::default())
            .await
            .unwrap()
            .len(),
        1,
        "failed SQLite batch must roll back earlier inserts"
    );
}

#[tokio::test]
async fn project_skill_filters_order_archived_and_missing_updates() {
    let conn = shared_test_connection();
    let skill_repo = SqliteProjectSkillRepository::from_shared(conn);
    let project_id = ProjectId::from_string("project-1".to_string());

    let execution = project_skill(
        "Execution Pattern",
        "execution",
        "execution",
        ProjectSkillLifecycleStatus::Approved,
        vec!["src-tauri/".to_string()],
    );
    let execution_id = execution.id.clone();
    skill_repo.create(execution).await.unwrap();
    skill_repo.update_pinned(&execution_id, true).await.unwrap();

    let review = project_skill(
        "Review Pattern",
        "review",
        "review",
        ProjectSkillLifecycleStatus::Approved,
        vec!["frontend/".to_string()],
    );
    skill_repo.create(review).await.unwrap();

    let archived = project_skill(
        "Archived Pattern",
        "execution",
        "execution",
        ProjectSkillLifecycleStatus::Approved,
        Vec::new(),
    );
    let archived_id = archived.id.clone();
    skill_repo.create(archived).await.unwrap();
    let archived = skill_repo
        .update_lifecycle_status(&archived_id, ProjectSkillLifecycleStatus::Retired)
        .await
        .unwrap()
        .unwrap();
    assert!(archived.archived);

    let scoped_execution = skill_repo
        .list_by_project(
            &project_id,
            ProjectSkillListOptions {
                status: Some(ProjectSkillLifecycleStatus::Approved),
                bucket: Some("execution".to_string()),
                stage: Some("execution".to_string()),
                scope_path: Some("src-tauri/src/lib.rs".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(scoped_execution.len(), 1);
    assert_eq!(scoped_execution[0].id.as_str(), execution_id.as_str());
    assert!(scoped_execution[0].pinned);

    let with_archived = skill_repo
        .list_by_project(
            &project_id,
            ProjectSkillListOptions {
                include_archived: true,
                bucket: Some("execution".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(with_archived.len(), 2);

    assert!(skill_repo
        .get_by_id(&ProjectSkillId::from_string("missing-skill".to_string()))
        .await
        .unwrap()
        .is_none());
    assert!(skill_repo
        .update_lifecycle_status(
            &ProjectSkillId::from_string("missing-skill".to_string()),
            ProjectSkillLifecycleStatus::Archived,
        )
        .await
        .unwrap()
        .is_none());
    assert!(skill_repo
        .update_pinned(
            &ProjectSkillId::from_string("missing-skill".to_string()),
            true
        )
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn project_skill_update_content_preserves_provenance_and_scope_filters() {
    let repo = SqliteProjectSkillRepository::from_shared(shared_test_connection());
    let mut skill = project_skill(
        "Draft Skill",
        "execution",
        "execution",
        ProjectSkillLifecycleStatus::Staged,
        Vec::new(),
    );
    let skill_id = skill.id.clone();
    repo.create(skill.clone()).await.unwrap();

    skill.title = "Updated Draft Skill".to_string();
    skill.bucket = "review".to_string();
    skill.stage = "review".to_string();
    skill.scope_paths = vec!["frontend/src/".to_string()];
    skill.compact_guidance = "Updated compact guidance.".to_string();
    skill.body_markdown = "Updated body.".to_string();
    skill.predicted_effect = Some("Updated effect.".to_string());
    let updated = repo.update_content(skill).await.unwrap().unwrap();

    assert_eq!(updated.id.as_str(), skill_id.as_str());
    assert_eq!(updated.bucket, "review");
    assert_eq!(updated.predicted_effect.as_deref(), Some("Updated effect."));
    assert_eq!(
        updated
            .provenance_json
            .get("source")
            .and_then(|value| value.as_str()),
        Some("sqlite-test")
    );

    let out_of_scope = repo
        .list_by_project(
            &ProjectId::from_string("project-1".to_string()),
            ProjectSkillListOptions {
                scope_path: Some("src-tauri/src/lib.rs".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(out_of_scope.is_empty());

    assert!(repo
        .update_content(ProjectSkill {
            id: ProjectSkillId::from_string("missing-skill".to_string()),
            ..project_skill(
                "Missing",
                "execution",
                "execution",
                ProjectSkillLifecycleStatus::Staged,
                Vec::new(),
            )
        })
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn project_skill_versions_are_appended_explicitly_and_content_updates_refresh_hashes() {
    let repo = SqliteProjectSkillRepository::from_shared(shared_test_connection());
    let mut skill = project_skill(
        "Versioned Skill",
        "review",
        "review",
        ProjectSkillLifecycleStatus::Staged,
        vec!["src-tauri/".to_string()],
    );
    skill.provenance_json = json!({
        "source": "task_outcome",
        "additional": {"pipeline_role": "reviewer"}
    });
    skill.created_by = ProjectSkillCreatedBy::Agent;

    let created = repo.create(skill).await.unwrap();
    assert_eq!(created.created_by, ProjectSkillCreatedBy::Agent);
    assert_eq!(created.pipeline_role.as_deref(), Some("reviewer"));
    assert_eq!(created.content_hash.len(), 64);
    assert_eq!(created.evidence_hash.len(), 64);
    assert!(repo.list_versions(&created.id).await.unwrap().is_empty());
    let v1 = ProjectSkillVersion::from_skill(&created, 1, created.updated_at);
    repo.append_version(v1).await.unwrap();

    let no_op = repo.update_content(created.clone()).await.unwrap().unwrap();
    assert_eq!(no_op.updated_at, created.updated_at);
    assert_eq!(no_op.content_hash, created.content_hash);
    assert_eq!(no_op.evidence_hash, created.evidence_hash);
    assert_eq!(repo.list_versions(&created.id).await.unwrap().len(), 1);

    let mut revision = created.clone();
    revision.body_markdown = "Revised body".to_string();
    revision.provenance_json["revision"] = json!(2);
    let revised = repo.update_content(revision).await.unwrap().unwrap();
    assert_ne!(revised.content_hash, created.content_hash);
    assert_ne!(revised.evidence_hash, created.evidence_hash);
    repo.append_version(ProjectSkillVersion::from_skill(
        &revised,
        2,
        revised.updated_at,
    ))
    .await
    .unwrap();
    let versions = repo.list_versions(&created.id).await.unwrap();
    assert_eq!(
        versions.iter().map(|row| row.version).collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert_eq!(versions[1].body_markdown, revised.body_markdown);
    assert_eq!(versions[1].provenance_json, revised.provenance_json);

    assert_eq!(repo.list_versions(&revised.id).await.unwrap().len(), 2);
}

#[tokio::test]
async fn project_skill_versions_cascade_and_malformed_rows_fail_closed() {
    let conn = shared_test_connection();
    let repo = SqliteProjectSkillRepository::from_shared(Arc::clone(&conn));
    let created = repo
        .create(project_skill(
            "Cascade Skill",
            "execution",
            "execution",
            ProjectSkillLifecycleStatus::Approved,
            Vec::new(),
        ))
        .await
        .unwrap();
    repo.append_version(ProjectSkillVersion::from_skill(
        &created,
        1,
        created.updated_at,
    ))
    .await
    .unwrap();

    conn.lock()
        .await
        .execute(
            "UPDATE project_skill_versions SET content_hash = 'invalid'
             WHERE project_skill_id = ?1 AND version = 1",
            [created.id.as_str()],
        )
        .unwrap();
    assert!(repo.list_versions(&created.id).await.is_err());

    conn.lock()
        .await
        .execute(
            "UPDATE project_skills SET evidence_hash = 'invalid' WHERE id = ?1",
            [created.id.as_str()],
        )
        .unwrap();
    assert!(repo.get_by_id(&created.id).await.is_err());

    conn.lock()
        .await
        .execute(
            "DELETE FROM project_skills WHERE id = ?1",
            [created.id.as_str()],
        )
        .unwrap();
    assert!(repo.list_versions(&created.id).await.unwrap().is_empty());
}

#[tokio::test]
async fn usage_events_filter_by_skill_and_run() {
    let conn = shared_test_connection();
    let outcome_repo = SqliteTaskOutcomeRepository::from_shared(Arc::clone(&conn));
    let skill_repo = SqliteProjectSkillRepository::from_shared(Arc::clone(&conn));
    let usage_repo = SqliteSkillUsageEventRepository::from_shared(conn);
    let project_id = ProjectId::from_string("project-1".to_string());
    let mut outcome = task_outcome(TaskOutcomeStatus::Succeeded, Some("execution_success"));
    outcome.id = TaskOutcomeId::from_string("outcome-1".to_string());
    outcome_repo
        .upsert(UpsertTaskOutcomeInput { outcome })
        .await
        .unwrap();
    let first_skill = project_skill(
        "First Usage Skill",
        "execution",
        "execution",
        ProjectSkillLifecycleStatus::Approved,
        Vec::new(),
    );
    let first_skill_id = first_skill.id.clone();
    let second_skill = project_skill(
        "Second Usage Skill",
        "review",
        "review",
        ProjectSkillLifecycleStatus::Approved,
        Vec::new(),
    );
    let second_skill_id = second_skill.id.clone();
    skill_repo.create(first_skill).await.unwrap();
    skill_repo.create(second_skill).await.unwrap();

    for (skill_id, run_id) in [
        (first_skill_id.clone(), "run-a"),
        (first_skill_id.clone(), "run-b"),
        (second_skill_id, "run-a"),
    ] {
        usage_repo
            .record(SkillUsageEvent {
                id: SkillUsageEventId::new(),
                project_id: project_id.clone(),
                project_skill_id: skill_id,
                conversation_id: Some("conversation-1".to_string()),
                agent_run_id: Some(run_id.to_string()),
                provider_harness: Some("codex".to_string()),
                stage: Some("execution".to_string()),
                bucket: Some("execution".to_string()),
                injection_kind: SkillUsageInjectionKind::CompactIndex,
                outcome_id: Some(TaskOutcomeId::from_string("outcome-1".to_string())),
                metadata_json: json!({ "run_id": run_id }),
                created_at: Utc::now(),
            })
            .await
            .unwrap();
    }

    let run_a_for_first_skill = usage_repo
        .list_by_project(
            &project_id,
            SkillUsageListOptions {
                project_skill_id: Some(first_skill_id),
                agent_run_id: Some("run-a".to_string()),
            },
        )
        .await
        .unwrap();
    assert_eq!(run_a_for_first_skill.len(), 1);
    assert_eq!(
        run_a_for_first_skill[0]
            .outcome_id
            .as_ref()
            .map(|id| id.as_str()),
        Some("outcome-1")
    );
}
