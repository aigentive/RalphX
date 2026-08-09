use std::path::PathBuf;
use std::sync::Arc;

use ralphx_events::emit_serialized;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;
use tauri::{AppHandle, Manager};

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

pub(crate) fn spawn_plan_complexity_assessor_from_app_handle(
    app_handle: AppHandle,
    session_id: String,
    artifact_id: String,
    artifact_version: u32,
) {
    tokio::spawn(async move {
        let state = app_handle.state::<AppState>();
        if let Err(error) =
            run_plan_complexity_assessor(&state, &session_id, &artifact_id, artifact_version).await
        {
            tracing::warn!(
                session_id,
                artifact_id,
                artifact_version,
                "Re-enabled plan complexity assessor failed: {}",
                error
            );
        }
    });
}

pub(crate) fn list_missing_plan_complexity_assessments_sync(
    conn: &Connection,
    limit: usize,
) -> AppResult<Vec<(String, String, u32)>> {
    let limit = i64::try_from(limit)
        .map_err(|_| AppError::Validation("Assessment reconciliation limit is too large".into()))?;
    let mut statement = conn.prepare(
        "SELECT session.id, artifact.id, artifact.version
         FROM ideation_sessions session
         INNER JOIN artifacts artifact ON artifact.id = session.plan_artifact_id
         INNER JOIN plan_artifact_approvals approval
            ON approval.session_id = session.id
           AND approval.artifact_id = artifact.id
           AND approval.artifact_version = artifact.version
           AND approval.blueprint_artifact_id IS session.plan_blueprint_artifact_id
           AND approval.status = 'approved'
         INNER JOIN agent_conversation_workspaces workspace
            ON workspace.linked_ideation_session_id = session.id
           AND workspace.status = 'active'
         LEFT JOIN plan_complexity_assessments assessment
            ON assessment.session_id = session.id
           AND assessment.artifact_id = artifact.id
           AND assessment.artifact_version = artifact.version
           AND assessment.blueprint_artifact_id IS session.plan_blueprint_artifact_id
         WHERE session.session_flow = 'planning'
           AND session.status = 'active'
           AND workspace.task_pipeline_session_id IS NULL
           AND assessment.id IS NULL
         GROUP BY session.id, artifact.id, artifact.version, approval.approved_at
         ORDER BY approval.approved_at ASC
         LIMIT ?1",
    )?;
    let rows = statement
        .query_map([limit], |row| {
            let version = row.get::<_, i64>(2)?;
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, version))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|(session_id, artifact_id, version)| {
            let version = u32::try_from(version)
                .map_err(|_| AppError::Database("Invalid artifact version".to_string()))?;
            Ok((session_id, artifact_id, version))
        })
        .collect()
}

