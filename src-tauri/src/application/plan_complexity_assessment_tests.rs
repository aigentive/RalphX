use super::plan_complexity_assessment::{
    build_plan_complexity_assessor_prompt, get_current_plan_complexity_assessment_sync,
    get_plan_complexity_assessment_by_key_sync, list_missing_plan_complexity_assessments_sync,
    truncate_chars, upsert_plan_complexity_assessment_sync,
};
use crate::domain::entities::{Artifact, ArtifactId, ArtifactType};
use crate::error::AppError;
use crate::http_server::types::SubmitPlanComplexityAssessmentRequest;
use rusqlite::{params, Connection};
use serde_json::Value;

fn valid_request() -> SubmitPlanComplexityAssessmentRequest {
    SubmitPlanComplexityAssessmentRequest {
        session_id: "session-1".to_string(),
        artifact_id: "artifact-1".to_string(),
        artifact_version: 3,
        blueprint_artifact_id: None,
        blueprint_artifact_version: None,
        level: "moderate".to_string(),
        score: 58,
        recommended_action: "create_proposals".to_string(),
        confidence: 0.82,
        reason_summary: "  Cross-layer plan with review risk.  ".to_string(),
        signals: Some(serde_json::json!({
            "fanout": 2,
            "requires_schema_change": false,
        })),
    }
}

fn setup_db() -> Connection {
    let conn = Connection::open_in_memory().expect("open in-memory db");
    conn.execute_batch(
        "
        CREATE TABLE ideation_sessions (
            id TEXT PRIMARY KEY,
            session_flow TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'active',
            plan_artifact_id TEXT,
            plan_blueprint_artifact_id TEXT,
            plan_contract_version INTEGER NOT NULL DEFAULT 1
        );
        CREATE TABLE artifacts (
            id TEXT PRIMARY KEY,
            version INTEGER NOT NULL
        );
        CREATE TABLE plan_artifact_approvals (
            session_id TEXT NOT NULL,
            artifact_id TEXT NOT NULL,
            artifact_version INTEGER NOT NULL,
            blueprint_artifact_id TEXT,
            blueprint_artifact_version INTEGER,
            status TEXT NOT NULL,
            approved_at TEXT
        );
        CREATE TABLE plan_complexity_assessments (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            artifact_id TEXT NOT NULL,
            artifact_version INTEGER NOT NULL,
            blueprint_artifact_id TEXT,
            blueprint_artifact_version INTEGER,
            level TEXT NOT NULL,
            score INTEGER NOT NULL,
            recommended_action TEXT NOT NULL,
            confidence REAL NOT NULL,
            reason_summary TEXT NOT NULL,
            signals_json TEXT NOT NULL,
            assessed_by TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            UNIQUE(session_id, artifact_id, artifact_version, blueprint_artifact_id, blueprint_artifact_version)
        );
        CREATE TABLE ideation_settings (
            id INTEGER PRIMARY KEY,
            plan_mode TEXT NOT NULL DEFAULT 'optional',
            require_plan_approval INTEGER NOT NULL DEFAULT 1,
            suggest_plans_for_complex INTEGER NOT NULL DEFAULT 1,
            auto_link_proposals INTEGER NOT NULL DEFAULT 1,
            require_verification_for_accept INTEGER NOT NULL DEFAULT 0,
            require_verification_for_proposals INTEGER NOT NULL DEFAULT 0,
            require_accept_for_finalize INTEGER,
            ext_require_verification_for_accept INTEGER,
            ext_require_verification_for_proposals INTEGER,
            ext_require_accept_for_finalize INTEGER,
            auto_verify_plans INTEGER NOT NULL DEFAULT 0,
            auto_verify_draft_plans INTEGER NOT NULL DEFAULT 1,
            ext_auto_verify_plans INTEGER,
            tasks_enabled INTEGER NOT NULL DEFAULT 1,
            tasks_feature_state TEXT NOT NULL DEFAULT 'enabled'
        );
        INSERT INTO ideation_settings (id, tasks_enabled) VALUES (1, 1);
        CREATE TABLE agent_conversation_workspaces (
            conversation_id TEXT PRIMARY KEY,
            linked_ideation_session_id TEXT,
            task_pipeline_session_id TEXT,
            status TEXT NOT NULL
        );
        ",
    )
    .expect("create test tables");
    conn
}

