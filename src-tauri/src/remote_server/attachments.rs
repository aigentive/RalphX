//! Binary attachment ingress/egress for the :3849 remote facade (§4.3, C-16).
//!
//! Attachments deliberately do NOT flow through `/invoke`: the facade rejects `Channel` and
//! raw-body parameters structurally, and widening it to carry bytes would put an unbounded
//! payload on the same parse path as every command argument. These two routes are the ONLY
//! byte-ingress on the remote surface.
//!
//! ## Path safety (CodeQL `rust/path-injection`)
//!
//! The storage root is app-owned (`AppPaths::app_data_dir`), never env- or request-derived.
//! The ONLY path component appended to it is a server-minted UUID; the client-supplied
//! filename is stored as a DB column (`display_name`) and never touches the filesystem. The
//! join is re-validated at the sink — provenance is not trusted, per repo rules — by rejecting
//! any id that is not a plain UUID and asserting the resolved parent is the canonical root.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::{
    extract::{Multipart, Path as AxumPath, State},
    http::{header, StatusCode},
    response::IntoResponse,
    Extension, Json,
};
use ralphx_remote_protocol::{ErrorCode, Scope};
use serde::Serialize;

use crate::domain::entities::RemoteAttachment;
use crate::domain::repositories::RemoteAttachmentRepository;
use crate::remote_server::auth::RemoteIdentity;
use crate::remote_server::endpoints::RemoteRouterState;
use crate::remote_server::remote_error_response;

pub(crate) const ATTACHMENT_UPLOAD_PATH: &str = "/remote/v1/attachments/upload";
/// axum 0.7 path-parameter syntax (`:id`), matching the rest of this router.
pub(crate) const ATTACHMENT_FETCH_PATH: &str = "/remote/v1/attachments/:id";

/// Per-request ceiling. Sized for a phone screenshot or a short voice memo, not a video.
pub(crate) const REMOTE_ATTACHMENT_MAX_BYTES: usize = 16 * 1024 * 1024;

/// Per-device durable ceiling, enforced BEFORE any byte is written.
///
/// Quota is per device rather than per host so one compromised or buggy client cannot fill the
/// disk that the whole app depends on.
pub(crate) const REMOTE_ATTACHMENT_DEVICE_QUOTA_BYTES: i64 = 256 * 1024 * 1024;

/// Directory name under the app data dir. A fixed literal — never interpolated.
pub(crate) const REMOTE_ATTACHMENT_DIR: &str = "remote_attachments";

const DEFAULT_MIME: &str = "application/octet-stream";

/// The attachment store plus its app-owned root.
pub(crate) struct RemoteAttachmentContext {
    store: Arc<dyn RemoteAttachmentRepository>,
    /// Absolute, app-owned. Built from `AppPaths::app_data_dir`, never from a request or env.
    root: PathBuf,
}

