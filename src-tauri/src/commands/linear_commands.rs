use serde::{Deserialize, Serialize};
use tauri::State;

use crate::application::AppState;
use crate::domain::services::SecretStore;
use crate::infrastructure::secret_store::MacosKeychainSecretStore;
use crate::infrastructure::sqlite::SqliteLinearWebhookStore;

const LINEAR_WEBHOOK_SIGNING_SECRET_REF: &str =
    "integrations/linear/default/webhook-signing-secret";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinearWebhookConfigResponse {
    pub enabled: bool,
    pub has_signing_secret: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveLinearWebhookSigningSecretInput {
    pub signing_secret: String,
    #[serde(default)]
    pub enabled: Option<bool>,
}

#[tauri::command]
pub async fn get_linear_webhook_config(
    state: State<'_, AppState>,
) -> Result<LinearWebhookConfigResponse, String> {
    let store = SqliteLinearWebhookStore::new(state.db.clone());
    let (enabled, signing_secret_ref) = store
        .get_config()
        .await
        .map_err(|error| error.to_string())?;

    Ok(LinearWebhookConfigResponse {
        enabled,
        has_signing_secret: signing_secret_ref.is_some(),
    })
}

#[tauri::command]
pub async fn save_linear_webhook_signing_secret(
    input: SaveLinearWebhookSigningSecretInput,
    state: State<'_, AppState>,
) -> Result<LinearWebhookConfigResponse, String> {
    let signing_secret = input.signing_secret.trim();
    if signing_secret.is_empty() {
        return Err("Linear webhook signing secret cannot be empty".to_string());
    }

    MacosKeychainSecretStore::new()
        .put_secret(LINEAR_WEBHOOK_SIGNING_SECRET_REF, signing_secret)
        .await
        .map_err(|error| error.to_string())?;

    let enabled = input.enabled.unwrap_or(true);
    let store = SqliteLinearWebhookStore::new(state.db.clone());
    store
        .set_signing_secret_ref(Some(LINEAR_WEBHOOK_SIGNING_SECRET_REF.to_string()), enabled)
        .await
        .map_err(|error| error.to_string())?;

    Ok(LinearWebhookConfigResponse {
        enabled,
        has_signing_secret: true,
    })
}
