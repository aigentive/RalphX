// RemoteEnvironmentService tests: pairing against a mock host, the staged
// add/remove machines, the P-27 partial-failure reconciler matrix, the P-26
// active-environment binding, and the P-18 no-token-to-JS proof.

use std::sync::{Arc, Mutex as StdMutex};

use async_trait::async_trait;
use ralphx_remote_protocol::{EnvironmentDescriptor, Scope, PROTOCOL_VERSION};

use super::*;
use crate::domain::entities::remote_environment::RemoteEnvironmentStatus;
use crate::infrastructure::memory::{MemoryRemoteEnvironmentRepository, MemorySecretStore};
use crate::infrastructure::remote_host_client::{
    MockRemoteHostClient, PairWireResponse, RecordedHostCall, RemoteHostClientError,
};

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
    service: RemoteEnvironmentService,
}

fn fixture() -> Fixture {
    fixture_with_host(MockRemoteHostClient::new(
        descriptor("env-1"),
        pair_response("env-1"),
    ))
}

fn fixture_with_host(host: MockRemoteHostClient) -> Fixture {
    let repo = Arc::new(MemoryRemoteEnvironmentRepository::new());
    let secrets = Arc::new(MemorySecretStore::new());
    let host = Arc::new(host);
    let service = RemoteEnvironmentService::new(
        Arc::clone(&repo) as Arc<dyn crate::domain::repositories::RemoteEnvironmentRepository>,
        Arc::clone(&secrets) as Arc<dyn crate::domain::services::SecretStore>,
        Arc::clone(&host) as Arc<dyn crate::infrastructure::remote_host_client::RemoteHostClient>,
    );
    Fixture {
        repo,
        secrets,
        host,
        service,
    }
}

// Trait methods on the concrete test doubles resolve through the imports the
// service module already provides via `use super::*` (RemoteEnvironmentRepository,
// SecretStore) — no extra trait imports needed here.

// ============================================================================
// Pairing against the mock host
// ============================================================================

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
    (
        env_a.id.as_str().to_string(),
        env_b.id.as_str().to_string(),
    )
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
async fn invoke_for_the_active_environment_passes_binding_and_hits_the_stub() {
    let f = fixture();
    let (a, _b) = two_paired_environments(&f).await;
    f.service
        .set_active_environment(&a)
        .await
        .expect("activating A should succeed");

    let error = f
        .service
        .invoke(&a, "req-1", "health_check", serde_json::json!({}))
        .await
        .expect_err("transport is a PR 2.2 stub");
    assert!(matches!(error, RemoteEnvironmentError::NotConnected));
    assert_eq!(error.code(), "NOT_CONNECTED");
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
        .fetch(&b, "/api/tasks")
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

    let value = f
        .service
        .fetch(
            &b,
            crate::infrastructure::remote_host_client::REMOTE_DESCRIPTOR_PATH,
        )
        .await
        .expect("health probe for a background env must be allowed");
    assert_eq!(value["environmentId"], "env-2");
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
            crate::infrastructure::remote_host_client::REMOTE_DESCRIPTOR_PATH,
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

    assert_eq!(
        report.completed_removals,
        vec![env.id.as_str().to_string()]
    );
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

    assert_eq!(
        report.completed_removals,
        vec![env.id.as_str().to_string()]
    );
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
// Proxy stubs: connect/disconnect
// ============================================================================

#[tokio::test]
async fn connect_requires_a_registered_active_environment() {
    let f = fixture();

    assert!(matches!(
        f.service.connect("nope").await,
        Err(RemoteEnvironmentError::UnknownEnvironment(_))
    ));

    let env = f
        .service
        .pair(HOST_URL, "rxp_code", "Mac Studio")
        .await
        .expect("pairing should succeed");
    // Transport is a stub until PR 2.3; the typed error proves authorization ran.
    assert!(matches!(
        f.service.connect(env.id.as_str()).await,
        Err(RemoteEnvironmentError::NotConnected)
    ));
    assert!(f.service.disconnect(env.id.as_str()).await.is_ok());
}
