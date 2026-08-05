use super::*;
use crate::domain::entities::{
    AgentConversationMute, AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentRun,
    Automation, AutomationId, AutomationPlanApprovalMode, AutomationPrMergeMode, AutomationRunId,
    AutomationStatus, ChatConversation, IdeationAnalysisBaseRefKind, Project,
};

fn sidebar_input(project_id: &ProjectId) -> AgentSidebarConversationsInput {
    AgentSidebarConversationsInput {
        project_ids: vec![project_id.as_str().to_string()],
        include_archived: None,
        archived_only: None,
        search: None,
        publication_states: None,
        group_by: Some("publication".to_string()),
        sort: None,
        limit_per_group: Some(6),
        offsets: None,
        pinned_conversation_ids: None,
        priority_conversation_ids: None,
    }
}

async fn create_project(state: &AppState, name: &str) -> Project {
    let mut project = Project::new(name.to_string(), format!("/tmp/{name}"));
    project.base_branch = Some("develop".to_string());
    state.project_repo.create(project).await.unwrap()
}

async fn create_automation(
    state: &AppState,
    project_id: &ProjectId,
    id: &str,
    name: &str,
) -> Automation {
    let now = Utc::now();
    let automation = Automation {
        id: AutomationId::from_string(id),
        project_id: project_id.clone(),
        name: name.to_string(),
        status: AutomationStatus::Active,
        paused_reason_code: None,
        paused_reason_detail: None,
        goal_prompt: "Keep improving the project".to_string(),
        setup_conversation_id: None,
        provider_harness: "claude".to_string(),
        model_id: "sonnet".to_string(),
        logical_effort: None,
        run_mode: "edit".to_string(),
        base_ref_kind: "project_default".to_string(),
        base_ref: String::new(),
        base_display_name: None,
        base_source_pull_request_json: None,
        goal_items_json: None,
        chain_mode: "merged_base".to_string(),
        completion_signal: "pr_merged".to_string(),
        max_runs: 25,
        max_consecutive_failures: 3,
        first_run_prompt: Some("Run the next slice".to_string()),
        setup_analysis_summary: None,
        spec_artifact_id: None,
        authoring_state_json: None,
        plan_approval_mode: AutomationPlanApprovalMode::Manual,
        pr_merge_mode: AutomationPrMergeMode::Manual,
        plan_deep_verification: false,
        created_at: now,
        updated_at: now,
    };
    state.automation_repo.create(automation).await.unwrap()
}

async fn create_conversation(
    state: &AppState,
    project_id: &ProjectId,
    title: &str,
    created_at: DateTime<Utc>,
) -> ChatConversation {
    let mut conversation = ChatConversation::new_project(project_id.clone());
    conversation.title = Some(title.to_string());
    conversation.created_at = created_at;
    conversation.updated_at = created_at;
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap()
}

async fn create_standalone_conversation(
    state: &AppState,
    title: &str,
    created_at: DateTime<Utc>,
) -> ChatConversation {
    let mut conversation = ChatConversation::new_standalone();
    conversation.title = Some(title.to_string());
    conversation.created_at = created_at;
    conversation.updated_at = created_at;
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap()
}

#[test]
fn blocked_exhausted_repair_escalates_row_to_needs_lane() {
    let now = Utc::now();
    assert_eq!(
        attention_lane_for_row(
            false,
            SidebarPublicationState::Active,
            Some(AgentRunStatus::Running),
            None,
            true,
            false,
            now,
            None,
        ),
        SidebarAttentionLane::Needs
    );
    assert_eq!(
        attention_lane_for_row(
            false,
            SidebarPublicationState::Active,
            Some(AgentRunStatus::Running),
            None,
            false,
            false,
            now,
            None,
        ),
        SidebarAttentionLane::Working
    );
    assert_eq!(
        attention_lane_for_row(
            true,
            SidebarPublicationState::Merged,
            None,
            None,
            true,
            false,
            now,
            None
        ),
        SidebarAttentionLane::Done
    );
}

#[test]
fn held_repair_stays_in_needs_even_when_stale_or_an_armed_park_is_working() {
    let stale = Utc::now() - chrono::Duration::days(STALE_AFTER_DAYS + 1);
    for (last_activity_at, has_armed_delegation_park) in
        [(Utc::now(), false), (stale, false), (Utc::now(), true)]
    {
        assert_eq!(
            attention_lane_for_row_with_armed_park(
                false,
                SidebarPublicationState::Active,
                Some(AgentRunStatus::Running),
                None,
                false,
                true,
                last_activity_at,
                None,
                has_armed_delegation_park,
            ),
            SidebarAttentionLane::Needs,
        );
    }
}

#[tokio::test]
async fn latest_sort_uses_last_message_or_updated_activity_not_creation_time() {
    let state = AppState::new_test();
    let project = create_project(&state, "latest-activity-sort").await;
    let now = Utc::now();

    let mut created_most_recently = ChatConversation::new_project(project.id.clone());
    created_most_recently.title = Some("Stale activity".to_string());
    created_most_recently.created_at = now;
    created_most_recently.updated_at = now - chrono::Duration::minutes(30);
    created_most_recently.last_message_at = Some(now - chrono::Duration::minutes(30));
    let created_most_recently = state
        .chat_conversation_repo
        .create(created_most_recently)
        .await
        .unwrap();

    let mut fallback_to_updated = ChatConversation::new_project(project.id.clone());
    fallback_to_updated.title = Some("Updated activity".to_string());
    fallback_to_updated.created_at = now - chrono::Duration::minutes(1);
    fallback_to_updated.updated_at = now - chrono::Duration::minutes(10);
    let fallback_to_updated = state
        .chat_conversation_repo
        .create(fallback_to_updated)
        .await
        .unwrap();

    let mut latest_message = ChatConversation::new_project(project.id.clone());
    latest_message.title = Some("Latest message".to_string());
    latest_message.created_at = now - chrono::Duration::minutes(2);
    latest_message.updated_at = now - chrono::Duration::minutes(20);
    latest_message.last_message_at = Some(now);
    let latest_message = state
        .chat_conversation_repo
        .create(latest_message)
        .await
        .unwrap();

    let response =
        list_agent_sidebar_conversations_for_app_state(sidebar_input(&project.id), &state)
            .await
            .expect("sidebar conversations should load");
    let conversation_ids = response
        .groups
        .iter()
        .flat_map(|group| group.rows.iter())
        .map(|row| row.conversation.id.as_str())
        .collect::<Vec<_>>();

    assert_eq!(
        conversation_ids,
        vec![
            latest_message.id.as_str(),
            fallback_to_updated.id.as_str(),
            created_most_recently.id.as_str(),
        ]
    );
}

