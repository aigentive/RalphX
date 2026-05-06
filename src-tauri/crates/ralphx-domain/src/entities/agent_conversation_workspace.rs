use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
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
            status: AgentConversationWorkspaceStatus::Active,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn is_execution_owned(&self) -> bool {
        self.linked_plan_branch_id.is_some()
    }
}

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
