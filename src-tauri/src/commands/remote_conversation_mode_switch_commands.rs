//! Spawn-free remote conversation MODE SWITCH (WP5a).
//!
//! `switch_agent_conversation_mode` itself can never be registered: its body prepares the
//! conversation workspace — `GitService::ref_exists` and the publish path's
//! `inspect_repository_capability` -> `ensure_git_worktree` — which are detector-(c)
//! process-launch sinks, and the process floor is absolute. The consequence for a paired device
//! was a dead end with teeth: remote conversations START pinned to mode `"chat"` (the
//! conversation-start intent host-pins it), so a remote client could reach chat and NOTHING else,
//! forever. Edit, Plan, and Ideation were unreachable, not merely slower.
//!
//! So the surface is redesigned rather than the floor relaxed: this module persists a MODE SWITCH
//! INTENT, and a host-owned dispatcher loop (`spawn_remote_conversation_mode_switch_dispatcher`)
//! is the sole holder of the worktree-preparing switch path.
//!
//! The contract this module keeps, and why it is its own module:
//!
//! - It takes **no** `tauri::AppHandle`, **no** `ExecutionState`, and never builds a
//!   `ChatService`. Those are the three carriers of process-spawn authority, so their absence is
//!   mechanically checkable by reading this file (asserted by
//!   `the_spawn_free_remote_conversation_mode_switch_module_carries_no_authority_carriers`, and
//!   over the CALL GRAPH by
//!   `remote_conversation_mode_switch_request_carries_no_spawn_authority`).
//! - Its only write is one durable intent row, the LAST statement (validate-then-persist,
//!   create-last).
//! - The intent carries the TARGET MODE and nothing else. `baseRefKind`, `baseBranchMode`,
//!   `baseRef`, `baseDisplayName`, `baseSourcePullRequest`, and `runtimeOverride` are all ABSENT
//!   from the wire rather than pinned. Every one of them steers real workspace preparation — a
//!   client-named `baseRef` would aim `ensure_git_worktree` at a ref of the client's choosing —
//!   and field absence is a stronger pin than a pinned value because there is nothing to forge.
//!   The host resolves all of them at drain time through its own defaults.
//!
//! ## The live-run rule, and why it is a REFUSAL
//!
//! The local `switch_agent_conversation_mode` Tauri command uses
//! `ModeSwitchRunningAgentPolicy::StopWithService`: it STOPS the running agent (through
//! `ChatService::stop_agent`, which resolves `pkill`) and only then switches. The AppState-only
//! sibling `switch_agent_conversation_mode_for_state` uses `ModeSwitchRunningAgentPolicy::Reject`
//! and returns `"Cannot change mode while the agent is running"` instead.
//!
//! This intent mirrors the REJECT policy, deliberately, and the dispatcher calls the
//! `..._for_state` variant so it never has to stop anything:
//!
//! - Stopping is a separate, explicitly user-initiated authority. WP2 already registered it as
//!   its own intent (`request_remote_agent_stop`) at `ui:operate`. Folding an implicit stop into
//!   the mode picker would let one `ui:agent` grant terminate a live run as a side effect of a
//!   dropdown change, which is exactly the kind of hidden authority the intent split exists to
//!   prevent.
//! - It keeps the dispatcher's reach exactly one function wide. If the dispatcher held a
//!   `ChatService` it would hold the process-terminating path too, and the WP2 argument that the
//!   stop path lives in exactly one host-owned loop would stop being true.
//!
//! So a remote user who wants to switch mid-run taps Stop (WP2) and then switches. The refusal is
//! typed (`REMOTE_MODE_SWITCH_RUN_ALREADY_LIVE`) so the client can say that, rather than showing
//! an opaque failure.

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspaceMode, ChatContextType, ChatConversationId, ProjectId,
    RemoteConversationModeSwitchRequest, RemoteConversationModeSwitchStatus,
};

/// A precondition read failed. Deliberately distinct from "absent" so an unavailable repository
/// can never be mistaken for a satisfied precondition.
pub const REMOTE_MODE_SWITCH_LOOKUP_FAILED: &str = "REMOTE_MODE_SWITCH_LOOKUP_FAILED";
/// No conversation id was supplied.
pub const REMOTE_MODE_SWITCH_EMPTY_CONVERSATION: &str = "REMOTE_MODE_SWITCH_EMPTY_CONVERSATION";
/// The named conversation does not exist on this host.
pub const REMOTE_MODE_SWITCH_CONVERSATION_NOT_FOUND: &str =
    "REMOTE_MODE_SWITCH_CONVERSATION_NOT_FOUND";
