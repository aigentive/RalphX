// RemoteEnvironmentService tests: pairing against a mock host, the staged
// add/remove machines, the P-27 partial-failure reconciler matrix, the P-26
// active-environment binding, and the P-18 no-token-to-JS proof.

use std::sync::{Arc, Mutex as StdMutex};

use async_trait::async_trait;
use ralphx_remote_protocol::{EnvironmentDescriptor, Scope, PROTOCOL_VERSION};

use super::*;
use crate::application::remote_event_relay::NoopFrameSink;
use crate::domain::entities::remote_environment::RemoteEnvironmentStatus;
use crate::infrastructure::memory::{MemoryRemoteEnvironmentRepository, MemorySecretStore};
use crate::infrastructure::remote_host_client::{
    MockRemoteHostClient, PairWireResponse, RecordedHostCall, RemoteHostClientError,
};
use crate::infrastructure::remote_ws_client::{
    MockRemoteWsClient, MockRemoteWsConnection, MockRemoteWsHandle, RemoteWsClient,
};
use ralphx_remote_protocol::ServerFrame;

const HOST_URL: &str = "https://mac-studio.tailnet.ts.net";
const HOST_URL_DIRECT: &str = "http://100.101.102.103:3849";
const TOKEN: &str = "rxd_live_0123456789abcdef";

fn descriptor(environment_id: &str) -> EnvironmentDescriptor {
    EnvironmentDescriptor {
        environment_id: environment_id.to_string(),
        app_version: "0.81.0".to_string(),
        protocol_version: PROTOCOL_VERSION,
        min_client_protocol: PROTOCOL_VERSION,
        platform: "macos".to_string(),
    }
}

fn pair_response(environment_id: &str) -> PairWireResponse {
    PairWireResponse {
        device_token: TOKEN.to_string(),
        device_id: "device-1".to_string(),
        scopes: vec![Scope::UiRead, Scope::UiOperate],
        environment_id: environment_id.to_string(),
        protocol_version: Some(PROTOCOL_VERSION),
    }
}

struct Fixture {
    repo: Arc<MemoryRemoteEnvironmentRepository>,
    secrets: Arc<MemorySecretStore>,
    host: Arc<MockRemoteHostClient>,
    ws: Arc<MockRemoteWsClient>,
    relay: Arc<RemoteEventRelay>,
    service: RemoteEnvironmentService,
}

fn fixture() -> Fixture {
    fixture_with_host(MockRemoteHostClient::new(
        descriptor("env-1"),
        pair_response("env-1"),
    ))
}

/// A relay over a scripted mock socket, for services that never dial in a test.
fn test_relay() -> Arc<RemoteEventRelay> {
    Arc::new(RemoteEventRelay::new(
        Arc::new(MockRemoteWsClient::new()),
        Arc::new(NoopFrameSink),
    ))
}

fn fixture_with_host(host: MockRemoteHostClient) -> Fixture {
    let repo = Arc::new(MemoryRemoteEnvironmentRepository::new());
    let secrets = Arc::new(MemorySecretStore::new());
    let host = Arc::new(host);
    let ws = Arc::new(MockRemoteWsClient::new());
    let relay = Arc::new(RemoteEventRelay::new(
        Arc::clone(&ws) as Arc<dyn RemoteWsClient>,
        Arc::new(NoopFrameSink),
    ));
    let service = RemoteEnvironmentService::new(
        Arc::clone(&repo) as Arc<dyn crate::domain::repositories::RemoteEnvironmentRepository>,
        Arc::clone(&secrets) as Arc<dyn crate::domain::services::SecretStore>,
        Arc::clone(&host) as Arc<dyn crate::infrastructure::remote_host_client::RemoteHostClient>,
        Arc::clone(&relay),
    );
    Fixture {
        repo,
        secrets,
        host,
        ws,
        relay,
        service,
    }
}

struct FailingRemoteEnvironmentRepository {
    inner: Arc<MemoryRemoteEnvironmentRepository>,
    fail_list: bool,
    fail_get: bool,
    fail_set_status: bool,
    fail_delete: bool,
}

impl FailingRemoteEnvironmentRepository {
    fn new(inner: Arc<MemoryRemoteEnvironmentRepository>) -> Self {
        Self {
            inner,
            fail_list: false,
            fail_get: false,
            fail_set_status: false,
            fail_delete: false,
        }
    }

    fn database_error<T>() -> crate::error::AppResult<T> {
        Err(crate::error::AppError::Database("boom".to_string()))
    }
}

#[async_trait]
impl crate::domain::repositories::RemoteEnvironmentRepository
    for FailingRemoteEnvironmentRepository
{
    async fn upsert_paired(
        &self,
        params: crate::domain::repositories::UpsertPairedEnvironment,
    ) -> crate::error::AppResult<RemoteEnvironment> {
        self.inner.upsert_paired(params).await
    }

    async fn get(
        &self,
        id: &crate::domain::entities::remote_environment::RemoteEnvironmentId,
    ) -> crate::error::AppResult<Option<RemoteEnvironment>> {
        if self.fail_get {
            Self::database_error()
        } else {
            self.inner.get(id).await
        }
    }

    async fn get_by_environment_id(
        &self,
        environment_id: &str,
    ) -> crate::error::AppResult<Option<RemoteEnvironment>> {
        self.inner.get_by_environment_id(environment_id).await
    }

    async fn list(&self) -> crate::error::AppResult<Vec<RemoteEnvironment>> {
        if self.fail_list {
            Self::database_error()
        } else {
            self.inner.list().await
        }
    }

    async fn set_status(
        &self,
        id: &crate::domain::entities::remote_environment::RemoteEnvironmentId,
        status: RemoteEnvironmentStatus,
    ) -> crate::error::AppResult<()> {
        if self.fail_set_status {
            Self::database_error()
        } else {
            self.inner.set_status(id, status).await
        }
    }

    async fn delete(
        &self,
        id: &crate::domain::entities::remote_environment::RemoteEnvironmentId,
    ) -> crate::error::AppResult<()> {
        if self.fail_delete {
            Self::database_error()
        } else {
            self.inner.delete(id).await
        }
    }

    async fn touch_last_connected(
        &self,
        id: &crate::domain::entities::remote_environment::RemoteEnvironmentId,
        timestamp: &str,
    ) -> crate::error::AppResult<()> {
        self.inner.touch_last_connected(id, timestamp).await
    }
}

fn service_with_repo(
    repo: Arc<dyn crate::domain::repositories::RemoteEnvironmentRepository>,
) -> RemoteEnvironmentService {
    RemoteEnvironmentService::new(
        repo,
        Arc::new(MemorySecretStore::new()),
        Arc::new(MockRemoteHostClient::new(
            descriptor("env-1"),
            pair_response("env-1"),
        )),
        test_relay(),
    )
}

// Trait methods on the concrete test doubles resolve through the imports the
// service module already provides via `use super::*` (RemoteEnvironmentRepository,
// SecretStore) — no extra trait imports needed here.

// ============================================================================
// Pairing against the mock host
// ============================================================================

#[test]
fn every_remote_environment_error_has_its_stable_code() {
    let cases = vec![
        (RemoteEnvironmentError::NotConnected, "NOT_CONNECTED"),
        (
            RemoteEnvironmentError::NotActiveEnvironment {
                requested: "a".into(),
                active: "b".into(),
            },
            "REMOTE_FORBIDDEN",
        ),
        (
            RemoteEnvironmentError::UnknownEnvironment("a".into()),
            "REMOTE_COMMAND_UNAVAILABLE",
        ),
        (
            RemoteEnvironmentError::EnvironmentNotUsable("a".into(), "pending_add"),
            "REMOTE_COMMAND_UNAVAILABLE",
        ),
        (RemoteEnvironmentError::LocalEnvironment, "REMOTE_FORBIDDEN"),
        (
            RemoteEnvironmentError::InvalidUrl("bad".into()),
            "INVALID_PAIRING_URL",
        ),
        (
            RemoteEnvironmentError::VersionSkew {
                host_min_client: 2,
                client: 1,
            },
            "REMOTE_VERSION_MISMATCH",
        ),
        (
            RemoteEnvironmentError::IdentityMismatch {
                descriptor: "a".into(),
                response: "b".into(),
            },
            "HOST_IDENTITY_MISMATCH",
        ),
        (
            RemoteEnvironmentError::PairRejected("bad".into()),
            "PAIRING_REJECTED",
        ),
        (
            RemoteEnvironmentError::Unreachable("offline".into()),
            "REMOTE_UNREACHABLE",
        ),
        (
            RemoteEnvironmentError::Transport {
                code: ralphx_remote_protocol::ErrorCode::RemoteRequestIdReused,
                message: "duplicate".into(),
            },
            "REMOTE_REQUEST_ID_REUSED",
        ),
        (
            RemoteEnvironmentError::InvalidFetchRequest("bad".into()),
            "REMOTE_COMMAND_UNAVAILABLE",
        ),
        (
            RemoteEnvironmentError::MissingCredential("a".into()),
            "REMOTE_UNAUTHORIZED",
        ),
        (
            RemoteEnvironmentError::Secret(SecretStoreError::Unavailable("locked".into())),
            "SECRET_STORE_UNAVAILABLE",
        ),
        (
            RemoteEnvironmentError::Db(crate::error::AppError::Database("boom".into())),
            "DATABASE_ERROR",
        ),
    ];

    for (error, expected) in cases {
        assert_eq!(error.code(), expected, "{error:?}");
    }
}

