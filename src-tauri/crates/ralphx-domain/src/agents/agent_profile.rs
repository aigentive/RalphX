// Agent Profile
// Complete agent configuration composed of Claude Code components

use serde::{Deserialize, Serialize};

/// Role of an agent in the RalphX system (for profiles)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileRole {
    Worker,
    Reviewer,
    Supervisor,
    Orchestrator,
    Researcher,
}

impl std::fmt::Display for ProfileRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProfileRole::Worker => write!(f, "worker"),
            ProfileRole::Reviewer => write!(f, "reviewer"),
            ProfileRole::Supervisor => write!(f, "supervisor"),
            ProfileRole::Orchestrator => write!(f, "orchestrator"),
            ProfileRole::Researcher => write!(f, "researcher"),
        }
    }
}

impl std::str::FromStr for ProfileRole {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "worker" => Ok(ProfileRole::Worker),
            "reviewer" => Ok(ProfileRole::Reviewer),
            "supervisor" => Ok(ProfileRole::Supervisor),
            "orchestrator" => Ok(ProfileRole::Orchestrator),
            "researcher" => Ok(ProfileRole::Researcher),
            _ => Err(format!("Invalid profile role: {}", s)),
        }
    }
}

/// Model short forms for Claude models
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Model {
    /// claude-opus-4-5-20251101
    Opus,
    /// claude-sonnet-4-6
    Sonnet,
    /// claude-haiku-4-5-20251001
    Haiku,
}

impl Model {
    /// Get the full model ID
    pub fn model_id(&self) -> &'static str {
        match self {
            Model::Opus => "claude-opus-4-5-20251101",
            Model::Sonnet => "claude-sonnet-4-6",
            Model::Haiku => "claude-haiku-4-5-20251001",
        }
    }
}

impl std::fmt::Display for Model {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Model::Opus => write!(f, "opus"),
            Model::Sonnet => write!(f, "sonnet"),
            Model::Haiku => write!(f, "haiku"),
        }
    }
}

/// Permission mode for agent execution
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(Default)]
pub enum PermissionMode {
    #[default]
    Default,
    AcceptEdits,
    BypassPermissions,
}

/// Autonomy level for agent behavior
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum AutonomyLevel {
    #[default]
    Supervised,
    SemiAutonomous,
    FullyAutonomous,
}

/// Claude Code component references
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeCodeConfig {
    /// Agent name (resolved via --plugin-dir against generated Claude assets derived from `agents/`)
    pub agent: String,
    /// Skills to inject at startup (resolved via plugin discovery)
    #[serde(default)]
    pub skills: Vec<String>,
    /// Agent-scoped hooks configuration (JSON)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hooks: Option<serde_json::Value>,
    /// MCP servers to enable
    #[serde(default)]
    pub mcp_servers: Vec<String>,
}

impl ClaudeCodeConfig {
    pub fn new(agent: impl Into<String>) -> Self {
        Self {
            agent: agent.into(),
            skills: vec![],
            hooks: None,
            mcp_servers: vec![],
        }
    }

    pub fn with_skills(mut self, skills: Vec<String>) -> Self {
        self.skills = skills;
        self
    }
}

/// Execution configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionConfig {
    /// Model to use
    pub model: Model,
    /// Maximum iterations before stopping
    pub max_iterations: u32,
    /// Timeout in minutes
    pub timeout_minutes: u32,
    /// Permission mode for file operations
    #[serde(default)]
    pub permission_mode: PermissionMode,
}

impl Default for ExecutionConfig {
    fn default() -> Self {
        Self {
            model: Model::Sonnet,
            max_iterations: 30,
            timeout_minutes: 30,
            permission_mode: PermissionMode::Default,
        }
    }
}

impl ExecutionConfig {
    pub fn with_model(mut self, model: Model) -> Self {
        self.model = model;
        self
    }

    pub fn with_max_iterations(mut self, max: u32) -> Self {
        self.max_iterations = max;
        self
    }

    pub fn with_timeout(mut self, minutes: u32) -> Self {
        self.timeout_minutes = minutes;
        self
    }

    pub fn with_permission_mode(mut self, mode: PermissionMode) -> Self {
        self.permission_mode = mode;
        self
    }
}

/// Artifact I/O configuration
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IoConfig {
    /// Types of artifacts this agent can consume
    #[serde(default)]
    pub input_artifact_types: Vec<String>,
    /// Types of artifacts this agent can produce
    #[serde(default)]
    pub output_artifact_types: Vec<String>,
}

impl IoConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_inputs(mut self, types: Vec<String>) -> Self {
        self.input_artifact_types = types;
        self
    }

    pub fn with_outputs(mut self, types: Vec<String>) -> Self {
        self.output_artifact_types = types;
        self
    }
}

