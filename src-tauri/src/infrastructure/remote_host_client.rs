// RemoteHostClient — trait abstraction for the client→host pairing/auth wire (§4.2).
//
// Trait-based (like WebhookHttpClient) so the pairing/reconciler logic can run against
// a scripted mock host in unit tests. Production uses hyper 1.x (no reqwest); it must
// speak BOTH https (Serve / MagicDNS certs) and plain http (direct 100.x endpoints).
//
// The device token flows through this seam only between the host response and the
// Keychain write — it is never returned to the webview (P-18).

use std::sync::Mutex;

use async_trait::async_trait;
use http_body_util::{BodyExt, Full};
use hyper::{Method, Request, StatusCode};
use hyper_rustls::HttpsConnector;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use ralphx_remote_protocol::{EnvironmentDescriptor, Scope};
use serde::{Deserialize, Serialize};
use tokio::time::Duration;
use tokio_util::bytes::Bytes;

/// Pre-auth descriptor route (mirrors `remote_server::DESCRIPTOR_PATH`).
pub const REMOTE_DESCRIPTOR_PATH: &str = "/.well-known/ralphx/environment";
/// Pairing exchange route (§4.2).
pub const REMOTE_PAIR_PATH: &str = "/remote/v1/auth/pair";
/// Bearer-authenticated session introspection route; a 200 proves the token is live.
pub const REMOTE_SESSION_PATH: &str = "/remote/v1/session";
/// Self-revocation route used by the staged remove machine (best-effort).
pub const REMOTE_REVOKE_PATH: &str = "/remote/v1/auth/revoke";

/// Wire request for `POST /remote/v1/auth/pair` (§4.2, C-11: camelCase).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairWireRequest {
    pub pairing_code: String,
    pub device_name: String,
    pub requested_scopes: Vec<Scope>,
}

/// Wire response for `POST /remote/v1/auth/pair` (§4.2, C-11: camelCase).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairWireResponse {
    /// The long-term device token (`rxd_live_…`). Keychain-bound; never reaches JS.
    pub device_token: String,
    pub device_id: String,
    pub scopes: Vec<Scope>,
    pub environment_id: String,
}

/// Typed failures from the client→host wire.
#[derive(Debug, Clone, thiserror::Error)]
pub enum RemoteHostClientError {
    #[error("host unreachable: {0}")]
    Unreachable(String),
    /// The host answered with a non-success status (bad pairing code, revoked token, …).
    #[error("host rejected the request ({status}): {message}")]
    Rejected { status: u16, message: String },
    #[error("host returned an invalid response: {0}")]
    InvalidResponse(String),
}

/// Client side of the host pairing/auth surface.
#[async_trait]
pub trait RemoteHostClient: Send + Sync {
    /// `GET /.well-known/ralphx/environment` — pre-auth identity + version negotiation.
    async fn fetch_descriptor(
        &self,
        base_url: &str,
    ) -> Result<EnvironmentDescriptor, RemoteHostClientError>;

    /// `POST /remote/v1/auth/pair` — exchanges a single-use pairing code for a device token.
    async fn pair(
        &self,
        base_url: &str,
        request: &PairWireRequest,
    ) -> Result<PairWireResponse, RemoteHostClientError>;

    /// Proves whether `token` is still a live bearer on the host.
    ///
    /// `Ok(true)` = live, `Ok(false)` = the host explicitly refused it (401/403);
    /// transport failures stay errors so callers can fail closed instead of
    /// mistaking an unreachable host for a revoked token.
    async fn validate_token(
        &self,
        base_url: &str,
        token: &str,
    ) -> Result<bool, RemoteHostClientError>;

    /// Best-effort self-revocation of `token` on the host (staged remove, P-27).
    async fn revoke_token(&self, base_url: &str, token: &str)
        -> Result<(), RemoteHostClientError>;
}

// ============================================================================
// Production implementation using hyper 1.x
// ============================================================================

