use std::fs;

use crate::domain::agents::{AgentHarnessKind, McpOverrideState, McpServerKey};

use super::mcp_policy_config::load_mcp_policy_file;

#[test]
fn parses_policy_only_and_reports_invalid_identifiers_without_definitions() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("mcp.yaml");
    fs::write(
        &path,
        "mcp:\n  providers:\n    claude:\n      servers:\n        github:\n          state: disabled\n          tools:\n            create_issue: disabled\n            ../unsafe: disabled\n",
    )
    .unwrap();

    let snapshot = load_mcp_policy_file(root.path(), &path, None).unwrap();
    let key = McpServerKey::new(AgentHarnessKind::Claude, "github").unwrap();
    let policy = snapshot.policies.get(&key).unwrap();
    assert_eq!(policy.server_state, McpOverrideState::Disabled);
    assert_eq!(
        policy.tool_states.get("create_issue"),
        Some(&McpOverrideState::Disabled)
    );
    assert_eq!(snapshot.diagnostics.len(), 1);
}

#[test]
fn rejects_unknown_structure_and_symlink_escape() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("mcp.yaml");
    fs::write(&path, "mcp:\n  commands: {}\n").unwrap();
    assert!(load_mcp_policy_file(root.path(), &path, None).is_err());

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let outside = tempfile::NamedTempFile::new().unwrap();
        fs::remove_file(&path).unwrap();
        symlink(outside.path(), &path).unwrap();
        assert!(load_mcp_policy_file(root.path(), &path, None).is_err());
    }
}
