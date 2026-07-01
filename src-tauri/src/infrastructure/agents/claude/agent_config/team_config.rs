// Team configuration: process mapping, team constraints, and validation.
//
// Implements configurable agent variants from the product brief:
// - ProcessMapping: maps logical processes to agent variants (solo, team, readonly)
// - TeamConstraints: guardrails for dynamic team composition per process
// - TeamMode: Dynamic (lead chooses) vs Constrained (presets only)
// - Validation: enforce constraints on team plans before spawning
//
// NOTE: Many public functions here are foundational APIs consumed by later tasks
// (state machine integration, HTTP handlers, MCP tools). Allow dead_code until wired up.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::OnceLock;

const EMBEDDED_PROCESS_CONFIG: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../config/processes.yaml"));

// ── Default value helpers ───────────────────────────────────────────────

fn default_max_teammates() -> u8 {
    5
}
fn default_model_cap() -> String {
    "sonnet".to_string()
}
fn default_mode() -> TeamMode {
    TeamMode::Dynamic
}
fn default_timeout() -> u32 {
    20
}

// ── Core types ──────────────────────────────────────────────────────────

/// A single process slot with a default agent and optional named variants.
///
/// ```yaml
/// execution:
///   default: ralphx-execution-worker
///   team: ralphx-execution-team-lead
/// ```
#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq, Eq)]
pub struct ProcessSlot {
    pub default: String,
    #[serde(flatten)]
    pub variants: HashMap<String, String>,
}

/// Maps logical process names to their agent slots.
///
/// ```yaml
/// process_mapping:
///   ideation:
///     default: ralphx-ideation
///     team: ralphx-ideation-team-lead
///   execution:
///     default: ralphx-execution-worker
///     team: ralphx-execution-team-lead
/// ```
#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq, Eq)]
pub struct ProcessMapping {
    #[serde(flatten)]
    pub slots: HashMap<String, ProcessSlot>,
}

#[derive(Debug, Clone, Deserialize)]
struct EmbeddedProcessConfig {
    process_mapping: ProcessMapping,
    team_constraints: TeamConstraintsConfig,
}

static CANONICAL_PROCESS_CONFIG: OnceLock<EmbeddedProcessConfig> = OnceLock::new();

fn embedded_process_config() -> &'static EmbeddedProcessConfig {
    CANONICAL_PROCESS_CONFIG.get_or_init(|| {
        serde_yaml::from_str(EMBEDDED_PROCESS_CONFIG)
            .expect("embedded config/processes.yaml should parse")
    })
}

pub fn canonical_process_mapping() -> ProcessMapping {
    embedded_process_config().process_mapping.clone()
}

pub fn resolve_canonical_process_mapping(raw: &ProcessMapping) -> ProcessMapping {
    let mut resolved = canonical_process_mapping();

    for (process, yaml_slot) in &raw.slots {
        match resolved.slots.get(process) {
            Some(canonical_slot) => {
                if canonical_slot != yaml_slot {
                    tracing::warn!(
                        process = %process,
                        yaml_slot = ?yaml_slot,
                        canonical_slot = ?canonical_slot,
                        "Canonical process mapping overrides divergent runtime YAML slot"
                    );
                }
            }
            None => {
                resolved.slots.insert(process.clone(), yaml_slot.clone());
            }
        }
    }

    resolved
}

/// Dynamic vs Constrained team mode.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TeamMode {
    Dynamic,
    Constrained,
}

impl Default for TeamMode {
    fn default() -> Self {
        TeamMode::Dynamic
    }
}

/// Per-process guardrails for team composition.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct TeamConstraints {
    #[serde(default = "default_max_teammates")]
    pub max_teammates: u8,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    #[serde(default)]
    pub allowed_mcp_tools: Vec<String>,
    #[serde(default = "default_model_cap")]
    pub model_cap: String,
    #[serde(default = "default_mode")]
    pub mode: TeamMode,
    #[serde(default)]
    pub presets: Vec<String>,
    #[serde(default = "default_timeout")]
    pub timeout_minutes: u32,
    #[serde(default)]
    pub budget_limit: Option<f64>,
    #[serde(default)]
    pub auto_approve: Option<bool>,
}

