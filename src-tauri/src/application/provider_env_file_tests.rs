use std::path::Path;
use std::sync::Arc;

use crate::application::provider_env_file::*;
use crate::domain::agents::{AgentHarnessKind, AgentProviderSettings};
use crate::domain::repositories::AgentProviderSettingsRepository;
use crate::infrastructure::memory::MemoryAgentProviderSettingsRepository;

fn settings_for(path: &Path) -> AgentProviderSettings {
    let mut settings = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Codex);
    settings.custom_env_file_enabled = true;
    settings.custom_env_file_path = Some(path.to_string_lossy().into_owned());
    settings
}

#[test]
fn parser_accepts_supported_env_subset_and_last_value_wins() {
    let values = parse_provider_env_file_contents(
        AgentHarnessKind::Codex,
        r#"
# comment
ANTHROPIC_AUTH_TOKEN = "secret"
PROXY_URL=https://one.example?a=b
PROXY_URL=https://two.example
"#,
    )
    .expect("parse env file");

    assert_eq!(
        values.get("ANTHROPIC_AUTH_TOKEN").map(String::as_str),
        Some("secret")
    );
    assert_eq!(
        values.get("PROXY_URL").map(String::as_str),
        Some("https://two.example")
    );
}

#[test]
fn parser_skips_protected_runtime_and_model_keys() {
    let values = parse_provider_env_file_contents(
        AgentHarnessKind::Claude,
        r#"
PATH=/tmp/bin
RUSTC=/opt/homebrew/bin/rustc
RUSTUP_TOOLCHAIN=1.85.1
RALPHX_PROJECT_ID=spoofed
ANTHROPIC_MODEL=wrong
ANTHROPIC_BASE_URL=https://anthropic.example
"#,
    )
    .expect("parse env file");

    assert!(!values.contains_key("PATH"));
    assert!(!values.contains_key("RUSTC"));
    assert!(!values.contains_key("RUSTUP_TOOLCHAIN"));
    assert!(!values.contains_key("RALPHX_PROJECT_ID"));
    assert!(!values.contains_key("ANTHROPIC_MODEL"));
    assert_eq!(
        values.get("ANTHROPIC_BASE_URL").map(String::as_str),
        Some("https://anthropic.example")
    );
}

#[test]
fn parser_rejects_invalid_lines_without_value_leakage() {
    let error = parse_provider_env_file_contents(
        AgentHarnessKind::Codex,
        "TOKEN=secret\nnot a key=leaked-secret",
    )
    .expect_err("invalid key should fail");

    assert!(error.contains("line 2"));
    assert!(!error.contains("secret"));
    assert!(!error.contains("leaked"));
}

#[test]
fn parser_rejects_export_and_missing_assignment_syntax() {
    let export_error =
        parse_provider_env_file_contents(AgentHarnessKind::Claude, "export TOKEN=secret")
            .expect_err("export syntax should fail");
    assert!(export_error.contains("line 1"));
    assert!(export_error.contains("unsupported export syntax"));
    assert!(!export_error.contains("secret"));

    let assignment_error =
        parse_provider_env_file_contents(AgentHarnessKind::Codex, "TOKEN_WITHOUT_VALUE")
            .expect_err("missing assignment should fail");
    assert!(assignment_error.contains("line 1"));
    assert!(assignment_error.contains("KEY=value"));
}

#[test]
fn parser_rejects_empty_or_numeric_initial_keys() {
    let empty_key_error = parse_provider_env_file_contents(AgentHarnessKind::Codex, "=secret")
        .expect_err("empty key should fail");
    assert!(empty_key_error.contains("invalid key"));
    assert!(!empty_key_error.contains("secret"));

    let numeric_key_error =
        parse_provider_env_file_contents(AgentHarnessKind::Codex, "1TOKEN=secret")
            .expect_err("numeric-leading key should fail");
    assert!(numeric_key_error.contains("invalid key"));
    assert!(!numeric_key_error.contains("secret"));
}

#[test]
fn disabled_settings_do_not_require_a_path() {
    let settings = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Codex);

    let values = load_provider_custom_env_file(&settings).expect("disabled env file");

    assert!(values.is_empty());
}

#[test]
fn enabled_settings_require_absolute_regular_file() {
    let mut settings = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Codex);
    settings.custom_env_file_enabled = true;
    settings.custom_env_file_path = Some("relative.env".to_string());

    let error = load_provider_custom_env_file(&settings).expect_err("relative path should fail");

    assert!(error.contains("absolute"));
}

#[test]
fn enabled_settings_require_non_empty_path() {
    let mut settings = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Claude);
    settings.custom_env_file_enabled = true;
    settings.custom_env_file_path = Some("  ".to_string());

    let error = load_provider_custom_env_file(&settings).expect_err("blank path should fail");

    assert!(error.contains("path is required"));
    assert!(error.contains("claude"));
}

#[test]
fn loader_reads_file_contents() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let env_path = temp_dir.path().join("provider.env");
    std::fs::write(&env_path, "ANTHROPIC_AUTH_TOKEN=secret\n").expect("write env file");

    let values = load_provider_custom_env_file(&settings_for(&env_path)).expect("load env file");

    assert_eq!(
        values.get("ANTHROPIC_AUTH_TOKEN").map(String::as_str),
        Some("secret")
    );
}

#[tokio::test]
async fn harness_loader_returns_empty_without_repo_or_settings() {
    let values = load_provider_custom_env_file_for_harness(None, AgentHarnessKind::Codex)
        .await
        .expect("no provider repo");
    assert!(values.is_empty());

    let provider_repo: Arc<dyn AgentProviderSettingsRepository> =
        Arc::new(MemoryAgentProviderSettingsRepository::new());
    let values =
        load_provider_custom_env_file_for_harness(Some(&provider_repo), AgentHarnessKind::Codex)
            .await
            .expect("missing provider settings");
    assert!(values.is_empty());
}

#[tokio::test]
async fn harness_loader_reads_enabled_provider_settings() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let env_path = temp_dir.path().join("provider.env");
    std::fs::write(&env_path, "CUSTOM_PROVIDER_TOKEN=from-harness\n").expect("write env file");
    let provider_repo = Arc::new(MemoryAgentProviderSettingsRepository::new());
    provider_repo
        .upsert(&settings_for(&env_path))
        .await
        .expect("save settings");
    let provider_repo: Arc<dyn AgentProviderSettingsRepository> = provider_repo;

    let values =
        load_provider_custom_env_file_for_harness(Some(&provider_repo), AgentHarnessKind::Codex)
            .await
            .expect("load harness env file");

    assert_eq!(
        values.get("CUSTOM_PROVIDER_TOKEN").map(String::as_str),
        Some("from-harness")
    );
}