/// The conversation exists but is archived, so its mode may not be changed.
pub const REMOTE_MODE_SWITCH_CONVERSATION_ARCHIVED: &str =
    "REMOTE_MODE_SWITCH_CONVERSATION_ARCHIVED";
/// The conversation is not a project conversation owned by the named project. The scope comes off
/// the PERSISTED row, never off the request, so a client cannot aim a switch at a project it does
/// not name.
pub const REMOTE_MODE_SWITCH_PROJECT_MISMATCH: &str = "REMOTE_MODE_SWITCH_PROJECT_MISMATCH";
/// The requested mode is not a known `AgentConversationWorkspaceMode`.
pub const REMOTE_MODE_SWITCH_INVALID_MODE: &str = "REMOTE_MODE_SWITCH_INVALID_MODE";
/// A run is live for this conversation. See the module doc: this mirrors the REJECT policy of
/// `switch_agent_conversation_mode_for_state` rather than implicitly stopping the agent.
pub const REMOTE_MODE_SWITCH_RUN_ALREADY_LIVE: &str = "REMOTE_MODE_SWITCH_RUN_ALREADY_LIVE";
/// An unsettled switch to a DIFFERENT mode is already in flight for this conversation. Refused
/// rather than replaced: the dispatcher may already have claimed it and begun preparing a
/// worktree for the mode it named.
pub const REMOTE_MODE_SWITCH_CONFLICTING_PENDING: &str = "REMOTE_MODE_SWITCH_CONFLICTING_PENDING";
/// The intent row could not be persisted.
pub const REMOTE_MODE_SWITCH_ENQUEUE_FAILED: &str = "REMOTE_MODE_SWITCH_ENQUEUE_FAILED";
/// The status read could not resolve the request.
pub const REMOTE_MODE_SWITCH_REQUEST_NOT_FOUND: &str = "REMOTE_MODE_SWITCH_REQUEST_NOT_FOUND";

/// Dispatcher-side: a run went live between persist and claim. Retryable — the client should stop
/// the agent first, then re-request.
pub const REMOTE_MODE_SWITCH_RUN_WENT_LIVE: &str = "REMOTE_MODE_SWITCH_RUN_WENT_LIVE";
/// Dispatcher-side: the conversation vanished between persist and drain.
pub const REMOTE_MODE_SWITCH_CONVERSATION_GONE: &str = "REMOTE_MODE_SWITCH_CONVERSATION_GONE";
/// Dispatcher-side: the host-side switch itself failed (an invalid transition, a locked
/// workspace, a disabled capability, a worktree that could not be prepared). Terminal and
/// VISIBLE — a mode the UI shows as changed but the host never applied is the exact drift this
/// surface exists to avoid.
pub const REMOTE_MODE_SWITCH_HOST_SWITCH_FAILED: &str = "REMOTE_MODE_SWITCH_HOST_SWITCH_FAILED";

/// Input for `request_remote_agent_conversation_mode_switch`.
///
/// There is deliberately NO base/branch/runtime-override field: see the module doc. The host
/// resolves workspace preparation from its own defaults at drain time.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestRemoteAgentConversationModeSwitchInput {
    pub conversation_id: String,
    pub project_id: String,
    pub mode: String,
}

/// Result of persisting a mode-switch intent. Named for what the closure did (persisted a
/// request), not for what the host later does (switch).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RequestRemoteAgentConversationModeSwitchResponse {
    pub mode_switch_request_id: String,
    pub conversation_id: String,
    pub status: RemoteConversationModeSwitchStatus,
    /// `true` when this call joined an already-unsettled switch to the SAME mode instead of
    /// creating a new one.
    pub deduplicated: bool,
    pub created_at: String,
}

/// The client's post-submit poll target.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteConversationModeSwitchRequestStatusView {
    pub id: String,
    pub conversation_id: String,
    pub target_mode: String,
    pub status: RemoteConversationModeSwitchStatus,
    pub error_code: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Persist a mode-switch intent for a host-run conversation. No workspace is prepared here.
#[tauri::command]
pub async fn request_remote_agent_conversation_mode_switch(
    input: RequestRemoteAgentConversationModeSwitchInput,
    state: State<'_, AppState>,
) -> Result<RequestRemoteAgentConversationModeSwitchResponse, String> {
    request_remote_agent_conversation_mode_switch_for_state(&state, input).await
}

