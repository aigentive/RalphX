use crate::commands::mcp_policy_commands::McpCatalogResponse;
use crate::commands::remote_mcp_policy_commands::{
    get_remote_mcp_catalog_for_app_state, RemoteMcpCatalogInput,
};
use crate::domain::agents::{AgentHarnessKind, McpOverrideState, McpServerKey};
use crate::domain::repositories::McpCatalogSnapshot;

fn input() -> RemoteMcpCatalogInput {
    RemoteMcpCatalogInput {
        project_id: None,
        provider: "codex".to_string(),
    }
}

fn serialized_catalog() -> String {
    r#"{"eligible_providers":["codex"],"eligible_default_provider":"codex","probed_at":"2026-08-05T18:00:00+00:00","probe_stale":false,"provider_diagnostics":{"codex":"captured diagnostic"},"policy_diagnostics":["captured policy"],"servers":[]}"#.to_string()
}

#[tokio::test]
async fn remote_catalog_is_absent_before_a_host_build() {
    let state = crate::application::AppState::new_test();
    let response = get_remote_mcp_catalog_for_app_state(&state, input())
        .await
        .unwrap();

    assert!(response.snapshot.is_none());
    assert!(response.captured_at.is_none());
}

#[tokio::test]
async fn remote_catalog_marks_the_whole_captured_snapshot_stale_without_refreshing_overrides() {
    let state = crate::application::AppState::new_test();
    let response_json = serialized_catalog();
    state
        .mcp_catalog_snapshot_repo
        .upsert(McpCatalogSnapshot {
            scope_project_id: None,
            provider: "codex".to_string(),
            response_json: response_json.clone(),
            captured_at: "2026-08-05T18:01:00+00:00".to_string(),
        })
        .await
        .unwrap();
    state
        .mcp_policy_repo
        .set_server_state(
            None,
            &McpServerKey::new(AgentHarnessKind::Codex, "changed-after-capture".to_string())
                .unwrap(),
            McpOverrideState::Disabled,
        )
        .await
        .unwrap();

    let response = get_remote_mcp_catalog_for_app_state(&state, input())
        .await
        .unwrap();
    let mut expected = serde_json::from_str::<McpCatalogResponse>(&response_json).unwrap();
    expected.probe_stale = true;

    assert_eq!(response.snapshot, Some(expected));
    assert_eq!(
        response.captured_at.as_deref(),
        Some("2026-08-05T18:01:00+00:00")
    );
}
