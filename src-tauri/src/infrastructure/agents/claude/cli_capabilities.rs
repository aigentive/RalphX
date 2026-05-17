use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use std::sync::{Mutex, OnceLock};

use crate::domain::agents::LogicalEffort;

const CLAUDE_XHIGH_MIN_VERSION: (u64, u64, u64) = (2, 1, 111);
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
    pub supported_efforts: Vec<LogicalEffort>,
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

    ClaudeCliCapabilities {
        version,
        supported_efforts,
    }
}

pub fn probe_claude_cli(cli_path: &Path) -> Result<ClaudeCliCapabilities, String> {
    let version_output = run_claude_command(cli_path, &["--version"])?;
    let help_output = run_claude_command(cli_path, &["--help"])?;
    Ok(parse_claude_cli_capabilities(
        &help_output,
        Some(&version_output),
    ))
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

fn run_claude_command(cli_path: &Path, args: &[&str]) -> Result<String, String> {
    let mut command = StdCommand::new(cli_path);
    command.args(args);
    command.env(
        "PATH",
        crate::infrastructure::tool_paths::agent_subprocess_env_path(),
    );
    crate::infrastructure::tool_paths::prepend_resolved_node_bin_to_path(&mut command);
    let output = command
        .output()
        .map_err(|error| format!("Failed to run {} {:?}: {}", cli_path.display(), args, error))?;

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

#[cfg(test)]
#[path = "cli_capabilities_tests.rs"]
mod tests;
