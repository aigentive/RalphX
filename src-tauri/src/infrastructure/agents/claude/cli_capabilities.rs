use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command as StdCommand, Output};
use std::sync::{Mutex, OnceLock};

use crate::domain::agents::LogicalEffort;

pub const CLAUDE_FABLE_MODEL_ALIAS: &str = "fable";
pub const CLAUDE_FABLE_API_MODEL_ID: &str = "claude-fable-5";
pub const CLAUDE_FABLE_MIN_VERSION: (u64, u64, u64) = (2, 1, 170);
pub const CLAUDE_SONNET_4_6_API_MODEL_ID: &str = "claude-sonnet-4-6";
pub const CLAUDE_SONNET_5_API_MODEL_ID: &str = "claude-sonnet-5";
pub const CLAUDE_SONNET_4_6_MIN_VERSION: (u64, u64, u64) = (2, 1, 197);
pub const CLAUDE_SONNET_5_MIN_VERSION: (u64, u64, u64) = (2, 1, 197);
pub const CLAUDE_OPUS_4_7_API_MODEL_ID: &str = "claude-opus-4-7";
pub const CLAUDE_OPUS_4_7_MIN_VERSION: (u64, u64, u64) = (2, 1, 111);
pub const CLAUDE_OPUS_4_8_API_MODEL_ID: &str = "claude-opus-4-8";
pub const CLAUDE_OPUS_4_8_MIN_VERSION: (u64, u64, u64) = (2, 1, 154);
pub const CLAUDE_OPUS_5_API_MODEL_ID: &str = "claude-opus-5";
pub const CLAUDE_OPUS_5_MIN_VERSION: (u64, u64, u64) = (2, 1, 219);
pub(crate) const CLAUDE_THINKING_DISPLAY_ACCEPTANCE_MARKER: &str = "option '--thinking-display";
pub(crate) const CLAUDE_THINKING_DISPLAY_PROBE_VALUE: &str = "ralphx-capability-probe";

const CLAUDE_XHIGH_MIN_VERSION: (u64, u64, u64) = (2, 1, 111);
const CLAUDE_FABLE_MIN_VERSION_LABEL: &str = "2.1.170";
const CLAUDE_SONNET_4_6_MIN_VERSION_LABEL: &str = "2.1.197";
const CLAUDE_SONNET_5_MIN_VERSION_LABEL: &str = "2.1.197";
const CLAUDE_OPUS_4_7_MIN_VERSION_LABEL: &str = "2.1.111";
const CLAUDE_OPUS_4_8_MIN_VERSION_LABEL: &str = "2.1.154";
const CLAUDE_OPUS_5_MIN_VERSION_LABEL: &str = "2.1.219";
const BASE_CLAUDE_MODEL_ALIASES: [&str; 3] = ["sonnet", "opus", "haiku"];
const CLAUDE_EFFORT_ORDER: [LogicalEffort; 5] = [
    LogicalEffort::Low,
    LogicalEffort::Medium,
    LogicalEffort::High,
    LogicalEffort::XHigh,
    LogicalEffort::Max,
];
const LEGACY_CLAUDE_EFFORTS: [LogicalEffort; 4] = [
    LogicalEffort::Low,
    LogicalEffort::Medium,
    LogicalEffort::High,
    LogicalEffort::Max,
];

static CLAUDE_CLI_CAPABILITY_CACHE: OnceLock<
    Mutex<HashMap<PathBuf, Result<ClaudeCliCapabilities, String>>>,
> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeCliCapabilities {
    pub version: Option<String>,
    pub supported_model_aliases: Vec<String>,
    pub supported_efforts: Vec<LogicalEffort>,
    pub supports_include_partial_messages: bool,
    pub supports_thinking_display: bool,
}

impl ClaudeCliCapabilities {
    pub fn supports_effort(&self, effort: LogicalEffort) -> bool {
        self.supported_efforts.contains(&effort)
    }

