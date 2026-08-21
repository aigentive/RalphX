use std::sync::Arc;

use chrono::Utc;

use crate::application::AppState;
use crate::commands::agent_conversation_mute_commands::{
    set_agent_conversation_muted_for_app_state, SetAgentConversationMutedInput,
};
use crate::commands::agent_sidebar_commands::{
    list_agent_sidebar_conversations_for_app_state, AgentSidebarConversationsInput,
};
use crate::commands::ExecutionState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentWorkspacePrReviewMonitor,
    AgentWorkspacePrReviewMonitorStatus, ChatConversation, IdeationAnalysisBaseRefKind, Project,
};

#[tokio::test]
async fn mute_command_persists_current_fingerprint_then_unmute_clears_it() {
    let state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_standalone())
        .await
        .expect("conversation should be created");

    set_agent_conversation_muted_for_app_state(
        SetAgentConversationMutedInput {
            conversation_id: conversation.id.as_str().to_string(),
            muted: true,
        },
        &state,
        &execution_state,
    )
    .await
    .expect("mute should persist");
    assert!(state
        .agent_conversation_mute_repo
        .get_by_conversation_id(&conversation.id)
        .await
        .expect("mute lookup should succeed")
        .is_some());

    set_agent_conversation_muted_for_app_state(
        SetAgentConversationMutedInput {
            conversation_id: conversation.id.as_str().to_string(),
            muted: false,
        },
        &state,
        &execution_state,
    )
    .await
    .expect("unmute should clear");
    assert!(state
        .agent_conversation_mute_repo
        .get_by_conversation_id(&conversation.id)
        .await
        .expect("mute lookup should succeed")
        .is_none());
}

#[tokio::test]
async fn mute_command_rejects_unknown_conversation() {
    let state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());
    let error = set_agent_conversation_muted_for_app_state(
        SetAgentConversationMutedInput {
            conversation_id: uuid::Uuid::new_v4().to_string(),
            muted: true,
        },
        &state,
        &execution_state,
    )
    .await
    .expect_err("unknown conversation cannot be muted");

    assert!(error.contains("agent conversation not found"));
}

/// The mute command must produce the SAME fingerprint as the sidebar read
/// path. This asserts it end-to-end: the saved mute has to actually match, and
/// a matched review mute demotes into `review_watching` rather than `Stale`.
#[tokio::test]
async fn mute_fingerprint_matches_the_sidebar_read_path_for_a_review_pr_conversation() {
    let state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());
    let project = state
        .project_repo
        .create(Project::new(
            "mute-review-parity".to_string(),
            "/tmp/mute-review-parity".to_string(),
        ))
        .await
        .expect("project should be created");
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project.id.clone()))
        .await
        .expect("conversation should be created");

    let mut workspace = AgentConversationWorkspace::new(
        conversation.id,
        project.id.clone(),
        AgentConversationWorkspaceMode::ReviewPr,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        None,
        format!("agent/{}", conversation.id),
        format!("/tmp/worktrees/{}", conversation.id),
    );
    workspace.publication_pr_number = Some(7);
    workspace.publication_pr_status = Some("open".to_string());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should be created");

    let now = Utc::now();
    state
        .agent_conversation_workspace_repo
        .upsert_pr_review_monitor(AgentWorkspacePrReviewMonitor {
            conversation_id: conversation.id,
            project_id: project.id.clone(),
            pr_number: 7,
            status: AgentWorkspacePrReviewMonitorStatus::AwaitingUser,
            monitor_enabled: true,
            auto_approve_enabled: false,
            first_review_completed: true,
            first_action_resolved: true,
            last_seen_head_sha: None,
            last_reviewed_head_sha: None,
            last_review_run_id: None,
            last_review_outcome: Some("approve".to_string()),
            last_submitted_review_id: None,
            review_artifact_id: None,
            review_artifact_head_sha: None,
            review_artifact_version: None,
            review_artifact_updated_at: None,
            last_error: None,
            created_at: now,
            updated_at: now,
        })
        .await
        .expect("monitor should be created");

    set_agent_conversation_muted_for_app_state(
        SetAgentConversationMutedInput {
            conversation_id: conversation.id.as_str().to_string(),
            muted: true,
        },
        &state,
        &execution_state,
    )
    .await
    .expect("mute should persist");

    let response = list_agent_sidebar_conversations_for_app_state(
        AgentSidebarConversationsInput {
            project_ids: vec![project.id.as_str().to_string()],
            include_archived: None,
            archived_only: None,
            search: None,
            publication_states: None,
            group_by: Some("inbox".to_string()),
            sort: None,
            limit_per_group: Some(6),
            offsets: None,
            pinned_conversation_ids: None,
            priority_conversation_ids: None,
        },
        &state,
    )
    .await
    .expect("sidebar listing should succeed");

    let (group_key, row) = response
        .groups
        .iter()
        .find_map(|group| {
            group
                .rows
                .iter()
                .find(|row| row.conversation.id == conversation.id.as_str())
                .map(|row| (group.key.as_str(), row))
        })
        .expect("conversation should appear in an inbox group");

    assert!(
        row.is_muted,
        "mute fingerprint diverged from the sidebar read path"
    );
    assert_eq!(group_key, "review_watching");
    assert_eq!(row.review_state.as_deref(), Some("needs_approval"));
}
