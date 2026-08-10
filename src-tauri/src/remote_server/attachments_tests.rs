//! Attachment ingress/egress: scope gates, quota, device scoping, and path containment.
//!
//! The path-safety tests are the load-bearing ones: they assert that a traversal attempt is
//! REJECTED rather than sanitized, and that rejection happens before any filesystem access.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::{
    body::to_bytes,
    body::Body,
    http::{header, Method, Request, StatusCode},
    Router,
};
use ralphx_remote_protocol::Scope;
use serde_json::Value;
use tower::ServiceExt;

use super::attachments::{
    attachment_path, is_safe_attachment_id, RemoteAttachmentContext, ATTACHMENT_UPLOAD_PATH,
    REMOTE_ATTACHMENT_DEVICE_QUOTA_BYTES, REMOTE_ATTACHMENT_DIR, REMOTE_ATTACHMENT_MAX_BYTES,
};
use super::auth::RemoteAuthContext;
use super::auth_tests::{pair_device_with_scopes, TEST_ENVIRONMENT_ID};
use super::authenticated_remote_routes;
use super::dedup::RemoteDedupState;
use super::endpoints::RemoteRouterState;
use super::invoke::RemoteInvokeDispatcher;
use super::registry::{DispatchOutcome, RemoteInvokeError};
use super::session_registry::RemoteSessionRegistry;
use super::settings::RemoteExposureMode;
use crate::domain::entities::{RemoteAttachment, RemoteDeviceId, RemoteScopeSet};
use crate::domain::repositories::RemoteAttachmentRepository;
use crate::infrastructure::sqlite::migrations::run_migrations;
use crate::infrastructure::sqlite::{DbConnection, SqliteRemoteRequestDedupRepository};

const BOUNDARY: &str = "ralphxboundary";

struct UnusedDispatcher;

#[async_trait::async_trait]
impl RemoteInvokeDispatcher for UnusedDispatcher {
    async fn dispatch(
        &self,
        _scopes: &[Scope],
        _command: &str,
        _args: &Value,
    ) -> Result<DispatchOutcome, RemoteInvokeError> {
        unreachable!("attachment tests never invoke commands")
    }
}

struct Harness {
    auth: RemoteAuthContext,
    store: Arc<SqliteRemoteRequestDedupRepository>,
    context: Arc<RemoteAttachmentContext>,
    root: PathBuf,
    _temp: tempfile::TempDir,
}

fn harness() -> Harness {
    let conn = rusqlite::Connection::open_in_memory().expect("in-memory database should open");
    run_migrations(&conn).expect("migrations should apply");
    let shared = Arc::new(tokio::sync::Mutex::new(conn));
    let auth = RemoteAuthContext::from_db(
        DbConnection::from_shared(shared.clone()),
        RemoteSessionRegistry::new(),
        RemoteExposureMode::Serve,
    );
    let store = Arc::new(SqliteRemoteRequestDedupRepository::from_db(
        DbConnection::from_shared(shared),
    ));
    let temp = tempfile::tempdir().expect("temp dir should create");
    let context = Arc::new(RemoteAttachmentContext::new(store.clone(), temp.path()));
    let root = temp.path().join(REMOTE_ATTACHMENT_DIR);
    Harness {
        auth,
        store,
        context,
        root,
        _temp: temp,
    }
}

impl Harness {
    fn router(&self) -> Router {
        authenticated_remote_routes(
            RemoteRouterState::new_with_invoke_dispatcher(
                TEST_ENVIRONMENT_ID,
                self.auth.clone(),
                Arc::new(UnusedDispatcher),
            )
            .with_dedup(Arc::new(RemoteDedupState::new(self.store.clone())))
            .with_attachments(self.context.clone()),
        )
    }

    /// A router with attachments deliberately unwired, to prove the fail-closed refusal.
    fn router_without_attachments(&self) -> Router {
        authenticated_remote_routes(
            RemoteRouterState::new_with_invoke_dispatcher(
                TEST_ENVIRONMENT_ID,
                self.auth.clone(),
                Arc::new(UnusedDispatcher),
            )
            .with_dedup(Arc::new(RemoteDedupState::new(self.store.clone()))),
        )
    }

