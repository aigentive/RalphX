use std::ffi::OsStr;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use super::tailscale::{
    parse_status, probe_magicdns_reachability, serve_acquire_args, serve_release_args,
    RealTailscaleCommandRunner, TailscaleCommandRunner, TailscaleSelfAddressProvider,
    TailscaleServeError,
};
use crate::infrastructure::tool_paths::TEST_ENV_MUTEX;
use crate::remote_server::settings::{
    is_tailnet_cgnat_ipv4, TailnetProviderError, TailnetSelfAddressProvider,
};

const RUNNING_STATUS: &str = r#"{
  "Version": "1.66.1",
  "BackendState": "Running",
  "Self": {
    "ID": "n1234567890CNTRL",
    "PublicKey": "nodekey:abc123",
    "HostName": "mac-studio",
    "DNSName": "mac-studio.tail1234.ts.net.",
    "OS": "macOS",
    "TailscaleIPs": [
      "100.101.102.103",
      "fd7a:115c:a1e0:0:0:0:0:1234"
    ],
    "Online": true
  },
  "MagicDNSSuffix": "tail1234.ts.net",
  "CurrentTailnet": {
    "Name": "example.ts.net",
    "MagicDNSSuffix": "tail1234.ts.net",
    "MagicDNSEnabled": true
  }
}"#;

const LOGGED_OUT_STATUS: &str = r#"{
  "Version": "1.66.1",
  "BackendState": "NeedsLogin",
  "MagicDNSSuffix": "",
  "CurrentTailnet": null
}"#;

/// The shape a real logged-out/stopped daemon actually emits: `Self` is present and its
/// `TailscaleIPs` is an explicit `null`, because Go marshals the nil slice with no `omitempty`.
/// `#[serde(default)]` does not cover an explicit null, so this is the fixture that catches a
/// regression back to `Vec<IpAddr>`.
const LOGGED_OUT_STATUS_WITH_NULL_IPS: &str = r#"{
  "Version": "1.66.1",
  "BackendState": "Stopped",
  "Self": {
    "ID": "n1234567890CNTRL",
    "HostName": "mac-studio",
    "DNSName": "",
    "OS": "macOS",
    "TailscaleIPs": null,
    "Online": false
  },
  "MagicDNSSuffix": "",
  "CurrentTailnet": null
}"#;

struct EnvGuard {
    key: &'static str,
    original: Option<std::ffi::OsString>,
}

impl EnvGuard {
    fn set_os(key: &'static str, value: impl AsRef<OsStr>) -> Self {
        let original = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, original }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.original {
            Some(value) => std::env::set_var(self.key, value),
            None => std::env::remove_var(self.key),
        }
    }
}

fn install_fake_tailscale(script: &str) -> (tempfile::TempDir, EnvGuard) {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let binary = temp_dir.path().join("tailscale");
    std::fs::write(&binary, script).expect("write fake tailscale");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&binary)
            .expect("fake tailscale metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&binary, permissions).expect("mark fake tailscale executable");
    }
    let path = EnvGuard::set_os("PATH", temp_dir.path());
    (temp_dir, path)
}

const STATUS_RUNNING_SCRIPT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/tailscale/status-running.sh"
));
const STATUS_DAEMON_DOWN_SCRIPT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/tailscale/status-daemon-down.sh"
));
const SERVE_OK_SCRIPT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/tailscale/serve-ok.sh"
));
const SERVE_FAIL_SCRIPT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/tailscale/serve-fail.sh"
));
const SERVE_FAIL_LONG_STDERR_SCRIPT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/tailscale/serve-fail-long-stderr.sh"
));
const LAUNCH_FAIL_SCRIPT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/tailscale/launch-fail.sh"
));

#[derive(Clone, Default)]
struct RecordingTailscaleCommandRunner {
    calls: Arc<Mutex<Vec<Vec<String>>>>,
}

#[async_trait]
impl TailscaleCommandRunner for RecordingTailscaleCommandRunner {
    async fn run_status(&self) -> Result<String, TailnetProviderError> {
        Ok(RUNNING_STATUS.to_string())
    }

