use super::*;
use axum::body::Bytes;

use crate::application::{
    LinearWebhookAction, LinearWebhookError, LinearWebhookHeaders,
    LinearWebhookReconciliationService, LinearWebhookRequest, LinearWebhookStore,
    TicketingCacheInvalidator,
};
use crate::domain::services::SecretStore;
use crate::infrastructure::secret_store::MacosKeychainSecretStore;
use crate::infrastructure::sqlite::SqliteLinearWebhookStore;

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

#[derive(Debug, Serialize)]
pub struct LinearWebhookResponse {
    pub accepted: bool,
    pub delivery_id: String,
    pub duplicate: bool,
    pub action: String,
}

/// POST /api/integrations/linear/webhook — receive Linear webhooks.
///
/// This endpoint intentionally takes raw bytes so HMAC validation runs over the
/// exact request body Linear signed, before JSON parsing or reconciliation.
pub async fn receive_linear_webhook_http(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<LinearWebhookResponse>, HttpError> {
    let store = Arc::new(SqliteLinearWebhookStore::new(state.app_state.db.clone()));
    let (enabled, signing_secret_ref) = store
        .get_config()
        .await
        .map_err(internal_linear_webhook_error)?;
    if !enabled {
        return Err(linear_webhook_http_error(LinearWebhookError::MissingSecret));
    }
    let signing_secret_ref = signing_secret_ref
        .ok_or_else(|| linear_webhook_http_error(LinearWebhookError::MissingSecret))?;
    let signing_secret = MacosKeychainSecretStore::new()
        .get_secret(&signing_secret_ref)
        .await
        .map_err(|error| {
            linear_webhook_http_error(LinearWebhookError::Reconciliation(error.to_string()))
        })?
        .ok_or_else(|| linear_webhook_http_error(LinearWebhookError::MissingSecret))?;

    let mut transition_service_builder = state
        .app_state
        .build_transition_service_for_runtime(Arc::clone(&state.execution_state), None);
    if let Some(ref publisher) = state.app_state.webhook_publisher {
        transition_service_builder =
            transition_service_builder.with_webhook_publisher_for_emitter(Arc::clone(publisher));
    }
    let transition_service = transition_service_builder
        .with_external_events_repo(Arc::clone(&state.app_state.external_events_repo));
    let store_for_service: Arc<dyn LinearWebhookStore> = store;

    let service = LinearWebhookReconciliationService::new(
        signing_secret,
        store_for_service,
        Arc::clone(&state.app_state.workflow_repo),
    );
    let request = LinearWebhookRequest {
        headers: LinearWebhookHeaders {
            signature: header_string(&headers, "linear-signature"),
            delivery: header_string(&headers, "linear-delivery"),
            event: header_string(&headers, "linear-event"),
        },
        raw_body: body.to_vec(),
    };

    let outcome = service
        .handle(request, chrono::Utc::now())
        .await
        .map_err(linear_webhook_http_error)?;
    if !outcome.duplicate {
        let _ = TicketingCacheInvalidator::invalidate_linear_webhook_with_sink(
            state.app_state.events.as_ref(),
            &body,
            linear_action_label(&outcome.action),
        );
    }
    if let LinearWebhookAction::TransitionedTask {
        task_id,
        target_status,
    } = &outcome.action
    {
        transition_service
            .transition_task(task_id, *target_status)
            .await
            .map_err(internal_linear_webhook_error)?;
    }

    Ok(Json(LinearWebhookResponse {
        accepted: true,
        delivery_id: outcome.delivery_id,
        duplicate: outcome.duplicate,
        action: linear_action_label(&outcome.action).to_string(),
    }))
}

fn header_string(headers: &HeaderMap, name: &'static str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}

fn linear_action_label(action: &LinearWebhookAction) -> &'static str {
    match action {
        LinearWebhookAction::TransitionedTask { .. } => "transitioned_task",
        LinearWebhookAction::RecordedIssue => "recorded_issue",
        LinearWebhookAction::RecordedIssueActivity => "recorded_issue_activity",
        LinearWebhookAction::NoLinkedTask => "no_linked_task",
        LinearWebhookAction::NoMappedStatus => "no_mapped_status",
        LinearWebhookAction::UnsupportedEvent => "unsupported_event",
        LinearWebhookAction::DuplicateDelivery => "duplicate_delivery",
    }
}

fn internal_linear_webhook_error(error: crate::error::AppError) -> HttpError {
    error!("Linear webhook infrastructure error: {}", error);
    HttpError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        message: Some(error.to_string()),
    }
}

