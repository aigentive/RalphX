use crate::domain::agents::{
    AgentHarnessKind, EffectiveMcpServerPolicy, McpOverrideState, McpPolicyOverride,
    McpPolicySource, McpServerKey, NativeMcpServerSnapshot, NativeMcpState,
};

use super::mcp_policy_commands::{
    ensure_project_scope_exists, known_policy_tools, mutable_key, policy_server_ids,
    response_contains_sensitive_definition_fields, select_codex_catalog, to_server_response,
    to_server_response_with_scope_for_test, validate_legacy_repair_request,
};
use crate::application::AppState;

fn policy(provider: AgentHarnessKind, server_id: &str, tools: &[&str]) -> McpPolicyOverride {
    McpPolicyOverride {
        project_id: None,
        key: McpServerKey::new(provider, server_id).unwrap(),
        server_state: McpOverrideState::Follow,
        tool_states: tools
            .iter()
            .map(|tool| (tool.to_string(), McpOverrideState::Disabled))
            .collect(),
        updated_at: chrono::Utc::now(),
    }
}

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

#[test]
fn response_reports_configured_project_state_separately_from_effective_state() {
    let key = McpServerKey::new(AgentHarnessKind::Claude, "github").unwrap();
    let scoped = policy(AgentHarnessKind::Claude, "github", &["delete_issue"]);
    let response = to_server_response_with_scope_for_test(
        EffectiveMcpServerPolicy {
            native: NativeMcpServerSnapshot {
                key,
                native_scope: Some("project".to_string()),
                native_state: NativeMcpState::Enabled,
                known_tools: vec!["delete_issue".to_string(), "list_issues".to_string()],
                diagnostic: None,
            },
            enabled: false,
            server_state: McpOverrideState::Disabled,
            server_source: McpPolicySource::GlobalUi,
            tool_states: [
                ("delete_issue".to_string(), McpOverrideState::Disabled),
                ("list_issues".to_string(), McpOverrideState::Follow),
            ]
            .into_iter()
            .collect(),
            tool_sources: [
                ("delete_issue".to_string(), McpPolicySource::ProjectUi),
                ("list_issues".to_string(), McpPolicySource::ProviderNative),
            ]
            .into_iter()
            .collect(),
            disabled_tools: vec!["delete_issue".to_string()],
            locked: false,
            locked_reason: None,
        },
        Some(&scoped),
    );

    assert!(!response.effective_enabled);
    assert_eq!(response.configured_state, McpOverrideState::Follow);
    assert_eq!(response.effective_state, McpOverrideState::Disabled);
    assert_eq!(response.disabled_tools, vec!["delete_issue"]);
    let delete_tool = response
        .known_tools
        .iter()
        .find(|tool| tool.tool_name == "delete_issue")
        .expect("disabled tool response");
    assert_eq!(delete_tool.configured_state, McpOverrideState::Disabled);
    assert_eq!(delete_tool.effective_source, McpPolicySource::ProjectUi);
}

#[test]
fn policy_catalog_includes_required_and_policy_only_servers_with_sorted_tools() {
    let global = [policy(
        AgentHarnessKind::Claude,
        "github",
        &["create_issue"],
    )];
    let project = [policy(
        AgentHarnessKind::Claude,
        "linear",
        &["delete_issue", "archive_issue"],
    )];

    let server_ids = policy_server_ids(
        AgentHarnessKind::Claude,
        global.iter().chain(project.iter()),
    );
    assert!(server_ids.contains("ralphx"));
    assert!(server_ids.contains("ralphx_internal"));
    assert!(server_ids.contains("github"));
    assert!(server_ids.contains("linear"));
    assert_eq!(
        known_policy_tools(
            AgentHarnessKind::Claude,
            "linear",
            global.iter().chain(project.iter()),
        ),
        vec!["archive_issue".to_string(), "delete_issue".to_string()]
    );
}

#[test]
fn locked_internal_servers_are_rejected_before_repository_mutation() {
    let error = mutable_key(AgentHarnessKind::Codex, "ralphx_internal".to_string())
        .expect_err("RalphX-owned server ids cannot be user-mutated");
    assert!(error.contains("locked_internal_server"));

    let valid = mutable_key(AgentHarnessKind::Codex, "github".to_string()).unwrap();
    assert_eq!(valid.server_id, "github");
}

#[test]
fn legacy_repair_command_accepts_only_claude_user_scoped_ralphx() {
    assert!(validate_legacy_repair_request(AgentHarnessKind::Claude, "ralphx", "user").is_ok());
    for (provider, server_id, scope) in [
        (AgentHarnessKind::Codex, "ralphx", "user"),
        (AgentHarnessKind::Claude, "ralphx_internal", "user"),
        (AgentHarnessKind::Claude, "ralphx", "project"),
        (AgentHarnessKind::Claude, "github", "user"),
    ] {
        assert!(validate_legacy_repair_request(provider, server_id, scope).is_err());
    }
}

#[tokio::test]
async fn project_scoped_mutations_reject_unknown_projects_before_writes() {
    let state = AppState::new_test();

    let error = ensure_project_scope_exists(&state, Some("missing-project"))
        .await
        .expect_err("unknown project scope must fail closed");

    assert!(error.contains("project_not_found"));
}

#[test]
fn codex_catalog_keeps_fixed_path_fallback_when_structured_probe_fails() {
    let fallback = NativeMcpServerSnapshot {
        key: McpServerKey::new(AgentHarnessKind::Codex, "github").unwrap(),
        native_scope: Some("user".to_string()),
        native_state: NativeMcpState::Enabled,
        known_tools: vec!["search".to_string()],
        diagnostic: None,
    };

    let discovery = select_codex_catalog(
        vec![fallback],
        Err("Codex CLI resolution failed".to_string()),
    );

    assert_eq!(discovery.servers.len(), 1);
    assert_eq!(discovery.servers[0].key.server_id, "github");
    assert!(discovery
        .diagnostic
        .as_deref()
        .is_some_and(|message| message.contains("limited redacted metadata")));
}