    pub fn supported_effort_labels(&self) -> Vec<String> {
        self.supported_efforts
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    pub fn supports_model_alias(&self, model: &str) -> bool {
        let normalized = normalize_model_alias(model);
        self.supported_model_aliases
            .iter()
            .any(|alias| alias == &normalized)
    }

    pub fn supports_fable_model(&self) -> bool {
        self.supports_model_alias(CLAUDE_FABLE_MODEL_ALIAS)
    }

    pub fn supports_include_partial_messages(&self) -> bool {
        self.supports_include_partial_messages
    }

    pub fn supports_thinking_display(&self) -> bool {
        self.supports_thinking_display
    }
}

pub fn parse_claude_version(output: &str) -> Option<String> {
    output.split_whitespace().find_map(|token| {
        let candidate = token.trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '.');
        parse_semver_triplet(candidate).map(|_| candidate.to_string())
    })
}

pub fn parse_claude_cli_capabilities(
    help_output: &str,
    version_output: Option<&str>,
) -> ClaudeCliCapabilities {
    let version = version_output.and_then(parse_claude_version);
    let supported_efforts = parse_supported_efforts_from_help(help_output)
        .filter(|efforts| !efforts.is_empty())
        .unwrap_or_else(|| fallback_supported_efforts(version.as_deref()));
    let supported_model_aliases = fallback_supported_model_aliases(version.as_deref());

    ClaudeCliCapabilities {
        version,
        supported_model_aliases,
        supported_efforts,
        // This boolean flag cannot use the value-rejection acceptance probe below.
        supports_include_partial_messages: help_output.contains("--include-partial-messages"),
        supports_thinking_display: help_output.contains("--thinking-display"),
    }
}

pub fn probe_claude_cli(cli_path: &Path) -> Result<ClaudeCliCapabilities, String> {
    let version_output = run_claude_command(cli_path, &["--version"])?;
    let help_output = run_claude_command(cli_path, &["--help"])?;
    let mut capabilities = parse_claude_cli_capabilities(&help_output, Some(&version_output));
    if !capabilities.supports_thinking_display {
        capabilities.supports_thinking_display =
            probe_claude_cli_thinking_display_acceptance(cli_path);
    }
    Ok(capabilities)
}

pub fn probe_claude_cli_cached(cli_path: &Path) -> Result<ClaudeCliCapabilities, String> {
    let cache = CLAUDE_CLI_CAPABILITY_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut cached = cache.lock().unwrap();
    if let Some(result) = cached.get(cli_path) {
        return result.clone();
    }

    let result = probe_claude_cli(cli_path);
    cached.insert(cli_path.to_path_buf(), result.clone());
    result
}

pub fn clear_claude_cli_capability_cache() {
    if let Some(cache) = CLAUDE_CLI_CAPABILITY_CACHE.get() {
        cache.lock().unwrap().clear();
    }
}

pub fn claude_cli_supports_partial_messages(cli_path: &Path) -> bool {
    match probe_claude_cli_cached(cli_path) {
        Ok(capabilities) => capabilities.supports_include_partial_messages(),
        Err(error) => {
            tracing::warn!(
                cli_path = %cli_path.display(),
                %error,
                "Claude CLI partial-message capability probe failed; omitting optional flag"
            );
            false
        }
    }
}

pub fn claude_cli_supports_thinking_display(cli_path: &Path) -> bool {
    match probe_claude_cli_cached(cli_path) {
        Ok(capabilities) => capabilities.supports_thinking_display(),
        Err(error) => {
            tracing::warn!(
                cli_path = %cli_path.display(),
                %error,
                "Claude CLI thinking-display capability probe failed; omitting optional flag"
            );
            false
        }
    }
}

