use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use super::tailscale::{
    parse_status, serve_acquire_args, serve_release_args, TailscaleCommandRunner,
    TailscaleServeError,
};
use crate::remote_server::settings::{is_tailnet_cgnat_ipv4, TailnetProviderError};

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
