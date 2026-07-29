//! Spawn-free transcript reads for the remote facade.
//!
//! # Why this module exists
//!
//! PR 3.2 (two-instance chat validation) needs a paired device to READ a conversation. The
//! three obvious commands — `get_agent_conversation`,
//! `get_agent_conversation_messages_page`, `get_agent_conversation_timeline_page` — all fire
//! authority detector (a), which is why batch 3 left them unregistered and flagged them as the
//! real remaining work.
//!
//! The trace (`capability_ledger_tests::probe_transcript_read_arming_paths`) shows the arming
//! edge is not in the read at all:
//!
//! ```text
//! get_agent_conversation
//!   -> wake_agent_workspace_for_bridge_events
//!     -> wake_agent_workspace_for_bridge_events_with_deps
//!       -> dispatch_prepared_agent_workspace_bridge_wakeup_with_deps  [STEER: send_message]
//! ```
//!
//! Each command opens by waking the conversation's agent workspace so bridge events are
//! delivered, and only then reads. The wake is a live-agent steer. It is also, by the local
//! commands' own construction, NOT load-bearing for the read: its error is `tracing::warn!`-ed
//! and discarded, and the read proceeds regardless. A read that is correct when the wake fails
//! is a read that does not need the wake.
//!
//! So this is the incidental config-then-exec shape, not genuine coupling, and the split is
//! the same one `remote_chat_commands` made for chat send: the spawning variant stays local
//! and unregistered, and a pure-read variant is registered.
//!
//! # The contract this module keeps
//!
//! - It takes **no** `tauri::AppHandle`, **no** `ExecutionState`, and never builds a
//!   `ChatService`. Those are the three carriers of spawn/steer authority, so their absence is
//!   checkable by reading this file's signatures.
//! - Every command here delegates to an existing `*_for_app_state` seam in
//!   `unified_chat_commands`. It forks no logic (A-7); the local and remote reads return the
//!   same payload from the same repository calls.
//! - The seams propagate repository errors. A remote client is never told "no messages" when
//!   the truth is "the query failed".
//!
//! `remote_transcript_reads_never_reach_the_wake` asserts the split mechanically, over the
//! same call graph the detector uses, so re-introducing a wake here fails CI rather than
//! silently re-arming the read.

use tauri::State;

use crate::application::AppState;
use crate::commands::unified_chat_commands::{
    get_agent_conversation_for_app_state, get_agent_conversation_messages_page_for_app_state,
    get_agent_conversation_timeline_page_for_app_state, AgentConversationMessagesPageResponse,
    AgentConversationTimelinePageResponse, AgentConversationWithMessagesResponse,
};
use ralphx_domain::entities::ChatConversationId;

/// Page bounds, kept identical to the local commands so a client cannot widen the read by
/// going remote.
const DEFAULT_PAGE_LIMIT: u32 = 40;
const MAX_PAGE_LIMIT: u32 = 200;

/// Read a conversation and its messages, without waking its agent.
#[tauri::command]
pub async fn get_remote_agent_conversation(
    conversation_id: String,
    state: State<'_, AppState>,
) -> Result<Option<AgentConversationWithMessagesResponse>, String> {
    get_agent_conversation_for_app_state(&state, ChatConversationId::from_string(&conversation_id))
        .await
}

/// Read a tail-first page of messages, without waking the conversation's agent.
#[tauri::command]
pub async fn get_remote_agent_conversation_messages_page(
    conversation_id: String,
    limit: Option<u32>,
    offset: Option<u32>,
    state: State<'_, AppState>,
) -> Result<Option<AgentConversationMessagesPageResponse>, String> {
    get_agent_conversation_messages_page_for_app_state(
        &state,
        ChatConversationId::from_string(&conversation_id),
        limit.unwrap_or(DEFAULT_PAGE_LIMIT).clamp(1, MAX_PAGE_LIMIT),
        offset.unwrap_or(0),
    )
    .await
}

/// Read a page of the conversation timeline, without waking the conversation's agent.
#[tauri::command]
pub async fn get_remote_agent_conversation_timeline_page(
    conversation_id: String,
    limit: Option<u32>,
    before_sequence: Option<i64>,
    state: State<'_, AppState>,
) -> Result<Option<AgentConversationTimelinePageResponse>, String> {
    get_agent_conversation_timeline_page_for_app_state(
        &state,
        ChatConversationId::from_string(&conversation_id),
        limit.unwrap_or(DEFAULT_PAGE_LIMIT).clamp(1, MAX_PAGE_LIMIT),
        before_sequence,
    )
    .await
}