    async fn run_serve_acquire(&self, port: u16) -> Result<(), TailscaleServeError> {
        self.calls
            .lock()
            .expect("command recorder mutex")
            .push(serve_acquire_args(port));
        Ok(())
    }

    async fn run_serve_release(&self) -> Result<(), TailscaleServeError> {
        self.calls
            .lock()
            .expect("command recorder mutex")
            .push(serve_release_args());
        Ok(())
    }
}

#[test]
fn running_status_parses_magicdns_and_filters_self_addresses() {
    let status = parse_status(RUNNING_STATUS).expect("running status parses");

    assert_eq!(status.magicdns_name(), Some("mac-studio.tail1234.ts.net"));
    assert_eq!(
        status.self_addresses(),
        vec![IpAddr::V4(Ipv4Addr::new(100, 101, 102, 103))]
    );
}

#[test]
fn logged_out_status_is_valid_and_has_no_self_addresses() {
    let status = parse_status(LOGGED_OUT_STATUS).expect("logged-out status parses");

    assert_eq!(status.magicdns_name(), None);
    assert!(status.self_addresses().is_empty());
}

/// §5.3: a logged-out host degrades to an empty endpoint list, never a provider error.
#[test]
fn logged_out_status_with_null_tailscale_ips_parses_to_no_self_addresses() {
    let status =
        parse_status(LOGGED_OUT_STATUS_WITH_NULL_IPS).expect("explicit null TailscaleIPs parses");

    assert_eq!(status.magicdns_name(), None);
    assert!(status.self_addresses().is_empty());
}

