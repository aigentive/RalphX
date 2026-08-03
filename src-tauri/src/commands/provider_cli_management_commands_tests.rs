use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use crate::application::interactive_process_registry::{
    InteractiveProcessKey, InteractiveProcessMetadata,
};
use crate::application::AppState;
use crate::domain::agents::{
    AgentHarnessKind, AgentProviderCliManagementMode, AgentProviderSettings,
};
use crate::domain::entities::{AgentRun, ChatConversation, ChatConversationId, ProjectId};
use crate::domain::services::RunningAgentKey;
use crate::infrastructure::agents::{CodexCliCapabilities, ResolvedCodexCli};
use tokio::process::Command;

use super::{
    compare_version_strings, ensure_managed_codex_dirs, fetch_latest_github_release_version,
    install_or_update_managed_provider_cli_inner, managed_claude_install_plan,
    managed_claude_observation, managed_cli_status_response, managed_codex_bin_dir,
    managed_codex_install_plan, managed_codex_observation,
    managed_provider_cli_status_for_settings, managed_provider_has_active_runtime,
    normalize_codex_release_tag, parse_codex_version, parse_provider, path_with_prepended_dir,
    probe_cli_version, read_managed_provider_cli_statuses, run_managed_claude_command,
    run_managed_claude_install_or_update, run_managed_claude_installer,
    run_managed_codex_installer, settings_for_provider, truncate_process_output,
    user_managed_claude_observation, user_managed_codex_observation,
    user_managed_codex_observation_from_resolved_cli, ManagedCodexInstallPlan,
    ManagedProviderCliObservation,
};

fn codex_settings(mode: AgentProviderCliManagementMode) -> AgentProviderSettings {
    let mut settings = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Codex);
    settings.cli_management_mode = mode;
    settings.auto_update_enabled = mode == AgentProviderCliManagementMode::RxManaged;
    settings
}

fn claude_settings(mode: AgentProviderCliManagementMode) -> AgentProviderSettings {
    let mut settings = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Claude);
    settings.cli_management_mode = mode;
    settings.auto_update_enabled = mode == AgentProviderCliManagementMode::RxManaged;
    settings
}

fn codex_observation(
    installed: bool,
    current_version: Option<&str>,
    latest_version: Option<&str>,
) -> ManagedProviderCliObservation {
    ManagedProviderCliObservation {
        supported: true,
        installed,
        binary_path: Some(PathBuf::from("/tmp/ralphx-managed/codex")),
        current_version: current_version.map(str::to_string),
        latest_version: latest_version.map(str::to_string),
        error: None,
    }
}

fn claude_observation(
    installed: bool,
    current_version: Option<&str>,
    latest_version: Option<&str>,
) -> ManagedProviderCliObservation {
    ManagedProviderCliObservation {
        supported: true,
        installed,
        binary_path: Some(PathBuf::from("/Users/example/.local/bin/claude")),
        current_version: current_version.map(str::to_string),
        latest_version: latest_version.map(str::to_string),
        error: None,
    }
}

#[test]
fn codex_rx_managed_missing_cli_suggests_install() {
    let status = managed_cli_status_response(
        codex_settings(AgentProviderCliManagementMode::RxManaged),
        codex_observation(false, None, Some("0.137.0")),
        false,
    );

    assert_eq!(status.provider, "codex");
    assert!(status.supported);
    assert!(!status.installed);
    assert_eq!(status.action, "install");
    assert!(status.status.contains("not installed"));
}

#[test]
fn codex_rx_managed_stale_cli_suggests_update() {
    let status = managed_cli_status_response(
        codex_settings(AgentProviderCliManagementMode::RxManaged),
        codex_observation(true, Some("0.136.0"), Some("0.137.0")),
        false,
    );

    assert_eq!(status.action, "update");
    assert!(status.update_available);
    assert!(status.status.contains("0.136.0"));
    assert!(status.status.contains("0.137.0"));
}

#[test]
fn codex_user_managed_stale_cli_reports_update_without_managed_action() {
    let status = managed_cli_status_response(
        codex_settings(AgentProviderCliManagementMode::UserManaged),
        codex_observation(true, Some("0.136.0"), Some("0.137.0")),
        false,
    );

    assert_eq!(status.action, "none");
    assert!(status.update_available);
    assert!(status.status.contains("user-managed"));
    assert!(status.status.contains("0.136.0"));
    assert!(status.status.contains("0.137.0"));
    assert!(status.status.contains("unless management is enabled"));
}

#[test]
fn custom_binary_status_never_reports_managed_update_action() {
    let mut settings = codex_settings(AgentProviderCliManagementMode::UserManaged);
    settings.custom_binary_enabled = true;
    settings.custom_binary_path = Some("/opt/tools/codex-wrapper".to_string());
    let status = managed_cli_status_response(
        settings,
        codex_observation(true, Some("0.136.0"), Some("0.137.0")),
        false,
    );

    assert_eq!(status.action, "none");
    assert!(!status.update_available);
    assert!(status.custom_binary_enabled);
    assert_eq!(
        status.custom_binary_path.as_deref(),
        Some("/opt/tools/codex-wrapper")
    );
    assert!(status.status.contains("Custom codex CLI 0.136.0"));
    assert!(status.status.contains("will not install or update"));
}