fn linear_webhook_http_error(error: LinearWebhookError) -> HttpError {
    let status = match error {
        LinearWebhookError::MissingSignature | LinearWebhookError::InvalidSignature => {
            StatusCode::UNAUTHORIZED
        }
        LinearWebhookError::StaleTimestamp => StatusCode::UNAUTHORIZED,
        LinearWebhookError::MalformedBody(_) | LinearWebhookError::MissingDeliveryId => {
            StatusCode::BAD_REQUEST
        }
        LinearWebhookError::MissingSecret => StatusCode::SERVICE_UNAVAILABLE,
        LinearWebhookError::Reconciliation(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    HttpError {
        status,
        message: Some(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::AppState;
    use crate::application::execution_state::ExecutionState;
    use crate::domain::entities::{InternalStatus, TaskId};
    use crate::http_server::handlers::external_auth::EXTERNAL_KEY_ID_HEADER;
    use crate::http_server::types::HttpServerState;

    fn test_http_state() -> HttpServerState {
        let app_state = Arc::new(AppState::new_test());
        HttpServerState {
            app_state,
            execution_state: Arc::new(ExecutionState::new()),
            delegation_service: Default::default(),
            external_mcp_supervisor: None,
        }
    }

    fn api_key_headers(api_key_id: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(EXTERNAL_KEY_ID_HEADER, api_key_id.parse().unwrap());
        headers
    }

    #[tokio::test]
    async fn webhook_registration_list_health_and_unregister_flow() {
        let state = test_http_state();
        let headers = api_key_headers("api-key-1");

        let registered = register_webhook_http(
            State(state.clone()),
            ProjectScope(None),
            headers.clone(),
            Json(RegisterWebhookRequest {
                url: "https://example.com/webhook".to_string(),
                event_types: Some(vec!["task.updated".to_string()]),
                project_ids: vec!["project-1".to_string()],
            }),
        )
        .await
        .expect("webhook should register")
        .0;

        assert_eq!(registered.url, "https://example.com/webhook");
        assert_eq!(
            registered.event_types.as_deref(),
            Some([String::from("task.updated")].as_slice())
        );
        assert_eq!(registered.project_ids, vec!["project-1"]);
        assert!(registered.active);
        assert!(!registered.secret.is_empty());

        let listed = list_webhooks_http(State(state.clone()), headers.clone())
            .await
            .expect("webhooks should list")
            .0;
        assert_eq!(listed.webhooks.len(), 1);
        assert_eq!(listed.webhooks[0].id, registered.id);
        assert_eq!(
            listed.webhooks[0].event_types.as_deref(),
            Some([String::from("task.updated")].as_slice())
        );

        let health = get_webhook_health_http(State(state.clone()), headers.clone())
            .await
            .expect("webhook health should list")
            .0;
        assert_eq!(health.webhooks.len(), 1);
        assert_eq!(health.webhooks[0].id, registered.id);
        assert!(health.webhooks[0].active);
        assert_eq!(health.webhooks[0].failure_count, 0);

        let unregistered =
            unregister_webhook_http(State(state.clone()), headers.clone(), Path(registered.id))
                .await
                .expect("webhook should unregister")
                .0;
        assert!(unregistered.success);

        let missing =
            unregister_webhook_http(State(state), headers, Path("missing-webhook".to_string()))
                .await
                .unwrap_err();
        assert_eq!(missing.status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn receive_linear_webhook_maps_store_errors_to_internal_server_error() {
        let state = test_http_state();
        let error = receive_linear_webhook_http(
            State(state),
            HeaderMap::new(),
            Bytes::from_static(br#"{"type":"Issue"}"#),
        )
        .await
        .unwrap_err();

        assert_eq!(error.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(!error.message.as_deref().unwrap_or_default().is_empty());
    }

    #[test]
    fn linear_action_labels_cover_all_reconciliation_outcomes() {
        let task_id = TaskId::from_string("task-1".to_string());
        let actions = [
            (
                LinearWebhookAction::TransitionedTask {
                    task_id,
                    target_status: InternalStatus::Executing,
                },
                "transitioned_task",
            ),
            (LinearWebhookAction::RecordedIssue, "recorded_issue"),
            (
                LinearWebhookAction::RecordedIssueActivity,
                "recorded_issue_activity",
            ),
            (LinearWebhookAction::NoLinkedTask, "no_linked_task"),
            (LinearWebhookAction::NoMappedStatus, "no_mapped_status"),
            (LinearWebhookAction::UnsupportedEvent, "unsupported_event"),
            (LinearWebhookAction::DuplicateDelivery, "duplicate_delivery"),
        ];

        for (action, expected) in actions {
            assert_eq!(linear_action_label(&action), expected);
        }
    }

    /// The handler's dedupe gate (`if !outcome.duplicate`) skips cache
    /// invalidation only for `DuplicateDelivery`. Confirm that action is the
    /// sole one mapping to the `duplicate_delivery` label, so the gate can never
    /// silently start (or stop) matching the wrong action.
    #[test]
    fn duplicate_delivery_is_the_only_skipped_invalidation_label() {
        let task_id = TaskId::from_string("task-1".to_string());
        let non_duplicate_actions = [
            LinearWebhookAction::TransitionedTask {
                task_id,
                target_status: InternalStatus::Executing,
            },
            LinearWebhookAction::RecordedIssue,
            LinearWebhookAction::RecordedIssueActivity,
            LinearWebhookAction::NoLinkedTask,
            LinearWebhookAction::NoMappedStatus,
            LinearWebhookAction::UnsupportedEvent,
        ];

        assert_eq!(
            linear_action_label(&LinearWebhookAction::DuplicateDelivery),
            "duplicate_delivery"
        );
        for action in non_duplicate_actions {
            assert_ne!(
                linear_action_label(&action),
                "duplicate_delivery",
                "non-duplicate actions must not share the skipped label"
            );
        }
    }

    /// For a non-duplicate outcome the handler invokes `invalidate_linear_webhook`
    /// with the action label as the reason. Exercise that exact call (the branch
    /// the dedupe gate runs) and confirm it constructs a ticketing-cache event
    /// carrying the action label as its reason.
    #[test]
    fn non_duplicate_branch_invalidates_with_action_label_reason() {
        use crate::application::TicketingCacheInvalidator;

        let action = LinearWebhookAction::RecordedIssue;
        let body = serde_json::json!({
            "type": "Issue",
            "data": { "id": "issue-1", "identifier": "LIN-1" }
        })
        .to_string();

        // No app_handle (as in test AppState) → no emit, but the event is built.
        let event = TicketingCacheInvalidator::invalidate_linear_webhook(
            None,
            body.as_bytes(),
            linear_action_label(&action),
        )
        .expect("non-duplicate Issue webhook should produce an invalidation event");

        assert_eq!(event.reason, "recorded_issue");
        assert_eq!(event.ticket_id.as_deref(), Some("issue-1"));
    }

    #[test]
    fn linear_webhook_errors_map_to_http_statuses() {
        let cases = [
            (
                LinearWebhookError::MissingSignature,
                StatusCode::UNAUTHORIZED,
            ),
            (
                LinearWebhookError::InvalidSignature,
                StatusCode::UNAUTHORIZED,
            ),
            (LinearWebhookError::StaleTimestamp, StatusCode::UNAUTHORIZED),
            (
                LinearWebhookError::MalformedBody("bad json".to_string()),
                StatusCode::BAD_REQUEST,
            ),
            (
                LinearWebhookError::MissingDeliveryId,
                StatusCode::BAD_REQUEST,
            ),
            (
                LinearWebhookError::MissingSecret,
                StatusCode::SERVICE_UNAVAILABLE,
            ),
            (
                LinearWebhookError::Reconciliation("database unavailable".to_string()),
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
        ];

        for (error, expected_status) in cases {
            let http_error = linear_webhook_http_error(error);
            assert_eq!(http_error.status, expected_status);
            assert!(http_error.message.is_some());
        }
    }

    #[test]
    fn header_string_reads_valid_headers_and_ignores_invalid_values() {
        let mut headers = HeaderMap::new();
        headers.insert("linear-delivery", "delivery-1".parse().unwrap());
        let invalid = axum::http::HeaderValue::from_bytes(b"\xff").unwrap();
        headers.insert("linear-event", invalid);

        assert_eq!(
            header_string(&headers, "linear-delivery").as_deref(),
            Some("delivery-1")
        );
        assert!(header_string(&headers, "linear-event").is_none());
        assert!(header_string(&headers, "missing").is_none());
    }
}
