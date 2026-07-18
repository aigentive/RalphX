use std::fs;

use crate::domain::agents::NativeMcpState;

use super::mcp_catalog::discover_native_mcp_servers;

#[test]
fn discovers_redacted_user_and_trust_gated_project_servers() {
    let home = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    fs::create_dir_all(home.path().join(".codex")).unwrap();
    fs::create_dir_all(project.path().join(".codex")).unwrap();
    fs::write(
        home.path().join(".codex/config.toml"),
        format!(
            "[mcp_servers.user]\ncommand = \"secret\"\ndisabled_tools = [\"delete_issue\"]\n\n[projects.\"{}\"]\ntrust_level = \"untrusted\"\n",
            project.path().display()
        ),
    )
    .unwrap();
    fs::write(
        project.path().join(".codex/config.toml"),
        "[mcp_servers.project]\nurl = \"https://token.invalid\"\n",
    )
    .unwrap();

    let servers = discover_native_mcp_servers(home.path(), Some(project.path())).unwrap();
    let user = servers
        .iter()
        .find(|row| row.key.server_id == "user")
        .unwrap();
    let project_server = servers
        .iter()
        .find(|row| row.key.server_id == "project")
        .unwrap();
    assert_eq!(user.native_state, NativeMcpState::Enabled);
    assert_eq!(user.known_tools, vec!["delete_issue".to_string()]);
    assert_eq!(project_server.native_state, NativeMcpState::Untrusted);
    let serialized = serde_json::to_string(&servers).unwrap();
    assert!(!serialized.contains("secret"));
    assert!(!serialized.contains("token.invalid"));
}

#[test]
fn trusted_project_config_overrides_same_named_user_metadata() {
    let home = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    fs::create_dir_all(home.path().join(".codex")).unwrap();
    fs::create_dir_all(project.path().join(".codex")).unwrap();
    fs::write(
        home.path().join(".codex/config.toml"),
        format!(
            "[mcp_servers.shared]\nenabled = false\n\n[projects.\"{}\"]\ntrust_level = \"trusted\"\n",
            project.path().display()
        ),
    )
    .unwrap();
    fs::write(
        project.path().join(".codex/config.toml"),
        "[mcp_servers.shared]\nenabled = true\n",
    )
    .unwrap();

    let servers = discover_native_mcp_servers(home.path(), Some(project.path())).unwrap();
    assert_eq!(servers.len(), 1);
    assert_eq!(servers[0].native_state, NativeMcpState::Enabled);
    assert_eq!(servers[0].native_scope.as_deref(), Some("project"));
}
