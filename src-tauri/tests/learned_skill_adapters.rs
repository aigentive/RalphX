use ralphx_lib::domain::services::learned_skill_adapters::{
    build_constraint_bundle_skill_citations, capture_plan_mode_verdict,
    project_skill_to_learned_skill_record, select_pre_execution_learned_skills,
    select_pre_execution_learned_skills_multi, verification_gap_recurrence_candidates,
    LearnedSkillBucket, LearnedSkillMultiSelectionRequest, LearnedSkillRecord,
    LearnedSkillSelectionRequest, LearnedSkillStage, LearnedSkillStatus, PlanModeVerdict,
    PlanModeVerdictCaptureInput, ProjectSkillMappingError, VerificationGapRecurrenceGate,
    VerificationGapSuppressionReason,
};
use ralphx_lib::domain::{
    entities::{
        ideation::VerificationRoundSnapshot, ProjectId, ProjectSkill, ProjectSkillCreatedBy,
        ProjectSkillId, ProjectSkillLifecycleStatus,
    },
    services::gap_fingerprint,
};
use ralphx_lib::infrastructure::agents::internal_skills::{
    inject_learned_skill_citations_into_system_prompt,
    inject_pre_execution_learned_skills_into_system_prompt, InternalSkillInjection,
};

#[test]
fn plan_mode_verdict_capture_records_compact_accepted_outcome_without_session_mutation() {
    let input = PlanModeVerdictCaptureInput {
        project_id: "project-1".to_string(),
        conversation_id: "conversation-1".to_string(),
        planning_session_id: Some("planning-session-1".to_string()),
        accepted_session_id: Some("accepted-session-1".to_string()),
        plan_artifact_id: Some("plan-1".to_string()),
        verdict: PlanModeVerdict::Accepted,
        reason: Some("Architecture work needs plan mode.".to_string()),
    };

    let outcome = capture_plan_mode_verdict(input).expect("accepted verdict is eligible");

    assert_eq!(outcome.project_id, "project-1");
    assert_eq!(outcome.outcome_class, "accepted");
    assert_eq!(outcome.status, "eligible");
    assert_eq!(
        outcome.refs.get("accepted_session_id").map(String::as_str),
        Some("accepted-session-1")
    );
    assert_eq!(
        outcome.refs.get("planning_session_id").map(String::as_str),
        Some("planning-session-1")
    );
    assert!(!outcome.mutates_accepted_session);
    assert!(outcome.evidence_summary.contains("Architecture work"));
}

#[test]
fn plan_mode_verdict_capture_suppresses_skipped_or_unlinked_verdicts() {
    let skipped = PlanModeVerdictCaptureInput {
        project_id: "project-1".to_string(),
        conversation_id: "conversation-1".to_string(),
        planning_session_id: Some("planning-session-1".to_string()),
        accepted_session_id: None,
        plan_artifact_id: None,
        verdict: PlanModeVerdict::Skipped,
        reason: Some("Narrow edit.".to_string()),
    };
    assert!(capture_plan_mode_verdict(skipped).is_none());

    let unlinked = PlanModeVerdictCaptureInput {
        project_id: "project-1".to_string(),
        conversation_id: "conversation-1".to_string(),
        planning_session_id: None,
        accepted_session_id: Some("accepted-session-1".to_string()),
        plan_artifact_id: None,
        verdict: PlanModeVerdict::Accepted,
        reason: None,
    };
    assert!(capture_plan_mode_verdict(unlinked).is_none());
}