async fn create_automation_conversation(
    state: &AppState,
    project_id: &ProjectId,
    title: &str,
    created_at: DateTime<Utc>,
    automation_id: AutomationId,
    automation_run_id: Option<AutomationRunId>,
) -> ChatConversation {
    let mut conversation = ChatConversation::new_project(project_id.clone());
    conversation.title = Some(title.to_string());
    conversation.created_at = created_at;
    conversation.updated_at = created_at;
    conversation.automation_id = Some(automation_id);
    conversation.automation_run_id = automation_run_id;
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap()
}

async fn create_workspace(
    state: &AppState,
    conversation: &ChatConversation,
    project_id: &ProjectId,
    pr_number: Option<i64>,
    pr_status: Option<&str>,
    push_status: Option<&str>,
) {
    let mut workspace = AgentConversationWorkspace::new(
        conversation.id,
        project_id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "develop".to_string(),
        Some("Current branch (develop)".to_string()),
        None,
        format!("agent/{}", conversation.id),
        format!("/tmp/worktrees/{}", conversation.id),
    );
    workspace.publication_pr_number = pr_number;
    workspace.publication_pr_status = pr_status.map(str::to_string);
    workspace.publication_push_status = push_status.map(str::to_string);
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();
}

async fn create_run_with_status(
    state: &AppState,
    conversation: &ChatConversation,
    status: AgentRunStatus,
) {
    let mut run = AgentRun::new(conversation.id);
    run.status = status;
    state.agent_run_repo.create(run).await.unwrap();
}

async fn set_workspace_supervision(
    state: &AppState,
    conversation: &ChatConversation,
    status: &str,
    auto_merge_current: Option<bool>,
) {
    let mut workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation.id)
        .await
        .unwrap()
        .unwrap();
    workspace.pr_supervision_status = Some(status.to_string());
    workspace.pr_auto_merge_current = auto_merge_current;
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();
}

#[test]
fn attention_state_fingerprint_changes_for_every_attention_component() {
    let now = Utc::now();
    let baseline = attention_state_fingerprint(
        false,
        SidebarPublicationState::Active,
        Some("run-a"),
        Some(AgentRunStatus::Completed),
        Some("blocked"),
        Some(now),
        None,
    );
    assert_eq!(
        baseline,
        attention_state_fingerprint(
            false,
            SidebarPublicationState::Active,
            Some("run-a"),
            Some(AgentRunStatus::Completed),
            Some("blocked"),
            Some(now),
            None,
        )
    );
    assert_ne!(
        baseline,
        attention_state_fingerprint(
            true,
            SidebarPublicationState::Active,
            Some("run-a"),
            Some(AgentRunStatus::Completed),
            Some("blocked"),
            Some(now),
            None,
        )
    );
    assert_ne!(
        baseline,
        attention_state_fingerprint(
            false,
            SidebarPublicationState::Draft,
            Some("run-a"),
            Some(AgentRunStatus::Completed),
            Some("blocked"),
            Some(now),
            None,
        )
    );
    assert_ne!(
        baseline,
        attention_state_fingerprint(
            false,
            SidebarPublicationState::Active,
            Some("run-b"),
            Some(AgentRunStatus::Completed),
            Some("blocked"),
            Some(now),
            None,
        )
    );
    assert_ne!(
        baseline,
        attention_state_fingerprint(
            false,
            SidebarPublicationState::Active,
            Some("run-a"),
            Some(AgentRunStatus::Running),
            Some("blocked"),
            Some(now),
            None,
        )
    );
    assert_ne!(
        baseline,
        attention_state_fingerprint(
            false,
            SidebarPublicationState::Active,
            Some("run-a"),
            Some(AgentRunStatus::Completed),
            Some("fixing"),
            Some(now),
            None,
        )
    );
    assert_ne!(
        baseline,
        attention_state_fingerprint(
            false,
            SidebarPublicationState::Active,
            Some("run-a"),
            Some(AgentRunStatus::Completed),
            Some("blocked"),
            Some(now + chrono::Duration::seconds(1)),
            None,
        )
    );
    assert_ne!(
        baseline,
        attention_state_fingerprint(
            false,
            SidebarPublicationState::Active,
            Some("run-a"),
            Some(AgentRunStatus::Completed),
            Some("blocked"),
            Some(now),
            Some("members=[worker:Working]"),
        )
    );
}

#[test]
fn team_activity_marks_an_otherwise_idle_row_as_working() {
    let team_activity = ManagedTeamActivity {
        is_working: true,
        fingerprint: "members=[worker:Working]".to_string(),
    };

    assert_eq!(
        attention_lane_for_row(
            false,
            SidebarPublicationState::Active,
            None,
            None,
            false,
            false,
            Utc::now(),
            Some(&team_activity),
        ),
        SidebarAttentionLane::Working,
    );
    assert_eq!(
        attention_lane_for_row(
            false,
            SidebarPublicationState::Active,
            None,
            None,
            false,
            false,
            Utc::now(),
            None,
        ),
        SidebarAttentionLane::Needs,
    );
    let idle_team = ManagedTeamActivity {
        is_working: false,
        fingerprint: "members=[worker:Idle]".to_string(),
    };
    assert_eq!(
        attention_lane_for_row(
            false,
            SidebarPublicationState::Active,
            None,
            None,
            false,
            false,
            Utc::now(),
            Some(&idle_team),
        ),
        SidebarAttentionLane::Needs,
    );
}

#[tokio::test]
async fn muted_needs_row_moves_to_stale_without_affecting_other_rows() {
    let state = AppState::new_test();
    let project = create_project(&state, "muted-needs").await;
    let muted = create_conversation(&state, &project.id, "Muted", Utc::now()).await;
    let other = create_conversation(&state, &project.id, "Other", Utc::now()).await;
    let fingerprint = attention_state_fingerprint(
        false,
        SidebarPublicationState::Active,
        None,
        None,
        None,
        None,
        None,
    );
    state
        .agent_conversation_mute_repo
        .set_muted(AgentConversationMute {
            conversation_id: muted.id,
            muted_at: Utc::now(),
            state_fingerprint: fingerprint,
        })
        .await
        .unwrap();

    let mut input = sidebar_input(&project.id);
    input.group_by = Some("inbox".to_string());
    let response = list_agent_sidebar_conversations_for_app_state(input, &state)
        .await
        .unwrap();
    let needs = response
        .groups
        .iter()
        .find(|group| group.key == "needs")
        .unwrap();
    let stale = response
        .groups
        .iter()
        .find(|group| group.key == "stale")
        .unwrap();
    assert_eq!(needs.total, 1);
    assert_eq!(needs.rows[0].conversation.id, other.id.as_str());
    assert!(!needs.rows[0].is_muted);
    assert_eq!(stale.total, 1);
    assert_eq!(stale.rows[0].conversation.id, muted.id.as_str());
    assert!(stale.rows[0].is_muted);
}

