// Project entity - represents a development project in RalphX
// Contains project configuration, git settings, and metadata

use chrono::{DateTime, TimeZone, Utc};
use rusqlite::Row;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

use super::ProjectId;

/// Git workflow mode for a project
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitMode {
    /// Use git worktrees for isolated development
    #[serde(alias = "local")]
    Worktree,
}

impl Default for GitMode {
    fn default() -> Self {
        Self::Worktree
    }
}

impl std::fmt::Display for GitMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GitMode::Worktree => write!(f, "worktree"),
        }
    }
}

/// Error type for parsing GitMode from a string
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseGitModeError {
    pub value: String,
}

impl std::fmt::Display for ParseGitModeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown git mode: '{}'", self.value)
    }
}

impl std::error::Error for ParseGitModeError {}

impl FromStr for GitMode {
    type Err = ParseGitModeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "local" | "worktree" => Ok(GitMode::Worktree),
            _ => Err(ParseGitModeError {
                value: s.to_string(),
            }),
        }
    }
}

fn default_use_feature_branches() -> bool {
    true
}

fn default_github_pr_enabled() -> bool {
    true
}

/// Merge strategy for combining branches
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MergeStrategy {
    /// Rebase source onto target, then fast-forward merge (linear history)
    Rebase,
    /// Direct merge commit (non-linear)
    Merge,
    /// Squash all commits into a single commit on target (clean linear history)
    Squash,
    /// Rebase first (resolve conflicts), then squash into single commit (cleanest history)
    #[default]
    RebaseSquash,
}

impl std::fmt::Display for MergeStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MergeStrategy::Rebase => write!(f, "rebase"),
            MergeStrategy::Merge => write!(f, "merge"),
            MergeStrategy::Squash => write!(f, "squash"),
            MergeStrategy::RebaseSquash => write!(f, "rebase_squash"),
        }
    }
}

impl FromStr for MergeStrategy {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "rebase" => Ok(MergeStrategy::Rebase),
            "merge" => Ok(MergeStrategy::Merge),
            "squash" => Ok(MergeStrategy::Squash),
            "rebase_squash" => Ok(MergeStrategy::RebaseSquash),
            _ => Err(format!("unknown merge strategy: '{}'", s)),
        }
    }
}

/// Merge validation behavior mode
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MergeValidationMode {
    /// Validation failure → MergeIncomplete (user decides)
    Block,
    /// Validation failure → spawn merger agent to attempt fix, then fall back to Block
    AutoFix,
    /// Validation failure → proceed to Merged, store warnings
    Warn,
    /// Skip validation entirely
    #[default]
    Off,
}

impl std::fmt::Display for MergeValidationMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MergeValidationMode::Block => write!(f, "block"),
            MergeValidationMode::AutoFix => write!(f, "auto_fix"),
            MergeValidationMode::Warn => write!(f, "warn"),
            MergeValidationMode::Off => write!(f, "off"),
        }
    }
}

impl FromStr for MergeValidationMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "block" => Ok(MergeValidationMode::Block),
            "auto_fix" => Ok(MergeValidationMode::AutoFix),
            "warn" => Ok(MergeValidationMode::Warn),
            "off" => Ok(MergeValidationMode::Off),
            _ => Err(format!("unknown merge validation mode: '{}'", s)),
        }
    }
}

/// A development project managed by RalphX
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    /// Unique identifier for this project
    pub id: ProjectId,
    /// Human-readable project name
    pub name: String,
    /// Absolute path to the project's working directory
    pub working_directory: String,
    /// Git workflow mode (worktree)
    pub git_mode: GitMode,
    /// Base branch for comparisons (e.g., "main" or "master")
    pub base_branch: Option<String>,
    /// Parent directory for task worktrees (default: ~/ralphx-worktrees)
    pub worktree_parent_directory: Option<String>,
    /// Whether to use feature branches for plan groups (default: true)
    #[serde(default = "default_use_feature_branches")]
    pub use_feature_branches: bool,
    /// Merge validation behavior mode (block/warn/off)
    #[serde(default)]
    pub merge_validation_mode: MergeValidationMode,
    /// Merge strategy (rebase for linear history, merge for merge commits)
    #[serde(default)]
    pub merge_strategy: MergeStrategy,
    /// Auto-detected analysis commands (JSON array, written by analyzer agent)
    pub detected_analysis: Option<String>,
    /// User-overridden analysis commands (JSON array, written by user via Settings UI)
    pub custom_analysis: Option<String>,
    /// Last analysis timestamp (RFC3339)
    pub analyzed_at: Option<String>,
    /// User preference for future GitHub PR workflows; live repository capability authorizes each new PR flow.
    #[serde(default = "default_github_pr_enabled")]
    pub github_pr_enabled: bool,
    /// When the project was created
    pub created_at: DateTime<Utc>,
    /// When the project was last updated
    pub updated_at: DateTime<Utc>,
    /// When the project was archived (soft-deleted). None = active.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<DateTime<Utc>>,
}

