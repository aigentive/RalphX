use super::harness_runtime_registry::{
    clear_harness_runtime_caches_for_tests, probe_supported_harnesses,
    refresh_harness_runtime_probe, refresh_supported_harnesses,
    resolve_startup_harness_integration_with_provider_repo,
    resolve_startup_harness_integration_with_provider_settings, HarnessRuntimeProbe,
    ResolvedHarnessStartupIntegration,
};
use crate::domain::agents::{
    AgentHarnessKind, AgentProviderCliManagementMode, AgentProviderSettings,
};
use crate::domain::repositories::AgentProviderSettingsRepository;
use crate::infrastructure::memory::MemoryAgentProviderSettingsRepository;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::Path;
use std::sync::Arc;

struct EnvGuard {
    key: &'static str,
    original: Option<std::ffi::OsString>,
}

impl EnvGuard {
    fn set_os(key: &'static str, value: impl AsRef<OsStr>) -> Self {
        let original = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, original }
    }

    fn unset(key: &'static str) -> Self {
        let original = std::env::var_os(key);
        std::env::remove_var(key);
        Self { key, original }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.original {
            Some(value) => std::env::set_var(self.key, value),
            None => std::env::remove_var(self.key),
        }
    }
}

#[cfg(unix)]
fn write_fake_executable(path: &Path, body: &str) {
    std::fs::write(path, body).expect("write fake executable");
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = std::fs::metadata(path)
        .expect("fake executable metadata")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions).expect("mark fake executable");
}

#[cfg(unix)]
fn write_fake_claude(path: &Path, version: &str) {
    write_fake_executable(
        path,
        &format!(
            r#"#!/bin/sh
case "$1" in
  --version)
    echo "{version} (Claude Code)"
    ;;
  --help)
    echo "Options:"
    echo "  --effort <level>  Effort level for the current session (low, medium, high, xhigh, max)"
    ;;
  *)
    exit 2
    ;;
esac
"#
        ),
    );
}

#[cfg(unix)]
fn write_fake_codex(path: &Path) {
    write_fake_executable(
        path,
        r#"#!/bin/sh
case "$1" in
  --version)
    echo "codex-cli 0.99.0"
    ;;
  --help)
    echo "Usage: codex [options] <prompt>"
    echo "Commands: exec resume mcp"
    echo "Options: --config --model --sandbox --add-dir --search"
    ;;
  exec)
    echo "Run Codex non-interactively"
    echo "Options: --config --model --sandbox --add-dir --json"
    ;;
  *)
    exit 2
    ;;
esac
"#,
    );
}

fn claude_probe(
    probes: &mut HashMap<AgentHarnessKind, HarnessRuntimeProbe>,
) -> HarnessRuntimeProbe {
    probes
        .remove(&AgentHarnessKind::Claude)
        .expect("Claude probe should be present")
}

fn model_aliases(probe: &HarnessRuntimeProbe) -> Vec<String> {
    probe.supported_model_aliases.clone().unwrap_or_default()
}

fn make_runtime_plugin_layout() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().to_path_buf();
    let plugin_dir = root.join("plugins/app");
    let generated_dir = root.join("generated/claude-plugin");

    std::fs::create_dir_all(plugin_dir.join("agents")).expect("create agents dir");
    std::fs::write(
        plugin_dir.join("agents/session-namer.md"),
        "# Session Namer\n",
    )
    .expect("write session namer prompt");
    std::fs::create_dir_all(plugin_dir.join("ralphx-mcp-server/build"))
        .expect("create mcp build dir");
    std::fs::create_dir_all(
        plugin_dir.join("ralphx-mcp-server/node_modules/@modelcontextprotocol/sdk"),
    )
    .expect("create mcp sdk marker dir");
    std::fs::write(
        plugin_dir.join("ralphx-mcp-server/build/index.js"),
        "// fake mcp runtime\n",
    )
    .expect("write mcp runtime entry");
    std::fs::write(
        plugin_dir.join("ralphx-mcp-server/node_modules/@modelcontextprotocol/sdk/package.json"),
        "{}\n",
    )
    .expect("write mcp runtime marker");

    (temp, plugin_dir, generated_dir)
}

#[cfg(unix)]
#[test]
fn startup_integration_uses_custom_claude_provider_binary() {
    let _env_lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("env mutex");
    let (_plugin_temp, plugin_dir, generated_dir) = make_runtime_plugin_layout();
    let _plugin_override =
        crate::infrastructure::agents::claude::override_runtime_plugin_dirs_for_tests(
            plugin_dir,
            generated_dir.clone(),
        );
    let temp = tempfile::tempdir().expect("tempdir");
    let bin_dir = temp.path().join("bin");
    std::fs::create_dir_all(&bin_dir).expect("create bin dir");
    let custom_claude = bin_dir.join("claude-wrapper");
    write_fake_claude(&custom_claude, "2.1.197");
    let mut settings = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Claude);
    settings.enabled = true;
    settings.cli_management_mode = AgentProviderCliManagementMode::UserManaged;
    settings.custom_binary_enabled = true;
    settings.custom_binary_path = Some(custom_claude.to_string_lossy().into_owned());

    let integration = resolve_startup_harness_integration_with_provider_settings(
        AgentHarnessKind::Claude,
        Some(&settings),
    )
    .expect("custom Claude startup integration should resolve")
    .expect("Claude startup integration should be present");

    match integration {
        ResolvedHarnessStartupIntegration::RegisterConfiguredMcpServer {
            harness,
            cli_path,
            plugin_dir,
        } => {
            assert_eq!(harness, AgentHarnessKind::Claude);
            assert_eq!(cli_path, custom_claude);
            assert_eq!(plugin_dir, generated_dir);
        }
    }
}

