use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::entities::ProjectId;

/// Persistence policy for a ticket branch binding.
///
/// Legacy canonical-base rows retain their historical terminal semantics.
/// Strict Git-convention bindings keep a stable branch across multiple PR
/// cycles and therefore use [`TicketCanonicalBranchCycleState`] instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TicketCanonicalBranchPolicyKind {
    LegacyCanonicalBase,
    StrictGitConvention,
}

impl std::fmt::Display for TicketCanonicalBranchPolicyKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::LegacyCanonicalBase => "legacy_canonical_base",
            Self::StrictGitConvention => "strict_git_convention",
        })
    }
}

impl FromStr for TicketCanonicalBranchPolicyKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "legacy_canonical_base" => Ok(Self::LegacyCanonicalBase),
            "strict_git_convention" => Ok(Self::StrictGitConvention),
            _ => Err(format!("unknown ticket canonical branch policy: '{value}'")),
        }
    }
}

/// Frozen provider-neutral convention values captured when a strict ticket is
/// first bound. Template and task/account changes must never rewrite this data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TicketGitConventionSnapshot {
    pub policy_version: i64,
    pub task_title: String,
    pub username: Option<String>,
    /// All ticket placeholders are rendered; only the optional `:summary:`
    /// marker may remain dynamic for individual commits.
    pub commit_subject_rule: String,
    pub pr_title: String,
}

/// Lifecycle state for the current strict-branch PR cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TicketCanonicalBranchCycleState {
    /// Sentinel used only by pre-feature legacy rows.
    Legacy,
    /// Binding exists and workspace provisioning/recovery is not yet complete.
    Preparing,
    /// A workspace owns the current cycle.
    Active,
    /// The current PR was proven merged and may become eligible for rollover.
    Merged,
    /// The current PR closed without merge; reuse must fail closed.
    ClosedUnmerged,
    /// Recovery evidence is incomplete or unsafe; operator action is required.
    Blocked,
}

impl std::fmt::Display for TicketCanonicalBranchCycleState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Legacy => "legacy",
            Self::Preparing => "preparing",
            Self::Active => "active",
            Self::Merged => "merged",
            Self::ClosedUnmerged => "closed_unmerged",
            Self::Blocked => "blocked",
        })
    }
}

impl FromStr for TicketCanonicalBranchCycleState {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "legacy" => Ok(Self::Legacy),
            "preparing" => Ok(Self::Preparing),
            "active" => Ok(Self::Active),
            "merged" => Ok(Self::Merged),
            "closed_unmerged" => Ok(Self::ClosedUnmerged),
            "blocked" => Ok(Self::Blocked),
            _ => Err(format!(
                "unknown ticket canonical branch cycle state: '{value}'"
            )),
        }
    }
}

/// Mutable, generation-guarded state for one strict ticket PR cycle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TicketCanonicalBranchCycle {
    pub generation: i64,
    pub state: TicketCanonicalBranchCycleState,
    pub base_commit: Option<String>,
    pub effective_merge_base: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub terminal_at: Option<DateTime<Utc>>,
}

impl TicketCanonicalBranchCycle {
    fn legacy() -> Self {
        Self {
            generation: 0,
            state: TicketCanonicalBranchCycleState::Legacy,
            base_commit: None,
            effective_merge_base: None,
            started_at: None,
            terminal_at: None,
        }
    }

    fn first_strict(base_commit: Option<String>, started_at: DateTime<Utc>) -> Self {
        Self {
            generation: 1,
            state: TicketCanonicalBranchCycleState::Preparing,
            base_commit,
            effective_merge_base: None,
            started_at: Some(started_at),
            terminal_at: None,
        }
    }

    pub fn validate_strict(&self) -> Result<(), String> {
        if self.generation < 1 {
            return Err("strict ticket branch cycle generation must be positive".to_string());
        }
        if self.state == TicketCanonicalBranchCycleState::Legacy {
            return Err("strict ticket branch cycle cannot use legacy state".to_string());
        }
        if self.started_at.is_none() {
            return Err("strict ticket branch cycle must record when it started".to_string());
        }
        if matches!(
            self.state,
            TicketCanonicalBranchCycleState::Active
                | TicketCanonicalBranchCycleState::Merged
                | TicketCanonicalBranchCycleState::ClosedUnmerged
        ) && self.base_commit.as_deref().is_none_or(str::is_empty)
        {
            return Err("active or terminal strict cycle must record its base commit".to_string());
        }
        if matches!(
            self.state,
            TicketCanonicalBranchCycleState::Merged
                | TicketCanonicalBranchCycleState::ClosedUnmerged
        ) && self.terminal_at.is_none()
        {
            return Err("terminal strict cycle must record when it became terminal".to_string());
        }
        if matches!(
            self.state,
            TicketCanonicalBranchCycleState::Preparing | TicketCanonicalBranchCycleState::Active
        ) && self.terminal_at.is_some()
        {
            return Err("non-terminal strict cycle cannot have a terminal timestamp".to_string());
        }
        Ok(())
    }

