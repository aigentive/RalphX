use chrono::Utc;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::application::data_retention_service::{
    DataRetentionService, RetentionCycleGuard, RetentionCycleReport,
};
use crate::domain::entities::data_retention::{DataRetentionPolicyUpdate, DataRetentionSettings};
use crate::infrastructure::sqlite::sqlite_chat_payload_retention_repo::SizeBudgetPreview;
use crate::AppState;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DataRetentionSettingsResponse {
    pub settings: DataRetentionSettings,
    /// Prefills the Settings control. Never an active cap — see the size-budget opt-in.
    pub recommended_size_budget_bytes: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDataRetentionSettingsInput {
    pub enabled: bool,
    pub days: u64,
    pub archived_days: u64,
    pub batch_rows: u64,
    pub size_budget_bytes: Option<u64>,
    /// The user's explicit consent for size-based deletion. The confirmation *timestamp*
    /// is stamped server-side from this flag — no caller may supply one.
    pub size_budget_confirmed: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewDataRetentionSizeBudgetInput {
    pub budget_bytes: u64,
}

#[tauri::command]
pub async fn get_data_retention_settings(
    state: State<'_, AppState>,
) -> Result<DataRetentionSettingsResponse, String> {
    let service = DataRetentionService::from_db(state.db.clone());
    let settings = service
        .settings()
        .await
        .map_err(|error| error.to_string())?;
    Ok(DataRetentionSettingsResponse {
        settings,
        recommended_size_budget_bytes: crate::infrastructure::agents::claude::stream_timeouts()
            .chat_payload_size_budget_recommended_bytes,
    })
}

#[tauri::command]
pub async fn update_data_retention_settings(
    input: UpdateDataRetentionSettingsInput,
    state: State<'_, AppState>,
) -> Result<DataRetentionSettingsResponse, String> {
    let service = DataRetentionService::from_db(state.db.clone());
    let settings = service
        .update_policy(policy_update_from(input))
        .await
        .map_err(|error| error.to_string())?;
    Ok(DataRetentionSettingsResponse {
        settings,
        recommended_size_budget_bytes: crate::infrastructure::agents::claude::stream_timeouts()
            .chat_payload_size_budget_recommended_bytes,
    })
}

#[tauri::command]
pub async fn run_data_retention_now(
    state: State<'_, AppState>,
) -> Result<RetentionCycleReport, String> {
    DataRetentionService::from_db(state.db.clone())
        .run_cycle(Utc::now())
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn preview_data_retention_size_budget(
    input: PreviewDataRetentionSizeBudgetInput,
    state: State<'_, AppState>,
) -> Result<SizeBudgetPreview, String> {
    // Writes nothing, but carries a full-scan cost, so it takes the same cycle slot.
    let _guard = RetentionCycleGuard::try_acquire()
        .ok_or("A data retention cleanup is already running.".to_string())?;
    DataRetentionService::from_db(state.db.clone())
        .preview_size_budget(input.budget_bytes)
        .await
        .map_err(|error| error.to_string())
}

/// Stamps consent server-side: `size_budget_confirmed` is a boolean intent, never a time.
pub(crate) fn policy_update_from(
    input: UpdateDataRetentionSettingsInput,
) -> DataRetentionPolicyUpdate {
    let size_budget_confirmed_at = match (input.size_budget_bytes, input.size_budget_confirmed) {
        (Some(_), true) => Some(Utc::now()),
        _ => None,
    };
    DataRetentionPolicyUpdate {
        enabled: input.enabled,
        days: input.days,
        archived_days: input.archived_days,
        batch_rows: input.batch_rows,
        size_budget_bytes: input.size_budget_bytes,
        size_budget_confirmed_at,
    }
}
