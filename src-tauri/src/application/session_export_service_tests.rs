// Integration tests for SessionExportService
// Uses real SQLite with migrations applied (not MemoryRepository).
// AppState::new_sqlite_test() provides a shared in-memory connection with FK off.

use crate::application::session_export_service::{
    DependencyData, PlanVersionData, ProposalData, PriorityFactorsData,
    SessionExport, SessionExportService, SessionData, SourceInstance, VerificationExportData,
};
use crate::application::AppState;
use crate::error::AppError;

// ─────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────

fn make_service(app_state: &AppState) -> SessionExportService {
    SessionExportService::new(app_state.db.clone())
}

/// Seed a project row directly via db.run. FK is off so project can be minimal.
async fn seed_project(app_state: &AppState, project_id: &str, name: &str) {
    let pid = project_id.to_string();
    let name = name.to_string();
    app_state
        .db
        .run(move |conn| {
            conn.execute(
                "INSERT INTO projects (id, name, working_directory, git_mode, created_at, updated_at) \
                 VALUES (?1, ?2, ?3, 'local', datetime('now'), datetime('now'))",
                rusqlite::params![pid, name, format!("/tmp/test-{}", pid)],
            )?;
            Ok(())
        })
        .await
        .expect("seed_project failed");
}

/// Seed an ideation_session row with a specific plan_artifact_id.
async fn seed_session(
    app_state: &AppState,
    session_id: &str,
    project_id: &str,
    plan_artifact_id: Option<&str>,
) {
    let sid = session_id.to_string();
    let pid = project_id.to_string();
    let aid = plan_artifact_id.map(|s| s.to_string());
    app_state
        .db
        .run(move |conn| {
            conn.execute(
                "INSERT INTO ideation_sessions \
                 (id, project_id, title, title_source, status, plan_artifact_id, \
                  inherited_plan_artifact_id, seed_task_id, parent_session_id, \
                  created_at, updated_at, archived_at, converted_at, team_mode, team_config_json) \
                 VALUES (?1, ?2, 'Test Session', 'user', 'active', ?3, NULL, NULL, NULL, \
                         datetime('now'), datetime('now'), NULL, NULL, 'solo', NULL)",
                rusqlite::params![sid, pid, aid],
            )?;
            Ok(())
        })
        .await
        .expect("seed_session failed");
}

/// Seed a followup session with NULL plan_artifact_id but a set inherited_plan_artifact_id.
async fn seed_followup_session(
    app_state: &AppState,
    session_id: &str,
    project_id: &str,
    inherited_plan_artifact_id: &str,
) {
    let sid = session_id.to_string();
    let pid = project_id.to_string();
    let iaid = inherited_plan_artifact_id.to_string();
    app_state
        .db
        .run(move |conn| {
            conn.execute(
                "INSERT INTO ideation_sessions \
                 (id, project_id, title, title_source, status, plan_artifact_id, \
                  inherited_plan_artifact_id, seed_task_id, parent_session_id, \
                  created_at, updated_at, archived_at, converted_at, team_mode, team_config_json) \
                 VALUES (?1, ?2, 'Followup Session', 'user', 'active', NULL, ?3, NULL, NULL, \
                         datetime('now'), datetime('now'), NULL, NULL, 'solo', NULL)",
                rusqlite::params![sid, pid, iaid],
            )?;
            Ok(())
        })
        .await
        .expect("seed_followup_session failed");
}

/// Seed an artifact row. Returns the new artifact_id.
async fn seed_artifact(
    app_state: &AppState,
    artifact_id: &str,
    version: u32,
    name: &str,
    content: &str,
    previous_version_id: Option<&str>,
) {
    let aid = artifact_id.to_string();
    let name = name.to_string();
    let content = content.to_string();
    let prev = previous_version_id.map(|s| s.to_string());
    app_state
        .db
        .run(move |conn| {
            conn.execute(
                "INSERT INTO artifacts \
                 (id, type, name, content_type, content_text, content_path, \
                  bucket_id, task_id, process_id, created_by, version, \
                  previous_version_id, created_at, metadata_json) \
                 VALUES (?1, 'specification', ?2, 'text/markdown', ?3, NULL, \
                         NULL, NULL, NULL, 'test', ?4, ?5, datetime('now'), NULL)",
                rusqlite::params![aid, name, content, version, prev],
            )?;
            Ok(())
        })
        .await
        .expect("seed_artifact failed");
}

