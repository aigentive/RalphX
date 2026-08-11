use serde::Deserialize;
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::infrastructure::agents::claude::canonical_short_agent_name;

const CANONICAL_AGENTS_DIR: &str = "agents";
const PROMPT_FILE_NAME: &str = "prompt.md";
const AGENT_FILE_NAME: &str = "agent.yaml";
const SHARED_PROMPT_DIR_NAME: &str = "shared";
const PROFILES_DIR_NAME: &str = "profiles";
const GENERATED_PLUGIN_RUNTIME_ENTRY_NAMES: &[&str] = &["ralphx-mcp-server", "ralphx-external-mcp"];
const PRIMARY_PLUGIN_DIR_COMPONENTS: &[&str] = &["plugins", "app"];
const LEGACY_PLUGIN_DIR_COMPONENTS: &[&str] = &["ralphx-plugin"];
const GENERATED_PLUGIN_DIR_COMPONENTS: &[&str] = &["generated", "claude-plugin"];
const RELEASE_GENERATED_PLUGIN_DIR_COMPONENTS: &[&str] = &["generated", "release", "claude-plugin"];
const DEBUG_PROFILE_GENERATED_PLUGIN_DIR_COMPONENTS: &[&str] =
    &["generated", "debug", "claude-plugin"];
const DEBUG_GENERATED_PLUGIN_DIR_COMPONENTS: &[&str] =
    &[".artifacts", "generated", "claude-plugin"];
const INTERNAL_MCP_SERVER_NAME_SUFFIX: &str = "_internal";

pub fn internal_mcp_server_name(primary_server_name: &str) -> String {
    format!("{primary_server_name}{INTERNAL_MCP_SERVER_NAME_SUFFIX}")
}

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
    #[serde(default)]
    pub profiles: BTreeMap<String, CanonicalAgentRuntimeProfile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct CanonicalAgentRuntimeProfile {
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub capabilities: CanonicalAgentCapabilities,
    #[serde(default)]
    pub harnesses: CanonicalAgentProfileHarnesses,
    #[serde(default)]
    pub delegation: Option<CanonicalDelegationMetadata>,
}