    pub fn validate_replacement(&self, expected_generation: i64) -> Result<(), String> {
        self.validate_strict()?;
        if self.generation == expected_generation {
            return Ok(());
        }
        if expected_generation.checked_add(1) == Some(self.generation)
            && self.state == TicketCanonicalBranchCycleState::Preparing
        {
            return Ok(());
        }
        Err(format!(
            "strict ticket cycle replacement must stay at generation {expected_generation} or prepare generation {}",
            expected_generation.saturating_add(1)
        ))
    }
}

/// The single canonical branch that all RalphX conversations for a given ticket
/// base off of, so work for one ticket converges on one branch via PR merge.
///
/// The canonical branch is never checked out into a worktree — each conversation
/// gets its own per-conversation branch based off this canonical branch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TicketCanonicalBranch {
    pub project_id: ProjectId,
    /// Provider slug, e.g. `jira` | `linear`.
    pub provider: String,
    /// Provider issue key, matching the link tables' `issue_key`.
    pub issue_key: String,
    /// The canonical branch name (e.g. `ralphx/ticket/linear-wise-24`).
    pub branch_name: String,
    /// Project default branch at canonical-branch creation time.
    pub base_branch: String,
    /// SHA the canonical branch was forged at.
    pub base_commit: Option<String>,
    /// `true` once the canonical branch has been confirmed pushed to origin.
    pub origin_pushed: bool,
    /// `true` once the canonical branch has been merged/closed; it must never be resurrected.
    pub terminal: bool,
    pub policy_kind: TicketCanonicalBranchPolicyKind,
    pub strict_policy: Option<TicketGitConventionSnapshot>,
    pub cycle: TicketCanonicalBranchCycle,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl TicketCanonicalBranch {
    /// Construct a freshly-forged canonical branch row (not yet pushed, not terminal).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        project_id: ProjectId,
        provider: impl Into<String>,
        issue_key: impl Into<String>,
        branch_name: impl Into<String>,
        base_branch: impl Into<String>,
        base_commit: Option<String>,
        created_at: DateTime<Utc>,
    ) -> Self {
        Self {
            project_id,
            provider: provider.into(),
            issue_key: issue_key.into(),
            branch_name: branch_name.into(),
            base_branch: base_branch.into(),
            base_commit,
            origin_pushed: false,
            terminal: false,
            policy_kind: TicketCanonicalBranchPolicyKind::LegacyCanonicalBase,
            strict_policy: None,
            cycle: TicketCanonicalBranchCycle::legacy(),
            created_at,
            updated_at: created_at,
        }
    }

    /// Construct a first-cycle strict binding with an immutable policy snapshot.
    #[allow(clippy::too_many_arguments)]
    pub fn new_strict(
        project_id: ProjectId,
        provider: impl Into<String>,
        issue_key: impl Into<String>,
        branch_name: impl Into<String>,
        base_branch: impl Into<String>,
        base_commit: Option<String>,
        strict_policy: TicketGitConventionSnapshot,
        created_at: DateTime<Utc>,
    ) -> Self {
        Self {
            project_id,
            provider: provider.into(),
            issue_key: issue_key.into(),
            branch_name: branch_name.into(),
            base_branch: base_branch.into(),
            base_commit: base_commit.clone(),
            origin_pushed: false,
            terminal: false,
            policy_kind: TicketCanonicalBranchPolicyKind::StrictGitConvention,
            strict_policy: Some(strict_policy),
            cycle: TicketCanonicalBranchCycle::first_strict(base_commit, created_at),
            created_at,
            updated_at: created_at,
        }
    }

    /// Validate cross-field persistence invariants before a repository write.
    pub fn validate_policy(&self) -> Result<(), String> {
        match self.policy_kind {
            TicketCanonicalBranchPolicyKind::LegacyCanonicalBase => {
                if self.strict_policy.is_some()
                    || self.cycle.generation != 0
                    || self.cycle.state != TicketCanonicalBranchCycleState::Legacy
                {
                    return Err(
                        "legacy canonical branch cannot contain strict policy or cycle state"
                            .to_string(),
                    );
                }
            }
            TicketCanonicalBranchPolicyKind::StrictGitConvention => {
                let policy = self.strict_policy.as_ref().ok_or_else(|| {
                    "strict ticket branch must contain a frozen policy snapshot".to_string()
                })?;
                if policy.policy_version < 1 {
                    return Err("strict ticket policy version must be positive".to_string());
                }
                if policy.task_title.trim().is_empty()
                    || policy.commit_subject_rule.trim().is_empty()
                    || policy.pr_title.trim().is_empty()
                {
                    return Err(
                        "strict ticket policy title, commit rule, and PR title cannot be empty"
                            .to_string(),
                    );
                }
                if policy
                    .username
                    .as_ref()
                    .is_some_and(|value| value.trim().is_empty())
                {
                    return Err("strict ticket policy username cannot be blank".to_string());
                }
                if self.terminal {
                    return Err(
                        "strict ticket branch cannot use the legacy terminal flag".to_string()
                    );
                }
                self.cycle.validate_strict()?;
            }
        }
        Ok(())
    }
}
