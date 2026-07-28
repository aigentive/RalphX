//! Command-surface tests: P-18 (no token reaches JS), serde casing (rule 14),
//! and registry guards (no credential-fetch command exists).

use super::*;
use crate::domain::entities::remote_environment::{
    RemoteEnvironment, RemoteEnvironmentId, RemoteEnvironmentStatus,
};
use crate::domain::repositories::{RemoteEnvironmentRepository, UpsertPairedEnvironment};
use crate::infrastructure::memory::{MemoryRemoteEnvironmentRepository, MemorySecretStore};
use std::sync::Arc;
use tauri::Manager;

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

// ============ Command wrapper coverage ============

fn test_app() -> tauri::App<tauri::test::MockRuntime> {
    tauri::test::mock_builder()
        .manage(AppState::new_test())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("build remote environment test app")
}

fn test_app_with_service(
    service: RemoteEnvironmentService,
) -> tauri::App<tauri::test::MockRuntime> {
    let mut state = AppState::new_test();
    state.remote_environment_service = Arc::new(service);
    tauri::test::mock_builder()
        .manage(state)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("build seeded remote environment test app")
}

#[tokio::test]
async fn get_active_environment_returns_the_local_environment_id() {
    let app = test_app();
    let id = get_active_environment(app.state::<AppState>())
        .await
        .expect("active environment should resolve");
    assert!(!id.is_empty());
}

#[tokio::test]
async fn list_remote_environments_returns_an_empty_fresh_registry() {
    let app = test_app();
    let environments = list_remote_environments(app.state::<AppState>())
        .await
        .expect("fresh registry should list");
    assert!(environments.is_empty());
}

#[tokio::test]
async fn pair_remote_environment_maps_host_client_errors() {
    let app = test_app();
    let error = pair_remote_environment(
        PairRemoteEnvironmentInput {
            url: "https://example.test".to_string(),
            code: "rxp_code".to_string(),
            name: "Example".to_string(),
        },
        app.state::<AppState>(),
    )
    .await
    .expect_err("unavailable host client should reject pairing");
    assert!(!error.is_empty());
}

#[tokio::test]
async fn remove_remote_environment_is_idempotent_for_an_unknown_id() {
    // `RemoteEnvironmentService::remove` documents removing an unknown environment as a
    // no-op success (idempotent), not a rejection — this proves the command wrapper
    // passes that Ok(()) straight through rather than turning it into an error.
    let app = test_app();
    remove_remote_environment(
        RemoteEnvironmentIdInput {
            id: "missing".to_string(),
        },
        app.state::<AppState>(),
    )
    .await
    .expect("removing an unknown environment should be an idempotent no-op success");
}

#[tokio::test]
async fn set_active_environment_rejects_an_unknown_id() {
    let app = test_app();
    let error = set_active_environment(
        RemoteEnvironmentIdInput {
            id: "missing".to_string(),
        },
        app.state::<AppState>(),
    )
    .await
    .expect_err("unknown environment should be rejected");
    assert!(!error.is_empty());
}

#[tokio::test]
async fn remote_connect_rejects_an_unknown_id() {
    let app = test_app();
    let error = remote_connect(
        RemoteEnvironmentIdInput {
            id: "missing".to_string(),
        },
        app.state::<AppState>(),
    )
    .await
    .expect_err("unknown environment should be rejected");
    assert!(!error.is_empty());
}

#[tokio::test]
async fn remote_disconnect_rejects_an_unknown_id() {
    let app = test_app();
    let error = remote_disconnect(
        RemoteEnvironmentIdInput {
            id: "missing".to_string(),
        },
        app.state::<AppState>(),
    )
    .await
    .expect_err("unknown environment should be rejected");
    assert!(!error.is_empty());
}

#[tokio::test]
async fn remote_invoke_rejects_an_unknown_id() {
    let app = test_app();
    let error = remote_invoke(
        RemoteInvokeInput {
            id: "missing".to_string(),
            request_id: "req-1".to_string(),
            cmd: "health_check".to_string(),
            args: serde_json::json!({}),
        },
        app.state::<AppState>(),
    )
    .await
    .expect_err("unknown environment should be rejected");
    assert!(!error.is_empty());
}

#[tokio::test]
async fn remote_fetch_rejects_an_unknown_id() {
    let app = test_app();
    let error = remote_fetch(
        RemoteFetchInput {
            id: "missing".to_string(),
            path: "/remote/v1/health".to_string(),
        },
        app.state::<AppState>(),
    )
    .await
    .expect_err("unknown environment should be rejected");
    assert!(!error.is_empty());
}

#[tokio::test]
async fn list_remote_environments_maps_a_seeded_environment() {
    let repo = Arc::new(MemoryRemoteEnvironmentRepository::new());
    let seeded = repo
        .upsert_paired(UpsertPairedEnvironment {
            environment_id: "env-seeded".to_string(),
            name: "Seeded Mac".to_string(),
            url: "https://seeded.example".to_string(),
            scopes: vec![ralphx_remote_protocol::Scope::UiRead],
            protocol_version: 1,
        })
        .await
        .expect("seed environment");
    let service = RemoteEnvironmentService::new(
        Arc::clone(&repo) as Arc<dyn RemoteEnvironmentRepository>,
        Arc::new(MemorySecretStore::new()),
        Arc::new(crate::infrastructure::UnavailableRemoteHostClient::new(
            "not needed by list",
        )),
    );
    let app = test_app_with_service(service);

    let environments = list_remote_environments(app.state::<AppState>())
        .await
        .expect("seeded registry should list");

    assert_eq!(environments.len(), 1);
    let summary = &environments[0];
    assert_eq!(summary.id, seeded.id.as_str());
    assert_eq!(summary.environment_id, "env-seeded");
    assert_eq!(summary.name, "Seeded Mac");
    assert_eq!(summary.base_url, "https://seeded.example");
    assert_eq!(summary.status, RemoteEnvironmentStatus::PendingAdd);
    assert_eq!(summary.protocol_version, 1);
}