/// Seed a task_proposal row. Returns the proposal_id.
async fn seed_proposal(
    app_state: &AppState,
    proposal_id: &str,
    session_id: &str,
    title: &str,
    sort_order: i32,
    plan_artifact_id: Option<&str>,
) {
    let pid = proposal_id.to_string();
    let sid = session_id.to_string();
    let title = title.to_string();
    let aid = plan_artifact_id.map(|s| s.to_string());
    app_state
        .db
        .run(move |conn| {
            conn.execute(
                "INSERT INTO task_proposals \
                 (id, session_id, title, description, category, steps, acceptance_criteria, \
                  suggested_priority, priority_score, priority_reason, priority_factors, \
                  estimated_complexity, user_priority, user_modified, status, selected, \
                  created_task_id, plan_artifact_id, plan_version_at_creation, sort_order, \
                  created_at, updated_at) \
                 VALUES (?1, ?2, ?3, NULL, 'feature', \
                         '[\"step1\",\"step2\"]', '[\"criterion1\"]', \
                         'high', 80, NULL, NULL, \
                         'moderate', NULL, 0, 'pending', 0, NULL, ?4, 1, ?5, \
                         datetime('now'), datetime('now'))",
                rusqlite::params![pid, sid, title, aid, sort_order],
            )?;
            Ok(())
        })
        .await
        .expect("seed_proposal failed");
}

/// Seed a proposal_dependency row.
async fn seed_dependency(
    app_state: &AppState,
    dep_id: &str,
    proposal_id: &str,
    depends_on_id: &str,
    source: &str,
) {
    let did = dep_id.to_string();
    let pid = proposal_id.to_string();
    let dpid = depends_on_id.to_string();
    let src = source.to_string();
    app_state
        .db
        .run(move |conn| {
            conn.execute(
                "INSERT INTO proposal_dependencies \
                 (id, proposal_id, depends_on_proposal_id, source) \
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![did, pid, dpid, src],
            )?;
            Ok(())
        })
        .await
        .expect("seed_dependency failed");
}

fn build_minimal_export() -> SessionExport {
    SessionExport {
        schema_version: 1,
        exported_at: "2026-03-13T00:00:00Z".into(),
        source_instance: SourceInstance {
            project_name: "Test Project".into(),
        },
        session: SessionData {
            title: Some("Imported Session".into()),
            status: "active".into(),
            team_mode: "solo".into(),
            verification: Some(VerificationExportData {
                status: "verified".into(),
                in_progress: false,
                generation: 0,
                current_round: None,
                max_rounds: None,
                gap_count: 0,
                gap_score: None,
                convergence_reason: None,
            }),
        },
        plan_versions: vec![PlanVersionData {
            version: 1,
            name: "Plan v1".into(),
            content: Some("# Plan".into()),
            content_type: "text/markdown".into(),
            created_at: "2026-03-13T00:00:00Z".into(),
            created_by: "test".into(),
        }],
        proposals: vec![ProposalData {
            index: 0,
            title: "Feature A".into(),
            description: None,
            category: "feature".into(),
            steps: Some(vec!["step1".into()]),
            acceptance_criteria: Some(vec!["criterion1".into()]),
            suggested_priority: "high".into(),
            priority_score: 80,
            priority_reason: None,
            priority_factors: None,
            estimated_complexity: "moderate".into(),
            user_priority: None,
            user_modified: false,
            sort_order: 0,
        }],
        dependencies: vec![],
    }
}

// ─────────────────────────────────────────────────────────────
// Export tests
// ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn export_session_not_found_returns_error() {
    let app_state = AppState::new_sqlite_test();
    let service = make_service(&app_state);

    let result = service.export("nonexistent-session", "project-1").await;
    assert!(matches!(result, Err(AppError::NotFound(_))));
}

#[tokio::test]
async fn export_cross_project_rejection() {
    let app_state = AppState::new_sqlite_test();
    seed_project(&app_state, "project-1", "Project 1").await;
    seed_project(&app_state, "project-2", "Project 2").await;
    seed_session(&app_state, "session-1", "project-1", None).await;

    let service = make_service(&app_state);
    // Attempt to export session-1 as if it belongs to project-2
    let result = service.export("session-1", "project-2").await;
    assert!(matches!(result, Err(AppError::NotFound(_))));
}

#[tokio::test]
async fn export_session_no_plan_returns_empty_versions() {
    let app_state = AppState::new_sqlite_test();
    seed_project(&app_state, "project-1", "Project 1").await;
    seed_session(&app_state, "session-1", "project-1", None).await;

    let service = make_service(&app_state);
    let export = service.export("session-1", "project-1").await.unwrap();

    assert_eq!(export.plan_versions.len(), 0);
    assert_eq!(export.schema_version, 1);
}