pub fn normalize_claude_effort_for_cli_path(cli_path: &Path, effort: &str) -> String {
    match probe_claude_cli_cached(cli_path) {
        Ok(capabilities) => normalize_claude_effort_for_capabilities(effort, &capabilities),
        Err(error) => {
            tracing::warn!(
                cli_path = %cli_path.display(),
                %error,
                "Claude CLI effort capability probe failed; using legacy effort fallback"
            );
            normalize_claude_effort_for_supported(effort, &LEGACY_CLAUDE_EFFORTS)
        }
    }
}

pub fn normalize_claude_effort_for_capabilities(
    effort: &str,
    capabilities: &ClaudeCliCapabilities,
) -> String {
    normalize_claude_effort_for_supported(effort, &capabilities.supported_efforts)
}

pub fn validate_claude_model_for_cli_path(cli_path: &Path, model: &str) -> Result<(), String> {
    let Some(requirement) = claude_model_version_requirement(model) else {
        return Ok(());
    };

    let capabilities = probe_claude_cli_cached(cli_path).map_err(|error| {
        format!(
            "Cannot verify Claude Code supports {} (requires Claude Code v{} or newer) before launching with --model {model:?}. Upgrade Claude Code before selecting --model {}: {error}",
            requirement.display_name, requirement.min_version_label, requirement.selection_hint
        )
    })?;
    if capabilities.supports_model_alias(requirement.required_alias) {
        return Ok(());
    }

    let installed_version = capabilities
        .version
        .as_deref()
        .map(|version| format!("Installed Claude Code version is {version}. "))
        .unwrap_or_default();
    Err(format!(
        "{} requires Claude Code v{} or newer. {installed_version}Upgrade Claude Code before selecting --model {}.",
        requirement.display_name, requirement.min_version_label, requirement.selection_hint
    ))
}

pub fn is_claude_fable_model(model: &str) -> bool {
    matches!(
        normalize_model_alias(model).as_str(),
        CLAUDE_FABLE_MODEL_ALIAS | CLAUDE_FABLE_API_MODEL_ID
    )
}

pub fn is_claude_sonnet_5_model(model: &str) -> bool {
    normalize_model_alias(model) == CLAUDE_SONNET_5_API_MODEL_ID
}

pub fn is_claude_sonnet_4_6_model(model: &str) -> bool {
    normalize_model_alias(model) == CLAUDE_SONNET_4_6_API_MODEL_ID
}

pub fn is_claude_opus_4_7_model(model: &str) -> bool {
    normalize_model_alias(model) == CLAUDE_OPUS_4_7_API_MODEL_ID
}

pub fn is_claude_opus_4_8_model(model: &str) -> bool {
    normalize_model_alias(model) == CLAUDE_OPUS_4_8_API_MODEL_ID
}

pub fn is_claude_opus_5_model(model: &str) -> bool {
    normalize_model_alias(model) == CLAUDE_OPUS_5_API_MODEL_ID
}

struct ClaudeModelVersionRequirement {
    required_alias: &'static str,
    display_name: &'static str,
    min_version_label: &'static str,
    selection_hint: &'static str,
}

