//! Conversation-lineage resolution for the delegation seam.
//!
//! Delegation calls carry two distinct conversation identities that older code conflated:
//!
//! * the **trusted caller conversation** (`x-ralphx-conversation-id`), which is the runtime
//!   actually invoking `delegate_start`, and
//! * the **workspace anchor conversation** (`parent_conversation_id` as sent by the MCP
//!   server), which is the conversation whose agent workspace owns the worktree.
//!
//! For child runtimes — Workspace Review conversations, forks, and started child
//! conversations — these differ, so the caller must be validated against the anchor's
//! lineage instead of compared for equality.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use axum::http::StatusCode;

use crate::application::agent_conversation_workspace::resolve_agent_conversation_workspace_path_for_send;
use crate::application::chat_service::chat_service_context;
use crate::domain::entities::{
    ChatContextType, ChatConversation, ChatConversationId, Project, ProjectId,
};
use crate::http_server::types::HttpServerState;

use super::{
    json_error, load_project_by_id, validate_project_working_directory, JsonError,
    ResolvedDelegateParent,
};

/// Fail-closed 400 for delegation calls whose trusted runtime conversation cannot be proven to
/// be the resolved caller or one of its descendants. This is a lineage failure, not a delegation
/// policy denial: the calling agent may well be allowed to delegate to the requested target.
pub const DELEGATION_CALLER_LINEAGE_ERROR: &str = "Trusted caller conversation is not the delegating runtime or a descendant of the resolved parent conversation";

/// Maximum number of conversations visited when climbing a `parent_conversation_id` chain.
/// Real lineages are one or two hops deep; the bound keeps corrupt data from driving unbounded
/// repository reads.
const MAX_CONVERSATION_LINEAGE_DEPTH: usize = 8;

async fn load_conversation_by_id(
    state: &HttpServerState,
    conversation_id: &str,
) -> Result<Option<ChatConversation>, JsonError> {
    state
        .app_state
        .chat_conversation_repo
        .get_by_id(&ChatConversationId::from_string(
            conversation_id.to_string(),
        ))
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load conversation lineage: {error}"),
            )
        })
}

/// Loads `start` plus its ancestors, nearest first, bounded by [`MAX_CONVERSATION_LINEAGE_DEPTH`].
///
/// A dangling `parent_conversation_id` ends the chain; a cycle is a durable-state defect and is
/// reported rather than silently truncated.
async fn load_conversation_lineage(
    state: &HttpServerState,
    start: ChatConversation,
) -> Result<Vec<ChatConversation>, JsonError> {
    let mut lineage: Vec<ChatConversation> = Vec::new();
    let mut visited: HashSet<String> = HashSet::new();
    let mut current = Some(start);

    while let Some(conversation) = current {
        if !visited.insert(conversation.id.as_str()) {
            return Err(json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!(
                    "Conversation lineage for '{}' contains a cycle",
                    conversation.id.as_str()
                ),
            ));
        }
        let next_id = conversation.parent_conversation_id.clone();
        lineage.push(conversation);
        if lineage.len() >= MAX_CONVERSATION_LINEAGE_DEPTH {
            break;
        }
        current = match next_id {
            Some(parent_id) => load_conversation_by_id(state, &parent_id).await?,
            None => None,
        };
    }

    Ok(lineage)
}

/// Resolves a project delegate's working directory from the nearest agent workspace in the
/// conversation's own lineage.
///
/// Self first: a forked conversation owning its own workspace keeps its own worktree. Only when
/// no conversation in the chain owns a workspace does this fall back to the project checkout,
/// preserving plain project-conversation behavior. Repository failures propagate instead of
/// collapsing into "no workspace, use the project root".
pub(super) async fn resolve_project_workspace_working_directory(
    state: &HttpServerState,
    project: &Project,
    start_conversation: Option<&ChatConversation>,
) -> Result<PathBuf, JsonError> {
    let Some(start_conversation) = start_conversation else {
        return validate_project_working_directory(project);
    };
    let lineage = load_conversation_lineage(state, start_conversation.clone()).await?;

    for conversation in &lineage {
        let workspace = state
            .app_state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation.id)
            .await
            .map_err(|error| {
                json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to load parent agent workspace: {error}"),
                )
            })?;
        if let Some(workspace) = workspace {
            return resolve_agent_conversation_workspace_path_for_send(project, &workspace)
                .map_err(|error| json_error(StatusCode::CONFLICT, error.to_string()));
        }
    }

    validate_project_working_directory(project)
}

