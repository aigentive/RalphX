use std::collections::BTreeMap;

use chrono::Utc;

use super::{AgentHarnessKind, McpLaunchPolicy, McpOverrideState, McpPolicyOverride, McpServerKey};

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
