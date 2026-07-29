use chrono::{Duration, Utc};
use serde_json::json;

use super::{
    MemoryProjectSkillRepository, MemorySkillUsageEventRepository, MemoryTaskOutcomeRepository,
};
use crate::domain::entities::{
    ProjectId, ProjectSkill, ProjectSkillId, ProjectSkillLifecycleStatus, SkillUsageInjectionKind,
    TaskOutcome, TaskOutcomeClass, TaskOutcomeId, TaskOutcomeSource, TaskOutcomeStatus,
};
use crate::domain::repositories::{
    canonical_terminal_pr_source_ref_id, ProjectSkillMatchedMutation, ProjectSkillRepository,
    ProjectSkillResolutionCommand, ProjectSkillResolutionIntent, ProjectSkillResolutionOutcome,
    ProjectSkillStagingPolicy, SkillUsageEventRepository, SkillUsageListOptions,
    TaskOutcomeRepository, UpsertTaskOutcomeInput, AGENT_WORKSPACE_PR_OUTCOME_SOURCE,
    TERMINAL_PR_SOURCE_REF_KIND, WORKSPACE_PR_CLOSED_CLASS, WORKSPACE_PR_FAILED_CLASS,
    WORKSPACE_PR_MERGED_CLASS, WORKSPACE_PR_MERGED_CLEAN_CLASS,
    WORKSPACE_PR_MERGED_WITH_FOLLOWUPS_CLASS, WORKSPACE_PR_TERMINAL_CLASS,
};
use crate::domain::services::learned_skill_substrate::{
    new_c2_skill_usage_event, SkillUsageAttribution,
};
use crate::domain::services::project_skill_resolution::import_title_resolution_identity;