#[tokio::test]
async fn export_uses_native_verification_shape_not_legacy_metadata_blob() {
    let app_state = AppState::new_sqlite_test();
    seed_project(&app_state, "project-1", "Project 1").await;
    seed_session(&app_state, "session-1", "project-1", None).await;

    app_state
        .db
        .run(move |conn| {
            conn.execute(
                "UPDATE ideation_sessions
                 SET verification_status = 'needs_revision',
                     verification_in_progress = 0,
                     verification_generation = 4,
                     verification_current_round = 2,
                     verification_max_rounds = 5,
                     verification_gap_count = 3,
                     verification_gap_score = 11,
                     verification_convergence_reason = 'critic_gap_found'
                 WHERE id = 'session-1'",
                [],
            )?;
            Ok(())
        })
        .await
        .unwrap();

    let service = make_service(&app_state);
    let export = service.export("session-1", "project-1").await.unwrap();
    let json = serde_json::to_value(&export).unwrap();

    assert!(json["session"]["verification"].is_object());
    assert_eq!(json["session"]["verification"]["status"], "needs_revision");
    assert_eq!(json["session"]["verification"]["generation"], 4);
    assert_eq!(json["session"]["verification"]["currentRound"], 2);
    assert_eq!(json["session"]["verification"]["gapCount"], 3);
    assert_eq!(json["session"]["verification"]["gapScore"], 11);
    assert_eq!(
        json["session"]["verification"]["convergenceReason"],
        "critic_gap_found"
    );
    assert!(
        json["session"].get("verificationMetadata").is_none(),
        "legacy verificationMetadata export field must be gone"
    );
}

#[tokio::test]
async fn export_session_no_proposals() {
    let app_state = AppState::new_sqlite_test();
    seed_project(&app_state, "project-1", "Project 1").await;
    seed_artifact(&app_state, "artifact-1", 1, "Plan v1", "# Content", None).await;
    seed_session(&app_state, "session-1", "project-1", Some("artifact-1")).await;

    let service = make_service(&app_state);
    let export = service.export("session-1", "project-1").await.unwrap();

    assert_eq!(export.plan_versions.len(), 1);
    assert_eq!(export.proposals.len(), 0);
    assert_eq!(export.dependencies.len(), 0);
}

#[tokio::test]
async fn export_single_version_chain() {
    let app_state = AppState::new_sqlite_test();
    seed_project(&app_state, "project-1", "Project 1").await;
    seed_artifact(&app_state, "artifact-1", 1, "Plan v1", "# Content v1", None).await;
    seed_session(&app_state, "session-1", "project-1", Some("artifact-1")).await;

    let service = make_service(&app_state);
    let export = service.export("session-1", "project-1").await.unwrap();

    assert_eq!(export.plan_versions.len(), 1);
    assert_eq!(export.plan_versions[0].version, 1);
    assert_eq!(export.plan_versions[0].content.as_deref(), Some("# Content v1"));
}

#[tokio::test]
async fn export_multi_version_chain_chronological_order() {
    let app_state = AppState::new_sqlite_test();
    seed_project(&app_state, "project-1", "Project 1").await;
    // Chain: artifact-1 → artifact-2 → artifact-3
    seed_artifact(&app_state, "artifact-1", 1, "Plan v1", "Content v1", None).await;
    seed_artifact(&app_state, "artifact-2", 2, "Plan v2", "Content v2", Some("artifact-1")).await;
    seed_artifact(&app_state, "artifact-3", 3, "Plan v3", "Content v3", Some("artifact-2")).await;
    // Session points to root (artifact-1)
    seed_session(&app_state, "session-1", "project-1", Some("artifact-1")).await;

    let service = make_service(&app_state);
    let export = service.export("session-1", "project-1").await.unwrap();

    assert_eq!(export.plan_versions.len(), 3);
    assert_eq!(export.plan_versions[0].version, 1);
    assert_eq!(export.plan_versions[1].version, 2);
    assert_eq!(export.plan_versions[2].version, 3);
}

