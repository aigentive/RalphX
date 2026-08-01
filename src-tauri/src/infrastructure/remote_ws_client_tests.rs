// remote_ws_client tests: URL construction (scheme mapping + ticket shape guard) and
// pure frame decode/encode against the documented wire shapes. No real sockets.

use super::*;

// ============================================================================
// ws_events_url — scheme mapping
// ============================================================================

#[test]
fn ws_events_url_maps_http_to_ws_and_appends_the_ticket() {
    assert_eq!(
        ws_events_url("http://100.101.102.103:3849", "abc_DEF-123").expect("valid inputs"),
        "ws://100.101.102.103:3849/remote/v1/events?ticket=abc_DEF-123"
    );
}

#[test]
fn ws_events_url_maps_https_to_wss_and_tolerates_a_trailing_slash() {
    assert_eq!(
        ws_events_url("https://mac-studio.tailnet.ts.net/", "t0").expect("valid inputs"),
        "wss://mac-studio.tailnet.ts.net/remote/v1/events?ticket=t0"
    );
}

#[test]
fn ws_events_url_rejects_non_http_schemes() {
    for base in ["file:///etc/passwd", "ftp://host", "mac-studio.local", ""] {
        let error = ws_events_url(base, "ticket").expect_err("non-http base must be rejected");
        assert!(
            matches!(error, RemoteWsError::Unreachable(_)),
            "base {base:?} produced {error:?}"
        );
    }
    assert!(
        matches!(
            ws_events_url("https://", "ticket"),
            Err(RemoteWsError::Unreachable(_))
        ),
        "a scheme without a host is not dialable"
    );
}

/// The ticket lands in a URL query string, so anything outside the URL-safe base64
/// alphabet is a request-forgery seam and must never be interpolated.
#[test]
fn ws_events_url_rejects_malformed_tickets() {
    for ticket in [
        "", "a b", "a?b", "a&b=1", "a#frag", "a/b", "a%2Fb", "über", "a\nb",
    ] {
        let error = ws_events_url("https://host.example", ticket)
            .expect_err("malformed ticket must be rejected");
        assert!(
            matches!(error, RemoteWsError::Unreachable(_)),
            "ticket {ticket:?} produced {error:?}"
        );
    }
}

// ============================================================================
// Frame decode/encode — the documented wire shapes (C-11: real serialization)
// ============================================================================

#[test]
fn server_frames_decode_from_the_camel_case_wire_shape() {
    let hello = decode_server_frame(
        r#"{"type":"hello","protocolVersion":1,"environmentId":"env-1","streamEpoch":"epoch-1","serverVersion":"0.81.0","maxSeq":42,"heartbeatSecs":20}"#,
    )
    .expect("hello should decode");
    assert_eq!(
        hello,
        ServerFrame::Hello {
            protocol_version: 1,
            environment_id: "env-1".to_string(),
            stream_epoch: "epoch-1".to_string(),
            server_version: "0.81.0".to_string(),
            max_seq: 42,
            heartbeat_secs: 20,
        }
    );

    let heartbeat =
        decode_server_frame(r#"{"type":"heartbeat","t":1753700000}"#).expect("heartbeat decodes");
    assert_eq!(heartbeat, ServerFrame::Heartbeat { t: 1_753_700_000 });
}

#[test]
fn an_undecodable_text_frame_is_a_protocol_error_not_a_skip() {
    let error = decode_server_frame(r#"{"type":"no_such_frame"}"#)
        .expect_err("unknown frame type must error");
    assert!(matches!(error, RemoteWsError::Protocol(_)));

    let error = decode_server_frame("not json at all").expect_err("garbage must error");
    assert!(matches!(error, RemoteWsError::Protocol(_)));
}

#[test]
fn client_frames_serialize_to_the_camel_case_wire_shape() {
    assert_eq!(
        serde_json::to_value(ClientFrame::Subscribe {
            after_seq: 10,
            stream_epoch: "epoch-1".to_string(),
        })
        .expect("subscribe serializes"),
        serde_json::json!({"type": "subscribe", "afterSeq": 10, "streamEpoch": "epoch-1"})
    );
    assert_eq!(
        serde_json::to_value(ClientFrame::HeartbeatAck { t: 7 }).expect("ack serializes"),
        serde_json::json!({"type": "heartbeatAck", "t": 7})
    );
}
