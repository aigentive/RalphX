use std::path::PathBuf;
use std::sync::Arc;

use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;
use tauri::Emitter;

use crate::application::harness_runtime_registry::resolve_harness_agent_bootstrap;
use crate::application::AppState;
use crate::domain::agents::{AgentConfig, AgentRole, DEFAULT_AGENT_HARNESS};
use crate::domain::entities::{Artifact, ArtifactContent, IdeationSessionId};
use crate::error::{AppError, AppResult};
use crate::http_server::types::{
    PlanComplexityAssessmentResponse, SubmitPlanComplexityAssessmentRequest,
};
use crate::infrastructure::agents::claude::agent_names;

const ASSESSOR_SUBMIT_TOOL: &str = "submit_plan_complexity_assessment";
const ASSESSOR_TIMEOUT_SECS: u64 = 90;
const MAX_PLAN_CONTEXT_CHARS: usize = 32_000;
const MAX_REASON_SUMMARY_CHARS: usize = 500;

pub(crate) fn spawn_plan_complexity_assessor_after_approval(
    state: Arc<AppState>,
    session_id: String,
    artifact_id: String,
    artifact_version: u32,
) {
    tokio::spawn(async move {
        if let Err(error) =
            run_plan_complexity_assessor(&state, &session_id, &artifact_id, artifact_version).await
        {
            tracing::warn!(
                session_id,
                artifact_id,
                artifact_version,
                "Plan complexity assessor failed: {}",
                error
            );
        }
    });
}