impl CanonicalAgentRuntimeProfile {
    fn overlay_onto(self, mut definition: CanonicalAgentDefinition) -> CanonicalAgentDefinition {
        if let Some(role) = self.role {
            definition.role = role;
        }
        if let Some(description) = self.description {
            definition.description = Some(description);
        }
        definition.capabilities = self.capabilities.overlay_onto(definition.capabilities);
        definition.harnesses = self.harnesses.overlay_onto(definition.harnesses);
        if let Some(delegation) = self.delegation {
            definition.delegation = delegation;
        }
        definition
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct CanonicalAgentProfileHarnesses {
    #[serde(default)]
    pub claude: CanonicalClaudeAgentProfileMetadata,
    #[serde(default)]
    pub codex: CanonicalCodexAgentProfileMetadata,
}

impl CanonicalAgentProfileHarnesses {
    fn overlay_onto(self, base: CanonicalAgentHarnesses) -> CanonicalAgentHarnesses {
        CanonicalAgentHarnesses {
            claude: self.claude.overlay_onto(base.claude),
            codex: self.codex.overlay_onto(base.codex),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct CanonicalAgentCapabilities {
    #[serde(default)]
    pub mcp_tools: Vec<String>,
    #[serde(default)]
    pub internal_skills: CanonicalInternalSkillsConfig,
}

impl CanonicalAgentCapabilities {
    fn overlay_onto(self, mut base: Self) -> Self {
        if !self.mcp_tools.is_empty() {
            base.mcp_tools = self.mcp_tools;
        }
        if !self.internal_skills.is_empty() {
            base.internal_skills = self.internal_skills;
        }
        base
    }
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

impl CanonicalInternalSkillsConfig {
    fn is_empty(&self) -> bool {
        !self.auto_match && self.allowed.is_empty() && self.max_auto_loaded.is_none()
    }
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
    pub internal_mcp_tools: Vec<String>,
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

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct CanonicalClaudeAgentProfileMetadata {
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
    #[serde(default)]
    pub mcp_transport: Option<String>,
    #[serde(default)]
    pub mcp_tools: Option<Vec<String>>,
    #[serde(default)]
    pub internal_mcp_tools: Option<Vec<String>>,
    #[serde(default)]
    pub tools: Option<CanonicalClaudeToolSpec>,
    #[serde(default)]
    pub disallowed_tools: Option<Vec<String>>,
    #[serde(default)]
    pub preapproved_cli_tools: Option<Vec<String>>,
    #[serde(default)]
    pub permission_mode: Option<String>,
    #[serde(default)]
    pub skills: Option<Vec<String>>,
    #[serde(default)]
    pub max_turns: Option<u32>,
}

impl CanonicalClaudeAgentProfileMetadata {
    fn overlay_onto(self, mut base: CanonicalClaudeAgentMetadata) -> CanonicalClaudeAgentMetadata {
        if self.model.is_some() {
            base.model = self.model;
        }
        if self.effort.is_some() {
            base.effort = self.effort;
        }
        if self.mcp_transport.is_some() {
            base.mcp_transport = self.mcp_transport;
        }
        if let Some(mcp_tools) = self.mcp_tools {
            base.mcp_tools = mcp_tools;
        }
        if let Some(internal_mcp_tools) = self.internal_mcp_tools {
            base.internal_mcp_tools = internal_mcp_tools;
        }
        if self.tools.is_some() {
            base.tools = self.tools;
        }
        if let Some(disallowed_tools) = self.disallowed_tools {
            base.disallowed_tools = disallowed_tools;
        }
        if let Some(preapproved_cli_tools) = self.preapproved_cli_tools {
            base.preapproved_cli_tools = preapproved_cli_tools;
        }
        if self.permission_mode.is_some() {
            base.permission_mode = self.permission_mode;
        }
        if let Some(skills) = self.skills {
            base.skills = skills;
        }
        if self.max_turns.is_some() {
            base.max_turns = self.max_turns;
        }
        base
    }
}

impl CanonicalClaudeAgentMetadata {
    fn is_empty(&self) -> bool {
        self.model.is_none()
            && self.effort.is_none()
            && self.mcp_transport.is_none()
            && self.mcp_tools.is_empty()
            && self.internal_mcp_tools.is_empty()
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
        if !self.internal_mcp_tools.is_empty() {
            base.internal_mcp_tools = self.internal_mcp_tools;
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
    #[serde(default)]
    pub internal_mcp_tools: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct CanonicalCodexAgentProfileMetadata {
    #[serde(default)]
    pub runtime_features: Option<BTreeMap<String, bool>>,
    #[serde(default)]
    pub mcp_transport: Option<String>,
    #[serde(default)]
    pub mcp_tools: Option<Vec<String>>,
    #[serde(default)]
    pub internal_mcp_tools: Option<Vec<String>>,
}

impl CanonicalCodexAgentProfileMetadata {
    fn overlay_onto(self, mut base: CanonicalCodexAgentMetadata) -> CanonicalCodexAgentMetadata {
        if let Some(runtime_features) = self.runtime_features {
            base.runtime_features = runtime_features;
        }
        if self.mcp_transport.is_some() {
            base.mcp_transport = self.mcp_transport;
        }
        if let Some(mcp_tools) = self.mcp_tools {
            base.mcp_tools = mcp_tools;
        }
        if let Some(internal_mcp_tools) = self.internal_mcp_tools {
            base.internal_mcp_tools = internal_mcp_tools;
        }
        base
    }
}

impl CanonicalCodexAgentMetadata {
    fn is_empty(&self) -> bool {
        self.runtime_features.is_empty()
            && self.mcp_transport.is_none()
            && self.mcp_tools.is_empty()
            && self.internal_mcp_tools.is_empty()
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
        || path_has_component_suffix(plugin_dir, RELEASE_GENERATED_PLUGIN_DIR_COMPONENTS)
        || path_has_component_suffix(plugin_dir, DEBUG_PROFILE_GENERATED_PLUGIN_DIR_COMPONENTS)
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

/// Accepts the same profile-name set as the MCP server's `SAFE_CANONICAL_PROFILE_NAME`
/// (`plugins/app/ralphx-mcp-server/src/canonical-agent-metadata.ts`), i.e.
/// `^[a-z0-9]+(?:[_-][a-z0-9]+)*$`. Both validators must stay in lockstep: a profile key
/// accepted by one layer and rejected by the other makes the profile unloadable at spawn.
fn is_safe_canonical_profile_component(profile_name: &str) -> bool {
    let bytes = profile_name.as_bytes();
    let is_alphanumeric = |byte: u8| byte.is_ascii_lowercase() || byte.is_ascii_digit();
    let is_separator = |byte: u8| byte == b'-' || byte == b'_';

    if bytes.is_empty() {
        return false;
    }
    // No leading or trailing separator.
    if !is_alphanumeric(bytes[0]) || !is_alphanumeric(bytes[bytes.len() - 1]) {
        return false;
    }

    let mut previous_was_separator = false;
    for &byte in bytes {
        if is_alphanumeric(byte) {
            previous_was_separator = false;
        } else if is_separator(byte) {
            // No two adjacent separators; this is what makes `..` unreachable.
            if previous_was_separator {
                return false;
            }
            previous_was_separator = true;
        } else {
            return false;
        }
    }

    true
}

fn trusted_canonical_profile_name(profile_name: &str) -> Option<&str> {
    let valid_component = !profile_name.contains("..")
        && !profile_name.contains('/')
        && !profile_name.contains('\\')
        && is_safe_canonical_profile_component(profile_name);

    if valid_component {
        Some(profile_name)
    } else {
        None
    }
}

pub(crate) fn escape_prompt_context_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub fn render_agent_runtime_profile_context(
    project_root: &Path,
    agent_name: &str,
    profile_name: Option<&str>,
) -> Option<String> {
    let profile_name = profile_name?;
    let trusted_profile_name = trusted_canonical_profile_name(profile_name)?;
    let definition =
        load_canonical_agent_definition_for_profile(project_root, agent_name, Some(profile_name))?;
    let agent_name = canonical_agent_name(agent_name);

    Some(format!(
        "<agent_runtime_profile>\n<agent_name>{}</agent_name>\n<profile_slug>{}</profile_slug>\n<profile_role>{}</profile_role>\n</agent_runtime_profile>",
        escape_prompt_context_text(agent_name),
        escape_prompt_context_text(trusted_profile_name),
        escape_prompt_context_text(&definition.role)
    ))
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

fn read_trusted_canonical_profile_prompt(
    project_root: &Path,
    agent_name: &str,
    harness: AgentPromptHarness,
    profile_name: &str,
) -> Option<String> {
    let agent_root = canonical_agent_root(project_root, agent_name)?;
    let trusted_profile_name = trusted_canonical_profile_name(profile_name)?;
    let candidate = agent_root
        .join(PROFILES_DIR_NAME)
        .join(trusted_profile_name)
        .join(harness.dir_name())
        .join(PROMPT_FILE_NAME);
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

pub fn load_canonical_agent_definition_for_profile(
    project_root: &Path,
    agent_name: &str,
    profile_name: Option<&str>,
) -> Option<CanonicalAgentDefinition> {
    let definition = load_canonical_agent_definition(project_root, agent_name)?;
    let Some(profile_name) = profile_name else {
        return Some(definition);
    };
    let trusted_profile_name = trusted_canonical_profile_name(profile_name)?;
    let profile = definition.profiles.get(trusted_profile_name)?.clone();
    Some(profile.overlay_onto(definition))
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
    try_load_canonical_claude_metadata_for_profile(project_root, agent_name, None)
        .unwrap_or_default()
}

pub fn try_load_canonical_claude_metadata(
    project_root: &Path,
    agent_name: &str,
) -> Result<CanonicalClaudeAgentMetadata, String> {
    try_load_canonical_claude_metadata_for_profile(project_root, agent_name, None)
}

pub fn load_canonical_claude_metadata_for_profile(
    project_root: &Path,
    agent_name: &str,
    profile_name: Option<&str>,
) -> CanonicalClaudeAgentMetadata {
    try_load_canonical_claude_metadata_for_profile(project_root, agent_name, profile_name)
        .unwrap_or_default()
}

/// Resolves the canonical definition a harness-metadata loader should merge from.
///
/// Fails closed: an invalid or unknown profile is an error, never a silent fall back to the
/// base agent definition. A coordinator profile exists precisely to drop `Write`/`Edit`/`Bash`,
/// so returning the base config on a profile miss would be a privilege escalation.
///
/// # Errors
///
/// Returns `Err` when `profile_name` is malformed, or when it is well-formed but the agent
/// declares no such profile. The two cases carry distinguishable messages.
fn resolve_canonical_definition_for_profile(
    project_root: &Path,
    agent_name: &str,
    profile_name: Option<&str>,
) -> Result<Option<CanonicalAgentDefinition>, String> {
    let Some(profile_name) = profile_name else {
        return Ok(load_canonical_agent_definition(project_root, agent_name));
    };

    if trusted_canonical_profile_name(profile_name).is_none() {
        return Err(format!(
            "Invalid canonical profile name {:?} for agent {}",
            profile_name,
            canonical_agent_name(agent_name)
        ));
    }

    load_canonical_agent_definition_for_profile(project_root, agent_name, Some(profile_name))
        .map(Some)
        .ok_or_else(|| {
            format!(
                "Missing canonical profile {:?} for agent {}",
                Some(profile_name),
                canonical_agent_name(agent_name)
            )
        })
}

pub fn try_load_canonical_claude_metadata_for_profile(
    project_root: &Path,
    agent_name: &str,
    profile_name: Option<&str>,
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

    let definition =
        resolve_canonical_definition_for_profile(project_root, agent_name, profile_name)?;

    let Some(definition) = definition else {
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
        // Log only the validated agent name; metadata structs can carry
        // credential-adjacent config (e.g. bearer-token env var names) and
        // must not be dumped into persistent logs.
        tracing::debug!(
            agent = %trusted_canonical_agent_name(agent_name).unwrap_or("<invalid-agent-name>"),
            "Canonical agent metadata overrides or augments Claude harness metadata"
        );
    }
    Ok(merged)
}

pub fn load_canonical_codex_metadata(
    project_root: &Path,
    agent_name: &str,
) -> CanonicalCodexAgentMetadata {
    try_load_canonical_codex_metadata_for_profile(project_root, agent_name, None)
        .unwrap_or_default()
}

pub fn try_load_canonical_codex_metadata(
    project_root: &Path,
    agent_name: &str,
) -> Result<CanonicalCodexAgentMetadata, String> {
    try_load_canonical_codex_metadata_for_profile(project_root, agent_name, None)
}

pub fn load_canonical_codex_metadata_for_profile(
    project_root: &Path,
    agent_name: &str,
    profile_name: Option<&str>,
) -> CanonicalCodexAgentMetadata {
    try_load_canonical_codex_metadata_for_profile(project_root, agent_name, profile_name)
        .unwrap_or_default()
}

pub fn try_load_canonical_codex_metadata_for_profile(
    project_root: &Path,
    agent_name: &str,
    profile_name: Option<&str>,
) -> Result<CanonicalCodexAgentMetadata, String> {
    let definition =
        resolve_canonical_definition_for_profile(project_root, agent_name, profile_name)?;

    if let Some(definition) = definition {
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
                    // Log only the validated agent name; metadata structs can
                    // carry credential-adjacent config (e.g. bearer-token env
                    // var names) and must not be dumped into persistent logs.
                    tracing::debug!(
                        agent = %trusted_canonical_agent_name(agent_name)
                            .unwrap_or("<invalid-agent-name>"),
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
    load_harness_agent_prompt_for_profile(project_root, agent_name, harness, None)
}

pub fn load_harness_agent_prompt_for_profile(
    project_root: &Path,
    agent_name: &str,
    harness: AgentPromptHarness,
    profile_name: Option<&str>,
) -> Option<String> {
    let definition =
        load_canonical_agent_definition_for_profile(project_root, agent_name, profile_name)?;
    let raw = profile_name
        .and_then(|profile_name| {
            read_trusted_canonical_profile_prompt(project_root, agent_name, harness, profile_name)
        })
        .or_else(|| {
            read_trusted_canonical_agent_file(
                project_root,
                agent_name,
                CanonicalAgentFileKind::HarnessPrompt(harness),
            )
        })
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
    if let Some(generated_appendix) = build_generated_agent_task_appendix(&definition, harness) {
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
        "- When delegating an existing item from your current task ledger, pass its `task_ref`; RalphX assigns it atomically, so the delegate must not claim or mirror it.".to_string(),
        "- Use `delegate_wait` before depending on delegated output.".to_string(),
        "- For short waits, call `delegate_wait` once with `wait_timeout_ms`; the backend blocks until a delegate settles. Do not spin on repeated immediate `delegate_wait` calls.".to_string(),
        "- For long waits, call `delegate_park` with the outstanding job ids and then END YOUR TURN. RalphX resumes you automatically when the wake condition is met, on failure, or at the deadline.".to_string(),
        "- Parking is not abandonment: state what you are waiting for before ending the turn.".to_string(),
        "- Use `delegate_cancel` only when delegated work is stale, superseded, or invalidated.".to_string(),
        "- The MCP transport injects caller identity automatically; do not spoof another agent.".to_string(),
        "### Delegated Implementation Coordination".to_string(),
        "Only when implementation coordination is within your live role and permitted by repository and profile restrictions (otherwise do not direct mutation or validation):".to_string(),
        "- Retain the full objective, integration decisions, integrated review, and final verification; decompose work into bounded, dependency-aware slices.".to_string(),
        "- Give each delegate an outcome, exclusive writable files or modules, established seam, required behavior, prohibited scope, acceptance criteria, permitted validation, and dependencies.".to_string(),
        "- Exploration may cover owned files and immediate dependencies needed for safe implementation.".to_string(),
        "- Parallel mutation requires disjoint mutation sets, including generated artifacts, and disjoint command resource sets; prefer one serialized heavyweight-validation lane while implementation delegates add behavioral tests and only permitted cheap or local checks.".to_string(),
        "- Directly verify suspected defects before a narrow, evidence-backed revision; review the integrated workspace and run final focused validation once after revisions settle.".to_string(),
        "- Do not publish unless the user requested it. Repository and profile rules remain authoritative for exact commands, cleanup, resource conflicts, and stricter permissions.".to_string(),
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

fn build_generated_agent_task_appendix(
    definition: &CanonicalAgentDefinition,
    harness: AgentPromptHarness,
) -> Option<String> {
    let (harness_tools, internal_harness_tools) = match harness {
        AgentPromptHarness::Claude => (
            &definition.harnesses.claude.mcp_tools,
            &definition.harnesses.claude.internal_mcp_tools,
        ),
        AgentPromptHarness::Codex => (
            &definition.harnesses.codex.mcp_tools,
            &definition.harnesses.codex.internal_mcp_tools,
        ),
    };
    let tools: Vec<&str> = if harness_tools.is_empty() && internal_harness_tools.is_empty() {
        definition
            .capabilities
            .mcp_tools
            .iter()
            .map(String::as_str)
            .collect()
    } else {
        harness_tools
            .iter()
            .chain(internal_harness_tools.iter())
            .map(String::as_str)
            .collect()
    };
    let tool_name =
        |name: &'static str| -> Option<&'static str> { tools.contains(&name).then_some(name) };
    let create_tool = tool_name("create_agent_task");
    let get_tool = tool_name("get_agent_task");
    let list_tool = tool_name("list_agent_tasks");
    let update_tool = tool_name("update_agent_task");
    let claim_tool = tool_name("claim_agent_task");
    let complete_tool = tool_name("complete_agent_task");
    let get_assignment_tool = tool_name("get_delegate_assignment");
    let complete_assignment_tool = tool_name("complete_delegate_assignment");
    let release_assignment_tool = tool_name("release_delegate_assignment");
    if [
        create_tool,
        get_tool,
        list_tool,
        update_tool,
        claim_tool,
        complete_tool,
    ]
    .iter()
    .all(Option::is_none)
    {
        return None;
    }

    let supports_full_task_flow = list_tool.is_some()
        && create_tool.is_some()
        && (claim_tool.is_some() || update_tool.is_some())
        && complete_tool.is_some();

    let mut lines = vec![
        "<agent_task_ledger_contract>".to_string(),
        "<purpose>".to_string(),
        "Use the RalphX agent task ledger for multi-step work, dependencies, and delegated follow-up that should survive across tool calls in this context.".to_string(),
        "</purpose>".to_string(),
    ];

    if supports_full_task_flow {
        lines.extend(
            [
                "<when_required>",
                "<rule>Use the ledger for non-trivial implementation, codebase investigation, edits, validation, multi-step work, multiple requested fixes, or follow-up that should survive context compaction.</rule>",
                "<rule>Use the ledger for complex multi-step work: three or more distinct actions, files, phases, or verification steps.</rule>",
                "<rule>Use the ledger for user-provided task lists: multiple requested fixes, features, checks, or follow-ups in one message.</rule>",
                "<rule>Read-only audits, multi-file investigations, prompt-surface reviews, and codebase inspections are tool-backed work; they are not conversational turns.</rule>",
                "<rule>Use the ledger for new instructions during an active run when they materially change scope or add work.</rule>",
                "<rule>Use the ledger for discovered follow-up work that becomes necessary during investigation.</rule>",
                "</when_required>",
                "<when_not_required>",
                "<rule>For simple conversational turns or simple questions that need no tool-backed investigation or implementation, answer directly without creating a ledger item.</rule>",
                "<rule>Do not create a ledger item for a single straightforward action with no meaningful intermediate state.</rule>",
                "<rule>Do not create a ledger item for a trivial command request where the result can be returned immediately.</rule>",
                "<rule>Do not create a ledger item for a one-location text or comment edit that needs no planning or validation beyond the edit itself.</rule>",
                "<rule>If you already created one task for genuinely single-step work, mark that accidental task `dropped` before continuing without the ledger.</rule>",
                "</when_not_required>",
                "<mandatory_start_sequence>",
                "<step>Before search, read, edit, shell, or other tool-backed work, call `list_agent_tasks`.</step>",
                "<step>If a matching open or active item exists, reuse it instead of creating a duplicate.</step>",
                "<step>If no suitable task exists, create concrete tasks for each independent work item.</step>",
                "<step>If the ledger is empty and this request requires ledger use, do not perform search, read, shell, edit, or other tool-backed work until you create the required task or tasks.</step>",
                "<step>Claim or mark active exactly the task you are starting before implementation begins.</step>",
                "</mandatory_start_sequence>",
                "<state_rules>",
                "<rule>Treat `open` as not started, `active` as currently being worked, `done` as fully completed, and `dropped` as intentionally abandoned.</rule>",
                "<rule>Keep at most one task `active` for your own work at a time unless another actor owns a separate item.</rule>",
                "<rule>Mark a task `active` before doing substantial work on it, not after the work is already complete.</rule>",
                "<rule>Mark completion immediately after the task is actually complete; do not batch completions at the end.</rule>",
                "<rule>Do not mark a task `done` while tests are failing, implementation is partial, required files are missing, or unresolved blockers remain.</rule>",
                "<rule>Do not claim, activate, or complete a ledger with only one meaningful task; decompose it into multiple concrete tasks or mark the lone accidental task `dropped`.</rule>",
                "<rule>If you are blocked, keep the blocked task active or open and create or update a concrete blocker task when useful.</rule>",
                "</state_rules>",
                "<breakdown_rules>",
                "<rule>Write specific, action-oriented titles that describe the next outcome, not vague process labels.</rule>",
                "<rule>Do not create one umbrella task for multiple independent fixes, checks, audit items, or investigation streams.</rule>",
                "<rule>For two or more requested fixes, checks, audit items, or investigation streams, create one ledger task per independent item unless the items are inseparable.</rule>",
                "<rule>For user-provided numbered audit, check, or investigation lists, create one ledger task per numbered item unless the items cannot be validated independently. This applies even when the work is read-only.</rule>",
                "<rule>A single umbrella task is acceptable only for a single-question investigation or for work items that cannot be validated independently. If the user asks to check items independently, mirror that independence in the ledger.</rule>",
                "<rule>Split broad requests into small items that can be individually completed and validated.</rule>",
                "<rule>Include acceptance criteria, file or module scope, validation expectations, and dependency notes in task details when they matter.</rule>",
                "<rule>Use dependencies for real ordering constraints instead of relying on memory.</rule>",
                "<rule>Remove or drop stale items when the plan changes and the work is no longer relevant.</rule>",
                "</breakdown_rules>",
            ]
            .into_iter()
            .map(String::from),
        );
    }

    lines.push("<tool_guidance>".to_string());
    if let Some(tool) = list_tool {
        lines.push(format!("<rule>Use `{tool}` before depending on prior ledger state or when resuming a multi-step thread.</rule>"));
    }
    if let Some(tool) = create_tool {
        lines.push(format!("<rule>Use `{tool}` for concrete work items with a clear title and details; connect blockers with `blocked_by` or `blocks` when ordering matters.</rule>"));
    }
    if let Some(tool) = claim_tool {
        lines.push(format!("<rule>Use `{tool}` before starting a ready ledger item; the backend rejects claims that still have unresolved blockers or a single-task ledger. If rejected, decompose the ledger or mark the lone accidental task `dropped`.</rule>"));
    } else if let Some(tool) = update_tool {
        lines.push(format!(
            "<rule>Use `{tool}` to mark a task `active` only when its unresolved blockers are clear.</rule>"
        ));
    }
    if let Some(tool) = update_tool {
        lines.push(format!("<rule>Use `{tool}` to update fields, owner, state, metadata, or dependency links as the plan changes; use `state=dropped` to clean up an accidental single-task ledger.</rule>"));
    }
    if let Some(tool) = complete_tool {
        lines.push(format!("<rule>Use `{tool}` when the item is actually done, optionally adding concise completion metadata.</rule>"));
    }
    if let Some(tool) = get_tool {
        lines.push(format!("<rule>Use `{tool}` when you need full details, including resolved blockers that list views may omit.</rule>"));
    }
    lines.push("</tool_guidance>".to_string());

    if let Some(get_assignment_tool) = get_assignment_tool {
        lines.extend(
            [
                "<delegate_assignment_contract>".to_string(),
                "<rule>In a delegated context, the ordinary agent-task tools operate only on your private delegate-local ledger. They never expose or mirror the caller ledger.</rule>".to_string(),
                format!("<rule>Use `{get_assignment_tool}` to inspect the exact caller task bound to this delegated run. Do not recreate that assigned task in your local ledger.</rule>"),
            ],
        );
        if let Some(complete_assignment_tool) = complete_assignment_tool {
            lines.push(format!("<rule>After every meaningful local task is done or dropped, use `{complete_assignment_tool}` to request settlement. The caller task becomes done only if this exact run then terminates successfully.</rule>"));
        }
        if let Some(release_assignment_tool) = release_assignment_tool {
            lines.push(format!("<rule>If the assigned work cannot be completed, use `{release_assignment_tool}` with a concise reason. Then stop work and return the final handoff so backend settlement can reopen the caller task.</rule>"));
        }
        lines.push("</delegate_assignment_contract>".to_string());
    }

    if supports_full_task_flow {
        lines.extend(
            [
                "<examples>",
                "<example>A provider-spawn bug requires checking logs, tracing the spawn builder, changing prompt materialization, adding a regression, and running targeted Rust tests. Create separate items for diagnosis, implementation, and validation so the fix cannot stop after the first patch.</example>",
                "<example>A user asks for three UI workflow fixes in one message. Create one item per workflow, add dependencies only where one fix must land before another, and keep the active item aligned with the workflow currently being edited.</example>",
                "<example>A user asks for a three-part read-only audit covering Codex prompt injection, Claude generated prompt files, and utility-agent exclusions. Create one task per audit check, claim each before inspecting files for that check, and complete each with findings.</example>",
                "<example>A search finds a renamed runtime field across several backend modules plus frontend consumers. After the search reveals the scope, create ledger items for backend updates, frontend updates, and compatibility validation.</example>",
                "<example>An intermittent stuck-run report needs production logs, database inspection, code-path review, and a narrow repro test. Track each investigation branch and convert confirmed findings into implementation tasks.</example>",
                "<example>The user asks what a specific command does. Answer directly without creating a ledger task.</example>",
                "<example>The user asks you to run one status command and report the output. Run it and report the result without creating a ledger task.</example>",
                "<example>The user asks for a one-word label change in a known file. Make the small edit directly and validate only as needed.</example>",
                "<example>The user is brainstorming and has not asked for implementation or tool-backed investigation. Discuss the idea without creating ledger state.</example>",
                "</examples>",
            ]
            .into_iter()
            .map(String::from),
        );
    }

    lines.push("</agent_task_ledger_contract>".to_string());

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
