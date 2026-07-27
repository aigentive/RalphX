use std::fs;

use crate::domain::agents::{ManualServiceTier, RoutingRole};
use crate::error::AppError;

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

#[test]
fn accepts_a_missing_router_at_the_owned_root() {
    let root = tempfile::tempdir().unwrap();

    let snapshot = load_manual_router_file(root.path(), &root.path().join("router.yaml")).unwrap();

    assert!(snapshot.entries.is_empty());
    assert!(snapshot.diagnostics.is_empty());
}

#[test]
fn rejects_legacy_team_router_default_without_discarding_valid_entries() {
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
        provider: claude
        coordination_mode: legacy_claude_team
      workspace_plan:
        provider: codex
        model: gpt-5.6
"#,
    )
    .unwrap();

    let snapshot = load_manual_router_file(root.path(), &path).unwrap();

    assert!(snapshot
        .entries
        .get(&RoutingRole::WorkspaceChat)
        .unwrap()
        .as_ref()
        .unwrap_err()
        .contains("unknown variant"));
    assert_eq!(
        snapshot
            .entries
            .get(&RoutingRole::WorkspacePlan)
            .unwrap()
            .as_ref()
            .unwrap()
            .model
            .as_deref(),
        Some("gpt-5.6")
    );
}

#[test]
fn records_unknown_router_keys_and_unknown_roles() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("router.yaml");
    fs::write(
        &path,
        r#"
unexpected: true
manual:
  extra: true
  defaults:
    extra: true
    roles:
      unknown_role:
        provider: codex
      workspace_chat:
        provider: codex
"#,
    )
    .unwrap();

    let snapshot = load_manual_router_file(root.path(), &path).unwrap();

    assert!(snapshot
        .diagnostics
        .iter()
        .any(|message| message.contains("router.unexpected")));
    assert!(snapshot
        .diagnostics
        .iter()
        .any(|message| message.contains("manual.extra")));
    assert!(snapshot
        .diagnostics
        .iter()
        .any(|message| message.contains("manual.defaults.extra")));
    assert!(snapshot
        .diagnostics
        .iter()
        .any(|message| message.contains("Unknown routing role 'unknown_role'")));
    assert!(snapshot.entries.contains_key(&RoutingRole::WorkspaceChat));
}

#[test]
fn rejects_non_mapping_router_sections() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("router.yaml");
    fs::write(&path, "manual: []").unwrap();

    let error = load_manual_router_file(root.path(), &path).unwrap_err();

    assert!(matches!(error, AppError::Validation(message) if message.contains("'manual'")));
}

#[test]
fn rejects_unsupported_router_file_names_under_owned_root() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join(".ralphx").join("other.yaml");

    let error = load_manual_router_file(root.path(), &path).unwrap_err();

    assert!(matches!(error, AppError::Validation(message) if message.contains("supported path")));
}

#[test]
fn rejects_a_missing_router_outside_the_owned_root() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let outside_path = outside.path().join("router.yaml");

    let error = load_manual_router_file(root.path(), &outside_path).unwrap_err();

    assert!(matches!(error, AppError::Validation(message) if message.contains("owned root")));
}

#[test]
fn rejects_a_router_path_with_parent_traversal() {
    let root = tempfile::tempdir().unwrap();
    let traversing_path = root.path().join(".ralphx").join("..").join("router.yaml");

    let error = load_manual_router_file(root.path(), &traversing_path).unwrap_err();

    assert!(
        matches!(error, AppError::Validation(message) if message.contains("unsafe components"))
    );
}