#[test]
fn codex_rx_managed_active_cli_suppresses_update_action() {
    let status = managed_cli_status_response(
        codex_settings(AgentProviderCliManagementMode::RxManaged),
        codex_observation(true, Some("0.136.0"), Some("0.137.0")),
        true,
    );

    assert_eq!(status.action, "none");
    assert!(status.update_available);
    assert!(status.status.contains("currently in use"));
}

#[test]
fn claude_rx_managed_missing_native_cli_suggests_install() {
    let status = managed_cli_status_response(
        claude_settings(AgentProviderCliManagementMode::RxManaged),
        claude_observation(false, None, Some("2.1.175")),
        false,
    );

    assert_eq!(status.provider, "claude");
    assert!(status.supported);
    assert!(!status.installed);
    assert_eq!(status.action, "install");
    assert!(status.status.contains("not installed"));
}

#[test]
fn claude_rx_managed_stale_native_cli_suggests_update() {
    let status = managed_cli_status_response(
        claude_settings(AgentProviderCliManagementMode::RxManaged),
        claude_observation(true, Some("2.1.170"), Some("2.1.175")),
        false,
    );

    assert_eq!(status.action, "update");
    assert!(status.update_available);
    assert!(status.status.contains("2.1.170"));
    assert!(status.status.contains("2.1.175"));
}

#[test]
fn unsupported_provider_status_reports_unavailable_action() {
    let status = managed_cli_status_response(
        codex_settings(AgentProviderCliManagementMode::RxManaged),
        ManagedProviderCliObservation {
            supported: false,
            installed: false,
            binary_path: None,
            current_version: None,
            latest_version: None,
            error: Some("not supported here".to_string()),
        },
        false,
    );

    assert_eq!(status.action, "unsupported");
    assert!(!status.supported);
    assert_eq!(status.error.as_deref(), Some("not supported here"));
    assert!(status.status.contains("unavailable"));
}

#[test]
fn claude_user_managed_stale_cli_reports_update_without_managed_action() {
    let status = managed_cli_status_response(
        claude_settings(AgentProviderCliManagementMode::UserManaged),
        claude_observation(true, Some("2.1.170"), Some("2.1.175")),
        false,
    );

    assert_eq!(status.action, "none");
    assert!(status.update_available);
    assert!(status.status.contains("user-managed"));
    assert!(status.status.contains("2.1.170"));
    assert!(status.status.contains("2.1.175"));
    assert!(status.status.contains("unless management is enabled"));
}

#[test]
fn passive_statuses_do_not_offer_managed_actions() {
    let user_current = managed_cli_status_response(
        codex_settings(AgentProviderCliManagementMode::UserManaged),
        codex_observation(true, Some("0.137.0"), Some("0.137.0")),
        false,
    );
    assert_eq!(user_current.action, "none");
    assert!(!user_current.update_available);
    assert!(user_current.status.contains("user-managed"));
    assert!(user_current.status.contains("0.137.0"));

    let user_unknown = managed_cli_status_response(
        claude_settings(AgentProviderCliManagementMode::UserManaged),
        claude_observation(false, None, None),
        false,
    );
    assert_eq!(user_unknown.action, "none");
    assert!(!user_unknown.update_available);
    assert!(user_unknown.status.contains("will not install or update"));

    let managed_current = managed_cli_status_response(
        codex_settings(AgentProviderCliManagementMode::RxManaged),
        codex_observation(true, Some("0.137.0"), Some("0.137.0")),
        false,
    );
    assert_eq!(managed_current.action, "none");
    assert!(!managed_current.update_available);
    assert!(managed_current.status.contains("0.137.0 is installed"));

    let managed_unknown = managed_cli_status_response(
        claude_settings(AgentProviderCliManagementMode::RxManaged),
        claude_observation(true, None, None),
        false,
    );
    assert_eq!(managed_unknown.action, "none");
    assert!(managed_unknown.status.contains("is installed"));
}

#[test]
fn provider_parsing_and_default_settings_are_provider_specific() {
    assert_eq!(parse_provider("codex").unwrap(), AgentHarnessKind::Codex);
    assert_eq!(parse_provider("claude").unwrap(), AgentHarnessKind::Claude);
    assert!(parse_provider("fable")
        .unwrap_err()
        .contains("Invalid provider"));

    let mut stored_claude = claude_settings(AgentProviderCliManagementMode::RxManaged);
    stored_claude.auto_update_enabled = true;
    let selected = settings_for_provider(&[stored_claude.clone()], AgentHarnessKind::Claude);
    assert_eq!(selected, stored_claude);

    let fallback = settings_for_provider(&[stored_claude], AgentHarnessKind::Codex);
    assert_eq!(fallback.provider, AgentHarnessKind::Codex);
    assert_eq!(
        fallback.cli_management_mode,
        AgentProviderCliManagementMode::UserManaged
    );
    assert!(!fallback.auto_update_enabled);
}