async fn inbox_row_for(
    state: &AppState,
    project_id: &ProjectId,
    conversation_id: &ChatConversationId,
) -> (String, bool) {
    let mut input = sidebar_input(project_id);
    input.group_by = Some("inbox".to_string());
    let response = list_agent_sidebar_conversations_for_app_state(input, state)
        .await
        .unwrap();
    response
        .groups
        .iter()
        .flat_map(|group| {
            group
                .rows
                .iter()
                .map(move |row| (group.key.clone(), row.conversation.id.clone(), row.is_muted))
        })
        .find(|(_, id, _)| *id == conversation_id.as_str())
        .map(|(lane, _, is_muted)| (lane, is_muted))
        .expect("conversation should appear in some lane")
}

async fn mute_via_command(state: &AppState, conversation_id: &ChatConversationId) {
    crate::commands::agent_conversation_mute_commands::set_agent_conversation_muted_for_app_state(
        crate::commands::agent_conversation_mute_commands::SetAgentConversationMutedInput {
            conversation_id: conversation_id.as_str().to_string(),
            muted: true,
        },
        state,
    )
    .await
    .expect("mute should persist");
}

/// The write path and the read path must fingerprint identically. If they
/// ever diverge, a freshly muted row still reads as unmuted and the whole
/// feature silently does nothing.
#[tokio::test]
async fn muting_through_the_command_is_visible_to_the_sidebar_immediately() {
    let state = AppState::new_test();
    let project = create_project(&state, "mute-roundtrip").await;
    let conversation = create_conversation(&state, &project.id, "Needs", Utc::now()).await;
    create_workspace(
        &state,
        &conversation,
        &project.id,
        Some(7),
        Some("open"),
        None,
    )
    .await;
    create_run_with_status(&state, &conversation, AgentRunStatus::Completed).await;

    assert_eq!(
        inbox_row_for(&state, &project.id, &conversation.id).await,
        ("needs".to_string(), false)
    );

    mute_via_command(&state, &conversation.id).await;

    assert_eq!(
        inbox_row_for(&state, &project.id, &conversation.id).await,
        ("stale".to_string(), true)
    );
}

#[tokio::test]
async fn a_new_run_ends_the_mute_and_returns_the_row_to_needs() {
    let state = AppState::new_test();
    let project = create_project(&state, "mute-new-run").await;
    let conversation = create_conversation(&state, &project.id, "Needs", Utc::now()).await;
    create_workspace(&state, &conversation, &project.id, None, Some("open"), None).await;
    create_run_with_status(&state, &conversation, AgentRunStatus::Completed).await;
    mute_via_command(&state, &conversation.id).await;
    assert_eq!(
        inbox_row_for(&state, &project.id, &conversation.id).await,
        ("stale".to_string(), true)
    );

    // Same terminal status, brand-new run: the run id alone must end the
    // mute, otherwise a rerun of the same shape stays silenced forever.
    create_run_with_status(&state, &conversation, AgentRunStatus::Completed).await;

    assert_eq!(
        inbox_row_for(&state, &project.id, &conversation.id).await,
        ("needs".to_string(), false)
    );
}

#[tokio::test]
async fn a_publication_change_ends_the_mute() {
    let state = AppState::new_test();
    let project = create_project(&state, "mute-publication").await;
    let conversation = create_conversation(&state, &project.id, "Needs", Utc::now()).await;
    create_workspace(
        &state,
        &conversation,
        &project.id,
        Some(3),
        Some("open"),
        None,
    )
    .await;
    mute_via_command(&state, &conversation.id).await;
    assert_eq!(
        inbox_row_for(&state, &project.id, &conversation.id).await,
        ("stale".to_string(), true)
    );

    create_workspace(
        &state,
        &conversation,
        &project.id,
        Some(3),
        Some("open"),
        Some("pending"),
    )
    .await;

    assert_eq!(
        inbox_row_for(&state, &project.id, &conversation.id).await,
        ("needs".to_string(), false)
    );
}

#[tokio::test]
async fn a_newer_message_ends_the_mute() {
    let state = AppState::new_test();
    let project = create_project(&state, "mute-message").await;
    let conversation = create_conversation(&state, &project.id, "Needs", Utc::now()).await;
    create_workspace(&state, &conversation, &project.id, None, Some("open"), None).await;
    mute_via_command(&state, &conversation.id).await;
    assert_eq!(
        inbox_row_for(&state, &project.id, &conversation.id).await,
        ("stale".to_string(), true)
    );

    state
        .chat_conversation_repo
        .update_message_stats(&conversation.id, 1, Utc::now())
        .await
        .unwrap();

    assert_eq!(
        inbox_row_for(&state, &project.id, &conversation.id).await,
        ("needs".to_string(), false)
    );
}

#[tokio::test]
async fn muting_never_moves_a_working_or_done_row_out_of_its_lane() {
    let state = AppState::new_test();
    let project = create_project(&state, "mute-other-lanes").await;

    let working = create_conversation(&state, &project.id, "Working", Utc::now()).await;
    create_workspace(&state, &working, &project.id, None, Some("open"), None).await;
    create_run_with_status(&state, &working, AgentRunStatus::Running).await;

    let done = create_conversation(&state, &project.id, "Done", Utc::now()).await;
    create_workspace(&state, &done, &project.id, Some(9), Some("merged"), None).await;

    mute_via_command(&state, &working.id).await;
    mute_via_command(&state, &done.id).await;

    assert_eq!(
        inbox_row_for(&state, &project.id, &working.id).await,
        ("working".to_string(), true)
    );
    assert_eq!(
        inbox_row_for(&state, &project.id, &done.id).await,
        ("done".to_string(), true)
    );
}

#[test]
fn sidebar_group_by_parse_accepts_known_modes_and_rejects_unknown_modes() {
    assert_eq!(
        SidebarGroupBy::parse(Some("automation")).unwrap(),
        SidebarGroupBy::Automation
    );
    assert_eq!(
        SidebarGroupBy::parse(Some("inbox")).unwrap(),
        SidebarGroupBy::Inbox
    );
    assert_eq!(
        SidebarGroupBy::parse(Some("definitely-not-valid")).unwrap_err(),
        "invalid sidebar group_by: definitely-not-valid"
    );
}

