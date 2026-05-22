//! Type definitions for the ideation system
//! Includes enums, error types, and helper functions

use chrono::{DateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// Session-selected code ref used for all ideation-family analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdeationAnalysisBaseRefKind {
    /// The project's configured default branch.
    ProjectDefault,
    /// The branch currently checked out in the project root when the session starts.
    CurrentBranch,
    /// Another local branch selected by the user.
    LocalBranch,
    /// A pull request ref selected by the user.
    PullRequest,
}

impl std::fmt::Display for IdeationAnalysisBaseRefKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IdeationAnalysisBaseRefKind::ProjectDefault => write!(f, "project_default"),
            IdeationAnalysisBaseRefKind::CurrentBranch => write!(f, "current_branch"),
            IdeationAnalysisBaseRefKind::LocalBranch => write!(f, "local_branch"),
            IdeationAnalysisBaseRefKind::PullRequest => write!(f, "pull_request"),
        }
    }
}

impl FromStr for IdeationAnalysisBaseRefKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "project_default" => Ok(Self::ProjectDefault),
            "current_branch" => Ok(Self::CurrentBranch),
            "local_branch" => Ok(Self::LocalBranch),
            "pull_request" => Ok(Self::PullRequest),
            _ => Err(format!("unknown ideation analysis base ref kind: '{s}'")),
        }
    }
}

/// Workspace shape used by ideation-family agents for a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdeationAnalysisWorkspaceKind {
    /// The project root is the correct analysis checkout.
    ProjectRoot,
    /// A dedicated ideation worktree was provisioned for the selected base.
    IdeationWorktree,
}

impl Default for IdeationAnalysisWorkspaceKind {
    fn default() -> Self {
        Self::ProjectRoot
    }
}

impl std::fmt::Display for IdeationAnalysisWorkspaceKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IdeationAnalysisWorkspaceKind::ProjectRoot => write!(f, "project_root"),
            IdeationAnalysisWorkspaceKind::IdeationWorktree => write!(f, "ideation_worktree"),
        }
    }
}

impl FromStr for IdeationAnalysisWorkspaceKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "project_root" => Ok(Self::ProjectRoot),
            "ideation_worktree" => Ok(Self::IdeationWorktree),
            _ => Err(format!("unknown ideation analysis workspace kind: '{s}'")),
        }
    }
}

/// Immutable analysis base and workspace state attached to an ideation session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdeationAnalysisState {
    pub base_ref_kind: Option<IdeationAnalysisBaseRefKind>,
    pub base_ref: Option<String>,
    pub base_display_name: Option<String>,
    pub workspace_kind: IdeationAnalysisWorkspaceKind,
    pub workspace_path: Option<String>,
    pub base_commit: Option<String>,
    pub base_locked_at: Option<DateTime<Utc>>,
}

impl Default for IdeationAnalysisState {
    fn default() -> Self {
        Self {
            base_ref_kind: None,
            base_ref: None,
            base_display_name: None,
            workspace_kind: IdeationAnalysisWorkspaceKind::ProjectRoot,
            workspace_path: None,
            base_commit: None,
            base_locked_at: None,
        }
    }
}

impl IdeationAnalysisState {
    pub fn requires_dedicated_workspace(&self) -> bool {
        self.workspace_kind == IdeationAnalysisWorkspaceKind::IdeationWorktree
    }
}

/// Origin of an ideation session — distinguishes internally created sessions from externally created ones
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionOrigin {
    /// Session created by an internal RalphX user or agent (default)
    Internal,
    /// Session created by an external agent via the External MCP API
    External,
}

impl Default for SessionOrigin {
    fn default() -> Self {
        Self::Internal
    }
}

impl std::fmt::Display for SessionOrigin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionOrigin::Internal => write!(f, "internal"),
            SessionOrigin::External => write!(f, "external"),
        }
    }
}

impl FromStr for SessionOrigin {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "internal" => Ok(SessionOrigin::Internal),
            "external" => Ok(SessionOrigin::External),
            _ => Err(format!("unknown session origin: '{s}'")),
        }
    }
}

/// Purpose of an ideation session — distinguishes general ideation from ralphx-plan-verifier child sessions
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionPurpose {
    /// Standard ideation session (default)
    General,
    /// Child session spawned by the ralphx-plan-verifier agent
    Verification,
}

impl Default for SessionPurpose {
    fn default() -> Self {
        Self::General
    }
}