/// State-only body, so every precondition can be falsified through the production path.
pub async fn request_remote_agent_conversation_mode_switch_for_state(
    state: &AppState,
    input: RequestRemoteAgentConversationModeSwitchInput,
) -> Result<RequestRemoteAgentConversationModeSwitchResponse, String> {
    // 1. Identity.
    let conversation_id = input.conversation_id.trim();
    if conversation_id.is_empty() {
        return Err(REMOTE_MODE_SWITCH_EMPTY_CONVERSATION.to_string());
    }
    let conversation_id = ChatConversationId::from_string(conversation_id.to_string());

    // 2. The target mode must be a KNOWN variant. Parsed here rather than carried as a free
    //    string so an unknown mode can never reach the host switch path at all.
    let target_mode = input
        .mode
        .trim()
        .parse::<AgentConversationWorkspaceMode>()
        .map_err(|_| REMOTE_MODE_SWITCH_INVALID_MODE.to_string())?;

    // 3. The conversation must exist — fail-closed tri-state (Err != None != Some). A repo
    //    outage must not read as "unknown conversation" and must not read as "fine either".
    let conversation = state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .map_err(|error| {
            tracing::warn!(
                conversation_id = %conversation_id.as_str(),
                %error,
                "remote mode switch refused: conversation lookup failed"
            );
            REMOTE_MODE_SWITCH_LOOKUP_FAILED.to_string()
        })?
        .ok_or_else(|| REMOTE_MODE_SWITCH_CONVERSATION_NOT_FOUND.to_string())?;

    if conversation.is_archived() {
        return Err(REMOTE_MODE_SWITCH_CONVERSATION_ARCHIVED.to_string());
    }

    // 4. Ownership. The scope is taken from the PERSISTED conversation and compared to the
    //    client's claim; a mismatch is a refusal, never a silent re-target. The host switch path
    //    additionally refuses any non-Project conversation ("Only project agent conversations can
    //    change mode"), so this check is the remote-side half of the same rule.
    let requested_project = input.project_id.trim();
    if conversation.context_type != ChatContextType::Project
        || conversation.context_id != requested_project
    {
        return Err(REMOTE_MODE_SWITCH_PROJECT_MISMATCH.to_string());
    }
    let project_id = ProjectId::from_string(conversation.context_id.clone());

    // 5. The project must still exist. `Err` is its own refusal, never "absent".
    if state
        .project_repo
        .get_by_id(&project_id)
        .await
        .map_err(|error| {
            tracing::warn!(%error, "remote mode switch refused: project lookup failed");
            REMOTE_MODE_SWITCH_LOOKUP_FAILED.to_string()
        })?
        .is_none()
    {
        return Err(REMOTE_MODE_SWITCH_PROJECT_MISMATCH.to_string());
    }

    // 6. Liveness — see the module doc. Mirrors `ModeSwitchRunningAgentPolicy::Reject`, and fails
    //    closed in both directions: a read error is its own refusal, never "no run" (which would
    //    let a switch race a live run into a worktree teardown) and never "live" (which would
    //    refuse a legitimate switch).
    let active_run = state
        .agent_run_repo
        .get_active_for_conversation(&conversation_id)
        .await
        .map_err(|error| {
            tracing::warn!(
                conversation_id = %conversation_id.as_str(),
                %error,
                "remote mode switch refused: run liveness read failed"
            );
            REMOTE_MODE_SWITCH_LOOKUP_FAILED.to_string()
        })?;
    if active_run.is_some() {
        return Err(REMOTE_MODE_SWITCH_RUN_ALREADY_LIVE.to_string());
    }

    // 7. Dedupe. A repeat pick of the SAME mode joins the in-flight switch; a pick of a DIFFERENT
    //    mode is REFUSED. Refusing rather than replacing is the simplest honest semantics: the
    //    dispatcher may already have claimed the row and begun preparing a worktree for the mode
    //    it named, and a replace would leave the host mid-preparation for a mode nobody wants.
    //    The read fails closed for the same reason as step 3.
    let existing = state
        .remote_conversation_mode_switch_request_repo
        .find_unsettled_mode_switch_request_for_conversation(&conversation_id)
        .await
        .map_err(|error| {
            tracing::warn!(
                conversation_id = %conversation_id.as_str(),
                %error,
                "remote mode switch refused: dedupe lookup failed"
            );
            REMOTE_MODE_SWITCH_LOOKUP_FAILED.to_string()
        })?;
    if let Some(existing) = existing {
        if existing.target_mode != target_mode {
            return Err(REMOTE_MODE_SWITCH_CONFLICTING_PENDING.to_string());
        }
        return Ok(RequestRemoteAgentConversationModeSwitchResponse {
            mode_switch_request_id: existing.id,
            conversation_id: conversation_id.as_str(),
            status: existing.status,
            deduplicated: true,
            created_at: existing.created_at.to_rfc3339(),
        });
    }

    // 8. Already there? A BENIGN terminal, not an error and not a no-op return: the row is
    //    persisted directly in `AlreadyInMode` so the client's poll target exists and settles on
    //    its first read, exactly like every other terminal. Returning without a row would leave
    //    the poll with nothing to read; returning `Pending` would make the dispatcher prepare a
    //    worktree for a mode the conversation is already in.
    //
    //    Current mode is resolved the SAME way the host switch path resolves it — the
    //    conversation's own `agent_mode`, else the workspace's mode, else Chat — so the two can
    //    never disagree about what "already there" means.
    let existing_workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation.id)
        .await
        .map_err(|error| {
            tracing::warn!(%error, "remote mode switch refused: workspace lookup failed");
            REMOTE_MODE_SWITCH_LOOKUP_FAILED.to_string()
        })?;
    let current_mode = conversation
        .agent_mode
        .or_else(|| existing_workspace.as_ref().map(|workspace| workspace.mode))
        .unwrap_or(AgentConversationWorkspaceMode::Chat);

    let status = if current_mode == target_mode {
        RemoteConversationModeSwitchStatus::AlreadyInMode
    } else {
        RemoteConversationModeSwitchStatus::Pending
    };

    // 9. The intent row — the LAST write (validate-then-persist, create-last).
    let now = chrono::Utc::now();
    let request = RemoteConversationModeSwitchRequest {
        id: uuid::Uuid::new_v4().to_string(),
        conversation_id: conversation_id.clone(),
        project_id,
        target_mode,
        status,
        error_code: None,
        // Deferred with the same rationale as the other three intent surfaces: stamping the
        // authenticated device id needs `identity.device_id` threaded from the invoke layer
        // through `dispatch`. Left empty here; revoke-cancel rides the same follow-up.
        requested_by_device_id: String::new(),
        claimed_at: None,
        created_at: now,
        updated_at: now,
    };
    let stored = state
        .remote_conversation_mode_switch_request_repo
        .create_mode_switch_request(request)
        .await
        .map_err(|error| {
            tracing::warn!(%error, "remote mode switch failed to persist intent");
            REMOTE_MODE_SWITCH_ENQUEUE_FAILED.to_string()
        })?;

    Ok(RequestRemoteAgentConversationModeSwitchResponse {
        mode_switch_request_id: stored.id,
        conversation_id: conversation_id.as_str(),
        status: stored.status,
        deduplicated: false,
        created_at: stored.created_at.to_rfc3339(),
    })
}