#[tokio::test]
async fn c2_memory_usage_batch_is_idempotent_and_failure_atomic() {
    let repo = MemorySkillUsageEventRepository::new();
    let project_id = ProjectId::from_string("project-1".to_string());
    let event = new_c2_skill_usage_event(
        project_id.clone(),
        ProjectSkillId::from_string("skill-1"),
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

    repo.record_batch(vec![event.clone(), event]).await.unwrap();
    assert_eq!(
        repo.list_by_project(&project_id, SkillUsageListOptions::default())
            .await
            .unwrap()
            .len(),
        1
    );

    repo.fail_next_batch_for_test();
    let second = new_c2_skill_usage_event(
        project_id.clone(),
        ProjectSkillId::from_string("skill-2"),
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
    assert!(repo.record_batch(vec![second]).await.is_err());
    assert_eq!(
        repo.list_by_project(&project_id, SkillUsageListOptions::default())
            .await
            .unwrap()
            .len(),
        1,
        "failed memory batch must not partially mutate rows"
    );
}

fn terminal_outcome(outcome_class: &str, evidence: &str) -> TaskOutcome {
    let now = Utc::now();
    TaskOutcome {
        id: TaskOutcomeId::new(),
        project_id: ProjectId::from_string("project-1".to_string()),
        source: AGENT_WORKSPACE_PR_OUTCOME_SOURCE,
        source_ref_kind: TERMINAL_PR_SOURCE_REF_KIND.to_string(),
        source_ref_id: canonical_terminal_pr_source_ref_id("42"),
        task_id: None,
        conversation_id: Some("conversation-1".to_string()),
        agent_run_id: None,
        pull_request_id: Some("42".to_string()),
        proposal_id: None,
        verification_id: None,
        review_id: None,
        outcome_class: Some(TaskOutcomeClass::from(outcome_class)),
        status: TaskOutcomeStatus::Eligible,
        evidence_json: json!({ "summary": evidence }),
        failure_fingerprint: None,
        provider_harness: Some("codex".to_string()),
        provider_session_id: Some("session-1".to_string()),
        created_at: now,
        updated_at: now,
    }
}

async fn upsert(repo: &MemoryTaskOutcomeRepository, outcome: TaskOutcome) -> TaskOutcome {
    repo.upsert(UpsertTaskOutcomeInput { outcome })
        .await
        .expect("upsert task outcome")
}

#[tokio::test]
async fn canonical_terminal_lattice_preserves_identity_context_and_lower_winner() {
    let repo = MemoryTaskOutcomeRepository::new();
    let generic = upsert(
        &repo,
        terminal_outcome(WORKSPACE_PR_TERMINAL_CLASS, "generic"),
    )
    .await;
    assert_eq!(generic.status, TaskOutcomeStatus::Unknown);

    let found = repo
        .get_by_dedupe(
            &generic.project_id,
            AGENT_WORKSPACE_PR_OUTCOME_SOURCE,
            TERMINAL_PR_SOURCE_REF_KIND,
            &canonical_terminal_pr_source_ref_id("42"),
        )
        .await
        .expect("read by dedupe")
        .expect("terminal outcome exists");
    assert_eq!(found.id.as_str(), generic.id.as_str());

    let closed = upsert(&repo, terminal_outcome(WORKSPACE_PR_CLOSED_CLASS, "closed")).await;
    assert_eq!(closed.id.as_str(), generic.id.as_str());
    assert_eq!(closed.created_at, generic.created_at);
    assert_eq!(closed.status, TaskOutcomeStatus::Failed);

    let mut equal = terminal_outcome(WORKSPACE_PR_FAILED_CLASS, "failed detail");
    equal.conversation_id = None;
    equal.provider_harness = None;
    equal.provider_session_id = None;
    let equal = upsert(&repo, equal).await;
    assert_eq!(
        equal.outcome_class.as_ref().map(TaskOutcomeClass::as_str),
        Some(WORKSPACE_PR_FAILED_CLASS)
    );
    assert_eq!(equal.evidence_json, json!({ "summary": "failed detail" }));
    assert_eq!(equal.conversation_id.as_deref(), Some("conversation-1"));
    assert_eq!(equal.provider_harness.as_deref(), Some("codex"));

    let merged = upsert(&repo, terminal_outcome(WORKSPACE_PR_MERGED_CLASS, "merged")).await;
    assert_eq!(merged.status, TaskOutcomeStatus::Succeeded);
    let stale = upsert(
        &repo,
        terminal_outcome(WORKSPACE_PR_CLOSED_CLASS, "stale close"),
    )
    .await;
    assert_eq!(stale.id.as_str(), merged.id.as_str());
    assert_eq!(stale.outcome_class, merged.outcome_class);
    assert_eq!(stale.status, merged.status);
    assert_eq!(stale.evidence_json, merged.evidence_json);
    assert_eq!(stale.updated_at, merged.updated_at);

    let clean = upsert(
        &repo,
        terminal_outcome(WORKSPACE_PR_MERGED_CLEAN_CLASS, "clean"),
    )
    .await;
    let followups = upsert(
        &repo,
        terminal_outcome(WORKSPACE_PR_MERGED_WITH_FOLLOWUPS_CLASS, "followups"),
    )
    .await;
    assert_eq!(followups.id.as_str(), clean.id.as_str());
    assert_eq!(
        followups
            .outcome_class
            .as_ref()
            .map(TaskOutcomeClass::as_str),
        Some(WORKSPACE_PR_MERGED_WITH_FOLLOWUPS_CLASS)
    );
    assert_eq!(followups.evidence_json, json!({ "summary": "followups" }));
}

#[tokio::test]
async fn noncanonical_outcomes_remain_last_write_wins_and_missing_dedupe_is_none() {
    let repo = MemoryTaskOutcomeRepository::new();
    let mut first = terminal_outcome("first", "first");
    first.source_ref_id = "42:terminal:legacy".to_string();
    first.status = TaskOutcomeStatus::Failed;
    upsert(&repo, first).await;

    let mut second = terminal_outcome("second", "second");
    second.source_ref_id = "42:terminal:legacy".to_string();
    second.status = TaskOutcomeStatus::Eligible;
    let saved = upsert(&repo, second).await;
    assert_eq!(
        saved.outcome_class.as_ref().map(TaskOutcomeClass::as_str),
        Some("second")
    );
    assert_eq!(saved.status, TaskOutcomeStatus::Eligible);

    let unknown = upsert(&repo, terminal_outcome("unrecognized", "unknown")).await;
    assert_eq!(unknown.status, TaskOutcomeStatus::Unknown);

    let mut mismatched = terminal_outcome(WORKSPACE_PR_MERGED_CLASS, "mismatched");
    mismatched.source_ref_id = canonical_terminal_pr_source_ref_id("99");
    mismatched.status = TaskOutcomeStatus::Eligible;
    let mismatched = upsert(&repo, mismatched).await;
    assert_eq!(mismatched.status, TaskOutcomeStatus::Eligible);
    assert!(repo
        .get_by_dedupe(
            &saved.project_id,
            AGENT_WORKSPACE_PR_OUTCOME_SOURCE,
            TERMINAL_PR_SOURCE_REF_KIND,
            "missing",
        )
        .await
        .expect("missing read")
        .is_none());
}

#[tokio::test]
async fn memory_recurrence_corpus_is_project_and_distinct_session_scoped() {
    let repo = MemoryTaskOutcomeRepository::new();
    let key = format!("token-set-v1:{}", "a".repeat(64));
    for (index, session) in ["session-1", "session-1", "session-2"].iter().enumerate() {
        let mut outcome = terminal_outcome("recurrence", "same failure");
        outcome.source = match index {
            0 => TaskOutcomeSource::Review,
            1 => TaskOutcomeSource::Merge,
            _ => TaskOutcomeSource::AgentConversation,
        };
        outcome.source_ref_kind = "fixture".to_string();
        outcome.source_ref_id = format!("row-{index}");
        outcome.status = if index == 2 {
            TaskOutcomeStatus::Eligible
        } else {
            TaskOutcomeStatus::Failed
        };
        outcome.evidence_json = json!({
            "recurrence_key": key,
            "recurrence_session": session,
        });
        upsert(&repo, outcome).await;
    }
    let mut missing_session = terminal_outcome("recurrence", "same failure");
    missing_session.source = TaskOutcomeSource::MergeValidation;
    missing_session.source_ref_kind = "fixture".to_string();
    missing_session.source_ref_id = "missing-session".to_string();
    missing_session.status = TaskOutcomeStatus::Failed;
    missing_session.evidence_json = json!({ "recurrence_key": key });
    upsert(&repo, missing_session).await;

    let corpus = repo
        .recurrence_corpus(&ProjectId::from_string("project-1".to_string()), &key)
        .await
        .expect("query recurrence corpus");

    assert_eq!(corpus.eligible_observations, 3);
    assert_eq!(corpus.distinct_sessions, 2);
    assert_eq!(
        repo.recurrence_corpus(&ProjectId::from_string("project-2".to_string()), &key,)
            .await
            .expect("query other project"),
        Default::default()
    );
}

fn project_skill() -> ProjectSkill {
    let now = Utc::now();
    ProjectSkill {
        id: ProjectSkillId::from_string("memory-skill"),
        project_id: ProjectId::from_string("project-1".to_string()),
        title: "Memory versioning".to_string(),
        bucket: "execution".to_string(),
        stage: "execution".to_string(),
        status: ProjectSkillLifecycleStatus::Staged,
        pinned: false,
        archived: false,
        scope_paths: Vec::new(),
        compact_guidance: "Keep current and snapshots together.".to_string(),
        body_markdown: "Version one".to_string(),
        predicted_effect: Some("Prevents split-brain state.".to_string()),
        provenance_json: json!({"source": "task_outcome"}),
        companion_of_skill_id: None,
        content_hash: "caller-controlled".to_string(),
        evidence_hash: "caller-controlled".to_string(),
        created_by: crate::domain::entities::ProjectSkillCreatedBy::Imported,
        pipeline_role: Some("caller-controlled".to_string()),
        created_at: now,
        updated_at: now,
    }
}

fn pipeline_command(
    mut skill: ProjectSkill,
    identity: &str,
    role: &str,
) -> ProjectSkillResolutionCommand {
    skill.id = ProjectSkillId::new();
    skill.title = identity.to_string();
    skill.created_by = crate::domain::entities::ProjectSkillCreatedBy::Agent;
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
async fn memory_project_skill_repository_persists_versions_only_when_explicitly_appended() {
    let repo = MemoryProjectSkillRepository::new();
    let created = repo.create(project_skill()).await.unwrap();
    assert_eq!(
        created.created_by,
        crate::domain::entities::ProjectSkillCreatedBy::Imported
    );
    assert_ne!(created.content_hash, "caller-controlled");
    assert_ne!(created.evidence_hash, "caller-controlled");
    assert!(repo.list_versions(&created.id).await.unwrap().is_empty());

    let v1 =
        crate::domain::entities::ProjectSkillVersion::from_skill(&created, 1, created.updated_at);
    repo.append_version(v1.clone()).await.unwrap();
    assert!(matches!(
        repo.append_version(v1).await,
        Err(crate::error::AppError::Conflict(_))
    ));

    let mut revised = created.clone();
    revised.body_markdown = "Version two".to_string();
    let revised = repo.update_content(revised).await.unwrap().unwrap();
    assert!(repo
        .list_versions(&created.id)
        .await
        .unwrap()
        .iter()
        .all(|row| row.version == 1));
    repo.append_version(crate::domain::entities::ProjectSkillVersion::from_skill(
        &revised,
        2,
        revised.updated_at,
    ))
    .await
    .unwrap();
    assert_eq!(repo.list_versions(&created.id).await.unwrap().len(), 2);
}

#[tokio::test]
async fn memory_pipeline_cap_is_project_scoped_and_leaves_blocked_state_unchanged() {
    let repo = MemoryProjectSkillRepository::new();
    let mut first = project_skill();
    first.project_id = ProjectId::from_string("project-1".to_string());
    let mut second = first.clone();
    second.id = ProjectSkillId::new();
    repo.resolve(pipeline_command(first, "project-one-a", "memory_capture"))
        .await
        .expect("first create");
    repo.resolve(pipeline_command(second, "project-one-b", "memory_capture"))
        .await
        .expect("second create");

    let mut blocked = project_skill();
    blocked.project_id = ProjectId::from_string("project-1".to_string());
    let project_id = blocked.project_id.clone();
    let rows_before = repo
        .list_by_project(&project_id, Default::default())
        .await
        .expect("rows before")
        .len();
    assert!(matches!(
        repo.resolve(pipeline_command(blocked, "project-one-c", "memory_capture"))
            .await,
        Err(crate::error::AppError::Conflict(_))
    ));
    assert_eq!(
        repo.list_by_project(&project_id, Default::default())
            .await
            .expect("rows after")
            .len(),
        rows_before
    );

    let mut other_project = project_skill();
    other_project.project_id = ProjectId::from_string("project-2".to_string());
    assert_eq!(
        repo.resolve(pipeline_command(
            other_project,
            "project-two-a",
            "memory_capture"
        ))
        .await
        .expect("other project")
        .outcome,
        ProjectSkillResolutionOutcome::CreateNew
    );
}

#[tokio::test]
async fn memory_pipeline_cap_runs_after_duplicate_and_counts_null_role_for_every_role() {
    let repo = MemoryProjectSkillRepository::new();
    let mut legacy = project_skill();
    legacy.project_id = ProjectId::from_string("project-null-role".to_string());
    legacy.bucket = "review".to_string();
    legacy.stage = "review".to_string();
    legacy.title = "Legacy NULL role".to_string();
    legacy.pipeline_role = None;
    repo.seed_for_test(legacy)
        .await
        .expect("seed legacy null-role row");

    let mut first = project_skill();
    first.project_id = ProjectId::from_string("project-null-role".to_string());
    first.bucket = "review".to_string();
    first.stage = "review".to_string();
    first.title = "Role-specific row".to_string();
    let first_command = pipeline_command(first, "role-specific", "memory_capture");
    repo.resolve(first_command.clone())
        .await
        .expect("create after null-role row");

    let duplicate = repo
        .resolve(first_command)
        .await
        .expect("duplicate remains allowed at cap");
    assert_eq!(duplicate.outcome, ProjectSkillResolutionOutcome::Duplicate);
    assert!(duplicate.version.is_none());

    let mut capture_blocked = project_skill();
    capture_blocked.project_id = ProjectId::from_string("project-null-role".to_string());
    capture_blocked.bucket = "review".to_string();
    capture_blocked.stage = "review".to_string();
    capture_blocked.title = "Blocked memory capture".to_string();
    assert!(matches!(
        repo.resolve(pipeline_command(
            capture_blocked,
            "capture-blocked",
            "memory_capture",
        ))
        .await,
        Err(crate::error::AppError::Conflict(_))
    ));

    let mut maintainer_first = project_skill();
    maintainer_first.project_id = ProjectId::from_string("project-null-role".to_string());
    maintainer_first.bucket = "review".to_string();
    maintainer_first.stage = "review".to_string();
    maintainer_first.title = "Maintainer remaining slot".to_string();
    repo.resolve(pipeline_command(
        maintainer_first,
        "maintainer-first",
        "memory_maintainer",
    ))
    .await
    .expect("maintainer has one slot after the null-role row");

    let mut maintainer_blocked = project_skill();
    maintainer_blocked.project_id = ProjectId::from_string("project-null-role".to_string());
    maintainer_blocked.bucket = "review".to_string();
    maintainer_blocked.stage = "review".to_string();
    maintainer_blocked.title = "Blocked memory maintainer".to_string();
    assert!(matches!(
        repo.resolve(pipeline_command(
            maintainer_blocked,
            "maintainer-blocked",
            "memory_maintainer",
        ))
        .await,
        Err(crate::error::AppError::Conflict(_))
    ));

    let mut other_bucket = project_skill();
    other_bucket.project_id = ProjectId::from_string("project-null-role".to_string());
    other_bucket.bucket = "planning".to_string();
    other_bucket.stage = "planning".to_string();
    other_bucket.title = "Independent bucket".to_string();
    assert_eq!(
        repo.resolve(pipeline_command(
            other_bucket,
            "independent-bucket",
            "memory_capture",
        ))
        .await
        .expect("other bucket")
        .outcome,
        ProjectSkillResolutionOutcome::CreateNew
    );
}
