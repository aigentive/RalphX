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
            wake_claimed_at: None,
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

/// The listing derives lanes, labels, refs, and verbs from `SidebarWorkspaceFacts` instead of a
/// composed workspace response. If the two ever disagree, a plan-branch-linked workspace would be
/// classified into a different lane than the response the rest of the surface renders — silently.
#[tokio::test]
async fn sidebar_workspace_facts_match_the_composed_response() {
    use crate::domain::entities::plan_branch::PrPushStatus;
    use crate::domain::entities::{
        AgentConversationWorkspaceMode, ArtifactId, IdeationAnalysisBaseRefKind, IdeationSessionId,
        PlanBranch, PlanBranchStatus,
    };

    let state = AppState::new_test();
    let project = state
        .project_repo
        .create(Project::new(
            "facts-parity".to_string(),
            "/tmp/facts-parity".to_string(),
        ))
        .await
        .unwrap();
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project.id.clone()))
        .await
        .unwrap();

    // A linked plan branch whose publication differs from the workspace's own columns is the only
    // shape where entity-derived and response-derived facts can diverge.
    let mut plan_branch = PlanBranch::new(
        ArtifactId::new(),
        IdeationSessionId::new(),
        project.id.clone(),
        "ralphx/plan/facts-parity".to_string(),
        "main".to_string(),
    );
    plan_branch.status = PlanBranchStatus::Merged;
    plan_branch.pr_number = Some(4242);
    plan_branch.pr_url = Some("https://github.com/owner/repo/pull/4242".to_string());
    plan_branch.pr_push_status = PrPushStatus::Pushed;
    let plan_branch = state.plan_branch_repo.create(plan_branch).await.unwrap();

    let mut workspace = AgentConversationWorkspace::new(
        conversation.id,
        project.id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        Some("base-1".to_string()),
        "ralphx/facts-parity/agent".to_string(),
        "/tmp/ralphx/facts-parity/agent".to_string(),
    );
    // Deliberately stale relative to the plan branch.
    workspace.publication_pr_number = Some(1);
    workspace.publication_pr_status = Some("open".to_string());
    workspace.publication_push_status = Some("pending".to_string());
    workspace.pr_supervision_status = Some("monitoring".to_string());
    workspace.pr_auto_merge_current = Some(true);
    workspace.linked_plan_branch_id = Some(plan_branch.id.clone());
    let workspace = state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();

    let facts = SidebarWorkspaceFacts::from_entity(&workspace, Some(&plan_branch));
    let response =
        agent_workspace_response_without_repair_recovery_for_state(&state, workspace.clone())
            .await
            .unwrap();

    assert_eq!(facts, SidebarWorkspaceFacts::from_response(&response));
    // Proves the overlay was actually exercised rather than both sides reading stale columns.
    assert_eq!(facts.publication_pr_number, Some(4242));
    assert_eq!(facts.publication_pr_status.as_deref(), Some("merged"));

    // Without the plan branch the facts fall back to the workspace's own columns, so the parity
    // above is not an artifact of the overlay always winning.
    let unlinked = SidebarWorkspaceFacts::from_entity(&workspace, None);
    assert_eq!(unlinked.publication_pr_number, Some(1));
    assert_eq!(unlinked.publication_pr_status.as_deref(), Some("open"));
}

/// Composition is the expensive part of the listing: each conversation response costs a runtime
/// attribution read and each workspace response costs a mode lock, a repair attempt, and an
/// autofix spend query. Those must scale with the returned page, not with the number of
/// conversations enumerated.
#[tokio::test]
async fn listing_composes_responses_only_for_rows_a_page_returns() {
    let state = AppState::new_test();
    let project = state
        .project_repo
        .create(Project::new(
            "hydration-scope".to_string(),
            "/tmp/hydration-scope".to_string(),
        ))
        .await
        .unwrap();

    const CONVERSATION_COUNT: usize = 9;
    const LIMIT_PER_GROUP: u32 = 2;
    for _ in 0..CONVERSATION_COUNT {
        state
            .chat_conversation_repo
            .create(ChatConversation::new_project(project.id.clone()))
            .await
            .unwrap();
    }

    let mut input = sidebar_input(&project.id);
    input.limit_per_group = Some(LIMIT_PER_GROUP);
    let response = list_agent_sidebar_conversations_for_app_state(input, &state)
        .await
        .unwrap();

    let returned_rows: usize = response.groups.iter().map(|group| group.rows.len()).sum();
    let enumerated: i64 = response.groups.iter().map(|group| group.total).sum();

    // Totals stay accurate over every enumerated row even though only a page is composed.
    assert_eq!(enumerated, CONVERSATION_COUNT as i64);
    assert!(
        returned_rows < CONVERSATION_COUNT,
        "the fixture must actually paginate for this test to mean anything"
    );
    // Each returned row carries a fully composed conversation response.
    for group in &response.groups {
        assert!(group.rows.len() <= LIMIT_PER_GROUP as usize);
        for row in &group.rows {
            assert!(!row.conversation.id.is_empty());
        }
    }
}

/// Enumerating the sidebar is a read boundary. It previously composed every workspace through
/// `agent_workspace_response_with_pr_supervision_for_state`, which schedules PR-supervision
/// recovery — work that can fetch, enqueue an agent, or continue publication. A listing must not
/// do that; workspace open, run completed, and startup remain the recovery triggers.
#[tokio::test]
async fn listing_schedules_no_pr_supervision_recovery() {
    use crate::application::agent_workspace_pr_supervision_recovery::recovery_was_claimed_for_test;
    use crate::domain::entities::{AgentConversationWorkspaceMode, IdeationAnalysisBaseRefKind};

    let state = AppState::new_test();
    let project = state
        .project_repo
        .create(Project::new(
            "no-recovery".to_string(),
            "/tmp/no-recovery".to_string(),
        ))
        .await
        .unwrap();
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project.id.clone()))
        .await
        .unwrap();

    // A published, actively supervised workspace: the shape that previously scheduled recovery.
    let mut workspace = AgentConversationWorkspace::new(
        conversation.id,
        project.id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        Some("base-1".to_string()),
        "ralphx/no-recovery/agent".to_string(),
        "/tmp/ralphx/no-recovery/agent".to_string(),
    );
    workspace.publication_pr_number = Some(77);
    workspace.publication_pr_status = Some("open".to_string());
    workspace.publication_push_status = Some("pushed".to_string());
    workspace.pr_supervision_status = Some("monitoring".to_string());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();

    assert!(
        !recovery_was_claimed_for_test(&conversation.id),
        "fixture must start with no recovery claimed"
    );

    let response =
        list_agent_sidebar_conversations_for_app_state(sidebar_input(&project.id), &state)
            .await
            .unwrap();

    // The row is genuinely enumerated, so the assertion below is about the read path rather than
    // about the workspace being filtered out.
    assert!(response
        .groups
        .iter()
        .flat_map(|group| group.rows.iter())
        .any(|row| row.conversation.id == conversation.id.as_str()));
    assert!(
        !recovery_was_claimed_for_test(&conversation.id),
        "the sidebar listing must not schedule PR supervision recovery"
    );
}