/// Behavioral configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BehaviorConfig {
    /// Whether this agent can spawn sub-agents
    #[serde(default)]
    pub can_spawn_sub_agents: bool,
    /// Whether to auto-commit changes
    #[serde(default)]
    pub auto_commit: bool,
    /// Autonomy level
    #[serde(default)]
    pub autonomy_level: AutonomyLevel,
}

impl Default for BehaviorConfig {
    fn default() -> Self {
        Self {
            can_spawn_sub_agents: false,
            auto_commit: false,
            autonomy_level: AutonomyLevel::Supervised,
        }
    }
}

impl BehaviorConfig {
    pub fn with_sub_agents(mut self, allowed: bool) -> Self {
        self.can_spawn_sub_agents = allowed;
        self
    }

    pub fn with_auto_commit(mut self, enabled: bool) -> Self {
        self.auto_commit = enabled;
        self
    }

    pub fn with_autonomy(mut self, level: AutonomyLevel) -> Self {
        self.autonomy_level = level;
        self
    }
}

/// Complete agent profile
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProfile {
    /// Unique identifier
    pub id: String,
    /// Display name
    pub name: String,
    /// Description of what this agent does
    pub description: String,
    /// Role of this agent
    pub role: ProfileRole,
    /// Claude Code component configuration
    pub claude_code: ClaudeCodeConfig,
    /// Execution configuration
    pub execution: ExecutionConfig,
    /// Artifact I/O configuration
    pub io: IoConfig,
    /// Behavioral flags
    pub behavior: BehaviorConfig,
}

impl AgentProfile {
    /// Create a new agent profile
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        description: impl Into<String>,
        role: ProfileRole,
        agent_definition: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            description: description.into(),
            role,
            claude_code: ClaudeCodeConfig::new(agent_definition),
            execution: ExecutionConfig::default(),
            io: IoConfig::default(),
            behavior: BehaviorConfig::default(),
        }
    }

    /// Create the built-in worker profile
    pub fn worker() -> Self {
        Self {
            id: "worker".to_string(),
            name: "Worker".to_string(),
            description: "Executes implementation tasks autonomously".to_string(),
            role: ProfileRole::Worker,
            claude_code: ClaudeCodeConfig::new("worker").with_skills(vec![
                "coding-standards".to_string(),
                "testing-patterns".to_string(),
                "git-workflow".to_string(),
            ]),
            execution: ExecutionConfig::default()
                .with_model(Model::Sonnet)
                .with_max_iterations(30)
                .with_permission_mode(PermissionMode::AcceptEdits),
            io: IoConfig::default(),
            behavior: BehaviorConfig::default()
                .with_auto_commit(true)
                .with_autonomy(AutonomyLevel::SemiAutonomous),
        }
    }

    /// Create the built-in reviewer profile
    pub fn reviewer() -> Self {
        Self {
            id: "reviewer".to_string(),
            name: "Reviewer".to_string(),
            description: "Reviews code changes for quality and correctness".to_string(),
            role: ProfileRole::Reviewer,
            claude_code: ClaudeCodeConfig::new("reviewer")
                .with_skills(vec!["code-review-checklist".to_string()]),
            execution: ExecutionConfig::default()
                .with_model(Model::Sonnet)
                .with_max_iterations(10),
            io: IoConfig::default(),
            behavior: BehaviorConfig::default(),
        }
    }

    /// Create the built-in orchestrator profile
    pub fn orchestrator() -> Self {
        Self {
            id: "orchestrator".to_string(),
            name: "Orchestrator".to_string(),
            description: "Plans and coordinates complex multi-step tasks".to_string(),
            role: ProfileRole::Orchestrator,
            claude_code: ClaudeCodeConfig::new("orchestrator"),
            execution: ExecutionConfig::default()
                .with_model(Model::Opus)
                .with_max_iterations(50),
            io: IoConfig::default(),
            behavior: BehaviorConfig::default()
                .with_sub_agents(true)
                .with_autonomy(AutonomyLevel::FullyAutonomous),
        }
    }

    /// Create the built-in deep-researcher profile
    pub fn deep_researcher() -> Self {
        Self {
            id: "deep-researcher".to_string(),
            name: "Deep Researcher".to_string(),
            description: "Conducts thorough research and analysis".to_string(),
            role: ProfileRole::Researcher,
            claude_code: ClaudeCodeConfig::new("deep-researcher")
                .with_skills(vec!["research-methodology".to_string()]),
            execution: ExecutionConfig::default()
                .with_model(Model::Opus)
                .with_max_iterations(200),
            io: IoConfig::default(),
            behavior: BehaviorConfig::default().with_autonomy(AutonomyLevel::FullyAutonomous),
        }
    }

    /// Get all built-in profiles
    pub fn builtin_profiles() -> Vec<Self> {
        vec![
            Self::worker(),
            Self::reviewer(),
            Self::orchestrator(),
            Self::deep_researcher(),
        ]
    }
}

#[cfg(test)]
#[path = "agent_profile_tests.rs"]
mod tests;