#[test]
fn pre_execution_selection_filters_by_approval_project_stage_bucket_and_path() {
    let selected = select_pre_execution_learned_skills(
        LearnedSkillSelectionRequest {
            project_id: "project-1".to_string(),
            caller_surface: "ralphx-execution-worker".to_string(),
            stage: LearnedSkillStage::Execution,
            bucket: LearnedSkillBucket::Execution,
            touched_paths: vec!["src-tauri/src/application/chat_service/mod.rs".to_string()],
            max_skills: 4,
        },
        &[
            skill("skill-match", "project-1", LearnedSkillStatus::Approved)
                .with_caller_surfaces(vec!["ralphx-execution-worker"])
                .with_stages(vec![LearnedSkillStage::Execution])
                .with_buckets(vec![LearnedSkillBucket::Execution])
                .with_path_scopes(vec!["src-tauri/src/application"]),
            skill("skill-surface", "project-1", LearnedSkillStatus::Approved)
                .with_caller_surfaces(vec!["ralphx-review-history"])
                .with_stages(vec![LearnedSkillStage::Execution])
                .with_buckets(vec![LearnedSkillBucket::Execution])
                .with_path_scopes(vec!["src-tauri/src/application"]),
            skill("skill-staged", "project-1", LearnedSkillStatus::Staged)
                .with_caller_surfaces(vec!["ralphx-execution-worker"])
                .with_stages(vec![LearnedSkillStage::Execution])
                .with_buckets(vec![LearnedSkillBucket::Execution])
                .with_path_scopes(vec!["src-tauri/src/application"]),
            skill("skill-project", "project-2", LearnedSkillStatus::Approved)
                .with_caller_surfaces(vec!["ralphx-execution-worker"])
                .with_stages(vec![LearnedSkillStage::Execution])
                .with_buckets(vec![LearnedSkillBucket::Execution])
                .with_path_scopes(vec!["src-tauri/src/application"]),
            skill("skill-stage", "project-1", LearnedSkillStatus::Approved)
                .with_caller_surfaces(vec!["ralphx-execution-worker"])
                .with_stages(vec![LearnedSkillStage::Planning])
                .with_buckets(vec![LearnedSkillBucket::Execution])
                .with_path_scopes(vec!["src-tauri/src/application"]),
            skill("skill-path", "project-1", LearnedSkillStatus::Approved)
                .with_caller_surfaces(vec!["ralphx-execution-worker"])
                .with_stages(vec![LearnedSkillStage::Execution])
                .with_buckets(vec![LearnedSkillBucket::Execution])
                .with_path_scopes(vec!["frontend/src"]),
        ],
    );

    assert_eq!(
        selected
            .iter()
            .map(|skill| skill.id.as_str())
            .collect::<Vec<_>>(),
        vec!["skill-match"]
    );
}

#[test]
fn pre_execution_selection_rejects_absolute_touched_paths_before_scope_matching() {
    let selected = select_pre_execution_learned_skills(
        LearnedSkillSelectionRequest {
            project_id: "project-1".to_string(),
            caller_surface: "ralphx-execution-worker".to_string(),
            stage: LearnedSkillStage::Execution,
            bucket: LearnedSkillBucket::Execution,
            touched_paths: vec!["/src-tauri/src/application/chat_service/mod.rs".to_string()],
            max_skills: 4,
        },
        &[
            skill("skill-match", "project-1", LearnedSkillStatus::Approved)
                .with_caller_surfaces(vec!["ralphx-execution-worker"])
                .with_stages(vec![LearnedSkillStage::Execution])
                .with_buckets(vec![LearnedSkillBucket::Execution])
                .with_path_scopes(vec!["src-tauri/src/application"]),
        ],
    );

    assert!(
        selected.is_empty(),
        "absolute paths must not be converted into relative scope matches"
    );
}

#[test]
fn project_skill_mapping_is_approved_only_and_fails_closed_for_invalid_capabilities() {
    let approved = project_skill("skill-approved", ProjectSkillLifecycleStatus::Approved);
    let mapped =
        project_skill_to_learned_skill_record(&approved, "ralphx-execution-reviewer").unwrap();
    assert_eq!(mapped.status, LearnedSkillStatus::Approved);
    assert_eq!(mapped.stages, vec![LearnedSkillStage::Review]);
    assert_eq!(mapped.buckets, vec![LearnedSkillBucket::Review]);
    assert!(mapped.pinned);

    for status in [
        ProjectSkillLifecycleStatus::Staged,
        ProjectSkillLifecycleStatus::Rejected,
        ProjectSkillLifecycleStatus::Stale,
        ProjectSkillLifecycleStatus::Archived,
        ProjectSkillLifecycleStatus::Retired,
    ] {
        let mapped =
            project_skill_to_learned_skill_record(&project_skill("excluded", status), "reviewer")
                .unwrap();
        assert_ne!(mapped.status, LearnedSkillStatus::Approved);
    }

    let mut invalid = approved.clone();
    invalid.bucket = "later-phase".to_string();
    assert!(matches!(
        project_skill_to_learned_skill_record(&invalid, "reviewer"),
        Err(ProjectSkillMappingError::InvalidBucket(_))
    ));
    invalid.bucket = "review".to_string();
    invalid.stage = "later-phase".to_string();
    assert!(matches!(
        project_skill_to_learned_skill_record(&invalid, "reviewer"),
        Err(ProjectSkillMappingError::InvalidStage(_))
    ));
}