#[tokio::test]
async fn database_read_failures_fail_closed_or_propagate_by_contract() {
    let list_inner = Arc::new(MemoryRemoteEnvironmentRepository::new());
    let mut list_repo = FailingRemoteEnvironmentRepository::new(list_inner);
    list_repo.fail_list = true;
    let list_service = service_with_repo(Arc::new(list_repo));
    assert_eq!(
        list_service.reconcile_on_startup().await,
        RemoteEnvironmentReconcileReport::default()
    );

    let get_inner = Arc::new(MemoryRemoteEnvironmentRepository::new());
    let mut get_repo = FailingRemoteEnvironmentRepository::new(get_inner);
    get_repo.fail_get = true;
    let get_service = service_with_repo(Arc::new(get_repo));
    assert!(matches!(
        get_service.set_active_environment("env-id").await,
        Err(RemoteEnvironmentError::Db(_))
    ));
    assert!(matches!(
        get_service
            .fetch("env-id", RemoteFetchCall::get(REMOTE_HEALTH_PATH))
            .await,
        Err(RemoteEnvironmentError::Db(_))
    ));
}

#[tokio::test]
async fn reconcile_defers_when_a_required_row_delete_fails() {
    for status in [
        RemoteEnvironmentStatus::PendingAdd,
        RemoteEnvironmentStatus::PendingDelete,
    ] {
        let inner = Arc::new(MemoryRemoteEnvironmentRepository::new());
        let env = inner
            .upsert_paired(crate::domain::repositories::UpsertPairedEnvironment {
                environment_id: format!("env-{status:?}"),
                name: "Mac".to_string(),
                url: HOST_URL.to_string(),
                scopes: vec![Scope::UiRead],
                protocol_version: PROTOCOL_VERSION,
            })
            .await
            .expect("seed");
        inner.set_status(&env.id, status).await.expect("status");
        let mut repo = FailingRemoteEnvironmentRepository::new(Arc::clone(&inner));
        repo.fail_delete = true;
        let service = service_with_repo(Arc::new(repo));

        let report = service.reconcile_on_startup().await;

        assert_eq!(report.deferred, vec![env.id.as_str().to_string()]);
        assert!(inner.get(&env.id).await.expect("get").is_some());
    }
}

#[tokio::test]
async fn pair_success_lands_an_active_row_with_the_token_in_the_secret_store() {
    let f = fixture();

    let env = f
        .service
        .pair(HOST_URL, "rxp_code", "Mac Studio")
        .await
        .expect("pairing should succeed");

    assert_eq!(env.status, RemoteEnvironmentStatus::Active);
    assert_eq!(env.environment_id, "env-1");
    let stored = f
        .repo
        .get(&env.id)
        .await
        .expect("get should succeed")
        .expect("row should exist");
    assert_eq!(stored.status, RemoteEnvironmentStatus::Active);
    assert_eq!(
        f.secrets
            .get_secret(&env.token_secret_ref)
            .await
            .expect("secret read should succeed")
            .as_deref(),
        Some(TOKEN)
    );
}

#[tokio::test]
async fn pair_bad_code_leaves_no_row_and_no_secret() {
    let f = fixture();
    *f.host.pair_response.lock().expect("mock") = Err(RemoteHostClientError::Rejected {
        status: 401,
        message: "invalid pairing code".to_string(),
    });

    let error = f
        .service
        .pair(HOST_URL, "rxp_wrong", "Mac Studio")
        .await
        .expect_err("bad code must fail");
    assert!(matches!(error, RemoteEnvironmentError::PairRejected(_)));
    assert!(f.repo.list().await.expect("list").is_empty());
    // No dangling secret: the Keychain write only happens after a row exists.
    assert!(f
        .secrets
        .get_secret("remote-env:any:token")
        .await
        .expect("secret read")
        .is_none());
}

#[tokio::test]
async fn pair_version_skew_aborts_before_the_pair_exchange() {
    let f = fixture();
    {
        let mut descriptor_slot = f.host.descriptor.lock().expect("mock");
        let mut skewed = descriptor("env-1");
        skewed.min_client_protocol = PROTOCOL_VERSION + 1;
        *descriptor_slot = Ok(skewed);
    }

    let error = f
        .service
        .pair(HOST_URL, "rxp_code", "Mac Studio")
        .await
        .expect_err("skew must abort");
    assert!(matches!(
        error,
        RemoteEnvironmentError::VersionSkew {
            host_min_client, ..
        } if host_min_client == PROTOCOL_VERSION + 1
    ));
    // The abort happened at the descriptor step: no pair call ever went out.
    let calls = f.host.recorded_calls();
    assert!(calls
        .iter()
        .all(|call| !matches!(call, RecordedHostCall::Pair { .. })));
    assert!(f.repo.list().await.expect("list").is_empty());
}

#[tokio::test]
async fn pair_identity_mismatch_fails_closed_without_a_row() {
    let f = fixture();
    *f.host.pair_response.lock().expect("mock") = Ok(pair_response("someone-else"));

    let error = f
        .service
        .pair(HOST_URL, "rxp_code", "Mac Studio")
        .await
        .expect_err("identity mismatch must fail");
    assert!(matches!(
        error,
        RemoteEnvironmentError::IdentityMismatch { .. }
    ));
    assert!(f.repo.list().await.expect("list").is_empty());
}

#[tokio::test]
async fn pair_rejects_non_http_urls_before_any_network_call() {
    let f = fixture();

    let error = f
        .service
        .pair("file:///etc/passwd", "rxp_code", "Mac Studio")
        .await
        .expect_err("non-http scheme must be rejected");
    assert!(matches!(error, RemoteEnvironmentError::InvalidUrl(_)));
    assert!(f.host.recorded_calls().is_empty());
}

#[tokio::test]
async fn pairing_the_same_host_via_a_second_url_merges_into_one_environment() {
    let f = fixture();

    let first = f
        .service
        .pair(HOST_URL, "rxp_code", "Mac Studio")
        .await
        .expect("first pairing should succeed");
    let second = f
        .service
        .pair(HOST_URL_DIRECT, "rxp_code2", "Mac Studio")
        .await
        .expect("second pairing should merge");

    let all = f.repo.list().await.expect("list");
    assert_eq!(all.len(), 1, "one host identity → one environment");
    assert_eq!(second.id, first.id);
    assert_eq!(second.base_url, HOST_URL);
    assert_eq!(second.candidate_urls, vec![HOST_URL_DIRECT.to_string()]);
    // The refreshed token overwrote the SAME Keychain entry.
    assert_eq!(second.token_secret_ref, first.token_secret_ref);
    assert_eq!(
        f.secrets
            .get_secret(&second.token_secret_ref)
            .await
            .expect("secret read")
            .as_deref(),
        Some(TOKEN)
    );
}

#[tokio::test]
async fn re_pairing_revokes_the_replaced_token_after_the_new_one_is_installed() {
    let f = fixture();
    f.service
        .pair(HOST_URL, "rxp_code", "Mac Studio")
        .await
        .expect("first pairing should succeed");

    // The host mints a fresh token for the re-pair.
    let second_token = "rxd_live_fedcba9876543210";
    {
        let mut pair_slot = f.host.pair_response.lock().expect("mock");
        let mut refreshed = pair_response("env-1");
        refreshed.device_token = second_token.to_string();
        *pair_slot = Ok(refreshed);
    }

    let env = f
        .service
        .pair(HOST_URL_DIRECT, "rxp_code2", "Mac Studio")
        .await
        .expect("re-pair should succeed");

    // The Keychain now holds the fresh bearer…
    assert_eq!(
        f.secrets
            .get_secret(&env.token_secret_ref)
            .await
            .expect("secret read")
            .as_deref(),
        Some(second_token)
    );
    // …and the replaced bearer was revoked host-side (best effort), so the old
    // device does not linger valid-but-unreferenced.
    assert!(f.host.recorded_calls().iter().any(|call| matches!(
        call,
        RecordedHostCall::Revoke { token, .. } if token == TOKEN
    )));
}

// ============================================================================
// P-18 — the token never reaches JS-serializable surfaces
// ============================================================================

#[tokio::test]
async fn no_pairing_surface_serializes_the_raw_token() {
    let f = fixture();

    let env = f
        .service
        .pair(HOST_URL, "rxp_code", "Mac Studio")
        .await
        .expect("pairing should succeed");
    let listed = f.service.list().await.expect("list should succeed");

    let env_json = serde_json::to_string(&env).expect("environment should serialize");
    let list_json = serde_json::to_string(&listed).expect("list should serialize");
    assert!(
        !env_json.contains("rxd_live_"),
        "pair result must never carry the device token: {env_json}"
    );
    assert!(
        !list_json.contains("rxd_live_"),
        "list result must never carry the device token: {list_json}"
    );
}

// ============================================================================
// P-26 — active-environment binding
// ============================================================================

/// Pairs and activates two environments (env-1 and env-2) and returns their row ids.
async fn two_paired_environments(f: &Fixture) -> (String, String) {
    let env_a = f
        .service
        .pair(HOST_URL, "rxp_code", "Mac Studio")
        .await
        .expect("pair A");
    *f.host.descriptor.lock().expect("mock") = Ok(descriptor("env-2"));
    *f.host.pair_response.lock().expect("mock") = Ok(pair_response("env-2"));
    let env_b = f
        .service
        .pair("https://mini.tailnet.ts.net", "rxp_code2", "Mac mini")
        .await
        .expect("pair B");
    (env_a.id.as_str().to_string(), env_b.id.as_str().to_string())
}

#[tokio::test]
async fn invoke_for_a_non_active_environment_is_rejected() {
    let f = fixture();
    let (a, b) = two_paired_environments(&f).await;
    f.service
        .set_active_environment(&a)
        .await
        .expect("activating A should succeed");

    let error = f
        .service
        .invoke(&b, "req-1", "health_check", serde_json::json!({}))
        .await
        .expect_err("invoke for background env must be rejected");
    assert!(matches!(
        &error,
        RemoteEnvironmentError::NotActiveEnvironment { requested, active }
            if requested == &b && active == &a
    ));
    assert_eq!(error.code(), "REMOTE_FORBIDDEN");
}