impl std::fmt::Display for SessionPurpose {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionPurpose::General => write!(f, "general"),
            SessionPurpose::Verification => write!(f, "verification"),
        }
    }
}

impl FromStr for SessionPurpose {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "general" => Ok(SessionPurpose::General),
            "verification" => Ok(SessionPurpose::Verification),
            _ => Err(format!("unknown session purpose: '{s}'")),
        }
    }
}

/// User-facing flow for an ideation-family session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdeationSessionFlow {
    /// Existing full ideation flow: plan, proposals, and task acceptance.
    Ideation,
    /// Plan-mode flow: plan refinement first, with optional later promotion.
    Planning,
}

impl Default for IdeationSessionFlow {
    fn default() -> Self {
        Self::Ideation
    }
}

impl std::fmt::Display for IdeationSessionFlow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IdeationSessionFlow::Ideation => write!(f, "ideation"),
            IdeationSessionFlow::Planning => write!(f, "planning"),
        }
    }
}

impl FromStr for IdeationSessionFlow {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ideation" => Ok(Self::Ideation),
            "planning" => Ok(Self::Planning),
            _ => Err(format!("unknown ideation session flow: '{s}'")),
        }
    }
}

/// Status of an ideation session
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdeationSessionStatus {
    /// Session is currently being worked on
    Active,
    /// Session has been archived (completed or paused for later)
    Archived,
    /// All proposals from this session have been accepted and applied to Kanban
    Accepted,
}

impl Default for IdeationSessionStatus {
    fn default() -> Self {
        Self::Active
    }
}

impl std::fmt::Display for IdeationSessionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IdeationSessionStatus::Active => write!(f, "active"),
            IdeationSessionStatus::Archived => write!(f, "archived"),
            IdeationSessionStatus::Accepted => write!(f, "accepted"),
        }
    }
}

/// Error type for parsing IdeationSessionStatus from a string
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseIdeationSessionStatusError {
    pub value: String,
}

impl std::fmt::Display for ParseIdeationSessionStatusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown ideation session status: '{}'", self.value)
    }
}

impl std::error::Error for ParseIdeationSessionStatusError {}

impl FromStr for IdeationSessionStatus {
    type Err = ParseIdeationSessionStatusError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "active" => Ok(IdeationSessionStatus::Active),
            "archived" => Ok(IdeationSessionStatus::Archived),
            "accepted" => Ok(IdeationSessionStatus::Accepted),
            _ => Err(ParseIdeationSessionStatusError {
                value: s.to_string(),
            }),
        }
    }
}

/// Suggested priority level for a task proposal
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Critical,
    High,
    Medium,
    Low,
}

impl Default for Priority {
    fn default() -> Self {
        Self::Medium
    }
}

impl std::fmt::Display for Priority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Priority::Critical => write!(f, "critical"),
            Priority::High => write!(f, "high"),
            Priority::Medium => write!(f, "medium"),
            Priority::Low => write!(f, "low"),
        }
    }
}

/// Error type for parsing Priority from a string
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsePriorityError {
    pub value: String,
}

impl std::fmt::Display for ParsePriorityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown priority: '{}'", self.value)
    }
}

impl std::error::Error for ParsePriorityError {}

impl FromStr for Priority {
    type Err = ParsePriorityError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "critical" => Ok(Priority::Critical),
            "high" => Ok(Priority::High),
            "medium" => Ok(Priority::Medium),
            "low" => Ok(Priority::Low),
            _ => Err(ParsePriorityError {
                value: s.to_string(),
            }),
        }
    }
}

/// Estimated complexity of a task
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Complexity {
    Trivial,
    Simple,
    Moderate,
    Complex,
    VeryComplex,
}

impl Default for Complexity {
    fn default() -> Self {
        Self::Moderate
    }
}

impl std::fmt::Display for Complexity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Complexity::Trivial => write!(f, "trivial"),
            Complexity::Simple => write!(f, "simple"),
            Complexity::Moderate => write!(f, "moderate"),
            Complexity::Complex => write!(f, "complex"),
            Complexity::VeryComplex => write!(f, "very_complex"),
        }
    }
}

/// Error type for parsing Complexity from a string
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseComplexityError {
    pub value: String,
}

impl std::fmt::Display for ParseComplexityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown complexity: '{}'", self.value)
    }
}

impl std::error::Error for ParseComplexityError {}