#[tokio::test]
async fn publication_grouping_returns_enriched_filtered_rows() {
    let state = AppState::new_test();
    let project = create_project(&state, "alpha").await;
    let now = Utc::now();
    let merged = create_conversation(&state, &project.id, "Merged work", now).await;
    create_workspace(
        &state,
        &merged,
        &project.id,
        Some(123),
        Some("merged"),
        Some("published"),
    )
    .await;
    let unpushed = create_conversation(
        &state,
        &project.id,
        "Needs push",
        now - chrono::Duration::minutes(1),
    )
    .await;
    create_workspace(
        &state,
        &unpushed,
        &project.id,
        None,
        Some("open"),
        Some("pending"),
    )
    .await;
    let active = create_conversation(
        &state,
        &project.id,
        "Active work",
        now - chrono::Duration::minutes(2),
    )
    .await;
    create_workspace(
        &state,
        &active,
        &project.id,
        None,
        Some("open"),
        Some("published"),
    )
    .await;

    let mut input = sidebar_input(&project.id);
    input.publication_states = Some(vec!["merged".to_string(), "unpushed".to_string()]);

    let response = list_agent_sidebar_conversations_for_app_state(input, &state)
        .await
        .unwrap();

    assert_eq!(response.groups.len(), 2);
    assert_eq!(response.groups[0].key, "merged");
    assert_eq!(response.groups[0].total, 1);
    assert_eq!(
        response.groups[0].rows[0].conversation.id,
        merged.id.as_str()
    );
    assert_eq!(response.groups[0].rows[0].ref_kind, "pull_request");
    assert_eq!(response.groups[0].rows[0].ref_label, "PR #123");
    assert_eq!(
        response.groups[0].rows[0].publication_label.as_deref(),
        Some("merged")
    );
    assert_eq!(response.groups[1].key, "unpushed");
    assert_eq!(response.groups[1].total, 1);
    assert_eq!(
        response.groups[1].rows[0].conversation.id,
        unpushed.id.as_str()
    );
    assert_eq!(response.groups[1].rows[0].publication_state, "unpushed");
}

#[tokio::test]
async fn sidebar_excludes_parent_owned_child_conversations() {
    let state = AppState::new_test();
    let project = create_project(&state, "alpha").await;
    let parent = create_conversation(&state, &project.id, "Parent work", Utc::now()).await;
    create_workspace(
        &state,
        &parent,
        &project.id,
        None,
        Some("open"),
        Some("published"),
    )
    .await;

    let mut child = ChatConversation::new_project(project.id.clone());
    child.title = Some("Review workspace changes".to_string());
    child.parent_conversation_id = Some(parent.id.as_str().to_string());
    state
        .chat_conversation_repo
        .create(child)
        .await
        .expect("child conversation should be created");

    let response =
        list_agent_sidebar_conversations_for_app_state(sidebar_input(&project.id), &state)
            .await
            .expect("sidebar conversations should load");

    let rows = &response.groups[0].rows;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].conversation.id, parent.id.as_str());
    assert_eq!(rows[0].conversation.title.as_deref(), Some("Parent work"));
}

#[tokio::test]
async fn sidebar_includes_child_conversations_with_owned_workspaces() {
    let state = AppState::new_test();
    let project = create_project(&state, "alpha").await;
    let now = Utc::now();
    let parent = create_conversation(&state, &project.id, "Parent work", now).await;
    create_workspace(
        &state,
        &parent,
        &project.id,
        Some(525),
        Some("merged"),
        Some("published"),
    )
    .await;

    let mut child = ChatConversation::new_project(project.id.clone());
    child.title = Some("Investigate follow-up".to_string());
    child.parent_conversation_id = Some(parent.id.as_str().to_string());
    child.created_at = now + chrono::Duration::minutes(1);
    child.updated_at = child.created_at;
    let child = state
        .chat_conversation_repo
        .create(child)
        .await
        .expect("child conversation should be created");
    create_workspace(&state, &child, &project.id, None, None, None).await;

    let response =
        list_agent_sidebar_conversations_for_app_state(sidebar_input(&project.id), &state)
            .await
            .expect("sidebar conversations should load");

    let conversation_ids = response
        .groups
        .iter()
        .flat_map(|group| group.rows.iter())
        .map(|row| row.conversation.id.clone())
        .collect::<Vec<_>>();
    assert!(
        conversation_ids.contains(&child.id.as_str()),
        "child conversations with their own workspace should be listed"
    );
}

#[tokio::test]
async fn sidebar_shows_automation_setup_and_hides_run_conversations() {
    let state = AppState::new_test();
    let project = create_project(&state, "alpha").await;
    let automation_id = AutomationId::from_string("automation-1");

    let mut setup = ChatConversation::new_project(project.id.clone());
    setup.title = Some("Automation setup".to_string());
    setup.automation_id = Some(automation_id.clone());
    let setup = state
        .chat_conversation_repo
        .create(setup)
        .await
        .expect("setup conversation should be created");

    let mut run = ChatConversation::new_project(project.id.clone());
    run.title = Some("Automation run 1".to_string());
    run.automation_id = Some(automation_id);
    run.automation_run_id = Some(AutomationRunId::from_string("run-1"));
    let run = state
        .chat_conversation_repo
        .create(run)
        .await
        .expect("run conversation should be created");

    let response =
        list_agent_sidebar_conversations_for_app_state(sidebar_input(&project.id), &state)
            .await
            .expect("sidebar conversations should load");

    let conversation_ids = response
        .groups
        .iter()
        .flat_map(|group| group.rows.iter())
        .map(|row| row.conversation.id.clone())
        .collect::<Vec<_>>();
    assert!(conversation_ids.contains(&setup.id.as_str().to_string()));
    assert!(!conversation_ids.contains(&run.id.as_str().to_string()));
}

#[tokio::test]
async fn automation_grouping_returns_named_and_standalone_groups_without_run_conversations() {
    let state = AppState::new_test();
    let project = create_project(&state, "alpha").await;
    let now = Utc::now();
    let automation = create_automation(
        &state,
        &project.id,
        "automation-setup-owner",
        "Release Train",
    )
    .await;

    let setup = create_automation_conversation(
        &state,
        &project.id,
        "Automation setup",
        now,
        automation.id.clone(),
        None,
    )
    .await;

    let standalone = create_conversation(
        &state,
        &project.id,
        "Standalone task",
        now - chrono::Duration::minutes(5),
    )
    .await;

    let run = create_automation_conversation(
        &state,
        &project.id,
        "Automation run",
        now + chrono::Duration::minutes(5),
        automation.id.clone(),
        Some(AutomationRunId::from_string("run-1")),
    )
    .await;

    let mut input = sidebar_input(&project.id);
    input.group_by = Some("automation".to_string());

    let response = list_agent_sidebar_conversations_for_app_state(input, &state)
        .await
        .expect("automation grouping should load");

    assert_eq!(response.groups.len(), 2);
    assert_eq!(response.groups[0].key, automation.id.as_str());
    assert_eq!(response.groups[0].label, "Release Train");
    assert_eq!(response.groups[0].total, 1);
    assert_eq!(
        response.groups[0].rows[0].conversation.id,
        setup.id.as_str()
    );
    assert_eq!(response.groups[1].key, "__standalone__");
    assert_eq!(response.groups[1].label, "Standalone");
    assert_eq!(response.groups[1].total, 1);
    assert_eq!(
        response.groups[1].rows[0].conversation.id,
        standalone.id.as_str()
    );
    let visible_ids = response
        .groups
        .iter()
        .flat_map(|group| group.rows.iter())
        .map(|row| row.conversation.id.clone())
        .collect::<Vec<_>>();
    assert!(!visible_ids.contains(&run.id.as_str().to_string()));
}