pub struct HyperRemoteHostClient {
    client: Client<HttpsConnector<HttpConnector>, Full<Bytes>>,
    request_timeout: Duration,
}

fn install_rustls_crypto_provider() {
    static INSTALL_PROVIDER: std::sync::Once = std::sync::Once::new();
    INSTALL_PROVIDER.call_once(|| {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    });
}

impl HyperRemoteHostClient {
    pub fn new() -> Result<Self, String> {
        install_rustls_crypto_provider();
        let mut connector = HttpConnector::new();
        connector.set_connect_timeout(Some(Duration::from_secs(10)));
        connector.enforce_http(false);
        let https = hyper_rustls::HttpsConnectorBuilder::new()
            .with_native_roots()
            .map_err(|error| format!("native root certificates unavailable: {error}"))?
            .https_or_http()
            .enable_http1()
            .wrap_connector(connector);
        Ok(Self {
            client: Client::builder(TokioExecutor::new()).build(https),
            request_timeout: Duration::from_secs(15),
        })
    }

    async fn request_json(
        &self,
        method: Method,
        url: &str,
        bearer: Option<&str>,
        body: Option<Vec<u8>>,
    ) -> Result<(StatusCode, Vec<u8>), RemoteHostClientError> {
        let uri: hyper::Uri = url
            .parse()
            .map_err(|error| RemoteHostClientError::Unreachable(format!("invalid URL: {error}")))?;
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header("Content-Type", "application/json");
        if let Some(bearer) = bearer {
            builder = builder.header("Authorization", format!("Bearer {bearer}"));
        }
        let request = builder
            .body(Full::new(Bytes::from(body.unwrap_or_default())))
            .map_err(|error| {
                RemoteHostClientError::Unreachable(format!("build request: {error}"))
            })?;
        let response = tokio::time::timeout(self.request_timeout, self.client.request(request))
            .await
            .map_err(|_| {
                RemoteHostClientError::Unreachable(format!(
                    "request timed out after {}s",
                    self.request_timeout.as_secs()
                ))
            })?
            .map_err(|error| RemoteHostClientError::Unreachable(error.to_string()))?;
        let status = response.status();
        let body = response
            .into_body()
            .collect()
            .await
            .map_err(|error| RemoteHostClientError::Unreachable(error.to_string()))?
            .to_bytes()
            .to_vec();
        Ok((status, body))
    }
}

fn join_url(base_url: &str, path: &str) -> String {
    format!("{}{}", base_url.trim_end_matches('/'), path)
}

fn rejected(status: StatusCode, body: &[u8]) -> RemoteHostClientError {
    RemoteHostClientError::Rejected {
        status: status.as_u16(),
        message: String::from_utf8_lossy(body).chars().take(300).collect(),
    }
}

#[async_trait]
impl RemoteHostClient for HyperRemoteHostClient {
    async fn fetch_descriptor(
        &self,
        base_url: &str,
    ) -> Result<EnvironmentDescriptor, RemoteHostClientError> {
        let url = join_url(base_url, REMOTE_DESCRIPTOR_PATH);
        let (status, body) = self.request_json(Method::GET, &url, None, None).await?;
        if !status.is_success() {
            return Err(rejected(status, &body));
        }
        serde_json::from_slice(&body)
            .map_err(|error| RemoteHostClientError::InvalidResponse(error.to_string()))
    }

    async fn pair(
        &self,
        base_url: &str,
        request: &PairWireRequest,
    ) -> Result<PairWireResponse, RemoteHostClientError> {
        let url = join_url(base_url, REMOTE_PAIR_PATH);
        let body = serde_json::to_vec(request)
            .map_err(|error| RemoteHostClientError::InvalidResponse(error.to_string()))?;
        let (status, body) = self
            .request_json(Method::POST, &url, None, Some(body))
            .await?;
        if !status.is_success() {
            return Err(rejected(status, &body));
        }
        serde_json::from_slice(&body)
            .map_err(|error| RemoteHostClientError::InvalidResponse(error.to_string()))
    }

