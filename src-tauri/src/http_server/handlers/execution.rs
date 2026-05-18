use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};

use super::*;

/// Response for global execution settings HTTP endpoint
#[derive(Debug, Serialize)]
pub struct GlobalSettingsHttpResponse {
    pub global_max_concurrent: u32,
    pub global_ideation_max: u32,
    pub allow_ideation_borrow_idle_execution: bool,
}

/// Request to update global execution settings
#[derive(Debug, Deserialize)]
pub struct UpdateGlobalSettingsRequest {
    pub global_max_concurrent: u32,
    pub global_ideation_max: u32,
    pub allow_ideation_borrow_idle_execution: bool,
}

/// GET /api/execution/global-settings
/// Returns the global max concurrent cap across all projects
pub async fn get_global_settings(
    State(state): State<HttpServerState>,
) -> Result<Json<GlobalSettingsHttpResponse>, StatusCode> {
    let settings = state
        .app_state
        .global_execution_settings_repo
        .get_settings()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(GlobalSettingsHttpResponse {
        global_max_concurrent: settings.global_max_concurrent,
        global_ideation_max: settings.global_ideation_max,
        allow_ideation_borrow_idle_execution: settings.allow_ideation_borrow_idle_execution,
    }))
}

/// POST /api/execution/global-settings
/// Updates the global max concurrent cap (clamped to [1, 50])
pub async fn update_global_settings(
    State(state): State<HttpServerState>,
    Json(req): Json<UpdateGlobalSettingsRequest>,
) -> Result<Json<GlobalSettingsHttpResponse>, StatusCode> {
    use crate::domain::execution::GlobalExecutionSettings;

    let settings = GlobalExecutionSettings {
        global_max_concurrent: req.global_max_concurrent,
        global_ideation_max: req.global_ideation_max,
        allow_ideation_borrow_idle_execution: req.allow_ideation_borrow_idle_execution,
    };

    let updated = state
        .app_state
        .global_execution_settings_repo
        .update_settings(&settings)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Sync in-memory execution state
    state
        .execution_state
        .set_global_max_concurrent(updated.global_max_concurrent);
    state
        .execution_state
        .set_global_ideation_max(updated.global_ideation_max);
    state
        .execution_state
        .set_allow_ideation_borrow_idle_execution(updated.allow_ideation_borrow_idle_execution);

    Ok(Json(GlobalSettingsHttpResponse {
        global_max_concurrent: updated.global_max_concurrent,
        global_ideation_max: updated.global_ideation_max,
        allow_ideation_borrow_idle_execution: updated.allow_ideation_borrow_idle_execution,
    }))
}