#[test]
fn malformed_or_unexpected_status_is_unavailable() {
    assert!(matches!(
        parse_status("not json at all"),
        Err(TailnetProviderError::Unavailable(_))
    ));
    assert!(matches!(
        parse_status(r#"{"Self":null}"#),
        Err(TailnetProviderError::Unavailable(_))
    ));
}

#[tokio::test]
async fn recorder_captures_exact_serve_acquire_and_release_argv() {
    let runner = RecordingTailscaleCommandRunner::default();
    runner
        .run_serve_acquire(3849)
        .await
        .expect("record acquire");
    runner.run_serve_release().await.expect("record release");

    assert_eq!(
        *runner.calls.lock().expect("command recorder mutex"),
        vec![
            vec![
                "serve".to_string(),
                "--bg".to_string(),
                "--https=443".to_string(),
                "http://127.0.0.1:3849".to_string(),
            ],
            vec![
                "serve".to_string(),
                "--https=443".to_string(),
                "off".to_string(),
            ],
        ]
    );
}

#[test]
fn cgnat_validation_covers_both_boundaries_and_nearby_non_tailnet_ranges() {
    for address in [
        Ipv4Addr::new(100, 64, 0, 0),
        Ipv4Addr::new(100, 127, 255, 255),
    ] {
        assert!(is_tailnet_cgnat_ipv4(address), "{address} should be CGNAT");
    }
    for address in [
        Ipv4Addr::new(100, 63, 255, 255),
        Ipv4Addr::new(100, 128, 0, 0),
        Ipv4Addr::new(192, 168, 1, 20),
        Ipv4Addr::new(127, 0, 0, 1),
    ] {
        assert!(
            !is_tailnet_cgnat_ipv4(address),
            "{address} should not be CGNAT"
        );
    }
}

#[tokio::test]
async fn real_runner_reads_status_stdout_from_the_cli() {
    let _lock = TEST_ENV_MUTEX.lock().expect("env mutex");
    let (_temp, _path) = install_fake_tailscale(STATUS_RUNNING_SCRIPT);

    let stdout = RealTailscaleCommandRunner
        .run_status()
        .await
        .expect("status succeeds");

    assert!(stdout.contains(r#""BackendState":"Running""#));
    assert_eq!(
        parse_status(&stdout)
            .expect("valid fixture")
            .magicdns_name(),
        Some("mac.tail.ts.net")
    );
}

#[tokio::test]
async fn real_runner_reports_status_exit_and_stderr() {
    let _lock = TEST_ENV_MUTEX.lock().expect("env mutex");
    let (_temp, _path) = install_fake_tailscale(STATUS_DAEMON_DOWN_SCRIPT);

    let error = RealTailscaleCommandRunner
        .run_status()
        .await
        .expect_err("non-zero status fails");

    assert!(matches!(
        error,
        TailnetProviderError::Unavailable(message)
            if message.contains("exit status: 1") && message.contains("failed to connect")
    ));
}

#[tokio::test]
async fn concrete_self_address_provider_reads_and_parses_the_cli() {
    let _lock = TEST_ENV_MUTEX.lock().expect("env mutex");
    let (_temp, _path) = install_fake_tailscale(STATUS_RUNNING_SCRIPT);

    let addresses = TailscaleSelfAddressProvider
        .self_addresses()
        .await
        .expect("provider should parse the real runner output");

    assert_eq!(
        addresses,
        vec![IpAddr::V4(Ipv4Addr::new(100, 101, 102, 103))]
    );
}

#[tokio::test]
async fn concrete_self_address_provider_propagates_daemon_failure() {
    let _lock = TEST_ENV_MUTEX.lock().expect("env mutex");
    let (_temp, _path) = install_fake_tailscale(STATUS_DAEMON_DOWN_SCRIPT);

    assert!(matches!(
        TailscaleSelfAddressProvider.self_addresses().await,
        Err(TailnetProviderError::Unavailable(message))
            if message.contains("failed to connect")
    ));
}

#[tokio::test]
async fn real_runner_executes_both_serve_success_paths() {
    let _lock = TEST_ENV_MUTEX.lock().expect("env mutex");
    let (_temp, _path) = install_fake_tailscale(SERVE_OK_SCRIPT);
    let runner = RealTailscaleCommandRunner;

    runner.run_serve_acquire(3849).await.expect("acquire");
    runner.run_serve_release().await.expect("release");
}

#[tokio::test]
async fn real_runner_preserves_serve_failure_stderr() {
    let _lock = TEST_ENV_MUTEX.lock().expect("env mutex");
    let (_temp, _path) = install_fake_tailscale(SERVE_FAIL_SCRIPT);
    let runner = RealTailscaleCommandRunner;

    for error in [
        runner
            .run_serve_acquire(3849)
            .await
            .expect_err("acquire fails"),
        runner.run_serve_release().await.expect_err("release fails"),
    ] {
        assert!(matches!(
            error,
            TailscaleServeError::Exit(message)
                if message.contains("exit status: 1")
                    && message.contains("Serve is not enabled")
        ));
    }
}

#[tokio::test]
async fn serve_failure_stderr_is_bounded_to_four_hundred_characters() {
    let _lock = TEST_ENV_MUTEX.lock().expect("env mutex");
    let (_temp, _path) = install_fake_tailscale(SERVE_FAIL_LONG_STDERR_SCRIPT);

    let error = RealTailscaleCommandRunner
        .run_serve_release()
        .await
        .expect_err("fixture exits unsuccessfully");

    let TailscaleServeError::Exit(message) = error else {
        panic!("expected an exit failure");
    };
    let (_, snippet) = message
        .rsplit_once(": ")
        .expect("non-empty stderr should be appended");
    assert_eq!(snippet.chars().count(), 400);
    assert!(!message.contains("EXCLUDED_SUFFIX"));
}

#[tokio::test]
async fn bad_interpreter_maps_launch_failures_for_status_and_serve() {
    let _lock = TEST_ENV_MUTEX.lock().expect("env mutex");
    let (_temp, _path) = install_fake_tailscale(LAUNCH_FAIL_SCRIPT);

    assert!(matches!(
        RealTailscaleCommandRunner.run_status().await,
        Err(TailnetProviderError::Unavailable(message))
            if message.contains("tailscale status could not be launched")
    ));
    assert!(matches!(
        RealTailscaleCommandRunner.run_serve_release().await,
        Err(TailscaleServeError::Launch(message)) if !message.is_empty()
    ));
}

#[tokio::test]
async fn whitespace_magicdns_name_is_rejected_without_network_io() {
    assert!(!probe_magicdns_reachability("   ").await);
}
