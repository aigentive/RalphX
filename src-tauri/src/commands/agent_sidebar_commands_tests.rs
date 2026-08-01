use chrono::{Duration, Utc};

use super::*;
use crate::domain::entities::{
    AgentRun, DelegationParkId, DelegationParkJob, DelegationParkState, DelegationWakePolicy,
};

fn sidebar_input(project_id: &ProjectId) -> AgentSidebarConversationsInput {
    AgentSidebarConversationsInput {
        project_ids: vec![project_id.as_str().to_string()],
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
    }
}

#[tokio::test]
async fn armed_park_keeps_completed_coordinator_working_and_counts_unsettled_delegates() {
    let state = AppState::new_test();
    let project = state
        .project_repo
        .create(Project::new(
            "parked-sidebar".to_string(),
            "/tmp/parked-sidebar".to_string(),
        ))
        .await
        .unwrap();
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project.id.clone()))
        .await
        .unwrap();
    let mut parent_run = AgentRun::new(conversation.id);
    parent_run.status = AgentRunStatus::Completed;
    let parent_run_id = parent_run.id.clone();
    state.agent_run_repo.create(parent_run).await.unwrap();

    let delegate_run = AgentRun::new(conversation.id);
    let now = Utc::now();
    state
        .delegation_park_repo
        .arm(DelegationPark {
            id: DelegationParkId::new(),
            parent_conversation_id: conversation.id,
            parent_agent_run_id: parent_run_id,
            generation: 0,
            wake_policy: DelegationWakePolicy::AllSettled,
            wake_on_failure: true,
            state: DelegationParkState::Armed,
            deadline_at: now + Duration::hours(1),
            wake_attempts: 0,
            last_error: None,
            created_at: now,
            updated_at: now,
            jobs: vec![
                DelegationParkJob {
                    job_id: "settled".to_string(),
                    delegated_session_id: "delegate-session-1".to_string(),
                    delegated_agent_run_id: delegate_run.id.clone(),
                    settled_status: Some("completed".to_string()),
                },
                DelegationParkJob {
                    job_id: "waiting-1".to_string(),
                    delegated_session_id: "delegate-session-2".to_string(),
                    delegated_agent_run_id: AgentRun::new(conversation.id).id,
                    settled_status: None,
                },
                DelegationParkJob {
                    job_id: "waiting-2".to_string(),
                    delegated_session_id: "delegate-session-3".to_string(),
                    delegated_agent_run_id: AgentRun::new(conversation.id).id,
                    settled_status: None,
                },
            ],
        })
        .await
        .unwrap();

    let response =
        list_agent_sidebar_conversations_for_app_state(sidebar_input(&project.id), &state)
            .await
            .unwrap();
    let working_row = response
        .groups
        .iter()
        .find(|group| group.key == "working")
        .and_then(|group| {
            group
                .rows
                .iter()
                .find(|row| row.conversation.id == conversation.id.as_str())
        })
        .expect("completed parked coordinator should be working");

    assert_eq!(working_row.attention_lane, "working");
    assert_eq!(working_row.parked_delegate_count, 2);
    assert!(response
        .groups
        .iter()
        .find(|group| group.key == "needs")
        .is_none_or(|group| group
            .rows
            .iter()
            .all(|row| row.conversation.id != conversation.id.as_str())));
}
