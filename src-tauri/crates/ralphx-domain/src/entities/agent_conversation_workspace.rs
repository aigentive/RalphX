use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::entities::{
    ArtifactId, ChatConversationId, IdeationAnalysisBaseRefKind, IdeationSessionId, PlanBranchId,
    ProjectId,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentConversationWorkspaceMode {
    Chat,
    Edit,
    Plan,
    Ideation,
    ReviewPr,
}

impl std::fmt::Display for AgentConversationWorkspaceMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentConversationWorkspaceMode::Chat => write!(f, "chat"),
            AgentConversationWorkspaceMode::Edit => write!(f, "edit"),
            AgentConversationWorkspaceMode::Plan => write!(f, "plan"),
            AgentConversationWorkspaceMode::Ideation => write!(f, "ideation"),
            AgentConversationWorkspaceMode::ReviewPr => write!(f, "review_pr"),
        }
    }
}

impl FromStr for AgentConversationWorkspaceMode {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "chat" => Ok(Self::Chat),
            "edit" => Ok(Self::Edit),
            "plan" => Ok(Self::Plan),
            "ideation" => Ok(Self::Ideation),
            "review_pr" => Ok(Self::ReviewPr),
            _ => Err(format!(
                "unknown agent conversation workspace mode: '{value}'"
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentConversationWorkspaceStatus {
    Active,
    Archived,
    Missing,
}

impl std::fmt::Display for AgentConversationWorkspaceStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentConversationWorkspaceStatus::Active => write!(f, "active"),
            AgentConversationWorkspaceStatus::Archived => write!(f, "archived"),
            AgentConversationWorkspaceStatus::Missing => write!(f, "missing"),
        }
    }
}

impl FromStr for AgentConversationWorkspaceStatus {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "active" => Ok(Self::Active),
            "archived" => Ok(Self::Archived),
            "missing" => Ok(Self::Missing),
            _ => Err(format!(
                "unknown agent conversation workspace status: '{value}'"
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentWorkspacePrReviewMonitorStatus {
    Idle,
    Reviewing,
    AwaitingUser,
    Watching,
    Submitting,
    Blocked,
    Terminal,
}

impl std::fmt::Display for AgentWorkspacePrReviewMonitorStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Idle => write!(f, "idle"),
            Self::Reviewing => write!(f, "reviewing"),
            Self::AwaitingUser => write!(f, "awaiting_user"),
            Self::Watching => write!(f, "watching"),
            Self::Submitting => write!(f, "submitting"),
            Self::Blocked => write!(f, "blocked"),
            Self::Terminal => write!(f, "terminal"),
        }
    }
}

impl FromStr for AgentWorkspacePrReviewMonitorStatus {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "idle" => Ok(Self::Idle),
            "reviewing" => Ok(Self::Reviewing),
            "awaiting_user" => Ok(Self::AwaitingUser),
            "watching" => Ok(Self::Watching),
            "submitting" => Ok(Self::Submitting),
            "blocked" => Ok(Self::Blocked),
            "terminal" => Ok(Self::Terminal),
            _ => Err(format!("unknown PR review monitor status: '{value}'")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentWorkspacePrReviewActionKind {
    RequestChanges,
    Approve,
    Comment,
}

impl std::fmt::Display for AgentWorkspacePrReviewActionKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RequestChanges => write!(f, "request_changes"),
            Self::Approve => write!(f, "approve"),
            Self::Comment => write!(f, "comment"),
        }
    }
}

impl FromStr for AgentWorkspacePrReviewActionKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "request_changes" => Ok(Self::RequestChanges),
            "approve" => Ok(Self::Approve),
            "comment" => Ok(Self::Comment),
            _ => Err(format!("unknown PR review action: '{value}'")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentWorkspacePrReviewActionStatus {
    Pending,
    Approved,
    Skipped,
    Submitting,
    Submitted,
    Failed,
}

impl std::fmt::Display for AgentWorkspacePrReviewActionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::Approved => write!(f, "approved"),
            Self::Skipped => write!(f, "skipped"),
            Self::Submitting => write!(f, "submitting"),
            Self::Submitted => write!(f, "submitted"),
            Self::Failed => write!(f, "failed"),
        }
    }
}