async fn run_plan_complexity_assessor(
    state: &AppState,
    session_id: &str,
    artifact_id: &str,
    artifact_version: u32,
) -> AppResult<()> {
    let tasks_enabled = state
        .ideation_settings_repo
        .get_settings()
        .await
        .map_err(|error| AppError::Database(error.to_string()))?
        .tasks_enabled;
    if !tasks_enabled {
        return Err(AppError::FeatureDisabled(
            crate::application::tasks_feature_policy::TASKS_DISABLED_MESSAGE.to_string(),
        ));
    }
    let session_id_typed = IdeationSessionId::from_string(session_id.to_string());
    let session = state
        .ideation_session_repo
        .get_by_id(&session_id_typed)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Planning session not found: {session_id}")))?;

    let bundle = session.plan_artifact_bundle().ok_or_else(|| {
        AppError::Conflict("Plan bundle is incomplete before complexity assessment".to_string())
    })?;
    if bundle.overview_id.as_str() != artifact_id {
        return Err(AppError::Conflict(
            "Plan changed before complexity assessment could start".to_string(),
        ));
    }

    let artifact = state
        .artifact_repo
        .get_by_id(&bundle.overview_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Plan artifact not found: {artifact_id}")))?;

    if artifact.metadata.version != artifact_version {
        return Err(AppError::Conflict(
            "Plan version changed before complexity assessment could start".to_string(),
        ));
    }
    let blueprint = if let Some(blueprint_id) = bundle.blueprint_id.as_ref() {
        Some(
            state
                .artifact_repo
                .get_by_id(blueprint_id)
                .await?
                .ok_or_else(|| {
                    AppError::NotFound(format!(
                        "Plan blueprint not found: {}",
                        blueprint_id.as_str()
                    ))
                })?,
        )
    } else {
        None
    };

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
            None,
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
    let prompt = build_plan_complexity_assessor_bundle_prompt(
        session_id,
        artifact_id,
        artifact_version,
        artifact.name.as_str(),
        &artifact,
        blueprint.as_ref(),
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
            mcp_launch_policy: Default::default(),
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
    if !crate::infrastructure::sqlite::sqlite_ideation_settings_repo::get_settings_sync(conn)?
        .tasks_enabled
    {
        return Err(AppError::FeatureDisabled(
            crate::application::tasks_feature_policy::TASKS_DISABLED_MESSAGE.to_string(),
        ));
    }
    let plan = current_planning_plan_sync(conn, &request.session_id)?
        .ok_or_else(|| AppError::Validation("Planning session has no current plan".to_string()))?;

    if plan.artifact_id != request.artifact_id
        || plan.artifact_version != i64::from(request.artifact_version)
        || plan.blueprint_artifact_id != request.blueprint_artifact_id
        || plan.blueprint_artifact_version != request.blueprint_artifact_version.map(i64::from)
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
             WHERE session_id = ?1 AND artifact_id = ?2 AND artifact_version = ?3
               AND blueprint_artifact_id IS ?4
               AND blueprint_artifact_version IS ?5",
            params![
                request.session_id,
                request.artifact_id,
                i64::from(request.artifact_version),
                request.blueprint_artifact_id,
                request.blueprint_artifact_version.map(i64::from),
            ],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let (id, created_at) =
        existing.unwrap_or_else(|| (uuid::Uuid::new_v4().to_string(), now.clone()));

    conn.execute(
        "INSERT INTO plan_complexity_assessments (
            id, session_id, artifact_id, artifact_version,
            blueprint_artifact_id, blueprint_artifact_version, level, score,
            recommended_action, confidence, reason_summary, signals_json,
            assessed_by, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
         ON CONFLICT(id) DO UPDATE SET
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
            request.blueprint_artifact_id,
            request.blueprint_artifact_version.map(i64::from),
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

    get_plan_complexity_assessment_by_bundle_key_sync(
        conn,
        &request.session_id,
        &request.artifact_id,
        request.artifact_version,
        request.blueprint_artifact_id.as_deref(),
        request.blueprint_artifact_version,
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

    get_plan_complexity_assessment_by_bundle_key_sync(
        conn,
        session_id,
        &plan.artifact_id,
        u32::try_from(plan.artifact_version)
            .map_err(|_| AppError::Database("Invalid artifact version".to_string()))?,
        plan.blueprint_artifact_id.as_deref(),
        plan.blueprint_artifact_version
            .map(u32::try_from)
            .transpose()
            .map_err(|_| AppError::Database("Invalid blueprint version".to_string()))?,
    )
}

pub(crate) fn emit_plan_complexity_assessed(
    state: &AppState,
    assessment: &PlanComplexityAssessmentResponse,
) {
    let _ = emit_serialized(
        state.events.as_ref(),
        "plan_complexity:assessed",
        assessment,
    );
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
    blueprint_artifact_id: Option<String>,
    blueprint_artifact_version: Option<i64>,
    approved: bool,
}

fn current_planning_plan_sync(
    conn: &Connection,
    session_id: &str,
) -> AppResult<Option<CurrentPlanningPlan>> {
    let session_row: Option<(String, Option<String>, Option<String>, i64)> = conn
        .query_row(
            "SELECT session_flow, plan_artifact_id, plan_blueprint_artifact_id,
                    plan_contract_version
             FROM ideation_sessions
             WHERE id = ?1",
            [session_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;

    let Some((session_flow, plan_artifact_id, blueprint_artifact_id, contract_version)) =
        session_row
    else {
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
    if contract_version >= 2 && blueprint_artifact_id.is_none() {
        return Ok(None);
    }
    let blueprint_artifact_version = blueprint_artifact_id
        .as_ref()
        .map(|id| {
            conn.query_row("SELECT version FROM artifacts WHERE id = ?1", [id], |row| {
                row.get(0)
            })
        })
        .transpose()?;
    let approved = conn
        .query_row(
            "SELECT 1
             FROM plan_artifact_approvals
             WHERE session_id = ?1
               AND artifact_id = ?2
               AND artifact_version = ?3
               AND blueprint_artifact_id IS ?4
               AND blueprint_artifact_version IS ?5
               AND status = 'approved'",
            params![
                session_id,
                artifact_id,
                artifact_version,
                blueprint_artifact_id,
                blueprint_artifact_version
            ],
            |_| Ok(()),
        )
        .optional()?
        .is_some();

    Ok(Some(CurrentPlanningPlan {
        artifact_id,
        artifact_version,
        blueprint_artifact_id,
        blueprint_artifact_version,
        approved,
    }))
}

#[cfg(test)]
pub(crate) fn get_plan_complexity_assessment_by_key_sync(
    conn: &Connection,
    session_id: &str,
    artifact_id: &str,
    artifact_version: u32,
) -> AppResult<Option<PlanComplexityAssessmentResponse>> {
    get_plan_complexity_assessment_by_bundle_key_sync(
        conn,
        session_id,
        artifact_id,
        artifact_version,
        None,
        None,
    )
}

fn get_plan_complexity_assessment_by_bundle_key_sync(
    conn: &Connection,
    session_id: &str,
    artifact_id: &str,
    artifact_version: u32,
    blueprint_artifact_id: Option<&str>,
    blueprint_artifact_version: Option<u32>,
) -> AppResult<Option<PlanComplexityAssessmentResponse>> {
    let row = conn
        .query_row(
            "SELECT id, session_id, artifact_id, artifact_version,
                    blueprint_artifact_id, blueprint_artifact_version, level, score,
                    recommended_action, confidence, reason_summary, signals_json,
                    assessed_by, created_at, updated_at
             FROM plan_complexity_assessments
             WHERE session_id = ?1 AND artifact_id = ?2 AND artifact_version = ?3
               AND blueprint_artifact_id IS ?4
               AND blueprint_artifact_version IS ?5",
            params![
                session_id,
                artifact_id,
                i64::from(artifact_version),
                blueprint_artifact_id,
                blueprint_artifact_version.map(i64::from)
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<i64>>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, f64>(9)?,
                    row.get::<_, String>(10)?,
                    row.get::<_, String>(11)?,
                    row.get::<_, String>(12)?,
                    row.get::<_, String>(13)?,
                    row.get::<_, String>(14)?,
                ))
            },
        )
        .optional()?;

    let Some(row) = row else {
        return Ok(None);
    };
    let signals = serde_json::from_str::<Value>(&row.11).unwrap_or_else(|_| serde_json::json!({}));
    Ok(Some(PlanComplexityAssessmentResponse {
        id: row.0,
        session_id: row.1,
        artifact_id: row.2,
        artifact_version: u32::try_from(row.3)
            .map_err(|_| AppError::Database("Invalid artifact version".to_string()))?,
        blueprint_artifact_id: row.4,
        blueprint_artifact_version: row
            .5
            .map(u32::try_from)
            .transpose()
            .map_err(|_| AppError::Database("Invalid blueprint version".to_string()))?,
        level: row.6,
        score: u8::try_from(row.7)
            .map_err(|_| AppError::Database("Invalid complexity score".to_string()))?,
        recommended_action: row.8,
        confidence: row.9,
        reason_summary: row.10,
        signals,
        assessed_by: row.12,
        created_at: row.13,
        updated_at: row.14,
    }))
}

#[cfg(test)]
pub(crate) fn build_plan_complexity_assessor_prompt(
    session_id: &str,
    artifact_id: &str,
    artifact_version: u32,
    artifact_name: &str,
    artifact: &Artifact,
) -> String {
    build_plan_complexity_assessor_bundle_prompt(
        session_id,
        artifact_id,
        artifact_version,
        artifact_name,
        artifact,
        None,
    )
}

fn build_plan_complexity_assessor_bundle_prompt(
    session_id: &str,
    artifact_id: &str,
    artifact_version: u32,
    artifact_name: &str,
    artifact: &Artifact,
    blueprint: Option<&Artifact>,
) -> String {
    let content = match &artifact.content {
        ArtifactContent::Inline { text } => text.as_str(),
        ArtifactContent::File { path } => path.as_str(),
    };
    let plan_content = truncate_chars(content, MAX_PLAN_CONTEXT_CHARS);
    let blueprint_section = blueprint
        .map(|blueprint| {
            let content = match &blueprint.content {
                ArtifactContent::Inline { text } => text.as_str(),
                ArtifactContent::File { path } => path.as_str(),
            };
            format!(
                "\n<implementation_blueprint artifact_id=\"{}\" artifact_version=\"{}\" title=\"{}\">\n{}\n</implementation_blueprint>",
                escape_xml_text(blueprint.id.as_str()),
                blueprint.metadata.version,
                escape_xml_text(&blueprint.name),
                escape_xml_text(&truncate_chars(content, MAX_PLAN_CONTEXT_CHARS))
            )
        })
        .unwrap_or_default();
    format!(
        "<task>\n\
         Grade the approved RalphX Plan Overview and Implementation Blueprint bundle complexity and call `{ASSESSOR_SUBMIT_TOOL}` exactly once.\n\
         </task>\n\
         <source_of_truth>\n\
         Use only the supplied plan bundle metadata and content. Do not inspect files, run commands, or infer repository state that is not in the bundle.\n\
         </source_of_truth>\n\
         <decision_policy>\n\
         Recommend `implement_directly` for small, linear, low-risk plans that one general agent can execute from the plan.\n\
         Recommend `create_proposals` for plans with multiple dependent work items, cross-layer or cross-project scope, migrations/schema changes, high uncertainty, verification-heavy risk, or work that benefits from tracked task execution.\n\
         Still classify level independently as one of: trivial, simple, moderate, complex, very_complex.\n\
         Use score 0-100, where higher means more likely to need proposals/task execution.\n\
         </decision_policy>\n\
         <output_contract>\n\
         Call `{ASSESSOR_SUBMIT_TOOL}` with session_id, artifact_id, artifact_version, blueprint_artifact_id, blueprint_artifact_version, level, score, recommended_action, confidence, reason_summary, and a compact signals object. Omit blueprint fields only for a legacy overview-only plan.\n\
         Keep reason_summary under {MAX_REASON_SUMMARY_CHARS} characters.\n\
         </output_contract>\n\
         <plan_artifact session_id=\"{}\" artifact_id=\"{}\" artifact_version=\"{}\" title=\"{}\">\n\
         {}\n\
         </plan_artifact>{}",
        escape_xml_text(session_id),
        escape_xml_text(artifact_id),
        artifact_version,
        escape_xml_text(artifact_name),
        escape_xml_text(&plan_content),
        blueprint_section,
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
