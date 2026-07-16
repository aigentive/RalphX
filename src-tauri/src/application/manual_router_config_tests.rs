use std::fs;

use crate::domain::agents::{ManualServiceTier, RoutingRole};

use super::manual_router_config::load_manual_router_file;

#[test]
fn parses_valid_roles_while_isolating_an_invalid_role_entry() {
    let root = tempfile::tempdir().unwrap();
    let config_dir = root.path().join(".ralphx");
    fs::create_dir_all(&config_dir).unwrap();
    let path = config_dir.join("router.yaml");
    fs::write(
        &path,
        r#"
manual:
  defaults:
    roles:
      workspace_chat:
        provider: codex
        unknown_control: true
      workspace_edit:
        provider: codex
        model: gpt-5.6
        service_tier: standard
"#,
    )
    .unwrap();

    let snapshot = load_manual_router_file(root.path(), &path).unwrap();
    assert!(snapshot
        .entries
        .get(&RoutingRole::WorkspaceChat)
        .unwrap()
        .is_err());
    let edit = snapshot
        .entries
        .get(&RoutingRole::WorkspaceEdit)
        .unwrap()
        .as_ref()
        .unwrap();
    assert_eq!(edit.model.as_deref(), Some("gpt-5.6"));
    assert_eq!(edit.service_tier, ManualServiceTier::Standard);
}

#[test]
fn rejects_a_router_symlink_that_escapes_the_owned_root() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let config_dir = root.path().join(".ralphx");
    fs::create_dir_all(&config_dir).unwrap();
    let outside_file = outside.path().join("router.yaml");
    fs::write(&outside_file, "manual: {}").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside_file, config_dir.join("router.yaml")).unwrap();

    #[cfg(unix)]
    assert!(load_manual_router_file(root.path(), &config_dir.join("router.yaml")).is_err());
}
