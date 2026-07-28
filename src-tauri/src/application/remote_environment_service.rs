// RemoteEnvironmentService — client-side registry, pairing, staged add/remove state
// machines, startup reconciler, and the active-environment authority for the Rust
// proxy surface (§4.2, §6.1, §6.4).
//
// Invariants owned here (not by the repository, not by the webview):
// - Add order: row as `pending_add` FIRST → Keychain secret → flip `active` (P-27).
// - Remove order: mark `pending_delete` → best-effort host revoke → Keychain delete
//   → row delete (P-27). Any other ordering can orphan a valid bearer.
// - `remote_invoke`/`remote_fetch` authorize against the Rust-side active-environment
//   mirror, never a trusted JS argument; background environments accept health ops
//   only (P-26).
// - The device token flows host-response → Keychain and Keychain → host header only;
//   no method returns it (P-18).
//
// Honest containment (N3-M3, documented residual — do NOT claim more): the binding
// prevents CONCURRENT fan-out and bearer EXTRACTION. A compromised renderer can still
// call `set_active_environment` and drive paired environments one at a time,
// sequentially. v1 accepts that residual; nothing here asserts it prevented.

use std::sync::Arc;

use ralphx_remote_protocol::{ErrorCode, Scope, PROTOCOL_VERSION};
use tokio::sync::RwLock;

use crate::domain::entities::remote_environment::{
    RemoteEnvironment, RemoteEnvironmentId, RemoteEnvironmentStatus,
};
use crate::domain::repositories::{RemoteEnvironmentRepository, UpsertPairedEnvironment};
use crate::domain::services::{SecretStore, SecretStoreError};
use crate::error::AppError;
use crate::infrastructure::remote_host_client::{
    InvokeWireRequest, PairWireRequest, RemoteFetchRequest, RemoteHostClient,
    RemoteHostClientError, RemoteHttpResponse, REMOTE_DESCRIPTOR_PATH,
};

/// The always-present local environment identity (§6.4). It has no supervisor, no
/// registry row, and never accepts remote proxy calls.
pub const LOCAL_ENVIRONMENT_ID: &str = "local";

/// Health-op fetch path for the host health probe.
pub const REMOTE_HEALTH_PATH: &str = "/health";

/// Scopes requested at pairing time. Default pairing intentionally does NOT request
/// `ui:agent` (§3.3); agent control is a per-device host-side grant.
const DEFAULT_REQUESTED_SCOPES: &[Scope] = &[Scope::UiRead, Scope::UiOperate];

/// Typed failures of the remote environment surface (rule 5: no string matching).
#[derive(Debug, thiserror::Error)]
pub enum RemoteEnvironmentError {
    /// Transport is not wired yet — the outbound HTTP invoke path and WS land in
    /// PR 2.2/2.3. Authorization already ran when this is returned.
    #[error("remote transport is not connected")]
    NotConnected,
    #[error("environment {requested} is not the active environment ({active})")]
    NotActiveEnvironment { requested: String, active: String },
    #[error("no paired remote environment with id {0}")]
    UnknownEnvironment(String),
    #[error("environment {0} is not active (status: {1})")]
    EnvironmentNotUsable(String, &'static str),
    #[error("the local environment does not accept remote proxy calls")]
    LocalEnvironment,
    #[error("invalid pairing URL: {0}")]
    InvalidUrl(String),
    #[error(
        "host requires client protocol >= {host_min_client}, this client speaks {client}"
    )]
    VersionSkew { host_min_client: u32, client: u32 },
    #[error("host identity mismatch: descriptor reported {descriptor}, pair response {response}")]
    IdentityMismatch {
        descriptor: String,
        response: String,
    },
    #[error("pairing rejected by host: {0}")]
    PairRejected(String),
    #[error("host unreachable: {0}")]
    Unreachable(String),
    /// A typed transport outcome carried straight from the protocol taxonomy (§6.3).
    /// Used for everything the HOST decided (scope refusal, unknown command, dedup
    /// reservation) plus the two unknown-outcome conditions.
    #[error("{message}")]
    Transport {
        code: ErrorCode,
        message: String,
    },
    /// The webview asked for a fetch the proxy will not perform (bad path shape,
    /// non-allowlisted method or header). Fail closed: a bearer is attached to this
    /// request, so an unvalidated target is a credential-forwarding hazard.
    #[error("invalid remote fetch request: {0}")]
    InvalidFetchRequest(String),
    /// No bearer is stored for an environment the registry still lists.
    #[error("no stored credential for environment {0}")]
    MissingCredential(String),
    #[error("secret store: {0}")]
    Secret(#[from] SecretStoreError),
    #[error(transparent)]
    Db(#[from] AppError),
}

impl RemoteEnvironmentError {
    /// Stable machine-readable code carried across the IPC boundary
    /// (`"{code}: {message}"`). Protocol-crate codes are reused where one exists.
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotConnected => "NOT_CONNECTED",
            Self::NotActiveEnvironment { .. } | Self::LocalEnvironment => {
                remote_error_code_str(ErrorCode::RemoteForbidden)
            }
            Self::UnknownEnvironment(_) | Self::EnvironmentNotUsable(..) => {
                remote_error_code_str(ErrorCode::RemoteCommandUnavailable)
            }
            Self::InvalidUrl(_) => "INVALID_PAIRING_URL",
            Self::VersionSkew { .. } => remote_error_code_str(ErrorCode::RemoteVersionMismatch),
            Self::IdentityMismatch { .. } => "HOST_IDENTITY_MISMATCH",
            Self::PairRejected(_) => "PAIRING_REJECTED",
            Self::Unreachable(_) => remote_error_code_str(ErrorCode::RemoteUnreachable),
            Self::Transport { code, .. } => remote_error_code_str(*code),
            Self::InvalidFetchRequest(_) => {
                remote_error_code_str(ErrorCode::RemoteCommandUnavailable)
            }
            Self::MissingCredential(_) => remote_error_code_str(ErrorCode::RemoteUnauthorized),
            Self::Secret(_) => "SECRET_STORE_UNAVAILABLE",
            Self::Db(_) => "DATABASE_ERROR",
        }
    }

    /// IPC-boundary rendering: `"{code}: {message}"`.
    pub fn to_command_error(&self) -> String {
        format!("{}: {}", self.code(), self)
    }
}

