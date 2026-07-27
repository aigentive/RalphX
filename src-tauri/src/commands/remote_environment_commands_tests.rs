//! Command-surface tests: P-18 (no token reaches JS), serde casing (rule 14),
//! and registry guards (no credential-fetch command exists).

use super::*;
use crate::domain::entities::remote_environment::{
    RemoteEnvironment, RemoteEnvironmentId, RemoteEnvironmentStatus,
};

fn sample_environment() -> RemoteEnvironment {
    RemoteEnvironment {
        id: RemoteEnvironmentId::from_string("row-1"),
        environment_id: "env-1".to_string(),
        name: "Mac Studio".to_string(),
        base_url: "https://mac-studio.tailnet.ts.net".to_string(),
        candidate_urls: vec!["http://100.101.102.103:3849".to_string()],
        token_secret_ref: "remote-env:row-1:token".to_string(),
        scopes: vec![
            ralphx_remote_protocol::Scope::UiRead,
            ralphx_remote_protocol::Scope::UiOperate,
        ],
        protocol_version: 1,
        status: RemoteEnvironmentStatus::Active,
        created_at: "2026-07-27T19:15:00+00:00".to_string(),
        last_connected_at: None,
    }
}

// ============================================================================
// P-18 — the JS-facing projection never carries secret material
// ============================================================================

#[test]
fn summary_is_an_explicit_allowlist_without_token_material() {
    let summary = RemoteEnvironmentSummary::from(sample_environment());
    let json = serde_json::to_value(&summary).expect("summary should serialize");

    let object = json.as_object().expect("summary should be an object");
    let keys: Vec<&str> = object.keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        vec![
            "id",
            "environmentId",
            "name",
            "baseUrl",
            "candidateUrls",
            "scopes",
            "protocolVersion",
            "status",
            "createdAt",
            "lastConnectedAt",
        ],
        "summary field set is an explicit allowlist — extending it requires a P-18 review"
    );
    // Neither the token nor the Keychain reference crosses the IPC boundary.
    let serialized = json.to_string();
    assert!(!serialized.contains("rxd_live_"));
    assert!(!serialized.contains("token"));
}

#[test]
fn summary_serializes_camel_case_with_snake_case_status_values() {
    let summary = RemoteEnvironmentSummary::from(sample_environment());
    let json = serde_json::to_value(&summary).expect("summary should serialize");

    assert_eq!(json["environmentId"], "env-1");
    assert_eq!(json["baseUrl"], "https://mac-studio.tailnet.ts.net");
    assert_eq!(json["status"], "active");
    assert_eq!(json["scopes"], serde_json::json!(["ui:read", "ui:operate"]));
}

// ============================================================================
// Rule 14 — invoke inputs deserialize from camelCase
// ============================================================================

#[test]
fn invoke_input_accepts_camel_case_fields() {
    let input: RemoteInvokeInput = serde_json::from_value(serde_json::json!({
        "id": "row-1",
        "requestId": "req-1",
        "cmd": "health_check",
        "args": {"limit": 1},
    }))
    .expect("camelCase input should deserialize");
    assert_eq!(input.request_id, "req-1");
    assert_eq!(input.args["limit"], 1);
}

#[test]
fn invoke_input_defaults_missing_args_to_null() {
    let input: RemoteInvokeInput = serde_json::from_value(serde_json::json!({
        "id": "row-1",
        "requestId": "req-1",
        "cmd": "health_check",
    }))
    .expect("args should be optional");
    assert!(input.args.is_null());
}

#[test]
fn pair_input_accepts_camel_case_fields() {
    let input: PairRemoteEnvironmentInput = serde_json::from_value(serde_json::json!({
        "url": "https://mac-studio.tailnet.ts.net",
        "code": "rxp_code",
        "name": "Mac Studio",
    }))
    .expect("pair input should deserialize");
    assert_eq!(input.name, "Mac Studio");
}

// ============================================================================
// P-18 — registry guards over the command surface
// ============================================================================

const REGISTRY_SOURCE: &str = include_str!("registry.rs");

#[test]
fn there_is_no_credential_fetch_command() {
    assert!(
        !REGISTRY_SOURCE.contains("get_credential"),
        "a get_credential-to-JS command must never exist (P-18)"
    );
    assert!(
        !REGISTRY_SOURCE.contains("get_remote_token"),
        "no command may hand the device token to JS (P-18)"
    );
}

#[test]
fn the_remote_environment_command_surface_is_registered() {
    for command in [
        "remote_environment_commands::pair_remote_environment",
        "remote_environment_commands::list_remote_environments",
        "remote_environment_commands::remove_remote_environment",
        "remote_environment_commands::get_active_environment",
        "remote_environment_commands::set_active_environment",
        "remote_environment_commands::remote_connect",
        "remote_environment_commands::remote_disconnect",
        "remote_environment_commands::remote_invoke",
        "remote_environment_commands::remote_fetch",
    ] {
        assert!(
            REGISTRY_SOURCE.contains(command),
            "{command} must be registered in the Tauri invoke handler"
        );
    }
}
