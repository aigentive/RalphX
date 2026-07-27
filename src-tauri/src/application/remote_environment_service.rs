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
    PairWireRequest, RemoteHostClient, RemoteHostClientError, REMOTE_DESCRIPTOR_PATH,
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
    /// `pending_add` rows whose token the host explicitly refused → surfaced for
    /// re-pair (row kept so the UI can offer it).
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
        let env = self
            .repo
            .upsert_paired(UpsertPairedEnvironment {
                environment_id: response.environment_id,
                name: name.to_string(),
                url,
                scopes: response.scopes,
                protocol_version: descriptor.protocol_version,
            })
            .await?;

        // 4. Keychain write. On failure the pending_add row stays behind and the
        //    startup reconciler deletes the husk — never a secret without a row.
        self.secret_store
            .put_secret(&env.token_secret_ref, &response.device_token)
            .await?;

        // 5. Flip to active. On failure the reconciler re-validates and activates.
        self.repo
            .set_status(&env.id, RemoteEnvironmentStatus::Active)
            .await?;

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
                // The host provably refuses this bearer — keep the row so the UI
                // can surface a re-pair; the dead token is not an orphan hazard.
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
    /// equal the Rust-side active environment; health ops only require a
    /// registered environment. `"local"` never routes through the remote proxy.
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

    /// Forwards one command invoke to the active environment (HTTP path lands in
    /// PR 2.2). Active-env-bound: a non-active id is rejected BEFORE any
    /// transport work, so the binding is proven independently of the stub.
    pub async fn invoke(
        &self,
        id: &str,
        _request_id: &str,
        _cmd: &str,
        _args: serde_json::Value,
    ) -> Result<serde_json::Value, RemoteEnvironmentError> {
        self.authorize_proxy_target(id, false).await?;
        Err(RemoteEnvironmentError::NotConnected)
    }

    /// Fetches a host resource. Health paths (descriptor probe, health probe) are
    /// permitted for background environments; anything else is active-env-bound.
    pub async fn fetch(
        &self,
        id: &str,
        path: &str,
    ) -> Result<serde_json::Value, RemoteEnvironmentError> {
        let health_op = path == REMOTE_DESCRIPTOR_PATH || path == REMOTE_HEALTH_PATH;
        let env = self.authorize_proxy_target(id, health_op).await?;
        if path == REMOTE_DESCRIPTOR_PATH {
            let descriptor = self
                .host_client
                .fetch_descriptor(&env.base_url)
                .await
                .map_err(descriptor_error)?;
            return serde_json::to_value(&descriptor).map_err(|error| {
                RemoteEnvironmentError::Unreachable(format!(
                    "descriptor serialization failed: {error}"
                ))
            });
        }
        // Authenticated fetch paths need the bearer-holding transport (PR 2.2).
        Err(RemoteEnvironmentError::NotConnected)
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
        RemoteHostClientError::Unreachable(message) => {
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
        RemoteHostClientError::Unreachable(message) => {
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