#[test]
fn missing_assessment_reconciliation_only_selects_active_direct_plan_workspaces() {
    let conn = setup_db();
    seed_current_plan(&conn, "planning", Some("approved"));
    conn.execute(
        "INSERT INTO agent_conversation_workspaces (
            conversation_id, linked_ideation_session_id, task_pipeline_session_id, status
         ) VALUES ('conversation-1', 'session-1', NULL, 'active')",
        [],
    )
    .unwrap();

    let pending = list_missing_plan_complexity_assessments_sync(&conn, 8).unwrap();
    assert_eq!(
        pending,
        vec![("session-1".to_string(), "artifact-1".to_string(), 3)]
    );

    conn.execute(
        "UPDATE agent_conversation_workspaces
         SET task_pipeline_session_id = 'session-1'
         WHERE conversation_id = 'conversation-1'",
        [],
    )
    .unwrap();
    assert!(list_missing_plan_complexity_assessments_sync(&conn, 8)
        .unwrap()
        .is_empty());
}

#[test]
fn assessment_submission_rejects_stale_output_while_tasks_are_disabled() {
    let conn = setup_db();
    seed_current_plan(&conn, "planning", Some("approved"));
    conn.execute(
        "UPDATE ideation_settings
         SET tasks_enabled = 0, tasks_feature_state = 'disabled'
         WHERE id = 1",
        [],
    )
    .unwrap();

    let error = upsert_plan_complexity_assessment_sync(&conn, valid_request(), "assessor")
        .expect_err("stale assessor output must be rejected while Tasks are off");
    assert!(matches!(
        error,
        AppError::FeatureDisabled(message) if message.starts_with("ralphx:tasks_disabled")
    ));
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM plan_complexity_assessments",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
}

fn seed_current_plan(conn: &Connection, session_flow: &str, approval_status: Option<&str>) {
    conn.execute(
        "INSERT INTO ideation_sessions (id, session_flow, plan_artifact_id)
         VALUES (?1, ?2, ?3)",
        params!["session-1", session_flow, "artifact-1"],
    )
    .expect("insert session");
    conn.execute(
        "INSERT INTO artifacts (id, version) VALUES (?1, ?2)",
        params!["artifact-1", 3_i64],
    )
    .expect("insert artifact");
    if let Some(status) = approval_status {
        conn.execute(
            "INSERT INTO plan_artifact_approvals (
                session_id, artifact_id, artifact_version, status, approved_at
             ) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                "session-1",
                "artifact-1",
                3_i64,
                status,
                "2026-06-12T00:00:00Z"
            ],
        )
        .expect("insert approval");
    }
}

#[test]
fn submit_request_validation_rejects_invalid_fields_before_db_access() {
    let conn = Connection::open_in_memory().expect("open in-memory db");

    let mut request = valid_request();
    request.level = "huge".to_string();
    assert!(matches!(
        upsert_plan_complexity_assessment_sync(&conn, request, "assessor"),
        Err(AppError::Validation(message)) if message == "Invalid complexity level"
    ));

    let mut request = valid_request();
    request.recommended_action = "delegate".to_string();
    assert!(matches!(
        upsert_plan_complexity_assessment_sync(&conn, request, "assessor"),
        Err(AppError::Validation(message)) if message == "Invalid recommended action"
    ));

    let mut request = valid_request();
    request.score = 101;
    assert!(matches!(
        upsert_plan_complexity_assessment_sync(&conn, request, "assessor"),
        Err(AppError::Validation(message))
            if message == "Complexity score must be between 0 and 100"
    ));

    let mut request = valid_request();
    request.confidence = f64::NAN;
    assert!(matches!(
        upsert_plan_complexity_assessment_sync(&conn, request, "assessor"),
        Err(AppError::Validation(message)) if message == "Confidence must be between 0.0 and 1.0"
    ));

    let mut request = valid_request();
    request.reason_summary = " ".to_string();
    assert!(matches!(
        upsert_plan_complexity_assessment_sync(&conn, request, "assessor"),
        Err(AppError::Validation(message)) if message == "Reason summary cannot be empty"
    ));
}

#[test]
fn upsert_persists_and_updates_current_approved_plan() {
    let conn = setup_db();
    seed_current_plan(&conn, "planning", Some("approved"));

    let created = upsert_plan_complexity_assessment_sync(&conn, valid_request(), "assessor-a")
        .expect("create assessment");
    assert_eq!(created.session_id, "session-1");
    assert_eq!(created.artifact_id, "artifact-1");
    assert_eq!(created.artifact_version, 3);
    assert_eq!(created.score, 58);
    assert_eq!(created.reason_summary, "Cross-layer plan with review risk.");
    assert_eq!(created.assessed_by, "assessor-a");
    assert_eq!(
        created.signals.get("fanout").and_then(Value::as_i64),
        Some(2)
    );

    let mut updated_request = valid_request();
    updated_request.level = "simple".to_string();
    updated_request.score = 18;
    updated_request.recommended_action = "implement_directly".to_string();
    updated_request.signals = None;
    let updated = upsert_plan_complexity_assessment_sync(&conn, updated_request, "assessor-b")
        .expect("update assessment");

    assert_eq!(updated.id, created.id);
    assert_eq!(updated.created_at, created.created_at);
    assert_eq!(updated.level, "simple");
    assert_eq!(updated.score, 18);
    assert_eq!(updated.recommended_action, "implement_directly");
    assert_eq!(updated.assessed_by, "assessor-b");
    assert_eq!(updated.signals, serde_json::json!({}));

    let current = get_current_plan_complexity_assessment_sync(&conn, "session-1")
        .expect("load current assessment")
        .expect("assessment exists");
    assert_eq!(current.id, updated.id);
}

