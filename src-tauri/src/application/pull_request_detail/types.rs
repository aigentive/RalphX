//! Read-model DTOs for the PR-detail view (Decision 4 full graph).
//!
//! All structs are `#[serde(rename_all = "camelCase")]` Tauri response shapes —
//! the single `get_pull_request_detail` command returns [`PullRequestDetail`]
//! directly. Every field is empty/`None` for the non-`Loaded` typed states so
//! the UI can render distinct empty/error surfaces without the command ever
//! returning an `Err` or panicking.

use serde::Serialize;

/// Distinct, non-throwing view states for the PR-detail surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PullRequestDetailState {
    /// Full graph resolved and returned.
    Loaded,
    /// No PR is associated with the requested branch/number.
    NoPr,
    /// `gh` is installed but not authenticated (`gh auth login` needed).
    GhUnauthenticated,
    /// The project's repo could not be resolved (missing working dir / service).
    RepoUnresolvable,
    /// The GitHub CLI could not be launched or resolved.
    CliUnavailable,
    /// A `gh` fetch exceeded its subprocess timeout.
    FetchTimeout,
    /// GitHub access could not be verified because the provider/probe is unavailable.
    FetchUnavailable,
    /// GitHub reported an API rate-limit.
    RateLimited,
}

/// How the PR was associated with the requested branch/number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PullRequestOrigin {
    /// RalphX-owned outbound publication PR (`agent_conversation_workspaces`).
    OwnedOutbound,
    /// RalphX-owned inbound review PR (`source_pull_request`).
    OwnedInbound,
    /// Task-pipeline PR-mode branch (`plan_branches`).
    PlanBranch,
    /// External PR resolved live via `gh`.
    External,
}

/// PR description section (title/body/author/state/refs).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequestDescription {
    pub number: i64,
    pub title: String,
    pub body: Option<String>,
    pub author: Option<String>,
    pub created_at: Option<String>,
    pub url: Option<String>,
    /// `open` | `draft` | `closed` | `merged`.
    pub state: String,
    pub is_draft: bool,
    pub head_ref_name: String,
    pub base_ref_name: String,
}

/// A single status check / check-run summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequestCheck {
    pub name: String,
    pub status: Option<String>,
    pub conclusion: Option<String>,
    pub details_url: Option<String>,
}

/// One inline comment inside the latest outstanding requested-changes review.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequestReviewFeedbackComment {
    pub id: String,
    pub author: String,
    pub path: Option<String>,
    pub line: Option<i64>,
    pub body: String,
}

/// Review summary: GitHub review decision + the latest CHANGES_REQUESTED review
/// (often `None`; never the full thread — see `review_thread` for that).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequestReviewSummary {
    pub review_decision: Option<String>,
    pub latest_changes_requested_author: Option<String>,
    pub latest_changes_requested_body: Option<String>,
    pub latest_changes_requested_submitted_at: Option<String>,
    pub latest_changes_requested_comments: Vec<PullRequestReviewFeedbackComment>,
}

/// An issue comment on the PR. `source` distinguishes the DB-cached evidence
/// path (owned published PRs) from the live `gh` path (inbound/external PRs).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequestIssueComment {
    pub id: String,
    pub author: Option<String>,
    pub body: String,
    pub url: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub is_bot: bool,
    pub is_codecov: bool,
    /// `evidence` (DB single-writer cache) | `live` (transient `gh` fetch).
    pub source: String,
}

/// An inline review-thread comment (live, transient).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequestReviewThreadComment {
    pub id: String,
    pub author: Option<String>,
    pub body: String,
    pub path: Option<String>,
    pub side: Option<String>,
    pub line: Option<i64>,
    pub url: Option<String>,
    pub created_at: Option<String>,
    pub in_reply_to_id: Option<String>,
    pub is_outdated: bool,
}

/// An attached RalphX conversation (workspace whose branch == the PR head ref).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequestRxConversation {
    pub conversation_id: String,
    pub branch_name: String,
    pub linked_ideation_session_id: Option<String>,
    pub publication_pr_number: Option<i64>,
    pub publication_pr_status: Option<String>,
}

/// A ticket linked to the PR. Reverse (branch→ticket) linkage is deferred in v1
/// (the forward ticket→PR path already ships via `get_ticket_associations`), so
/// this is currently always empty on the detail payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequestLinkedTicket {
    pub provider: String,
    pub issue_key: String,
    pub url: Option<String>,
}

/// The full PR-detail graph returned by `get_pull_request_detail`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequestDetail {
    pub state: PullRequestDetailState,
    pub origin: Option<PullRequestOrigin>,
    pub description: Option<PullRequestDescription>,
    pub checks: Vec<PullRequestCheck>,
    pub review_summary: Option<PullRequestReviewSummary>,
    pub issue_comments: Vec<PullRequestIssueComment>,
    pub review_thread: Vec<PullRequestReviewThreadComment>,
    pub rx_conversations: Vec<PullRequestRxConversation>,
    pub linked_tickets: Vec<PullRequestLinkedTicket>,
    /// Per-section soft failures (e.g. `checks`, `reviewThread`) that did not
    /// fail the whole view. Empty on a clean load.
    pub sources_unavailable: Vec<String>,
}

impl PullRequestDetail {
    /// A typed empty payload for a non-`Loaded` state (never panics downstream).
    pub fn empty(state: PullRequestDetailState) -> Self {
        Self {
            state,
            origin: None,
            description: None,
            checks: Vec::new(),
            review_summary: None,
            issue_comments: Vec::new(),
            review_thread: Vec::new(),
            rx_conversations: Vec::new(),
            linked_tickets: Vec::new(),
            sources_unavailable: Vec::new(),
        }
    }
}