#[tokio::test]
async fn export_parses_steps_and_acceptance_criteria() {
    let app_state = AppState::new_sqlite_test();
    seed_project(&app_state, "project-1", "Project 1").await;
    seed_session(&app_state, "session-1", "project-1", None).await;
    seed_proposal(&app_state, "proposal-1", "session-1", "Feature A", 0, None).await;

    let service = make_service(&app_state);
    let export = service.export("session-1", "project-1").await.unwrap();

    assert_eq!(export.proposals.len(), 1);
    let steps = export.proposals[0].steps.as_ref().unwrap();
    assert_eq!(steps, &["step1", "step2"]);
    let criteria = export.proposals[0].acceptance_criteria.as_ref().unwrap();
    assert_eq!(criteria, &["criterion1"]);
}

#[tokio::test]
async fn export_dependencies_converted_to_index_based() {
    let app_state = AppState::new_sqlite_test();
    seed_project(&app_state, "project-1", "Project 1").await;
    seed_session(&app_state, "session-1", "project-1", None).await;
    seed_proposal(&app_state, "proposal-0", "session-1", "Feature A", 0, None).await;
    seed_proposal(&app_state, "proposal-1", "session-1", "Feature B", 1, None).await;
    // proposal-1 depends on proposal-0
    seed_dependency(&app_state, "dep-1", "proposal-1", "proposal-0", "auto").await;

    let service = make_service(&app_state);
    let export = service.export("session-1", "project-1").await.unwrap();

    assert_eq!(export.proposals.len(), 2);
    assert_eq!(export.dependencies.len(), 1);
    let dep = &export.dependencies[0];
    // from_index=1 (proposal-1), to_index=0 (proposal-0)
    assert_eq!(dep.from_index, 1);
    assert_eq!(dep.to_index, 0);
    assert_eq!(dep.source, "auto");
}

#[tokio::test]
async fn export_dependency_source_preserved() {
    let app_state = AppState::new_sqlite_test();
    seed_project(&app_state, "project-1", "Project 1").await;
    seed_session(&app_state, "session-1", "project-1", None).await;
    seed_proposal(&app_state, "proposal-0", "session-1", "Feature A", 0, None).await;
    seed_proposal(&app_state, "proposal-1", "session-1", "Feature B", 1, None).await;
    seed_dependency(&app_state, "dep-1", "proposal-1", "proposal-0", "manual").await;

    let service = make_service(&app_state);
    let export = service.export("session-1", "project-1").await.unwrap();

    assert_eq!(export.dependencies[0].source, "manual");
}

#[tokio::test]
async fn export_followup_session_uses_inherited_plan_artifact_id() {
    let app_state = AppState::new_sqlite_test();
    seed_project(&app_state, "project-1", "Project 1").await;
    // Seed artifact chain belonging to a parent session
    seed_artifact(&app_state, "artifact-1", 1, "Plan v1", "# Inherited Content", None).await;
    seed_artifact(&app_state, "artifact-2", 2, "Plan v2", "# Inherited Content v2", Some("artifact-1")).await;
    // Followup session has no own plan_artifact_id, only inherited_plan_artifact_id
    seed_followup_session(&app_state, "followup-session", "project-1", "artifact-1").await;

    let service = make_service(&app_state);
    let export = service.export("followup-session", "project-1").await.unwrap();

    // Export should include plan versions from the inherited artifact chain
    assert_eq!(export.plan_versions.len(), 2);
    assert_eq!(export.plan_versions[0].content.as_deref(), Some("# Inherited Content"));
    assert_eq!(export.plan_versions[1].content.as_deref(), Some("# Inherited Content v2"));
}

#[tokio::test]
async fn export_own_plan_takes_precedence_over_inherited() {
    let app_state = AppState::new_sqlite_test();
    seed_project(&app_state, "project-1", "Project 1").await;
    // Own artifact
    seed_artifact(&app_state, "own-artifact", 1, "Own Plan", "# Own Content", None).await;
    // Inherited artifact (different chain)
    seed_artifact(&app_state, "inherited-artifact", 1, "Inherited Plan", "# Inherited Content", None).await;

    // Session with both plan_artifact_id and inherited_plan_artifact_id set
    app_state
        .db
        .run(|conn| {
            conn.execute(
                "INSERT INTO ideation_sessions \
                 (id, project_id, title, title_source, status, plan_artifact_id, \
                  inherited_plan_artifact_id, seed_task_id, parent_session_id, \
                  created_at, updated_at, archived_at, converted_at, team_mode, team_config_json) \
                 VALUES ('session-both', 'project-1', 'Session', 'user', 'active', \
                         'own-artifact', 'inherited-artifact', NULL, NULL, \
                         datetime('now'), datetime('now'), NULL, NULL, 'solo', NULL)",
                [],
            )?;
            Ok(())
        })
        .await
        .unwrap();

    let service = make_service(&app_state);
    let export = service.export("session-both", "project-1").await.unwrap();

    // Own plan takes precedence — should only see own artifact content
    assert_eq!(export.plan_versions.len(), 1);
    assert_eq!(export.plan_versions[0].content.as_deref(), Some("# Own Content"));
}

