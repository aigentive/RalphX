use super::*;

#[derive(Debug, Deserialize)]
pub struct RegisterWebhookRequest {
    pub url: String,
    #[serde(default)]
    pub event_types: Option<Vec<String>>,
    #[serde(default)]
    pub project_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct RegisterWebhookResponse {
    pub id: String,
    pub url: String,
    pub secret: String,
    pub event_types: Option<Vec<String>>,
    pub project_ids: Vec<String>,
    pub active: bool,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
pub struct WebhookSummary {
    pub id: String,
    pub url: String,
    pub event_types: Option<Vec<String>>,
    pub project_ids: Vec<String>,
    pub active: bool,
    pub failure_count: i64,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
pub struct ListWebhooksResponse {
    pub webhooks: Vec<WebhookSummary>,
}

#[derive(Debug, Serialize)]
pub struct UnregisterWebhookResponse {
    pub success: bool,
    pub id: String,
}

/// POST /api/external/webhooks/register — register a webhook URL
pub async fn register_webhook_http(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
    headers: axum::http::HeaderMap,
    Json(req): Json<RegisterWebhookRequest>,
) -> Result<Json<RegisterWebhookResponse>, HttpError> {
    // Extract the API key ID from the X-RalphX-Key-Id header (injected by external MCP server)
    let api_key_id = headers
        .get(crate::http_server::handlers::external_auth::EXTERNAL_KEY_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_default();

    // Extract authorized project IDs from scope (empty means unrestricted)
    let authorized_project_ids: Vec<String> = scope
        .0
        .as_deref()
        .map(|ids| ids.iter().map(|id| id.to_string()).collect())
        .unwrap_or_default();

    let svc = crate::application::WebhookService::new(Arc::clone(
        &state.app_state.webhook_registration_repo,
    ));

    let registration = svc
        .register(
            &api_key_id,
            &req.url,
            req.event_types,
            req.project_ids,
            &authorized_project_ids,
        )
        .await
        .map_err(|e| {
            error!("Failed to register webhook: {}", e);
            HttpError {
                status: axum::http::StatusCode::UNPROCESSABLE_ENTITY,
                message: Some(e.to_string()),
            }
        })?;

    // Invalidate publisher DashMap cache for affected projects so the next publish()
    // call re-queries the repo and picks up the refreshed project_ids.
    if let Some(publisher) = &state.app_state.webhook_publisher {
        let project_ids: Vec<String> =
            serde_json::from_str(&registration.project_ids).unwrap_or_default();
        for pid in &project_ids {
            publisher.invalidate_project(pid);
        }
    }

    let event_types: Option<Vec<String>> = registration
        .event_types
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok());
    let project_ids: Vec<String> =
        serde_json::from_str(&registration.project_ids).unwrap_or_default();

    Ok(Json(RegisterWebhookResponse {
        id: registration.id,
        url: registration.url,
        secret: registration.secret,
        event_types,
        project_ids,
        active: registration.active,
        created_at: registration.created_at,
    }))
}

/// DELETE /api/external/webhooks/:id — unregister a webhook
pub async fn unregister_webhook_http(
    State(state): State<HttpServerState>,
    headers: axum::http::HeaderMap,
    Path(webhook_id): Path<String>,
) -> Result<Json<UnregisterWebhookResponse>, HttpError> {
    let api_key_id = headers
        .get(crate::http_server::handlers::external_auth::EXTERNAL_KEY_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_default();

    let svc = crate::application::WebhookService::new(Arc::clone(
        &state.app_state.webhook_registration_repo,
    ));

    let found = svc
        .unregister(&webhook_id, &api_key_id)
        .await
        .map_err(|e| {
            error!("Failed to unregister webhook: {}", e);
            HttpError {
                status: axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                message: Some(e.to_string()),
            }
        })?;

    if !found {
        return Err(HttpError {
            status: axum::http::StatusCode::NOT_FOUND,
            message: Some("Webhook not found or not owned by this API key".to_string()),
        });
    }

    Ok(Json(UnregisterWebhookResponse {
        success: true,
        id: webhook_id,
    }))
}

/// GET /api/external/webhooks — list webhooks for this API key
pub async fn list_webhooks_http(
    State(state): State<HttpServerState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<ListWebhooksResponse>, HttpError> {
    let api_key_id = headers
        .get(crate::http_server::handlers::external_auth::EXTERNAL_KEY_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_default();

    let svc = crate::application::WebhookService::new(Arc::clone(
        &state.app_state.webhook_registration_repo,
    ));

    let registrations = svc.list(&api_key_id).await.map_err(|e| {
        error!("Failed to list webhooks: {}", e);
        HttpError {
            status: axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            message: Some(e.to_string()),
        }
    })?;

    let webhooks = registrations
        .into_iter()
        .map(|r| {
            let event_types: Option<Vec<String>> = r
                .event_types
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok());
            let project_ids: Vec<String> = serde_json::from_str(&r.project_ids).unwrap_or_default();
            WebhookSummary {
                id: r.id,
                url: r.url,
                event_types,
                project_ids,
                active: r.active,
                failure_count: r.failure_count,
                created_at: r.created_at,
            }
        })
        .collect();

    Ok(Json(ListWebhooksResponse { webhooks }))
}

/// GET /api/external/webhooks/health — delivery health stats per webhook
pub async fn get_webhook_health_http(
    State(state): State<HttpServerState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<WebhookHealthResponse>, HttpError> {
    let api_key_id = headers
        .get(crate::http_server::handlers::external_auth::EXTERNAL_KEY_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_default();

    let svc = crate::application::WebhookService::new(Arc::clone(
        &state.app_state.webhook_registration_repo,
    ));

    let registrations = svc.list(&api_key_id).await.map_err(|e| {
        error!("Failed to get webhook health: {}", e);
        HttpError {
            status: axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            message: Some(e.to_string()),
        }
    })?;

    let webhooks = registrations
        .into_iter()
        .map(|r| WebhookHealthItem {
            id: r.id,
            url: r.url,
            active: r.active,
            failure_count: r.failure_count,
            last_failure_at: r.last_failure_at,
        })
        .collect();

    Ok(Json(WebhookHealthResponse { webhooks }))
}

#[derive(Debug, Serialize)]
pub struct WebhookHealthItem {
    pub id: String,
    pub url: String,
    pub active: bool,
    pub failure_count: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_failure_at: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct WebhookHealthResponse {
    pub webhooks: Vec<WebhookHealthItem>,
}
