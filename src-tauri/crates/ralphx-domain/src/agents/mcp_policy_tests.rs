use std::collections::BTreeMap;

use chrono::Utc;

use super::{
    AgentHarnessKind, McpLaunchPolicy, McpOverrideState, McpPolicyOverride, McpRepairStatus,
    McpServerKey, McpSetupConflictKind, McpSetupPreflightFailure, MCP_SETUP_PREFLIGHT_MARKER,
};

#[test]
fn identifiers_reject_path_and_shell_shaped_values() {
    for value in ["", "..", "../server", "a/b", "a b", "a$HOME"] {
        assert!(McpServerKey::new(AgentHarnessKind::Claude, value).is_err());
    }
    assert!(McpServerKey::new(AgentHarnessKind::Codex, "github.com-tools_1").is_ok());
}

#[test]
fn launch_policy_renders_provider_specific_deny_controls() {
    let policy = McpLaunchPolicy {
        disabled_servers: vec!["github.enterprise".to_string()],
        disabled_tools: [(
            "linear.internal".to_string(),
            vec!["delete_issue".to_string()],
        )]
        .into_iter()
        .collect(),
    };

    assert_eq!(
        policy.claude_disallowed_tools(),
        vec![
            "mcp__github.enterprise__*",
            "mcp__linear.internal__delete_issue",
        ]
    );
    assert_eq!(
        policy.codex_config_overrides(),
        vec![
            "mcp_servers.\"github.enterprise\".enabled=false",
            "mcp_servers.\"linear.internal\".disabled_tools=[\"delete_issue\"]",
        ]
    );
}

#[test]
fn required_servers_reject_server_and_tool_disables() {
    let server_disabled = McpPolicyOverride {
        project_id: None,
        key: McpServerKey::new(AgentHarnessKind::Claude, "ralphx").unwrap(),
        server_state: McpOverrideState::Disabled,
        tool_states: BTreeMap::new(),
        updated_at: Utc::now(),
    };
    assert!(server_disabled.validate().is_err());

    let mut tool_states = BTreeMap::new();
    tool_states.insert("list_agent_tasks".to_string(), McpOverrideState::Disabled);
    let tool_disabled = McpPolicyOverride {
        server_state: McpOverrideState::Follow,
        tool_states,
        ..server_disabled
    };
    assert!(tool_disabled.validate().is_err());
}

#[test]
fn setup_preflight_failures_have_stable_safe_markers() {
    let ambiguous = McpSetupPreflightFailure::ambiguous(
        AgentHarnessKind::Codex,
        "ralphx_internal",
        Some("project".to_string()),
    );
    assert_eq!(
        ambiguous.conflict_kind,
        McpSetupConflictKind::AmbiguousReservedId
    );
    assert_eq!(ambiguous.repair_status, McpRepairStatus::ManualOnly);
    assert_eq!(
        ambiguous.to_start_error_marker(),
        format!(
            "{MCP_SETUP_PREFLIGHT_MARKER}{{\"conflict_kind\":\"ambiguous_reserved_id\",\"provider\":\"codex\",\"repair_status\":\"manual_only\",\"scope\":\"project\",\"server_id\":\"ralphx_internal\"}}"
        )
    );
    assert!(ambiguous.to_string().contains("ralphx_internal"));

    let failed = McpSetupPreflightFailure::legacy_repair_failed();
    assert_eq!(
        failed.conflict_kind,
        McpSetupConflictKind::LegacyRepairFailed
    );
    assert_eq!(failed.repair_status, McpRepairStatus::Failed);
    assert!(failed
        .to_start_error_marker()
        .contains("legacy_repair_failed"));
    assert!(failed.to_string().contains("failed"));
}

#[test]
fn setup_conflict_and_repair_statuses_serialize_to_protocol_values() {
    assert_eq!(
        McpSetupConflictKind::LegacyRegistration.to_string(),
        "legacy_registration"
    );
    assert_eq!(McpRepairStatus::Repairable.to_string(), "repairable");
    assert_eq!(McpRepairStatus::Repaired.to_string(), "repaired");
    assert_eq!(McpRepairStatus::Failed.to_string(), "failed");
    assert_eq!(McpRepairStatus::ManualOnly.to_string(), "manual_only");
}