// ─────────────────────────────────────────────────────────────
// Import tests
// ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn import_file_too_large_returns_error() {
    let app_state = AppState::new_sqlite_test();
    let service = make_service(&app_state);

    // 10MB + 1 byte
    let oversized = "x".repeat(10_485_761);
    let result = service.import(&oversized, "project-1").await;
    assert!(matches!(result, Err(AppError::ImportInvalidFormat { .. })));
}

#[tokio::test]
async fn import_malformed_json_returns_error() {
    let app_state = AppState::new_sqlite_test();
    let service = make_service(&app_state);

    let result = service.import("not valid json {{", "project-1").await;
    assert!(matches!(result, Err(AppError::ImportInvalidFormat { .. })));
}

#[tokio::test]
async fn import_unsupported_schema_version_returns_error() {
    let app_state = AppState::new_sqlite_test();
    seed_project(&app_state, "project-1", "Project 1").await;
    let service = make_service(&app_state);

    let mut export = build_minimal_export();
    export.schema_version = 99;
    let json = serde_json::to_string(&export).unwrap();

    let result = service.import(&json, "project-1").await;
    assert!(matches!(
        result,
        Err(AppError::ImportVersionUnsupported { version: 99 })
    ));
}

#[tokio::test]
async fn import_out_of_bounds_dependency_index_returns_error() {
    let app_state = AppState::new_sqlite_test();
    seed_project(&app_state, "project-1", "Project 1").await;
    let service = make_service(&app_state);

    let mut export = build_minimal_export();
    // Only 1 proposal (index 0), but dep references index 5
    export.dependencies = vec![DependencyData {
        from_index: 0,
        to_index: 5,
        source: "auto".into(),
    }];
    let json = serde_json::to_string(&export).unwrap();

    let result = service.import(&json, "project-1").await;
    assert!(matches!(result, Err(AppError::ImportInvalidDependency { .. })));
}

#[tokio::test]
async fn import_cycle_detection_self_reference() {
    let app_state = AppState::new_sqlite_test();
    seed_project(&app_state, "project-1", "Project 1").await;
    let service = make_service(&app_state);

    let mut export = build_minimal_export();
    // Self-reference: proposal 0 depends on itself
    export.dependencies = vec![DependencyData {
        from_index: 0,
        to_index: 0,
        source: "auto".into(),
    }];
    let json = serde_json::to_string(&export).unwrap();

    let result = service.import(&json, "project-1").await;
    assert!(matches!(result, Err(AppError::ImportInvalidDependency { .. })));
}

#[tokio::test]
async fn import_cycle_detection_a_depends_b_depends_a() {
    let app_state = AppState::new_sqlite_test();
    seed_project(&app_state, "project-1", "Project 1").await;
    let service = make_service(&app_state);

    let mut export = build_minimal_export();
    // Add a second proposal
    export.proposals.push(ProposalData {
        index: 1,
        title: "Feature B".into(),
        description: None,
        category: "feature".into(),
        steps: None,
        acceptance_criteria: None,
        suggested_priority: "medium".into(),
        priority_score: 50,
        priority_reason: None,
        priority_factors: None,
        estimated_complexity: "simple".into(),
        user_priority: None,
        user_modified: false,
        sort_order: 1,
    });
    // A→B and B→A forms a cycle
    export.dependencies = vec![
        DependencyData { from_index: 0, to_index: 1, source: "auto".into() },
        DependencyData { from_index: 1, to_index: 0, source: "auto".into() },
    ];
    let json = serde_json::to_string(&export).unwrap();

    let result = service.import(&json, "project-1").await;
    assert!(matches!(result, Err(AppError::ImportInvalidDependency { .. })));
}

#[tokio::test]
async fn import_missing_project_returns_error() {
    let app_state = AppState::new_sqlite_test();
    let service = make_service(&app_state);

    let export = build_minimal_export();
    let json = serde_json::to_string(&export).unwrap();

    // Project "project-nonexistent" doesn't exist
    let result = service.import(&json, "project-nonexistent").await;
    assert!(matches!(result, Err(AppError::NotFound(_))));
}