/// Adopts the transport-trusted runtime conversation as the delegation caller once it is proven
/// to sit at or below the resolved parent/anchor conversation.
///
/// # Errors
///
/// * `404` when the trusted conversation does not exist (fail-closed; never falls back to the
///   anchor).
/// * `400` [`DELEGATION_CALLER_LINEAGE_ERROR`] when the trusted conversation belongs to another
///   project or is not a descendant of the resolved caller/anchor.
/// * `500` when a repository read fails.
pub(super) async fn apply_trusted_caller_conversation(
    state: &HttpServerState,
    parent: &mut ResolvedDelegateParent,
    trusted_caller_conversation_id: Option<&str>,
) -> Result<(), JsonError> {
    let Some(trusted_id) = trusted_caller_conversation_id else {
        return Ok(());
    };
    // Nested delegation keeps its exact-match rule in `resolve_trusted_caller_agent_run_id`.
    if parent.context_type == ChatContextType::Delegation {
        return Ok(());
    }
    if parent.caller_conversation_id.as_deref() == Some(trusted_id) {
        return Ok(());
    }

    let trusted_conversation = load_conversation_by_id(state, trusted_id)
        .await?
        .ok_or_else(|| {
            json_error(
                StatusCode::NOT_FOUND,
                "Trusted caller conversation not found",
            )
        })?;

    let trusted_project_id = chat_service_context::resolve_project_id(
        trusted_conversation.context_type,
        &trusted_conversation.context_id,
        Arc::clone(&state.app_state.task_repo),
        Arc::clone(&state.app_state.ideation_session_repo),
        Arc::clone(&state.app_state.delegated_session_repo),
    )
    .await
    .ok_or_else(|| json_error(StatusCode::BAD_REQUEST, DELEGATION_CALLER_LINEAGE_ERROR))?;
    if trusted_project_id != parent.project_id {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            DELEGATION_CALLER_LINEAGE_ERROR,
        ));
    }

    let lineage = load_conversation_lineage(state, trusted_conversation).await?;
    let anchors: Vec<&str> = [
        parent.caller_conversation_id.as_deref(),
        parent.workspace_anchor_conversation_id.as_deref(),
    ]
    .into_iter()
    .flatten()
    .collect();
    if !anchors.is_empty() {
        // Downward-only: the trusted conversation must sit strictly below an anchor, so a
        // sibling or an ancestor impersonating a child is still rejected.
        let ancestor_ids: HashSet<String> = lineage
            .iter()
            .skip(1)
            .map(|conversation| conversation.id.as_str())
            .collect();
        if !anchors.iter().any(|anchor| ancestor_ids.contains(*anchor)) {
            return Err(json_error(
                StatusCode::BAD_REQUEST,
                DELEGATION_CALLER_LINEAGE_ERROR,
            ));
        }
    }

    parent.caller_conversation_id = Some(trusted_id.to_string());
    // The delegation belongs to the runtime that requested it: its Delegate widget, streaming
    // run binding, and delegated-conversation lineage all target the caller instead of the
    // workspace anchor, which would otherwise receive another conversation's writes.
    parent.parent_conversation_id = Some(trusted_id.to_string());
    if parent.context_type == ChatContextType::Project {
        let project =
            load_project_by_id(state, &ProjectId::from_string(parent.project_id.clone())).await?;
        parent.working_directory =
            resolve_project_workspace_working_directory(state, &project, lineage.first()).await?;
    }

    Ok(())
}
