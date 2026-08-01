//! Intent row for a spawn-free remote CONTINUATION of an existing idle conversation.
//!
//! Sibling of [`crate::entities::RemoteConversationStartRequest`], deliberately NOT a reuse of it:
//! a start seeds a brand-new conversation and mints a fresh run, while a continuation resumes the
//! provider session of a conversation the host already owns. Sharing one table would make the
//! two indistinguishable to the dispatcher, and the dispatcher's terminal call differs
//! (`ChatService::send_message` vs `AgentConversationStartService::start`).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

use crate::entities::{ChatConversationId, ProjectId};

/// Lifecycle of one remote continuation intent.
///
/// Every non-`Dispatched` terminal state is a VISIBLE failure the client must surface: the
/// hazard this table exists to avoid (design doc §7) is a message persisted as "sent" that no
/// agent ever saw.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RemoteConversationMessageStatus {
    /// Persisted, awaiting the host dispatcher's CAS claim.
    Pending,
    /// CAS-claimed by the dispatcher; the host send is in flight.
    Dispatching,
    /// The host send completed and a run owns the turn.
    Dispatched,
    /// Terminal failure with an `error_code`.
    Failed,
    /// Terminal: revoked before dispatch.
    Cancelled,
    /// Terminal: a `Dispatching` claim outlived its lease (host crash). Never auto-retried —
    /// a re-dispatch of a claim whose outcome is unknown is a duplicate-turn factory.
    FailedStale,
}

impl RemoteConversationMessageStatus {
    /// Canonical DB/wire string. Matches the camelCase serde representation so the persisted
    /// TEXT equals the serialized wire value.
    pub fn as_db_str(&self) -> &'static str {
        match self {
            RemoteConversationMessageStatus::Pending => "pending",
            RemoteConversationMessageStatus::Dispatching => "dispatching",
            RemoteConversationMessageStatus::Dispatched => "dispatched",
            RemoteConversationMessageStatus::Failed => "failed",
            RemoteConversationMessageStatus::Cancelled => "cancelled",
            RemoteConversationMessageStatus::FailedStale => "failedStale",
        }
    }

    /// Whether the client may stop polling. `Pending`/`Dispatching` are the only non-terminal
    /// states; everything else is a settled outcome the composer renders.
    pub fn is_terminal(&self) -> bool {
        !matches!(
            self,
            RemoteConversationMessageStatus::Pending | RemoteConversationMessageStatus::Dispatching
        )
    }
}

impl fmt::Display for RemoteConversationMessageStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_db_str())
    }
}

impl FromStr for RemoteConversationMessageStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(RemoteConversationMessageStatus::Pending),
            "dispatching" => Ok(RemoteConversationMessageStatus::Dispatching),
            "dispatched" => Ok(RemoteConversationMessageStatus::Dispatched),
            "failed" => Ok(RemoteConversationMessageStatus::Failed),
            "cancelled" => Ok(RemoteConversationMessageStatus::Cancelled),
            "failedStale" => Ok(RemoteConversationMessageStatus::FailedStale),
            other => Err(format!("invalid RemoteConversationMessageStatus: {other}")),
        }
    }
}

/// One persisted remote continuation intent.
///
/// `model_override` / `logical_effort` exist because the composer's options must TRAVEL rather
/// than be silently dropped (reassessment UX-5). They are validated against the conversation's
/// own provider at persist time and RE-validated at dispatch time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteConversationMessageRequest {
    pub id: String,
    pub conversation_id: ChatConversationId,
    pub project_id: ProjectId,
    pub content: String,
    /// The harness that owned the conversation when the intent was persisted. Recorded so the
    /// dispatcher can re-prove the provider is still enabled without re-deriving it.
    pub provider: String,
    pub model_override: Option<String>,
    pub logical_effort: Option<String>,
    pub status: RemoteConversationMessageStatus,
    pub error_code: Option<String>,
    pub requested_by_device_id: String,
    pub agent_run_id: Option<String>,
    pub claimed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