/// Serialized form of a protocol error code (single authority: the protocol crate).
fn remote_error_code_str(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::RemoteCommandUnavailable => "REMOTE_COMMAND_UNAVAILABLE",
        ErrorCode::RemoteForbidden => "REMOTE_FORBIDDEN",
        ErrorCode::RemoteUnauthorized => "REMOTE_UNAUTHORIZED",
        ErrorCode::RemoteUnreachable => "REMOTE_UNREACHABLE",
        ErrorCode::RemoteVersionMismatch => "REMOTE_VERSION_MISMATCH",
        ErrorCode::RemoteTimeoutUnknown => "REMOTE_TIMEOUT_UNKNOWN",
        ErrorCode::RemoteRequestInProgress => "REMOTE_REQUEST_IN_PROGRESS",
        ErrorCode::RemoteRequestIdReused => "REMOTE_REQUEST_ID_REUSED",
    }
}

/// Outcome of one proxied command dispatch, as the webview sees it (§6.3).
///
/// The discriminant is load-bearing. `Result<Value, String>` alone cannot separate
/// "the host's command returned `Err(E)`" from "the transport failed": `E` may be any
/// JSON value, and flattening it into the IPC error string would destroy the shape
/// existing `catch` paths read over local Tauri IPC. Transport failures travel on the
/// `Err` channel as `"{CODE}: {message}"`; command failures travel here.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(tag = "outcome", rename_all = "camelCase")]
pub enum RemoteInvokeOutcome {
    Ok { result: serde_json::Value },
    CommandError { error: serde_json::Value },
}

/// Outcome of one proxied `/api/…` fetch (§3.5).
///
/// Status and body travel separately so `backendFetch` can rebuild a real `Response`
/// on the JS side — every migrated call site branches on `res.ok`/`res.status` and
/// then calls `res.json()`, and collapsing a non-2xx into an IPC rejection would
/// silently change all of them.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteFetchOutcome {
    pub status: u16,
    pub body: String,
}

/// A validated request for one remounted `/api/…` route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteFetchCall {
    pub path: String,
    pub method: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
}

impl RemoteFetchCall {
    /// A bare GET, the shape the health/descriptor probes use.
    pub fn get(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            method: "GET".to_string(),
            headers: Vec::new(),
            body: None,
        }
    }
}

/// Methods the proxy will forward. Anything else is refused before a bearer is read.
const ALLOWED_FETCH_METHODS: &[&str] = &["GET", "HEAD", "POST", "PUT", "PATCH", "DELETE"];

/// Request headers the webview may set on a proxied fetch.
///
/// An allowlist rather than a denylist: the proxy attaches the device bearer, so any
/// header the webview can inject is a header attached to an authenticated request.
/// `Authorization` and `Cookie` are not merely absent from this list — the point of
/// the list is that they can never be added to it by accident.
const ALLOWED_FETCH_HEADERS: &[&str] = &["content-type", "accept"];

/// What the startup reconciler did, for logs and tests (row ids).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RemoteEnvironmentReconcileReport {
    /// `pending_add` rows whose secret validated against the host → flipped `active`.
    pub activated: Vec<String>,
    /// `pending_add` rows with no Keychain secret → deleted (crash before the
    /// Keychain write left a husk).
    pub deleted_husks: Vec<String>,
    /// `pending_delete` rows completed: revoke retried, secret deleted, row deleted.
    pub completed_removals: Vec<String>,
    /// `pending_add` rows whose token the host explicitly refused.
    ///
    /// **Not durable.** The row keeps `status = pending_add`, which is
    /// indistinguishable through `list_remote_environments` from a row still
    /// waiting on an unreachable host, so this verdict lives only for the
    /// lifetime of the report (it is logged per row). §6.1's "surfaced for
    /// re-pair" therefore has no consumer-visible carrier yet: the status
    /// vocabulary is pinned to `active | pending_add | pending_delete` by the
    /// spec, so giving the pairing UI a real signal needs a spec-level decision
    /// (extra status value or a `last_reconcile_outcome` column) — an obligation
    /// for whichever PR builds that pane, not a silent local invention.
    pub needs_repair: Vec<String>,
    /// Rows skipped fail-closed (Keychain unavailable, host unreachable, write
    /// failure) — retried on the next startup.
    pub deferred: Vec<String>,
}