impl Project {
    /// Creates a new project with the given name and working directory
    /// Uses sensible defaults for git mode (Worktree) and timestamps (now)
    pub fn new(name: String, working_directory: String) -> Self {
        let now = Utc::now();
        Self {
            id: ProjectId::new(),
            name,
            working_directory,
            git_mode: GitMode::default(),
            base_branch: None,
            worktree_parent_directory: None,
            use_feature_branches: true,
            merge_validation_mode: MergeValidationMode::default(),
            merge_strategy: MergeStrategy::default(),
            detected_analysis: None,
            custom_analysis: None,
            analyzed_at: None,
            // New projects require an explicit user opt-in in Repository Settings.
            // Keep the serde/database fallback above for legacy records that predate
            // the persisted preference.
            github_pr_enabled: false,
            created_at: now,
            updated_at: now,
            archived_at: None,
        }
    }

    /// Returns true if the project uses worktree mode
    pub fn is_worktree(&self) -> bool {
        self.git_mode == GitMode::Worktree
    }

    /// Returns the base branch, falling back to "main" if None or empty
    pub fn base_branch_or_default(&self) -> &str {
        self.base_branch
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or("main")
    }

    /// Returns the worktree parent directory, falling back to "~/ralphx-worktrees" if None or empty
    pub fn worktree_parent_or_default(&self) -> &str {
        self.worktree_parent_directory
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or("~/ralphx-worktrees")
    }

    /// Returns the full path to the task execution worktree for the given task ID.
    ///
    /// Convention: `{worktree_parent}/{slug}/task-{task_id}`
    /// where `slug` is the project name lowercased with non-alphanumeric chars replaced by `-`.
    pub fn task_worktree_path(&self, task_id: &str) -> std::path::PathBuf {
        let parent = self.worktree_parent_or_default();
        // Expand ~/  prefix to home directory
        let expanded = if let Some(stripped) = parent.strip_prefix("~/") {
            if let Ok(home) = std::env::var("HOME") {
                format!("{}/{}", home, stripped)
            } else {
                parent.to_string()
            }
        } else {
            parent.to_string()
        };
        // Slugify project name: lowercase, non-alphanumeric → '-', trim leading/trailing hyphens
        let slug = self
            .name
            .to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '-' })
            .collect::<String>()
            .trim_matches('-')
            .to_string();
        std::path::PathBuf::from(format!("{}/{}/task-{}", expanded, slug, task_id))
    }

    /// Updates the updated_at timestamp to now
    pub fn touch(&mut self) {
        self.updated_at = Utc::now();
    }

    /// Deserialize a Project from a SQLite row.
    /// Expects columns: id, name, working_directory, git_mode,
    /// base_branch, worktree_parent_directory, created_at, updated_at
    pub fn from_row(row: &Row) -> rusqlite::Result<Self> {
        Ok(Self {
            id: ProjectId::from_string(row.get("id")?),
            name: row.get("name")?,
            working_directory: row.get("working_directory")?,
            git_mode: row
                .get::<_, String>("git_mode")?
                .parse()
                .unwrap_or(GitMode::Worktree),
            base_branch: row.get("base_branch")?,
            worktree_parent_directory: row.get("worktree_parent_directory")?,
            use_feature_branches: row.get::<_, i64>("use_feature_branches").unwrap_or(1) != 0,
            merge_validation_mode: row
                .get::<_, String>("merge_validation_mode")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or_default(),
            merge_strategy: row
                .get::<_, String>("merge_strategy")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or_default(),
            detected_analysis: row.get("detected_analysis").unwrap_or(None),
            custom_analysis: row.get("custom_analysis").unwrap_or(None),
            analyzed_at: row.get("analyzed_at").unwrap_or(None),
            github_pr_enabled: row.get::<_, i64>("github_pr_enabled").unwrap_or(1) != 0,
            created_at: Self::parse_datetime(row.get("created_at")?),
            updated_at: Self::parse_datetime(row.get("updated_at")?),
            archived_at: row
                .get::<_, Option<String>>("archived_at")?
                .map(Self::parse_datetime),
        })
    }

    /// Parse a datetime string from SQLite into a DateTime<Utc>
    /// Handles both RFC3339 format and SQLite's CURRENT_TIMESTAMP format
    fn parse_datetime(s: String) -> DateTime<Utc> {
        // Try RFC3339 first (our preferred format)
        if let Ok(dt) = DateTime::parse_from_rfc3339(&s) {
            return dt.with_timezone(&Utc);
        }
        // Try SQLite's default datetime format (YYYY-MM-DD HH:MM:SS)
        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S") {
            return Utc.from_utc_datetime(&dt);
        }
        // Fallback to now if parsing fails
        Utc::now()
    }
}

#[cfg(test)]
#[path = "project_tests.rs"]
mod tests;
