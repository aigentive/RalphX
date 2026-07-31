//! Spawn-free conversation reads for the remote facade.
//!
//! # Why this module exists
//!
//! PR 3.2 (two-instance chat validation) needs a paired device to READ a conversation — and,
//! since batch 5, to pick one from a LIST first. Transcripts without a list are useless, so the
//! two halves of that read surface live here together.
//!
//! The module holds two different splits, and the difference between them is worth keeping
//! straight because it changes what the tests can honestly assert:
//!
//! * **The transcript reads (batch 4) were disqualified by a WAKE.** They fired authority
//!   detector (a); the split removes a live-agent steer from the read path.
//! * **The list reads (batch 5) were disqualified by a CARRIER.** They never armed —
//!   `probe_conversation_list_arming_paths` reports NO ARMING HITS for the local commands too.
//!   `list_agent_conversations` merely *accepted* `tauri::AppHandle` and `ExecutionState` in
//!   order to build a chat service whose invoked method was a straight repository delegation.
//!   Nothing dangerous was ever reached; the authority was carried and never used.
//!
//! So detector silence disqualified nothing on its own in batch 5, and the calibration for that
//! split is `the_spawn_free_remote_read_module_carries_no_authority_carriers`, which asserts
//! this module's contract over its own source rather than trusting this comment.
//!
//! # The transcript split (batch 4)
//!
//! The three obvious commands — `get_agent_conversation`,
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
use crate::commands::agent_sidebar_commands::{
    list_agent_sidebar_conversations_read_only_for_app_state,
    AgentSidebarConversationGroupsResponse, AgentSidebarConversationsInput,
};
use crate::commands::unified_chat_commands::{
    get_agent_conversation_for_app_state, get_agent_conversation_messages_page_for_app_state,
    get_agent_conversation_timeline_page_for_app_state, list_agent_conversations_for_app_state,
    list_agent_conversations_page_for_app_state, parse_context_type,
    AgentConversationListPageResponse, AgentConversationMessagesPageResponse,
    AgentConversationResponse, AgentConversationTimelinePageResponse,
    AgentConversationWithMessagesResponse,
};
use ralphx_domain::entities::ChatConversationId;

/// Page bounds, kept identical to the local commands so a client cannot widen the read by
/// going remote.
const DEFAULT_PAGE_LIMIT: u32 = 40;
const MAX_PAGE_LIMIT: u32 = 200;

/// The local conversation-list page default. Clamped rather than merely defaulted, so a remote
/// client cannot request a wider page than the local UI can.
const DEFAULT_LIST_PAGE_LIMIT: u32 = 6;

/// List a context's conversations, without accepting spawn authority to do it.
#[tauri::command]
pub async fn list_remote_agent_conversations(
    context_type: String,
    context_id: String,
    include_archived: Option<bool>,
    state: State<'_, AppState>,
) -> Result<Vec<AgentConversationResponse>, String> {
    list_agent_conversations_for_app_state(
        &state,
        parse_context_type(&context_type)?,
        &context_id,
        include_archived.unwrap_or(false),
    )
    .await
}

/// List a page of a context's conversations, with optional title search.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn list_remote_agent_conversations_page(
    context_type: String,
    context_id: String,
    include_archived: Option<bool>,
    archived_only: Option<bool>,
    offset: Option<u32>,
    limit: Option<u32>,
    search: Option<String>,
    state: State<'_, AppState>,
) -> Result<AgentConversationListPageResponse, String> {
    list_agent_conversations_page_for_app_state(
        &state,
        parse_context_type(&context_type)?,
        &context_id,
        include_archived.unwrap_or(false),
        archived_only.unwrap_or(false),
        offset.unwrap_or(0),
        limit
            .unwrap_or(DEFAULT_LIST_PAGE_LIMIT)
            .clamp(1, MAX_PAGE_LIMIT),
        search.as_deref(),
    )
    .await
}

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

/// List the Agents-sidebar inbox groups for a paired device, without scheduling recovery or
/// disclosing host paths.
///
/// This twin exists for two reasons, both security boundaries rather than conveniences:
///
/// * **No host-side spawn.** It delegates to
///   `list_agent_sidebar_conversations_read_only_for_app_state`, which hydrates workspaces
///   through the recovery-free hydrator. The LOCAL `list_agent_sidebar_conversations` schedules
///   PR-supervision recovery on every inbox load (and thereby reaches the git CLI resolver), so
///   it is deliberately unregistered on the facade — a paired viewer polling its inbox must not
///   nudge host-owned background recovery. The read-only seam is what keeps this closure clear
///   of detector (c)'s process-launch floor.
/// * **No host path disclosure.** Every other field of `AgentConversationWorkspaceResponse`
///   (branch names, PR/publication status, IDs, mode, timestamps) is safe metadata already
///   exposed through registered reads, but `worktree_path` is an absolute host filesystem path.
///   [`strip_worktree_paths_from_sidebar_groups`] blanks it out of every row before the payload
///   crosses the wire. The blanking happens HERE, at the facade, so the shared local response
///   type keeps the field for local callers (the desktop UI reads it for the terminal/publish
///   surfaces).
#[tauri::command]
pub async fn list_remote_agent_sidebar_conversations(
    input: AgentSidebarConversationsInput,
    state: State<'_, AppState>,
) -> Result<AgentSidebarConversationGroupsResponse, String> {
    let mut groups =
        list_agent_sidebar_conversations_read_only_for_app_state(input, state.inner()).await?;
    strip_worktree_paths_from_sidebar_groups(&mut groups);
    Ok(groups)
}

/// Blank the host `worktree_path` on every sidebar row's workspace.
///
/// The field is set to an empty string rather than dropped: the frontend zod schema requires
/// `worktree_path` (it is not optional), and the Agents sidebar never renders it, so an empty
/// string is both wire-compatible and non-disclosing. The absence guarantee is asserted by
/// `commands::agent_sidebar_commands::tests::remote_agent_sidebar_projection_blanks_only_the_worktree_path`.
pub(crate) fn strip_worktree_paths_from_sidebar_groups(
    groups: &mut AgentSidebarConversationGroupsResponse,
) {
    for group in &mut groups.groups {
        for row in &mut group.rows {
            if let Some(workspace) = row.workspace.as_mut() {
                workspace.worktree_path = String::new();
            }
        }
    }
}