impl Default for TeamConstraints {
    fn default() -> Self {
        Self {
            max_teammates: default_max_teammates(),
            allowed_tools: Vec::new(),
            allowed_mcp_tools: Vec::new(),
            model_cap: default_model_cap(),
            mode: default_mode(),
            presets: Vec::new(),
            timeout_minutes: default_timeout(),
            budget_limit: None,
            auto_approve: None,
        }
    }
}

/// All process constraints plus global defaults.
///
/// ```yaml
/// team_constraints:
///   _defaults:
///     max_teammates: 5
///     model_cap: sonnet
///   execution:
///     max_teammates: 5
///     allowed_tools: [Read, Write, Edit, Bash]
/// ```
#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq)]
pub struct TeamConstraintsConfig {
    #[serde(rename = "_defaults", default)]
    pub defaults: Option<TeamConstraints>,
    #[serde(flatten)]
    pub processes: HashMap<String, TeamConstraints>,
}

pub fn canonical_team_constraints_config() -> TeamConstraintsConfig {
    embedded_process_config().team_constraints.clone()
}

pub fn resolve_canonical_team_constraints_config(raw: &TeamConstraintsConfig) -> TeamConstraintsConfig {
    let mut resolved = canonical_team_constraints_config();

    if let Some(yaml_defaults) = &raw.defaults {
        if resolved.defaults.as_ref() != Some(yaml_defaults) {
            tracing::warn!(
                yaml_defaults = ?yaml_defaults,
                canonical_defaults = ?resolved.defaults,
                "Canonical team constraints defaults override divergent runtime YAML defaults"
            );
        }
    }

    for (process, yaml_constraints) in &raw.processes {
        match resolved.processes.get(process) {
            Some(canonical_constraints) => {
                if canonical_constraints != yaml_constraints {
                    tracing::warn!(
                        process = %process,
                        yaml_constraints = ?yaml_constraints,
                        canonical_constraints = ?canonical_constraints,
                        "Canonical team constraints override divergent runtime YAML process constraints"
                    );
                }
            }
            None => {
                resolved
                    .processes
                    .insert(process.clone(), yaml_constraints.clone());
            }
        }
    }

    resolved
}

/// A single teammate in a spawn request from a team lead.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TeammateSpawnRequest {
    pub role: String,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub preset: Option<String>,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub mcp_tools: Vec<String>,
    #[serde(default = "default_model_cap")]
    pub model: String,
    #[serde(default)]
    pub prompt_summary: Option<String>,
}

/// A validated teammate in an approved team plan.
#[derive(Debug, Clone, Serialize)]
pub struct ApprovedTeammate {
    pub role: String,
    pub approved_tools: Vec<String>,
    pub approved_mcp_tools: Vec<String>,
    pub approved_model: String,
    pub from_preset: bool,
}

/// Backend-approved team composition.
#[derive(Debug, Clone, Serialize)]
pub struct ApprovedTeamPlan {
    pub plan_id: String,
    pub process: String,
    pub teammates: Vec<ApprovedTeammate>,
}

/// Errors from team constraint validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TeamConstraintError {
    MaxTeammatesExceeded { max: u8, requested: usize },
    ToolNotAllowed { tool: String, role: String },
    McpToolNotAllowed { tool: String, role: String },
    ModelExceedsCap { requested: String, cap: String },
    PresetRequired { role: String },
    AgentNotInPresets { agent: String, allowed: Vec<String> },
    UnknownProcess { process: String },
}