impl RemoteAttachmentContext {
    pub(crate) fn new(store: Arc<dyn RemoteAttachmentRepository>, app_data_dir: &Path) -> Self {
        Self {
            store,
            root: app_data_dir.join(REMOTE_ATTACHMENT_DIR),
        }
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UploadResponse {
    attachment_id: String,
    size: i64,
    mime: String,
}

/// Accepts an id ONLY if it is a canonical hyphenated UUID.
///
/// This is the containment primitive: a value that passes cannot contain `/`, `\`, `.`, `..`,
/// a drive prefix, or a root component, so joining it onto the root cannot escape. Rejecting is
/// the only failure mode — the id is never sanitized into something acceptable.
pub(crate) fn is_safe_attachment_id(id: &str) -> bool {
    uuid::Uuid::parse_str(id).is_ok_and(|parsed| parsed.hyphenated().to_string() == id)
}

/// Joins an already-validated id onto the app-owned root and re-proves containment at the sink.
///
/// Provenance is deliberately re-checked here rather than trusted from the caller: CodeQL
/// tracks taint across helper layers and, more importantly, a future caller could pass an id
/// from a new source.
pub(crate) fn attachment_path(root: &Path, id: &str) -> Option<PathBuf> {
    if !is_safe_attachment_id(id) {
        return None;
    }
    let candidate = root.join(id);
    // Component-level proof, independent of the string check above: the joined path must be the
    // root plus exactly one normal component.
    let mut remainder = candidate.strip_prefix(root).ok()?.components();
    let only = remainder.next()?;
    if remainder.next().is_some() {
        return None;
    }
    match only {
        std::path::Component::Normal(_) => Some(candidate),
        _ => None,
    }
}

fn attachment_error(
    status: StatusCode,
    code: ErrorCode,
    message: &str,
) -> axum::response::Response {
    remote_error_response(status, code, message)
}

/// `POST /remote/v1/attachments/upload` — requires `ui:operate`.
pub(crate) async fn upload_handler(
    State(state): State<RemoteRouterState>,
    Extension(identity): Extension<RemoteIdentity>,
    mut multipart: Multipart,
) -> axum::response::Response {
    if !identity.has_scope(Scope::UiOperate) {
        return attachment_error(
            StatusCode::FORBIDDEN,
            ErrorCode::RemoteForbidden,
            "Uploading attachments requires the ui:operate scope.",
        );
    }
    let Some(context) = state.attachments().cloned() else {
        tracing::error!("attachment upload reached a router with no attachment store; refusing");
        return attachment_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            ErrorCode::RemoteInternalError,
            "Attachment storage is not configured.",
        );
    };

    // Quota is checked BEFORE any byte is written. A store error fails closed: an unreadable
    // quota must not be read as "plenty of room".
    let used = match context.store.device_usage_bytes(&identity.device_id).await {
        Ok(used) => used,
        Err(error) => {
            tracing::error!(%error, "attachment quota read failed; refusing upload");
            return attachment_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                ErrorCode::RemoteInternalError,
                "Attachment storage is unavailable.",
            );
        }
    };
    if used >= REMOTE_ATTACHMENT_DEVICE_QUOTA_BYTES {
        return attachment_error(
            StatusCode::FORBIDDEN,
            ErrorCode::RemoteForbidden,
            "This device has reached its attachment storage quota.",
        );
    }

    let mut field = match multipart.next_field().await {
        Ok(Some(field)) => field,
        Ok(None) => {
            return attachment_error(
                StatusCode::BAD_REQUEST,
                ErrorCode::RemoteInvalidArguments,
                "The upload contained no file part.",
            )
        }
        Err(error) => {
            return attachment_error(
                StatusCode::BAD_REQUEST,
                ErrorCode::RemoteInvalidArguments,
                &format!("The upload could not be parsed: {error}"),
            )
        }
    };

    // Stored as DATA only — this value never becomes a path component.
    let display_name = field.file_name().map(str::to_string);
    let mime = field
        .content_type()
        .map(str::to_string)
        .unwrap_or_else(|| DEFAULT_MIME.to_string());

    let remaining = REMOTE_ATTACHMENT_DEVICE_QUOTA_BYTES.saturating_sub(used);
    let mut bytes: Vec<u8> = Vec::new();
    loop {
        match field.chunk().await {
            Ok(Some(chunk)) => {
                bytes.extend_from_slice(&chunk);
                if bytes.len() > REMOTE_ATTACHMENT_MAX_BYTES {
                    return attachment_error(
                        StatusCode::PAYLOAD_TOO_LARGE,
                        ErrorCode::RemoteInvalidArguments,
                        "The attachment exceeds the per-request size limit.",
                    );
                }
                if bytes.len() as i64 > remaining {
                    return attachment_error(
                        StatusCode::FORBIDDEN,
                        ErrorCode::RemoteForbidden,
                        "This upload would exceed the device's attachment storage quota.",
                    );
                }
            }
            Ok(None) => break,
            Err(error) => {
                return attachment_error(
                    StatusCode::BAD_REQUEST,
                    ErrorCode::RemoteInvalidArguments,
                    &format!("The upload stream failed: {error}"),
                )
            }
        }
    }