    async fn validate_token(
        &self,
        base_url: &str,
        token: &str,
    ) -> Result<bool, RemoteHostClientError> {
        let url = join_url(base_url, REMOTE_SESSION_PATH);
        let (status, body) = self
            .request_json(Method::GET, &url, Some(token), None)
            .await?;
        if status.is_success() {
            return Ok(true);
        }
        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            return Ok(false);
        }
        Err(rejected(status, &body))
    }

    async fn revoke_token(
        &self,
        base_url: &str,
        token: &str,
    ) -> Result<(), RemoteHostClientError> {
        let url = join_url(base_url, REMOTE_REVOKE_PATH);
        let (status, body) = self
            .request_json(Method::POST, &url, Some(token), None)
            .await?;
        // A host that no longer knows the token has nothing left to revoke.
        if status.is_success()
            || status == StatusCode::UNAUTHORIZED
            || status == StatusCode::FORBIDDEN
            || status == StatusCode::NOT_FOUND
        {
            return Ok(());
        }
        Err(rejected(status, &body))
    }
}

/// Fallback client used when TLS roots are unavailable at AppState construction
/// (mirrors `UnavailableAtlassianApiClient`): every call fails typed-unreachable.
pub struct UnavailableRemoteHostClient {
    reason: String,
}

impl UnavailableRemoteHostClient {
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }
}

#[async_trait]
impl RemoteHostClient for UnavailableRemoteHostClient {
    async fn fetch_descriptor(
        &self,
        _base_url: &str,
    ) -> Result<EnvironmentDescriptor, RemoteHostClientError> {
        Err(RemoteHostClientError::Unreachable(self.reason.clone()))
    }

    async fn pair(
        &self,
        _base_url: &str,
        _request: &PairWireRequest,
    ) -> Result<PairWireResponse, RemoteHostClientError> {
        Err(RemoteHostClientError::Unreachable(self.reason.clone()))
    }

    async fn validate_token(
        &self,
        _base_url: &str,
        _token: &str,
    ) -> Result<bool, RemoteHostClientError> {
        Err(RemoteHostClientError::Unreachable(self.reason.clone()))
    }

    async fn revoke_token(
        &self,
        _base_url: &str,
        _token: &str,
    ) -> Result<(), RemoteHostClientError> {
        Err(RemoteHostClientError::Unreachable(self.reason.clone()))
    }
}

// ============================================================================
// Test mock — the "mock host" the pairing/reconciler tests run against
// ============================================================================

/// Scripted response for one mock-host call.
pub type MockHostResult<T> = Result<T, RemoteHostClientError>;

/// Recorded call log entry for assertion in tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordedHostCall {
    Descriptor { base_url: String },
    Pair { base_url: String, request: PairWireRequest },
    Validate { base_url: String, token: String },
    Revoke { base_url: String, token: String },
}

/// Mock host for pairing/reconciler tests: scripted responses + a recorded call log.
pub struct MockRemoteHostClient {
    pub descriptor: Mutex<MockHostResult<EnvironmentDescriptor>>,
    pub pair_response: Mutex<MockHostResult<PairWireResponse>>,
    pub validate_response: Mutex<MockHostResult<bool>>,
    pub revoke_response: Mutex<MockHostResult<()>>,
    pub calls: Mutex<Vec<RecordedHostCall>>,
}

impl MockRemoteHostClient {
    pub fn new(descriptor: EnvironmentDescriptor, pair_response: PairWireResponse) -> Self {
        Self {
            descriptor: Mutex::new(Ok(descriptor)),
            pair_response: Mutex::new(Ok(pair_response)),
            validate_response: Mutex::new(Ok(true)),
            revoke_response: Mutex::new(Ok(())),
            calls: Mutex::new(Vec::new()),
        }
    }

    /// A mock host whose every surface fails with `Unreachable`.
    pub fn unreachable() -> Self {
        let error = RemoteHostClientError::Unreachable("mock host offline".to_string());
        Self {
            descriptor: Mutex::new(Err(error.clone())),
            pair_response: Mutex::new(Err(error.clone())),
            validate_response: Mutex::new(Err(error.clone())),
            revoke_response: Mutex::new(Err(error)),
            calls: Mutex::new(Vec::new()),
        }
    }