#[tokio::test]
async fn automation_grouping_sorts_by_fallback_label_and_paginates_visible_rows() {
    let state = AppState::new_test();
    let project = create_project(&state, "alpha").await;
    let now = Utc::now();
    let fallback = create_automation(&state, &project.id, "automation-fallback-id", "   ").await;
    create_automation(&state, &project.id, "automation-zed", "Zed Automation").await;

    let _alpha = create_automation_conversation(
        &state,
        &project.id,
        "Alpha visible",
        now - chrono::Duration::minutes(2),
        fallback.id.clone(),
        None,
    )
    .await;

    let beta = create_automation_conversation(
        &state,
        &project.id,
        "Beta visible",
        now - chrono::Duration::minutes(1),
        fallback.id.clone(),
        None,
    )
    .await;

    let merged = create_automation_conversation(
        &state,
        &project.id,
        "Merged hidden",
        now,
        fallback.id.clone(),
        None,
    )
    .await;
    create_workspace(
        &state,
        &merged,
        &project.id,
        Some(55),
        Some("merged"),
        Some("published"),
    )
    .await;

    let zed = create_automation_conversation(
        &state,
        &project.id,
        "Zed visible",
        now,
        AutomationId::from_string("automation-zed"),
        None,
    )
    .await;

    let mut input = sidebar_input(&project.id);
    input.group_by = Some("automation".to_string());
    input.sort = Some("az".to_string());
    input.publication_states = Some(vec!["active".to_string()]);
    input.limit_per_group = Some(1);
    input.offsets = Some(HashMap::from([("automation-fallback-id".to_string(), 1)]));

    let response = list_agent_sidebar_conversations_for_app_state(input, &state)
        .await
        .expect("automation grouping should load");

    assert_eq!(response.groups.len(), 2);
    assert_eq!(response.groups[0].key, "automation-fallback-id");
    assert_eq!(
        response.groups[0].label,
        "Automation automation-fallback-id"
    );
    assert_eq!(response.groups[0].total, 2);
    assert_eq!(response.groups[0].offset, 1);
    assert!(!response.groups[0].has_more);
    assert_eq!(response.groups[0].rows[0].conversation.id, beta.id.as_str());
    assert_eq!(response.groups[1].key, "automation-zed");
    assert_eq!(response.groups[1].label, "Zed Automation");
    assert_eq!(response.groups[1].total, 1);
    assert_eq!(response.groups[1].rows[0].conversation.id, zed.id.as_str());
}

#[tokio::test]
async fn publication_grouping_keeps_failed_unpublished_workspace_active() {
    let state = AppState::new_test();
    let project = create_project(&state, "alpha").await;
    let now = Utc::now();
    let stopped = create_conversation(&state, &project.id, "Stopped work", now).await;
    create_workspace(&state, &stopped, &project.id, None, None, None).await;
    create_run_with_status(&state, &stopped, AgentRunStatus::Failed).await;

    let mut input = sidebar_input(&project.id);
    input.publication_states = Some(vec!["active".to_string(), "closed".to_string()]);

    let response = list_agent_sidebar_conversations_for_app_state(input, &state)
        .await
        .unwrap();

    assert_eq!(response.groups.len(), 2);
    assert_eq!(response.groups[0].key, "active");
    assert_eq!(response.groups[0].total, 1);
    assert_eq!(
        response.groups[0].rows[0].conversation.id,
        stopped.id.as_str()
    );
    assert_eq!(response.groups[0].rows[0].publication_state, "active");
    assert!(response.groups[0].rows[0].publication_label.is_none());
    assert_eq!(response.groups[1].key, "closed");
    assert_eq!(response.groups[1].total, 0);
}

#[tokio::test]
async fn bulk_publication_states_keep_cancelled_unpublished_workspace_active() {
    let state = AppState::new_test();
    let project = create_project(&state, "alpha").await;
    let conversation = create_conversation(&state, &project.id, "Cancelled work", Utc::now()).await;
    create_workspace(&state, &conversation, &project.id, None, None, None).await;
    create_run_with_status(&state, &conversation, AgentRunStatus::Cancelled).await;

    let response = get_bulk_workspace_publication_states_inner(
        &[conversation.id.as_str().to_string()],
        &state,
    )
    .await
    .unwrap();
    let conversation_id = conversation.id.as_str();

    assert_eq!(
        response
            .get(&conversation_id)
            .map(|row| row.publication_state.as_str()),
        Some("active")
    );
    assert_eq!(
        response
            .get(&conversation_id)
            .and_then(|row| row.publication_label.as_deref()),
        None
    );
}

#[tokio::test]
async fn publication_grouping_surfaces_pr_supervision_attention_labels() {
    let state = AppState::new_test();
    let project = create_project(&state, "alpha").await;
    let now = Utc::now();
    let fixing = create_conversation(&state, &project.id, "Fixing PR", now).await;
    create_workspace(
        &state,
        &fixing,
        &project.id,
        Some(77),
        Some("open"),
        Some("needs_agent"),
    )
    .await;
    set_workspace_supervision(&state, &fixing, "fixing", Some(false)).await;

    let monitored = create_conversation(
        &state,
        &project.id,
        "Auto merge ready",
        now - chrono::Duration::minutes(1),
    )
    .await;
    create_workspace(
        &state,
        &monitored,
        &project.id,
        Some(78),
        Some("open"),
        Some("pushed"),
    )
    .await;
    set_workspace_supervision(&state, &monitored, "monitoring", Some(true)).await;

    let held = create_conversation(
        &state,
        &project.id,
        "Held PR repair",
        now - chrono::Duration::minutes(2),
    )
    .await;
    create_workspace(
        &state,
        &held,
        &project.id,
        Some(79),
        Some("open"),
        Some("pushed"),
    )
    .await;
    set_workspace_supervision(&state, &held, "held", Some(false)).await;

    let mut input = sidebar_input(&project.id);
    input.group_by = Some("project".to_string());
    input.publication_states = Some(vec!["active".to_string(), "uncommitted".to_string()]);

    let response = list_agent_sidebar_conversations_for_app_state(input, &state)
        .await
        .unwrap();

    let rows = &response.groups[0].rows;
    let fixing_row = rows
        .iter()
        .find(|row| row.conversation.id == fixing.id.as_str())
        .unwrap();
    assert_eq!(fixing_row.publication_state, "uncommitted");
    assert_eq!(fixing_row.publication_label.as_deref(), Some("fixing"));

    let monitored_row = rows
        .iter()
        .find(|row| row.conversation.id == monitored.id.as_str())
        .unwrap();
    assert_eq!(monitored_row.publication_state, "active");
    assert_eq!(
        monitored_row.publication_label.as_deref(),
        Some("auto-merge")
    );
    let held_row = rows
        .iter()
        .find(|row| row.conversation.id == held.id.as_str())
        .unwrap();
    assert_eq!(held_row.publication_state, "active");
    assert_eq!(held_row.publication_label.as_deref(), Some("paused"));
}