    let id = uuid::Uuid::new_v4().hyphenated().to_string();
    let Some(path) = attachment_path(context.root(), &id) else {
        tracing::error!("minted attachment id failed containment validation; refusing");
        return attachment_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            ErrorCode::RemoteInternalError,
            "Attachment storage is unavailable.",
        );
    };

    // Fixed, app-owned root; the only child component is the validated UUID above.
    // codeql[rust/path-injection]
    if let Err(error) = tokio::fs::create_dir_all(context.root()).await {
        tracing::error!(%error, "attachment root could not be created");
        return attachment_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            ErrorCode::RemoteInternalError,
            "Attachment storage is unavailable.",
        );
    }

    let size = bytes.len() as i64;

    // Root is app-owned and the child component is a validated server-minted UUID.
    // codeql[rust/path-injection]
    if let Err(error) = tokio::fs::write(&path, &bytes).await {
        tracing::error!(%error, "attachment bytes could not be written");
        return attachment_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            ErrorCode::RemoteInternalError,
            "Attachment storage is unavailable.",
        );
    }

    if let Err(error) = context
        .store
        .record(RemoteAttachment {
            id: id.clone(),
            device_id: identity.device_id.clone(),
            display_name,
            mime: mime.clone(),
            size,
            created_at: chrono::Utc::now()
                .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                .to_string(),
        })
        .await
    {
        tracing::error!(%error, "attachment metadata write failed; removing the orphan blob");
        // Without the row the blob is unreachable AND uncounted against quota, so it is
        // removed rather than left as silent disk growth.
        // codeql[rust/path-injection]
        let _ = tokio::fs::remove_file(&path).await;
        return attachment_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            ErrorCode::RemoteInternalError,
            "Attachment storage is unavailable.",
        );
    }

    (
        StatusCode::OK,
        Json(UploadResponse {
            attachment_id: id,
            size,
            mime,
        }),
    )
        .into_response()
}

/// `GET /remote/v1/attachments/{id}` — requires `ui:read`, device-scoped.
///
/// Cross-device reads are refused with the same 404 as a missing id: a distinguishable 403
/// would confirm that another device holds that attachment.
pub(crate) async fn fetch_handler(
    State(state): State<RemoteRouterState>,
    Extension(identity): Extension<RemoteIdentity>,
    AxumPath(id): AxumPath<String>,
) -> axum::response::Response {
    if !identity.has_scope(Scope::UiRead) {
        return attachment_error(
            StatusCode::FORBIDDEN,
            ErrorCode::RemoteForbidden,
            "Reading attachments requires the ui:read scope.",
        );
    }
    let Some(context) = state.attachments().cloned() else {
        return attachment_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            ErrorCode::RemoteInternalError,
            "Attachment storage is not configured.",
        );
    };

    // Rejected before any store or filesystem access: a traversal attempt never reaches a sink.
    if !is_safe_attachment_id(&id) {
        return not_found();
    }

    let record = match context.store.get_for_device(&identity.device_id, &id).await {
        Ok(Some(record)) => record,
        // Device-scoped in the query itself, so this arm covers both "no such id" and
        // "another device's id".
        Ok(None) => return not_found(),
        Err(error) => {
            tracing::error!(%error, "attachment metadata read failed; refusing");
            return attachment_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                ErrorCode::RemoteInternalError,
                "Attachment storage is unavailable.",
            );
        }
    };

    let Some(path) = attachment_path(context.root(), &record.id) else {
        return not_found();
    };

    // Root is app-owned; the child is a validated UUID that also matched a device-scoped row.
    // codeql[rust/path-injection]
    match tokio::fs::read(&path).await {
        Ok(bytes) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, record.mime.clone())],
            bytes,
        )
            .into_response(),
        Err(error) => {
            tracing::error!(%error, attachment_id = %record.id, "attachment blob is unreadable");
            not_found()
        }
    }
}

fn not_found() -> axum::response::Response {
    attachment_error(
        StatusCode::NOT_FOUND,
        ErrorCode::RemoteCommandUnavailable,
        "No such attachment.",
    )
}