/// Poll one mode-switch intent by id. Pure repository read, fail-closed.
#[tauri::command]
pub async fn get_remote_conversation_mode_switch_request(
    mode_switch_request_id: String,
    state: State<'_, AppState>,
) -> Result<RemoteConversationModeSwitchRequestStatusView, String> {
    get_remote_conversation_mode_switch_request_for_state(&state, &mode_switch_request_id).await
}

pub async fn get_remote_conversation_mode_switch_request_for_state(
    state: &AppState,
    mode_switch_request_id: &str,
) -> Result<RemoteConversationModeSwitchRequestStatusView, String> {
    let request = state
        .remote_conversation_mode_switch_request_repo
        .get_mode_switch_request(mode_switch_request_id)
        .await
        .map_err(|error| {
            tracing::warn!(%error, "remote mode switch status read failed");
            REMOTE_MODE_SWITCH_LOOKUP_FAILED.to_string()
        })?
        .ok_or_else(|| REMOTE_MODE_SWITCH_REQUEST_NOT_FOUND.to_string())?;

    Ok(RemoteConversationModeSwitchRequestStatusView {
        id: request.id,
        conversation_id: request.conversation_id.as_str(),
        target_mode: request.target_mode.to_string(),
        status: request.status,
        error_code: request.error_code,
        created_at: request.created_at.to_rfc3339(),
        updated_at: request.updated_at.to_rfc3339(),
    })
}

#[cfg(test)]
#[path = "remote_conversation_mode_switch_commands_tests.rs"]
mod tests;
