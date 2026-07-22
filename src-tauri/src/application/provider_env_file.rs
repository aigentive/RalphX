use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::domain::agents::{AgentHarnessKind, AgentProviderSettings};
use crate::domain::repositories::AgentProviderSettingsRepository;
use crate::utils::path_safety::{
    checked_is_file, checked_read_to_string, validate_absolute_non_root_path,
};

const PROTECTED_ENV_KEYS: &[&str] = &[
    "PATH",
    "RUSTC",
    "RUSTUP_TOOLCHAIN",
    "TAURI_API_URL",
    "DEBUG",
    "CLAUDECODE",
    "CLAUDE_PLUGIN_ROOT",
    "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC",
    "CLAUDE_CODE_ENABLE_TASKS",
    "CLAUDE_CODE_SUBAGENT_MODEL",
    "ANTHROPIC_MODEL",
    "CLAUDE_MODEL",
    "OPENAI_MODEL",
    "CODEX_MODEL",
];

pub(crate) fn validate_provider_custom_env_file_settings(
    settings: &AgentProviderSettings,
) -> Result<Option<PathBuf>, String> {
    if !settings.custom_env_file_enabled {
        return Ok(None);
    }

    validate_custom_env_file_path(settings).map(Some)
}

pub(crate) fn load_provider_custom_env_file(
    settings: &AgentProviderSettings,
) -> Result<HashMap<String, String>, String> {
    let Some(path) = validate_provider_custom_env_file_settings(settings)? else {
        return Ok(HashMap::new());
    };

    let context = custom_env_file_context(settings.provider);
    let contents = checked_read_to_string(&path, &context).map_err(|err| err.to_string())?;
    parse_provider_env_file_contents(settings.provider, &contents)
}

pub(crate) async fn load_provider_custom_env_file_for_harness(
    provider_repo: Option<&Arc<dyn AgentProviderSettingsRepository>>,
    harness: AgentHarnessKind,
) -> Result<HashMap<String, String>, String> {
    let Some(provider_repo) = provider_repo else {
        return Ok(HashMap::new());
    };
    let Some(settings) = provider_repo
        .get(harness)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Ok(HashMap::new());
    };

    load_provider_custom_env_file(&settings)
}

pub(crate) fn parse_provider_env_file_contents(
    provider: AgentHarnessKind,
    contents: &str,
) -> Result<HashMap<String, String>, String> {
    let mut values = HashMap::new();
    for (index, raw_line) in contents.lines().enumerate() {
        let line_number = index + 1;
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with("export ") {
            return Err(format!(
                "Custom {provider} env file line {line_number} uses unsupported export syntax"
            ));
        }

        let (key, value) = line.split_once('=').ok_or_else(|| {
            format!("Custom {provider} env file line {line_number} must use KEY=value syntax")
        })?;
        let key = key.trim();
        if !is_valid_env_key(key) {
            return Err(format!(
                "Custom {provider} env file line {line_number} has an invalid key"
            ));
        }
        if is_protected_env_key(key) {
            continue;
        }

        values.insert(
            key.to_string(),
            strip_surrounding_double_quotes(value.trim()),
        );
    }

    Ok(values)
}

fn validate_custom_env_file_path(settings: &AgentProviderSettings) -> Result<PathBuf, String> {
    let raw_path = settings
        .custom_env_file_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| custom_env_file_path_required_error(settings.provider))?;
    let path = Path::new(raw_path);
    let context = custom_env_file_context(settings.provider);
    let safe_path =
        validate_absolute_non_root_path(path, &context).map_err(|err| err.to_string())?;
    if !checked_is_file(&safe_path, &context).map_err(|err| err.to_string())? {
        return Err(format!(
            "Custom {} env file path is not a readable regular file: {}",
            settings.provider,
            safe_path.display()
        ));
    }
    Ok(safe_path)
}

fn custom_env_file_context(provider: AgentHarnessKind) -> String {
    format!("Custom {provider} env file")
}

fn custom_env_file_path_required_error(provider: AgentHarnessKind) -> String {
    format!("Custom {provider} env file path is required before enabling custom env file mode")
}

fn is_valid_env_key(key: &str) -> bool {
    let mut chars = key.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first == '_' || first.is_ascii_alphabetic()) {
        return false;
    }
    chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn is_protected_env_key(key: &str) -> bool {
    key.starts_with("RALPHX_") || PROTECTED_ENV_KEYS.contains(&key)
}

fn strip_surrounding_double_quotes(value: &str) -> String {
    if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        value[1..value.len() - 1].to_string()
    } else {
        value.to_string()
    }
}