pub struct RemoteEnvironmentService {
    repo: Arc<dyn RemoteEnvironmentRepository>,
    secret_store: Arc<dyn SecretStore>,
    host_client: Arc<dyn RemoteHostClient>,
    /// Rust-side mirror of the frontend `environmentStore` identity (§6.4).
    /// The ONLY writer is `set_active_environment`; proxy authorization reads it.
    active_environment_id: RwLock<String>,
}

impl RemoteEnvironmentService {
    pub fn new(
        repo: Arc<dyn RemoteEnvironmentRepository>,
        secret_store: Arc<dyn SecretStore>,
        host_client: Arc<dyn RemoteHostClient>,
    ) -> Self {
        Self {
            repo,
            secret_store,
            host_client,
            active_environment_id: RwLock::new(LOCAL_ENVIRONMENT_ID.to_string()),
        }
    }

    // ------------------------------------------------------------------
    // Pairing (staged add machine, §6.1 / P-27)
    // ------------------------------------------------------------------

    /// Pairs this client with a host: descriptor → pair exchange → row as
    /// `pending_add` → Keychain write → flip `active`.
    ///
    /// The ordering is load-bearing. A crash after the row write leaves a
    /// reconcilable `pending_add` husk; a crash after the Keychain write leaves a
    /// `pending_add` row the reconciler re-validates and activates. There is no
    /// ordering in which a valid bearer exists without a row referencing it.
    pub async fn pair(
        &self,
        url: &str,
        code: &str,
        name: &str,
    ) -> Result<RemoteEnvironment, RemoteEnvironmentError> {
        let url = validate_pairing_url(url)?;

        // 1. Descriptor: learn the host identity + protocol, abort on skew (§4.2).
        let descriptor = self
            .host_client
            .fetch_descriptor(&url)
            .await
            .map_err(descriptor_error)?;
        if descriptor.min_client_protocol > PROTOCOL_VERSION {
            return Err(RemoteEnvironmentError::VersionSkew {
                host_min_client: descriptor.min_client_protocol,
                client: PROTOCOL_VERSION,
            });
        }

        // 2. Pair exchange (single-use code consumption is host-side).
        let response = self
            .host_client
            .pair(
                &url,
                &PairWireRequest {
                    pairing_code: code.to_string(),
                    device_name: client_device_name(),
                    client_version: env!("CARGO_PKG_VERSION").to_string(),
                    requested_scopes: DEFAULT_REQUESTED_SCOPES.to_vec(),
                },
            )
            .await
            .map_err(pair_error)?;
        if response.environment_id != descriptor.environment_id {
            return Err(RemoteEnvironmentError::IdentityMismatch {
                descriptor: descriptor.environment_id,
                response: response.environment_id,
            });
        }

        // 3. Row FIRST, as pending_add (dedup-merges on environment_id, §6.1).
        //
        // Dedup is keyed on the host's SELF-ASSERTED environment_id (readable pre-auth from
        // any host's descriptor), so a host the user is talked into pairing with can claim an
        // existing row and have its token overwrite that row's Keychain entry. `base_url` is
        // preserved, so no traffic is redirected, but the merged row's credential is
        // clobbered. Accepted v1 residual of self-asserted identity — recorded here so it is
        // not mistaken for cross-host verification.
        let env = self
            .repo
            .upsert_paired(UpsertPairedEnvironment {
                environment_id: response.environment_id,
                name: name.to_string(),
                url: url.clone(),
                scopes: response.scopes,
                // Prefer the authenticated exchange's version over the pre-auth descriptor's.
                protocol_version: response
                    .protocol_version
                    .unwrap_or(descriptor.protocol_version),
            })
            .await?;
        // The upsert just moved the row to `pending_add`. If it was the active environment,
        // the mirror must not keep pointing at it while its credential is being replaced.
        self.reset_active_if(env.id.as_str()).await;

        // On a dedup re-pair the same Keychain entry is about to be overwritten;
        // remember the replaced bearer so it can be revoked on the host after the
        // staged add completes (otherwise the previous device would stay
        // valid-but-unreferenced host-side).
        let replaced_token = self
            .secret_store
            .get_secret(&env.token_secret_ref)
            .await
            .ok()
            .flatten()
            .filter(|previous| previous != &response.device_token);

        // 4. Keychain write. On failure the pending_add row stays behind and the
        //    startup reconciler deletes the husk — never a secret without a row.
        self.secret_store
            .put_secret(&env.token_secret_ref, &response.device_token)
            .await?;

        // 5. Flip to active. On failure the reconciler re-validates and activates.
        self.repo
            .set_status(&env.id, RemoteEnvironmentStatus::Active)
            .await?;

        // 6. Best-effort cleanup of the replaced bearer, only after the new one is
        //    fully installed (never before — a failed re-pair must not kill the
        //    working credential).
        if let Some(previous_token) = replaced_token {
            // Against `url`, not `env.base_url`: after a dedup merge the stored base_url is
            // still the OLD preferred endpoint, which is exactly the one that may have died
            // and prompted this re-pair. `url` just completed a descriptor+pair exchange.
            if let Err(error) = self.host_client.revoke_token(&url, &previous_token).await {
                tracing::warn!(
                    environment = env.id.as_str(),
                    %error,
                    "Best-effort revoke of the replaced device token failed"
                );
            }
        }

        Ok(RemoteEnvironment {
            status: RemoteEnvironmentStatus::Active,
            ..env
        })
    }