impl FromStr for Complexity {
    type Err = ParseComplexityError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "trivial" => Ok(Complexity::Trivial),
            "simple" => Ok(Complexity::Simple),
            "moderate" => Ok(Complexity::Moderate),
            "complex" => Ok(Complexity::Complex),
            "very_complex" => Ok(Complexity::VeryComplex),
            _ => Err(ParseComplexityError {
                value: s.to_string(),
            }),
        }
    }
}

/// Status of a task proposal in the ideation workflow
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalStatus {
    /// Proposal is pending review
    Pending,
    /// Proposal has been accepted and will be converted to a task
    Accepted,
    /// Proposal has been rejected
    Rejected,
    /// Proposal has been modified by the user
    Modified,
}

impl Default for ProposalStatus {
    fn default() -> Self {
        Self::Pending
    }
}

impl std::fmt::Display for ProposalStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProposalStatus::Pending => write!(f, "pending"),
            ProposalStatus::Accepted => write!(f, "accepted"),
            ProposalStatus::Rejected => write!(f, "rejected"),
            ProposalStatus::Modified => write!(f, "modified"),
        }
    }
}

/// Error type for parsing ProposalStatus from a string
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseProposalStatusError {
    pub value: String,
}

impl std::fmt::Display for ParseProposalStatusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown proposal status: '{}'", self.value)
    }
}

impl std::error::Error for ParseProposalStatusError {}

impl FromStr for ProposalStatus {
    type Err = ParseProposalStatusError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(ProposalStatus::Pending),
            "accepted" => Ok(ProposalStatus::Accepted),
            "rejected" => Ok(ProposalStatus::Rejected),
            "modified" => Ok(ProposalStatus::Modified),
            _ => Err(ParseProposalStatusError {
                value: s.to_string(),
            }),
        }
    }
}

/// Category of a task proposal
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalCategory {
    /// Initial project setup
    Setup,
    /// New feature implementation
    Feature,
    /// Bug fix
    Fix,
    /// Code refactoring
    Refactor,
    /// Documentation
    Docs,
    /// Testing
    Test,
    /// Performance optimization
    Performance,
    /// Security-related
    Security,
    /// DevOps/CI/CD
    DevOps,
    /// Research/investigation
    Research,
    /// Design work
    Design,
    /// Chore/maintenance
    Chore,
}

impl Default for ProposalCategory {
    fn default() -> Self {
        Self::Feature
    }
}

impl std::fmt::Display for ProposalCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProposalCategory::Setup => write!(f, "setup"),
            ProposalCategory::Feature => write!(f, "feature"),
            ProposalCategory::Fix => write!(f, "fix"),
            ProposalCategory::Refactor => write!(f, "refactor"),
            ProposalCategory::Docs => write!(f, "docs"),
            ProposalCategory::Test => write!(f, "test"),
            ProposalCategory::Performance => write!(f, "performance"),
            ProposalCategory::Security => write!(f, "security"),
            ProposalCategory::DevOps => write!(f, "devops"),
            ProposalCategory::Research => write!(f, "research"),
            ProposalCategory::Design => write!(f, "design"),
            ProposalCategory::Chore => write!(f, "chore"),
        }
    }
}

/// Error type for parsing ProposalCategory from a string
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseProposalCategoryError {
    pub value: String,
}

impl std::fmt::Display for ParseProposalCategoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown proposal category: '{}'", self.value)
    }
}

impl std::error::Error for ParseProposalCategoryError {}

impl FromStr for ProposalCategory {
    type Err = ParseProposalCategoryError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "setup" => Ok(ProposalCategory::Setup),
            "feature" => Ok(ProposalCategory::Feature),
            "fix" => Ok(ProposalCategory::Fix),
            "refactor" => Ok(ProposalCategory::Refactor),
            "docs" => Ok(ProposalCategory::Docs),
            "test" => Ok(ProposalCategory::Test),
            "performance" => Ok(ProposalCategory::Performance),
            "security" => Ok(ProposalCategory::Security),
            "devops" => Ok(ProposalCategory::DevOps),
            "research" => Ok(ProposalCategory::Research),
            "design" => Ok(ProposalCategory::Design),
            "chore" => Ok(ProposalCategory::Chore),
            _ => Err(ParseProposalCategoryError {
                value: s.to_string(),
            }),
        }
    }
}

/// Priority scoring factors used for automated prioritization
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PriorityFactors {
    /// Score from dependency analysis (blocks other tasks)
    #[serde(default)]
    pub dependency: i32,
    /// Score from business value
    #[serde(default)]
    pub business_value: i32,
    /// Score from technical risk
    #[serde(default)]
    pub technical_risk: i32,
    /// Score from user request frequency
    #[serde(default)]
    pub user_demand: i32,
}

