use std::ffi::OsStr;
use std::path::PathBuf;

use crate::application::AppState;
use crate::domain::agents::{
    AgentHarnessKind, AgentProviderCliManagementMode, AgentProviderSettings,
};
use crate::domain::entities::{AgentRun, ChatConversationId};
use crate::domain::services::RunningAgentKey;

use super::{
    compare_version_strings, install_or_update_managed_provider_cli_inner,
    managed_cli_status_response, managed_codex_bin_dir, managed_codex_install_plan,
    managed_provider_has_active_runtime, normalize_codex_release_tag, parse_codex_version,
    path_with_prepended_dir, unsupported_claude_observation, ManagedProviderCliObservation,
};

fn codex_settings(mode: AgentProviderCliManagementMode) -> AgentProviderSettings {
    let mut settings = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Codex);
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
fn codex_user_managed_policy_suppresses_managed_actions() {
    let status = managed_cli_status_response(
        codex_settings(AgentProviderCliManagementMode::UserManaged),
        codex_observation(true, Some("0.136.0"), Some("0.137.0")),
        false,
    );

    assert_eq!(status.action, "none");
    assert!(status.update_available);
    assert!(status.status.contains("user-managed"));
}

#[test]
fn claude_managed_installs_are_reported_as_unsupported() {
    let settings = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Claude);
    let status = managed_cli_status_response(settings, unsupported_claude_observation(), false);

    assert_eq!(status.provider, "claude");
    assert!(!status.supported);
    assert_eq!(status.action, "unsupported");
    assert!(status.status.contains("unavailable"));
    assert!(status.error.unwrap().contains("install prefix"));
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
fn path_with_prepended_dir_preserves_existing_path_order() {
    let path = path_with_prepended_dir(&PathBuf::from("/managed/bin"), OsStr::new("/usr/bin:/bin"));
    let entries = std::env::split_paths(&path).collect::<Vec<_>>();

    assert_eq!(entries[0], PathBuf::from("/managed/bin"));
    assert_eq!(entries[1], PathBuf::from("/usr/bin"));
    assert_eq!(entries[2], PathBuf::from("/bin"));
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
