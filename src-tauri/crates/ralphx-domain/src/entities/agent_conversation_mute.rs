use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::entities::ChatConversationId;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentConversationMute {
    pub conversation_id: ChatConversationId,
    pub muted_at: DateTime<Utc>,
    pub state_fingerprint: String,
}