#[tokio::test]
async fn import_basic_session_creates_session_row() {
    let app_state = AppState::new_sqlite_test();
    seed_project(&app_state, "project-1", "Project 1").await;
    let service = make_service(&app_state);

    let export = build_minimal_export();
    let json = serde_json::to_string(&export).unwrap();

    let result = service.import(&json, "project-1").await.unwrap();
    assert!(!result.session_id.is_empty());
    assert_eq!(result.title.as_deref(), Some("Imported Session"));
    assert_eq!(result.proposal_count, 1);
    assert_eq!(result.plan_version_count, 1);

    // Verify session was persisted
    let session_id = result.session_id.clone();
    let found: bool = app_state
        .db
        .run(move |conn| {
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM ideation_sessions WHERE id = ?1",
                [&session_id],
                |row| row.get(0),
            )?;
            Ok(count > 0)
        })
        .await
        .unwrap();
    assert!(found, "Imported session should exist in DB");
}

#[tokio::test]
async fn import_session_no_plan_versions() {
    let app_state = AppState::new_sqlite_test();
    seed_project(&app_state, "project-1", "Project 1").await;
    let service = make_service(&app_state);

    let mut export = build_minimal_export();
    export.plan_versions = vec![];
    export.proposals = vec![];
    let json = serde_json::to_string(&export).unwrap();

    let result = service.import(&json, "project-1").await.unwrap();
    assert_eq!(result.plan_version_count, 0);
    assert_eq!(result.proposal_count, 0);

    // plan_artifact_id should be NULL
    let session_id = result.session_id.clone();
    let plan_artifact_id: Option<String> = app_state
        .db
        .run(move |conn| {
            let val: Option<String> = conn.query_row(
                "SELECT plan_artifact_id FROM ideation_sessions WHERE id = ?1",
                [&session_id],
                |row| row.get(0),
            )?;
            Ok(val)
        })
        .await
        .unwrap();
    assert!(plan_artifact_id.is_none());
}

#[tokio::test]
async fn import_creates_artifact_chain() {
    let app_state = AppState::new_sqlite_test();
    seed_project(&app_state, "project-1", "Project 1").await;
    let service = make_service(&app_state);

    let mut export = build_minimal_export();
    export.plan_versions = vec![
        PlanVersionData {
            version: 1,
            name: "v1".into(),
            content: Some("Content v1".into()),
            content_type: "text/markdown".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            created_by: "user".into(),
        },
        PlanVersionData {
            version: 2,
            name: "v2".into(),
            content: Some("Content v2".into()),
            content_type: "text/markdown".into(),
            created_at: "2026-01-02T00:00:00Z".into(),
            created_by: "user".into(),
        },
    ];
    let json = serde_json::to_string(&export).unwrap();

    let result = service.import(&json, "project-1").await.unwrap();
    assert_eq!(result.plan_version_count, 2);

    // Verify artifact chain: first artifact has no prev, second has prev = first
    let session_id = result.session_id.clone();
    let (root_artifact_id, artifact_count): (String, i64) = app_state
        .db
        .run(move |conn| {
            let root: String = conn.query_row(
                "SELECT plan_artifact_id FROM ideation_sessions WHERE id = ?1",
                [&session_id],
                |row| row.get(0),
            )?;
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM artifacts WHERE previous_version_id IS NOT NULL \
                 OR id = (SELECT plan_artifact_id FROM ideation_sessions WHERE id = ?1)",
                [&session_id],
                |row| row.get(0),
            )?;
            Ok((root, count))
        })
        .await
        .unwrap();

    assert!(!root_artifact_id.is_empty());
    // At least the root artifact exists
    assert!(artifact_count >= 1);
}

#[tokio::test]
async fn import_proposals_get_pending_status() {
    let app_state = AppState::new_sqlite_test();
    seed_project(&app_state, "project-1", "Project 1").await;
    let service = make_service(&app_state);

    let export = build_minimal_export();
    let json = serde_json::to_string(&export).unwrap();

    let result = service.import(&json, "project-1").await.unwrap();
    let session_id = result.session_id.clone();

    let status: String = app_state
        .db
        .run(move |conn| {
            let s: String = conn.query_row(
                "SELECT status FROM task_proposals WHERE session_id = ?1",
                [&session_id],
                |row| row.get(0),
            )?;
            Ok(s)
        })
        .await
        .unwrap();

    assert_eq!(status, "pending");
}

