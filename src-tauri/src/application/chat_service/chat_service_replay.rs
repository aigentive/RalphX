// Conversation replay builder for session recovery
//
// Rebuilds conversation history from database for rehydrating fresh provider sessions.

use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::domain::entities::{ChatContextType, ChatConversationId, MessageRole};
use crate::domain::repositories::ChatMessageRepository;
use crate::error::AppResult;

use super::chat_service_selection_snapshot::{
    append_selection_snapshot_for_prompt, selection_snapshot_from_metadata,
};

/// Metadata for enriching recovery prompts with ideation-specific state.
///
/// When recovering an ideation session, this metadata provides context about
/// the session's current state, enabling the recovered agent to continue
/// seamlessly with full awareness of the ideation context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdeationRecoveryMetadata {
    /// Current session status (e.g., "active", "archived", "accepted")
    pub session_status: String,
    /// ID of the implementation plan artifact, if one exists
    pub plan_artifact_id: Option<String>,
    /// Number of task proposals in the session
    pub proposal_count: u32,
    /// Parent session ID for linked sessions (follow-on work)
    pub parent_session_id: Option<String>,
    /// Human-readable session title
    pub session_title: Option<String>,
    /// Verification status before recovery (e.g., "unverified", "reviewing", "verified")
    pub verification_status: String,
    /// Whether verification was in-progress when the session crashed
    pub verification_in_progress: bool,
    /// Current verification round number (0 if not started)
    pub current_round: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationReplay {
    pub turns: Vec<Turn>,
    pub total_tokens: usize,
    pub is_truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Turn {
    pub role: MessageRole,
    pub content: String,
    pub tool_calls: Vec<serde_json::Value>,
    pub tool_results: Vec<serde_json::Value>,
}

pub struct ReplayBuilder {
    token_budget: usize,
}

impl ReplayBuilder {
    pub fn new(token_budget: usize) -> Self {
        Self { token_budget }
    }

    pub async fn build_replay(
        &self,
        chat_message_repo: &Arc<dyn ChatMessageRepository>,
        conversation_id: &ChatConversationId,
    ) -> AppResult<ConversationReplay> {
        // Load all messages ordered by created_at ASC
        let messages = chat_message_repo
            .get_by_conversation(conversation_id)
            .await
            .map_err(|e| crate::error::AppError::Database(e.to_string()))?;

        let mut turns = Vec::new();
        let mut current_token_count = 0;
        let total_messages = messages.len();

        // Process newest-first for budget, then reverse for chronological order
        for msg in messages.iter().rev() {
            if Self::should_skip_message(&msg.role, &msg.content) {
                continue;
            }

            let turn = Self::message_to_turn(msg)?;
            let estimated_tokens = Self::estimate_tokens(&turn);

            if current_token_count + estimated_tokens > self.token_budget {
                break;
            }

            turns.insert(0, turn); // maintain chronological order
            current_token_count += estimated_tokens;
        }

        let turns_len = turns.len();
        Ok(ConversationReplay {
            turns,
            total_tokens: current_token_count,
            is_truncated: turns_len < total_messages,
        })
    }

    fn should_skip_message(role: &MessageRole, content: &str) -> bool {
        // Skip system messages containing error wrappers
        if *role == MessageRole::System {
            return content.contains(super::AGENT_ERROR_PREFIX);
        }
        false
    }

    fn message_to_turn(msg: &crate::domain::entities::ChatMessage) -> AppResult<Turn> {
        let tool_calls = if let Some(ref tc_json) = msg.tool_calls {
            serde_json::from_str(tc_json).map_err(|e| {
                crate::error::AppError::Infrastructure(format!("Failed to parse tool_calls: {}", e))
            })?
        } else {
            vec![]
        };

        let content = if msg.role == MessageRole::User {
            let snapshot = selection_snapshot_from_metadata(msg.metadata.as_deref())
                .map_err(|error| crate::error::AppError::Validation(error.to_string()))?;
            append_selection_snapshot_for_prompt(&msg.content, snapshot.as_ref())
                .map_err(|error| crate::error::AppError::Validation(error.to_string()))?
        } else {
            msg.content.clone()
        };

        Ok(Turn {
            role: msg.role,
            content,
            tool_calls,
            tool_results: vec![], // extracted from content_blocks if needed
        })
    }

    fn estimate_tokens(turn: &Turn) -> usize {
        // Rough heuristic: 4 chars per token
        (turn.content.len() / 4) + (turn.tool_calls.len() * 200) // average tool call JSON
    }
}

/// Build rehydration prompt with conversation history.
///
/// For ideation contexts, optionally includes `<ideation_state>` XML block
/// with session metadata to enable seamless context continuity.
pub fn build_rehydration_prompt(
    replay: &ConversationReplay,
    context_type: ChatContextType,
    context_id: &str,
    new_user_message: &str,
    ideation_metadata: Option<&IdeationRecoveryMetadata>,
) -> String {
    let history_xml = replay
        .turns
        .iter()
        .map(|turn| {
            let tool_info = if !turn.tool_calls.is_empty() {
                format!(" tools=\"{}\"", turn.tool_calls.len())
            } else {
                String::new()
            };
            format!(
                "<turn role=\"{}\"{}>{}</turn>",
                turn.role, tool_info, turn.content
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Build ideation state XML block if metadata is provided
    let ideation_state_xml = if let Some(meta) = ideation_metadata {
        let plan_artifact = meta
            .plan_artifact_id
            .as_ref()
            .map(|id| format!("\n  <plan_artifact_id>{}</plan_artifact_id>", id))
            .unwrap_or_default();
        let parent_session = meta
            .parent_session_id
            .as_ref()
            .map(|id| format!("\n  <parent_session_id>{}</parent_session_id>", id))
            .unwrap_or_default();
        let session_title = meta
            .session_title
            .as_ref()
            .map(|t| format!("\n  <session_title>{}</session_title>", t))
            .unwrap_or_default();

        // If verification was in-progress when session crashed, note that it was reset.
        let verification_note = if meta.verification_in_progress {
            format!(
                "\n  <verification_status>unverified</verification_status>\
                 \n  <verification_note>Verification was in-progress (round {}) when session crashed. State has been reset to unverified — start a fresh verification cycle if needed.</verification_note>",
                meta.current_round
            )
        } else {
            format!(
                "\n  <verification_status>{}</verification_status>",
                meta.verification_status
            )
        };

        format!(
            "<ideation_state>\n\
             <session_status>{}</session_status>\n\
             <proposal_count>{}</proposal_count>{}{}{}{}\n\
             </ideation_state>\n\
             <recovery_note>Session recovered from local storage. Phase 0 may call get_session_messages for additional context if needed.</recovery_note>\n",
            meta.session_status,
            meta.proposal_count,
            plan_artifact,
            parent_session,
            session_title,
            verification_note
        )
    } else {
        String::new()
    };

    format!(
        "<instructions>\n\
         Your previous session expired. Here is the conversation history restored from local storage.\n\
         Continue the conversation naturally from where it left off.\n\
         Context: {} ({})\n\
         </instructions>\n\
         {}\
         <conversation_history>\n{}\n</conversation_history>\n\
         <current_message>{}</current_message>",
        context_type, context_id, ideation_state_xml, history_xml, new_user_message
    )
}

#[cfg(test)]
#[path = "chat_service_replay_tests.rs"]
mod tests;
