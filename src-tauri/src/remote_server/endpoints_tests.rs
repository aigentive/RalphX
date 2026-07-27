use std::net::Ipv4Addr;

use super::endpoints::{advertised_endpoints, AdvertisedEndpoint, AdvertisedEndpointKind};
use super::settings::RemoteExposureMode;

#[test]
fn serve_mode_advertises_resolved_magicdns_reachability() {
    assert_eq!(
        advertised_endpoints(
            RemoteExposureMode::Serve,
            3849,
            Some("mac-studio.tail1234.ts.net."),
            true,
            None,
        ),
        vec![AdvertisedEndpoint {
            kind: AdvertisedEndpointKind::LoopbackServe,
            url: "https://mac-studio.tail1234.ts.net".to_string(),
            available: true,
        }]
    );
}

#[test]
fn serve_mode_without_magicdns_degrades_to_no_endpoint() {
    assert!(advertised_endpoints(RemoteExposureMode::Serve, 3849, None, false, None).is_empty());
}

#[test]
fn tailnet_direct_mode_advertises_the_self_ip_and_listener_port() {
    assert_eq!(
        advertised_endpoints(
            RemoteExposureMode::TailnetDirect,
            3849,
            None,
            false,
            Some(Ipv4Addr::new(100, 101, 102, 103)),
        ),
        vec![AdvertisedEndpoint {
            kind: AdvertisedEndpointKind::TailnetDirect,
            url: "https://100.101.102.103:3849".to_string(),
            available: true,
        }]
    );
}
