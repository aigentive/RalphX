use std::collections::BTreeMap;

use tokio::process::Command;

use crate::domain::agents::{AgentHarnessKind, McpLaunchPolicy};
use crate::infrastructure::agents::claude::SpawnableCommand;

use super::mcp_launch_policy::apply_mcp_launch_policy;
use super::mcp_launch_policy::ensure_no_reserved_native_mcp_collision_at;

fn command() -> SpawnableCommand {
    SpawnableCommand::new(Command::new("provider"), None)
}

#[test]
fn applies_claude_denies_without_suppressing_native_config_sources() {
    let mut command = command();
    let policy = McpLaunchPolicy {
        disabled_servers: vec!["github".to_string()],
        disabled_tools: BTreeMap::new(),
    };

    apply_mcp_launch_policy(&mut command, AgentHarnessKind::Claude, &policy);

    assert_eq!(
        command.get_args_for_test(),
        vec!["--disallowedTools", "mcp__github__*"]
    );
}

#[test]
fn applies_codex_dotted_overrides_only_for_denies() {
    let mut command = command();
    let policy = McpLaunchPolicy {
        disabled_servers: vec!["github.enterprise".to_string()],
        disabled_tools: [(
            "linear.internal".to_string(),
            vec!["delete_issue".to_string()],
        )]
        .into_iter()
        .collect(),
    };

    apply_mcp_launch_policy(&mut command, AgentHarnessKind::Codex, &policy);

    assert_eq!(
        command.get_args_for_test(),
        vec![
            "-c",
            "mcp_servers.\"github.enterprise\".enabled=false",
            "-c",
            "mcp_servers.\"linear.internal\".disabled_tools=[\"delete_issue\"]",
        ]
    );
}

#[test]
fn rejects_provider_native_reserved_server_collision_before_launch() {
    let home = tempfile::tempdir().unwrap();
    std::fs::write(
        home.path().join(".claude.json"),
        r#"{"mcpServers":{"ralphx":{"command":"user-owned"}}}"#,
    )
    .unwrap();

    let error =
        ensure_no_reserved_native_mcp_collision_at(AgentHarnessKind::Claude, home.path(), None)
            .unwrap_err();

    let collision = error.collision().expect("typed collision");
    assert_eq!(collision.provider, AgentHarnessKind::Claude);
    assert_eq!(collision.server_id, "ralphx");
    assert_eq!(collision.native_scope.as_deref(), Some("user"));
    assert!(!error.safe_message().contains("user-owned"));
}

#[test]
fn unreadable_provider_catalog_fails_closed_without_exposing_config_content() {
    let home = tempfile::tempdir().unwrap();
    std::fs::write(home.path().join(".claude.json"), "not json").unwrap();

    let error =
        ensure_no_reserved_native_mcp_collision_at(AgentHarnessKind::Claude, home.path(), None)
            .unwrap_err();

    assert!(error.collision().is_none());
    assert_eq!(
        error.safe_message(),
        "MCP provider configuration could not be read safely"
    );
}

#[test]
fn internal_reserved_collision_is_typed_and_never_repairable() {
    let home = tempfile::tempdir().unwrap();
    std::fs::write(
        home.path().join(".claude.json"),
        r#"{"mcpServers":{"ralphx_internal":{"command":"user-owned"}}}"#,
    )
    .unwrap();

    let error =
        ensure_no_reserved_native_mcp_collision_at(AgentHarnessKind::Claude, home.path(), None)
            .unwrap_err();

    let collision = error.collision().expect("typed collision");
    assert_eq!(collision.server_id, "ralphx_internal");
    assert_eq!(collision.repair_status.to_string(), "manual_only");
}

#[test]
fn codex_reserved_collision_remains_fail_closed_without_repair() {
    let codex_home = tempfile::tempdir().unwrap();
    std::fs::write(
        codex_home.path().join("config.toml"),
        "[mcp_servers.ralphx]\ncommand = \"user-owned\"\n",
    )
    .unwrap();

    let error = ensure_no_reserved_native_mcp_collision_at(
        AgentHarnessKind::Codex,
        codex_home.path(),
        None,
    )
    .unwrap_err();

    let collision = error.collision().expect("typed collision");
    assert_eq!(collision.provider, AgentHarnessKind::Codex);
    assert_eq!(collision.server_id, "ralphx");
    assert_eq!(collision.repair_status.to_string(), "manual_only");
}

#[test]
fn rejects_codex_provider_native_reserved_server_collision_before_launch() {
    let codex_home = tempfile::tempdir().unwrap();
    std::fs::write(
        codex_home.path().join("config.toml"),
        "[mcp_servers.ralphx]\ncommand = \"user-owned\"\n",
    )
    .unwrap();

    let error = ensure_no_reserved_native_mcp_collision_at(
        AgentHarnessKind::Codex,
        codex_home.path(),
        None,
    )
    .unwrap_err();

    let collision = error.collision().expect("typed collision");
    assert_eq!(collision.provider, AgentHarnessKind::Codex);
    assert_eq!(collision.server_id, "ralphx");
    assert_eq!(collision.native_scope.as_deref(), Some("user"));
    assert!(!error.safe_message().contains("user-owned"));
}
