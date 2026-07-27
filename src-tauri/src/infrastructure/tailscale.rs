//! Tailscale CLI integration for remote-host discovery and Serve exposure.

use std::net::IpAddr;
use std::process::Stdio;
use std::time::Duration;

use async_trait::async_trait;
use http_body_util::Full;
use hyper::{Method, Request};
use hyper_rustls::HttpsConnector;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio_util::bytes::Bytes;

use crate::infrastructure::tool_paths::find_tailscale_cli_path;
use crate::remote_server::settings::{
    is_tailnet_cgnat_ipv4, TailnetProviderError, TailnetSelfAddressProvider,
};
use crate::remote_server::DESCRIPTOR_PATH;

const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);
const HTTPS_PORT: &str = "--https=443";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum TailscaleServeError {
    #[error("tailscale CLI is unavailable")]
    CliUnavailable,
    #[error("tailscale Serve command could not be launched: {0}")]
    Launch(String),
    #[error("tailscale Serve command timed out")]
    Timeout,
    #[error("tailscale Serve output could not be read: {0}")]
    Output(String),
    #[error("tailscale Serve command failed with status {0}")]
    Exit(String),
}

#[async_trait]
pub(crate) trait TailscaleCommandRunner: Send + Sync {
    async fn run_status(&self) -> Result<String, TailnetProviderError>;
    async fn run_serve_acquire(&self, port: u16) -> Result<(), TailscaleServeError>;
    async fn run_serve_release(&self) -> Result<(), TailscaleServeError>;
}

pub(crate) struct RealTailscaleCommandRunner;

#[async_trait]
impl TailscaleCommandRunner for RealTailscaleCommandRunner {
    async fn run_status(&self) -> Result<String, TailnetProviderError> {
        let path = find_tailscale_cli_path().ok_or_else(|| {
            TailnetProviderError::Unavailable("tailscale CLI disappeared after resolution".into())
        })?;
        let output = run_command(path, status_args())
            .await
            .map_err(TailscaleProcessError::into_provider_error)?;
        Ok(output.stdout)
    }

    async fn run_serve_acquire(&self, port: u16) -> Result<(), TailscaleServeError> {
        run_serve_command(serve_acquire_args(port)).await
    }

    async fn run_serve_release(&self) -> Result<(), TailscaleServeError> {
        run_serve_command(serve_release_args()).await
    }
}

pub(crate) struct TailscaleSelfAddressProvider;

#[async_trait]
impl TailnetSelfAddressProvider for TailscaleSelfAddressProvider {
    async fn self_addresses(&self) -> Result<Vec<IpAddr>, TailnetProviderError> {
        if find_tailscale_cli_path().is_none() {
            return Ok(Vec::new());
        }
        let stdout = RealTailscaleCommandRunner.run_status().await?;
        Ok(parse_status(&stdout)?.self_addresses())
    }
}

/// Acquires the process-independent Tailscale Serve mapping for a loopback listener.
#[allow(dead_code)]
pub(crate) async fn acquire_serve(port: u16) -> Result<(), TailscaleServeError> {
    RealTailscaleCommandRunner.run_serve_acquire(port).await
}

/// Releases the Tailscale Serve mapping.
#[allow(dead_code)]
pub(crate) async fn release_serve() -> Result<(), TailscaleServeError> {
    RealTailscaleCommandRunner.run_serve_release().await
}

#[derive(Debug, Deserialize)]
pub(crate) struct TailscaleStatus {
    #[serde(rename = "Version")]
    _version: String,
    #[serde(rename = "BackendState")]
    _backend_state: String,
    #[serde(rename = "Self", default)]
    self_status: Option<TailscaleSelfStatus>,
}

#[derive(Debug, Deserialize)]
struct TailscaleSelfStatus {
    #[serde(rename = "DNSName", default)]
    dns_name: String,
    #[serde(rename = "TailscaleIPs", default)]
    tailscale_ips: Vec<IpAddr>,
}

impl TailscaleStatus {
    pub(crate) fn self_addresses(&self) -> Vec<IpAddr> {
        self.self_status
            .as_ref()
            .into_iter()
            .flat_map(|status| status.tailscale_ips.iter().copied())
            .filter(|address| matches!(address, IpAddr::V4(ip) if is_tailnet_cgnat_ipv4(*ip)))
            .collect()
    }

    pub(crate) fn magicdns_name(&self) -> Option<&str> {
        self.self_status
            .as_ref()
            .map(|status| status.dns_name.trim_end_matches('.'))
            .filter(|name| !name.is_empty())
    }
}