#[tokio::test]
async fn publication_grouping_paginates_each_group_independently() {
    let state = AppState::new_test();
    let project = create_project(&state, "alpha").await;
    let now = Utc::now();
    let newest = create_conversation(&state, &project.id, "Newest merged", now).await;
    create_workspace(&state, &newest, &project.id, Some(11), Some("merged"), None).await;
    let older = create_conversation(
        &state,
        &project.id,
        "Older merged",
        now - chrono::Duration::minutes(1),
    )
    .await;
    create_workspace(&state, &older, &project.id, Some(10), Some("merged"), None).await;

    let mut input = sidebar_input(&project.id);
    input.publication_states = Some(vec!["merged".to_string()]);
    input.limit_per_group = Some(1);
    input.offsets = Some(HashMap::from([("merged".to_string(), 1)]));

    let response = list_agent_sidebar_conversations_for_app_state(input, &state)
        .await
        .unwrap();

    assert_eq!(response.groups.len(), 1);
    assert_eq!(response.groups[0].total, 2);
    assert_eq!(response.groups[0].offset, 1);
    assert!(!response.groups[0].has_more);
    assert_eq!(response.groups[0].rows.len(), 1);
    assert_eq!(
        response.groups[0].rows[0].conversation.id,
        older.id.as_str()
    );
}

#[tokio::test]
async fn inbox_grouping_emits_all_lanes_in_fixed_order_including_empty_ones() {
    let state = AppState::new_test();
    let project = create_project(&state, "alpha").await;

    let mut input = sidebar_input(&project.id);
    input.group_by = Some("inbox".to_string());

    let response = list_agent_sidebar_conversations_for_app_state(input, &state)
        .await
        .unwrap();

    assert_eq!(response.groups.len(), 4);
    assert_eq!(
        response
            .groups
            .iter()
            .map(|group| (group.key.as_str(), group.label.as_str(), group.total))
            .collect::<Vec<_>>(),
        vec![
            ("needs", "Needs you", 0),
            ("working", "Working", 0),
            ("stale", "Stale", 0),
            ("done", "Done", 0),
        ]
    );
}

#[tokio::test]
async fn inbox_grouping_derives_attention_lanes_and_action_verbs() {
    let state = AppState::new_test();
    let project = create_project(&state, "alpha").await;
    let now = Utc::now();

    let merged = create_conversation(&state, &project.id, "Merged", now).await;
    create_workspace(&state, &merged, &project.id, Some(1), Some("merged"), None).await;

    let running = create_conversation(&state, &project.id, "Running", now).await;
    create_workspace(&state, &running, &project.id, None, Some("open"), None).await;
    create_run_with_status(&state, &running, AgentRunStatus::Running).await;

    let fixing = create_conversation(&state, &project.id, "Fixing", now).await;
    create_workspace(&state, &fixing, &project.id, Some(2), Some("open"), None).await;
    set_workspace_supervision(&state, &fixing, "fixing", Some(false)).await;

    let blocked = create_conversation(&state, &project.id, "Blocked", now).await;
    create_workspace(&state, &blocked, &project.id, Some(3), Some("open"), None).await;
    set_workspace_supervision(&state, &blocked, "blocked", Some(false)).await;

    let stale = create_conversation(
        &state,
        &project.id,
        "Stale",
        now - chrono::Duration::days(STALE_AFTER_DAYS + 1),
    )
    .await;
    create_workspace(&state, &stale, &project.id, None, Some("open"), None).await;

    let fresh = create_conversation(&state, &project.id, "Fresh", now).await;
    create_workspace(&state, &fresh, &project.id, None, Some("open"), None).await;

    let mut input = sidebar_input(&project.id);
    input.group_by = Some("inbox".to_string());

    let response = list_agent_sidebar_conversations_for_app_state(input, &state)
        .await
        .unwrap();
    let rows = response
        .groups
        .iter()
        .flat_map(|group| group.rows.iter())
        .map(|row| (row.conversation.id.clone(), row))
        .collect::<HashMap<_, _>>();

    assert_eq!(rows[&merged.id.as_str()].attention_lane, "done");
    assert_eq!(rows[&merged.id.as_str()].action_verb, "Merged");
    assert_eq!(rows[&running.id.as_str()].attention_lane, "working");
    assert_eq!(rows[&running.id.as_str()].action_verb, "Running");
    assert_eq!(rows[&fixing.id.as_str()].attention_lane, "working");
    assert_eq!(rows[&fixing.id.as_str()].action_verb, "Fixing");
    assert_ne!(rows[&blocked.id.as_str()].attention_lane, "working");
    assert_eq!(rows[&blocked.id.as_str()].action_verb, "Unblock");
    assert_eq!(rows[&stale.id.as_str()].attention_lane, "stale");
    assert_eq!(rows[&fresh.id.as_str()].attention_lane, "needs");
    assert_eq!(rows[&fresh.id.as_str()].action_verb, "Continue");
}

#[tokio::test]
async fn inbox_grouping_paginates_each_lane_independently_and_pins_first() {
    let state = AppState::new_test();
    let project = create_project(&state, "alpha").await;
    let now = Utc::now();

    let newest_needs = create_conversation(&state, &project.id, "Newest needs", now).await;
    create_workspace(&state, &newest_needs, &project.id, None, Some("open"), None).await;
    let pinned_needs = create_conversation(
        &state,
        &project.id,
        "Pinned needs",
        now - chrono::Duration::minutes(1),
    )
    .await;
    create_workspace(&state, &pinned_needs, &project.id, None, Some("open"), None).await;
    let newest_done = create_conversation(&state, &project.id, "Newest done", now).await;
    create_workspace(
        &state,
        &newest_done,
        &project.id,
        Some(1),
        Some("merged"),
        None,
    )
    .await;
    let older_done = create_conversation(
        &state,
        &project.id,
        "Older done",
        now - chrono::Duration::minutes(1),
    )
    .await;
    create_workspace(
        &state,
        &older_done,
        &project.id,
        Some(2),
        Some("merged"),
        None,
    )
    .await;

    let mut input = sidebar_input(&project.id);
    input.group_by = Some("inbox".to_string());
    input.limit_per_group = Some(1);
    input.pinned_conversation_ids = Some(vec![pinned_needs.id.as_str().to_string()]);
    input.offsets = Some(HashMap::from([
        ("needs".to_string(), 0),
        ("done".to_string(), 1),
    ]));

    let response = list_agent_sidebar_conversations_for_app_state(input, &state)
        .await
        .unwrap();
    let needs = response
        .groups
        .iter()
        .find(|group| group.key == "needs")
        .unwrap();
    let done = response
        .groups
        .iter()
        .find(|group| group.key == "done")
        .unwrap();

    assert_eq!(needs.total, 2);
    assert_eq!(needs.rows[0].conversation.id, pinned_needs.id.as_str());
    assert_eq!(done.total, 2);
    assert_eq!(done.offset, 1);
    assert_eq!(done.rows[0].conversation.id, older_done.id.as_str());
}