#[test]
fn upsert_requires_current_approved_plan_version() {
    let conn = setup_db();
    seed_current_plan(&conn, "planning", None);
    assert!(matches!(
        upsert_plan_complexity_assessment_sync(&conn, valid_request(), "assessor"),
        Err(AppError::Conflict(message))
            if message == "Plan complexity assessment requires the current approved plan version"
    ));

    let conn = setup_db();
    seed_current_plan(&conn, "planning", Some("approved"));
    let mut stale = valid_request();
    stale.artifact_version = 2;
    assert!(matches!(
        upsert_plan_complexity_assessment_sync(&conn, stale, "assessor"),
        Err(AppError::Conflict(message))
            if message == "Plan changed before complexity assessment was submitted"
    ));
}

#[test]
fn current_assessment_handles_missing_plan_invalid_flow_and_missing_session() {
    let conn = setup_db();
    conn.execute(
        "INSERT INTO ideation_sessions (id, session_flow, plan_artifact_id)
         VALUES (?1, ?2, NULL)",
        params!["session-1", "planning"],
    )
    .expect("insert session");
    assert!(
        get_current_plan_complexity_assessment_sync(&conn, "session-1")
            .expect("missing plan is valid")
            .is_none()
    );

    let conn = setup_db();
    seed_current_plan(&conn, "ideation", Some("approved"));
    assert!(matches!(
        get_current_plan_complexity_assessment_sync(&conn, "session-1"),
        Err(AppError::Validation(message))
            if message == "Plan complexity assessment is only available for planning sessions"
    ));

    assert!(matches!(
        get_current_plan_complexity_assessment_sync(&conn, "missing-session"),
        Err(AppError::NotFound(message)) if message.contains("missing-session")
    ));
}

#[test]
fn assessment_reader_defaults_invalid_signals_to_empty_object() {
    let conn = setup_db();
    conn.execute(
        "INSERT INTO plan_complexity_assessments (
            id, session_id, artifact_id, artifact_version,
            blueprint_artifact_id, blueprint_artifact_version, level, score,
            recommended_action, confidence, reason_summary, signals_json,
            assessed_by, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        params![
            "assessment-1",
            "session-1",
            "artifact-1",
            3_i64,
            Option::<String>::None,
            Option::<i64>::None,
            "complex",
            77_i64,
            "create_proposals",
            0.7_f64,
            "Needs coordination",
            "{not-json",
            "assessor",
            "2026-06-12T00:00:00Z",
            "2026-06-12T00:01:00Z"
        ],
    )
    .expect("insert assessment");

    let assessment =
        get_plan_complexity_assessment_by_key_sync(&conn, "session-1", "artifact-1", 3)
            .expect("read assessment")
            .expect("assessment exists");
    assert_eq!(assessment.signals, serde_json::json!({}));
}

#[test]
fn assessor_prompt_uses_supplied_artifact_content_and_escapes_xml() {
    let mut artifact = Artifact::new_inline(
        "Plan <Alpha>",
        ArtifactType::Specification,
        "Use A&B < C > D",
        "planner",
    );
    artifact.id = ArtifactId::from_string("artifact-1");
    artifact.metadata.version = 3;

    let prompt = build_plan_complexity_assessor_prompt(
        "session<&>",
        "artifact-1",
        3,
        "Plan <Alpha>",
        &artifact,
    );

    assert!(prompt.contains("session_id=\"session&lt;&amp;&gt;\""));
    assert!(prompt.contains("title=\"Plan &lt;Alpha&gt;\""));
    assert!(prompt.contains("Use A&amp;B &lt; C &gt; D"));

    let file_artifact = Artifact::new_file(
        "Plan File",
        ArtifactType::Specification,
        "/tmp/plan.md",
        "planner",
    );
    let file_prompt = build_plan_complexity_assessor_prompt(
        "session-1",
        "artifact-1",
        1,
        "Plan File",
        &file_artifact,
    );
    assert!(file_prompt.contains("/tmp/plan.md"));
    assert_eq!(truncate_chars("åßc", 2), "åß");
}
