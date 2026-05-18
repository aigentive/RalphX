use axum::{
    extract::{Path, Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::Response,
    Json,
};

use super::*;
use crate::domain::entities::{
    ApiKey, ApiKeyId, PERMISSION_ADMIN, PERMISSION_CREATE_PROJECT, PERMISSION_MAX, PERMISSION_READ,
    PERMISSION_WRITE,
};
use crate::domain::repositories::ApiKeyRepository;
use crate::domain::services::api_key_service::{ApiKeyService, KeySource};

/// Response for GET /api/validate_key
#[derive(Debug, serde::Serialize)]
pub struct ValidateKeyResponse {
    pub valid: bool,
    pub key_id: Option<String>,
    pub key_name: Option<String>,
    pub permissions: Option<Vec<String>>,
    pub message: String,
}

/// Response for GET /api/auth/validate-key (external MCP server endpoint).
/// Returns permissions as i32 bitmask and project_ids as string list — matching
/// the TypeScript ValidateKeyResponse interface in ralphx-external-mcp.
#[derive(Debug, serde::Serialize)]
pub struct ExternalValidateKeyResponse {
    pub valid: bool,
    pub key_id: Option<String>,
    pub key_name: Option<String>,
    /// Raw permission bitmask (1=read, 2=write, 4=admin)
    pub permissions: Option<i32>,
    /// Project IDs this key is scoped to
    pub project_ids: Vec<String>,
    pub message: String,
}

/// Compute SHA-256 hex digest of the given raw key string.
fn sha256_hex(raw: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(raw.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Log an error and return 500 INTERNAL_SERVER_ERROR.
fn internal_error<E: std::fmt::Display>(msg: &str, e: E) -> StatusCode {
    tracing::error!("{}: {}", msg, e);
    StatusCode::INTERNAL_SERVER_ERROR
}

/// Middleware: require a valid admin-level API key for management routes.
///
/// Bootstrap exception: if no active (non-revoked) keys exist, access is
/// allowed so the first key can be created from the Tauri UI.
///
/// Returns 401 UNAUTHORIZED if no key is provided or the key is invalid/revoked.
/// Returns 403 FORBIDDEN if the key is valid but lacks admin permission.
pub async fn require_admin_key(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // Bootstrap mode: allow unauthenticated access when no active keys exist,
    // so the first key can be created from the UI.
    let active_key_count = state
        .app_state
        .api_key_repo
        .list()
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|k| k.revoked_at.is_none())
        .count();

    if active_key_count == 0 {
        return Ok(next.run(request).await);
    }

    let raw_key = match extract_raw_key(&headers) {
        Some(k) => k,
        None => return Err(StatusCode::UNAUTHORIZED),
    };

    let key_hash = sha256_hex(&raw_key);
    let key = state
        .app_state
        .api_key_repo
        .get_by_hash(&key_hash)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    match key {
        None => Err(StatusCode::UNAUTHORIZED),
        Some(k) if !k.is_active() && !k.is_in_grace_period() => Err(StatusCode::UNAUTHORIZED),
        Some(k) if k.permissions & PERMISSION_ADMIN == 0 => Err(StatusCode::FORBIDDEN),
        Some(_) => Ok(next.run(request).await),
    }
}

/// POST /api/auth/keys — Create a new API key
pub async fn create_api_key(
    State(state): State<HttpServerState>,
    Json(req): Json<CreateApiKeyRequest>,
) -> Result<Json<CreateApiKeyResponse>, StatusCode> {
    if let Some(p) = req.permissions {
        if !(0..=PERMISSION_MAX).contains(&p) {
            return Err(StatusCode::UNPROCESSABLE_ENTITY);
        }
    }
    let project_ids = req.project_ids.unwrap_or_default();
    let result = ApiKeyService::create_key(
        state.app_state.api_key_repo.as_ref(),
        &req.name,
        req.permissions,
        &project_ids,
        KeySource::HttpApi,
    )
    .await
    .map_err(|e| internal_error("Failed to create API key", e))?;

    Ok(Json(CreateApiKeyResponse {
        id: result.key.id.to_string(),
        name: result.key.name,
        key: result.raw_key,
        key_prefix: result.key.key_prefix,
        permissions: result.key.permissions,
        created_at: result.key.created_at,
    }))
}

/// GET /api/auth/keys — List all API keys
pub async fn list_api_keys(
    State(state): State<HttpServerState>,
) -> Result<Json<ListApiKeysResponse>, StatusCode> {
    let keys = state
        .app_state
        .api_key_repo
        .list()
        .await
        .map_err(|e| internal_error("Failed to list API keys", e))?;

    let mut key_infos = Vec::with_capacity(keys.len());
    for key in keys {
        let project_ids = state
            .app_state
            .api_key_repo
            .get_projects(&key.id)
            .await
            .unwrap_or_default();
        key_infos.push(ApiKeyInfo {
            id: key.id.to_string(),
            name: key.name,
            key_prefix: key.key_prefix,
            permissions: key.permissions,
            created_at: key.created_at,
            revoked_at: key.revoked_at,
            last_used_at: key.last_used_at,
            project_ids,
        });
    }

    let count = key_infos.len();
    Ok(Json(ListApiKeysResponse {
        keys: key_infos,
        count,
    }))
}

/// DELETE /api/auth/keys/:id — Revoke an API key
pub async fn delete_api_key(
    State(state): State<HttpServerState>,
    Path(id): Path<String>,
) -> Result<Json<SuccessResponse>, StatusCode> {
    // Verify key exists before revoking
    let key_id = ApiKeyId::from_string(&id);
    let key = state
        .app_state
        .api_key_repo
        .get_by_id(&key_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if key.is_none() {
        return Err(StatusCode::NOT_FOUND);
    }

    ApiKeyService::revoke_key(
        state.app_state.api_key_repo.as_ref(),
        &id,
        KeySource::HttpApi,
    )
    .await
    .map_err(|e| internal_error("Failed to revoke API key", e))?;

    Ok(Json(SuccessResponse {
        success: true,
        message: "API key revoked".to_string(),
    }))
}

/// POST /api/auth/keys/:id/rotate — Rotate an API key with 60s grace period on the old key
pub async fn rotate_api_key(
    State(state): State<HttpServerState>,
    Path(id): Path<String>,
) -> Result<Json<RotateApiKeyResponse>, StatusCode> {
    let result = ApiKeyService::rotate_key(
        state.app_state.api_key_repo.as_ref(),
        &id,
        KeySource::HttpApi,
    )
    .await
    .map_err(|e| match e {
        crate::error::AppError::NotFound(_) => StatusCode::NOT_FOUND,
        crate::error::AppError::Validation(_) => StatusCode::UNPROCESSABLE_ENTITY,
        other => internal_error("Failed to rotate API key", other),
    })?;

    // Reconstruct the grace expiry that was applied to the old key.
    // The service set now+60s at call time; we replicate that for the response.
    let grace_expires_at = (chrono::Utc::now() + chrono::Duration::seconds(60))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();

    Ok(Json(RotateApiKeyResponse {
        id: result.key.id.to_string(),
        new_key: result.raw_key,
        key_prefix: result.key.key_prefix,
        old_key_grace_expires_at: grace_expires_at,
    }))
}

/// PUT /api/auth/keys/:id/projects — Update project associations for a key
pub async fn update_api_key_projects(
    State(state): State<HttpServerState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateApiKeyProjectsRequest>,
) -> Result<Json<SuccessResponse>, StatusCode> {
    let key_id = ApiKeyId::from_string(id);

    let key = state
        .app_state
        .api_key_repo
        .get_by_id(&key_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if key.is_none() {
        return Err(StatusCode::NOT_FOUND);
    }

    state
        .app_state
        .api_key_repo
        .set_projects(&key_id, &req.project_ids)
        .await
        .map_err(|e| internal_error("Failed to update API key projects", e))?;

    Ok(Json(SuccessResponse {
        success: true,
        message: "Project associations updated".to_string(),
    }))
}

/// GET /api/auth/keys/:id/audit — Retrieve audit log entries for a key
pub async fn get_audit_log(
    State(state): State<HttpServerState>,
    Path(id): Path<String>,
) -> Result<Json<AuditLogResponse>, StatusCode> {
    let key_id = ApiKeyId::from_string(id);

    let entries = state
        .app_state
        .api_key_repo
        .get_audit_log(key_id.as_str(), Some(100))
        .await
        .map_err(|e| internal_error("Failed to get audit log", e))?;

    Ok(Json(AuditLogResponse { entries }))
}

/// PUT /api/auth/keys/:id/permissions — Update the permission bitmask for a key
pub async fn update_key_permissions(
    State(state): State<HttpServerState>,
    Path(id): Path<String>,
    Json(req): Json<UpdatePermissionsRequest>,
) -> Result<Json<SuccessResponse>, StatusCode> {
    if req.permissions < 0 || req.permissions > PERMISSION_MAX as i64 {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }

    let key_id = ApiKeyId::from_string(id);

    state
        .app_state
        .api_key_repo
        .update_api_key_permissions(key_id.as_str(), req.permissions)
        .await
        .map_err(|e| {
            if matches!(e, crate::error::AppError::NotFound(_)) {
                StatusCode::NOT_FOUND
            } else {
                internal_error("Failed to update API key permissions", e)
            }
        })?;

    Ok(Json(SuccessResponse {
        success: true,
        message: "Permissions updated".to_string(),
    }))
}

/// GET /api/auth/validate-key — Validate a bearer token, return metadata + project_ids
/// Used by external MCP server to validate incoming API keys.
/// Returns permissions as i32 bitmask and project_ids list (not human-readable names).
/// Returns HTTP 401 for missing, invalid, or revoked keys.
pub async fn validate_api_key(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
) -> Result<Json<ExternalValidateKeyResponse>, StatusCode> {
    let key = resolve_key_from_headers(state.app_state.api_key_repo.as_ref(), &headers).await?;

    // Fetch project associations synchronously before spawning background tasks
    let project_ids = state
        .app_state
        .api_key_repo
        .get_projects(&key.id)
        .await
        .unwrap_or_default();

    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let repo = state.app_state.api_key_repo.clone();
    let key_id_clone = key.id.clone();
    tokio::spawn(async move {
        let _ = repo.update_last_used(&key_id_clone, &now).await;
        let _ = repo
            .log_audit(key_id_clone.as_str(), "validate_key", None, true, None)
            .await;
    });

    Ok(Json(ExternalValidateKeyResponse {
        valid: true,
        key_id: Some(key.id.as_str().to_string()),
        key_name: Some(key.name),
        permissions: Some(key.permissions),
        project_ids,
        message: "API key is valid.".to_string(),
    }))
}

/// Extract the raw API key value from request headers.
///
/// Supports two header formats:
/// - `Authorization: Bearer <key>`
/// - `X-RalphX-Key: <key>`
fn extract_raw_key(headers: &HeaderMap) -> Option<String> {
    // Prefer Authorization: Bearer <key>
    if let Some(auth) = headers.get("authorization") {
        if let Ok(auth_str) = auth.to_str() {
            let trimmed = auth_str.trim();
            if let Some(key) = trimmed.strip_prefix("Bearer ") {
                let key = key.trim();
                if !key.is_empty() {
                    return Some(key.to_string());
                }
            }
        }
    }

    // Fall back to X-RalphX-Key header
    if let Some(val) = headers.get("x-ralphx-key") {
        if let Ok(key_str) = val.to_str() {
            let key = key_str.trim();
            if !key.is_empty() {
                return Some(key.to_string());
            }
        }
    }

    None
}

/// Look up and validate an API key from the Authorization or X-RalphX-Key header.
///
/// Returns the active [`ApiKey`] on success.
///
/// # Errors
/// - `StatusCode::UNAUTHORIZED` — missing, unknown, or revoked key
/// - `StatusCode::INTERNAL_SERVER_ERROR` — repository failure
async fn resolve_key_from_headers(
    repo: &dyn ApiKeyRepository,
    headers: &HeaderMap,
) -> Result<ApiKey, StatusCode> {
    let raw_key = extract_raw_key(headers).ok_or(StatusCode::UNAUTHORIZED)?;
    let key_hash = sha256_hex(&raw_key);
    let key = repo
        .get_by_hash(&key_hash)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    match key {
        None => Err(StatusCode::UNAUTHORIZED),
        Some(k) if !k.is_active() && !k.is_in_grace_period() => Err(StatusCode::UNAUTHORIZED),
        Some(k) => Ok(k),
    }
}

/// Convert numeric permission bitmask to human-readable list.
fn permission_names(permissions: i32) -> Vec<String> {
    let mut names = Vec::new();
    if permissions & PERMISSION_READ != 0 {
        names.push("read".to_string());
    }
    if permissions & PERMISSION_WRITE != 0 {
        names.push("write".to_string());
    }
    if permissions & PERMISSION_ADMIN != 0 {
        names.push("admin".to_string());
    }
    if permissions & PERMISSION_CREATE_PROJECT != 0 {
        names.push("create_project".to_string());
    }
    names
}

/// GET /api/validate_key
///
/// Validates an API key supplied via the `Authorization: Bearer <key>` header
/// or the `X-RalphX-Key` header.
///
/// Returns 200 with `{ valid: true, ... }` for active keys and
/// 401 with `{ valid: false, ... }` for missing, invalid, or revoked keys.
pub async fn validate_key(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
) -> Result<Json<ValidateKeyResponse>, StatusCode> {
    match resolve_key_from_headers(state.app_state.api_key_repo.as_ref(), &headers).await {
        Err(e) if e == StatusCode::UNAUTHORIZED => Ok(Json(ValidateKeyResponse {
            valid: false,
            key_id: None,
            key_name: None,
            permissions: None,
            message: "API key is missing, invalid, or revoked.".to_string(),
        })),
        Err(e) => Err(e),
        Ok(key) => {
            // Update last_used_at in the background (non-blocking; ignore errors)
            let repo = state.app_state.api_key_repo.clone();
            let key_id = key.id.clone();
            let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
            tokio::spawn(async move {
                let _ = repo.update_last_used(&key_id, &now).await;
            });

            Ok(Json(ValidateKeyResponse {
                valid: true,
                key_id: Some(key.id.as_str().to_string()),
                key_name: Some(key.name.clone()),
                permissions: Some(permission_names(key.permissions)),
                message: "API key is valid.".to_string(),
            }))
        }
    }
}