// ---------------------------------------------------------------------------
// Verification types
// ---------------------------------------------------------------------------

/// Verification status of an ideation session's plan
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStatus {
    /// Plan has not been verified yet
    Unverified,
    /// Verification loop is currently running
    Reviewing,
    /// Plan passed all verification rounds (0 critical gaps)
    Verified,
    /// Critic found gaps; plan needs revision
    NeedsRevision,
    /// User explicitly skipped verification
    Skipped,
    /// Plan was imported from another project and is pre-verified
    ImportedVerified,
}

impl Default for VerificationStatus {
    fn default() -> Self {
        Self::Unverified
    }
}

impl std::fmt::Display for VerificationStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VerificationStatus::Unverified => write!(f, "unverified"),
            VerificationStatus::Reviewing => write!(f, "reviewing"),
            VerificationStatus::Verified => write!(f, "verified"),
            VerificationStatus::NeedsRevision => write!(f, "needs_revision"),
            VerificationStatus::Skipped => write!(f, "skipped"),
            VerificationStatus::ImportedVerified => write!(f, "imported_verified"),
        }
    }
}

/// Error type for parsing VerificationStatus from a string
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseVerificationStatusError {
    pub value: String,
}

impl std::fmt::Display for ParseVerificationStatusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown verification status: '{}'", self.value)
    }
}

impl std::error::Error for ParseVerificationStatusError {}

impl FromStr for VerificationStatus {
    type Err = ParseVerificationStatusError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "unverified" => Ok(VerificationStatus::Unverified),
            "reviewing" => Ok(VerificationStatus::Reviewing),
            "verified" => Ok(VerificationStatus::Verified),
            "needs_revision" => Ok(VerificationStatus::NeedsRevision),
            "skipped" => Ok(VerificationStatus::Skipped),
            "imported_verified" => Ok(VerificationStatus::ImportedVerified),
            _ => Err(ParseVerificationStatusError {
                value: s.to_string(),
            }),
        }
    }
}

/// A single gap identified by the critic during a verification round
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VerificationGap {
    /// Severity: "critical" | "high" | "medium" | "low"
    pub severity: String,
    /// Category for display grouping (e.g., "security", "architecture")
    pub category: String,
    /// Human-readable description of the gap
    pub description: String,
    /// Why this gap matters for the plan's success
    #[serde(default)]
    pub why_it_matters: Option<String>,
    /// Which critic layer identified this gap: "layer1" | "layer2" (NOT included in fingerprint)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// Native persisted snapshot of a single verification round.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VerificationRoundSnapshot {
    /// 1-based round number
    pub round: u32,
    /// Aggregate gap score: critical*10 + high*3 + medium*1
    pub gap_score: u32,
    /// Normalized gap fingerprints (one per gap)
    #[serde(default)]
    pub fingerprints: Vec<String>,
    /// Full gap snapshot for the round
    #[serde(default)]
    pub gaps: Vec<VerificationGap>,
    /// True when the critic output for this round could not be fully parsed
    #[serde(default)]
    pub parse_failed: bool,
}

/// Native persisted snapshot of a verification run keyed by session + generation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VerificationRunSnapshot {
    /// Verification generation on the parent ideation session
    pub generation: i32,
    /// Terminal or in-progress verification status for this generation
    pub status: VerificationStatus,
    /// Whether the run is still active
    pub in_progress: bool,
    /// Current round number (1-based)
    #[serde(default)]
    pub current_round: u32,
    /// Maximum allowed rounds before hard-cap exit
    #[serde(default)]
    pub max_rounds: u32,
    /// Round index with the lowest gap_score (for best-version tracking)
    #[serde(default)]
    pub best_round_index: Option<u32>,
    /// Why verification converged (set on terminal status)
    #[serde(default)]
    pub convergence_reason: Option<String>,
    /// Full current gap list for the run
    #[serde(default)]
    pub current_gaps: Vec<VerificationGap>,
    /// Native per-round snapshots in chronological order
    #[serde(default)]
    pub rounds: Vec<VerificationRoundSnapshot>,
}