#[tokio::test]
async fn publication_grouping_sorts_rows_by_requested_title_order() {
    let state = AppState::new_test();
    let project = create_project(&state, "alpha").await;
    let now = Utc::now();
    let zulu = create_conversation(&state, &project.id, "Zulu merged", now).await;
    create_workspace(&state, &zulu, &project.id, Some(12), Some("merged"), None).await;
    let alpha = create_conversation(
        &state,
        &project.id,
        "Alpha merged",
        now - chrono::Duration::minutes(5),
    )
    .await;
    create_workspace(&state, &alpha, &project.id, Some(11), Some("merged"), None).await;

    let mut input = sidebar_input(&project.id);
    input.publication_states = Some(vec!["merged".to_string()]);
    input.sort = Some("az".to_string());

    let response = list_agent_sidebar_conversations_for_app_state(input, &state)
        .await
        .unwrap();

    assert_eq!(response.groups.len(), 1);
    assert_eq!(response.groups[0].rows.len(), 2);
    assert_eq!(
        response.groups[0].rows[0].conversation.id,
        alpha.id.as_str()
    );
    assert_eq!(response.groups[0].rows[1].conversation.id, zulu.id.as_str());
}

#[tokio::test]
async fn bulk_publication_states_returns_active_for_no_workspace() {
    let state = AppState::new_test();
    let project = create_project(&state, "alpha").await;
    let now = Utc::now();
    let conv = create_conversation(&state, &project.id, "No workspace", now).await;
    let conv_id = conv.id.as_str();

    let result =
        get_bulk_workspace_publication_states_inner(std::slice::from_ref(&conv_id), &state)
            .await
            .unwrap();

    assert_eq!(result.len(), 1);
    let entry = result.get(&conv_id).unwrap();
    assert_eq!(entry.publication_state, "active");
    assert!(entry.publication_label.is_none());
}

#[tokio::test]
async fn bulk_publication_states_returns_correct_states_for_various_workspaces() {
    let state = AppState::new_test();
    let project = create_project(&state, "alpha").await;
    let now = Utc::now();

    let merged_conv = create_conversation(&state, &project.id, "Merged", now).await;
    create_workspace(
        &state,
        &merged_conv,
        &project.id,
        Some(10),
        Some("merged"),
        None,
    )
    .await;
    let merged_id = merged_conv.id.as_str();

    let draft_conv = create_conversation(
        &state,
        &project.id,
        "Draft",
        now - chrono::Duration::minutes(1),
    )
    .await;
    create_workspace(
        &state,
        &draft_conv,
        &project.id,
        Some(11),
        Some("draft"),
        None,
    )
    .await;
    let draft_id = draft_conv.id.as_str();

    let uncommitted_conv = create_conversation(
        &state,
        &project.id,
        "Uncommitted",
        now - chrono::Duration::minutes(2),
    )
    .await;
    create_workspace(
        &state,
        &uncommitted_conv,
        &project.id,
        None,
        None,
        Some("needs_agent"),
    )
    .await;
    let uncommitted_id = uncommitted_conv.id.as_str();

    let unpushed_conv = create_conversation(
        &state,
        &project.id,
        "Unpushed",
        now - chrono::Duration::minutes(3),
    )
    .await;
    create_workspace(
        &state,
        &unpushed_conv,
        &project.id,
        None,
        None,
        Some("pending"),
    )
    .await;
    let unpushed_id = unpushed_conv.id.as_str();

    let closed_conv = create_conversation(
        &state,
        &project.id,
        "Closed",
        now - chrono::Duration::minutes(4),
    )
    .await;
    create_workspace(
        &state,
        &closed_conv,
        &project.id,
        Some(12),
        Some("closed"),
        None,
    )
    .await;
    let closed_id = closed_conv.id.as_str();

    let ids: Vec<String> = vec![
        merged_id.clone(),
        draft_id.clone(),
        uncommitted_id.clone(),
        unpushed_id.clone(),
        closed_id.clone(),
    ];

    let result = get_bulk_workspace_publication_states_inner(&ids, &state)
        .await
        .unwrap();

    assert_eq!(result.len(), 5);
    assert_eq!(result.get(&merged_id).unwrap().publication_state, "merged");
    assert_eq!(
        result.get(&merged_id).unwrap().publication_label.as_deref(),
        Some("merged")
    );
    assert_eq!(result.get(&draft_id).unwrap().publication_state, "draft");
    assert_eq!(
        result.get(&uncommitted_id).unwrap().publication_state,
        "uncommitted"
    );
    assert_eq!(
        result.get(&unpushed_id).unwrap().publication_state,
        "unpushed"
    );
    assert_eq!(result.get(&closed_id).unwrap().publication_state, "closed");
}

#[tokio::test]
async fn bulk_publication_states_returns_empty_for_empty_input() {
    let state = AppState::new_test();

    let result = get_bulk_workspace_publication_states_inner(&[], &state)
        .await
        .unwrap();

    assert!(result.is_empty());
}

#[tokio::test]
async fn project_grouping_returns_project_groups_with_pinned_rows_first() {
    let state = AppState::new_test();
    let alpha = create_project(&state, "alpha").await;
    let beta = create_project(&state, "beta").await;
    let now = Utc::now();

    let newest = create_conversation(&state, &alpha.id, "Newest alpha", now).await;
    create_workspace(&state, &newest, &alpha.id, None, Some("open"), None).await;
    let pinned = create_conversation(
        &state,
        &alpha.id,
        "Pinned alpha",
        now - chrono::Duration::minutes(5),
    )
    .await;
    create_workspace(&state, &pinned, &alpha.id, Some(42), Some("open"), None).await;
    let beta_conversation = create_conversation(
        &state,
        &beta.id,
        "Beta work",
        now - chrono::Duration::seconds(1),
    )
    .await;
    create_workspace(
        &state,
        &beta_conversation,
        &beta.id,
        None,
        Some("draft"),
        None,
    )
    .await;

    let mut input = sidebar_input(&alpha.id);
    input.project_ids = vec![alpha.id.as_str().to_string(), beta.id.as_str().to_string()];
    input.group_by = Some("project".to_string());
    input.pinned_conversation_ids = Some(vec![pinned.id.as_str().to_string()]);

    let response = list_agent_sidebar_conversations_for_app_state(input, &state)
        .await
        .unwrap();

    assert_eq!(response.groups.len(), 2);
    assert_eq!(response.groups[0].key, alpha.id.as_str());
    assert_eq!(response.groups[0].label, "alpha");
    assert_eq!(response.groups[0].total, 2);
    assert_eq!(
        response.groups[0].rows[0].conversation.id,
        pinned.id.as_str()
    );
    assert_eq!(response.groups[0].rows[0].ref_label, "PR #42");
    assert_eq!(
        response.groups[0].rows[1].conversation.id,
        newest.id.as_str()
    );
    assert_eq!(response.groups[1].key, beta.id.as_str());
    assert_eq!(response.groups[1].label, "beta");
    assert_eq!(response.groups[1].total, 1);
    assert_eq!(
        response.groups[1].rows[0].conversation.id,
        beta_conversation.id.as_str()
    );
}