#[cfg(unix)]
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn provider_observations_probe_fake_clis_without_latest_lookup() {
    let _lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("test env mutex");
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let home_dir = temp_dir.path().join("home");
    let path_bin_dir = temp_dir.path().join("path-bin");
    let native_bin_dir = home_dir.join(".local").join("bin");
    let _managed_cli_dir =
        crate::utils::runtime_log_paths::override_managed_provider_cli_dir_for_tests(
            temp_dir.path().join("managed-cli"),
        );
    let managed_codex_path = crate::utils::runtime_log_paths::managed_codex_binary_path();
    let _managed_codex_cleanup = FileCleanup::new(managed_codex_path.clone());

    write_compatible_codex_script(&path_bin_dir.join("codex"), "0.141.0");
    write_executable_script(
        &path_bin_dir.join("claude"),
        "#!/bin/sh\nprintf '2.1.177 (Claude Code)\\n'\n",
    );
    write_executable_script(
        &native_bin_dir.join("claude"),
        "#!/bin/sh\nprintf '2.1.178 (Claude Code)\\n'\n",
    );
    write_executable_script(
        &managed_codex_path,
        "#!/bin/sh\nprintf 'codex-cli 0.142.0\\n'\n",
    );

    let _home = EnvGuard::set_os("HOME", &home_dir);
    let _path = EnvGuard::set_os("PATH", &path_bin_dir);
    let _nvm_bin = EnvGuard::unset("NVM_BIN");
    let _volta_home = EnvGuard::unset("VOLTA_HOME");

    let managed_codex = managed_codex_observation(false).await;
    assert!(managed_codex.supported);
    assert!(managed_codex.installed);
    assert_eq!(
        managed_codex.binary_path.as_deref(),
        Some(managed_codex_path.as_path())
    );
    assert_eq!(managed_codex.current_version.as_deref(), Some("0.142.0"));
    assert_eq!(managed_codex.latest_version, None);
    assert_eq!(managed_codex.error, None);

    let user_codex = user_managed_codex_observation(false).await;
    assert!(user_codex.installed);
    assert_eq!(
        user_codex.binary_path.as_deref(),
        Some(path_bin_dir.join("codex").as_path())
    );
    assert_eq!(user_codex.current_version.as_deref(), Some("0.141.0"));
    assert_eq!(user_codex.latest_version, None);

    let managed_claude = managed_claude_observation(false).await;
    assert!(managed_claude.supported);
    assert!(managed_claude.installed);
    assert_eq!(
        managed_claude.binary_path.as_deref(),
        Some(native_bin_dir.join("claude").as_path())
    );
    assert_eq!(managed_claude.current_version.as_deref(), Some("2.1.178"));
    assert_eq!(managed_claude.latest_version, None);

    let user_claude = user_managed_claude_observation(false).await;
    assert!(user_claude.installed);
    assert_eq!(
        user_claude.binary_path.as_deref(),
        Some(path_bin_dir.join("claude").as_path())
    );
    assert_eq!(user_claude.current_version.as_deref(), Some("2.1.177"));
    assert_eq!(user_claude.latest_version, None);
}

#[cfg(unix)]
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn user_managed_codex_observation_uses_resolved_runtime_cli_after_legacy_node_candidate() {
    let _lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("test env mutex");
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let home_dir = temp_dir.path().join("home");
    let legacy_codex_bin = home_dir
        .join(".nvm")
        .join("versions")
        .join("node")
        .join("v22.16.0")
        .join("bin");
    let runtime_codex_path = home_dir.join(".local").join("bin").join("codex");

    write_executable_script(
        &legacy_codex_bin.join("codex"),
        r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  printf '0.1.2505172129\n'
elif [ "$1" = "--help" ]; then
  printf '%s\n' 'Usage' '  $ codex [options] <prompt>' 'Options:' '  --version'
elif [ "$1" = "exec" ] && [ "$2" = "--help" ]; then
  exit 2
else
  exit 64
fi
"#,
    );
    write_compatible_codex_script(&runtime_codex_path, "0.142.0");

    let _home = EnvGuard::set_os("HOME", &home_dir);
    let _path = EnvGuard::set_os("PATH", OsStr::new(""));
    let _nvm_bin = EnvGuard::unset("NVM_BIN");
    let _volta_home = EnvGuard::unset("VOLTA_HOME");

    let user_codex = user_managed_codex_observation(false).await;

    assert!(user_codex.installed);
    assert_eq!(
        user_codex.binary_path.as_deref(),
        Some(runtime_codex_path.as_path())
    );
    assert_eq!(user_codex.current_version.as_deref(), Some("0.142.0"));
    assert_eq!(user_codex.latest_version, None);
    assert_eq!(user_codex.error, None);
}