fn claude_model_version_requirement(model: &str) -> Option<ClaudeModelVersionRequirement> {
    if is_claude_fable_model(model) {
        return Some(ClaudeModelVersionRequirement {
            required_alias: CLAUDE_FABLE_MODEL_ALIAS,
            display_name: "Claude Fable 5",
            min_version_label: CLAUDE_FABLE_MIN_VERSION_LABEL,
            selection_hint: CLAUDE_FABLE_MODEL_ALIAS,
        });
    }
    if is_claude_sonnet_5_model(model) {
        return Some(ClaudeModelVersionRequirement {
            required_alias: CLAUDE_SONNET_5_API_MODEL_ID,
            display_name: "Claude Sonnet 5",
            min_version_label: CLAUDE_SONNET_5_MIN_VERSION_LABEL,
            selection_hint: CLAUDE_SONNET_5_API_MODEL_ID,
        });
    }
    if is_claude_sonnet_4_6_model(model) {
        return Some(ClaudeModelVersionRequirement {
            required_alias: CLAUDE_SONNET_4_6_API_MODEL_ID,
            display_name: "Claude Sonnet 4.6",
            min_version_label: CLAUDE_SONNET_4_6_MIN_VERSION_LABEL,
            selection_hint: CLAUDE_SONNET_4_6_API_MODEL_ID,
        });
    }
    if is_claude_opus_4_7_model(model) {
        return Some(ClaudeModelVersionRequirement {
            required_alias: CLAUDE_OPUS_4_7_API_MODEL_ID,
            display_name: "Claude Opus 4.7",
            min_version_label: CLAUDE_OPUS_4_7_MIN_VERSION_LABEL,
            selection_hint: CLAUDE_OPUS_4_7_API_MODEL_ID,
        });
    }
    if is_claude_opus_4_8_model(model) {
        return Some(ClaudeModelVersionRequirement {
            required_alias: CLAUDE_OPUS_4_8_API_MODEL_ID,
            display_name: "Claude Opus 4.8",
            min_version_label: CLAUDE_OPUS_4_8_MIN_VERSION_LABEL,
            selection_hint: CLAUDE_OPUS_4_8_API_MODEL_ID,
        });
    }
    if is_claude_opus_5_model(model) {
        return Some(ClaudeModelVersionRequirement {
            required_alias: CLAUDE_OPUS_5_API_MODEL_ID,
            display_name: "Claude Opus 5",
            min_version_label: CLAUDE_OPUS_5_MIN_VERSION_LABEL,
            selection_hint: CLAUDE_OPUS_5_API_MODEL_ID,
        });
    }
    None
}

fn normalize_claude_effort_for_supported(effort: &str, supported: &[LogicalEffort]) -> String {
    let Ok(requested) = effort.parse::<LogicalEffort>() else {
        tracing::warn!(
            effort,
            "Invalid Claude effort requested; falling back to medium"
        );
        return LogicalEffort::Medium.to_string();
    };

    if supported.contains(&requested) {
        return requested.to_string();
    }

    let requested_rank = effort_rank(requested);
    CLAUDE_EFFORT_ORDER
        .iter()
        .copied()
        .rev()
        .find(|candidate| {
            supported.contains(candidate) && effort_rank(*candidate) <= requested_rank
        })
        .or_else(|| supported.first().copied())
        .unwrap_or(LogicalEffort::Medium)
        .to_string()
}

fn parse_supported_efforts_from_help(help_output: &str) -> Option<Vec<LogicalEffort>> {
    let effort_line = help_output.lines().find(|line| line.contains("--effort"))?;
    let efforts = CLAUDE_EFFORT_ORDER
        .iter()
        .copied()
        .filter(|effort| contains_label(effort_line, &effort.to_string()))
        .collect::<Vec<_>>();
    Some(efforts)
}

fn contains_label(line: &str, label: &str) -> bool {
    line.split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
        .any(|token| token == label)
}

fn fallback_supported_efforts(version: Option<&str>) -> Vec<LogicalEffort> {
    if version
        .and_then(parse_semver_triplet)
        .is_some_and(|version| version >= CLAUDE_XHIGH_MIN_VERSION)
    {
        CLAUDE_EFFORT_ORDER.to_vec()
    } else {
        LEGACY_CLAUDE_EFFORTS.to_vec()
    }
}