impl FromStr for AgentWorkspacePrReviewActionStatus {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "pending" => Ok(Self::Pending),
            "approved" => Ok(Self::Approved),
            "skipped" => Ok(Self::Skipped),
            "submitting" => Ok(Self::Submitting),
            "submitted" => Ok(Self::Submitted),
            "failed" => Ok(Self::Failed),
            _ => Err(format!("unknown PR review action status: '{value}'")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentWorkspaceSourcePullRequest {
    pub number: i64,
    pub url: Option<String>,
    pub title: Option<String>,
    pub head_ref_name: String,
    pub base_ref_name: Option<String>,
    pub head_ref_oid: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentWorkspacePrReviewMonitor {
    pub conversation_id: ChatConversationId,
    pub project_id: ProjectId,
    pub pr_number: i64,
    pub status: AgentWorkspacePrReviewMonitorStatus,
    pub monitor_enabled: bool,
    pub first_review_completed: bool,
    pub last_seen_head_sha: Option<String>,
    pub last_reviewed_head_sha: Option<String>,
    pub last_review_run_id: Option<String>,
    pub last_review_outcome: Option<String>,
    pub last_submitted_review_id: Option<String>,
    pub review_artifact_id: Option<ArtifactId>,
    pub review_artifact_head_sha: Option<String>,
    pub review_artifact_version: Option<u32>,
    pub review_artifact_updated_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl AgentWorkspacePrReviewMonitor {
    pub fn new(
        conversation_id: ChatConversationId,
        project_id: ProjectId,
        pr_number: i64,
        head_sha: Option<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            conversation_id,
            project_id,
            pr_number,
            status: AgentWorkspacePrReviewMonitorStatus::Idle,
            monitor_enabled: false,
            first_review_completed: false,
            last_seen_head_sha: head_sha,
            last_reviewed_head_sha: None,
            last_review_run_id: None,
            last_review_outcome: None,
            last_submitted_review_id: None,
            review_artifact_id: None,
            review_artifact_head_sha: None,
            review_artifact_version: None,
            review_artifact_updated_at: None,
            last_error: None,
            created_at: now,
            updated_at: now,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentWorkspacePrReviewAction {
    pub id: String,
    pub conversation_id: ChatConversationId,
    pub pr_number: i64,
    pub head_sha: String,
    pub proposed_action: AgentWorkspacePrReviewActionKind,
    pub summary: String,
    pub review_body: String,
    pub findings_json: Option<String>,
    pub status: AgentWorkspacePrReviewActionStatus,
    pub submitted_review_id: Option<String>,
    pub created_by_run_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
}

impl AgentWorkspacePrReviewAction {
    pub fn new(
        conversation_id: ChatConversationId,
        pr_number: i64,
        head_sha: String,
        proposed_action: AgentWorkspacePrReviewActionKind,
        summary: String,
        review_body: String,
        findings_json: Option<String>,
        created_by_run_id: Option<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            conversation_id,
            pr_number,
            head_sha,
            proposed_action,
            summary,
            review_body,
            findings_json,
            status: AgentWorkspacePrReviewActionStatus::Pending,
            submitted_review_id: None,
            created_by_run_id,
            created_at: now,
            updated_at: now,
            resolved_at: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentConversationWorkspace {
    pub conversation_id: ChatConversationId,
    pub project_id: ProjectId,
    pub mode: AgentConversationWorkspaceMode,
    pub base_ref_kind: IdeationAnalysisBaseRefKind,
    pub base_ref: String,
    pub base_display_name: Option<String>,
    pub base_commit: Option<String>,
    pub branch_name: String,
    pub worktree_path: String,
    pub linked_ideation_session_id: Option<IdeationSessionId>,
    pub linked_plan_branch_id: Option<PlanBranchId>,
    pub source_pull_request: Option<AgentWorkspaceSourcePullRequest>,
    pub publication_pr_number: Option<i64>,
    pub publication_pr_url: Option<String>,
    pub publication_pr_status: Option<String>,
    pub publication_push_status: Option<String>,
    pub auto_publish_enabled: bool,
    pub auto_publish_initial_pr_enabled: bool,
    pub auto_publish_paused_pr_autofix_enabled: Option<bool>,
    pub auto_publish_paused_pr_auto_merge_desired: Option<bool>,
    pub pr_autofix_enabled: bool,
    pub pr_auto_merge_desired: bool,
    pub pr_auto_merge_method: String,
    pub pr_auto_merge_current: Option<bool>,
    pub pr_supervision_status: Option<String>,
    pub pr_supervision_summary: Option<String>,
    pub pr_supervision_updated_at: Option<DateTime<Utc>>,
    pub status: AgentConversationWorkspaceStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl AgentConversationWorkspace {
    pub fn new(
        conversation_id: ChatConversationId,
        project_id: ProjectId,
        mode: AgentConversationWorkspaceMode,
        base_ref_kind: IdeationAnalysisBaseRefKind,
        base_ref: String,
        base_display_name: Option<String>,
        base_commit: Option<String>,
        branch_name: String,
        worktree_path: String,
    ) -> Self {
        let now = Utc::now();
        Self {
            conversation_id,
            project_id,
            mode,
            base_ref_kind,
            base_ref,
            base_display_name,
            base_commit,
            branch_name,
            worktree_path,
            linked_ideation_session_id: None,
            linked_plan_branch_id: None,
            source_pull_request: None,
            publication_pr_number: None,
            publication_pr_url: None,
            publication_pr_status: None,
            publication_push_status: None,
            auto_publish_enabled: true,
            auto_publish_initial_pr_enabled: false,
            auto_publish_paused_pr_autofix_enabled: None,
            auto_publish_paused_pr_auto_merge_desired: None,
            pr_autofix_enabled: false,
            pr_auto_merge_desired: false,
            pr_auto_merge_method: DEFAULT_AGENT_WORKSPACE_PR_AUTO_MERGE_METHOD.to_string(),
            pr_auto_merge_current: None,
            pr_supervision_status: None,
            pr_supervision_summary: None,
            pr_supervision_updated_at: None,
            status: AgentConversationWorkspaceStatus::Active,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn is_execution_owned(&self) -> bool {
        self.linked_plan_branch_id.is_some()
    }

    pub fn has_terminal_publication_pr_status(&self) -> bool {
        is_terminal_publication_pr_status(self.publication_pr_status.as_deref())
    }

    pub fn has_pr_status_pollable_push_status(&self) -> bool {
        is_pr_status_pollable_push_status(self.publication_push_status.as_deref())
    }

    /// Whether this workspace currently has an open (non-terminal) publication PR.
    pub fn has_open_pr(&self) -> bool {
        is_open_pr(self.publication_pr_number, self.publication_pr_status.as_deref())
    }
}

pub fn is_terminal_publication_pr_status(status: Option<&str>) -> bool {
    matches!(status, Some("merged" | "closed"))
}

/// A workspace has an OPEN pull request when a PR number exists and its status is
/// not terminal (merged/closed). Draft PRs count as open. Single source of truth
/// for the "has an open PR" rule across ticketing, sidebar, and chat header.
pub fn is_open_pr(publication_pr_number: Option<i64>, publication_pr_status: Option<&str>) -> bool {
    publication_pr_number.is_some() && !is_terminal_publication_pr_status(publication_pr_status)
}

pub fn is_pr_status_pollable_push_status(status: Option<&str>) -> bool {
    matches!(status, None | Some("pushed" | "refreshed"))
}

pub const DEFAULT_AGENT_WORKSPACE_PR_AUTO_MERGE_METHOD: &str = "squash";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentConversationWorkspacePublicationEvent {
    pub id: String,
    pub conversation_id: ChatConversationId,
    pub step: String,
    pub status: String,
    pub summary: String,
    pub classification: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentWorkspacePrDescription {
    pub title: Option<String>,
    pub body_markdown: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentWorkspacePrCommentEvidence {
    pub conversation_id: ChatConversationId,
    pub pr_number: i64,
    pub comment_id: String,
    pub author: Option<String>,
    pub body: String,
    pub body_excerpt: String,
    pub body_sha256: String,
    pub url: Option<String>,
    pub github_created_at: Option<String>,
    pub github_updated_at: Option<String>,
    pub is_codecov: bool,
    pub is_bot: bool,
    pub first_seen_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    pub last_included_at: Option<DateTime<Utc>>,
    pub last_read_at: Option<DateTime<Utc>>,
    pub edit_count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentWorkspacePrCommentEvidenceUpsert {
    pub pr_number: i64,
    pub comment_id: String,
    pub author: Option<String>,
    pub body: String,
    pub body_excerpt: String,
    pub body_sha256: String,
    pub url: Option<String>,
    pub github_created_at: Option<String>,
    pub github_updated_at: Option<String>,
    pub is_codecov: bool,
    pub is_bot: bool,
}

impl AgentWorkspacePrCommentEvidenceUpsert {
    pub fn new(
        pr_number: i64,
        comment_id: String,
        author: Option<String>,
        body: String,
        url: Option<String>,
        github_created_at: Option<String>,
        github_updated_at: Option<String>,
        is_codecov: bool,
        is_bot: bool,
    ) -> Self {
        let body_excerpt = pr_comment_body_excerpt(&body, 480);
        let body_sha256 = pr_comment_body_sha256(&body);
        Self {
            pr_number,
            comment_id,
            author,
            body,
            body_excerpt,
            body_sha256,
            url,
            github_created_at,
            github_updated_at,
            is_codecov,
            is_bot,
        }
    }
}

pub fn pr_comment_body_sha256(body: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(body.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub fn pr_comment_body_excerpt(body: &str, max_chars: usize) -> String {
    let compact = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() <= max_chars {
        return compact;
    }
    if max_chars == 0 {
        return String::new();
    }
    if max_chars <= 3 {
        return ".".repeat(max_chars);
    }
    let truncated: String = compact.chars().take(max_chars - 3).collect();
    format!("{truncated}...")
}

impl AgentWorkspacePrDescription {
    pub fn new(title: Option<String>, body_markdown: String) -> Self {
        Self {
            title: title.and_then(|value| {
                let trimmed = value.trim();
                (!trimmed.is_empty()).then(|| trimmed.to_string())
            }),
            body_markdown,
        }
    }
}

#[cfg(test)]
mod pr_comment_evidence_tests {
    use super::{pr_comment_body_excerpt, pr_comment_body_sha256};

    #[test]
    fn pr_comment_body_excerpt_respects_tiny_limits() {
        assert_eq!(pr_comment_body_excerpt("abcdef", 0), "");
        assert_eq!(pr_comment_body_excerpt("abcdef", 2), "..");
        assert_eq!(pr_comment_body_excerpt("abcdef", 3), "...");
        assert_eq!(pr_comment_body_excerpt("abcdef", 5), "ab...");
        assert_eq!(pr_comment_body_excerpt("abcdef", 6), "abcdef");
    }

    #[test]
    fn pr_comment_body_sha256_is_stable() {
        assert_eq!(
            pr_comment_body_sha256("coverage"),
            pr_comment_body_sha256("coverage")
        );
        assert_ne!(
            pr_comment_body_sha256("coverage"),
            pr_comment_body_sha256("different")
        );
    }
}

impl AgentConversationWorkspacePublicationEvent {
    pub fn new(
        conversation_id: ChatConversationId,
        step: impl Into<String>,
        status: impl Into<String>,
        summary: impl Into<String>,
        classification: Option<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            conversation_id,
            step: step.into(),
            status: status.into(),
            summary: summary.into(),
            classification,
            created_at: Utc::now(),
        }
    }
}