impl std::fmt::Display for TeamConstraintError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MaxTeammatesExceeded { max, requested } => {
                write!(f, "Max teammates exceeded: {requested} > {max}")
            }
            Self::ToolNotAllowed { tool, role } => {
                write!(f, "Tool '{tool}' not allowed for role '{role}'")
            }
            Self::McpToolNotAllowed { tool, role } => {
                write!(f, "MCP tool '{tool}' not allowed for role '{role}'")
            }
            Self::ModelExceedsCap { requested, cap } => {
                write!(f, "Model '{requested}' exceeds cap '{cap}'")
            }
            Self::PresetRequired { role } => {
                write!(f, "Constrained mode requires preset for role '{role}'")
            }
            Self::AgentNotInPresets { agent, allowed } => {
                write!(f, "Agent '{agent}' not in allowed presets: {allowed:?}")
            }
            Self::UnknownProcess { process } => {
                write!(f, "Unknown process: '{process}'")
            }
        }
    }
}

impl std::error::Error for TeamConstraintError {}

// ── Model tier ordering ─────────────────────────────────────────────────

/// Model tier for comparison. Higher ordinal = more capable.
fn model_tier(model: &str) -> u8 {
    match model.to_lowercase().as_str() {
        "haiku" => 1,
        "sonnet" | "claude-sonnet-4-6" | "claude-sonnet-5" => 2,
        "opus" => 3,
        "fable" | "claude-fable-5" => 4,
        _ => 0, // unknown models get lowest tier
    }
}

/// Check if a requested model is within the allowed cap.
pub fn model_within_cap(requested: &str, cap: &str) -> bool {
    model_tier(requested) <= model_tier(cap)
}

/// Return the lesser (more restrictive) of two model caps.
/// Ordering: haiku < sonnet < opus < fable (lower tier = more restrictive).
/// Unknown models are treated as tier 0 (most restrictive).
pub fn min_model_cap(a: &str, b: &str) -> String {
    let a_tier = model_tier(a);
    let b_tier = model_tier(b);
    if a_tier <= b_tier {
        a.to_lowercase()
    } else {
        b.to_lowercase()
    }
}

// ── Constraint resolution ───────────────────────────────────────────────

/// Get effective constraints for a process, merging with defaults.
///
/// Priority: process-specific > `_defaults` > hardcoded defaults.
pub fn get_team_constraints(config: &TeamConstraintsConfig, process: &str) -> TeamConstraints {
    let defaults = config.defaults.as_ref();
    let process_specific = config.processes.get(process);

    match (process_specific, defaults) {
        (Some(proc), Some(def)) => merge_constraints(def, proc),
        (Some(proc), None) => proc.clone(),
        (None, Some(def)) => def.clone(),
        (None, None) => TeamConstraints::default(),
    }
}

/// Merge default constraints with process-specific overrides.
/// Process-specific values take precedence; empty collections fall through to defaults.
fn merge_constraints(defaults: &TeamConstraints, specific: &TeamConstraints) -> TeamConstraints {
    TeamConstraints {
        max_teammates: specific.max_teammates,
        allowed_tools: if specific.allowed_tools.is_empty() {
            defaults.allowed_tools.clone()
        } else {
            specific.allowed_tools.clone()
        },
        allowed_mcp_tools: if specific.allowed_mcp_tools.is_empty() {
            defaults.allowed_mcp_tools.clone()
        } else {
            specific.allowed_mcp_tools.clone()
        },
        model_cap: specific.model_cap.clone(),
        mode: specific.mode.clone(),
        presets: if specific.presets.is_empty() {
            defaults.presets.clone()
        } else {
            specific.presets.clone()
        },
        timeout_minutes: specific.timeout_minutes,
        budget_limit: specific.budget_limit.or(defaults.budget_limit),
        auto_approve: Some(
            specific
                .auto_approve
                .or(defaults.auto_approve)
                .unwrap_or(true),
        ),
    }
}