    // ------------------------------------------------------------------
    // Removal (staged remove machine, §6.1 / P-27)
    // ------------------------------------------------------------------

    /// Removes a paired environment: mark `pending_delete` → best-effort host
    /// revoke → Keychain delete → row delete.
    ///
    /// Keychain failures abort BEFORE the row delete so the row keeps referencing
    /// the secret and the startup reconciler can finish the removal — deleting the
    /// row first would orphan a valid bearer.
    pub async fn remove(&self, id: &str) -> Result<(), RemoteEnvironmentError> {
        let env_id = RemoteEnvironmentId::from_string(id);
        let Some(env) = self.repo.get(&env_id).await? else {
            // Idempotent: removing an unknown environment is a no-op, but never
            // leave the mirror pointing at a gone id.
            self.reset_active_if(id).await;
            return Ok(());
        };

        self.repo
            .set_status(&env.id, RemoteEnvironmentStatus::PendingDelete)
            .await?;
        // Proxy authority dies before any network effect.
        self.reset_active_if(id).await;

        match self.secret_store.get_secret(&env.token_secret_ref).await {
            Ok(Some(token)) => {
                // Best-effort revoke; an unreachable host must not block removal
                // (the reconciler retries on the next startup only if the later
                // Keychain delete fails).
                if let Err(error) = self.host_client.revoke_token(&env.base_url, &token).await {
                    tracing::warn!(
                        environment = env.id.as_str(),
                        %error,
                        "Best-effort remote token revoke failed during removal"
                    );
                }
            }
            Ok(None) => {}
            // Fail closed: cannot prove the secret state — keep the pending_delete
            // row so the reconciler retries.
            Err(error) => return Err(error.into()),
        }

        self.secret_store.delete_secret(&env.token_secret_ref).await?;
        self.repo.delete(&env.id).await?;
        Ok(())
    }

    // ------------------------------------------------------------------
    // Startup reconciler (P-27)
    // ------------------------------------------------------------------

    /// Resolves staged add/remove states left behind by crashes. Runs in the same
    /// app-setup recovery phase as other startup recovery.
    pub async fn reconcile_on_startup(&self) -> RemoteEnvironmentReconcileReport {
        let mut report = RemoteEnvironmentReconcileReport::default();
        let environments = match self.repo.list().await {
            Ok(environments) => environments,
            Err(error) => {
                // Fail closed: an unreadable registry reconciles nothing.
                tracing::warn!(%error, "Remote environment reconciler could not read the registry");
                return report;
            }
        };

        for env in environments {
            match env.status {
                RemoteEnvironmentStatus::Active => {}
                RemoteEnvironmentStatus::PendingAdd => {
                    self.reconcile_pending_add(&env, &mut report).await;
                }
                RemoteEnvironmentStatus::PendingDelete => {
                    self.reconcile_pending_delete(&env, &mut report).await;
                }
            }
        }
        report
    }

    async fn reconcile_pending_add(
        &self,
        env: &RemoteEnvironment,
        report: &mut RemoteEnvironmentReconcileReport,
    ) {
        let row_id = env.id.as_str().to_string();
        let secret = match self.secret_store.get_secret(&env.token_secret_ref).await {
            Ok(secret) => secret,
            Err(error) => {
                // Fail closed: an unreadable Keychain is NOT "no secret". Deleting
                // the row here could orphan a live bearer.
                tracing::warn!(environment = %row_id, %error, "Keychain unreadable; deferring pending_add reconcile");
                report.deferred.push(row_id);
                return;
            }
        };
        let Some(token) = secret else {
            // Crash before the Keychain write: the row is a husk with no bearer.
            match self.repo.delete(&env.id).await {
                Ok(()) => report.deleted_husks.push(row_id),
                Err(error) => {
                    tracing::warn!(environment = %row_id, %error, "Failed to delete pending_add husk");
                    report.deferred.push(row_id);
                }
            }
            return;
        };
        match self.host_client.validate_token(&env.base_url, &token).await {
            Ok(true) => match self
                .repo
                .set_status(&env.id, RemoteEnvironmentStatus::Active)
                .await
            {
                Ok(()) => report.activated.push(row_id),
                Err(error) => {
                    tracing::warn!(environment = %row_id, %error, "Failed to activate reconciled environment");
                    report.deferred.push(row_id);
                }
            },
            Ok(false) => {
                // The host provably refuses this bearer. The row is kept (the dead token is
                // not an orphan hazard), but nothing durable records WHY it is still
                // pending_add — see `needs_repair` — so log it per row rather than only
                // counting it.
                tracing::warn!(
                    environment = %row_id,
                    "Host refused this environment's stored token; it needs re-pairing"
                );
                report.needs_repair.push(row_id);
            }
            Err(error) => {
                // Fail closed: unreachable is not proof in either direction.
                tracing::debug!(environment = %row_id, %error, "Host unreachable; deferring pending_add validation");
                report.deferred.push(row_id);
            }
        }
    }

