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

/// §3.4: "if host mode was never configured, capture is not installed — no cost". Configured IS
/// the settings row, so the pane's mount reads must never mint it — otherwise merely OPENING
/// Settings → Remote Access buys capture, the sequencer, the pruner, and durable event persistence
/// on every subsequent boot.
#[test]
fn the_status_reads_never_configure_host_mode() {
    let source = include_str!("remote_host_commands.rs");
    for command in [
        "pub async fn get_remote_listener_status",
        "pub async fn list_remote_advertised_endpoints",
    ] {
        let body = source
            .split(command)
            .nth(1)
            .unwrap_or_else(|| panic!("{command} should exist"));
        let end = body.find("\n}\n").expect("the command body should end");
        // Match the call form `.get_or_create()` so the guard catches real reads while ignoring
        // the explanatory comment inside `list_remote_advertised_endpoints` that names the method.
        assert!(
            !body[..end].contains(".get_or_create()"),
            "{command} must read with get(), not mint the settings row"
        );
    }
}

#[test]
fn an_unconfigured_host_reports_a_disabled_status_instead_of_configuring_itself() {
    let status = super::unconfigured_status();

    assert!(!status.enabled);
    assert!(!status.running);
    assert!(status.bind_address.is_none());
    assert!(!status.serve_active);
    assert!(status.serve_degraded_kind.is_none());
    assert!(status.environment_id.is_empty());
    assert_eq!(
        status.port,
        crate::remote_server::settings::DEFAULT_REMOTE_PORT
    );
}

/// P-23 inside the configuring process. Capture + the sequencer install at app setup only when the
/// settings row ALREADY exists, so a first-ever enable would otherwise leave the stream absent for
/// the whole process: every WS subscribe 503s and nothing is captured until the app restarts.
#[test]
fn enabling_the_listener_also_installs_the_event_stream() {
    let source = include_str!("remote_host_commands.rs");
    let body = source
        .split("pub async fn start_remote_listener")
        .nth(1)
        .expect("the start command should exist");
    let end = body.find("\n}\n").expect("the command body should end");
    let body = &body[..end];
    let start = body.find("start_listener(").expect("listener start");
    let install = body
        .find("install_remote_stream_from_handle(")
        .expect("the enable path must install the event stream");
    assert!(
        start < install,
        "the settings row is minted by start_listener; the install is gated on it existing"
    );
}

#[tokio::test]
async fn listener_status_reports_not_running_for_a_fresh_handle() {
    let handle = crate::remote_server::RemoteListenerHandle::new();
    let settings = crate::remote_server::settings::RemoteHostSettings {
        enabled: false,
        exposure_mode: RemoteExposureMode::Serve,
        port: crate::remote_server::settings::DEFAULT_REMOTE_PORT,
        environment_id: "env-1".to_string(),
    };

    let status = super::listener_status(settings, &handle).await;

    assert!(!status.enabled);
    assert_eq!(status.exposure_mode, RemoteExposureMode::Serve);
    assert_eq!(status.port, crate::remote_server::settings::DEFAULT_REMOTE_PORT);
    assert_eq!(status.environment_id, "env-1");
    assert!(!status.running);
    assert!(status.bind_address.is_none());
    assert!(!status.serve_active);
    assert!(status.serve_degraded_reason.is_none());
}
