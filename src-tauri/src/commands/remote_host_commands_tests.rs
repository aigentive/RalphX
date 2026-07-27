use std::net::{Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;

use super::advertised_endpoints_for_status;
use crate::infrastructure::tailscale::{TailscaleCommandRunner, TailscaleServeError};
use crate::remote_server::settings::{RemoteExposureMode, TailnetProviderError};

struct StatusRunner {
    result: Result<String, TailnetProviderError>,
    calls: AtomicUsize,
}

impl StatusRunner {
    fn failing() -> Self {
        Self {
            result: Err(TailnetProviderError::Unavailable(
                "tailscale CLI is unavailable".to_string(),
            )),
            calls: AtomicUsize::new(0),
        }
    }

    fn returning(stdout: &str) -> Self {
        Self {
            result: Ok(stdout.to_string()),
            calls: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl TailscaleCommandRunner for StatusRunner {
    async fn run_status(&self) -> Result<String, TailnetProviderError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.result.clone()
    }

    async fn run_serve_acquire(&self, _port: u16) -> Result<(), TailscaleServeError> {
        unreachable!("endpoint listing must not acquire Serve")
    }

    async fn run_serve_release(&self) -> Result<(), TailscaleServeError> {
        unreachable!("endpoint listing must not release Serve")
    }
}

#[tokio::test]
async fn listener_not_running_returns_empty_without_querying_tailscale() {
    let runner = StatusRunner::failing();

    let endpoints =
        advertised_endpoints_for_status(RemoteExposureMode::Serve, None, false, &runner).await;

    assert!(endpoints.is_empty());
    assert_eq!(runner.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn tailscale_status_failure_is_a_degraded_empty_result_not_an_error() {
    let runner = StatusRunner::failing();
    let bound = SocketAddr::from((Ipv4Addr::LOCALHOST, 48_912));

    let endpoints =
        advertised_endpoints_for_status(RemoteExposureMode::Serve, Some(bound), true, &runner)
            .await;

    assert!(endpoints.is_empty());
    assert_eq!(runner.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn populated_endpoint_serializes_with_the_frontend_field_names_and_kind_casing() {
    let runner = StatusRunner::returning(
        r#"{
            "Version": "1.82.0",
            "BackendState": "Running",
            "Self": {
                "DNSName": "studio.example.ts.net.",
                "TailscaleIPs": ["100.101.102.103"]
            }
        }"#,
    );
    let bound = SocketAddr::from((Ipv4Addr::LOCALHOST, 48_912));

    let endpoints =
        advertised_endpoints_for_status(RemoteExposureMode::Serve, Some(bound), true, &runner)
            .await;
    let json = serde_json::to_value(&endpoints[0]).expect("endpoint should serialize");

    assert_eq!(
        json,
        serde_json::json!({
            "kind": "loopbackServe",
            "url": "https://studio.example.ts.net",
            "available": true,
        })
    );
}
