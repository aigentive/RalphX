// WebhookHttpClient — trait abstraction for HTTP POST delivery.
//
// Trait-based design allows swapping the production Hyper implementation
// for a mock in unit tests without modifying WebhookPublisher.
//
// Production: HyperWebhookClient using hyper 1.x (no reqwest — see external_mcp_supervisor.rs)
// Test: MockWebhookHttpClient with configurable response codes and call recording.

use async_trait::async_trait;
use tokio_util::bytes::Bytes;
use http_body_util::Full;
use hyper::{Method, Request};
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use std::collections::HashMap;
use std::sync::Mutex;
use thiserror::Error;
use tokio::time::Duration;

/// Errors from webhook HTTP delivery.
#[derive(Debug, Error)]
pub enum WebhookDeliveryError {
    #[error("HTTP request failed: {0}")]
    Request(String),
    #[error("Delivery timed out after {0}s")]
    Timeout(u64),
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),
}

/// Abstraction for making HTTP POST requests to webhook endpoints.
///
/// Trait-based so tests can inject a mock without network calls.
#[async_trait]
pub trait WebhookHttpClient: Send + Sync {
    /// POST JSON body to the given URL with provided headers.
    ///
    /// Returns the HTTP status code on success, or a delivery error on failure.
    ///
    /// # Errors
    ///
    /// Returns [`WebhookDeliveryError::InvalidUrl`] if the URL cannot be parsed.
    /// Returns [`WebhookDeliveryError::Request`] if the HTTP request fails.
    /// Returns [`WebhookDeliveryError::Timeout`] if the request exceeds the timeout.
    async fn post(
        &self,
        url: &str,
        body: Vec<u8>,
        headers: HashMap<String, String>,
    ) -> Result<u16, WebhookDeliveryError>;
}

// ============================================================================
// Production implementation using hyper 1.x
// ============================================================================

/// Production webhook HTTP client backed by hyper 1.x.
///
/// Uses `hyper_util::client::legacy::Client` which provides connection pooling.
/// Does NOT use reqwest — codebase avoids it (consistent with external_mcp_supervisor.rs).
pub struct HyperWebhookClient {
    client: Client<HttpConnector, Full<Bytes>>,
    /// Total request timeout (default 30s)
    request_timeout: Duration,
}

impl Default for HyperWebhookClient {
    fn default() -> Self {
        Self::new()
    }
}

impl HyperWebhookClient {
    /// Create a new client with default timeouts (10s connect, 30s total).
    pub fn new() -> Self {
        let mut connector = HttpConnector::new();
        connector.set_connect_timeout(Some(Duration::from_secs(10)));
        let client = Client::builder(TokioExecutor::new()).build::<_, Full<Bytes>>(connector);
        Self {
            client,
            request_timeout: Duration::from_secs(30),
        }
    }
}

#[async_trait]
impl WebhookHttpClient for HyperWebhookClient {
    async fn post(
        &self,
        url: &str,
        body: Vec<u8>,
        headers: HashMap<String, String>,
    ) -> Result<u16, WebhookDeliveryError> {
        let uri: hyper::Uri = url
            .parse()
            .map_err(|e| WebhookDeliveryError::InvalidUrl(format!("{e}")))?;

        let body_bytes = Bytes::from(body);
        let mut builder = Request::builder()
            .method(Method::POST)
            .uri(uri)
            .header("Content-Type", "application/json");

        for (key, value) in &headers {
            builder = builder.header(key.as_str(), value.as_str());
        }

        let request = builder
            .body(Full::new(body_bytes))
            .map_err(|e| WebhookDeliveryError::Request(format!("build request: {e}")))?;

        let timeout = self.request_timeout;
        match tokio::time::timeout(timeout, self.client.request(request)).await {
            Ok(Ok(response)) => Ok(response.status().as_u16()),
            Ok(Err(e)) => Err(WebhookDeliveryError::Request(e.to_string())),
            Err(_) => Err(WebhookDeliveryError::Timeout(timeout.as_secs())),
        }
    }
}

// ============================================================================
// Test mock
// ============================================================================

/// Recorded call in [`MockWebhookHttpClient`].
#[derive(Debug, Clone)]
pub struct RecordedCall {
    pub url: String,
    pub body: Vec<u8>,
    pub headers: HashMap<String, String>,
}

/// Mock webhook HTTP client for unit tests.
///
/// Returns a configurable status code for all requests and records all calls
/// so tests can assert on delivery behavior.
pub struct MockWebhookHttpClient {
    /// Status code to return. Default 200.
    pub status_code: u16,
    /// Recorded calls for assertion in tests.
    pub calls: Mutex<Vec<RecordedCall>>,
    /// If true, return an error instead of status code.
    pub force_error: bool,
}

impl Default for MockWebhookHttpClient {
    fn default() -> Self {
        Self::new(200)
    }
}

impl MockWebhookHttpClient {
    /// Create a mock that returns the given status code.
    pub fn new(status_code: u16) -> Self {
        Self {
            status_code,
            calls: Mutex::new(Vec::new()),
            force_error: false,
        }
    }

    /// Return an error on all calls (simulates connection failure).
    pub fn with_error() -> Self {
        Self {
            status_code: 0,
            calls: Mutex::new(Vec::new()),
            force_error: true,
        }
    }

    /// Return the number of recorded calls.
    pub fn call_count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }
}

#[async_trait]
impl WebhookHttpClient for MockWebhookHttpClient {
    async fn post(
        &self,
        url: &str,
        body: Vec<u8>,
        headers: HashMap<String, String>,
    ) -> Result<u16, WebhookDeliveryError> {
        self.calls.lock().unwrap().push(RecordedCall {
            url: url.to_string(),
            body,
            headers,
        });
        if self.force_error {
            return Err(WebhookDeliveryError::Request(
                "simulated connection error".to_string(),
            ));
        }
        Ok(self.status_code)
    }
}