async fn run_plan_complexity_assessor(
    state: &AppState,
    session_id: &str,
    artifact_id: &str,
    artifact_version: u32,
) -> AppResult<()> {
    let session_id_typed = IdeationSessionId::from_string(session_id.to_string());
    let session = state
        .ideation_session_repo
        .get_by_id(&session_id_typed)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Planning session not found: {session_id}")))?;

    if session.plan_artifact_id.as_ref().map(|id| id.as_str()) != Some(artifact_id) {
        return Err(AppError::Conflict(
            "Plan changed before complexity assessment could start".to_string(),
        ));
    }

    let artifact = state
        .artifact_repo
        .get_by_id(
            session
                .plan_artifact_id
                .as_ref()
                .expect("checked plan_artifact_id above"),
        )
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Plan artifact not found: {artifact_id}")))?;

    if artifact.metadata.version != artifact_version {
        return Err(AppError::Conflict(
            "Plan version changed before complexity assessment could start".to_string(),
        ));
    }

    let project = state
        .project_repo
        .get_by_id(&session.project_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Project not found: {}", session.project_id)))?;
    let runtime = state
        .resolve_plan_complexity_runtime_for_session(&session)
        .await?;
    let harness = runtime.harness.unwrap_or(DEFAULT_AGENT_HARNESS);
    let bootstrap = resolve_harness_agent_bootstrap(
        harness,
        agent_names::AGENT_PLAN_COMPLEXITY_ASSESSOR,
        PathBuf::from(&project.working_directory),
    );
    let prompt = build_plan_complexity_assessor_prompt(
        session_id,
        artifact_id,
        artifact_version,
        artifact.name.as_str(),
        &artifact,
    );
    let client = Arc::clone(&runtime.client);
    let handle = client
        .spawn_agent(AgentConfig {
            role: AgentRole::Custom(bootstrap.agent_role),
            prompt,
            working_directory: bootstrap.working_directory,
            plugin_dir: Some(bootstrap.plugin_dir),
            agent: Some(bootstrap.agent_name),
            model: runtime.model,
            harness: runtime.harness,
            logical_effort: runtime.logical_effort,
            approval_policy: runtime.approval_policy,
            sandbox_mode: runtime.sandbox_mode,
            max_tokens: None,
            timeout_secs: Some(ASSESSOR_TIMEOUT_SECS),
            env: bootstrap.env,
        })
        .await
        .map_err(|error| {
            AppError::Infrastructure(format!("failed to spawn plan complexity assessor: {error}"))
        })?;

    let output = client.wait_for_completion(&handle).await.map_err(|error| {
        AppError::Infrastructure(format!("plan complexity assessor failed: {error}"))
    })?;
    if !output.success {
        return Err(AppError::Infrastructure(format!(
            "plan complexity assessor exited unsuccessfully: {}",
            output.content.trim()
        )));
    }

    Ok(())
}

pub(crate) fn upsert_plan_complexity_assessment_sync(
    conn: &Connection,
    request: SubmitPlanComplexityAssessmentRequest,
    assessed_by: &str,
) -> AppResult<PlanComplexityAssessmentResponse> {
    validate_submit_request(&request)?;
    let plan = current_planning_plan_sync(conn, &request.session_id)?
        .ok_or_else(|| AppError::Validation("Planning session has no current plan".to_string()))?;

    if plan.artifact_id != request.artifact_id
        || plan.artifact_version != i64::from(request.artifact_version)
    {
        return Err(AppError::Conflict(
            "Plan changed before complexity assessment was submitted".to_string(),
        ));
    }
    if !plan.approved {
        return Err(AppError::Conflict(
            "Plan complexity assessment requires the current approved plan version".to_string(),
        ));
    }

    let now = chrono::Utc::now().to_rfc3339();
    let signals = request.signals.unwrap_or_else(|| serde_json::json!({}));
    let signals_json = serde_json::to_string(&signals)
        .map_err(|error| AppError::Validation(format!("Invalid signals payload: {error}")))?;
    let reason_summary = request.reason_summary.trim().to_string();
    let existing: Option<(String, String)> = conn
        .query_row(
            "SELECT id, created_at
             FROM plan_complexity_assessments
             WHERE session_id = ?1 AND artifact_id = ?2 AND artifact_version = ?3",
            params![
                request.session_id,
                request.artifact_id,
                i64::from(request.artifact_version),
            ],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let (id, created_at) =
        existing.unwrap_or_else(|| (uuid::Uuid::new_v4().to_string(), now.clone()));

    conn.execute(
        "INSERT INTO plan_complexity_assessments (
            id, session_id, artifact_id, artifact_version, level, score,
            recommended_action, confidence, reason_summary, signals_json,
            assessed_by, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
         ON CONFLICT(session_id, artifact_id, artifact_version) DO UPDATE SET
            level = excluded.level,
            score = excluded.score,
            recommended_action = excluded.recommended_action,
            confidence = excluded.confidence,
            reason_summary = excluded.reason_summary,
            signals_json = excluded.signals_json,
            assessed_by = excluded.assessed_by,
            updated_at = excluded.updated_at",
        params![
            id,
            request.session_id,
            request.artifact_id,
            i64::from(request.artifact_version),
            request.level,
            i64::from(request.score),
            request.recommended_action,
            request.confidence,
            reason_summary,
            signals_json,
            assessed_by,
            created_at,
            now,
        ],
    )?;

    get_plan_complexity_assessment_by_key_sync(
        conn,
        &request.session_id,
        &request.artifact_id,
        request.artifact_version,
    )?
    .ok_or_else(|| AppError::Database("Plan complexity assessment was not saved".to_string()))
}

pub(crate) fn get_current_plan_complexity_assessment_sync(
    conn: &Connection,
    session_id: &str,
) -> AppResult<Option<PlanComplexityAssessmentResponse>> {
    let Some(plan) = current_planning_plan_sync(conn, session_id)? else {
        return Ok(None);
    };

    get_plan_complexity_assessment_by_key_sync(
        conn,
        session_id,
        &plan.artifact_id,
        u32::try_from(plan.artifact_version)
            .map_err(|_| AppError::Database("Invalid artifact version".to_string()))?,
    )
}

pub(crate) fn emit_plan_complexity_assessed(
    state: &AppState,
    assessment: &PlanComplexityAssessmentResponse,
) {
    if let Some(app_handle) = &state.app_handle {
        let _ = app_handle.emit("plan_complexity:assessed", assessment);
    }
}

fn validate_submit_request(request: &SubmitPlanComplexityAssessmentRequest) -> AppResult<()> {
    if !matches!(
        request.level.as_str(),
        "trivial" | "simple" | "moderate" | "complex" | "very_complex"
    ) {
        return Err(AppError::Validation("Invalid complexity level".to_string()));
    }
    if !matches!(
        request.recommended_action.as_str(),
        "implement_directly" | "create_proposals"
    ) {
        return Err(AppError::Validation(
            "Invalid recommended action".to_string(),
        ));
    }
    if request.score > 100 {
        return Err(AppError::Validation(
            "Complexity score must be between 0 and 100".to_string(),
        ));
    }
    if !request.confidence.is_finite() || !(0.0..=1.0).contains(&request.confidence) {
        return Err(AppError::Validation(
            "Confidence must be between 0.0 and 1.0".to_string(),
        ));
    }
    let reason = request.reason_summary.trim();
    if reason.is_empty() {
        return Err(AppError::Validation(
            "Reason summary cannot be empty".to_string(),
        ));
    }
    if reason.chars().count() > MAX_REASON_SUMMARY_CHARS {
        return Err(AppError::Validation(format!(
            "Reason summary is too long; maximum is {MAX_REASON_SUMMARY_CHARS} characters"
        )));
    }
    Ok(())
}

struct CurrentPlanningPlan {
    artifact_id: String,
    artifact_version: i64,
    approved: bool,
}

fn current_planning_plan_sync(
    conn: &Connection,
    session_id: &str,
) -> AppResult<Option<CurrentPlanningPlan>> {
    let session_row: Option<(String, Option<String>)> = conn
        .query_row(
            "SELECT session_flow, plan_artifact_id
             FROM ideation_sessions
             WHERE id = ?1",
            [session_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;

    let Some((session_flow, plan_artifact_id)) = session_row else {
        return Err(AppError::NotFound(format!(
            "Planning session not found: {session_id}"
        )));
    };
    if session_flow != "planning" {
        return Err(AppError::Validation(
            "Plan complexity assessment is only available for planning sessions".to_string(),
        ));
    }
    let Some(artifact_id) = plan_artifact_id else {
        return Ok(None);
    };

    let artifact_version: i64 = conn.query_row(
        "SELECT version FROM artifacts WHERE id = ?1",
        [&artifact_id],
        |row| row.get(0),
    )?;
    let approved = conn
        .query_row(
            "SELECT 1
             FROM plan_artifact_approvals
             WHERE session_id = ?1
               AND artifact_id = ?2
               AND artifact_version = ?3
               AND status = 'approved'",
            params![session_id, artifact_id, artifact_version],
            |_| Ok(()),
        )
        .optional()?
        .is_some();

    Ok(Some(CurrentPlanningPlan {
        artifact_id,
        artifact_version,
        approved,
    }))
}

fn get_plan_complexity_assessment_by_key_sync(
    conn: &Connection,
    session_id: &str,
    artifact_id: &str,
    artifact_version: u32,
) -> AppResult<Option<PlanComplexityAssessmentResponse>> {
    let row = conn
        .query_row(
            "SELECT id, session_id, artifact_id, artifact_version, level, score,
                    recommended_action, confidence, reason_summary, signals_json,
                    assessed_by, created_at, updated_at
             FROM plan_complexity_assessments
             WHERE session_id = ?1 AND artifact_id = ?2 AND artifact_version = ?3",
            params![session_id, artifact_id, i64::from(artifact_version)],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, f64>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                    row.get::<_, String>(10)?,
                    row.get::<_, String>(11)?,
                    row.get::<_, String>(12)?,
                ))
            },
        )
        .optional()?;

    let Some(row) = row else {
        return Ok(None);
    };
    let signals = serde_json::from_str::<Value>(&row.9).unwrap_or_else(|_| serde_json::json!({}));
    Ok(Some(PlanComplexityAssessmentResponse {
        id: row.0,
        session_id: row.1,
        artifact_id: row.2,
        artifact_version: u32::try_from(row.3)
            .map_err(|_| AppError::Database("Invalid artifact version".to_string()))?,
        level: row.4,
        score: u8::try_from(row.5)
            .map_err(|_| AppError::Database("Invalid complexity score".to_string()))?,
        recommended_action: row.6,
        confidence: row.7,
        reason_summary: row.8,
        signals,
        assessed_by: row.10,
        created_at: row.11,
        updated_at: row.12,
    }))
}

fn build_plan_complexity_assessor_prompt(
    session_id: &str,
    artifact_id: &str,
    artifact_version: u32,
    artifact_name: &str,
    artifact: &Artifact,
) -> String {
    let content = match &artifact.content {
        ArtifactContent::Inline { text } => text.as_str(),
        ArtifactContent::File { path } => path.as_str(),
    };
    let plan_content = truncate_chars(content, MAX_PLAN_CONTEXT_CHARS);
    format!(
        "<task>\n\
         Grade the approved RalphX Plan-mode artifact complexity and call `{ASSESSOR_SUBMIT_TOOL}` exactly once.\n\
         </task>\n\
         <source_of_truth>\n\
         Use only the supplied plan artifact metadata and content. Do not inspect files, run commands, or infer repository state that is not in the plan.\n\
         </source_of_truth>\n\
         <decision_policy>\n\
         Recommend `implement_directly` for small, linear, low-risk plans that one general agent can execute from the plan.\n\
         Recommend `create_proposals` for plans with multiple dependent work items, cross-layer or cross-project scope, migrations/schema changes, high uncertainty, verification-heavy risk, or work that benefits from tracked task execution.\n\
         Still classify level independently as one of: trivial, simple, moderate, complex, very_complex.\n\
         Use score 0-100, where higher means more likely to need proposals/task execution.\n\
         </decision_policy>\n\
         <output_contract>\n\
         Call `{ASSESSOR_SUBMIT_TOOL}` with session_id, artifact_id, artifact_version, level, score, recommended_action, confidence, reason_summary, and a compact signals object.\n\
         Keep reason_summary under {MAX_REASON_SUMMARY_CHARS} characters.\n\
         </output_contract>\n\
         <plan_artifact session_id=\"{}\" artifact_id=\"{}\" artifact_version=\"{}\" title=\"{}\">\n\
         {}\n\
         </plan_artifact>",
        escape_xml_text(session_id),
        escape_xml_text(artifact_id),
        artifact_version,
        escape_xml_text(artifact_name),
        escape_xml_text(&plan_content)
    )
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

fn escape_xml_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::entities::ArtifactType;

    fn valid_request() -> SubmitPlanComplexityAssessmentRequest {
        SubmitPlanComplexityAssessmentRequest {
            session_id: "session-1".to_string(),
            artifact_id: "artifact-1".to_string(),
            artifact_version: 3,
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
                plan_artifact_id TEXT
            );
            CREATE TABLE artifacts (
                id TEXT PRIMARY KEY,
                version INTEGER NOT NULL
            );
            CREATE TABLE plan_artifact_approvals (
                session_id TEXT NOT NULL,
                artifact_id TEXT NOT NULL,
                artifact_version INTEGER NOT NULL,
                status TEXT NOT NULL,
                approved_at TEXT
            );
            CREATE TABLE plan_complexity_assessments (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                artifact_id TEXT NOT NULL,
                artifact_version INTEGER NOT NULL,
                level TEXT NOT NULL,
                score INTEGER NOT NULL,
                recommended_action TEXT NOT NULL,
                confidence REAL NOT NULL,
                reason_summary TEXT NOT NULL,
                signals_json TEXT NOT NULL,
                assessed_by TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                UNIQUE(session_id, artifact_id, artifact_version)
            );
            ",
        )
        .expect("create test tables");
        conn
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
    fn validate_submit_request_accepts_supported_payload() {
        validate_submit_request(&valid_request()).expect("valid assessment request");
    }

    #[test]
    fn validate_submit_request_rejects_invalid_fields() {
        let mut request = valid_request();
        request.level = "huge".to_string();
        assert!(matches!(
            validate_submit_request(&request),
            Err(AppError::Validation(message)) if message == "Invalid complexity level"
        ));

        let mut request = valid_request();
        request.recommended_action = "delegate".to_string();
        assert!(matches!(
            validate_submit_request(&request),
            Err(AppError::Validation(message)) if message == "Invalid recommended action"
        ));

        let mut request = valid_request();
        request.score = 101;
        assert!(matches!(
            validate_submit_request(&request),
            Err(AppError::Validation(message))
                if message == "Complexity score must be between 0 and 100"
        ));

        let mut request = valid_request();
        request.confidence = f64::NAN;
        assert!(matches!(
            validate_submit_request(&request),
            Err(AppError::Validation(message)) if message == "Confidence must be between 0.0 and 1.0"
        ));

        let mut request = valid_request();
        request.reason_summary = " ".to_string();
        assert!(matches!(
            validate_submit_request(&request),
            Err(AppError::Validation(message)) if message == "Reason summary cannot be empty"
        ));

        let mut request = valid_request();
        request.reason_summary = "a".repeat(MAX_REASON_SUMMARY_CHARS + 1);
        assert!(matches!(
            validate_submit_request(&request),
            Err(AppError::Validation(message)) if message.contains("Reason summary is too long")
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
    fn current_plan_handles_missing_plan_and_invalid_session_flow() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO ideation_sessions (id, session_flow, plan_artifact_id)
             VALUES (?1, ?2, NULL)",
            params!["session-1", "planning"],
        )
        .expect("insert session");
        assert!(current_planning_plan_sync(&conn, "session-1")
            .expect("missing plan is valid")
            .is_none());

        let conn = setup_db();
        seed_current_plan(&conn, "ideation", Some("approved"));
        assert!(matches!(
            current_planning_plan_sync(&conn, "session-1"),
            Err(AppError::Validation(message))
                if message == "Plan complexity assessment is only available for planning sessions"
        ));

        assert!(matches!(
            current_planning_plan_sync(&conn, "missing-session"),
            Err(AppError::NotFound(message)) if message.contains("missing-session")
        ));
    }

    #[test]
    fn assessment_reader_defaults_invalid_signals_to_empty_object() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO plan_complexity_assessments (
                id, session_id, artifact_id, artifact_version, level, score,
                recommended_action, confidence, reason_summary, signals_json,
                assessed_by, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                "assessment-1",
                "session-1",
                "artifact-1",
                3_i64,
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
    fn prompt_uses_supplied_artifact_content_and_escapes_xml() {
        let mut artifact = Artifact::new_inline(
            "Plan <Alpha>",
            ArtifactType::Specification,
            "Use A&B < C > D",
            "planner",
        );
        artifact.id = crate::domain::entities::ArtifactId::from_string("artifact-1");
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
}