#[cfg(unix)]
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn status_readers_use_provider_settings_and_probe_without_latest_lookup() {
    let _lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("test env mutex");
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let home_dir = temp_dir.path().join("home");
    let native_bin_dir = home_dir.join(".local").join("bin");
    let _managed_cli_dir =
        crate::utils::runtime_log_paths::override_managed_provider_cli_dir_for_tests(
            temp_dir.path().join("managed-cli"),
        );
    let managed_codex_path = crate::utils::runtime_log_paths::managed_codex_binary_path();
    let _managed_codex_cleanup = FileCleanup::new(managed_codex_path.clone());

    write_executable_script(
        &managed_codex_path,
        "#!/bin/sh\nprintf 'codex-cli 0.143.0\\n'\n",
    );
    write_executable_script(
        &native_bin_dir.join("claude"),
        "#!/bin/sh\nprintf '2.1.179 (Claude Code)\\n'\n",
    );

    let _home = EnvGuard::set_os("HOME", &home_dir);
    let _path = EnvGuard::set_os("PATH", OsStr::new(""));
    let _nvm_bin = EnvGuard::unset("NVM_BIN");
    let _volta_home = EnvGuard::unset("VOLTA_HOME");
    let state = AppState::new_test();
    let codex_settings = codex_settings(AgentProviderCliManagementMode::RxManaged);
    let claude_settings = claude_settings(AgentProviderCliManagementMode::RxManaged);
    state
        .agent_provider_settings_repo
        .upsert(&codex_settings)
        .await
        .unwrap();
    state
        .agent_provider_settings_repo
        .upsert(&claude_settings)
        .await
        .unwrap();

    let codex_status = managed_provider_cli_status_for_settings(&state, codex_settings, false)
        .await
        .unwrap();
    assert_eq!(codex_status.provider, "codex");
    assert!(codex_status.installed);
    assert_eq!(codex_status.current_version.as_deref(), Some("0.143.0"));
    assert_eq!(codex_status.latest_version, None);
    assert_eq!(codex_status.action, "none");

    let claude_status = managed_provider_cli_status_for_settings(&state, claude_settings, false)
        .await
        .unwrap();
    assert_eq!(claude_status.provider, "claude");
    assert!(claude_status.installed);
    assert_eq!(claude_status.current_version.as_deref(), Some("2.1.179"));
    assert_eq!(claude_status.latest_version, None);
    assert_eq!(claude_status.action, "none");

    let statuses = read_managed_provider_cli_statuses(&state, false)
        .await
        .expect("provider CLI statuses");
    assert_eq!(statuses.providers.len(), 2);
    assert!(statuses
        .providers
        .iter()
        .any(|status| status.provider == "codex" && status.installed));
    assert!(statuses
        .providers
        .iter()
        .any(|status| status.provider == "claude" && status.installed));
}

#[cfg(unix)]
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn provider_observations_report_missing_and_probe_errors() {
    let _lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("test env mutex");
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let home_dir = temp_dir.path().join("home");
    let path_bin_dir = temp_dir.path().join("path-bin");
    let native_bin_dir = home_dir.join(".local").join("bin");
    let _managed_cli_dir =
        crate::utils::runtime_log_paths::override_managed_provider_cli_dir_for_tests(
            temp_dir.path().join("managed-cli"),
        );
    let managed_codex_path = crate::utils::runtime_log_paths::managed_codex_binary_path();
    let _managed_codex_cleanup = FileCleanup::new(managed_codex_path.clone());

    let _home = EnvGuard::set_os("HOME", &home_dir);
    let _path = EnvGuard::set_os("PATH", &path_bin_dir);
    let _nvm_bin = EnvGuard::unset("NVM_BIN");
    let _volta_home = EnvGuard::unset("VOLTA_HOME");

    let missing_codex = managed_codex_observation(false).await;
    assert!(missing_codex.supported);
    assert!(!missing_codex.installed);
    assert_eq!(
        missing_codex.binary_path.as_deref(),
        Some(managed_codex_path.as_path())
    );
    assert_eq!(missing_codex.current_version, None);
    assert_eq!(missing_codex.error, None);

    let missing_claude = managed_claude_observation(false).await;
    assert!(missing_claude.supported);
    assert!(!missing_claude.installed);
    assert_eq!(
        missing_claude.binary_path.as_deref(),
        Some(native_bin_dir.join("claude").as_path())
    );
    assert_eq!(missing_claude.current_version, None);
    assert_eq!(missing_claude.error, None);

    write_executable_script(
        &managed_codex_path,
        "#!/bin/sh\nprintf 'managed codex probe failed\\n' >&2\nexit 9\n",
    );
    write_executable_script(
        &path_bin_dir.join("codex"),
        "#!/bin/sh\nprintf 'user codex probe failed\\n' >&2\nexit 9\n",
    );
    write_executable_script(
        &native_bin_dir.join("claude"),
        "#!/bin/sh\nprintf 'managed claude probe failed\\n' >&2\nexit 9\n",
    );
    write_executable_script(
        &path_bin_dir.join("claude"),
        "#!/bin/sh\nprintf 'user claude probe failed\\n' >&2\nexit 9\n",
    );

    let managed_codex = managed_codex_observation(false).await;
    assert!(managed_codex.installed);
    assert_eq!(managed_codex.current_version, None);
    assert!(managed_codex
        .error
        .as_deref()
        .unwrap_or_default()
        .contains("managed codex probe failed"));

    let user_codex = user_managed_codex_observation_from_resolved_cli(
        ResolvedCodexCli {
            path: path_bin_dir.join("codex"),
            capabilities: CodexCliCapabilities {
                version: None,
                supports_exec_subcommand: true,
                supports_json_output: true,
                supports_model_flag: true,
                supports_config_override: true,
                supports_sandbox_flag: true,
                supports_add_dir: true,
                supports_search_flag: true,
                supports_resume_subcommand: true,
                supports_mcp_subcommand: true,
                supports_fast_mode_feature: false,
                fast_mode_supported_models: Vec::new(),
                supported_model_aliases: vec!["gpt-5.5".to_string()],
                supported_efforts: vec![
                    "low".to_string(),
                    "medium".to_string(),
                    "high".to_string(),
                    "xhigh".to_string(),
                ],
                model_supported_efforts: std::collections::BTreeMap::new(),
                ultra_supported_models: Vec::new(),
            },
        },
        false,
    )
    .await;
    assert!(user_codex.installed);
    assert_eq!(user_codex.current_version, None);
    assert!(user_codex
        .error
        .as_deref()
        .unwrap_or_default()
        .contains("user codex probe failed"));

    let managed_claude = managed_claude_observation(false).await;
    assert!(managed_claude.installed);
    assert_eq!(managed_claude.current_version, None);
    assert!(managed_claude
        .error
        .as_deref()
        .unwrap_or_default()
        .contains("managed claude probe failed"));

    let user_claude = user_managed_claude_observation(false).await;
    assert!(user_claude.installed);
    assert_eq!(user_claude.current_version, None);
    assert!(user_claude
        .error
        .as_deref()
        .unwrap_or_default()
        .contains("user claude probe failed"));
}