    async fn reconcile_pending_delete(
        &self,
        env: &RemoteEnvironment,
        report: &mut RemoteEnvironmentReconcileReport,
    ) {
        let row_id = env.id.as_str().to_string();
        match self.secret_store.get_secret(&env.token_secret_ref).await {
            Ok(Some(token)) => {
                // Retry the best-effort revoke, then delete secret before row.
                if let Err(error) = self.host_client.revoke_token(&env.base_url, &token).await {
                    tracing::debug!(environment = %row_id, %error, "Reconciler revoke retry failed (best-effort)");
                }
                if let Err(error) = self.secret_store.delete_secret(&env.token_secret_ref).await
                {
                    // Keychain delete failed: the row must survive to keep the
                    // secret referenced for the next retry.
                    tracing::warn!(environment = %row_id, %error, "Keychain delete failed; deferring removal");
                    report.deferred.push(row_id);
                    return;
                }
                match self.repo.delete(&env.id).await {
                    Ok(()) => report.completed_removals.push(row_id),
                    Err(error) => {
                        tracing::warn!(environment = %row_id, %error, "Row delete failed after secret delete");
                        report.deferred.push(row_id);
                    }
                }
            }
            Ok(None) => match self.repo.delete(&env.id).await {
                Ok(()) => report.completed_removals.push(row_id),
                Err(error) => {
                    tracing::warn!(environment = %row_id, %error, "Row delete failed for secretless pending_delete");
                    report.deferred.push(row_id);
                }
            },
            Err(error) => {
                tracing::warn!(environment = %row_id, %error, "Keychain unreadable; deferring pending_delete reconcile");
                report.deferred.push(row_id);
            }
        }
    }

    // ------------------------------------------------------------------
    // Registry reads
    // ------------------------------------------------------------------

    pub async fn list(&self) -> Result<Vec<RemoteEnvironment>, RemoteEnvironmentError> {
        Ok(self.repo.list().await?)
    }

    // ------------------------------------------------------------------
    // Active-environment mirror (§6.4) + proxy authorization (P-26)
    // ------------------------------------------------------------------

    pub async fn active_environment_id(&self) -> String {
        self.active_environment_id.read().await.clone()
    }

    /// Switches the authoritative active environment. `"local"` is always valid;
    /// a remote id must reference an `active` registry row.
    pub async fn set_active_environment(&self, id: &str) -> Result<(), RemoteEnvironmentError> {
        if id != LOCAL_ENVIRONMENT_ID {
            let env = self
                .repo
                .get(&RemoteEnvironmentId::from_string(id))
                .await?
                .ok_or_else(|| RemoteEnvironmentError::UnknownEnvironment(id.to_string()))?;
            if env.status != RemoteEnvironmentStatus::Active {
                return Err(RemoteEnvironmentError::EnvironmentNotUsable(
                    id.to_string(),
                    env.status.as_str(),
                ));
            }
        }
        *self.active_environment_id.write().await = id.to_string();
        Ok(())
    }

    async fn reset_active_if(&self, id: &str) {
        let mut active = self.active_environment_id.write().await;
        if *active == id {
            *active = LOCAL_ENVIRONMENT_ID.to_string();
        }
    }

    /// Authorizes a proxy call for `id` (P-26). Non-health ops require `id` to
    /// equal the Rust-side active environment **and** a settled `active` row;
    /// health ops only require a registered row that is not being removed.
    /// `"local"` never routes through the remote proxy.
    ///
    /// The status check is load-bearing, not belt-and-braces: the active mirror
    /// can outlive a row's `Active` status (a dedup re-pair demotes the row to
    /// `pending_add` while its Keychain token is mid-replacement), and PR 2.2
    /// hangs the real HTTP transport off this authorization. Without it an
    /// invoke could be signed with a half-installed credential.
    async fn authorize_proxy_target(
        &self,
        id: &str,
        health_op: bool,
    ) -> Result<RemoteEnvironment, RemoteEnvironmentError> {
        if id == LOCAL_ENVIRONMENT_ID {
            return Err(RemoteEnvironmentError::LocalEnvironment);
        }
        let env = self
            .repo
            .get(&RemoteEnvironmentId::from_string(id))
            .await?
            .ok_or_else(|| RemoteEnvironmentError::UnknownEnvironment(id.to_string()))?;
        let usable = if health_op {
            // Health probes are how a background or reconciling environment is
            // observed at all, so `pending_add` stays probeable; a row on its way
            // out does not.
            env.status != RemoteEnvironmentStatus::PendingDelete
        } else {
            env.status == RemoteEnvironmentStatus::Active
        };
        if !usable {
            return Err(RemoteEnvironmentError::EnvironmentNotUsable(
                id.to_string(),
                env.status.as_str(),
            ));
        }
        if !health_op {
            let active = self.active_environment_id.read().await;
            if *active != id {
                return Err(RemoteEnvironmentError::NotActiveEnvironment {
                    requested: id.to_string(),
                    active: active.clone(),
                });
            }
        }
        Ok(env)
    }

