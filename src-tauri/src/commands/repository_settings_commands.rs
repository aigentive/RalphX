use serde::{Deserialize, Serialize};
use tauri::State;

use crate::infrastructure::subprocess_env_policy;
use crate::AppState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RepositorySettingsResponse {
    pub remove_inherited_github_cli_tokens: bool,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRepositorySettingsInput {
    pub remove_inherited_github_cli_tokens: bool,
}

#[tauri::command]
pub async fn get_repository_settings(
    state: State<'_, AppState>,
) -> Result<RepositorySettingsResponse, String> {
    let settings = state
        .app_state_repo
        .get()
        .await
        .map_err(|error| error.to_string())?;
    Ok(RepositorySettingsResponse {
        remove_inherited_github_cli_tokens: settings.remove_inherited_github_cli_tokens,
    })
}

#[tauri::command]
pub async fn update_repository_settings(
    input: UpdateRepositorySettingsInput,
    state: State<'_, AppState>,
) -> Result<RepositorySettingsResponse, String> {
    state
        .app_state_repo
        .set_remove_inherited_github_cli_tokens(input.remove_inherited_github_cli_tokens)
        .await
        .map_err(|error| error.to_string())?;
    subprocess_env_policy::set_remove_inherited_github_cli_tokens(
        input.remove_inherited_github_cli_tokens,
    );

    Ok(RepositorySettingsResponse {
        remove_inherited_github_cli_tokens: input.remove_inherited_github_cli_tokens,
    })
}
