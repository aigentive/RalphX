use std::ffi::OsStr;
use std::path::PathBuf;

use crate::application::AppState;
use crate::domain::agents::{
    AgentHarnessKind, AgentProviderCliManagementMode, AgentProviderSettings,
};
use crate::domain::entities::{AgentRun, ChatConversation, ChatConversationId, ProjectId};
use crate::domain::services::RunningAgentKey;
use tokio::process::Command;

use super::{
    compare_version_strings, install_or_update_managed_provider_cli_inner,
    managed_claude_install_plan, managed_cli_status_response, managed_codex_bin_dir,
    managed_codex_install_plan, managed_provider_has_active_runtime, normalize_codex_release_tag,
    parse_codex_version, parse_provider, path_with_prepended_dir, probe_cli_version,
    run_managed_claude_command, settings_for_provider, truncate_process_output,
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