/// Validate child session's team config against a constraint ceiling.
///
/// Caps values at min(resolved, ceiling) to prevent privilege escalation.
/// Used when a child session inherits team config from its parent.
///
/// # Rules
/// - `max_teammates`: min(resolved, ceiling)
/// - `model_cap`: lesser model tier (haiku < sonnet < opus < fable)
/// - `allowed_tools`: intersection of both lists (empty ceiling = no restriction)
/// - `allowed_mcp_tools`: intersection of both lists
/// - `presets`: intersection of both lists
/// - `timeout_minutes`: min(resolved, ceiling)
/// - `budget_limit`: min(resolved, ceiling); None is most restrictive
/// - `mode`: inherited from resolved (not capped)
/// - `auto_approve`: inherited from ceiling (parent controls whether child can auto-approve)
pub fn validate_child_team_config(
    resolved: &TeamConstraints,
    ceiling: &TeamConstraints,
) -> TeamConstraints {
    TeamConstraints {
        max_teammates: resolved.max_teammates.min(ceiling.max_teammates),
        allowed_tools: intersect_lists(&resolved.allowed_tools, &ceiling.allowed_tools),
        allowed_mcp_tools: intersect_lists(&resolved.allowed_mcp_tools, &ceiling.allowed_mcp_tools),
        model_cap: min_model_cap(&resolved.model_cap, &ceiling.model_cap),
        mode: resolved.mode.clone(),
        presets: intersect_lists(&resolved.presets, &ceiling.presets),
        timeout_minutes: resolved.timeout_minutes.min(ceiling.timeout_minutes),
        budget_limit: min_budget(resolved.budget_limit, ceiling.budget_limit),
        auto_approve: ceiling.auto_approve,
    }
}

/// Intersect two lists. If ceiling is empty, return resolved (no restriction).
fn intersect_lists(resolved: &[String], ceiling: &[String]) -> Vec<String> {
    if ceiling.is_empty() {
        return resolved.to_vec();
    }
    resolved
        .iter()
        .filter(|item| ceiling.contains(item))
        .cloned()
        .collect()
}

/// Return the lesser of two optional budgets. None = no limit (most permissive).
fn min_budget(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    match (a, b) {
        (Some(a_val), Some(b_val)) => Some(a_val.min(b_val)),
        (Some(a_val), None) => Some(a_val),
        (None, Some(_)) => None, // None = no budget allowed (most restrictive)
        (None, None) => None,
    }
}

// ── Process agent resolution ────────────────────────────────────────────

/// Resolve which agent to use for a process + variant combination.
///
/// Fallback chain: process_mapping variant → process_mapping default → None.
pub fn resolve_process_agent(
    mapping: &ProcessMapping,
    process: &str,
    variant: &str,
) -> Option<String> {
    let slot = mapping.slots.get(process)?;

    if variant == "default" {
        return Some(slot.default.clone());
    }

    slot.variants
        .get(variant)
        .cloned()
        .or_else(|| Some(slot.default.clone()))
}

// ── Team plan validation ────────────────────────────────────────────────

/// Validate an entire team composition plan against constraints.
pub fn validate_team_plan(
    constraints: &TeamConstraints,
    process: &str,
    teammates: &[TeammateSpawnRequest],
) -> Result<ApprovedTeamPlan, TeamConstraintError> {
    // 1. Check teammate count
    if teammates.len() > constraints.max_teammates as usize {
        return Err(TeamConstraintError::MaxTeammatesExceeded {
            max: constraints.max_teammates,
            requested: teammates.len(),
        });
    }

    // 2. Per-teammate validation
    let mut approved = Vec::with_capacity(teammates.len());
    for req in teammates {
        let teammate = validate_teammate(constraints, req)?;
        approved.push(teammate);
    }

    Ok(ApprovedTeamPlan {
        plan_id: format!("tp-{}", uuid::Uuid::new_v4().simple()),
        process: process.to_string(),
        teammates: approved,
    })
}