    pub fn recorded_calls(&self) -> Vec<RecordedHostCall> {
        self.calls.lock().expect("mock call log poisoned").clone()
    }

    fn record(&self, call: RecordedHostCall) {
        self.calls.lock().expect("mock call log poisoned").push(call);
    }
}

#[async_trait]
impl RemoteHostClient for MockRemoteHostClient {
    async fn fetch_descriptor(
        &self,
        base_url: &str,
    ) -> Result<EnvironmentDescriptor, RemoteHostClientError> {
        self.record(RecordedHostCall::Descriptor {
            base_url: base_url.to_string(),
        });
        self.descriptor
            .lock()
            .expect("mock descriptor poisoned")
            .clone()
    }

    async fn pair(
        &self,
        base_url: &str,
        request: &PairWireRequest,
    ) -> Result<PairWireResponse, RemoteHostClientError> {
        self.record(RecordedHostCall::Pair {
            base_url: base_url.to_string(),
            request: request.clone(),
        });
        self.pair_response
            .lock()
            .expect("mock pair response poisoned")
            .clone()
    }

    async fn validate_token(
        &self,
        base_url: &str,
        token: &str,
    ) -> Result<bool, RemoteHostClientError> {
        self.record(RecordedHostCall::Validate {
            base_url: base_url.to_string(),
            token: token.to_string(),
        });
        self.validate_response
            .lock()
            .expect("mock validate response poisoned")
            .clone()
    }

    async fn revoke_token(
        &self,
        base_url: &str,
        token: &str,
    ) -> Result<(), RemoteHostClientError> {
        self.record(RecordedHostCall::Revoke {
            base_url: base_url.to_string(),
            token: token.to_string(),
        });
        self.revoke_response
            .lock()
            .expect("mock revoke response poisoned")
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // C-11: parity tests use REAL serialization shapes — explicit camelCase keys,
    // protocol scope strings — not mock-convenient shapes.
    #[test]
    fn pair_request_serializes_the_documented_wire_shape() {
        let request = PairWireRequest {
            pairing_code: "rxp_0123456789abcdefghijklmnopqrstuv".to_string(),
            device_name: "RalphX Desktop".to_string(),
            requested_scopes: vec![Scope::UiRead, Scope::UiOperate],
        };
        let json = serde_json::to_value(&request).expect("request should serialize");
        assert_eq!(
            json,
            serde_json::json!({
                "pairingCode": "rxp_0123456789abcdefghijklmnopqrstuv",
                "deviceName": "RalphX Desktop",
                "requestedScopes": ["ui:read", "ui:operate"],
            })
        );
    }

    #[test]
    fn pair_response_parses_the_documented_wire_shape() {
        let raw = r#"{
            "deviceToken": "rxd_live_secret",
            "deviceId": "device-1",
            "scopes": ["ui:read", "ui:operate"],
            "environmentId": "env-1"
        }"#;
        let response: PairWireResponse =
            serde_json::from_str(raw).expect("response should parse");
        assert_eq!(response.device_token, "rxd_live_secret");
        assert_eq!(response.device_id, "device-1");
        assert_eq!(response.scopes, vec![Scope::UiRead, Scope::UiOperate]);
        assert_eq!(response.environment_id, "env-1");
    }

    #[test]
    fn join_url_tolerates_trailing_slashes() {
        assert_eq!(
            join_url("https://mac-studio.tailnet.ts.net/", REMOTE_PAIR_PATH),
            "https://mac-studio.tailnet.ts.net/remote/v1/auth/pair"
        );
        assert_eq!(
            join_url("http://100.101.102.103:3849", REMOTE_DESCRIPTOR_PATH),
            "http://100.101.102.103:3849/.well-known/ralphx/environment"
        );
    }
}