    // ------------------------------------------------------------------
    // Proxy command surface (stubs; transport lands in PR 2.2/2.3)
    // ------------------------------------------------------------------

    /// Opens the outbound WS for `id`. The socket body lands in PR 2.3; the stub
    /// still enforces that only a registered, usable environment can be connected.
    pub async fn connect(&self, id: &str) -> Result<(), RemoteEnvironmentError> {
        if id == LOCAL_ENVIRONMENT_ID {
            return Err(RemoteEnvironmentError::LocalEnvironment);
        }
        let env = self
            .repo
            .get(&RemoteEnvironmentId::from_string(id))
            .await?
            .ok_or_else(|| RemoteEnvironmentError::UnknownEnvironment(id.to_string()))?;
        if env.status != RemoteEnvironmentStatus::Active {
            return Err(RemoteEnvironmentError::EnvironmentNotUsable(
                id.to_string(),
                env.status.as_str(),
            ));
        }
        Err(RemoteEnvironmentError::NotConnected)
    }

    /// Closes the outbound WS for `id`. Disconnecting an unconnected environment
    /// is idempotent success; the socket teardown body lands in PR 2.3.
    pub async fn disconnect(&self, id: &str) -> Result<(), RemoteEnvironmentError> {
        if id == LOCAL_ENVIRONMENT_ID {
            return Err(RemoteEnvironmentError::LocalEnvironment);
        }
        self.repo
            .get(&RemoteEnvironmentId::from_string(id))
            .await?
            .ok_or_else(|| RemoteEnvironmentError::UnknownEnvironment(id.to_string()))?;
        Ok(())
    }

    /// Forwards one command invoke to the active environment (§6.3).
    ///
    /// Active-env-bound: a non-active id is rejected BEFORE any transport work or
    /// credential read, so the binding holds independently of what the host does.
    /// `request_id` is passed through verbatim — the host binds it to a `cmd`+args
    /// hash for mutation dedup (§3.3), so re-minting it here would break the client's
    /// only defence against a double-applied mutation.
    ///
    /// A-5: exactly one request. No retry lives on this path.
    pub async fn invoke(
        &self,
        id: &str,
        request_id: &str,
        cmd: &str,
        args: serde_json::Value,
    ) -> Result<RemoteInvokeOutcome, RemoteEnvironmentError> {
        let env = self.authorize_proxy_target(id, false).await?;
        let token = self.bearer_for(&env).await?;
        let response = self
            .host_client
            .invoke(
                &env.base_url,
                &token,
                &InvokeWireRequest {
                    request_id: request_id.to_string(),
                    cmd: cmd.to_string(),
                    args,
                },
            )
            .await
            .map_err(transport_error)?;
        parse_invoke_response(response)
    }

    /// Performs one proxied request against the host (§3.5).
    ///
    /// Health paths (descriptor probe, health probe) are permitted for background
    /// environments; anything else is active-env-bound. The descriptor path keeps its
    /// pre-auth shortcut so a background environment can be probed before its bearer
    /// is usable.
    pub async fn fetch(
        &self,
        id: &str,
        call: RemoteFetchCall,
    ) -> Result<RemoteFetchOutcome, RemoteEnvironmentError> {
        let path = validate_remote_fetch_path(&call.path)?;
        let method = validate_remote_fetch_method(&call.method)?;
        let headers = validate_remote_fetch_headers(&call.headers)?;

        let health_op = path == REMOTE_DESCRIPTOR_PATH || path == REMOTE_HEALTH_PATH;
        let env = self.authorize_proxy_target(id, health_op).await?;

        if path == REMOTE_DESCRIPTOR_PATH {
            let descriptor = self
                .host_client
                .fetch_descriptor(&env.base_url)
                .await
                .map_err(descriptor_error)?;
            let body = serde_json::to_string(&descriptor).map_err(|error| {
                RemoteEnvironmentError::Unreachable(format!(
                    "descriptor serialization failed: {error}"
                ))
            })?;
            return Ok(RemoteFetchOutcome { status: 200, body });
        }

        let token = self.bearer_for(&env).await?;
        let response = self
            .host_client
            .fetch(
                &env.base_url,
                &token,
                &RemoteFetchRequest {
                    path,
                    method,
                    headers,
                    body: call.body,
                },
            )
            .await
            .map_err(transport_error)?;

        // A non-2xx is DATA here, not an error: every migrated call site reads
        // `res.ok`/`res.status` and its own error body. Only 401/403 are lifted into
        // the taxonomy, because those describe the transport's authority rather than
        // the endpoint's answer and the supervisor keys on them (§6.5).
        match response.status {
            401 => Err(RemoteEnvironmentError::Transport {
                code: ErrorCode::RemoteUnauthorized,
                message: format!("host refused this device's credential for {}", call.path),
            }),
            403 => Err(RemoteEnvironmentError::Transport {
                code: ErrorCode::RemoteForbidden,
                message: format!("this device's scopes do not permit {}", call.path),
            }),
            status => Ok(RemoteFetchOutcome {
                status,
                body: response.body,
            }),
        }
    }

