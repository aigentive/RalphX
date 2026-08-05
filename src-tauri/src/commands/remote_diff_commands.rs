//! Spawn-free workspace-change snapshot reads for the remote facade.
//!
//! The local diff commands populate snapshots while they already own live inspection. These
//! twins only read those inert snapshots and deliberately carry no `DiffService`, `GitService`,
//! or `resolve_git_cli_path`; a paired client cannot turn a Changes read into a git launch.

use serde::Serialize;

use crate::application::{DiffRefKind, FileDiff, FileDiffPage};
use crate::commands::diff_commands::{
    get_agent_workspace_change_summary_snapshot, get_agent_workspace_file_diff_page_snapshot,
    get_agent_workspace_file_diff_snapshot, get_agent_workspace_review_snapshot,
    AgentWorkspaceChangeSummaryResponse, AgentWorkspaceContextSource, AgentWorkspaceReviewResponse,
};
use crate::domain::entities::ChatConversationId;

#[derive(Debug, Clone, Serialize)]
pub struct RemoteWorkspaceSnapshotResponse<T> {
    pub snapshot: Option<T>,
    pub captured_at: Option<String>,
    pub cache_version: Option<String>,
    pub context_source: Option<AgentWorkspaceContextSource>,
}

fn envelope<T>(
    snapshot: Option<(T, String, String, AgentWorkspaceContextSource)>,
) -> RemoteWorkspaceSnapshotResponse<T> {
    match snapshot {
        Some((payload, captured_at, cache_version, context_source)) => {
            RemoteWorkspaceSnapshotResponse {
                snapshot: Some(payload),
                captured_at: Some(captured_at),
                cache_version: Some(cache_version),
                context_source: Some(context_source),
            }
        }
        None => RemoteWorkspaceSnapshotResponse {
            snapshot: None,
            captured_at: None,
            cache_version: None,
            context_source: None,
        },
    }
}

#[tauri::command]
pub async fn get_remote_agent_conversation_workspace_change_summary(
    conversation_id: String,
) -> RemoteWorkspaceSnapshotResponse<AgentWorkspaceChangeSummaryResponse> {
    let conversation_id = ChatConversationId::from_string(conversation_id);
    envelope(get_agent_workspace_change_summary_snapshot(
        &conversation_id,
    ))
}

#[tauri::command]
pub async fn get_remote_agent_conversation_workspace_review(
    conversation_id: String,
) -> RemoteWorkspaceSnapshotResponse<AgentWorkspaceReviewResponse> {
    let conversation_id = ChatConversationId::from_string(conversation_id);
    envelope(get_agent_workspace_review_snapshot(&conversation_id))
}

#[tauri::command]
pub async fn get_remote_agent_conversation_workspace_file_diff(
    conversation_id: String,
    file_path: String,
) -> RemoteWorkspaceSnapshotResponse<FileDiff> {
    let conversation_id = ChatConversationId::from_string(conversation_id);
    envelope(get_agent_workspace_file_diff_snapshot(
        &conversation_id,
        &file_path,
        "head",
    ))
}

#[tauri::command]
pub async fn get_remote_agent_conversation_workspace_commit_file_diff(
    conversation_id: String,
    commit_sha: String,
    file_path: String,
) -> RemoteWorkspaceSnapshotResponse<FileDiff> {
    let conversation_id = ChatConversationId::from_string(conversation_id);
    envelope(get_agent_workspace_file_diff_snapshot(
        &conversation_id,
        &file_path,
        &format!("commit:{commit_sha}"),
    ))
}

#[tauri::command]
pub async fn get_remote_agent_conversation_workspace_cumulative_file_diff(
    conversation_id: String,
    file_path: String,
) -> RemoteWorkspaceSnapshotResponse<FileDiff> {
    let conversation_id = ChatConversationId::from_string(conversation_id);
    envelope(get_agent_workspace_file_diff_snapshot(
        &conversation_id,
        &file_path,
        "cumulative_head",
    ))
}

#[tauri::command]
pub async fn get_remote_agent_conversation_workspace_file_diff_page(
    conversation_id: String,
    file_path: String,
    ref_kind: DiffRefKind,
    offset: usize,
    limit: usize,
) -> RemoteWorkspaceSnapshotResponse<FileDiffPage> {
    let conversation_id = ChatConversationId::from_string(conversation_id);
    envelope(get_agent_workspace_file_diff_page_snapshot(
        &conversation_id,
        &file_path,
        &ref_kind,
        offset,
        limit,
    ))
}