fn fallback_supported_model_aliases(version: Option<&str>) -> Vec<String> {
    let mut aliases = BASE_CLAUDE_MODEL_ALIASES
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    if version
        .and_then(parse_semver_triplet)
        .is_some_and(|version| version >= CLAUDE_OPUS_4_7_MIN_VERSION)
    {
        aliases.push(CLAUDE_OPUS_4_7_API_MODEL_ID.to_string());
    }
    if version
        .and_then(parse_semver_triplet)
        .is_some_and(|version| version >= CLAUDE_OPUS_4_8_MIN_VERSION)
    {
        aliases.push(CLAUDE_OPUS_4_8_API_MODEL_ID.to_string());
    }
    if version
        .and_then(parse_semver_triplet)
        .is_some_and(|version| version >= CLAUDE_FABLE_MIN_VERSION)
    {
        aliases.push(CLAUDE_FABLE_MODEL_ALIAS.to_string());
    }
    if version
        .and_then(parse_semver_triplet)
        .is_some_and(|version| version >= CLAUDE_SONNET_4_6_MIN_VERSION)
    {
        aliases.push(CLAUDE_SONNET_4_6_API_MODEL_ID.to_string());
    }
    if version
        .and_then(parse_semver_triplet)
        .is_some_and(|version| version >= CLAUDE_SONNET_5_MIN_VERSION)
    {
        aliases.push(CLAUDE_SONNET_5_API_MODEL_ID.to_string());
    }
    if version
        .and_then(parse_semver_triplet)
        .is_some_and(|version| version >= CLAUDE_OPUS_5_MIN_VERSION)
    {
        aliases.push(CLAUDE_OPUS_5_API_MODEL_ID.to_string());
    }
    aliases
}

fn normalize_model_alias(model: &str) -> String {
    model.trim().to_ascii_lowercase()
}

fn parse_semver_triplet(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    Some((major, minor, patch))
}

fn effort_rank(effort: LogicalEffort) -> usize {
    CLAUDE_EFFORT_ORDER
        .iter()
        .position(|candidate| *candidate == effort)
        .unwrap_or(usize::MAX)
}

/// Probes Claude CLI 2.1.220's value-rejection surface because exit status alone is invalid:
/// unknown flags with `--help` exit successfully, while a recognized `--thinking-display`
/// value rejects this bogus probe value with a distinct error.
///
/// The stderr check requires BOTH the option marker and the echoed probe value: a CLI that
/// rejects unknown options would emit `error: unknown option '--thinking-display'` (which
/// contains the marker) but never echoes the probe value, while genuine value rejection does.
fn probe_claude_cli_thinking_display_acceptance(cli_path: &Path) -> bool {
    match run_claude_command_output(
        cli_path,
        &[
            "--thinking-display",
            CLAUDE_THINKING_DISPLAY_PROBE_VALUE,
            "--help",
        ],
    ) {
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            !output.status.success()
                && stderr.contains(CLAUDE_THINKING_DISPLAY_ACCEPTANCE_MARKER)
                && stderr.contains(CLAUDE_THINKING_DISPLAY_PROBE_VALUE)
        }
        Err(error) => {
            tracing::debug!(
                cli_path = %cli_path.display(),
                %error,
                "Claude CLI thinking-display acceptance probe could not start; omitting optional flag"
            );
            false
        }
    }
}

fn run_claude_command(cli_path: &Path, args: &[&str]) -> Result<String, String> {
    let output = run_claude_command_output(cli_path, args)?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(format!(
            "Command {} {:?} exited with status {}: {}",
            cli_path.display(),
            args,
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

fn run_claude_command_output(cli_path: &Path, args: &[&str]) -> Result<Output, String> {
    let mut command = StdCommand::new(cli_path);
    command.args(args);
    command.env(
        "PATH",
        crate::infrastructure::tool_paths::agent_subprocess_env_path(),
    );
    crate::infrastructure::tool_paths::ensure_resolved_node_bin_in_path(&mut command);
    crate::infrastructure::subprocess_env_policy::github_cli_env_policy()
        .apply_to_std_command(&mut command);
    let output = command
        .output()
        .map_err(|error| format!("Failed to run {} {:?}: {}", cli_path.display(), args, error))?;
    Ok(output)
}

#[cfg(test)]
#[path = "cli_capabilities_tests.rs"]
mod tests;