    /// Reads the environment's bearer from the Keychain.
    ///
    /// Fail-closed in both directions: an unreadable Keychain surfaces as a secret
    /// -store error and a MISSING secret is `REMOTE_UNAUTHORIZED`, never an
    /// unauthenticated request. The token is returned to this module only — it is
    /// handed to the host client and never enters any serialized response (P-18).
    async fn bearer_for(
        &self,
        env: &RemoteEnvironment,
    ) -> Result<String, RemoteEnvironmentError> {
        self.secret_store
            .get_secret(&env.token_secret_ref)
            .await?
            .ok_or_else(|| RemoteEnvironmentError::MissingCredential(env.id.as_str().to_string()))
    }
}

/// Shape-validates a proxied fetch path.
///
/// The bearer is attached AFTER this runs, so an unvalidated path is a credential
/// -forwarding hazard: an absolute or protocol-relative value would aim an
/// authenticated request at an attacker-chosen origin, and `..` segments would climb
/// out of the remounted `/api` surface into routes §3.5 deliberately never mounts.
/// Mirrors `backendApiUrl`'s guards on the local side.
fn validate_remote_fetch_path(path: &str) -> Result<String, RemoteEnvironmentError> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err(RemoteEnvironmentError::InvalidFetchRequest(
            "path must not be empty".to_string(),
        ));
    }
    if !trimmed.starts_with('/') {
        return Err(RemoteEnvironmentError::InvalidFetchRequest(format!(
            "path must be host-relative and start with '/': {path}"
        )));
    }
    if trimmed.starts_with("//") {
        return Err(RemoteEnvironmentError::InvalidFetchRequest(format!(
            "protocol-relative path is not addressable on the paired host: {path}"
        )));
    }
    // Shape guards apply to the PATH half only, exactly as the local transport's
    // `backendApiPath` splits it. Query VALUES are caller data — workspace file paths, commit
    // shas — that legitimately contain dots and colons; rejecting them here would make a file
    // named `notes..md` fetchable locally and not remotely, which is the parity the transport
    // exists to guarantee. A query cannot escape the mounted path either way.
    let path_part = trimmed.split('?').next().unwrap_or(trimmed);
    if path_part.contains("://") {
        return Err(RemoteEnvironmentError::InvalidFetchRequest(format!(
            "absolute URL is not a host path: {path}"
        )));
    }
    if path_part.contains("..") {
        return Err(RemoteEnvironmentError::InvalidFetchRequest(format!(
            "path traversal is not permitted: {path}"
        )));
    }
    if trimmed
        .chars()
        .any(|ch| ch.is_control() || ch.is_whitespace())
    {
        return Err(RemoteEnvironmentError::InvalidFetchRequest(format!(
            "path contains control or whitespace characters: {path}"
        )));
    }
    Ok(trimmed.to_string())
}

fn validate_remote_fetch_method(method: &str) -> Result<String, RemoteEnvironmentError> {
    let upper = method.trim().to_ascii_uppercase();
    if ALLOWED_FETCH_METHODS.contains(&upper.as_str()) {
        return Ok(upper);
    }
    Err(RemoteEnvironmentError::InvalidFetchRequest(format!(
        "method {method} is not forwardable"
    )))
}

fn validate_remote_fetch_headers(
    headers: &[(String, String)],
) -> Result<Vec<(String, String)>, RemoteEnvironmentError> {
    let mut out = Vec::with_capacity(headers.len());
    for (name, value) in headers {
        let lower = name.trim().to_ascii_lowercase();
        if !ALLOWED_FETCH_HEADERS.contains(&lower.as_str()) {
            return Err(RemoteEnvironmentError::InvalidFetchRequest(format!(
                "header {name} may not be forwarded on an authenticated proxy request"
            )));
        }
        if value.chars().any(char::is_control) {
            return Err(RemoteEnvironmentError::InvalidFetchRequest(format!(
                "header {name} has a control character in its value"
            )));
        }
        out.push((lower, value.clone()));
    }
    Ok(out)
}

/// Maps a client→host wire failure into the transport taxonomy.
///
/// `Timeout` is deliberately NOT `REMOTE_UNREACHABLE`: the request was sent, so the
/// mutation may already be committed host-side. Collapsing it into "unreachable"
/// would invite a caller to resend something that already ran (§3.3).
fn transport_error(error: RemoteHostClientError) -> RemoteEnvironmentError {
    match error {
        RemoteHostClientError::Timeout(message) => RemoteEnvironmentError::Transport {
            code: ErrorCode::RemoteTimeoutUnknown,
            message,
        },
        RemoteHostClientError::Unreachable(message) => RemoteEnvironmentError::Transport {
            code: ErrorCode::RemoteUnreachable,
            message,
        },
        RemoteHostClientError::Rejected { status, message } => {
            RemoteEnvironmentError::Transport {
                code: status_error_code(status),
                message,
            }
        }
        RemoteHostClientError::InvalidResponse(message) => RemoteEnvironmentError::Transport {
            code: ErrorCode::RemoteVersionMismatch,
            message,
        },
    }
}