    async fn device(&self, name: &str, scopes: &[Scope]) -> (String, RemoteDeviceId) {
        pair_device_with_scopes(
            &self.auth,
            name,
            RemoteScopeSet::from_scopes(scopes.iter().copied()),
        )
        .await
    }
}

fn multipart_body(filename: &str, mime: &str, bytes: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(
        format!(
            "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; \
             filename=\"{filename}\"\r\nContent-Type: {mime}\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());
    body
}

fn upload_request(token: &str, filename: &str, mime: &str, bytes: &[u8]) -> Request<Body> {
    Request::builder()
        .method(Method::POST)
        .uri(ATTACHMENT_UPLOAD_PATH)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={BOUNDARY}"),
        )
        .body(Body::from(multipart_body(filename, mime, bytes)))
        .expect("request should build")
}

fn fetch_request(token: &str, id: &str) -> Request<Body> {
    Request::builder()
        .method(Method::GET)
        .uri(format!("/remote/v1/attachments/{id}"))
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .expect("request should build")
}

async fn body_bytes(response: axum::response::Response) -> Vec<u8> {
    to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body should read")
        .to_vec()
}

async fn body_json(response: axum::response::Response) -> Value {
    serde_json::from_slice(&body_bytes(response).await).expect("body should be JSON")
}

// ---------------------------------------------------------------------------------------
// Path containment (CodeQL rust/path-injection)
// ---------------------------------------------------------------------------------------

#[test]
fn only_a_canonical_uuid_passes_the_id_guard() {
    let good = uuid::Uuid::new_v4().hyphenated().to_string();
    assert!(is_safe_attachment_id(&good));

    let with_traversal_suffix = format!("{good}/..");
    let with_traversal_prefix = format!("../{good}");
    let braced = format!("{{{good}}}");
    let urn = format!("urn:uuid:{good}");
    let unhyphenated = good.replace('-', "");
    let upper = good.to_uppercase();

    for hostile in [
        "..",
        "../secrets",
        "../../etc/passwd",
        "/etc/passwd",
        "/",
        "",
        ".",
        "sub/dir",
        "sub\\dir",
        "C:\\Windows",
        "notes.txt",
        with_traversal_suffix.as_str(),
        with_traversal_prefix.as_str(),
        // Braced and urn spellings parse as UUIDs but are not our canonical component.
        braced.as_str(),
        urn.as_str(),
        unhyphenated.as_str(),
        upper.as_str(),
    ] {
        assert!(
            !is_safe_attachment_id(hostile),
            "{hostile:?} must be rejected, never sanitized"
        );
    }
}

#[test]
fn the_path_builder_refuses_every_hostile_id_and_never_escapes_the_root() {
    let root = Path::new("/app/data/remote_attachments");

    for hostile in ["..", "../../etc/passwd", "/etc/passwd", "a/b", ""] {
        assert!(
            attachment_path(root, hostile).is_none(),
            "{hostile:?} must not produce a path"
        );
    }

    let good = uuid::Uuid::new_v4().hyphenated().to_string();
    let path = attachment_path(root, &good).expect("a canonical uuid should resolve");
    assert_eq!(path.parent(), Some(root), "the parent must be the root");
    assert!(path.starts_with(root));
}

// ---------------------------------------------------------------------------------------
// Scope gates
// ---------------------------------------------------------------------------------------

#[tokio::test]
async fn uploading_with_only_ui_read_is_forbidden_and_writes_nothing() {
    let harness = harness();
    let (token, device) = harness.device("reader", &[Scope::UiRead]).await;

    let response = harness
        .router()
        .oneshot(upload_request(&token, "notes.txt", "text/plain", b"hello"))
        .await
        .expect("request should complete");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        RemoteAttachmentRepository::device_usage_bytes(harness.store.as_ref(), &device)
            .await
            .expect("usage should read"),
        0,
        "ABSENCE: a forbidden upload must persist nothing"
    );
    assert!(
        !harness.root.exists(),
        "ABSENCE: a forbidden upload must not touch the filesystem"
    );
}