#[tokio::test]
async fn import_dependencies_remapped_to_new_ids() {
    let app_state = AppState::new_sqlite_test();
    seed_project(&app_state, "project-1", "Project 1").await;
    let service = make_service(&app_state);

    let mut export = build_minimal_export();
    export.proposals.push(ProposalData {
        index: 1,
        title: "Feature B".into(),
        description: None,
        category: "feature".into(),
        steps: None,
        acceptance_criteria: None,
        suggested_priority: "medium".into(),
        priority_score: 50,
        priority_reason: None,
        priority_factors: None,
        estimated_complexity: "simple".into(),
        user_priority: None,
        user_modified: false,
        sort_order: 1,
    });
    // Feature B (index 1) depends on Feature A (index 0)
    export.dependencies = vec![DependencyData {
        from_index: 1,
        to_index: 0,
        source: "auto".into(),
    }];
    let json = serde_json::to_string(&export).unwrap();

    let result = service.import(&json, "project-1").await.unwrap();
    assert_eq!(result.proposal_count, 2);

    let session_id = result.session_id.clone();
    let dep_count: i64 = app_state
        .db
        .run(move |conn| {
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM proposal_dependencies pd \
                 JOIN task_proposals p ON p.id = pd.proposal_id \
                 WHERE p.session_id = ?1",
                [&session_id],
                |row| row.get(0),
            )?;
            Ok(count)
        })
        .await
        .unwrap();

    assert_eq!(dep_count, 1);
}

#[tokio::test]
async fn import_dependency_source_preserved() {
    let app_state = AppState::new_sqlite_test();
    seed_project(&app_state, "project-1", "Project 1").await;
    let service = make_service(&app_state);

    let mut export = build_minimal_export();
    export.proposals.push(ProposalData {
        index: 1,
        title: "Feature B".into(),
        description: None,
        category: "feature".into(),
        steps: None,
        acceptance_criteria: None,
        suggested_priority: "medium".into(),
        priority_score: 50,
        priority_reason: None,
        priority_factors: None,
        estimated_complexity: "simple".into(),
        user_priority: None,
        user_modified: false,
        sort_order: 1,
    });
    export.dependencies = vec![DependencyData {
        from_index: 1,
        to_index: 0,
        source: "manual".into(),
    }];
    let json = serde_json::to_string(&export).unwrap();

    let result = service.import(&json, "project-1").await.unwrap();
    let session_id = result.session_id.clone();

    let source: String = app_state
        .db
        .run(move |conn| {
            let s: String = conn.query_row(
                "SELECT pd.source FROM proposal_dependencies pd \
                 JOIN task_proposals p ON p.id = pd.proposal_id \
                 WHERE p.session_id = ?1",
                [&session_id],
                |row| row.get(0),
            )?;
            Ok(s)
        })
        .await
        .unwrap();

    assert_eq!(source, "manual");
}

// ─────────────────────────────────────────────────────────────
// Round-trip test
// ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn round_trip_export_import_re_export() {
    let app_state = AppState::new_sqlite_test();
    seed_project(&app_state, "project-1", "Project 1").await;

    // Seed source session
    seed_artifact(&app_state, "art-1", 1, "Plan v1", "# Plan", None).await;
    seed_artifact(&app_state, "art-2", 2, "Plan v2", "# Plan v2", Some("art-1")).await;
    seed_session(&app_state, "sess-1", "project-1", Some("art-1")).await;
    seed_proposal(&app_state, "prop-0", "sess-1", "Feature A", 0, Some("art-2")).await;
    seed_proposal(&app_state, "prop-1", "sess-1", "Feature B", 1, Some("art-2")).await;
    seed_dependency(&app_state, "dep-1", "prop-1", "prop-0", "auto").await;

    let service = make_service(&app_state);

    // Export
    let export = service.export("sess-1", "project-1").await.unwrap();
    assert_eq!(export.plan_versions.len(), 2);
    assert_eq!(export.proposals.len(), 2);
    assert_eq!(export.dependencies.len(), 1);

    // Import
    let json = serde_json::to_string(&export).unwrap();
    let imported = service.import(&json, "project-1").await.unwrap();
    assert_eq!(imported.proposal_count, 2);
    assert_eq!(imported.plan_version_count, 2);

    // Re-export the imported session
    let re_export = service
        .export(&imported.session_id, "project-1")
        .await
        .unwrap();

    // Verify structural equivalence
    assert_eq!(re_export.plan_versions.len(), export.plan_versions.len());
    assert_eq!(re_export.proposals.len(), export.proposals.len());
    assert_eq!(re_export.dependencies.len(), export.dependencies.len());
    assert_eq!(
        re_export.plan_versions[0].content,
        export.plan_versions[0].content
    );
    assert_eq!(
        re_export.plan_versions[1].content,
        export.plan_versions[1].content
    );
    // Dependency structure preserved
    assert_eq!(
        re_export.dependencies[0].from_index,
        export.dependencies[0].from_index
    );
    assert_eq!(
        re_export.dependencies[0].to_index,
        export.dependencies[0].to_index
    );
    assert_eq!(
        re_export.dependencies[0].source,
        export.dependencies[0].source
    );
}

