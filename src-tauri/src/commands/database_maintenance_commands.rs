use serde::{Deserialize, Serialize};
use tauri::State;

use crate::infrastructure::sqlite::database_maintenance::{
    read_stats_at, set_pending_compaction_at,
};
use crate::infrastructure::sqlite::database_maintenance_outcome::CompactionRecord;
use crate::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct DatabaseMaintenanceStatsResponse {
    pub database_bytes: u64,
    pub reclaimable_bytes: u64,
    pub headroom_ok: bool,
    pub pending_compaction: bool,
    /// Outcome of the last compaction attempt, including a skip reason. Serialized
    /// snake_case to match the rest of this response, not the camelCase retention API.
    pub last_compaction: Option<CompactionRecord>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetDatabaseCompactionPendingInput {
    pub pending: bool,
}

#[tauri::command]
pub async fn get_database_maintenance_stats(
    state: State<'_, AppState>,
) -> Result<DatabaseMaintenanceStatsResponse, String> {
    let paths = state
        .app_paths
        .database_maintenance_paths()
        .map_err(|error| error.to_string())?;
    let stats = read_stats_at(&paths).map_err(|error| error.to_string())?;
    Ok(DatabaseMaintenanceStatsResponse {
        database_bytes: stats.database_bytes,
        reclaimable_bytes: stats.reclaimable_bytes,
        headroom_ok: stats.headroom_ok,
        pending_compaction: stats.pending_compaction,
        last_compaction: stats.last_compaction,
    })
}

#[tauri::command]
pub async fn set_database_compaction_pending(
    input: SetDatabaseCompactionPendingInput,
    state: State<'_, AppState>,
) -> Result<(), String> {
    set_pending_compaction_at(
        &state.app_paths.database_compaction_marker_path(),
        input.pending,
    )
    .map_err(|error| error.to_string())
}
