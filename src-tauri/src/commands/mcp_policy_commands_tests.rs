use crate::domain::agents::{
    AgentHarnessKind, EffectiveMcpServerPolicy, McpOverrideState, McpPolicySource, McpServerKey,
    NativeMcpServerSnapshot, NativeMcpState,
};

use super::mcp_policy_commands::{
    response_contains_sensitive_definition_fields, to_server_response,
};

#[test]
fn catalog_response_is_redacted_by_construction() {
    let response = to_server_response(EffectiveMcpServerPolicy {
        native: NativeMcpServerSnapshot {
            key: McpServerKey::new(AgentHarnessKind::Claude, "github").unwrap(),
            native_scope: Some("user".to_string()),
            native_state: NativeMcpState::Unknown,
            known_tools: vec!["create_issue".to_string()],
            diagnostic: Some("Catalog unavailable".to_string()),
        },
        enabled: true,
        server_state: McpOverrideState::Follow,
        server_source: McpPolicySource::ProviderNative,
        tool_states: [("create_issue".to_string(), McpOverrideState::Follow)]
            .into_iter()
            .collect(),
        tool_sources: [("create_issue".to_string(), McpPolicySource::ProviderNative)]
            .into_iter()
            .collect(),
        disabled_tools: Vec::new(),
        locked: false,
        locked_reason: None,
    });
    let json = serde_json::to_value(response).unwrap();
    assert!(!response_contains_sensitive_definition_fields(&json));
}
