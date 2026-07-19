use std::fs;

use crate::domain::agents::NativeMcpState;

use super::mcp_catalog::discover_native_mcp_servers;

#[test]
fn discovers_redacted_user_local_and_project_metadata_with_approval_state() {
    let home = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    fs::write(
        home.path().join(".claude.json"),
        serde_json::json!({
            "mcpServers": {"user-server": {"command": "secret", "env": {"TOKEN": "secret"}}},
            "projects": {
                project.path().to_string_lossy(): {
                    "mcpServers": {"local-server": {"url": "https://secret.invalid"}},
                    "enabledMcpjsonServers": ["approved"],
                    "disabledMcpjsonServers": ["disabled"]
                }
            }
        })
        .to_string(),
    )
    .unwrap();
    fs::write(
        project.path().join(".mcp.json"),
        r#"{"mcpServers":{"approved":{"command":"secret"},"disabled":{"command":"secret"},"pending":{"command":"secret"}}}"#,
    )
    .unwrap();

    let servers = discover_native_mcp_servers(home.path(), Some(project.path())).unwrap();
    let state = |id: &str| {
        servers
            .iter()
            .find(|server| server.key.server_id == id)
            .unwrap()
            .native_state
    };
    assert_eq!(state("user-server"), NativeMcpState::Enabled);
    assert_eq!(state("local-server"), NativeMcpState::Enabled);
    assert_eq!(state("approved"), NativeMcpState::Enabled);
    assert_eq!(state("disabled"), NativeMcpState::Disabled);
    assert_eq!(state("pending"), NativeMcpState::PendingApproval);
    let serialized = serde_json::to_string(&servers).unwrap();
    assert!(!serialized.contains("secret"));
    assert!(!serialized.contains("TOKEN"));
}

#[test]
fn rejects_project_config_symlink_escape() {
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        symlink(outside.path(), project.path().join(".mcp.json")).unwrap();
        assert!(discover_native_mcp_servers(home.path(), Some(project.path())).is_err());
    }
}