#[tokio::test]
async fn fetching_without_ui_read_is_forbidden() {
    let harness = harness();
    let (token, _) = harness.device("operator", &[Scope::UiOperate]).await;
    let id = uuid::Uuid::new_v4().hyphenated().to_string();

    let response = harness
        .router()
        .oneshot(fetch_request(&token, &id))
        .await
        .expect("request should complete");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

// ---------------------------------------------------------------------------------------
// Round trip
// ---------------------------------------------------------------------------------------

#[tokio::test]
async fn an_uploaded_attachment_round_trips_with_identical_bytes_and_mime() {
    let harness = harness();
    let (token, device) = harness
        .device("phone", &[Scope::UiRead, Scope::UiOperate])
        .await;
    let payload = b"the quick brown fox".to_vec();

    let upload = harness
        .router()
        .oneshot(upload_request(&token, "notes.txt", "text/plain", &payload))
        .await
        .expect("upload should complete");
    assert_eq!(upload.status(), StatusCode::OK);
    let body = body_json(upload).await;
    let id = body["attachmentId"]
        .as_str()
        .expect("an attachment id is returned")
        .to_string();
    assert_eq!(body["size"], serde_json::json!(payload.len()));
    assert_eq!(body["mime"], serde_json::json!("text/plain"));

    // The client-supplied filename is DATA, never a path component.
    let stored = RemoteAttachmentRepository::get_for_device(harness.store.as_ref(), &device, &id)
        .await
        .expect("metadata should read")
        .expect("the row should exist");
    assert_eq!(stored.display_name.as_deref(), Some("notes.txt"));
    assert!(harness.root.join(&id).exists());
    assert!(
        !harness.root.join("notes.txt").exists(),
        "the display name must never become a filename"
    );

    let fetched = harness
        .router()
        .oneshot(fetch_request(&token, &id))
        .await
        .expect("fetch should complete");
    assert_eq!(fetched.status(), StatusCode::OK);
    assert_eq!(
        fetched
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("text/plain")
    );
    assert_eq!(body_bytes(fetched).await, payload);
}

#[tokio::test]
async fn a_cross_device_fetch_is_indistinguishable_from_a_missing_attachment() {
    let harness = harness();
    let (owner_token, _) = harness
        .device("owner", &[Scope::UiRead, Scope::UiOperate])
        .await;
    let (other_token, _) = harness.device("other", &[Scope::UiRead]).await;

    let upload = harness
        .router()
        .oneshot(upload_request(
            &owner_token,
            "a.txt",
            "text/plain",
            b"secret",
        ))
        .await
        .expect("upload should complete");
    let id = body_json(upload).await["attachmentId"]
        .as_str()
        .expect("an id is returned")
        .to_string();

    let cross = harness
        .router()
        .oneshot(fetch_request(&other_token, &id))
        .await
        .expect("fetch should complete");
    assert_eq!(cross.status(), StatusCode::NOT_FOUND);

    let missing = harness
        .router()
        .oneshot(fetch_request(
            &other_token,
            &uuid::Uuid::new_v4().hyphenated().to_string(),
        ))
        .await
        .expect("fetch should complete");
    assert_eq!(
        missing.status(),
        StatusCode::NOT_FOUND,
        "a cross-device read must not be distinguishable from a miss"
    );
}

/// A traversal attempt in the only client-controlled path field is rejected outright.
#[tokio::test]
async fn a_traversal_attempt_in_the_fetch_id_is_rejected_without_filesystem_access() {
    let harness = harness();
    let (token, _) = harness.device("phone", &[Scope::UiRead]).await;

    for hostile in ["..", "not-a-uuid", "%2e%2e"] {
        let response = harness
            .router()
            .oneshot(fetch_request(&token, hostile))
            .await
            .expect("request should complete");
        assert_eq!(
            response.status(),
            StatusCode::NOT_FOUND,
            "{hostile:?} must be refused"
        );
    }
    assert!(
        !harness.root.exists(),
        "ABSENCE: a rejected id must not create or read the storage root"
    );
}

/// A filename carrying traversal must not steer the write; only the minted UUID does.
#[tokio::test]
async fn a_hostile_upload_filename_never_becomes_a_path_component() {
    let harness = harness();
    let (token, _) = harness
        .device("phone", &[Scope::UiRead, Scope::UiOperate])
        .await;

    let upload = harness
        .router()
        .oneshot(upload_request(
            &token,
            "../../../etc/passwd",
            "text/plain",
            b"payload",
        ))
        .await
        .expect("upload should complete");
    assert_eq!(upload.status(), StatusCode::OK);
    let id = body_json(upload).await["attachmentId"]
        .as_str()
        .expect("an id is returned")
        .to_string();

    assert!(is_safe_attachment_id(&id));
    let children: Vec<_> = std::fs::read_dir(&harness.root)
        .expect("root should exist")
        .map(|entry| entry.expect("entry should read").file_name())
        .collect();
    assert_eq!(
        children,
        vec![std::ffi::OsString::from(&id)],
        "the storage root must contain exactly the minted uuid"
    );
}

// ---------------------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------------------

#[tokio::test]
async fn an_over_cap_upload_is_rejected_before_any_disk_write() {
    let harness = harness();
    let (token, device) = harness
        .device("phone", &[Scope::UiRead, Scope::UiOperate])
        .await;

    let payload = vec![b'x'; REMOTE_ATTACHMENT_MAX_BYTES + 4_096];
    let response = harness
        .router()
        .oneshot(upload_request(
            &token,
            "big.bin",
            "application/octet-stream",
            &payload,
        ))
        .await
        .expect("request should complete");

    assert!(
        response.status() == StatusCode::PAYLOAD_TOO_LARGE
            || response.status() == StatusCode::BAD_REQUEST,
        "an over-cap upload must be refused, got {}",
        response.status()
    );
    assert_eq!(
        RemoteAttachmentRepository::device_usage_bytes(harness.store.as_ref(), &device)
            .await
            .expect("usage should read"),
        0,
        "ABSENCE: nothing may be persisted for a rejected upload"
    );
}

#[tokio::test]
async fn an_upload_from_a_device_at_its_quota_is_refused_before_any_byte_is_written() {
    let harness = harness();
    let (token, device) = harness
        .device("phone", &[Scope::UiRead, Scope::UiOperate])
        .await;

    // Seed the device at its ceiling without writing blobs.
    RemoteAttachmentRepository::record(
        harness.store.as_ref(),
        RemoteAttachment {
            id: uuid::Uuid::new_v4().hyphenated().to_string(),
            device_id: device.clone(),
            display_name: None,
            mime: "application/octet-stream".to_string(),
            size: REMOTE_ATTACHMENT_DEVICE_QUOTA_BYTES,
            created_at: "2026-07-28T10:00:00.000Z".to_string(),
        },
    )
    .await
    .expect("quota row should seed");

    let response = harness
        .router()
        .oneshot(upload_request(&token, "a.txt", "text/plain", b"more"))
        .await
        .expect("request should complete");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(
        !harness.root.exists(),
        "ABSENCE: a quota refusal must not touch the filesystem"
    );
    assert_eq!(
        RemoteAttachmentRepository::device_usage_bytes(harness.store.as_ref(), &device)
            .await
            .expect("usage should read"),
        REMOTE_ATTACHMENT_DEVICE_QUOTA_BYTES,
        "usage must be unchanged"
    );
}

#[tokio::test]
async fn quota_is_per_device_so_one_device_cannot_exhaust_anothers_allowance() {
    let harness = harness();
    let (_, saturated) = harness
        .device("saturated", &[Scope::UiRead, Scope::UiOperate])
        .await;
    let (token, fresh) = harness
        .device("fresh", &[Scope::UiRead, Scope::UiOperate])
        .await;

    RemoteAttachmentRepository::record(
        harness.store.as_ref(),
        RemoteAttachment {
            id: uuid::Uuid::new_v4().hyphenated().to_string(),
            device_id: saturated,
            display_name: None,
            mime: "application/octet-stream".to_string(),
            size: REMOTE_ATTACHMENT_DEVICE_QUOTA_BYTES,
            created_at: "2026-07-28T10:00:00.000Z".to_string(),
        },
    )
    .await
    .expect("quota row should seed");

    let response = harness
        .router()
        .oneshot(upload_request(&token, "a.txt", "text/plain", b"hello"))
        .await
        .expect("request should complete");
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "another device's usage must not consume this device's quota"
    );
    assert_eq!(
        RemoteAttachmentRepository::device_usage_bytes(harness.store.as_ref(), &fresh)
            .await
            .expect("usage should read"),
        5
    );
}