#[tokio::test]
async fn active_runtime_detection_matches_provider_from_agent_run() {
    let state = state_with_active_run(AgentHarnessKind::Codex).await;

    assert!(
        managed_provider_has_active_runtime(&state, AgentHarnessKind::Codex)
            .await
            .unwrap()
    );
    assert!(
        !managed_provider_has_active_runtime(&state, AgentHarnessKind::Claude)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn active_runtime_detection_falls_back_to_conversation_provider() {
    let state = AppState::new_test();
    let mut conversation = ChatConversation::new_project(ProjectId::new());
    conversation.provider_harness = Some(AgentHarnessKind::Claude);
    let conversation_id = conversation.id;
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();
    state
        .running_agent_registry
        .register(
            RunningAgentKey::new("project", "conversation-backed"),
            0,
            conversation_id.as_str(),
            "missing-agent-run".to_string(),
            None,
            None,
        )
        .await;

    assert!(
        managed_provider_has_active_runtime(&state, AgentHarnessKind::Claude)
            .await
            .unwrap()
    );
    assert!(
        !managed_provider_has_active_runtime(&state, AgentHarnessKind::Codex)
            .await
            .unwrap()
    );
}

#[cfg(unix)]
#[tokio::test]
async fn active_runtime_detection_matches_interactive_process_metadata() {
    let state = AppState::new_test();
    let key = InteractiveProcessKey::new("conversation", "metadata-backed");
    let mut child = Command::new("/bin/cat")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn cat process");
    let stdin = child.stdin.take().expect("cat stdin");
    state
        .interactive_process_registry
        .register_with_metadata(
            key.clone(),
            stdin,
            InteractiveProcessMetadata {
                agent_run_id: None,
                harness: Some(AgentHarnessKind::Codex),
                provider_session_id: Some("thread-123".to_string()),
                persona_id: None,
                persona_content_hash: None,
                agent_name: None,
                agent_profile: None,
            },
        )
        .await;

    assert!(
        managed_provider_has_active_runtime(&state, AgentHarnessKind::Codex)
            .await
            .unwrap()
    );
    assert!(
        !managed_provider_has_active_runtime(&state, AgentHarnessKind::Claude)
            .await
            .unwrap()
    );

    let _removed = state.interactive_process_registry.remove(&key).await;
    let _ = tokio::time::timeout(std::time::Duration::from_secs(1), child.wait()).await;
}

#[tokio::test]
async fn install_or_update_rejects_active_managed_provider() {
    let state = state_with_active_run(AgentHarnessKind::Codex).await;
    let settings = codex_settings(AgentProviderCliManagementMode::RxManaged);
    state
        .agent_provider_settings_repo
        .upsert(&settings)
        .await
        .unwrap();

    let error = install_or_update_managed_provider_cli_inner(AgentHarnessKind::Codex, &state)
        .await
        .expect_err("active Codex runtime should block managed updates");

    assert!(error.contains("currently in use"));
}

#[tokio::test]
async fn install_or_update_rejects_user_managed_provider_before_installer() {
    let state = AppState::new_test();
    state
        .agent_provider_settings_repo
        .upsert(&codex_settings(AgentProviderCliManagementMode::UserManaged))
        .await
        .unwrap();

    let error = install_or_update_managed_provider_cli_inner(AgentHarnessKind::Codex, &state)
        .await
        .expect_err("user-managed provider should not invoke installer");

    assert!(error.contains("user-managed"));
    assert!(error.contains("Enable RX-managed installs"));
}

#[tokio::test]
async fn install_or_update_rejects_active_managed_claude_provider() {
    let state = state_with_active_run(AgentHarnessKind::Claude).await;
    let settings = claude_settings(AgentProviderCliManagementMode::RxManaged);
    state
        .agent_provider_settings_repo
        .upsert(&settings)
        .await
        .unwrap();

    let error = install_or_update_managed_provider_cli_inner(AgentHarnessKind::Claude, &state)
        .await
        .expect_err("active Claude runtime should block managed updates");

    assert!(error.contains("currently in use"));
}

#[cfg(unix)]
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn managed_claude_install_or_update_uses_existing_native_cli_update() {
    let _lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("test env mutex");
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let home_dir = temp_dir.path().join("home");
    let native_path = home_dir.join(".local").join("bin").join("claude");
    write_executable_script(
        &native_path,
        "#!/bin/sh\ncase \"$1\" in\n  update)\n    printf 'updated native claude\\n'\n    printf 'native update warning\\n' >&2\n    ;;\n  --version)\n    printf '2.1.180 (Claude Code)\\n'\n    ;;\n  *)\n    printf 'unexpected arg: %s\\n' \"$1\" >&2\n    exit 2\n    ;;\nesac\n",
    );

    let _home = EnvGuard::set_os("HOME", &home_dir);
    let _path = EnvGuard::set_os("PATH", OsStr::new(""));
    let _nvm_bin = EnvGuard::unset("NVM_BIN");
    let _volta_home = EnvGuard::unset("VOLTA_HOME");

    let output = run_managed_claude_install_or_update()
        .await
        .expect("existing native Claude update");
    assert_eq!(output.stdout.as_deref(), Some("updated native claude"));
    assert_eq!(output.stderr.as_deref(), Some("native update warning"));

    let observation = managed_claude_observation(false).await;
    assert!(observation.installed);
    assert_eq!(
        observation.binary_path.as_deref(),
        Some(native_path.as_path())
    );
    assert_eq!(observation.current_version.as_deref(), Some("2.1.180"));
}

#[cfg(unix)]
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn managed_codex_installer_runs_fake_installer_with_rx_owned_env() {
    let _lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("test env mutex");
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let fake_bin_dir = temp_dir.path().join("fake-bin");
    let _managed_cli_dir =
        crate::utils::runtime_log_paths::override_managed_provider_cli_dir_for_tests(
            temp_dir.path().join("managed-cli"),
        );
    let managed_codex_path = crate::utils::runtime_log_paths::managed_codex_binary_path();
    let _managed_codex_cleanup = FileCleanup::new(managed_codex_path.clone());
    write_executable_script(
        &fake_bin_dir.join("curl"),
        "#!/bin/sh\ncat <<'INSTALLER'\ncat > \"$CODEX_INSTALL_DIR/codex\" <<'BIN'\n#!/bin/sh\nprintf 'codex-cli 0.144.0\\n'\nBIN\nchmod +x \"$CODEX_INSTALL_DIR/codex\"\nprintf 'codex installer complete\\n'\nINSTALLER\n",
    );

    let _path = EnvGuard::set_os("PATH", &fake_bin_dir);
    let _nvm_bin = EnvGuard::unset("NVM_BIN");
    let _volta_home = EnvGuard::unset("VOLTA_HOME");
    let _login_shell_env =
        EnvGuard::set_os(crate::infrastructure::login_shell_env::DISABLE_ENV_VAR, "1");

    let output = run_managed_codex_installer()
        .await
        .expect("fake Codex installer");
    assert_eq!(output.stdout.as_deref(), Some("codex installer complete"));
    assert_eq!(output.stderr, None);
    assert!(managed_codex_path.is_file());
    assert_eq!(
        probe_cli_version(&managed_codex_path).await.unwrap(),
        "codex-cli 0.144.0"
    );
}

#[cfg(unix)]
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn managed_claude_installer_runs_fake_native_installer() {
    let _lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("test env mutex");
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let home_dir = temp_dir.path().join("home");
    let fake_bin_dir = temp_dir.path().join("fake-bin");
    let native_path = home_dir.join(".local").join("bin").join("claude");
    write_executable_script(
        &fake_bin_dir.join("curl"),
        "#!/bin/sh\ncat <<'INSTALLER'\nmkdir -p \"$HOME/.local/bin\"\ncat > \"$HOME/.local/bin/claude\" <<'BIN'\n#!/bin/sh\nprintf '2.1.181 (Claude Code)\\n'\nBIN\nchmod +x \"$HOME/.local/bin/claude\"\nprintf 'claude installer complete\\n'\nINSTALLER\n",
    );

    let _home = EnvGuard::set_os("HOME", &home_dir);
    let _path = EnvGuard::set_os("PATH", &fake_bin_dir);
    let _nvm_bin = EnvGuard::unset("NVM_BIN");
    let _volta_home = EnvGuard::unset("VOLTA_HOME");
    let _login_shell_env =
        EnvGuard::set_os(crate::infrastructure::login_shell_env::DISABLE_ENV_VAR, "1");

    let output = run_managed_claude_installer()
        .await
        .expect("fake native Claude installer");
    assert_eq!(output.stdout.as_deref(), Some("claude installer complete"));
    assert_eq!(output.stderr, None);
    assert!(native_path.is_file());

    let observation = managed_claude_observation(false).await;
    assert_eq!(
        observation.binary_path.as_deref(),
        Some(native_path.as_path())
    );
    assert_eq!(observation.current_version.as_deref(), Some("2.1.181"));
}

#[test]
fn parses_codex_version_outputs() {
    assert_eq!(
        parse_codex_version("codex-cli 0.137.0\n").as_deref(),
        Some("0.137.0")
    );
    assert_eq!(
        parse_codex_version("codex 0.138.1").as_deref(),
        Some("0.138.1")
    );
    assert_eq!(parse_codex_version("weird").as_deref(), None);
}

#[test]
fn normalizes_codex_release_tags() {
    assert_eq!(
        normalize_codex_release_tag("rust-v0.137.0").as_deref(),
        Some("0.137.0")
    );
    assert_eq!(
        normalize_codex_release_tag("v0.138.0").as_deref(),
        Some("0.138.0")
    );
    assert_eq!(
        normalize_codex_release_tag("0.139.0").as_deref(),
        Some("0.139.0")
    );
    assert_eq!(normalize_codex_release_tag(" ").as_deref(), None);
}

#[tokio::test]
async fn latest_release_fetch_rejects_invalid_release_url_before_request() {
    let error = fetch_latest_github_release_version("not a valid uri", "test release version")
        .await
        .expect_err("invalid release URL");

    assert!(error.contains("Invalid GitHub release URL"));
}

#[test]
fn compares_version_number_parts() {
    assert_eq!(
        compare_version_strings("0.136.0", "0.137.0"),
        Some(std::cmp::Ordering::Less)
    );
    assert_eq!(
        compare_version_strings("0.137.0", "0.137.0"),
        Some(std::cmp::Ordering::Equal)
    );
    assert_eq!(
        compare_version_strings("0.138.0", "0.137.0"),
        Some(std::cmp::Ordering::Greater)
    );
    assert_eq!(compare_version_strings("unknown", "0.137.0"), None);
}

#[test]
fn managed_codex_install_plan_uses_rx_owned_dirs_and_prepended_path() {
    let plan = managed_codex_install_plan();
    let first_path = std::env::split_paths(&plan.path_env)
        .next()
        .expect("first PATH entry");

    assert_eq!(
        plan.command,
        "curl -fsSL https://chatgpt.com/codex/install.sh | sh"
    );
    assert_eq!(first_path, managed_codex_bin_dir());
    assert!(plan.home_dir.starts_with(plan.bin_dir.parent().unwrap()));
    assert!(plan
        .installer_home_dir
        .starts_with(plan.bin_dir.parent().unwrap()));
    assert!(plan.binary_path.starts_with(&plan.bin_dir));
}

#[test]
fn ensure_managed_codex_dirs_creates_all_plan_directories() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let plan = ManagedCodexInstallPlan {
        shell_path: PathBuf::from("/bin/sh"),
        command: "true",
        bin_dir: temp_dir.path().join("bin"),
        home_dir: temp_dir.path().join("home"),
        installer_home_dir: temp_dir.path().join("installer-home"),
        binary_path: temp_dir.path().join("bin").join("codex"),
        path_env: OsString::from("/bin"),
    };

    ensure_managed_codex_dirs(&plan).expect("managed Codex directories");

    assert!(plan.bin_dir.is_dir());
    assert!(plan.home_dir.is_dir());
    assert!(plan.installer_home_dir.is_dir());
}

#[test]
fn managed_claude_install_plan_uses_native_installer_and_home_local_path() {
    let _lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("test env mutex");
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let _home = EnvGuard::set_os("HOME", temp_dir.path());

    let plan = managed_claude_install_plan().expect("managed Claude install plan");
    let first_path = std::env::split_paths(&plan.path_env)
        .next()
        .expect("first PATH entry");
    let expected_bin_dir = temp_dir.path().join(".local").join("bin");

    assert_eq!(
        plan.command,
        "curl -fsSL https://claude.ai/install.sh | bash -s latest"
    );
    assert_eq!(plan.home_dir, temp_dir.path());
    assert_eq!(first_path, expected_bin_dir);
    assert_eq!(
        plan.binary_path,
        expected_bin_dir.join(if cfg!(windows) {
            "claude.exe"
        } else {
            "claude"
        })
    );
}

#[test]
fn path_with_prepended_dir_preserves_existing_path_order() {
    let path = path_with_prepended_dir(&PathBuf::from("/managed/bin"), OsStr::new("/usr/bin:/bin"));
    let entries = std::env::split_paths(&path).collect::<Vec<_>>();

    assert_eq!(entries[0], PathBuf::from("/managed/bin"));
    assert_eq!(entries[1], PathBuf::from("/usr/bin"));
    assert_eq!(entries[2], PathBuf::from("/bin"));
}

#[test]
fn truncate_process_output_omits_empty_and_marks_truncated_text() {
    assert_eq!(truncate_process_output(" \n\t"), None);
    assert_eq!(truncate_process_output("  done\n").as_deref(), Some("done"));

    let long_output = "x".repeat(4_001);
    let truncated = truncate_process_output(&long_output).expect("truncated output");
    assert_eq!(truncated.chars().count(), 4_004);
    assert!(truncated.ends_with("\n..."));
}

#[cfg(unix)]
#[tokio::test]
async fn managed_claude_command_captures_success_and_failure_output() {
    let mut success = Command::new("/bin/sh");
    success
        .arg("-c")
        .arg("printf ' installed\\n'; printf ' warnings\\n' >&2");
    let output = run_managed_claude_command(success, "test Claude command")
        .await
        .expect("successful command output");
    assert_eq!(output.stdout.as_deref(), Some("installed"));
    assert_eq!(output.stderr.as_deref(), Some("warnings"));

    let mut failure = Command::new("/bin/sh");
    failure.arg("-c").arg("printf 'nope\\n' >&2; exit 7");
    let error = run_managed_claude_command(failure, "test Claude command")
        .await
        .expect_err("failed command should include stderr");
    assert!(error.contains("test Claude command failed"));
    assert!(error.contains("nope"));
}

#[cfg(unix)]
#[tokio::test]
async fn probe_cli_version_reads_stdout_and_reports_stderr_failures() {
    use std::os::unix::fs::PermissionsExt;

    let temp_dir = tempfile::tempdir().expect("temp dir");
    let success_path = temp_dir.path().join("codex-success");
    std::fs::write(&success_path, "#!/bin/sh\nprintf 'codex-cli 0.140.0\\n'\n")
        .expect("write success probe");
    std::fs::set_permissions(&success_path, std::fs::Permissions::from_mode(0o755))
        .expect("chmod success probe");

    let version = probe_cli_version(&success_path)
        .await
        .expect("version probe output");
    assert_eq!(version, "codex-cli 0.140.0");

    let failure_path = temp_dir.path().join("codex-failure");
    std::fs::write(
        &failure_path,
        "#!/bin/sh\nprintf 'bad probe\\n' >&2\nexit 9\n",
    )
    .expect("write failure probe");
    std::fs::set_permissions(&failure_path, std::fs::Permissions::from_mode(0o755))
        .expect("chmod failure probe");

    let error = probe_cli_version(&failure_path)
        .await
        .expect_err("failed probe");
    assert!(error.contains("Failed to check"));
    assert!(error.contains("bad probe"));
}

#[cfg(unix)]
fn write_executable_script(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;

    let parent = path.parent().expect("script parent");
    std::fs::create_dir_all(parent).expect("script parent directory");
    std::fs::write(path, body).expect("write executable script");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .expect("chmod executable script");
}

#[cfg(unix)]
fn write_compatible_codex_script(path: &Path, version: &str) {
    let script = format!(
        r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  printf 'codex-cli {version}\n'
elif [ "$1" = "--help" ]; then
  printf '%s\n' 'Codex CLI' 'Commands:' '  exec' '  resume' '  mcp' 'Options:' '  -c, --config <key=value>' '  -m, --model <MODEL>' '  -s, --sandbox <SANDBOX>' '      --search' '      --add-dir <DIR>'
elif [ "$1" = "exec" ] && [ "$2" = "--help" ]; then
  printf '%s\n' 'Run Codex non-interactively' 'Options:' '  -c, --config <key=value>' '  -m, --model <MODEL>' '  -s, --sandbox <SANDBOX>' '      --add-dir <DIR>' '      --json'
else
  exit 64
fi
"#
    );
    write_executable_script(path, &script);
}

#[cfg(unix)]
struct FileCleanup {
    path: PathBuf,
}

#[cfg(unix)]
impl FileCleanup {
    fn new(path: PathBuf) -> Self {
        let _ = std::fs::remove_file(&path);
        Self { path }
    }
}

#[cfg(unix)]
impl Drop for FileCleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

struct EnvGuard {
    key: &'static str,
    original: Option<OsString>,
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

async fn state_with_active_run(provider: AgentHarnessKind) -> AppState {
    let state = AppState::new_test();
    let conversation_id = ChatConversationId::new();
    let mut run = AgentRun::new(conversation_id);
    run.harness = Some(provider);
    let run_id = run.id;
    state.agent_run_repo.create(run).await.unwrap();
    state
        .running_agent_registry
        .register(
            RunningAgentKey::new("project", provider.to_string()),
            0,
            conversation_id.as_str(),
            run_id.as_str(),
            None,
            None,
        )
        .await;
    state
}
