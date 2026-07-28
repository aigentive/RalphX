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

/// The branch PR 1.7's pane renders as "configured but unreachable".
#[test]
fn serve_mode_with_unreachable_magicdns_advertises_an_unavailable_endpoint() {
    assert_eq!(
        advertised_endpoints(
            RemoteExposureMode::Serve,
            3849,
            Some("mac-studio.tail1234.ts.net."),
            false,
            None,
        ),
        vec![AdvertisedEndpoint {
            kind: AdvertisedEndpointKind::LoopbackServe,
            url: "https://mac-studio.tail1234.ts.net".to_string(),
            available: false,
        }]
    );
}

#[test]
fn serve_mode_without_magicdns_degrades_to_no_endpoint() {
    assert!(advertised_endpoints(RemoteExposureMode::Serve, 3849, None, false, None).is_empty());
}

/// Direct exposure is plain HTTP over WireGuard (§4.4): the listener has no TLS acceptor, so an
/// `https://` direct URL would always fail its handshake.
#[test]
fn tailnet_direct_mode_advertises_a_plain_http_self_ip_and_listener_port() {
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
            url: "http://100.101.102.103:3849".to_string(),
            available: true,
        }]
    );
}

// ---------------------------------------------------------------------------------------
// Owner decision R1 — the client floor is pinned independently of the protocol version.
// ---------------------------------------------------------------------------------------

/// The value itself, pinned.
///
/// `MIN_CLIENT_PROTOCOL` used to read `= PROTOCOL_VERSION`, which turned every protocol bump
/// into an automatic hard cutover. The whole point of the fix is that the two numbers move
/// independently, so a test that derived the expectation from `PROTOCOL_VERSION` would pass
/// against the exact bug it exists to prevent. The literal is therefore spelled out.
#[test]
fn min_client_protocol_is_pinned_independently_of_the_protocol_version() {
    assert_eq!(
        super::endpoints::MIN_CLIENT_PROTOCOL,
        1,
        "MIN_CLIENT_PROTOCOL changed. Raising it is a DELIBERATE compatibility cut: every \
         client below the new floor is refused at the descriptor gate. It must never move as \
         a side effect of bumping PROTOCOL_VERSION, and a raise requires a spec note \
         recording which clients are being dropped and why."
    );
}

/// The published descriptor carries the pinned floor, not the running protocol version.
///
/// This is the wire-visible half: the descriptor is the one pre-auth response a client reads,
/// and `min_client_protocol` is the field its skew gate keys on.
#[test]
fn the_descriptor_publishes_the_pinned_floor() {
    let descriptor = super::endpoints::environment_descriptor("env-r1");
    assert_eq!(descriptor.min_client_protocol, 1);
    assert_eq!(
        descriptor.protocol_version,
        ralphx_remote_protocol::PROTOCOL_VERSION
    );
}