pub(crate) fn parse_status(stdout: &str) -> Result<TailscaleStatus, TailnetProviderError> {
    serde_json::from_str(stdout).map_err(|error| {
        TailnetProviderError::Unavailable(format!("invalid tailscale status JSON: {error}"))
    })
}

/// Probes the descriptor route through Tailscale's MagicDNS hostname.
pub(crate) async fn probe_magicdns_reachability(magicdns_name: &str) -> bool {
    let hostname = magicdns_name.trim_end_matches('.');
    if hostname.is_empty() {
        return false;
    }
    let Ok(uri) = format!("https://{hostname}{DESCRIPTOR_PATH}").parse::<hyper::Uri>() else {
        return false;
    };
    install_rustls_crypto_provider();
    let Ok(https) = hyper_rustls::HttpsConnectorBuilder::new()
        .with_native_roots()
        .map(|builder| builder.https_only().enable_http1().build())
    else {
        return false;
    };
    let client: Client<HttpsConnector<HttpConnector>, Full<Bytes>> =
        Client::builder(TokioExecutor::new()).build(https);
    let Ok(request) = Request::builder()
        .method(Method::GET)
        .uri(uri)
        .body(Full::new(Bytes::new()))
    else {
        return false;
    };

    matches!(
        tokio::time::timeout(COMMAND_TIMEOUT, client.request(request)).await,
        Ok(Ok(response)) if response.status().is_success()
    )
}

fn install_rustls_crypto_provider() {
    static INSTALL_PROVIDER: std::sync::Once = std::sync::Once::new();
    INSTALL_PROVIDER.call_once(|| {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    });
}

fn status_args() -> Vec<String> {
    vec!["status".into(), "--json".into()]
}

pub(crate) fn serve_acquire_args(port: u16) -> Vec<String> {
    vec![
        "serve".into(),
        "--bg".into(),
        HTTPS_PORT.into(),
        format!("http://127.0.0.1:{port}"),
    ]
}

pub(crate) fn serve_release_args() -> Vec<String> {
    vec!["serve".into(), HTTPS_PORT.into(), "off".into()]
}

async fn run_serve_command(args: Vec<String>) -> Result<(), TailscaleServeError> {
    let path = find_tailscale_cli_path().ok_or(TailscaleServeError::CliUnavailable)?;
    let output = run_command(path, args).await.map_err(|error| match error {
        TailscaleProcessError::Launch(message) => TailscaleServeError::Launch(message),
        TailscaleProcessError::Timeout => TailscaleServeError::Timeout,
        TailscaleProcessError::Output(message) => TailscaleServeError::Output(message),
    })?;
    if output.success {
        Ok(())
    } else {
        Err(TailscaleServeError::Exit(output.status))
    }
}

struct CommandOutput {
    stdout: String,
    success: bool,
    status: String,
}

enum TailscaleProcessError {
    Launch(String),
    Timeout,
    Output(String),
}

impl TailscaleProcessError {
    fn into_provider_error(self) -> TailnetProviderError {
        let message = match self {
            Self::Launch(message) => format!("tailscale status could not be launched: {message}"),
            Self::Timeout => "tailscale status timed out".to_string(),
            Self::Output(message) => {
                format!("tailscale status output could not be read: {message}")
            }
        };
        TailnetProviderError::Unavailable(message)
    }
}

async fn run_command(
    path: std::path::PathBuf,
    args: Vec<String>,
) -> Result<CommandOutput, TailscaleProcessError> {
    let mut child = tokio::process::Command::new(path)
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| TailscaleProcessError::Launch(error.to_string()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| TailscaleProcessError::Output("stdout pipe was not captured".to_string()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| TailscaleProcessError::Output("stderr pipe was not captured".to_string()))?;

    let collect = async {
        let (stdout, stderr, status) =
            tokio::join!(read_stream(stdout), read_stream(stderr), child.wait());
        let stdout = stdout.map_err(TailscaleProcessError::Output)?;
        stderr.map_err(TailscaleProcessError::Output)?;
        let status = status.map_err(|error| TailscaleProcessError::Output(error.to_string()))?;
        Ok(CommandOutput {
            stdout,
            success: status.success(),
            status: status.to_string(),
        })
    };

    tokio::time::timeout(COMMAND_TIMEOUT, collect)
        .await
        .map_err(|_| TailscaleProcessError::Timeout)?
}

async fn read_stream(stream: impl tokio::io::AsyncRead + Unpin) -> Result<String, String> {
    let mut reader = BufReader::new(stream).lines();
    let mut lines = Vec::new();
    while let Some(line) = reader
        .next_line()
        .await
        .map_err(|error| error.to_string())?
    {
        lines.push(line);
    }
    Ok(lines.join("\n"))
}
