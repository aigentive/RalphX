use tauri::State;

use crate::domain::entities::app_state::UpdateChannel;
use crate::AppState;

#[tauri::command]
pub async fn get_update_channel(state: State<'_, AppState>) -> Result<UpdateChannel, String> {
    state
        .app_state_repo
        .get()
        .await
        .map(|settings| settings.update_channel)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn set_update_channel(
    update_channel: UpdateChannel,
    state: State<'_, AppState>,
) -> Result<UpdateChannel, String> {
    state
        .app_state_repo
        .set_update_channel(update_channel)
        .await
        .map_err(|error| error.to_string())?;
    Ok(update_channel)
}