#[tokio::test]
async fn invoke_for_the_active_environment_dispatches_with_the_stored_bearer() {
    let f = fixture();
    let (a, _b) = two_paired_environments(&f).await;
    f.service
        .set_active_environment(&a)
        .await
        .expect("activating A should succeed");
    f.host
        .script_invoke(200, r#"{"ok":true,"result":{"status":"ok"}}"#);

    let outcome = f
        .service
        .invoke(&a, "req-1", "health_check", serde_json::json!({}))
        .await
        .expect("an authorized dispatch should reach the host");

    assert_eq!(
        outcome,
        RemoteInvokeOutcome::Ok {
            result: serde_json::json!({"status": "ok"})
        }
    );
    // The client-minted requestId must arrive verbatim: the host binds it to a
    // cmd+args hash for mutation dedup (§3.3), so re-minting would silently disarm
    // the client's only defence against a double-applied mutation.
    let dispatched = f
        .host
        .recorded_calls()
        .into_iter()
        .find_map(|call| match call {
            RecordedHostCall::Invoke { token, request, .. } => Some((token, request)),
            _ => None,
        })
        .expect("the host should have seen one dispatch");
    assert_eq!(dispatched.0, TOKEN, "the bearer comes from the Keychain");
    assert_eq!(dispatched.1.request_id, "req-1");
    assert_eq!(dispatched.1.cmd, "health_check");
}

#[tokio::test]
async fn a_host_command_error_is_not_a_transport_error() {
    let f = fixture();
    let (a, _b) = two_paired_environments(&f).await;
    f.service
        .set_active_environment(&a)
        .await
        .expect("activating A should succeed");
    f.host
        .script_invoke(200, r#"{"ok":false,"error":"task not found"}"#);

    let outcome = f
        .service
        .invoke(&a, "req-1", "get_task", serde_json::json!({"id": "nope"}))
        .await
        .expect("a command error must resolve, so JS can reject with the value itself");

    assert_eq!(
        outcome,
        RemoteInvokeOutcome::CommandError {
            error: serde_json::json!("task not found")
        }
    );
}

/// A host that answers with the bare command result (no `{ok}` envelope) is still a
/// valid answer — the client must not pin itself to a body shape the host facade is
/// still landing.
#[tokio::test]
async fn a_bare_result_body_is_read_as_success() {
    let f = fixture();
    let (a, _b) = two_paired_environments(&f).await;
    f.service
        .set_active_environment(&a)
        .await
        .expect("activate");
    f.host.script_invoke(200, r#"[{"id":"task-1"}]"#);

    let outcome = f
        .service
        .invoke(&a, "req-1", "list_tasks", serde_json::json!({}))
        .await
        .expect("a bare result body is a success");
    assert_eq!(
        outcome,
        RemoteInvokeOutcome::Ok {
            result: serde_json::json!([{"id": "task-1"}])
        }
    );
}

/// Every code the host can name must survive the trip to JS as itself — a mapping
/// that collapsed any of them would turn a scope refusal into a retryable-looking
/// unreachable, or an unknown outcome into a safe-to-resend failure.
#[tokio::test]
async fn host_error_bodies_map_to_their_taxonomy_codes() {
    let cases: &[(u16, &str, &str)] = &[
        (
            404,
            "REMOTE_COMMAND_UNAVAILABLE",
            "REMOTE_COMMAND_UNAVAILABLE",
        ),
        (403, "REMOTE_FORBIDDEN", "REMOTE_FORBIDDEN"),
        (401, "REMOTE_UNAUTHORIZED", "REMOTE_UNAUTHORIZED"),
        (426, "REMOTE_VERSION_MISMATCH", "REMOTE_VERSION_MISMATCH"),
        (
            409,
            "REMOTE_REQUEST_IN_PROGRESS",
            "REMOTE_REQUEST_IN_PROGRESS",
        ),
        (409, "REMOTE_REQUEST_ID_REUSED", "REMOTE_REQUEST_ID_REUSED"),
        (504, "REMOTE_TIMEOUT_UNKNOWN", "REMOTE_TIMEOUT_UNKNOWN"),
        (502, "REMOTE_UNREACHABLE", "REMOTE_UNREACHABLE"),
    ];
    for (status, body_code, expected) in cases {
        let f = fixture();
        let (a, _b) = two_paired_environments(&f).await;
        f.service
            .set_active_environment(&a)
            .await
            .expect("activate");
        f.host.script_invoke(
            *status,
            format!(r#"{{"code":"{body_code}","message":"nope"}}"#),
        );

        let error = f
            .service
            .invoke(&a, "req-1", "list_tasks", serde_json::json!({}))
            .await
            .expect_err("a host refusal is a transport error");
        assert_eq!(
            error.code(),
            *expected,
            "status {status} / body {body_code}"
        );
    }
}

/// A host that refuses without a typed body still produces a typed code — the status
/// alone must never degrade into an untyped string on the IPC boundary.
#[tokio::test]
async fn an_untyped_host_refusal_maps_by_status() {
    let f = fixture();
    let (a, _b) = two_paired_environments(&f).await;
    f.service
        .set_active_environment(&a)
        .await
        .expect("activate");
    f.host.script_invoke(403, "forbidden");

    let error = f
        .service
        .invoke(&a, "req-1", "list_tasks", serde_json::json!({}))
        .await
        .expect_err("403 is a refusal");
    assert_eq!(error.code(), "REMOTE_FORBIDDEN");
}

/// A timed-out dispatch is an UNKNOWN outcome, never "unreachable": the request WAS
/// sent, so the mutation may already be committed host-side and a caller that read
/// "unreachable" could resend it (§3.3).
#[tokio::test]
async fn a_dispatch_timeout_is_an_unknown_outcome_not_unreachable() {
    let f = fixture();
    let (a, _b) = two_paired_environments(&f).await;
    f.service
        .set_active_environment(&a)
        .await
        .expect("activate");
    f.host
        .script_invoke_error(RemoteHostClientError::Timeout("no answer after 30s".into()));

    let error = f
        .service
        .invoke(&a, "req-1", "send_agent_message", serde_json::json!({}))
        .await
        .expect_err("a timeout is a transport error");
    assert_eq!(error.code(), "REMOTE_TIMEOUT_UNKNOWN");
}

/// A registry row without a Keychain secret must not produce an unauthenticated
/// request: fail closed with the code the supervisor parks `blocked` on.
#[tokio::test]
async fn a_missing_bearer_fails_closed_before_any_request() {
    let f = fixture();
    let (a, _b) = two_paired_environments(&f).await;
    f.service
        .set_active_environment(&a)
        .await
        .expect("activate");
    let env = f
        .repo
        .get(&crate::domain::entities::remote_environment::RemoteEnvironmentId::from_string(&a))
        .await
        .expect("registry read")
        .expect("row exists");
    f.secrets
        .delete_secret(&env.token_secret_ref)
        .await
        .expect("secret delete");

    let error = f
        .service
        .invoke(&a, "req-1", "list_tasks", serde_json::json!({}))
        .await
        .expect_err("no bearer, no request");
    assert_eq!(error.code(), "REMOTE_UNAUTHORIZED");
    assert!(
        !f.host
            .recorded_calls()
            .iter()
            .any(|call| matches!(call, RecordedHostCall::Invoke { .. })),
        "nothing may leave this Mac without a credential"
    );
}

/// The bearer is attached AFTER path validation, so an unvalidated target would aim
/// an authenticated request at an attacker-chosen origin.
#[tokio::test]
async fn unsafe_fetch_targets_are_refused_before_a_bearer_is_read() {
    let cases = [
        "https://evil.example/api/tasks",
        "//evil.example/api/tasks",
        "/api/../remote/v1/admin/devices",
        "api/tasks",
        "",
    ];
    for path in cases {
        let f = fixture();
        let (a, _b) = two_paired_environments(&f).await;
        f.service
            .set_active_environment(&a)
            .await
            .expect("activate");

        let error = f
            .service
            .fetch(&a, RemoteFetchCall::get(path))
            .await
            .expect_err("unsafe fetch target must be refused");
        assert!(
            matches!(error, RemoteEnvironmentError::InvalidFetchRequest(_)),
            "path {path:?} produced {error:?}"
        );
        assert!(
            !f.host
                .recorded_calls()
                .iter()
                .any(|call| matches!(call, RecordedHostCall::Fetch { .. })),
            "path {path:?} must not reach the wire"
        );
    }
}

/// The webview cannot smuggle headers onto a bearer-carrying request.
#[tokio::test]
async fn only_allowlisted_headers_are_forwarded() {
    let f = fixture();
    let (a, _b) = two_paired_environments(&f).await;
    f.service
        .set_active_environment(&a)
        .await
        .expect("activate");

    let error = f
        .service
        .fetch(
            &a,
            RemoteFetchCall {
                path: "/api/tasks".to_string(),
                method: "POST".to_string(),
                headers: vec![("Authorization".to_string(), "Bearer stolen".to_string())],
                body: None,
            },
        )
        .await
        .expect_err("Authorization may never be caller-supplied");
    assert!(matches!(
        error,
        RemoteEnvironmentError::InvalidFetchRequest(_)
    ));
}

/// A non-2xx from a remounted route is DATA: `backendFetch` rebuilds a `Response` and
/// the call site reads its own error body, exactly as it does locally.
#[tokio::test]
async fn a_non_success_fetch_status_is_returned_not_raised() {
    let f = fixture();
    let (a, _b) = two_paired_environments(&f).await;
    f.service
        .set_active_environment(&a)
        .await
        .expect("activate");
    f.host.script_fetch(422, r#"{"error":"bad input"}"#);

    let outcome = f
        .service
        .fetch(&a, RemoteFetchCall::get("/api/tasks"))
        .await
        .expect("a 422 is the endpoint's answer, not a transport failure");
    assert_eq!(outcome.status, 422);
    assert_eq!(outcome.body, r#"{"error":"bad input"}"#);
}

/// 401/403 are the exception: they describe the transport's authority, and the
/// supervisor keys on them (§6.5).
#[tokio::test]
async fn fetch_auth_refusals_lift_into_the_taxonomy() {
    for (status, expected) in [(401, "REMOTE_UNAUTHORIZED"), (403, "REMOTE_FORBIDDEN")] {
        let f = fixture();
        let (a, _b) = two_paired_environments(&f).await;
        f.service
            .set_active_environment(&a)
            .await
            .expect("activate");
        f.host.script_fetch(status, "no");

        let error = f
            .service
            .fetch(&a, RemoteFetchCall::get("/api/tasks"))
            .await
            .expect_err("an auth refusal is a transport error");
        assert_eq!(error.code(), expected);
    }
}

#[tokio::test]
async fn non_health_fetch_for_a_background_environment_is_rejected() {
    let f = fixture();
    let (a, b) = two_paired_environments(&f).await;
    f.service
        .set_active_environment(&a)
        .await
        .expect("activating A should succeed");

    let error = f
        .service
        .fetch(&b, RemoteFetchCall::get("/api/tasks"))
        .await
        .expect_err("background env must be health-only");
    assert!(matches!(
        error,
        RemoteEnvironmentError::NotActiveEnvironment { .. }
    ));
}

#[tokio::test]
async fn descriptor_probe_for_a_background_environment_succeeds() {
    let f = fixture();
    let (a, b) = two_paired_environments(&f).await;
    f.service
        .set_active_environment(&a)
        .await
        .expect("activating A should succeed");

    let outcome = f
        .service
        .fetch(
            &b,
            RemoteFetchCall::get(crate::infrastructure::remote_host_client::REMOTE_DESCRIPTOR_PATH),
        )
        .await
        .expect("health probe for a background env must be allowed");
    assert_eq!(outcome.status, 200);
    let descriptor: serde_json::Value =
        serde_json::from_str(&outcome.body).expect("descriptor body should be JSON");
    assert_eq!(descriptor["environmentId"], "env-2");
}

#[tokio::test]
async fn proxy_calls_for_the_local_environment_are_refused() {
    let f = fixture();

    let error = f
        .service
        .invoke(
            LOCAL_ENVIRONMENT_ID,
            "req-1",
            "health_check",
            serde_json::json!({}),
        )
        .await
        .expect_err("local never routes through the remote proxy");
    assert!(matches!(error, RemoteEnvironmentError::LocalEnvironment));
}

#[tokio::test]
async fn set_active_environment_rejects_unknown_and_non_active_rows() {
    let f = fixture();
    let env = f
        .service
        .pair(HOST_URL, "rxp_code", "Mac Studio")
        .await
        .expect("pairing should succeed");

    assert!(matches!(
        f.service.set_active_environment("nope").await,
        Err(RemoteEnvironmentError::UnknownEnvironment(_))
    ));

    f.repo
        .set_status(&env.id, RemoteEnvironmentStatus::PendingDelete)
        .await
        .expect("status write");
    assert!(matches!(
        f.service.set_active_environment(env.id.as_str()).await,
        Err(RemoteEnvironmentError::EnvironmentNotUsable(..))
    ));
    assert_eq!(f.service.active_environment_id().await, "local");
}

/// P-26: the mirror is not the whole authority. A row that leaves `active` — a dedup re-pair
/// demotes it to `pending_add` while its Keychain token is being replaced — must not be
/// invocable just because the mirror still names it. PR 2.2 hangs the real transport off this
/// authorization, so an invoke here would be signed with a half-installed credential.
#[tokio::test]
async fn invoke_is_refused_while_the_active_environment_is_mid_re_pair() {
    let f = fixture();
    let env = f
        .service
        .pair(HOST_URL, "rxp_code", "Mac Studio")
        .await
        .expect("pairing should succeed");
    f.service
        .set_active_environment(env.id.as_str())
        .await
        .expect("activating should succeed");
    f.repo
        .set_status(&env.id, RemoteEnvironmentStatus::PendingAdd)
        .await
        .expect("status write");

    let error = f
        .service
        .invoke(
            env.id.as_str(),
            "req-1",
            "health_check",
            serde_json::json!({}),
        )
        .await
        .expect_err("a non-active row must not be invocable");
    let probe = f
        .service
        .fetch(
            env.id.as_str(),
            RemoteFetchCall::get(crate::infrastructure::remote_host_client::REMOTE_DESCRIPTOR_PATH),
        )
        .await;

    assert!(
        matches!(error, RemoteEnvironmentError::EnvironmentNotUsable(..)),
        "expected a status refusal, got {error:?}"
    );
    assert!(
        probe.is_ok(),
        "health probes stay available while a row reconciles"
    );
}

/// Re-pairing the CURRENTLY ACTIVE environment demotes its row to `pending_add`; the mirror
/// must not keep naming it while the credential is swapped underneath.
#[tokio::test]
async fn re_pairing_the_active_environment_drops_the_active_mirror() {
    let f = fixture();
    let env = f
        .service
        .pair(HOST_URL, "rxp_code", "Mac Studio")
        .await
        .expect("first pairing should succeed");
    f.service
        .set_active_environment(env.id.as_str())
        .await
        .expect("activating should succeed");

    f.service
        .pair(HOST_URL_DIRECT, "rxp_code2", "Mac Studio")
        .await
        .expect("re-pairing the same host should succeed");

    assert_eq!(
        f.service.active_environment_id().await,
        LOCAL_ENVIRONMENT_ID,
        "the mirror falls back to local rather than pointing through a re-pair window"
    );
}

#[tokio::test]
async fn removing_the_active_environment_resets_the_mirror_to_local() {
    let f = fixture();
    let env = f
        .service
        .pair(HOST_URL, "rxp_code", "Mac Studio")
        .await
        .expect("pairing should succeed");
    f.service
        .set_active_environment(env.id.as_str())
        .await
        .expect("activation should succeed");

    f.service
        .remove(env.id.as_str())
        .await
        .expect("removal should succeed");
    assert_eq!(f.service.active_environment_id().await, "local");
}

// ============================================================================
// Staged remove ordering (P-27: revoke → Keychain → row)
// ============================================================================

#[tokio::test]
async fn remove_revokes_on_the_host_then_deletes_secret_then_row() {
    let f = fixture();
    let env = f
        .service
        .pair(HOST_URL, "rxp_code", "Mac Studio")
        .await
        .expect("pairing should succeed");

    f.service
        .remove(env.id.as_str())
        .await
        .expect("removal should succeed");

    // Revoke went out with the real bearer before local state was destroyed.
    assert!(f.host.recorded_calls().iter().any(|call| matches!(
        call,
        RecordedHostCall::Revoke { token, .. } if token == TOKEN
    )));
    assert!(f
        .secrets
        .get_secret(&env.token_secret_ref)
        .await
        .expect("secret read")
        .is_none());
    assert!(f.repo.get(&env.id).await.expect("get").is_none());
}

#[tokio::test]
async fn remove_survives_an_unreachable_host_because_revoke_is_best_effort() {
    let f = fixture();
    let env = f
        .service
        .pair(HOST_URL, "rxp_code", "Mac Studio")
        .await
        .expect("pairing should succeed");
    *f.host.revoke_response.lock().expect("mock") = Err(RemoteHostClientError::Unreachable(
        "host offline".to_string(),
    ));

    f.service
        .remove(env.id.as_str())
        .await
        .expect("removal must not require the host");
    assert!(f
        .secrets
        .get_secret(&env.token_secret_ref)
        .await
        .expect("secret read")
        .is_none());
    assert!(f.repo.get(&env.id).await.expect("get").is_none());
}

/// SecretStore decorator that fails deletes, for proving the Keychain→row ordering.
struct FailingDeleteSecretStore {
    inner: Arc<MemorySecretStore>,
    fail_delete: StdMutex<bool>,
}

#[async_trait]
impl crate::domain::services::SecretStore for FailingDeleteSecretStore {
    async fn put_secret(&self, key: &str, value: &str) -> Result<(), SecretStoreError> {
        self.inner.put_secret(key, value).await
    }

    async fn get_secret(&self, key: &str) -> Result<Option<String>, SecretStoreError> {
        self.inner.get_secret(key).await
    }

    async fn delete_secret(&self, key: &str) -> Result<(), SecretStoreError> {
        if *self.fail_delete.lock().expect("flag") {
            return Err(SecretStoreError::Unavailable("keychain locked".to_string()));
        }
        self.inner.delete_secret(key).await
    }
}

#[tokio::test]
async fn remove_keeps_the_pending_delete_row_when_the_keychain_delete_fails() {
    let repo = Arc::new(MemoryRemoteEnvironmentRepository::new());
    let inner_secrets = Arc::new(MemorySecretStore::new());
    let secrets = Arc::new(FailingDeleteSecretStore {
        inner: Arc::clone(&inner_secrets),
        fail_delete: StdMutex::new(true),
    });
    let host = Arc::new(MockRemoteHostClient::new(
        descriptor("env-1"),
        pair_response("env-1"),
    ));
    let service = RemoteEnvironmentService::new(
        Arc::clone(&repo) as _,
        Arc::clone(&secrets) as _,
        Arc::clone(&host) as _,
        test_relay(),
    );

    let env = service
        .pair(HOST_URL, "rxp_code", "Mac Studio")
        .await
        .expect("pairing should succeed");
    let error = service
        .remove(env.id.as_str())
        .await
        .expect_err("a failed Keychain delete must not report success");
    assert!(matches!(error, RemoteEnvironmentError::Secret(_)));

    // The row survives, still referencing the secret, so the reconciler can
    // finish the removal — deleting it would orphan a valid bearer.
    let row = repo
        .get(&env.id)
        .await
        .expect("get")
        .expect("row must survive");
    assert_eq!(row.status, RemoteEnvironmentStatus::PendingDelete);
    assert!(inner_secrets
        .get_secret(&env.token_secret_ref)
        .await
        .expect("secret read")
        .is_some());
}

// ============================================================================
// P-27 — startup reconciler partial-failure matrix
// ============================================================================

/// Seeds a pending_add row WITHOUT a secret: the crash point between the row
/// write and the Keychain write.
async fn seed_husk(f: &Fixture) -> RemoteEnvironment {
    f.repo
        .upsert_paired(crate::domain::repositories::UpsertPairedEnvironment {
            environment_id: "env-husk".to_string(),
            name: "Mac Studio".to_string(),
            url: HOST_URL.to_string(),
            scopes: vec![Scope::UiRead],
            protocol_version: PROTOCOL_VERSION,
        })
        .await
        .expect("seed should succeed")
}

#[tokio::test]
async fn reconciler_deletes_a_pending_add_husk_with_no_secret() {
    let f = fixture();
    let husk = seed_husk(&f).await;

    let report = f.service.reconcile_on_startup().await;

    assert_eq!(report.deleted_husks, vec![husk.id.as_str().to_string()]);
    assert!(f.repo.get(&husk.id).await.expect("get").is_none());
}

#[tokio::test]
async fn reconciler_activates_a_pending_add_row_whose_secret_validates() {
    // Crash point: after the Keychain write, before the flip to active.
    let f = fixture();
    let env = seed_husk(&f).await;
    f.secrets
        .put_secret(&env.token_secret_ref, TOKEN)
        .await
        .expect("seed secret");

    let report = f.service.reconcile_on_startup().await;

    assert_eq!(report.activated, vec![env.id.as_str().to_string()]);
    let row = f
        .repo
        .get(&env.id)
        .await
        .expect("get")
        .expect("row should exist");
    assert_eq!(row.status, RemoteEnvironmentStatus::Active);
    // The validation went to the host with the stored bearer.
    assert!(f.host.recorded_calls().iter().any(|call| matches!(
        call,
        RecordedHostCall::Validate { token, .. } if token == TOKEN
    )));
}

#[tokio::test]
async fn reconciler_surfaces_a_refused_token_for_repair_instead_of_activating() {
    let f = fixture();
    let env = seed_husk(&f).await;
    f.secrets
        .put_secret(&env.token_secret_ref, TOKEN)
        .await
        .expect("seed secret");
    *f.host.validate_response.lock().expect("mock") = Ok(false);

    let report = f.service.reconcile_on_startup().await;

    assert_eq!(report.needs_repair, vec![env.id.as_str().to_string()]);
    let row = f
        .repo
        .get(&env.id)
        .await
        .expect("get")
        .expect("row must survive for re-pair");
    assert_eq!(row.status, RemoteEnvironmentStatus::PendingAdd);
}

#[tokio::test]
async fn reconciler_defers_when_the_host_is_unreachable_instead_of_guessing() {
    let f = fixture();
    let env = seed_husk(&f).await;
    f.secrets
        .put_secret(&env.token_secret_ref, TOKEN)
        .await
        .expect("seed secret");
    *f.host.validate_response.lock().expect("mock") = Err(RemoteHostClientError::Unreachable(
        "host offline".to_string(),
    ));

    let report = f.service.reconcile_on_startup().await;

    // Fail closed: neither activated nor deleted — the bearer may be live.
    assert_eq!(report.deferred, vec![env.id.as_str().to_string()]);
    let row = f
        .repo
        .get(&env.id)
        .await
        .expect("get")
        .expect("row must survive");
    assert_eq!(row.status, RemoteEnvironmentStatus::PendingAdd);
    assert!(f
        .secrets
        .get_secret(&env.token_secret_ref)
        .await
        .expect("secret read")
        .is_some());
}

/// SecretStore decorator whose reads fail: an unreadable Keychain must defer,
/// never delete (deleting the row would orphan a possibly-live bearer).
struct UnreadableSecretStore;

#[async_trait]
impl crate::domain::services::SecretStore for UnreadableSecretStore {
    async fn put_secret(&self, _key: &str, _value: &str) -> Result<(), SecretStoreError> {
        Err(SecretStoreError::Unavailable("keychain locked".to_string()))
    }

    async fn get_secret(&self, _key: &str) -> Result<Option<String>, SecretStoreError> {
        Err(SecretStoreError::Unavailable("keychain locked".to_string()))
    }

    async fn delete_secret(&self, _key: &str) -> Result<(), SecretStoreError> {
        Err(SecretStoreError::Unavailable("keychain locked".to_string()))
    }
}

#[tokio::test]
async fn reconciler_defers_pending_add_when_the_keychain_read_errors() {
    let repo = Arc::new(MemoryRemoteEnvironmentRepository::new());
    let host = Arc::new(MockRemoteHostClient::new(
        descriptor("env-1"),
        pair_response("env-1"),
    ));
    let service = RemoteEnvironmentService::new(
        Arc::clone(&repo) as _,
        Arc::new(UnreadableSecretStore) as _,
        Arc::clone(&host) as _,
        test_relay(),
    );
    let env = repo
        .upsert_paired(crate::domain::repositories::UpsertPairedEnvironment {
            environment_id: "env-1".to_string(),
            name: "Mac Studio".to_string(),
            url: HOST_URL.to_string(),
            scopes: vec![Scope::UiRead],
            protocol_version: PROTOCOL_VERSION,
        })
        .await
        .expect("seed");

    let report = service.reconcile_on_startup().await;

    // A read ERROR is not "no data" (fail-closed read): the row survives.
    assert_eq!(report.deferred, vec![env.id.as_str().to_string()]);
    assert!(report.deleted_husks.is_empty());
    assert!(repo.get(&env.id).await.expect("get").is_some());
}

#[tokio::test]
async fn reconciler_completes_a_pending_delete_with_revoke_then_secret_then_row() {
    // Crash point: after the pending_delete mark, before any deletion.
    let f = fixture();
    let env = seed_husk(&f).await;
    f.secrets
        .put_secret(&env.token_secret_ref, TOKEN)
        .await
        .expect("seed secret");
    f.repo
        .set_status(&env.id, RemoteEnvironmentStatus::PendingDelete)
        .await
        .expect("mark pending_delete");

    let report = f.service.reconcile_on_startup().await;

    assert_eq!(report.completed_removals, vec![env.id.as_str().to_string()]);
    assert!(f.host.recorded_calls().iter().any(|call| matches!(
        call,
        RecordedHostCall::Revoke { token, .. } if token == TOKEN
    )));
    assert!(f
        .secrets
        .get_secret(&env.token_secret_ref)
        .await
        .expect("secret read")
        .is_none());
    assert!(f.repo.get(&env.id).await.expect("get").is_none());
}

#[tokio::test]
async fn reconciler_deletes_a_pending_delete_row_whose_secret_is_already_gone() {
    // Crash point: after the Keychain delete, before the row delete.
    let f = fixture();
    let env = seed_husk(&f).await;
    f.repo
        .set_status(&env.id, RemoteEnvironmentStatus::PendingDelete)
        .await
        .expect("mark pending_delete");

    let report = f.service.reconcile_on_startup().await;

    assert_eq!(report.completed_removals, vec![env.id.as_str().to_string()]);
    assert!(f.repo.get(&env.id).await.expect("get").is_none());
    // No orphaned valid bearer anywhere: nothing was in the secret store.
}

#[tokio::test]
async fn reconciler_leaves_active_rows_untouched() {
    let f = fixture();
    let env = f
        .service
        .pair(HOST_URL, "rxp_code", "Mac Studio")
        .await
        .expect("pairing should succeed");

    let report = f.service.reconcile_on_startup().await;

    assert_eq!(report, RemoteEnvironmentReconcileReport::default());
    let row = f
        .repo
        .get(&env.id)
        .await
        .expect("get")
        .expect("row should exist");
    assert_eq!(row.status, RemoteEnvironmentStatus::Active);
    assert!(f
        .secrets
        .get_secret(&env.token_secret_ref)
        .await
        .expect("secret read")
        .is_some());
}

// ============================================================================
// Event stream (PR 2.3): connect / disconnect / stream_send
// ============================================================================

fn stream_hello(protocol_version: u32) -> ServerFrame {
    ServerFrame::Hello {
        protocol_version,
        environment_id: "env-1".to_string(),
        stream_epoch: "epoch-1".to_string(),
        server_version: "0.81.0".to_string(),
        max_seq: 42,
        heartbeat_secs: 20,
    }
}

/// Scripts the two-step happy path: a 200 ticket mint and a socket whose first
/// frame is `hello`.
fn script_stream_success(f: &Fixture, protocol_version: u32) -> MockRemoteWsHandle {
    f.host
        .script_fetch(200, r#"{"ticket":"tick-1","expiresInSecs":60}"#);
    let (connection, handle) = MockRemoteWsConnection::scripted();
    handle
        .inbound
        .send(Ok(stream_hello(protocol_version)))
        .expect("scripted hello should queue");
    f.ws.script_connection(connection);
    handle
}

#[tokio::test]
async fn connect_requires_a_registered_active_environment() {
    let f = fixture();

    assert!(matches!(
        f.service.connect("nope").await,
        Err(RemoteEnvironmentError::UnknownEnvironment(_))
    ));
    assert!(matches!(
        f.service.connect(LOCAL_ENVIRONMENT_ID).await,
        Err(RemoteEnvironmentError::LocalEnvironment)
    ));
    // Authorization runs BEFORE any network effect: no ticket mint, no dial.
    assert!(f.host.recorded_calls().is_empty());
    assert!(f.ws.dialed_urls().is_empty());
}

#[tokio::test]
async fn connect_mints_a_ticket_and_returns_the_hello_outcome() {
    let f = fixture();
    let env = f
        .service
        .pair(HOST_URL, "rxp_code", "Mac Studio")
        .await
        .expect("pairing should succeed");
    let _handle = script_stream_success(&f, PROTOCOL_VERSION);

    let outcome = f
        .service
        .connect(env.id.as_str())
        .await
        .expect("connect should succeed");

    // The outcome is keyed to the registry ROW, not the host's self-reported id.
    assert_eq!(outcome.environment_id, env.id.as_str());
    assert_eq!(outcome.host_environment_id, "env-1");
    assert_eq!(outcome.stream_epoch, "epoch-1");
    assert_eq!(outcome.max_seq, 42);
    assert_eq!(outcome.protocol_version, PROTOCOL_VERSION);
    assert!(f.relay.is_connected(env.id.as_str()));

    // The ticket was minted with the STORED bearer against the ws-ticket route…
    let minted = f
        .host
        .recorded_calls()
        .into_iter()
        .find_map(|call| match call {
            RecordedHostCall::Fetch { token, request, .. } => Some((token, request)),
            _ => None,
        })
        .expect("the host should have seen one ticket mint");
    assert_eq!(minted.0, TOKEN);
    assert_eq!(
        minted.1.path,
        crate::infrastructure::remote_host_client::REMOTE_WS_TICKET_PATH
    );
    assert_eq!(minted.1.method, "POST");
    // …and the dialed URL is the wss form of the base URL carrying that ticket.
    assert_eq!(
        f.ws.dialed_urls(),
        vec!["wss://mac-studio.tailnet.ts.net/remote/v1/events?ticket=tick-1".to_string()]
    );
    // Best-effort bookkeeping recorded the connect.
    let row = f
        .repo
        .get(&env.id)
        .await
        .expect("get")
        .expect("row exists");
    assert!(row.last_connected_at.is_some());
}

/// 401/403 on the ticket mint are the supervisor's `blocked` entries; they must
/// stay typed and must stop the dial.
#[tokio::test]
async fn a_refused_ticket_mint_is_typed_and_never_dials() {
    for (status, expected) in [(401, "REMOTE_UNAUTHORIZED"), (403, "REMOTE_FORBIDDEN")] {
        let f = fixture();
        let env = f
            .service
            .pair(HOST_URL, "rxp_code", "Mac Studio")
            .await
            .expect("pairing should succeed");
        f.host.script_fetch(status, "no");

        let error = f
            .service
            .connect(env.id.as_str())
            .await
            .expect_err("a refused ticket must fail");
        assert_eq!(error.code(), expected, "status {status}");
        assert!(f.ws.dialed_urls().is_empty(), "no ticket, no dial");
        assert!(!f.relay.is_connected(env.id.as_str()));
    }
}

/// A ticket body that does not parse is `Unreachable` — never a silent empty
/// ticket smuggled into the dial URL.
#[tokio::test]
async fn an_unparsable_ticket_body_is_unreachable() {
    let f = fixture();
    let env = f
        .service
        .pair(HOST_URL, "rxp_code", "Mac Studio")
        .await
        .expect("pairing should succeed");
    f.host.script_fetch(200, r#"{"ok":true,"result":null}"#);

    let error = f
        .service
        .connect(env.id.as_str())
        .await
        .expect_err("an unparsable ticket body must fail");
    assert_eq!(error.code(), "REMOTE_UNREACHABLE");
    assert!(f.ws.dialed_urls().is_empty());
}

/// A refused WS handshake keeps its auth typing so the supervisor blocks instead
/// of retrying a dead credential.
#[tokio::test]
async fn a_rejected_ws_handshake_maps_to_the_auth_taxonomy() {
    let f = fixture();
    let env = f
        .service
        .pair(HOST_URL, "rxp_code", "Mac Studio")
        .await
        .expect("pairing should succeed");
    f.host
        .script_fetch(200, r#"{"ticket":"tick-1","expiresInSecs":60}"#);
    f.ws
        .script_error(crate::infrastructure::remote_ws_client::RemoteWsError::Rejected {
            status: 403,
            message: "scope refused".to_string(),
        });

    let error = f
        .service
        .connect(env.id.as_str())
        .await
        .expect_err("a rejected handshake must fail");
    assert_eq!(error.code(), "REMOTE_FORBIDDEN");
    assert!(!f.relay.is_connected(env.id.as_str()));
}

/// The hello version is relayed VERBATIM, never gated in Rust: negotiation is
/// `minClientProtocol`-based and only the descriptor carries that field, so a
/// Rust-side gate against the pairing-time snapshot would false-block a
/// legitimately upgraded host. The TS supervisor owns the `blocked` decision
/// (§6.5) with the descriptor in hand.
#[tokio::test]
async fn a_hello_version_that_differs_from_the_stored_row_still_connects() {
    let f = fixture();
    let env = f
        .service
        .pair(HOST_URL, "rxp_code", "Mac Studio")
        .await
        .expect("pairing should succeed");
    let _handle = script_stream_success(&f, PROTOCOL_VERSION + 1);

    let outcome = f
        .service
        .connect(env.id.as_str())
        .await
        .expect("a newer host hello must not be blocked in Rust");
    assert_eq!(
        outcome.protocol_version,
        PROTOCOL_VERSION + 1,
        "the hello version reaches TS verbatim for the supervisor's gate"
    );
    assert!(
        f.relay.is_connected(env.id.as_str()),
        "the socket survives; the skew decision belongs to the supervisor"
    );
}

#[tokio::test]
async fn disconnect_tears_the_session_down_and_is_idempotent_when_unconnected() {
    let f = fixture();
    let env = f
        .service
        .pair(HOST_URL, "rxp_code", "Mac Studio")
        .await
        .expect("pairing should succeed");
    let _handle = script_stream_success(&f, PROTOCOL_VERSION);
    f.service
        .connect(env.id.as_str())
        .await
        .expect("connect should succeed");

    f.service
        .disconnect(env.id.as_str())
        .await
        .expect("disconnect should succeed");
    assert!(!f.relay.is_connected(env.id.as_str()));
    // Idempotent: disconnecting an unconnected environment stays success.
    assert!(f.service.disconnect(env.id.as_str()).await.is_ok());
}

#[test]
fn fetch_validation_rejects_every_unsafe_shape() {
    for path in [
        "//host/path",
        "/http://host/path",
        "/a/../b",
        "/two words",
        "/x\ny",
    ] {
        assert!(matches!(
            validate_remote_fetch_path(path),
            Err(RemoteEnvironmentError::InvalidFetchRequest(_))
        ));
    }
    assert!(matches!(
        validate_remote_fetch_method("TRACE"),
        Err(RemoteEnvironmentError::InvalidFetchRequest(_))
    ));
    for headers in [
        vec![("authorization".to_string(), "secret".to_string())],
        vec![(
            "content-type".to_string(),
            "text/plain\ninjected".to_string(),
        )],
    ] {
        assert!(matches!(
            validate_remote_fetch_headers(&headers),
            Err(RemoteEnvironmentError::InvalidFetchRequest(_))
        ));
    }
}

#[test]
fn transport_failures_map_to_stable_codes() {
    let cases = [
        (
            RemoteHostClientError::Timeout("late".into()),
            "REMOTE_TIMEOUT_UNKNOWN",
        ),
        (
            RemoteHostClientError::Unreachable("offline".into()),
            "REMOTE_UNREACHABLE",
        ),
        (
            RemoteHostClientError::Rejected {
                status: 422,
                message: "reused".into(),
            },
            "REMOTE_REQUEST_ID_REUSED",
        ),
        (
            RemoteHostClientError::InvalidResponse("bad".into()),
            "REMOTE_VERSION_MISMATCH",
        ),
    ];
    for (error, expected) in cases {
        assert_eq!(transport_error(error).code(), expected);
    }
}

#[test]
fn every_untyped_status_maps_to_the_expected_code() {
    for (status, expected) in [
        (401, "REMOTE_UNAUTHORIZED"),
        (403, "REMOTE_FORBIDDEN"),
        (404, "REMOTE_COMMAND_UNAVAILABLE"),
        (501, "REMOTE_COMMAND_UNAVAILABLE"),
        (408, "REMOTE_TIMEOUT_UNKNOWN"),
        (504, "REMOTE_TIMEOUT_UNKNOWN"),
        (409, "REMOTE_REQUEST_IN_PROGRESS"),
        (422, "REMOTE_REQUEST_ID_REUSED"),
        (426, "REMOTE_VERSION_MISMATCH"),
        (505, "REMOTE_VERSION_MISMATCH"),
        // A malformed-argument refusal must NOT arrive as "command unavailable": the client
        // reads that code as "this host does not support the command at all".
        (400, "REMOTE_INVALID_ARGUMENTS"),
        (500, "REMOTE_INTERNAL_ERROR"),
        (599, "REMOTE_UNREACHABLE"),
    ] {
        let error = transport_error(RemoteHostClientError::Rejected {
            status,
            message: "refused".into(),
        });
        assert_eq!(error.code(), expected, "status {status}");
    }
}

#[test]
fn invoke_envelopes_preserve_results_errors_and_null_defaults() {
    for (body, expected) in [
        (
            r#"{"ok":false,"error":{"code":"bad"}}"#,
            RemoteInvokeOutcome::CommandError {
                error: serde_json::json!({"code": "bad"}),
            },
        ),
        (
            r#"{"ok":false}"#,
            RemoteInvokeOutcome::CommandError {
                error: serde_json::Value::Null,
            },
        ),
        (
            r#"{"ok":true,"result":{"id":1}}"#,
            RemoteInvokeOutcome::Ok {
                result: serde_json::json!({"id": 1}),
            },
        ),
        (
            r#"{"ok":true}"#,
            RemoteInvokeOutcome::Ok {
                result: serde_json::Value::Null,
            },
        ),
        (
            r#"{"plain":"body"}"#,
            RemoteInvokeOutcome::Ok {
                result: serde_json::json!({"plain": "body"}),
            },
        ),
    ] {
        assert_eq!(
            parse_invoke_response(RemoteHttpResponse {
                status: 200,
                body: body.to_string(),
            })
            .expect("valid success response"),
            expected
        );
    }

    let error = parse_invoke_response(RemoteHttpResponse {
        status: 200,
        body: "not-json".to_string(),
    })
    .expect_err("an invalid success body is a version mismatch");
    assert_eq!(error.code(), "REMOTE_VERSION_MISMATCH");
}

#[test]
fn pairing_url_and_pairing_wire_errors_cover_every_rejection() {
    for url in ["not a url", "ftp://host/path", "http:/missing-host"] {
        assert!(
            validate_pairing_url(url).is_err(),
            "{url:?} must not reach pairing"
        );
    }
    assert!(client_device_name().starts_with("RalphX Desktop "));

    for (error, expected_code) in [
        (
            descriptor_error(RemoteHostClientError::Unreachable("offline".into())),
            "REMOTE_UNREACHABLE",
        ),
        (
            descriptor_error(RemoteHostClientError::Timeout("late".into())),
            "REMOTE_UNREACHABLE",
        ),
        (
            descriptor_error(RemoteHostClientError::Rejected {
                status: 503,
                message: "busy".into(),
            }),
            "REMOTE_UNREACHABLE",
        ),
        (
            descriptor_error(RemoteHostClientError::InvalidResponse("bad json".into())),
            "REMOTE_UNREACHABLE",
        ),
        (
            pair_error(RemoteHostClientError::Unreachable("offline".into())),
            "REMOTE_UNREACHABLE",
        ),
        (
            pair_error(RemoteHostClientError::Timeout("late".into())),
            "REMOTE_UNREACHABLE",
        ),
        (
            pair_error(RemoteHostClientError::Rejected {
                status: 409,
                message: "used".into(),
            }),
            "PAIRING_REJECTED",
        ),
        (
            pair_error(RemoteHostClientError::InvalidResponse("bad json".into())),
            "PAIRING_REJECTED",
        ),
    ] {
        assert_eq!(error.code(), expected_code);
    }
}

#[tokio::test]
async fn active_environment_success_and_stub_guard_matrix() {
    let f = fixture();
    assert!(matches!(
        f.service.connect(LOCAL_ENVIRONMENT_ID).await,
        Err(RemoteEnvironmentError::LocalEnvironment)
    ));
    assert!(matches!(
        f.service.disconnect(LOCAL_ENVIRONMENT_ID).await,
        Err(RemoteEnvironmentError::LocalEnvironment)
    ));
    assert!(matches!(
        f.service.disconnect("missing").await,
        Err(RemoteEnvironmentError::UnknownEnvironment(_))
    ));

    let pending = seed_husk(&f).await;
    assert!(matches!(
        f.service.connect(pending.id.as_str()).await,
        Err(RemoteEnvironmentError::EnvironmentNotUsable(..))
    ));

    f.repo
        .set_status(&pending.id, RemoteEnvironmentStatus::Active)
        .await
        .expect("activate row");
    f.service
        .set_active_environment(pending.id.as_str())
        .await
        .expect("active row may become authoritative");
    assert_eq!(f.service.active_environment_id().await, pending.id.as_str());
}

#[tokio::test]
async fn re_pair_succeeds_when_replaced_token_revoke_fails() {
    let f = fixture();
    f.service
        .pair(HOST_URL, "rxp_code", "Mac Studio")
        .await
        .expect("first pair");
    *f.host.pair_response.lock().expect("mock") = Ok(PairWireResponse {
        device_token: "rxd_replacement".to_string(),
        ..pair_response("env-1")
    });
    *f.host.revoke_response.lock().expect("mock") = Err(RemoteHostClientError::Unreachable(
        "old host offline".into(),
    ));

    let repaired = f
        .service
        .pair(HOST_URL_DIRECT, "rxp_code2", "Mac Studio")
        .await
        .expect("best-effort cleanup cannot fail the completed re-pair");
    assert_eq!(repaired.status, RemoteEnvironmentStatus::Active);
    assert_eq!(
        f.secrets
            .get_secret(&repaired.token_secret_ref)
            .await
            .expect("secret read")
            .as_deref(),
        Some("rxd_replacement")
    );
}

#[tokio::test]
async fn reconciler_defers_when_activation_write_fails() {
    let inner = Arc::new(MemoryRemoteEnvironmentRepository::new());
    let env = inner
        .upsert_paired(crate::domain::repositories::UpsertPairedEnvironment {
            environment_id: "env-pending".to_string(),
            name: "Pending".to_string(),
            url: HOST_URL.to_string(),
            scopes: vec![Scope::UiRead],
            protocol_version: PROTOCOL_VERSION,
        })
        .await
        .expect("seed");
    let secrets = Arc::new(MemorySecretStore::new());
    secrets
        .put_secret(&env.token_secret_ref, TOKEN)
        .await
        .expect("seed secret");
    let repo = Arc::new(FailingRemoteEnvironmentRepository {
        fail_set_status: true,
        ..FailingRemoteEnvironmentRepository::new(Arc::clone(&inner))
    });
    let service = RemoteEnvironmentService::new(
        repo,
        secrets,
        Arc::new(MockRemoteHostClient::new(
            descriptor("env-pending"),
            pair_response("env-pending"),
        )),
        test_relay(),
    );

    let report = service.reconcile_on_startup().await;
    assert_eq!(report.deferred, vec![env.id.as_str().to_string()]);
    assert_eq!(
        inner
            .get(&env.id)
            .await
            .expect("row")
            .expect("exists")
            .status,
        RemoteEnvironmentStatus::PendingAdd
    );
}

#[tokio::test]
async fn reconciler_pending_delete_defers_each_destructive_failure() {
    let inner = Arc::new(MemoryRemoteEnvironmentRepository::new());
    let repo = Arc::new(FailingRemoteEnvironmentRepository {
        fail_delete: true,
        ..FailingRemoteEnvironmentRepository::new(Arc::clone(&inner))
    });
    let secrets = Arc::new(MemorySecretStore::new());
    let host = Arc::new(MockRemoteHostClient::new(
        descriptor("env-delete"),
        pair_response("env-delete"),
    ));
    let service = RemoteEnvironmentService::new(
        Arc::clone(&repo) as _,
        Arc::clone(&secrets) as _,
        Arc::clone(&host) as _,
        test_relay(),
    );
    let env = inner
        .upsert_paired(crate::domain::repositories::UpsertPairedEnvironment {
            environment_id: "env-delete".to_string(),
            name: "Delete".to_string(),
            url: HOST_URL.to_string(),
            scopes: vec![Scope::UiRead],
            protocol_version: PROTOCOL_VERSION,
        })
        .await
        .expect("seed");
    inner
        .set_status(&env.id, RemoteEnvironmentStatus::PendingDelete)
        .await
        .expect("pending delete");
    secrets
        .put_secret(&env.token_secret_ref, TOKEN)
        .await
        .expect("seed secret");
    *host.revoke_response.lock().expect("mock") =
        Err(RemoteHostClientError::Unreachable("offline".into()));

    let report = service.reconcile_on_startup().await;
    assert_eq!(report.deferred, vec![env.id.as_str().to_string()]);
    assert!(inner.get(&env.id).await.expect("row").is_some());
    assert!(
        secrets
            .get_secret(&env.token_secret_ref)
            .await
            .expect("secret read")
            .is_none(),
        "row deletion was attempted only after the secret was deleted"
    );

    let secretless_retry = service.reconcile_on_startup().await;
    assert_eq!(secretless_retry.deferred, vec![env.id.as_str().to_string()]);
}

#[tokio::test]
async fn reconciler_keeps_pending_delete_row_when_secret_delete_fails() {
    let repo = Arc::new(MemoryRemoteEnvironmentRepository::new());
    let inner_secrets = Arc::new(MemorySecretStore::new());
    let secrets = Arc::new(FailingDeleteSecretStore {
        inner: Arc::clone(&inner_secrets),
        fail_delete: StdMutex::new(true),
    });
    let host = Arc::new(MockRemoteHostClient::new(
        descriptor("env-delete"),
        pair_response("env-delete"),
    ));
    let service = RemoteEnvironmentService::new(
        Arc::clone(&repo) as _,
        Arc::clone(&secrets) as _,
        Arc::clone(&host) as _,
        test_relay(),
    );
    let env = repo
        .upsert_paired(crate::domain::repositories::UpsertPairedEnvironment {
            environment_id: "env-delete".to_string(),
            name: "Delete".to_string(),
            url: HOST_URL.to_string(),
            scopes: vec![Scope::UiRead],
            protocol_version: PROTOCOL_VERSION,
        })
        .await
        .expect("seed");
    repo.set_status(&env.id, RemoteEnvironmentStatus::PendingDelete)
        .await
        .expect("pending delete");
    inner_secrets
        .put_secret(&env.token_secret_ref, TOKEN)
        .await
        .expect("seed secret");

    let report = service.reconcile_on_startup().await;
    assert_eq!(report.deferred, vec![env.id.as_str().to_string()]);
    assert!(repo.get(&env.id).await.expect("row").is_some());
    assert!(inner_secrets
        .get_secret(&env.token_secret_ref)
        .await
        .expect("secret read")
        .is_some());
}

#[tokio::test]
async fn stream_send_reaches_the_live_socket() {
    let f = fixture();
    let env = f
        .service
        .pair(HOST_URL, "rxp_code", "Mac Studio")
        .await
        .expect("pairing should succeed");
    let mut handle = script_stream_success(&f, PROTOCOL_VERSION);
    f.service
        .connect(env.id.as_str())
        .await
        .expect("connect should succeed");

    f.service
        .stream_send(
            env.id.as_str(),
            ralphx_remote_protocol::ClientFrame::Subscribe {
                after_seq: 42,
                stream_epoch: "epoch-1".to_string(),
            },
        )
        .await
        .expect("stream_send should reach the live session");

    let sent = tokio::time::timeout(std::time::Duration::from_secs(5), handle.outbound.recv())
        .await
        .expect("the frame should arrive in time")
        .expect("the outbound channel should stay open");
    assert_eq!(
        sent,
        ralphx_remote_protocol::ClientFrame::Subscribe {
            after_seq: 42,
            stream_epoch: "epoch-1".to_string(),
        }
    );
}

/// The stream surface shares `connect`'s guards — local and unknown targets are
/// refused, and NO active-environment binding applies: background environments
/// keep their sockets (and their `subscribe`/`cursorAck` frames) alive (§6.4).
#[tokio::test]
async fn stream_send_uses_the_stream_guards_not_the_active_binding() {
    let f = fixture();
    let (a, b) = two_paired_environments(&f).await;
    f.service
        .set_active_environment(&a)
        .await
        .expect("activating A should succeed");

    assert!(matches!(
        f.service
            .stream_send(
                LOCAL_ENVIRONMENT_ID,
                ralphx_remote_protocol::ClientFrame::CursorAck { seq: 1 },
            )
            .await,
        Err(RemoteEnvironmentError::LocalEnvironment)
    ));
    assert!(matches!(
        f.service
            .stream_send(
                "nope",
                ralphx_remote_protocol::ClientFrame::CursorAck { seq: 1 },
            )
            .await,
        Err(RemoteEnvironmentError::UnknownEnvironment(_))
    ));

    // BACKGROUND environment: the control frame is authorized (no active-env gate)
    // and fails only because no session is live — as `Unreachable`, which the
    // supervisor retries, never `NotActiveEnvironment`, which it would block on.
    let error = f
        .service
        .stream_send(
            &b,
            ralphx_remote_protocol::ClientFrame::CursorAck { seq: 1 },
        )
        .await
        .expect_err("no live session for B");
    assert_eq!(error.code(), "REMOTE_UNREACHABLE");
    assert!(
        !matches!(error, RemoteEnvironmentError::NotActiveEnvironment { .. }),
        "background environments' stream control must not be active-env-gated"
    );
}

// ------------------------------------------------------------------------------
// preview_remote_environment (PR 2.5): the read-only pre-pair identity probe.
//
// The load-bearing property is ABSENCE. Preview runs before a single-use pairing code
// is consumed and may be re-run freely, so it must leave the registry, the Keychain,
// and the active-environment mirror exactly as it found them.
// ------------------------------------------------------------------------------

/// A secret store that counts every call, so "preview touched no secret" is proven by
/// call count rather than by the map happening to look unchanged.
struct CountingSecretStore {
    inner: MemorySecretStore,
    calls: Arc<StdMutex<usize>>,
}

impl CountingSecretStore {
    fn new() -> Self {
        Self {
            inner: MemorySecretStore::new(),
            calls: Arc::new(StdMutex::new(0)),
        }
    }

    fn count(&self) -> usize {
        *self.calls.lock().expect("counter")
    }

    fn record(&self) {
        *self.calls.lock().expect("counter") += 1;
    }
}

#[async_trait]
impl crate::domain::services::SecretStore for CountingSecretStore {
    async fn put_secret(&self, key: &str, value: &str) -> Result<(), SecretStoreError> {
        self.record();
        self.inner.put_secret(key, value).await
    }

    async fn get_secret(&self, key: &str) -> Result<Option<String>, SecretStoreError> {
        self.record();
        self.inner.get_secret(key).await
    }

    async fn delete_secret(&self, key: &str) -> Result<(), SecretStoreError> {
        self.record();
        self.inner.delete_secret(key).await
    }
}

#[tokio::test]
async fn preview_returns_descriptor_identity_without_touching_anything() {
    let repo = Arc::new(MemoryRemoteEnvironmentRepository::new());
    let secrets = Arc::new(CountingSecretStore::new());
    let host = Arc::new(MockRemoteHostClient::new(
        descriptor("env-1"),
        pair_response("env-1"),
    ));
    let service = RemoteEnvironmentService::new(
        Arc::clone(&repo) as Arc<dyn crate::domain::repositories::RemoteEnvironmentRepository>,
        Arc::clone(&secrets) as Arc<dyn crate::domain::services::SecretStore>,
        Arc::clone(&host) as Arc<dyn crate::infrastructure::remote_host_client::RemoteHostClient>,
        test_relay(),
    );

    let preview = service
        .preview(HOST_URL)
        .await
        .expect("preview should succeed");

    assert_eq!(preview.environment_id, "env-1");
    assert_eq!(preview.app_version, "0.81.0");
    assert_eq!(preview.platform, "macos");
    assert_eq!(preview.protocol_version, PROTOCOL_VERSION);
    assert_eq!(preview.min_client_protocol, PROTOCOL_VERSION);
    assert_eq!(
        preview.already_paired_as, None,
        "an unknown host is not reported as already paired"
    );

    // Absence assertions — the whole point of a preview.
    assert!(
        repo.list().await.expect("list").is_empty(),
        "preview must not write a registry row"
    );
    assert_eq!(
        secrets.count(),
        0,
        "preview must never reach the secret store (P-18)"
    );
    assert_eq!(
        service.active_environment_id().await,
        LOCAL_ENVIRONMENT_ID,
        "preview must not move the active-environment mirror"
    );

    // The only host call is the descriptor read: no pairing exchange was attempted.
    let calls = host.recorded_calls();
    assert_eq!(calls.len(), 1);
    assert!(matches!(calls[0], RecordedHostCall::Descriptor { .. }));
}

#[tokio::test]
async fn preview_reports_the_existing_name_for_an_already_paired_host() {
    let f = fixture();
    f.service
        .pair(HOST_URL, "rxp_code", "Studio Mac")
        .await
        .expect("seed pairing should succeed");
    let before = f.repo.list().await.expect("list");

    let preview = f
        .service
        .preview(HOST_URL)
        .await
        .expect("preview of a known host should succeed");

    assert_eq!(
        preview.already_paired_as,
        Some("Studio Mac".to_string()),
        "a known host identity surfaces its row name so the flow can say re-pairing UPDATES it"
    );
    // Re-previewing a paired host must not disturb the row it just read.
    assert_eq!(f.repo.list().await.expect("list"), before);
}

#[tokio::test]
async fn preview_version_skew_returns_the_same_typed_error_pair_would() {
    let f = fixture();
    {
        let mut descriptor_slot = f.host.descriptor.lock().expect("mock");
        let mut skewed = descriptor("env-1");
        skewed.min_client_protocol = PROTOCOL_VERSION + 1;
        *descriptor_slot = Ok(skewed);
    }

    let error = f
        .service
        .preview(HOST_URL)
        .await
        .expect_err("skew must block the preview");
    assert!(matches!(
        error,
        RemoteEnvironmentError::VersionSkew {
            host_min_client,
            client,
        } if host_min_client == PROTOCOL_VERSION + 1 && client == PROTOCOL_VERSION
    ));
    assert_eq!(
        error.code(),
        "REMOTE_VERSION_MISMATCH",
        "the flow renders the service's taxonomy; it never re-derives version comparisons"
    );
    assert!(f.repo.list().await.expect("list").is_empty());
}

#[tokio::test]
async fn preview_of_an_unreachable_host_is_typed_and_writes_nothing() {
    let f = fixture_with_host(MockRemoteHostClient::unreachable());

    let error = f
        .service
        .preview(HOST_URL)
        .await
        .expect_err("an unreachable host must surface as a typed error");
    assert!(matches!(error, RemoteEnvironmentError::Unreachable(_)));
    assert_eq!(error.code(), "REMOTE_UNREACHABLE");
    assert!(f.repo.list().await.expect("list").is_empty());
}

#[tokio::test]
async fn preview_rejects_unshaped_urls_before_any_network_call() {
    let f = fixture();

    for url in [
        "file:///etc/passwd",
        "https://host.ts.net:3849?redirect=evil",
        "https://host.ts.net:3849#code=rxp_leak",
        "not a url",
    ] {
        let error = f
            .service
            .preview(url)
            .await
            .expect_err("an unshaped URL must be rejected");
        assert!(
            matches!(error, RemoteEnvironmentError::InvalidUrl(_)),
            "{url:?} must be refused as a bad URL, got {error:?}"
        );
    }
    assert!(
        f.host.recorded_calls().is_empty(),
        "rejection happens before any network call"
    );
}

#[tokio::test]
async fn pairing_urls_carrying_a_query_or_fragment_are_rejected_at_the_sink() {
    // base_url is the stem every derived URL is built from; an unshaped one would
    // reappear glued after the pairing/ticket/descriptor paths.
    for url in [
        "https://host.ts.net:3849?redirect=evil",
        "https://host.ts.net:3849/#code=rxp_leak",
        "http://host.ts.net:3849?a=1#b",
    ] {
        assert!(
            matches!(
                validate_pairing_url(url),
                Err(RemoteEnvironmentError::InvalidUrl(_))
            ),
            "{url:?} must not become a base_url"
        );
    }
    // The canonical shapes still pass, including trailing-slash normalisation.
    assert_eq!(
        validate_pairing_url("https://host.ts.net:3849/").expect("canonical url"),
        "https://host.ts.net:3849"
    );
}