#[tokio::test]
async fn concurrent_uploads_cannot_both_reserve_more_than_the_devices_remaining_quota() {
    let harness = harness();
    let (token, device) = harness
        .device("phone", &[Scope::UiRead, Scope::UiOperate])
        .await;
    let payload = vec![b'x'; 1024 * 1024];
    let seeded = REMOTE_ATTACHMENT_DEVICE_QUOTA_BYTES - (payload.len() as i64 * 3 / 2);

    RemoteAttachmentRepository::record(
        harness.store.as_ref(),
        RemoteAttachment {
            id: uuid::Uuid::new_v4().hyphenated().to_string(),
            device_id: device.clone(),
            display_name: None,
            mime: "application/octet-stream".to_string(),
            size: seeded,
            created_at: "2026-07-28T10:00:00.000Z".to_string(),
        },
    )
    .await
    .expect("baseline quota row should seed");

    let first = harness.router().oneshot(upload_request(
        &token,
        "first.bin",
        "application/octet-stream",
        &payload,
    ));
    let second = harness.router().oneshot(upload_request(
        &token,
        "second.bin",
        "application/octet-stream",
        &payload,
    ));
    let (first, second) = tokio::join!(first, second);
    let statuses = [
        first.expect("first request should complete").status(),
        second.expect("second request should complete").status(),
    ];

    assert_eq!(
        statuses
            .iter()
            .filter(|status| **status == StatusCode::OK)
            .count(),
        1,
        "exactly one concurrent reservation may fit: {statuses:?}"
    );
    assert_eq!(
        statuses
            .iter()
            .filter(|status| **status == StatusCode::FORBIDDEN)
            .count(),
        1,
        "the losing upload must fail closed on quota: {statuses:?}"
    );
    assert_eq!(
        RemoteAttachmentRepository::device_usage_bytes(harness.store.as_ref(), &device)
            .await
            .expect("usage should read"),
        seeded + payload.len() as i64,
        "only the winning upload may reserve quota"
    );
}

// ---------------------------------------------------------------------------------------
// Fail-closed wiring
// ---------------------------------------------------------------------------------------

#[tokio::test]
async fn a_router_without_attachment_storage_refuses_instead_of_guessing_a_directory() {
    let harness = harness();
    let (token, _) = harness
        .device("phone", &[Scope::UiRead, Scope::UiOperate])
        .await;

    let response = harness
        .router_without_attachments()
        .oneshot(upload_request(&token, "a.txt", "text/plain", b"hello"))
        .await
        .expect("request should complete");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert!(!harness.root.exists());
}

#[tokio::test]
async fn attachment_routes_require_a_bearer_token() {
    let harness = harness();
    let response = harness
        .router()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(ATTACHMENT_UPLOAD_PATH)
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={BOUNDARY}"),
                )
                .body(Body::from(multipart_body("a.txt", "text/plain", b"hello")))
                .expect("request should build"),
        )
        .await
        .expect("request should complete");

    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "attachment ingress must sit behind the bearer check like every other route"
    );
}
