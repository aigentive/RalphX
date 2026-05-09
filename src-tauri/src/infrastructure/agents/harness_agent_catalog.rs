use serde::Deserialize;
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::infrastructure::agents::claude::canonical_short_agent_name;

const CANONICAL_AGENTS_DIR: &str = "agents";
const PROMPT_FILE_NAME: &str = "prompt.md";
const AGENT_FILE_NAME: &str = "agent.yaml";
const SHARED_PROMPT_DIR_NAME: &str = "shared";
const GENERATED_PLUGIN_RUNTIME_ENTRY_NAMES: &[&str] = &["ralphx-mcp-server", "ralphx-external-mcp"];
const PRIMARY_PLUGIN_DIR_COMPONENTS: &[&str] = &["plugins", "app"];
const LEGACY_PLUGIN_DIR_COMPONENTS: &[&str] = &["ralphx-plugin"];
const GENERATED_PLUGIN_DIR_COMPONENTS: &[&str] = &["generated", "claude-plugin"];
const DEBUG_GENERATED_PLUGIN_DIR_COMPONENTS: &[&str] =
    &[".artifacts", "generated", "claude-plugin"];

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanonicalAgentDefinition {
    pub name: String,
    pub role: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub capabilities: CanonicalAgentCapabilities,
    #[serde(default)]
    pub harnesses: CanonicalAgentHarnesses,
    #[serde(default)]
    pub delegation: CanonicalDelegationMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct CanonicalAgentCapabilities {
    #[serde(default)]
    pub mcp_tools: Vec<String>,
    #[serde(default)]
    pub internal_skills: CanonicalInternalSkillsConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct CanonicalInternalSkillsConfig {
    #[serde(default)]
    pub auto_match: bool,
    #[serde(default)]
    pub allowed: Vec<String>,
    #[serde(default)]
    pub max_auto_loaded: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct CanonicalDelegationMetadata {
    #[serde(default)]
    pub allowed_targets: Vec<String>,
}

impl CanonicalDelegationMetadata {
    pub fn is_enabled(&self) -> bool {
        !self.allowed_targets.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct CanonicalAgentHarnesses {
    #[serde(default)]
    pub claude: CanonicalClaudeAgentMetadata,
    #[serde(default)]
    pub codex: CanonicalCodexAgentMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct CanonicalClaudeAgentMetadata {
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
    #[serde(default)]
    pub mcp_transport: Option<String>,
    #[serde(default)]
    pub mcp_tools: Vec<String>,
    #[serde(default)]
    pub tools: Option<CanonicalClaudeToolSpec>,
    #[serde(default)]
    pub disallowed_tools: Vec<String>,
    #[serde(default)]
    pub preapproved_cli_tools: Vec<String>,
    #[serde(default)]
    pub permission_mode: Option<String>,
    #[serde(default)]
    pub skills: Vec<String>,
    #[serde(default)]
    pub max_turns: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct CanonicalClaudeToolSpec {
    #[serde(default)]
    pub mcp_only: bool,
    #[serde(default)]
    pub extends: Option<String>,
    #[serde(default)]
    pub include: Vec<String>,
}

impl CanonicalClaudeAgentMetadata {
    fn is_empty(&self) -> bool {
        self.model.is_none()
            && self.effort.is_none()
            && self.mcp_transport.is_none()
            && self.mcp_tools.is_empty()
            && self.tools.is_none()
            && self.disallowed_tools.is_empty()
            && self.preapproved_cli_tools.is_empty()
            && self.permission_mode.is_none()
            && self.skills.is_empty()
            && self.max_turns.is_none()
    }

    fn overlay_onto(self, mut base: Self) -> Self {
        if self.model.is_some() {
            base.model = self.model;
        }
        if self.effort.is_some() {
            base.effort = self.effort;
        }
        if self.mcp_transport.is_some() {
            base.mcp_transport = self.mcp_transport;
        }
        if !self.mcp_tools.is_empty() {
            base.mcp_tools = self.mcp_tools;
        }
        if self.tools.is_some() {
            base.tools = self.tools;
        }
        if !self.disallowed_tools.is_empty() {
            base.disallowed_tools = self.disallowed_tools;
        }
        if !self.preapproved_cli_tools.is_empty() {
            base.preapproved_cli_tools = self.preapproved_cli_tools;
        }
        if self.permission_mode.is_some() {
            base.permission_mode = self.permission_mode;
        }
        if !self.skills.is_empty() {
            base.skills = self.skills;
        }
        if self.max_turns.is_some() {
            base.max_turns = self.max_turns;
        }
        base
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct CanonicalCodexAgentMetadata {
    #[serde(default)]
    pub runtime_features: BTreeMap<String, bool>,
    #[serde(default)]
    pub mcp_transport: Option<String>,
    #[serde(default)]
    pub mcp_tools: Vec<String>,
}

impl CanonicalCodexAgentMetadata {
    fn is_empty(&self) -> bool {
        self.runtime_features.is_empty()
            && self.mcp_transport.is_none()
            && self.mcp_tools.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentPromptHarness {
    Claude,
    Codex,
}

impl AgentPromptHarness {
    fn dir_name(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CanonicalAgentFileKind {
    Definition,
    HarnessPrompt(AgentPromptHarness),
    SharedPrompt,
    HarnessMetadata(AgentPromptHarness),
}

fn canonical_agent_name(agent_name: &str) -> &str {
    canonical_short_agent_name(agent_name)
}

fn trusted_runtime_plugin_dir(plugin_dir: &Path) -> Option<&Path> {
    let has_expected_shape = path_has_component_suffix(plugin_dir, PRIMARY_PLUGIN_DIR_COMPONENTS)
        || path_has_component_suffix(plugin_dir, LEGACY_PLUGIN_DIR_COMPONENTS)
        || path_has_component_suffix(plugin_dir, GENERATED_PLUGIN_DIR_COMPONENTS)
        || path_has_component_suffix(plugin_dir, DEBUG_GENERATED_PLUGIN_DIR_COMPONENTS);

    if !has_expected_shape {
        return None;
    }

    if !path_has_only_safe_components(plugin_dir) {
        return None;
    }

    Some(plugin_dir)
}

fn path_has_only_safe_components(path: &Path) -> bool {
    path.components().all(|component| {
        matches!(
            component,
            std::path::Component::Prefix(_)
                | std::path::Component::RootDir
                | std::path::Component::Normal(_)
        )
    })
}

fn path_has_component_suffix(path: &Path, suffix: &[&str]) -> bool {
    let components = path
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>();

    components.as_slice().ends_with(
        &suffix
            .iter()
            .map(|part| part.to_string())
            .collect::<Vec<_>>(),
    )
}

fn trusted_canonical_agent_name(agent_name: &str) -> Option<&str> {
    let short_name = canonical_agent_name(agent_name);
    let valid_component = !short_name.is_empty()
        && !short_name.contains("..")
        && !short_name.contains('/')
        && !short_name.contains('\\')
        && short_name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');

    if valid_component {
        Some(short_name)
    } else {
        None
    }
}

fn canonical_agent_root(project_root: &Path, agent_name: &str) -> Option<PathBuf> {
    let trusted_agent_name = trusted_canonical_agent_name(agent_name)?;
    let agents_root = trusted_canonical_agents_root(project_root)?;
    let agent_root = agents_root.join(trusted_agent_name);
    let canonical_agent_root = agent_root.canonicalize().ok()?;
    if canonical_agent_root.starts_with(&agents_root)
        && canonical_agent_root.file_name() == Some(OsStr::new(trusted_agent_name))
        && canonical_agent_root.is_dir()
    {
        Some(canonical_agent_root)
    } else {
        None
    }
}

fn has_canonical_agents_tree(root: &Path) -> bool {
    let Ok(canonical_root) = root.canonicalize() else {
        return false;
    };
    if !path_has_only_safe_components(&canonical_root) {
        return false;
    }

    let agents_dir = canonical_root.join(CANONICAL_AGENTS_DIR);
    let Ok(canonical_agents_dir) = agents_dir.canonicalize() else {
        return false;
    };
    if !canonical_agents_dir.starts_with(&canonical_root)
        || canonical_agents_dir.file_name() != Some(OsStr::new(CANONICAL_AGENTS_DIR))
        || !canonical_agents_dir.is_dir()
    {
        return false;
    }

    // codeql[rust/path-injection]
    let Ok(entries) = std::fs::read_dir(&canonical_agents_dir) else {
        return false;
    };

    entries.filter_map(Result::ok).any(|entry| {
        let Ok(canonical_entry_path) = entry.path().canonicalize() else {
            return false;
        };
        if !canonical_entry_path.starts_with(&canonical_agents_dir)
            || !canonical_entry_path.is_dir()
        {
            return false;
        }

        let agent_file = canonical_entry_path.join(AGENT_FILE_NAME);
        let Ok(canonical_agent_file) = agent_file.canonicalize() else {
            return false;
        };
        canonical_agent_file.starts_with(&canonical_entry_path)
            && canonical_agent_file.file_name() == Some(OsStr::new(AGENT_FILE_NAME))
            && canonical_agent_file.is_file()
    })
}

fn trusted_canonical_project_root(project_root: &Path) -> Option<PathBuf> {
    let canonical_root = project_root.canonicalize().ok()?;
    if !path_has_only_safe_components(&canonical_root)
        || !has_canonical_agents_tree(&canonical_root)
    {
        return None;
    }

    Some(canonical_root)
}

fn trusted_canonical_agents_root(project_root: &Path) -> Option<PathBuf> {
    let canonical_root = trusted_canonical_project_root(project_root)?;
    let agents_root = canonical_root.join(CANONICAL_AGENTS_DIR);
    let canonical_agents_root = agents_root.canonicalize().ok()?;
    if canonical_agents_root.starts_with(&canonical_root)
        && canonical_agents_root.file_name() == Some(OsStr::new(CANONICAL_AGENTS_DIR))
        && canonical_agents_root.is_dir()
    {
        Some(canonical_agents_root)
    } else {
        None
    }
}

fn trusted_canonical_agent_file(
    project_root: &Path,
    agent_name: &str,
    file_kind: CanonicalAgentFileKind,
) -> Option<PathBuf> {
    let agent_root = canonical_agent_root(project_root, agent_name)?;
    let candidate = match file_kind {
        CanonicalAgentFileKind::Definition => agent_root.join(AGENT_FILE_NAME),
        CanonicalAgentFileKind::HarnessPrompt(harness) => {
            agent_root.join(harness.dir_name()).join(PROMPT_FILE_NAME)
        }
        CanonicalAgentFileKind::SharedPrompt => agent_root
            .join(SHARED_PROMPT_DIR_NAME)
            .join(PROMPT_FILE_NAME),
        CanonicalAgentFileKind::HarnessMetadata(harness) => {
            agent_root.join(harness.dir_name()).join(AGENT_FILE_NAME)
        }
    };
    let canonical_candidate = candidate.canonicalize().ok()?;
    if canonical_candidate.starts_with(&agent_root) && canonical_candidate.is_file() {
        Some(canonical_candidate)
    } else {
        None
    }
}

fn read_trusted_canonical_agent_file(
    project_root: &Path,
    agent_name: &str,
    file_kind: CanonicalAgentFileKind,
) -> Option<String> {
    let agent_root = canonical_agent_root(project_root, agent_name)?;
    let candidate = match file_kind {
        CanonicalAgentFileKind::Definition => agent_root.join(AGENT_FILE_NAME),
        CanonicalAgentFileKind::HarnessPrompt(harness) => {
            agent_root.join(harness.dir_name()).join(PROMPT_FILE_NAME)
        }
        CanonicalAgentFileKind::SharedPrompt => agent_root
            .join(SHARED_PROMPT_DIR_NAME)
            .join(PROMPT_FILE_NAME),
        CanonicalAgentFileKind::HarnessMetadata(harness) => {
            agent_root.join(harness.dir_name()).join(AGENT_FILE_NAME)
        }
    };
    let canonical_candidate = candidate.canonicalize().ok()?;
    if !canonical_candidate.starts_with(&agent_root) || !canonical_candidate.is_file() {
        return None;
    }

    // codeql[rust/path-injection]
    std::fs::read_to_string(canonical_candidate).ok()
}

pub(crate) fn list_canonical_agent_names(project_root: &Path) -> Vec<String> {
    let Some(agents_root) = trusted_canonical_agents_root(project_root) else {
        return Vec::new();
    };
    // codeql[rust/path-injection]
    let Ok(entries) = std::fs::read_dir(&agents_root) else {
        return Vec::new();
    };

    let mut names = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let short_name = entry.file_name().to_string_lossy().to_string();
            let trusted_name = trusted_canonical_agent_name(&short_name)?;
            let canonical_entry_path = entry.path().canonicalize().ok()?;
            if canonical_entry_path.starts_with(&agents_root)
                && canonical_entry_path.file_name() == Some(OsStr::new(trusted_name))
                && canonical_entry_path.is_dir()
            {
                Some(trusted_name.to_string())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    names.sort();
    names
}

fn find_project_root_with_canonical_agents(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|candidate| has_canonical_agents_tree(candidate))
        .map(Path::to_path_buf)
}

pub(crate) fn resolve_project_root_from_catalog_path(start: &Path) -> Option<PathBuf> {
    find_project_root_with_canonical_agents(start)
        .and_then(|root| root.canonicalize().ok().or(Some(root)))
}

fn find_project_root_via_resolved_plugin_targets(plugin_dir: &Path) -> Option<PathBuf> {
    let plugin_dir = trusted_runtime_plugin_dir(plugin_dir)?;

    if let Ok(canonical_plugin_dir) = plugin_dir.canonicalize() {
        if canonical_plugin_dir != plugin_dir {
            if let Some(project_root) =
                find_project_root_with_canonical_agents(&canonical_plugin_dir)
            {
                return Some(project_root);
            }
        }
    }

    for entry_name in GENERATED_PLUGIN_RUNTIME_ENTRY_NAMES {
        let Ok(canonical_entry_path) = plugin_dir.join(entry_name).canonicalize() else {
            continue;
        };
        if let Some(project_root) = find_project_root_with_canonical_agents(&canonical_entry_path) {
            return Some(project_root);
        }
    }

    None
}

pub fn resolve_project_root_from_plugin_dir(plugin_dir: &Path) -> PathBuf {
    let Some(plugin_dir) = trusted_runtime_plugin_dir(plugin_dir) else {
        let parent = plugin_dir.parent().unwrap_or(plugin_dir);
        return parent.to_path_buf();
    };

    if let Some(project_root) = find_project_root_with_canonical_agents(plugin_dir) {
        return project_root;
    }

    if let Some(project_root) = find_project_root_via_resolved_plugin_targets(plugin_dir) {
        return project_root;
    }

    let parent = plugin_dir.parent().unwrap_or(plugin_dir);
    if plugin_dir.ends_with(Path::new("plugins/app")) {
        parent
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| parent.to_path_buf())
    } else {
        parent.to_path_buf()
    }
}

pub fn load_canonical_agent_definition(
    project_root: &Path,
    agent_name: &str,
) -> Option<CanonicalAgentDefinition> {
    let short_name = trusted_canonical_agent_name(agent_name)?;
    let raw = read_trusted_canonical_agent_file(
        project_root,
        short_name,
        CanonicalAgentFileKind::Definition,
    )?;
    let definition = serde_yaml::from_str::<CanonicalAgentDefinition>(&raw).ok()?;
    if definition.name == short_name {
        Some(definition)
    } else {
        None
    }
}

pub fn list_canonical_prompt_backed_agents(
    project_root: &Path,
    harness: AgentPromptHarness,
) -> Vec<String> {
    let mut names = list_canonical_agent_names(project_root)
        .into_iter()
        .filter(|agent_name| {
            trusted_canonical_agent_file(
                project_root,
                agent_name,
                CanonicalAgentFileKind::HarnessPrompt(harness),
            )
            .is_some()
                || trusted_canonical_agent_file(
                    project_root,
                    agent_name,
                    CanonicalAgentFileKind::SharedPrompt,
                )
                .is_some()
        })
        .collect::<Vec<_>>();
    names.sort();
    names
}

pub fn has_canonical_agent_definition(project_root: &Path, agent_name: &str) -> bool {
    trusted_canonical_agent_file(project_root, agent_name, CanonicalAgentFileKind::Definition)
        .is_some()
}

pub fn load_canonical_claude_metadata(
    project_root: &Path,
    agent_name: &str,
) -> CanonicalClaudeAgentMetadata {
    try_load_canonical_claude_metadata(project_root, agent_name).unwrap_or_default()
}

pub fn try_load_canonical_claude_metadata(
    project_root: &Path,
    agent_name: &str,
) -> Result<CanonicalClaudeAgentMetadata, String> {
    let legacy =
        match load_harness_agent_metadata(project_root, agent_name, AgentPromptHarness::Claude) {
            Some(raw) => {
                serde_yaml::from_str::<CanonicalClaudeAgentMetadata>(&raw).map_err(|error| {
                    format!(
                        "Failed to parse Claude harness metadata for agent {}: {error}",
                        canonical_agent_name(agent_name)
                    )
                })?
            }
            None => CanonicalClaudeAgentMetadata::default(),
        };

    let Some(definition) = load_canonical_agent_definition(project_root, agent_name) else {
        return Ok(legacy);
    };
    if definition.harnesses.claude.is_empty() {
        return Ok(legacy);
    }

    let merged = definition
        .harnesses
        .claude
        .clone()
        .overlay_onto(legacy.clone());
    if merged != legacy {
        tracing::debug!(
            agent = %canonical_agent_name(agent_name),
            canonical_claude_metadata = ?definition.harnesses.claude,
            legacy_claude_metadata = ?legacy,
            "Canonical agent metadata overrides or augments Claude harness metadata"
        );
    }
    Ok(merged)
}

pub fn load_canonical_codex_metadata(
    project_root: &Path,
    agent_name: &str,
) -> CanonicalCodexAgentMetadata {
    try_load_canonical_codex_metadata(project_root, agent_name).unwrap_or_default()
}

pub fn try_load_canonical_codex_metadata(
    project_root: &Path,
    agent_name: &str,
) -> Result<CanonicalCodexAgentMetadata, String> {
    if let Some(definition) = load_canonical_agent_definition(project_root, agent_name) {
        if !definition.harnesses.codex.is_empty() {
            if let Some(raw) =
                load_harness_agent_metadata(project_root, agent_name, AgentPromptHarness::Codex)
            {
                let fallback =
                    serde_yaml::from_str::<CanonicalCodexAgentMetadata>(&raw).map_err(|error| {
                        format!(
                            "Failed to parse Codex harness metadata for agent {}: {error}",
                            canonical_agent_name(agent_name)
                        )
                    })?;
                if fallback != definition.harnesses.codex {
                    tracing::debug!(
                        agent = %canonical_agent_name(agent_name),
                        canonical_codex_metadata = ?definition.harnesses.codex,
                        harness_codex_metadata = ?fallback,
                        "Canonical agent metadata overrides divergent Codex harness metadata"
                    );
                }
            }
            return Ok(definition.harnesses.codex);
        }
    }

    match load_harness_agent_metadata(project_root, agent_name, AgentPromptHarness::Codex) {
        Some(raw) => serde_yaml::from_str::<CanonicalCodexAgentMetadata>(&raw).map_err(|error| {
            format!(
                "Failed to parse Codex harness metadata for agent {}: {error}",
                canonical_agent_name(agent_name)
            )
        }),
        None => Ok(CanonicalCodexAgentMetadata::default()),
    }
}

pub fn resolve_harness_agent_prompt_path(
    project_root: &Path,
    agent_name: &str,
    harness: AgentPromptHarness,
) -> Option<PathBuf> {
    load_canonical_agent_definition(project_root, agent_name)?;
    let prompt_path = trusted_canonical_agent_file(
        project_root,
        agent_name,
        CanonicalAgentFileKind::HarnessPrompt(harness),
    );
    if let Some(prompt_path) = prompt_path {
        return Some(prompt_path);
    }

    trusted_canonical_agent_file(
        project_root,
        agent_name,
        CanonicalAgentFileKind::SharedPrompt,
    )
}

pub fn load_harness_agent_prompt(
    project_root: &Path,
    agent_name: &str,
    harness: AgentPromptHarness,
) -> Option<String> {
    let definition = load_canonical_agent_definition(project_root, agent_name)?;
    let raw = read_trusted_canonical_agent_file(
        project_root,
        agent_name,
        CanonicalAgentFileKind::HarnessPrompt(harness),
    )
    .or_else(|| {
        read_trusted_canonical_agent_file(
            project_root,
            agent_name,
            CanonicalAgentFileKind::SharedPrompt,
        )
    })?;
    let mut prompt = raw.trim().to_string();
    if let Some(generated_appendix) = build_generated_delegation_appendix(&definition) {
        prompt.push_str("\n\n");
        prompt.push_str(&generated_appendix);
    }
    Some(prompt)
}

fn load_harness_agent_metadata(
    project_root: &Path,
    agent_name: &str,
    harness: AgentPromptHarness,
) -> Option<String> {
    load_canonical_agent_definition(project_root, agent_name)?;
    read_trusted_canonical_agent_file(
        project_root,
        agent_name,
        CanonicalAgentFileKind::HarnessMetadata(harness),
    )
}

fn build_generated_delegation_appendix(definition: &CanonicalAgentDefinition) -> Option<String> {
    let policy = &definition.delegation;
    if !policy.is_enabled() {
        return None;
    }

    let mut lines = vec![
        "## RalphX Delegation Policy (AUTO-GENERATED)".to_string(),
        "This agent is allowed to delegate only through RalphX-native delegation tools. This policy is enforced outside the prompt as well.".to_string(),
        format!(
            "- Allowed delegate targets: {}",
            policy
                .allowed_targets
                .iter()
                .map(|target| format!("`{target}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        "- Prefer the narrowest delegate that matches the required capability. Keep read-only analysis on read-only delegates.".to_string(),
        "- Use `delegate_start` to launch an allowed canonical agent with a bounded prompt and exact output contract.".to_string(),
        "- Use `delegate_wait` before depending on delegated output.".to_string(),
        "- Use `delegate_cancel` only when delegated work is stale, superseded, or invalidated.".to_string(),
        "- The MCP transport injects caller identity automatically; do not spoof another agent.".to_string(),
    ];

    let general_target_guidance = policy
        .allowed_targets
        .iter()
        .filter_map(|target| general_delegate_target_guidance(target))
        .collect::<Vec<_>>();
    if !general_target_guidance.is_empty() {
        lines.push("- General target guidance:".to_string());
        for guidance in general_target_guidance {
            lines.push(guidance);
        }
    }

    Some(lines.join("\n"))
}

fn general_delegate_target_guidance(target: &str) -> Option<String> {
    match target {
        "ralphx-general-explorer" => Some(
            "  - `ralphx-general-explorer`: read-only exploration delegate. Use it for bounded file inspection, pattern search, or evidence gathering without edits. It uses the active harness's read-only analysis surface.".to_string(),
        ),
        "ralphx-general-worker" => Some(
            "  - `ralphx-general-worker`: bounded implementation delegate. Use it when a child must inspect code and make scoped edits, while still being able to do read-only codebase analysis.".to_string(),
        ),
        _ => None,
    }
}

#[cfg(test)]
#[path = "harness_agent_catalog_tests.rs"]
mod tests;
