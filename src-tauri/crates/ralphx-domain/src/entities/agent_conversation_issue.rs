use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{ChatConversationId, ProjectId};

pub const AGENT_CONVERSATION_ISSUE_STATUS_OPEN: &str = "open";
pub const AGENT_CONVERSATION_ISSUE_STATUS_RESOLVED: &str = "resolved";
pub const AGENT_CONVERSATION_ISSUE_STATUS_DISMISSED: &str = "dismissed";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentConversationIssue {
    pub id: String,
    pub project_id: ProjectId,
    pub conversation_id: ChatConversationId,
    pub source_task_id: Option<String>,
    pub source_context_type: Option<String>,
    pub source_context_id: Option<String>,
    pub source_agent_name: Option<String>,
    pub issue_kind: String,
    pub severity: String,
    pub status: String,
    pub blocking_scope: String,
    pub title: String,
    pub summary: String,
    pub evidence: Option<String>,
    pub recommendation: Option<String>,
    pub blocker_fingerprint: Option<String>,
    pub followup_title: Option<String>,
    pub followup_prompt: Option<String>,
    pub auto_followup_eligible: bool,
    pub linked_followup_conversation_id: Option<ChatConversationId>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
}

impl AgentConversationIssue {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        project_id: ProjectId,
        conversation_id: ChatConversationId,
        source_task_id: Option<String>,
        source_context_type: Option<String>,
        source_context_id: Option<String>,
        source_agent_name: Option<String>,
        issue_kind: String,
        severity: String,
        blocking_scope: String,
        title: String,
        summary: String,
        evidence: Option<String>,
        recommendation: Option<String>,
        blocker_fingerprint: Option<String>,
        followup_title: Option<String>,
        followup_prompt: Option<String>,
        auto_followup_eligible: bool,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            project_id,
            conversation_id,
            source_task_id,
            source_context_type,
            source_context_id,
            source_agent_name,
            issue_kind,
            severity,
            status: AGENT_CONVERSATION_ISSUE_STATUS_OPEN.to_string(),
            blocking_scope,
            title,
            summary,
            evidence,
            recommendation,
            blocker_fingerprint,
            followup_title,
            followup_prompt,
            auto_followup_eligible,
            linked_followup_conversation_id: None,
            created_at: now,
            updated_at: now,
            resolved_at: None,
        }
    }

    pub fn refresh_from(&mut self, incoming: Self) {
        self.source_context_type = incoming.source_context_type;
        self.source_context_id = incoming.source_context_id;
        self.source_agent_name = incoming.source_agent_name;
        self.severity = incoming.severity;
        self.blocking_scope = incoming.blocking_scope;
        self.title = incoming.title;
        self.summary = incoming.summary;
        self.evidence = incoming.evidence;
        self.recommendation = incoming.recommendation;
        self.followup_title = incoming.followup_title;
        self.followup_prompt = incoming.followup_prompt;
        self.auto_followup_eligible = incoming.auto_followup_eligible;
        self.status = AGENT_CONVERSATION_ISSUE_STATUS_OPEN.to_string();
        self.resolved_at = None;
        self.updated_at = Utc::now();
    }
}
