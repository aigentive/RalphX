use std::fs;

use crate::domain::agents::NativeMcpState;

use super::mcp_catalog::{
    classify_legacy_user_registration, discover_native_mcp_servers, LegacyClaudeRegistration,
};

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

#[test]
fn classifies_v081_trace_enabled_user_registration() {
    let home = tempfile::tempdir().unwrap();
    let app_data = tempfile::tempdir().unwrap();
    let script = app_data
        .path()
        .join("generated/release/claude-plugin/ralphx-mcp-server/build/index.js");
    fs::create_dir_all(script.parent().unwrap()).unwrap();
    fs::write(&script, "fixture").unwrap();
    let trace_dir = app_data.path().join("logs/mcp-proxy");
    fs::write(
        home.path().join(".claude.json"),
        serde_json::json!({
            "mcpServers": {
                "ralphx": {
                    "type": "stdio",
                    "command": "node",
                    "args": [script, "--trace-dir", trace_dir]
                }
            }
        })
        .to_string(),
    )
    .unwrap();

    assert_eq!(
        classify_legacy_user_registration(home.path(), app_data.path()).unwrap(),
        LegacyClaudeRegistration::ExactHistorical
    );
}

#[test]
fn classifies_plugin_template_user_registration() {
    let home = tempfile::tempdir().unwrap();
    let app_data = tempfile::tempdir().unwrap();
    let script = app_data
        .path()
        .join("generated/claude-plugin/ralphx-mcp-server/build/index.js");
    fs::create_dir_all(script.parent().unwrap()).unwrap();
    fs::write(&script, "fixture").unwrap();
    fs::write(
        home.path().join(".claude.json"),
        serde_json::json!({
            "mcpServers": {
                "ralphx": {
                    "type": "stdio",
                    "command": "node",
                    "args": [script]
                }
            }
        })
        .to_string(),
    )
    .unwrap();

    assert_eq!(
        classify_legacy_user_registration(home.path(), app_data.path()).unwrap(),
        LegacyClaudeRegistration::ExactHistorical
    );
}

#[cfg(unix)]
#[test]
fn classifies_app_managed_symlinked_runtime_registration() {
    use std::os::unix::fs::symlink;

    use crate::infrastructure::agents::claude::override_runtime_plugin_dirs_for_tests;

    let home = tempfile::tempdir().unwrap();
    let app_data = tempfile::tempdir().unwrap();
    let runtime = tempfile::tempdir().unwrap();
    let runtime_server = runtime.path().join("plugins/app/ralphx-mcp-server");
    let runtime_script = runtime_server.join("build/index.js");
    fs::create_dir_all(runtime_script.parent().unwrap()).unwrap();
    fs::write(&runtime_script, "fixture").unwrap();

    let generated_plugin = app_data.path().join("generated/release/claude-plugin");
    fs::create_dir_all(&generated_plugin).unwrap();
    symlink(&runtime_server, generated_plugin.join("ralphx-mcp-server")).unwrap();
    let generated_script = generated_plugin.join("ralphx-mcp-server/build/index.js");
    fs::write(
        home.path().join(".claude.json"),
        serde_json::json!({
            "mcpServers": {
                "ralphx": {
                    "type": "stdio",
                    "command": "node",
                    "args": [
                        generated_script,
                        "--trace-dir",
                        app_data.path().join("logs/mcp-proxy")
                    ]
                }
            }
        })
        .to_string(),
    )
    .unwrap();
    let _runtime_dirs = override_runtime_plugin_dirs_for_tests(
        runtime.path().join("plugins/app"),
        generated_plugin,
    );

    assert_eq!(
        classify_legacy_user_registration(home.path(), app_data.path()).unwrap(),
        LegacyClaudeRegistration::ExactHistorical
    );
}

#[test]
fn classifies_missing_and_unregistered_user_config_as_not_present() {
    let home = tempfile::tempdir().unwrap();
    let app_data = tempfile::tempdir().unwrap();

    assert_eq!(
        classify_legacy_user_registration(home.path(), app_data.path()).unwrap(),
        LegacyClaudeRegistration::NotPresent
    );

    fs::write(
        home.path().join(".claude.json"),
        r#"{"mcpServers":{"github":{"command":"provider-owned"}}}"#,
    )
    .unwrap();
    assert_eq!(
        classify_legacy_user_registration(home.path(), app_data.path()).unwrap(),
        LegacyClaudeRegistration::NotPresent
    );
}

#[test]
fn rejects_plugin_template_one_field_deviations_and_internal_reserved_registration() {
    let home = tempfile::tempdir().unwrap();
    let app_data = tempfile::tempdir().unwrap();
    let script = app_data
        .path()
        .join("generated/claude-plugin/ralphx-mcp-server/build/index.js");
    fs::create_dir_all(script.parent().unwrap()).unwrap();
    fs::write(&script, "fixture").unwrap();
    for registration in [
        serde_json::json!({
            "type": "stdio",
            "command": "node",
            "args": [script],
            "env": {"TOKEN": "must-not-be-accepted"}
        }),
        serde_json::json!({
            "type": "stdio",
            "command": "node",
            "args": [script, "--unexpected"]
        }),
        serde_json::json!({
            "type": "stdio",
            "command": "other-node",
            "args": [script]
        }),
        serde_json::json!({
            "command": "node",
            "args": [script]
        }),
    ] {
        fs::write(
            home.path().join(".claude.json"),
            serde_json::json!({
                "mcpServers": {
                    "ralphx": registration,
                    "ralphx_internal": {"command": "node", "args": []}
                }
            })
            .to_string(),
        )
        .unwrap();

        assert_eq!(
            classify_legacy_user_registration(home.path(), app_data.path()).unwrap(),
            LegacyClaudeRegistration::AmbiguousCollision
        );
    }
}

#[test]
fn rejects_historical_shape_when_the_script_symlink_escapes_app_data() {
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        let home = tempfile::tempdir().unwrap();
        let app_data = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let outside_script = outside.path().join("index.js");
        fs::write(&outside_script, "outside").unwrap();
        let script = app_data
            .path()
            .join("generated/release/claude-plugin/ralphx-mcp-server/build/index.js");
        fs::create_dir_all(script.parent().unwrap()).unwrap();
        symlink(&outside_script, &script).unwrap();
        fs::write(
            home.path().join(".claude.json"),
            serde_json::json!({
                "mcpServers": {"ralphx": {
                    "type": "stdio",
                    "command": "node",
                    "args": [script, "--trace-dir", app_data.path().join("logs/mcp-proxy")]
                }}
            })
            .to_string(),
        )
        .unwrap();

        assert_eq!(
            classify_legacy_user_registration(home.path(), app_data.path()).unwrap(),
            LegacyClaudeRegistration::AmbiguousCollision
        );
    }
}