/// Typed errors for verification state machine violations (D17)
#[derive(Debug, thiserror::Error)]
pub enum VerificationError {
    #[error("Plan must be verified before accepting")]
    NotVerified,
    #[error("Plan verification is in progress (round {round}/{max_rounds})")]
    InProgress { round: u32, max_rounds: u32 },
    #[error("Plan has {count} unresolved gaps")]
    HasUnresolvedGaps { count: u32 },
    #[error("Verification was skipped — cannot update from critic")]
    SkippedCannotUpdate,
    #[error("Invalid verification transition: {from} → {to}")]
    InvalidTransition { from: String, to: String },
    #[error("Round {round} exceeds max_rounds ({max})")]
    RoundExceedsMax { round: u32, max: u32 },
    #[error("Verification agent crashed during round {round}")]
    AgentCrashed { round: u32 },

    /// Gate for proposal mutations (create/update/delete).
    /// Distinct from `NotVerified` which gates acceptance.
    #[error("Cannot create proposals: plan verification has not been run. Start verification before mutating proposals.")]
    ProposalNotVerified,

    /// Gate for proposal mutations when verification is in progress.
    /// Distinct from `InProgress` which gates acceptance.
    #[error("Cannot {operation} proposals: plan verification is in progress (round {round}/{max_rounds}). Complete the current verification round before modifying proposals.")]
    ProposalReviewInProgress {
        operation: String,
        round: u32,
        max_rounds: u32,
    },

    /// Gate for proposal mutations when plan has unresolved gaps.
    /// Distinct from `HasUnresolvedGaps` which gates acceptance.
    #[error("Cannot {operation} proposals: plan verification found {gap_count} unresolved gap(s). Update the plan to address gaps (update_plan_artifact), then re-run verification.")]
    ProposalHasUnresolvedGaps { operation: String, gap_count: usize },

    /// Gate for proposal creation when verification was skipped.
    /// Skipping verification blocks NEW proposal creation for all sessions (internal and external).
    /// Existing proposals can still be updated/deleted; only Create is blocked.
    #[error("Cannot create proposals: plan verification was skipped. Re-run verification to create new proposals.")]
    ProposalSkippedNotAllowed,

    /// Gate for skip operations on external-origin sessions.
    /// External sessions cannot skip plan verification — they must run it to completion.
    #[error("External sessions cannot skip plan verification. Run verification to completion.")]
    ExternalCannotSkip,
}

// ---------------------------------------------------------------------------

/// Status of user acceptance for the finalize confirmation gate
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcceptanceStatus {
    /// Proposals have been validated but not yet applied, awaiting user confirmation
    Pending,
    /// User has accepted the proposals, apply_proposals_core will be called
    Accepted,
    /// User has rejected the proposals, acceptance_status will be reset to null
    Rejected,
}

impl Default for AcceptanceStatus {
    fn default() -> Self {
        Self::Pending
    }
}

impl std::fmt::Display for AcceptanceStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AcceptanceStatus::Pending => write!(f, "pending"),
            AcceptanceStatus::Accepted => write!(f, "accepted"),
            AcceptanceStatus::Rejected => write!(f, "rejected"),
        }
    }
}

impl FromStr for AcceptanceStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(AcceptanceStatus::Pending),
            "accepted" => Ok(AcceptanceStatus::Accepted),
            "rejected" => Ok(AcceptanceStatus::Rejected),
            _ => Err(format!("unknown acceptance status: '{s}'")),
        }
    }
}

// ---------------------------------------------------------------------------

/// Status of user confirmation for the plan verification gate.
/// Tracks whether the user has acknowledged and confirmed the verified plan
/// before proceeding to proposal creation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationConfirmationStatus {
    /// Plan has been verified but user has not yet confirmed
    Pending,
    /// User has accepted the verified plan and confirmed proceeding
    Accepted,
    /// User has rejected the verified plan
    Rejected,
}

impl Default for VerificationConfirmationStatus {
    fn default() -> Self {
        Self::Pending
    }
}

impl std::fmt::Display for VerificationConfirmationStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VerificationConfirmationStatus::Pending => write!(f, "pending"),
            VerificationConfirmationStatus::Accepted => write!(f, "accepted"),
            VerificationConfirmationStatus::Rejected => write!(f, "rejected"),
        }
    }
}

impl FromStr for VerificationConfirmationStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(VerificationConfirmationStatus::Pending),
            "accepted" => Ok(VerificationConfirmationStatus::Accepted),
            "rejected" => Ok(VerificationConfirmationStatus::Rejected),
            _ => Err(format!("unknown verification confirmation status: '{s}'")),
        }
    }
}

// ---------------------------------------------------------------------------

/// Helper function to parse datetime strings from SQLite
pub fn parse_datetime_helper(s: String) -> DateTime<Utc> {
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
