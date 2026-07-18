use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;

use tempfile::TempDir;

use super::app_server_mcp_catalog::discover_native_mcp_servers_via_app_server;
use crate::domain::agents::NativeMcpState;

fn fake_app_server(script_body: &str) -> (TempDir, std::path::PathBuf) {
    let temp = tempfile::tempdir().expect("temp dir");
    let path = temp.path().join("codex");
    fs::write(
        &path,
        format!("#!/bin/sh\nwhile IFS= read -r line; do\n{script_body}\ndone\n"),
    )
    .expect("write fake Codex");
    let mut permissions = fs::metadata(&path).expect("fake metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions).expect("make fake executable");
    (temp, path)
}

#[tokio::test]
async fn reads_redacted_config_and_paginated_status_metadata() {
    let (_temp, cli) = fake_app_server(
        r#"case "$line" in
  *'"id":1'*) printf '%s\n' '{"id":1,"result":{"codexHome":"/secret/home"}}' ;;
  *'"id":2'*) printf '%s\n' '{"id":2,"result":{"config":{"mcp_servers":{"github":{"enabled":true,"url":"https://example.test"},"github.enterprise":{"enabled":true,"url":"https://token@example.test"},"disabled":{"enabled":false,"env":{"SECRET":"value"}}}},"origins":{"mcp_servers.github.url":{"name":{"type":"project"}},"mcp_servers.github.enterprise.url":{"name":{"type":"user","file":"/secret/config.toml"},"version":"secret"}}}}' ;;
  *'"id":3'*) printf '%s\n' '{"id":3,"result":{"data":[{"name":"github.enterprise","tools":{"search":{"description":"secret description"}},"authStatus":"oAuth"}],"nextCursor":"next"}}' ;;
  *'"id":4'*) printf '%s\n' '{"id":4,"result":{"data":[{"name":"plugin-only","tools":{"read":{}},"authStatus":"notLoggedIn"}],"nextCursor":null}}' ;;
esac"#,
    );
    let codex_home = tempfile::tempdir().expect("Codex home");

    let snapshots =
        discover_native_mcp_servers_via_app_server(&cli, codex_home.path(), None, &HashMap::new())
            .await
            .expect("structured catalog");

    let github = snapshots
        .iter()
        .find(|row| row.key.server_id == "github.enterprise")
        .expect("dotted server ID");
    assert_eq!(github.native_scope.as_deref(), Some("user"));
    assert_eq!(github.known_tools, vec!["search"]);
    let github_base = snapshots
        .iter()
        .find(|row| row.key.server_id == "github")
        .expect("base server ID");
    assert_eq!(github_base.native_scope.as_deref(), Some("project"));
    let disabled = snapshots
        .iter()
        .find(|row| row.key.server_id == "disabled")
        .expect("disabled config server");
    assert_eq!(disabled.native_state, NativeMcpState::Disabled);
    let auth = snapshots
        .iter()
        .find(|row| row.key.server_id == "plugin-only")
        .expect("status-only server");
    assert_eq!(auth.native_state, NativeMcpState::AuthRequired);

    let debug = format!("{snapshots:?}");
    for secret in [
        "token@example",
        "SECRET",
        "secret description",
        "/secret/home",
    ] {
        assert!(
            !debug.contains(secret),
            "catalog leaked sensitive provider content"
        );
    }
}

#[tokio::test]
async fn malformed_protocol_fails_without_echoing_provider_content() {
    let (_temp, cli) = fake_app_server(
        r#"case "$line" in
  *'"id":1'*) printf '%s\n' '{"id":1,"result":{}}' ;;
  *'"id":2'*) printf '%s\n' 'not-json-super-secret' ;;
esac"#,
    );
    let codex_home = tempfile::tempdir().expect("Codex home");

    let error =
        discover_native_mcp_servers_via_app_server(&cli, codex_home.path(), None, &HashMap::new())
            .await
            .expect_err("malformed protocol must fail");

    assert!(error.contains("malformed JSON"));
    assert!(!error.contains("super-secret"));
}