#[tokio::test]
#[cfg(unix)]
#[allow(clippy::await_holding_lock)]
async fn startup_integration_provider_repo_uses_custom_claude_provider_binary() {
    let _env_lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("env mutex");
    let (_plugin_temp, plugin_dir, generated_dir) = make_runtime_plugin_layout();
    let _plugin_override =
        crate::infrastructure::agents::claude::override_runtime_plugin_dirs_for_tests(
            plugin_dir,
            generated_dir,
        );
    let temp = tempfile::tempdir().expect("tempdir");
    let bin_dir = temp.path().join("bin");
    std::fs::create_dir_all(&bin_dir).expect("create bin dir");
    let custom_claude = bin_dir.join("claude-wrapper");
    write_fake_claude(&custom_claude, "2.1.197");
    let mut settings = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Claude);
    settings.enabled = true;
    settings.cli_management_mode = AgentProviderCliManagementMode::UserManaged;
    settings.custom_binary_enabled = true;
    settings.custom_binary_path = Some(custom_claude.to_string_lossy().into_owned());
    let repo = Arc::new(MemoryAgentProviderSettingsRepository::new());
    repo.upsert(&settings)
        .await
        .expect("upsert custom Claude settings");
    let repo = repo as Arc<dyn AgentProviderSettingsRepository>;

    let integration =
        resolve_startup_harness_integration_with_provider_repo(AgentHarnessKind::Claude, &repo)
            .await
            .expect("custom Claude startup integration should resolve")
            .expect("Claude startup integration should be present");

    match integration {
        ResolvedHarnessStartupIntegration::RegisterConfiguredMcpServer { cli_path, .. } => {
            assert_eq!(cli_path, custom_claude);
        }
    }
}

#[test]
fn startup_integration_reports_invalid_custom_claude_binary_settings() {
    let mut settings = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Claude);
    settings.enabled = true;
    settings.cli_management_mode = AgentProviderCliManagementMode::UserManaged;
    settings.custom_binary_enabled = true;
    settings.custom_binary_path = Some("   ".to_string());

    let error = resolve_startup_harness_integration_with_provider_settings(
        AgentHarnessKind::Claude,
        Some(&settings),
    )
    .expect_err("invalid custom Claude path should fail startup integration");

    assert!(error.contains("Custom claude binary path is required"));
}

#[tokio::test]
async fn codex_startup_integration_keeps_provider_repo_noop() {
    let repo = Arc::new(MemoryAgentProviderSettingsRepository::new())
        as Arc<dyn AgentProviderSettingsRepository>;

    let integration =
        resolve_startup_harness_integration_with_provider_repo(AgentHarnessKind::Codex, &repo)
            .await
            .expect("Codex startup integration should resolve");

    assert!(integration.is_none());
}

#[cfg(unix)]
#[test]
fn refresh_supported_harnesses_reprobes_claude_aliases_after_cli_update() {
    let _env_lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("env mutex");
    let temp = tempfile::tempdir().expect("tempdir");
    let bin_dir = temp.path().join("bin");
    std::fs::create_dir_all(&bin_dir).expect("create bin dir");
    let claude_cli = bin_dir.join("claude");
    let codex_cli = bin_dir.join("codex");
    write_fake_claude(&claude_cli, "2.1.146");
    write_fake_codex(&codex_cli);

    let _path = EnvGuard::set_os("PATH", &bin_dir);
    let _home = EnvGuard::set_os("HOME", temp.path());
    let _zdotdir = EnvGuard::set_os("ZDOTDIR", temp.path());
    let _nvm = EnvGuard::unset("NVM_BIN");
    let _volta = EnvGuard::unset("VOLTA_HOME");

    clear_harness_runtime_caches_for_tests(AgentHarnessKind::Claude);
    clear_harness_runtime_caches_for_tests(AgentHarnessKind::Codex);

    let initial_probe = refresh_harness_runtime_probe(AgentHarnessKind::Claude);
    assert_eq!(initial_probe.cli_version.as_deref(), Some("2.1.146"));
    assert!(!model_aliases(&initial_probe).contains(&"fable".to_string()));
    assert!(!model_aliases(&initial_probe).contains(&"claude-sonnet-4-6".to_string()));
    assert!(!model_aliases(&initial_probe).contains(&"claude-sonnet-5".to_string()));

    write_fake_claude(&claude_cli, "2.1.197");
    let cached_probe = claude_probe(&mut probe_supported_harnesses());
    assert_eq!(cached_probe.cli_version.as_deref(), Some("2.1.146"));
    assert!(!model_aliases(&cached_probe).contains(&"fable".to_string()));

    let refreshed_probe = claude_probe(&mut refresh_supported_harnesses());
    assert_eq!(refreshed_probe.cli_version.as_deref(), Some("2.1.197"));
    assert!(model_aliases(&refreshed_probe).contains(&"fable".to_string()));
    assert!(model_aliases(&refreshed_probe).contains(&"claude-sonnet-4-6".to_string()));
    assert!(model_aliases(&refreshed_probe).contains(&"claude-sonnet-5".to_string()));

    clear_harness_runtime_caches_for_tests(AgentHarnessKind::Claude);
    clear_harness_runtime_caches_for_tests(AgentHarnessKind::Codex);
}