/// HTTP status → taxonomy, for a host that answered without a typed body.
fn status_error_code(status: u16) -> ErrorCode {
    match status {
        401 => ErrorCode::RemoteUnauthorized,
        403 => ErrorCode::RemoteForbidden,
        404 | 501 => ErrorCode::RemoteCommandUnavailable,
        408 | 504 => ErrorCode::RemoteTimeoutUnknown,
        409 => ErrorCode::RemoteRequestInProgress,
        422 => ErrorCode::RemoteRequestIdReused,
        426 | 505 => ErrorCode::RemoteVersionMismatch,
        _ => ErrorCode::RemoteUnreachable,
    }
}

/// Parses a taxonomy code out of a host error body, if it carries one.
fn body_error_code(body: &serde_json::Value) -> Option<ErrorCode> {
    let code = body.get("code")?.as_str()?;
    serde_json::from_value::<ErrorCode>(serde_json::Value::String(code.to_string())).ok()
}

fn body_error_message(body: &serde_json::Value, fallback: &str) -> String {
    body.get("message")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| fallback.to_string())
}

/// Interprets the host's `/remote/v1/invoke` answer (§3.1 / registry expansion
/// contract (f)/(g)).
///
/// Tolerant on purpose: a host that answers with the bare command result is accepted
/// alongside the `{ok, result}` / `{ok:false, error}` envelope, so the client is not
/// pinned to a body shape PR 1.3 has not finished landing. What is NOT tolerant is
/// the error direction — a non-2xx always produces a typed taxonomy code, never a
/// success with an error-shaped body.
fn parse_invoke_response(
    response: RemoteHttpResponse,
) -> Result<RemoteInvokeOutcome, RemoteEnvironmentError> {
    let parsed: Option<serde_json::Value> = serde_json::from_str(&response.body).ok();

    if !(200..300).contains(&response.status) {
        let body = parsed.unwrap_or(serde_json::Value::Null);
        let code = body_error_code(&body).unwrap_or_else(|| status_error_code(response.status));
        let message = body_error_message(
            &body,
            &format!("host answered {} for this command", response.status),
        );
        return Err(RemoteEnvironmentError::Transport { code, message });
    }

    let Some(body) = parsed else {
        return Err(RemoteEnvironmentError::Transport {
            code: ErrorCode::RemoteVersionMismatch,
            message: "host answered with a body this client cannot parse as JSON".to_string(),
        });
    };

    match body.get("ok").and_then(serde_json::Value::as_bool) {
        Some(false) => Ok(RemoteInvokeOutcome::CommandError {
            error: body
                .get("error")
                .cloned()
                .unwrap_or(serde_json::Value::Null),
        }),
        Some(true) => Ok(RemoteInvokeOutcome::Ok {
            result: body
                .get("result")
                .cloned()
                .unwrap_or(serde_json::Value::Null),
        }),
        // No envelope: the host returned the command result directly.
        None => Ok(RemoteInvokeOutcome::Ok { result: body }),
    }
}

/// Shape-validates a pairing URL: http(s), a host, and nothing else is required.
/// Pairing inputs never reach filesystem or process sinks (C-5) — this guards the
/// network sink only.
fn validate_pairing_url(url: &str) -> Result<String, RemoteEnvironmentError> {
    let trimmed = url.trim();
    let parsed: hyper::Uri = trimmed
        .parse()
        .map_err(|error| RemoteEnvironmentError::InvalidUrl(format!("{error}")))?;
    match parsed.scheme_str() {
        Some("http") | Some("https") => {}
        other => {
            return Err(RemoteEnvironmentError::InvalidUrl(format!(
                "unsupported scheme: {}",
                other.unwrap_or("none")
            )))
        }
    }
    if parsed.host().is_none() {
        return Err(RemoteEnvironmentError::InvalidUrl(
            "missing host".to_string(),
        ));
    }
    Ok(trimmed.trim_end_matches('/').to_string())
}

fn client_device_name() -> String {
    format!("RalphX Desktop {}", env!("CARGO_PKG_VERSION"))
}

fn descriptor_error(error: RemoteHostClientError) -> RemoteEnvironmentError {
    match error {
        RemoteHostClientError::Unreachable(message)
        | RemoteHostClientError::Timeout(message) => {
            RemoteEnvironmentError::Unreachable(message)
        }
        RemoteHostClientError::Rejected { status, message } => {
            RemoteEnvironmentError::Unreachable(format!(
                "descriptor request refused ({status}): {message}"
            ))
        }
        RemoteHostClientError::InvalidResponse(message) => {
            RemoteEnvironmentError::Unreachable(format!("invalid descriptor: {message}"))
        }
    }
}

fn pair_error(error: RemoteHostClientError) -> RemoteEnvironmentError {
    match error {
        RemoteHostClientError::Unreachable(message)
        | RemoteHostClientError::Timeout(message) => {
            RemoteEnvironmentError::Unreachable(message)
        }
        RemoteHostClientError::Rejected { status, message } => {
            RemoteEnvironmentError::PairRejected(format!("({status}) {message}"))
        }
        RemoteHostClientError::InvalidResponse(message) => {
            RemoteEnvironmentError::PairRejected(format!("invalid pair response: {message}"))
        }
    }
}

#[cfg(test)]
#[path = "remote_environment_service_tests.rs"]
mod tests;
