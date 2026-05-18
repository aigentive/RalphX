use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::entities::{
    ChatConversationId, IdeationAnalysisBaseRefKind, IdeationSessionId, PlanBranchId, ProjectId,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentConversationWorkspaceMode {
    Chat,
    Edit,
    Ideation,
}

impl std::fmt::Display for AgentConversationWorkspaceMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentConversationWorkspaceMode::Chat => write!(f, "chat"),
            AgentConversationWorkspaceMode::Edit => write!(f, "edit"),
            AgentConversationWorkspaceMode::Ideation => write!(f, "ideation"),
        }
    }
}

impl FromStr for AgentConversationWorkspaceMode {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "chat" => Ok(Self::Chat),
            "edit" => Ok(Self::Edit),
            "ideation" => Ok(Self::Ideation),
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
    pub publication_pr_number: Option<i64>,
    pub publication_pr_url: Option<String>,
    pub publication_pr_status: Option<String>,
    pub publication_push_status: Option<String>,
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
            publication_pr_number: None,
            publication_pr_url: None,
            publication_pr_status: None,
            publication_push_status: None,
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
}

pub fn is_terminal_publication_pr_status(status: Option<&str>) -> bool {
    matches!(status, Some("merged" | "closed"))
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
