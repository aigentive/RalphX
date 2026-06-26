use std::path::Path;

use crate::application::provider_env_file::*;
use crate::domain::agents::{AgentHarnessKind, AgentProviderSettings};

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
RALPHX_PROJECT_ID=spoofed
ANTHROPIC_MODEL=wrong
ANTHROPIC_BASE_URL=https://anthropic.example
"#,
    )
    .expect("parse env file");

    assert!(!values.contains_key("PATH"));
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