fn validate_teammate(
    constraints: &TeamConstraints,
    req: &TeammateSpawnRequest,
) -> Result<ApprovedTeammate, TeamConstraintError> {
    match constraints.mode {
        TeamMode::Constrained => {
            let preset =
                req.preset
                    .as_ref()
                    .ok_or_else(|| TeamConstraintError::PresetRequired {
                        role: req.role.clone(),
                    })?;
            if !constraints.presets.contains(preset) {
                return Err(TeamConstraintError::AgentNotInPresets {
                    agent: preset.clone(),
                    allowed: constraints.presets.clone(),
                });
            }
            Ok(ApprovedTeammate {
                role: req.role.clone(),
                approved_tools: Vec::new(), // filled from preset config at spawn time
                approved_mcp_tools: Vec::new(),
                approved_model: String::new(),
                from_preset: true,
            })
        }
        TeamMode::Dynamic => {
            // Validate CLI tools
            if !constraints.allowed_tools.is_empty() {
                for tool in &req.tools {
                    if !constraints.allowed_tools.contains(tool) {
                        return Err(TeamConstraintError::ToolNotAllowed {
                            tool: tool.clone(),
                            role: req.role.clone(),
                        });
                    }
                }
            }
            // Validate MCP tools
            if !constraints.allowed_mcp_tools.is_empty() {
                for tool in &req.mcp_tools {
                    if !constraints.allowed_mcp_tools.contains(tool) {
                        return Err(TeamConstraintError::McpToolNotAllowed {
                            tool: tool.clone(),
                            role: req.role.clone(),
                        });
                    }
                }
            }
            // Validate model tier
            if !model_within_cap(&req.model, &constraints.model_cap) {
                return Err(TeamConstraintError::ModelExceedsCap {
                    requested: req.model.clone(),
                    cap: constraints.model_cap.clone(),
                });
            }
            Ok(ApprovedTeammate {
                role: req.role.clone(),
                approved_tools: req.tools.clone(),
                approved_mcp_tools: req.mcp_tools.clone(),
                approved_model: req.model.clone(),
                from_preset: req.preset.is_some(),
            })
        }
    }
}

// ── Environment variable overrides ──────────────────────────────────────

/// Apply environment variable overrides to team constraints.
///
/// Supported vars:
/// - `RALPHX_TEAM_MODE_<PROCESS>` → override mode (dynamic|constrained)
/// - `RALPHX_TEAM_MAX_<PROCESS>` → override max_teammates
/// - `RALPHX_TEAM_MODEL_CAP_<PROCESS>` → override model_cap
pub fn apply_env_overrides(constraints: &mut TeamConstraints, process: &str) {
    apply_env_overrides_with(constraints, process, &|name| std::env::var(name).ok());
}

fn apply_env_overrides_with(
    constraints: &mut TeamConstraints,
    process: &str,
    lookup: &dyn Fn(&str) -> Option<String>,
) {
    let key = process.to_ascii_uppercase();

    if let Some(mode) = lookup(&format!("RALPHX_TEAM_MODE_{key}")) {
        match mode.to_lowercase().as_str() {
            "dynamic" => constraints.mode = TeamMode::Dynamic,
            "constrained" => constraints.mode = TeamMode::Constrained,
            _ => {
                tracing::warn!(var = %format!("RALPHX_TEAM_MODE_{key}"), value = %mode, "Invalid team mode")
            }
        }
    }

    if let Some(max_str) = lookup(&format!("RALPHX_TEAM_MAX_{key}")) {
        if let Ok(max) = max_str.parse::<u8>() {
            constraints.max_teammates = max;
        }
    }

    if let Some(cap) = lookup(&format!("RALPHX_TEAM_MODEL_CAP_{key}")) {
        let trimmed = cap.trim().to_lowercase();
        if matches!(trimmed.as_str(), "haiku" | "sonnet" | "opus" | "fable") {
            constraints.model_cap = trimmed;
        }
    }
}

/// Read process variant override from env.
///
/// `RALPHX_PROCESS_VARIANT_<PROCESS>` → variant name (e.g. "team")
pub fn env_variant_override(process: &str) -> Option<String> {
    env_variant_override_with(process, &|name| std::env::var(name).ok())
}

fn env_variant_override_with(
    process: &str,
    lookup: &dyn Fn(&str) -> Option<String>,
) -> Option<String> {
    let key = format!("RALPHX_PROCESS_VARIANT_{}", process.to_ascii_uppercase());
    lookup(&key).and_then(|v| {
        let trimmed = v.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "team_config_tests.rs"]
mod tests;