#[tokio::test]
async fn project_grouping_adds_no_project_group_for_standalone_conversations() {
    let state = AppState::new_test();
    let alpha = create_project(&state, "alpha-standalone").await;
    let now = Utc::now();

    let project_conversation = create_conversation(&state, &alpha.id, "Alpha work", now).await;
    let standalone_conversation = create_standalone_conversation(
        &state,
        "Standalone chat",
        now - chrono::Duration::minutes(1),
    )
    .await;

    let mut input = sidebar_input(&alpha.id);
    input.group_by = Some("project".to_string());

    let response = list_agent_sidebar_conversations_for_app_state(input, &state)
        .await
        .unwrap();

    assert_eq!(
        response.groups.len(),
        2,
        "the requested project group plus a data-driven 'No project' group"
    );
    assert_eq!(response.groups[0].key, alpha.id.as_str());
    assert_eq!(
        response.groups[0].rows[0].conversation.id,
        project_conversation.id.as_str()
    );

    let no_project_group = &response.groups[1];
    assert_eq!(no_project_group.key, "__no_project__");
    assert_eq!(no_project_group.label, "No project");
    assert_eq!(no_project_group.total, 1);
    assert_eq!(
        no_project_group.rows[0].conversation.id,
        standalone_conversation.id.as_str()
    );
    assert_eq!(
        no_project_group.rows[0].conversation.context_type,
        "standalone"
    );
    assert!(no_project_group.rows[0].workspace.is_none());
}

#[tokio::test]
async fn project_grouping_omits_no_project_group_when_no_standalone_conversations_exist() {
    // Regression guard for the OTHER direction: unlike explicitly requested
    // project_ids (which always get a group even when empty), the "No
    // project" group must be entirely absent when there are zero
    // standalone conversations — it is data-driven, not
    // always-present, so callers with no standalone rows don't render an
    // empty phantom group.
    let state = AppState::new_test();
    let alpha = create_project(&state, "alpha-no-standalone").await;
    let now = Utc::now();
    create_conversation(&state, &alpha.id, "Alpha only", now).await;

    let mut input = sidebar_input(&alpha.id);
    input.group_by = Some("project".to_string());

    let response = list_agent_sidebar_conversations_for_app_state(input, &state)
        .await
        .unwrap();

    assert_eq!(response.groups.len(), 1);
    assert_eq!(response.groups[0].key, alpha.id.as_str());
    assert!(!response
        .groups
        .iter()
        .any(|group| group.key == "__no_project__"));
}

#[tokio::test]
async fn project_grouping_sorts_pinned_rows_before_priority_rows() {
    let state = AppState::new_test();
    let project = create_project(&state, "alpha-priority").await;
    let now = Utc::now();

    let unpinned = create_conversation(&state, &project.id, "Unpinned newest", now).await;
    create_workspace(&state, &unpinned, &project.id, None, Some("open"), None).await;
    let priority = create_conversation(
        &state,
        &project.id,
        "Selected priority",
        now - chrono::Duration::minutes(5),
    )
    .await;
    create_workspace(&state, &priority, &project.id, None, Some("open"), None).await;
    let pinned = create_conversation(
        &state,
        &project.id,
        "Pinned oldest",
        now - chrono::Duration::minutes(10),
    )
    .await;
    create_workspace(&state, &pinned, &project.id, None, Some("open"), None).await;

    let mut input = sidebar_input(&project.id);
    input.group_by = Some("project".to_string());
    input.pinned_conversation_ids = Some(vec![pinned.id.as_str().to_string()]);
    input.priority_conversation_ids = Some(vec![priority.id.as_str().to_string()]);

    let response = list_agent_sidebar_conversations_for_app_state(input, &state)
        .await
        .unwrap();

    let rows = &response.groups[0].rows;
    assert_eq!(rows[0].conversation.id, pinned.id.as_str());
    assert_eq!(rows[1].conversation.id, priority.id.as_str());
    assert_eq!(rows[2].conversation.id, unpinned.id.as_str());
}

#[test]
fn publication_state_for_workspace_no_workspace_failed_run_is_closed() {
    assert_eq!(
        publication_state_for_workspace(None, Some(AgentRunStatus::Failed)),
        SidebarPublicationState::Closed
    );
}

#[test]
fn publication_state_for_workspace_no_workspace_cancelled_run_is_closed() {
    assert_eq!(
        publication_state_for_workspace(None, Some(AgentRunStatus::Cancelled)),
        SidebarPublicationState::Closed
    );
}

#[test]
fn publication_state_for_workspace_no_workspace_running_run_is_active() {
    // A non-terminal latest run (or no run) must not flip an unpublished
    // workspace-less conversation to Closed.
    assert_eq!(
        publication_state_for_workspace(None, Some(AgentRunStatus::Running)),
        SidebarPublicationState::Active
    );
    assert_eq!(
        publication_state_for_workspace(None, None),
        SidebarPublicationState::Active
    );
}

#[test]
fn publication_state_from_domain_no_workspace_failed_run_is_closed() {
    assert_eq!(
        publication_state_from_domain(None, Some(AgentRunStatus::Failed)),
        SidebarPublicationState::Closed
    );
}

#[test]
fn publication_state_from_domain_no_workspace_cancelled_run_is_closed() {
    assert_eq!(
        publication_state_from_domain(None, Some(AgentRunStatus::Cancelled)),
        SidebarPublicationState::Closed
    );
}

#[test]
fn publication_state_from_domain_no_workspace_running_run_is_active() {
    assert_eq!(
        publication_state_from_domain(None, Some(AgentRunStatus::Running)),
        SidebarPublicationState::Active
    );
    assert_eq!(
        publication_state_from_domain(None, None),
        SidebarPublicationState::Active
    );
}

#[test]
fn publication_state_from_domain_active_workspace_failed_run_is_active() {
    let conversation_id = ChatConversationId::from_string("conversation-1".to_string());
    let project_id = ProjectId::from_string("project-1".to_string());
    let workspace = AgentConversationWorkspace::new(
        conversation_id,
        project_id,
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        None,
        "ralphx/project/agent-conversation-1".to_string(),
        "/tmp/worktrees/agent-conversation-1".to_string(),
    );

    assert_eq!(
        publication_state_from_domain(Some(&workspace), Some(AgentRunStatus::Failed)),
        SidebarPublicationState::Active
    );
    assert_eq!(
        publication_state_from_domain(Some(&workspace), Some(AgentRunStatus::Cancelled)),
        SidebarPublicationState::Active
    );
}