#[test]
fn multi_bucket_selection_is_deterministic_deduped_and_pinned_first() {
    let planning = skill("skill-planning", "project-1", LearnedSkillStatus::Approved)
        .with_caller_surfaces(vec!["ralphx-general-worker"])
        .with_stages(vec![LearnedSkillStage::Planning])
        .with_buckets(vec![LearnedSkillBucket::Planning]);
    let review = skill("skill-review", "project-1", LearnedSkillStatus::Approved)
        .with_caller_surfaces(vec!["ralphx-general-worker"])
        .with_stages(vec![LearnedSkillStage::Review])
        .with_buckets(vec![LearnedSkillBucket::Review]);
    let pinned_review = skill(
        "skill-pinned-review",
        "project-1",
        LearnedSkillStatus::Approved,
    )
    .with_caller_surfaces(vec!["ralphx-general-worker"])
    .with_stages(vec![LearnedSkillStage::Review])
    .with_buckets(vec![LearnedSkillBucket::Review])
    .with_pinned(true);

    let request = LearnedSkillMultiSelectionRequest {
        project_id: "project-1".to_string(),
        caller_surface: "ralphx-general-worker".to_string(),
        stages: vec![LearnedSkillStage::Planning, LearnedSkillStage::Review],
        buckets: vec![LearnedSkillBucket::Planning, LearnedSkillBucket::Review],
        touched_paths: Vec::new(),
        max_skills: 4,
    };
    let selected = select_pre_execution_learned_skills_multi(
        request.clone(),
        &[
            review.clone(),
            planning.clone(),
            pinned_review.clone(),
            planning.clone(),
        ],
    );
    let shuffled =
        select_pre_execution_learned_skills_multi(request, &[planning, pinned_review, review]);

    let ids = |records: &[LearnedSkillRecord]| {
        records
            .iter()
            .map(|record| record.id.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        ids(&selected),
        vec!["skill-pinned-review", "skill-planning", "skill-review"]
    );
    assert_eq!(ids(&selected), ids(&shuffled));
}

#[test]
fn constraint_bundle_citations_render_compact_approved_skill_context_without_tool_mentions() {
    let selected = vec![skill(
        "skill-review-loop",
        "project-1",
        LearnedSkillStatus::Approved,
    )
    .with_caller_surfaces(vec!["ralphx-ideation"])
    .with_stages(vec![LearnedSkillStage::Planning])
    .with_buckets(vec![LearnedSkillBucket::Planning])
    .with_path_scopes(vec!["src-tauri/src/domain"])
    .with_predicted_effect("Reduce repeated verification gaps about missing tests.")
    .with_provenance_refs(vec!["outcome-1", "verification-round-3"])];
    let citations = build_constraint_bundle_skill_citations(&selected);

    assert_eq!(citations.len(), 1);
    assert_eq!(citations[0].skill_id, "skill-review-loop");
    assert_eq!(
        citations[0].predicted_effect,
        "Reduce repeated verification gaps about missing tests."
    );

    let injection: InternalSkillInjection =
        inject_learned_skill_citations_into_system_prompt("Base system prompt.", &citations);

    assert!(injection
        .system_prompt
        .contains("<ralphx_learned_skill_citations>"));
    assert!(injection.system_prompt.contains("skill-review-loop"));
    assert!(injection.system_prompt.contains("outcome-1"));
    assert!(!injection.system_prompt.contains("list_project_skills"));
    assert!(!injection.system_prompt.contains("get_project_skill"));
    assert_eq!(
        injection.injected_skill_names,
        vec!["learned:skill-review-loop"]
    );
}

#[test]
fn pre_execution_injection_filters_eligible_skills_before_rendering_citations() {
    let injection = inject_pre_execution_learned_skills_into_system_prompt(
        "Base system prompt.",
        LearnedSkillSelectionRequest {
            project_id: "project-1".to_string(),
            caller_surface: "ralphx-ideation".to_string(),
            stage: LearnedSkillStage::Planning,
            bucket: LearnedSkillBucket::Planning,
            touched_paths: vec!["src-tauri/src/domain/services/mod.rs".to_string()],
            max_skills: 4,
        },
        &[
            skill("skill-planning", "project-1", LearnedSkillStatus::Approved)
                .with_caller_surfaces(vec!["ralphx-ideation"])
                .with_stages(vec![LearnedSkillStage::Planning])
                .with_buckets(vec![LearnedSkillBucket::Planning])
                .with_path_scopes(vec!["src-tauri/src/domain"])
                .with_predicted_effect("Reduce repeated planning misses."),
            skill(
                "skill-wrong-surface",
                "project-1",
                LearnedSkillStatus::Approved,
            )
            .with_caller_surfaces(vec!["ralphx-review-history"])
            .with_stages(vec![LearnedSkillStage::Planning])
            .with_buckets(vec![LearnedSkillBucket::Planning])
            .with_path_scopes(vec!["src-tauri/src/domain"]),
        ],
    );

    assert!(injection
        .system_prompt
        .contains("<ralphx_learned_skill_citations>"));
    assert!(injection.system_prompt.contains("skill-planning"));
    assert!(!injection.system_prompt.contains("skill-wrong-surface"));
    assert_eq!(
        injection.injected_skill_names,
        vec!["learned:skill-planning"]
    );
}

#[test]
fn verification_gap_recurrence_suppresses_low_volume_gap_evidence() {
    let report = verification_gap_recurrence_candidates(
        &[round(1, &["Missing reviewer regression test"])],
        VerificationGapRecurrenceGate {
            min_occurrences: 2,
            min_rounds: 2,
            min_corpus_size: 3,
        },
    );

    assert_eq!(report.corpus_size, 1);
    assert!(report.recurring_gaps.is_empty());
    assert_eq!(report.suppressed_gaps.len(), 1);
    assert_eq!(
        report.suppressed_gaps[0].reason,
        VerificationGapSuppressionReason::BelowCorpusGate
    );
}

#[test]
fn verification_gap_recurrence_promotes_only_after_min_occurrence_and_corpus_gates() {
    let report = verification_gap_recurrence_candidates(
        &[
            round(1, &["Missing reviewer regression test"]),
            round(2, &["reviewer regression test missing"]),
            round(
                3,
                &[
                    "Missing reviewer regression test",
                    "Document export preview",
                ],
            ),
        ],
        VerificationGapRecurrenceGate {
            min_occurrences: 2,
            min_rounds: 2,
            min_corpus_size: 3,
        },
    );

    assert_eq!(report.corpus_size, 4);
    assert_eq!(report.recurring_gaps.len(), 1);
    let recurring = &report.recurring_gaps[0];
    assert_eq!(
        recurring.fingerprint,
        gap_fingerprint("Missing reviewer regression test")
    );
    assert_eq!(recurring.occurrences, 3);
    assert_eq!(recurring.distinct_rounds, 3);
    assert!(recurring
        .descriptions
        .contains(&"Missing reviewer regression test".to_string()));
    assert_eq!(report.suppressed_gaps.len(), 1);
}

fn skill(id: &str, project_id: &str, status: LearnedSkillStatus) -> LearnedSkillRecord {
    LearnedSkillRecord {
        id: id.to_string(),
        project_id: project_id.to_string(),
        title: format!("Skill {id}"),
        status,
        pinned: false,
        caller_surfaces: Vec::new(),
        stages: Vec::new(),
        buckets: Vec::new(),
        path_scopes: Vec::new(),
        compact_guidance: "Keep the learned guidance compact.".to_string(),
        predicted_effect: "Reduce repeated mistakes.".to_string(),
        provenance_refs: Vec::new(),
    }
}

fn project_skill(id: &str, status: ProjectSkillLifecycleStatus) -> ProjectSkill {
    let now = chrono::Utc::now();
    ProjectSkill {
        id: ProjectSkillId::from_string(id),
        project_id: ProjectId::from_string("project-1".to_string()),
        title: format!("Skill {id}"),
        bucket: "review".to_string(),
        stage: "review".to_string(),
        status,
        pinned: true,
        archived: status == ProjectSkillLifecycleStatus::Archived,
        scope_paths: Vec::new(),
        compact_guidance: "Keep review evidence current.".to_string(),
        body_markdown: "Use the current diff and production entry paths.".to_string(),
        predicted_effect: Some("Reduce repeated review misses.".to_string()),
        provenance_json: serde_json::json!({"outcome_id": "outcome-1"}),
        companion_of_skill_id: None,
        content_hash: "content-hash".to_string(),
        evidence_hash: "evidence-hash".to_string(),
        created_by: ProjectSkillCreatedBy::Agent,
        pipeline_role: Some("skill_distiller".to_string()),
        created_at: now,
        updated_at: now,
    }
}

fn round(round: u32, descriptions: &[&str]) -> VerificationRoundSnapshot {
    let gaps = descriptions
        .iter()
        .map(
            |description| ralphx_lib::domain::entities::ideation::VerificationGap {
                severity: "high".to_string(),
                category: "testing".to_string(),
                description: (*description).to_string(),
                why_it_matters: Some(
                    "Recurring verification gaps should stay descriptive until gated.".to_string(),
                ),
                source: Some("layer2".to_string()),
            },
        )
        .collect::<Vec<_>>();
    VerificationRoundSnapshot {
        round,
        fingerprints: gaps
            .iter()
            .map(|gap| gap_fingerprint(&gap.description))
            .collect(),
        gap_score: gaps.len() as u32 * 3,
        gaps,
        parse_failed: false,
    }
}
