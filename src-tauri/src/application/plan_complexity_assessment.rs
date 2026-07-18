use std::path::PathBuf;
use std::sync::Arc;

use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;
use tauri::Emitter;

use crate::application::harness_runtime_registry::resolve_harness_agent_bootstrap;
use crate::application::AppState;
use crate::domain::agents::{AgentConfig, AgentRole, RoutingRole, DEFAULT_AGENT_HARNESS};
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
        .resolve_manual_role_background_agent_runtime(
            Some(project.id.as_str()),
            Some(std::path::Path::new(&project.working_directory)),
            RoutingRole::UtilityLightweight,
            agent_names::AGENT_PLAN_COMPLEXITY_ASSESSOR,
            "plan complexity assessor",
            None,
        )
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
    let env = runtime.env_with_overrides(bootstrap.env);
    let handle = client
        .spawn_agent(AgentConfig {
            role: AgentRole::Custom(bootstrap.agent_role),
            prompt,
            working_directory: bootstrap.working_directory,
            plugin_dir: Some(bootstrap.plugin_dir),
            agent: Some(bootstrap.agent_name),
            model: runtime.model,
            harness: runtime.harness,
            cli_path_override: runtime.cli_path_override,
            logical_effort: runtime.logical_effort,
            approval_policy: runtime.approval_policy,
            sandbox_mode: runtime.sandbox_mode,
            service_tier: runtime.service_tier,
            max_tokens: None,
            timeout_secs: Some(ASSESSOR_TIMEOUT_SECS),
            env,
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

pub(crate) fn get_plan_complexity_assessment_by_key_sync(
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

pub(crate) fn build_plan_complexity_assessor_prompt(
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

pub(crate) fn truncate_chars(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

pub(crate) fn escape_xml_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