#[tokio::test]
async fn import_unknown_team_mode_defaults_to_solo() {
    let app_state = AppState::new_sqlite_test();
    seed_project(&app_state, "project-1", "Project 1").await;
    let service = make_service(&app_state);

    let mut export = build_minimal_export();
    export.session.team_mode = "unknown_future_mode".into();
    let json = serde_json::to_string(&export).unwrap();

    let result = service.import(&json, "project-1").await.unwrap();
    let session_id = result.session_id.clone();

    let team_mode: String = app_state
        .db
        .run(move |conn| {
            let s: String = conn.query_row(
                "SELECT team_mode FROM ideation_sessions WHERE id = ?1",
                [&session_id],
                |row| row.get(0),
            )?;
            Ok(s)
        })
        .await
        .unwrap();

    assert_eq!(team_mode, "solo");
}

#[tokio::test]
async fn import_priority_factors_round_trip() {
    let app_state = AppState::new_sqlite_test();
    seed_project(&app_state, "project-1", "Project 1").await;
    let service = make_service(&app_state);

    let mut export = build_minimal_export();
    export.proposals[0].priority_factors = Some(PriorityFactorsData {
        dependency: 40,
        business_value: 90,
        technical_risk: 20,
        user_demand: 70,
    });
    let json = serde_json::to_string(&export).unwrap();

    let result = service.import(&json, "project-1").await.unwrap();
    let session_id = result.session_id.clone();

    // Re-export and check factors preserved
    let re_export = service.export(&session_id, "project-1").await.unwrap();
    let factors = re_export.proposals[0].priority_factors.as_ref().unwrap();
    assert_eq!(factors.dependency, 40);
    assert_eq!(factors.business_value, 90);
    assert_eq!(factors.technical_risk, 20);
    assert_eq!(factors.user_demand, 70);
}

#[tokio::test]
async fn import_steps_serialized_as_json_strings() {
    let app_state = AppState::new_sqlite_test();
    seed_project(&app_state, "project-1", "Project 1").await;
    let service = make_service(&app_state);

    let export = build_minimal_export();
    let json = serde_json::to_string(&export).unwrap();

    let result = service.import(&json, "project-1").await.unwrap();
    let session_id = result.session_id.clone();

    // Verify the raw steps column is a JSON string (not double-encoded)
    let steps_raw: String = app_state
        .db
        .run(move |conn| {
            let s: String = conn.query_row(
                "SELECT steps FROM task_proposals WHERE session_id = ?1",
                [&session_id],
                |row| row.get(0),
            )?;
            Ok(s)
        })
        .await
        .unwrap();

    // Should parse as a JSON array of strings
    let parsed: Vec<String> = serde_json::from_str(&steps_raw).unwrap();
    assert_eq!(parsed, vec!["step1"]);
}

#[tokio::test]
async fn export_session_with_null_team_mode_defaults_to_solo() {
    let app_state = AppState::new_sqlite_test();
    seed_project(&app_state, "project-1", "Project 1").await;

    // Insert session row with explicit NULL team_mode via raw SQL
    // (bypasses seed_session helper which always provides 'solo')
    // Mirrors seed_session column list for compatibility with all migrations
    app_state
        .db
        .run(|conn| {
            conn.execute(
                "INSERT INTO ideation_sessions \
                 (id, project_id, title, title_source, status, plan_artifact_id, \
                  inherited_plan_artifact_id, seed_task_id, parent_session_id, \
                  created_at, updated_at, archived_at, converted_at, \
                  team_mode, team_config_json) \
                 VALUES ('sess-null', 'project-1', 'Test Session', 'user', \
                         'active', NULL, NULL, NULL, NULL, \
                         datetime('now'), datetime('now'), NULL, NULL, \
                         NULL, NULL)",
                [],
            )?;
            Ok(())
        })
        .await
        .unwrap();

    let service = make_service(&app_state);
    let export = service.export("sess-null", "project-1").await.unwrap();
    assert_eq!(export.session.team_mode, "solo");
}
