use super::plan_edit_handoff::{
    finish_plan_to_edit_handoff_after_commit, stop_plan_to_edit_handoff_before_commit,
};
use super::{
    agent_conversation_response_for_state, agent_conversation_responses_for_state,
    agent_workspace_freshness_cache, agent_workspace_freshness_cache_key,
    agent_workspace_interactive_slot_key, agent_workspace_post_repair_action_from_events,
    agent_workspace_repair_wait_released, agent_workspace_response_for_state,
    apply_base_resolution_to_publish_target, archive_agent_conversation,
    build_agent_workspace_publish_repair_message_for_target,
    build_agent_workspace_repair_message_for_target, cached_agent_workspace_freshness,
    compose_blocked_repair_retry_context, create_agent_conversation,
    emit_agent_conversation_fork_events, ensure_plan_workspace_planning_session_link_for_send,
    existing_pr_retarget_block_reason, filter_agent_list_visible_conversations,
    fork_agent_conversation, fork_agent_conversation_response_for_state,
    fork_terminal_agent_conversation_for_send, get_agent_conversation_runtime_index_for_app_state,
    get_agent_conversation_runtime_statuses_for_app_state,
    get_agent_conversation_summary_for_app_state,
    get_agent_conversation_timeline_page_for_app_state, get_agent_conversation_workspace_freshness,
    get_agent_run_attribution, get_agent_run_attributions,
    get_agent_timeline_item_tool_call_detail_for_app_state, hidden_user_message_metadata,
    invalidate_agent_workspace_freshness_cache, list_agent_conversations_page,
    load_delegated_tool_runtime_snapshot, mark_agent_workspace_failure_with_routing_and_action,
    mark_agent_workspace_publish_failure_with_target, mark_agent_workspace_publish_status,
    mark_agent_workspace_update_failure_with_target, merge_delegated_snapshot_into_result,
    normalize_agent_runtime_selection, normalize_agent_workspace_source_pull_request,
    normalize_explicit_publish_base_selection, normalized_effort_for_supported,
    parse_wrapped_mcp_result_object, persist_workspace_base_resolution_if_retargeted,
    precompute_agent_conversation_workspace_pr_description_for_app_state,
    preview_tool_payloads_for_message, project_plan_branch_publication_into_workspace_response,
    publication_event_status_for_push_status, publication_event_summary_for_push_status,
    publish_agent_conversation_workspace_for_app_state, resolve_agent_workspace_pr_metadata_target,
    restore_agent_conversation, retarget_existing_workspace_pr_base_if_needed,
    retry_blocked_agent_workspace_repair_for_explicit_user_action,
    schedule_external_pr_reconciliation_for_conversation_id,
    schedule_external_pr_reconciliation_for_workspace,
    schedule_pr_supervision_recovery_for_conversation_id,
    send_agent_workspace_publish_repair_message_for_target,
    set_agent_conversation_workspace_auto_publish_for_state,
    set_agent_conversation_workspace_pr_supervision_for_state,
    settle_agent_workspace_publish_lease_status,
    should_defer_agent_workspace_repair_message_for_registry,
    spawn_deferred_agent_workspace_repair_message, store_agent_workspace_freshness,
    switch_agent_conversation_mode_for_state,
    switch_agent_conversation_mode_for_state_allowing_running,
    switch_agent_conversation_mode_for_state_stopping_running_agent,
    try_acquire_agent_workspace_publish_guard, update_agent_conversation_coordination_mode,
    update_agent_conversation_workspace_from_base_for_app_state,
    validate_explicit_publish_base_ref, AgentConversationResponse,
    AgentConversationRuntimeIndexGroup, AgentConversationRuntimeIndexKind,
    AgentConversationRuntimeLifecycle, AgentConversationRuntimeSource,
    AgentConversationWorkspaceAutoPublishInput, AgentConversationWorkspaceFreshnessResponse,
    AgentConversationWorkspacePrSupervisionInput, AgentConversationWorkspacePublishTarget,
    AgentConversationWorkspaceRepairTarget, AgentConversationWorkspaceResponse,
    AgentTimelineItemResponse, AgentWorkspaceExternalPrReconciliationTrigger,
    AgentWorkspaceFreshnessCacheEntry, AgentWorkspaceFreshnessCacheStatus,
    AgentWorkspaceFreshnessInvalidationGuard, AgentWorkspaceFreshnessScope,
    AgentWorkspacePostRepairAction, AgentWorkspacePrDescriptionInvalidationGuard,
    AgentWorkspaceRepairRuntimeOverrides, AgentWorkspaceSourcePullRequestInput,
    CommitAgentConversationWorkspaceLocallyResponse, CreateAgentConversationInput,
    DelegatedToolRuntimeSnapshot, ForkAgentConversationInput, ForkAgentConversationResponse,
    ModeSwitchInitiator, SwitchAgentConversationModeInput,
    UpdateAgentConversationCoordinationModeInput, AGENT_WORKSPACE_PUBLISH_IN_PROGRESS_MESSAGE,
    MAX_ATTRIBUTION_BATCH, STANDALONE_TEAM_INTENT_REJECTED_ERROR,
};
use crate::application::agent_conversation_workspace::{
    ensure_linked_plan_branch_agent_worktree, prepare_agent_conversation_workspace,
    resolve_linked_plan_branch_agent_worktree_path, AgentConversationWorkspaceBaseSelection,
};
use crate::application::agent_conversation_workspace_base::{
    BaseResolutionResult, BaseStatus, BLOCK_REASON_MISSING_BASE_COMMIT,
};
use crate::application::agent_workspace_pr_supervision_recovery::AgentWorkspacePrSupervisionRecoveryTrigger;
use crate::application::git_service::GitService;
use crate::application::managed_team::{
    ManagedTeamAssignmentRequest, ManagedTeamMemberSpec, ManagedTeamService,
    ManagedTeamWorkspaceRequest,
};
use crate::application::publish_resilience::PublishBranchFreshnessStatus;
use crate::application::{
    chat_service::{AgentRuntimeStatus, MockChatService},
    AgentTaskService, AppState,
};
use crate::commands::ExecutionState;
use crate::domain::agents::{
    AgentConfig, AgentHandle, AgentHarnessKind, AgentModelDefinition, AgentOutput, AgentResponse,
    AgentResult, AgenticClient, ClientCapabilities, LogicalEffort, ManualRoleRuntimeOverride,
    ManualServiceTier, ProviderSessionRef, ResponseChunk,
};
use crate::domain::entities::plan_branch::{PrPushStatus, PrStatus};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceBranchMode,
    AgentConversationWorkspaceMode, AgentConversationWorkspacePublicationEvent, AgentRun,
    AgentRunId, AgentRunStatus, AgentTaskAssignmentState, AgentTaskCreate, AgentTaskScope,
    AgentWorkspacePrMetadataDecision, AgentWorkspaceRepairAttempt,
    AgentWorkspaceRepairContinuation, AgentWorkspaceRepairPhase, AgentWorkspaceRepairSource,
    AgentWorkspaceReviewAutoMergeGuard, AgentWorkspaceReviewAutoMergeGuardStatus,
    AgentWorkspaceReviewGateStatus, AgentWorkspaceReviewMonitor, AgentWorkspaceReviewMonitorStatus,
    AgentWorkspaceReviewOutcome, AgentWorkspaceReviewTargetScope, AgentWorkspaceSourcePullRequest,
    ArtifactId, AutomationId, AutomationRunId, ChatContextType, ChatConversation,
    ChatConversationId, ChatMessage, ChatMessageId, ChatTimelineItem, ChatTimelineItemId,
    ChatTimelineItemKind, ChatTimelineItemStatus, CoordinationMode, DelegatedSession,
    DelegatedSessionId, ExecutionPlan, ExecutionPlanId, ExecutionPlanStatus,
    IdeationAnalysisBaseRefKind, IdeationSession, IdeationSessionFlow, IdeationSessionId,
    InternalStatus, MessageRole, PlanBranch, PlanBranchId, PlanBranchStatus, Project, ProjectId,
    RuntimeSource, SessionPurpose, Task, TaskId, TeamIntent, TeamMember, TeamMemberId,
    TeamMemberStatus, TeamRunBindingStatus, TeamSession, TeamSessionId, TeamSessionStatus,
    TeamWorkClassification, DEFAULT_AGENT_WORKSPACE_PR_AUTO_MERGE_METHOD,
};
use crate::domain::execution::ExecutionSettings;
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, AgentWorkspaceRepairAttemptTransition,
    AgentWorkspaceRepairAttemptTransitionOutcome, StartOrJoinAgentWorkspaceRepairAttempt,
    StartOrJoinAgentWorkspaceRepairAttemptOutcome, TeamRepository,
    TeamWorkspaceReservationRepository,
};
use crate::domain::review::ReviewSettings;
use crate::domain::services::github_generated_markdown::{
    decompose_ralphx_managed_pr_body, RALPHX_GENERATED_FOOTER, RALPHX_MANAGED_PR_BODY_END,
    RALPHX_MANAGED_PR_BODY_START,
};
use crate::domain::services::github_service::PrDetail;
use crate::domain::services::github_service::{PrAutoMergeRequest, PrHealth};
use crate::domain::services::{
    GithubServiceTrait, MemoryRunningAgentRegistry, PrBranchMatch, PrMergeStateStatus,
    PrMergeableState, PrStatus as GithubPrStatus, PrSyncState, RunningAgentKey,
    RunningAgentRegistry,
};
use crate::error::{AppError, AppResult};
use crate::infrastructure::memory::{
    MemoryAgentConversationWorkspaceRepository, MemoryQueuedMessageRepository,
    MemoryTeamCoordinationTransitionRepository, MemoryTeamMessageRepository, MemoryTeamRepository,
    MemoryTeamRunBindingRepository, MemoryTeamWakeBatchRepository,
    MemoryTeamWorkspaceReservationRepository,
};
use crate::infrastructure::{MockAgenticClient, MockCallType};
use crate::tests::mock_github_service::MockGithubService;
use async_trait::async_trait;
use futures::{stream, Stream};
use serde_json::json;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Command;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::test::{mock_builder, mock_context, noop_assets};
use tauri::Manager;

#[test]
fn hidden_user_message_metadata_suppresses_visible_chat_message() {
    let metadata: serde_json::Value =
        serde_json::from_str(&hidden_user_message_metadata()).expect("metadata json");

    assert_eq!(metadata["source"], "hidden_user_message");
    assert_eq!(metadata["resume_in_place"], true);
    assert_eq!(metadata["persist_hidden_marker"], true);
    assert_eq!(metadata["hidden_from_ui"], true);
    assert_eq!(metadata["recovery_context"], true);
}

#[tokio::test]
async fn terminal_publish_and_repair_handoff_statuses_release_the_owned_lease() {
    let state = AppState::new_test();
    let workspace = command_test_workspace();
    let conversation_id = workspace.conversation_id.clone();
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should seed");
    let operation_scope =
        crate::application::agent_workspace_publish_lease::begin_publish_operation_scope(
            &conversation_id,
        );
    mark_agent_workspace_publish_status(&state, &workspace, "checking", &operation_scope)
        .await
        .expect("publish operation should claim its lease");
    mark_agent_workspace_publish_status(&state, &workspace, "refreshed", &operation_scope)
        .await
        .expect("successful update should settle its lease");
    let refreshed = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace should load")
        .expect("workspace should exist");
    assert_eq!(refreshed.publish_lease_token, None);
    assert_eq!(
        refreshed.publication_push_status.as_deref(),
        Some("refreshed")
    );

    state
        .agent_conversation_workspace_repo
        .claim_publish_lease(
            &conversation_id,
            &format!("publish-operation:{conversation_id}"),
            "repair-handoff-token",
            chrono::Utc::now(),
            None,
            false,
        )
        .await
        .expect("repair handoff lease should seed");
    settle_agent_workspace_publish_lease_status(
        &state,
        &workspace,
        "needs_agent",
        Some("repair-handoff-token"),
    )
    .await
    .expect("repair handoff should settle its lease");
    let handed_off = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace should load")
        .expect("workspace should exist");
    assert_eq!(handed_off.publish_lease_owner_run_id, None);
    assert_eq!(handed_off.publish_lease_token, None);
    assert_eq!(
        handed_off.publication_push_status.as_deref(),
        Some("needs_agent")
    );
}

#[tokio::test]
async fn terminal_status_without_an_owned_token_preserves_a_live_foreign_lease() {
    let state = AppState::new_test();
    let workspace = command_test_workspace();
    let conversation_id = workspace.conversation_id.clone();
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should seed");
    state
        .agent_conversation_workspace_repo
        .claim_publish_lease(
            &conversation_id,
            &format!("publish-operation:{conversation_id}"),
            "live-owner-token",
            chrono::Utc::now(),
            None,
            false,
        )
        .await
        .expect("live owner lease should seed");

    settle_agent_workspace_publish_lease_status(&state, &workspace, "failed", None)
        .await
        .expect("non-owner failure status should still persist");

    let refreshed = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace should load")
        .expect("workspace should exist");
    assert_eq!(
        refreshed.publish_lease_token.as_deref(),
        Some("live-owner-token"),
        "a pre-claim failure must not clear another operation's live lease"
    );
    assert_eq!(refreshed.publication_push_status.as_deref(), Some("failed"));

    let competing_scope =
        crate::application::agent_workspace_publish_lease::begin_publish_operation_scope(
            &conversation_id,
        );
    mark_agent_workspace_publish_status(&state, &workspace, "no_changes", &competing_scope)
        .await
        .expect("non-owner terminal status should remain a lease no-op");
    let refreshed = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace should reload")
        .expect("workspace should exist");
    assert_eq!(
        refreshed.publish_lease_token.as_deref(),
        Some("live-owner-token"),
        "a competing operation must not release the row's current token"
    );
    assert_eq!(
        refreshed.publication_push_status.as_deref(),
        Some("no_changes")
    );
}

fn workspace_for_runtime_test(
    conversation_id: &ChatConversationId,
    project_id: &ProjectId,
) -> AgentConversationWorkspace {
    AgentConversationWorkspace::new(
        conversation_id.clone(),
        project_id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        None,
        "ralphx/test".to_string(),
        "/tmp/ralphx-test-worktree".to_string(),
    )
}

#[tokio::test]
async fn pr_metadata_target_uses_authoritative_existing_pr_snapshot() {
    let conversation_id = ChatConversationId::new();
    let project_id = ProjectId::new();
    let mut workspace = workspace_for_runtime_test(&conversation_id, &project_id);
    workspace.publication_pr_number = Some(42);
    let github = Arc::new(MockGithubService::new());
    github.will_return_pr_detail(PrDetail {
        number: 42,
        title: "Existing title".to_string(),
        body: Some("Existing body".to_string()),
        author: Some("octocat".to_string()),
        created_at: None,
        url: Some("https://github.com/example/project/pull/42".to_string()),
        state: GithubPrStatus::Open,
        is_draft: true,
        head_ref_name: workspace.branch_name.clone(),
        base_ref_name: workspace.base_ref.clone(),
    });

    let target = resolve_agent_workspace_pr_metadata_target(
        Some(github.as_ref()),
        Path::new("/tmp/ralphx-test-worktree"),
        &workspace,
    )
    .await
    .expect("existing target should resolve");

    match target {
        super::ResolvedAgentWorkspacePrTarget::Existing(snapshot) => {
            assert_eq!(snapshot.number, 42);
            assert_eq!(snapshot.title, "Existing title");
            assert_eq!(snapshot.body.as_deref(), Some("Existing body"));
            assert!(!snapshot.authority_fingerprint().is_empty());
        }
        super::ResolvedAgentWorkspacePrTarget::NewPr => {
            panic!("linked workspace must resolve an existing PR target")
        }
    }
    let state = github.state();
    assert_eq!(state.fetch_pr_detail_calls, 1);
    assert_eq!(state.last_fetch_pr_detail_number, Some(42));
}

async fn register_runtime_context(
    state: &AppState,
    context_type: ChatContextType,
    context_id: &str,
) {
    state
        .running_agent_registry
        .register(
            RunningAgentKey::new(context_type.to_string(), context_id.to_string()),
            0,
            format!("{context_type}-{context_id}-conversation"),
            String::new(),
            None,
            None,
        )
        .await;
}

#[test]
fn local_commit_ipc_response_serializes_camel_case_contract_fields() {
    let conversation_id =
        ChatConversationId::from_string("commit-contract-conversation".to_string());
    let project_id = ProjectId::from_string("commit-contract-project".to_string());
    let response = CommitAgentConversationWorkspaceLocallyResponse {
        workspace: AgentConversationWorkspaceResponse::from(workspace_for_runtime_test(
            &conversation_id,
            &project_id,
        )),
        outcome: "committed_local".to_string(),
        branch_name: "ralphx/commit-contract".to_string(),
        previous_head_sha: "before".to_string(),
        commit_sha: "after".to_string(),
        had_changes: true,
        attempt_token: "attempt-1".to_string(),
    };

    let value = serde_json::to_value(response).expect("IPC response should serialize");

    assert_eq!(value["branchName"], "ralphx/commit-contract");
    assert_eq!(value["previousHeadSha"], "before");
    assert_eq!(value["commitSha"], "after");
    assert_eq!(value["hadChanges"], true);
    assert_eq!(value["attemptToken"], "attempt-1");
    assert!(value.get("branch").is_none());
    assert!(value.get("currentHeadSha").is_none());
}

#[tokio::test]
async fn agent_conversation_runtime_status_includes_linked_ideation_and_verification() {
    let state = AppState::new_sqlite_test();
    let execution_state = Arc::new(ExecutionState::new());
    let project_id = ProjectId::from_string("project-runtime-status".to_string());
    let conversation_id = ChatConversationId::new();

    let parent = IdeationSession::new_with_title(project_id.clone(), "Plan draft");
    let parent_id = parent.id.clone();
    state.ideation_session_repo.create(parent).await.unwrap();

    let mut child = IdeationSession::new_with_title(project_id.clone(), "Verification run");
    child.parent_session_id = Some(parent_id.clone());
    child.session_purpose = SessionPurpose::Verification;
    let child_id = child.id.clone();
    state.ideation_session_repo.create(child).await.unwrap();

    let mut workspace = workspace_for_runtime_test(&conversation_id, &project_id);
    workspace.linked_ideation_session_id = Some(parent_id.clone());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();

    register_runtime_context(&state, ChatContextType::Ideation, parent_id.as_str()).await;
    register_runtime_context(&state, ChatContextType::Ideation, child_id.as_str()).await;

    let statuses = get_agent_conversation_runtime_statuses_for_app_state(
        &state,
        execution_state,
        vec![conversation_id.as_str().to_string()],
    )
    .await
    .unwrap();
    let conversation_key = conversation_id.as_str();
    let runtime = statuses.get(&conversation_key).unwrap();

    assert!(runtime.is_running);
    assert_eq!(runtime.summary_label.as_deref(), Some("Verifying"));
    assert_eq!(
        runtime.primary_source,
        Some(AgentConversationRuntimeSource::Verification)
    );
    assert!(runtime.items.iter().any(|item| item.source
        == AgentConversationRuntimeSource::Ideation
        && item.context_id == parent_id.as_str()));
    let verification = runtime
        .items
        .iter()
        .find(|item| item.source == AgentConversationRuntimeSource::Verification)
        .expect("verification child item");
    assert_eq!(verification.context_id, child_id.as_str());
    assert_eq!(
        verification.parent_session_id.as_deref(),
        Some(parent_id.as_str())
    );
    assert_eq!(
        verification.child_session_id.as_deref(),
        Some(child_id.as_str())
    );
}

#[tokio::test]
async fn agent_conversation_runtime_status_includes_workspace_review_child_chat() {
    let state = AppState::new_sqlite_test();
    let execution_state = Arc::new(ExecutionState::new());
    let project_id = ProjectId::from_string("project-workspace-review-runtime".to_string());
    let conversation_id = ChatConversationId::new();
    let review_conversation_id = ChatConversationId::new();

    let workspace = workspace_for_runtime_test(&conversation_id, &project_id);
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();

    let mut review_conversation = ChatConversation::new_project(project_id.clone());
    review_conversation.id = review_conversation_id.clone();
    review_conversation.parent_conversation_id = Some(conversation_id.as_str());
    review_conversation.title = Some("Review workspace changes".to_string());
    state
        .chat_conversation_repo
        .create(review_conversation)
        .await
        .unwrap();

    let review_run = AgentRun::new(review_conversation_id.clone());
    let review_run_id = review_run.id;
    state.agent_run_repo.create(review_run).await.unwrap();
    state
        .running_agent_registry
        .register(
            RunningAgentKey::new(
                ChatContextType::Project.to_string(),
                review_conversation_id.as_str(),
            ),
            0,
            review_conversation_id.as_str(),
            review_run_id.as_str().to_string(),
            None,
            None,
        )
        .await;

    let mut monitor = AgentWorkspaceReviewMonitor::new(conversation_id.clone(), project_id.clone());
    monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    monitor.review_conversation_id = Some(review_conversation_id.clone());
    monitor.last_run_id = Some(review_run_id.as_str().to_string());
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .unwrap();

    let statuses = get_agent_conversation_runtime_statuses_for_app_state(
        &state,
        execution_state,
        vec![conversation_id.as_str().to_string()],
    )
    .await
    .unwrap();
    let conversation_key = conversation_id.as_str();
    let review_conversation_key = review_conversation_id.as_str();
    let runtime = statuses.get(&conversation_key).unwrap();

    assert!(runtime.is_running);
    assert_eq!(runtime.summary_label.as_deref(), Some("Reviewing"));
    assert_eq!(
        runtime.primary_source,
        Some(AgentConversationRuntimeSource::WorkspaceReview)
    );
    assert_eq!(runtime.items.len(), 1);
    let item = &runtime.items[0];
    assert_eq!(item.source, AgentConversationRuntimeSource::WorkspaceReview);
    assert_eq!(item.context_type, "project");
    assert_eq!(item.context_id, review_conversation_key);
    assert_eq!(
        item.conversation_id.as_deref(),
        Some(review_conversation_key.as_str())
    );
    assert_eq!(item.title, "Review workspace changes");
    assert!(item.task_id.is_none());
}

#[tokio::test]
async fn agent_conversation_runtime_status_ignores_terminal_workspace_review_child_run() {
    let state = AppState::new_sqlite_test();
    let execution_state = Arc::new(ExecutionState::new());
    let project_id =
        ProjectId::from_string("project-workspace-review-terminal-runtime".to_string());
    let conversation_id = ChatConversationId::new();
    let review_conversation_id = ChatConversationId::new();

    let workspace = workspace_for_runtime_test(&conversation_id, &project_id);
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();

    let mut review_conversation = ChatConversation::new_project(project_id.clone());
    review_conversation.id = review_conversation_id.clone();
    review_conversation.parent_conversation_id = Some(conversation_id.as_str());
    review_conversation.title = Some("Review workspace changes".to_string());
    state
        .chat_conversation_repo
        .create(review_conversation)
        .await
        .unwrap();

    let mut review_run = AgentRun::new(review_conversation_id.clone());
    let review_run_id = review_run.id;
    review_run.fail("Workspace reviewer stopped by user");
    state.agent_run_repo.create(review_run).await.unwrap();

    let mut monitor = AgentWorkspaceReviewMonitor::new(conversation_id.clone(), project_id.clone());
    monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    monitor.review_conversation_id = Some(review_conversation_id);
    monitor.last_run_id = Some(review_run_id.as_str().to_string());
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .unwrap();

    let statuses = get_agent_conversation_runtime_statuses_for_app_state(
        &state,
        execution_state,
        vec![conversation_id.as_str().to_string()],
    )
    .await
    .unwrap();
    let runtime = statuses.get(&conversation_id.as_str()).unwrap();

    assert!(!runtime.is_running);
    assert!(runtime.items.is_empty());
}

#[tokio::test]
async fn agent_conversation_runtime_index_keeps_terminal_workspace_review_row() {
    let state = AppState::new_sqlite_test();
    let execution_state = ExecutionState::new();
    let project_id = ProjectId::from_string("project-workspace-review-index-terminal".to_string());
    let conversation_id = ChatConversationId::new();
    let review_conversation_id = ChatConversationId::new();

    let workspace = workspace_for_runtime_test(&conversation_id, &project_id);
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();

    let mut review_conversation = ChatConversation::new_project(project_id.clone());
    review_conversation.id = review_conversation_id.clone();
    review_conversation.parent_conversation_id = Some(conversation_id.as_str());
    review_conversation.title = Some("Review workspace changes".to_string());
    state
        .chat_conversation_repo
        .create(review_conversation)
        .await
        .unwrap();

    let mut review_run = AgentRun::new(review_conversation_id.clone());
    let review_run_id = review_run.id;
    review_run.fail("Workspace reviewer stopped by user");
    state.agent_run_repo.create(review_run).await.unwrap();

    let mut monitor = AgentWorkspaceReviewMonitor::new(conversation_id.clone(), project_id.clone());
    monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    monitor.review_conversation_id = Some(review_conversation_id.clone());
    monitor.last_run_id = Some(review_run_id.as_str().to_string());
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .unwrap();

    let index = get_agent_conversation_runtime_index_for_app_state(
        &state,
        &execution_state,
        conversation_id.as_str(),
    )
    .await
    .unwrap();

    assert_eq!(
        index.rows[0].group,
        AgentConversationRuntimeIndexGroup::Main
    );
    assert_eq!(
        index.rows[0].kind,
        AgentConversationRuntimeIndexKind::Workspace
    );
    let review = index
        .rows
        .iter()
        .find(|row| row.kind == AgentConversationRuntimeIndexKind::WorkspaceReview)
        .expect("durable workspace review row");
    assert_eq!(review.lifecycle, AgentConversationRuntimeLifecycle::Failed);
    assert_eq!(
        review.conversation_id.as_deref(),
        Some(review_conversation_id.as_str().as_str())
    );
    assert_eq!(
        review.error_message.as_deref(),
        Some("Workspace reviewer stopped by user")
    );
}

#[tokio::test]
async fn agent_conversation_runtime_index_includes_terminal_children_and_planned_tasks() {
    let state = AppState::new_sqlite_test();
    let execution_state = ExecutionState::new();
    let project_id = ProjectId::from_string("project-runtime-index-children".to_string());
    let conversation_id = ChatConversationId::new();
    let plan_branch_id = PlanBranchId::from_string("plan-branch-runtime-index");
    let execution_plan_id = ExecutionPlanId::from_string("execution-plan-runtime-index");

    let parent = IdeationSession::new_with_title(project_id.clone(), "Plan draft");
    let parent_id = parent.id.clone();
    state.ideation_session_repo.create(parent).await.unwrap();

    let mut parent_conversation = ChatConversation::new_ideation(parent_id.clone());
    parent_conversation.provider_harness = Some(AgentHarnessKind::Codex);
    parent_conversation.provider_session_id = Some("codex-session-parent".to_string());
    let parent_conversation = state
        .chat_conversation_repo
        .create(parent_conversation)
        .await
        .unwrap();
    let mut parent_run = AgentRun::new(parent_conversation.id.clone());
    parent_run.harness = Some(AgentHarnessKind::Codex);
    parent_run.provider_session_id = Some("codex-run-parent".to_string());
    parent_run.complete();
    state.agent_run_repo.create(parent_run).await.unwrap();

    let mut child = IdeationSession::new_with_title(project_id.clone(), "Verification run");
    child.parent_session_id = Some(parent_id.clone());
    child.session_purpose = SessionPurpose::Verification;
    child.status = crate::domain::entities::IdeationSessionStatus::Accepted;
    let child_id = child.id.clone();
    state.ideation_session_repo.create(child).await.unwrap();

    let mut plan_branch = PlanBranch::new(
        ArtifactId::from_string("artifact-runtime-index"),
        parent_id.clone(),
        project_id.clone(),
        "ralphx/test-plan".to_string(),
        "main".to_string(),
    );
    plan_branch.id = plan_branch_id.clone();
    plan_branch.execution_plan_id = Some(execution_plan_id.clone());
    state.plan_branch_repo.create(plan_branch).await.unwrap();

    let mut workspace = workspace_for_runtime_test(&conversation_id, &project_id);
    workspace.linked_ideation_session_id = Some(parent_id.clone());
    workspace.linked_plan_branch_id = Some(plan_branch_id);
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();

    let mut planned_task = Task::new(project_id.clone(), "Planned pipeline task".to_string());
    planned_task.internal_status = InternalStatus::Ready;
    planned_task.execution_plan_id = Some(execution_plan_id);
    let planned_task = state.task_repo.create(planned_task).await.unwrap();

    let index = get_agent_conversation_runtime_index_for_app_state(
        &state,
        &execution_state,
        conversation_id.as_str(),
    )
    .await
    .unwrap();

    let ideation = index
        .rows
        .iter()
        .find(|row| row.kind == AgentConversationRuntimeIndexKind::Ideation)
        .expect("ideation row");
    assert_eq!(
        ideation.lifecycle,
        AgentConversationRuntimeLifecycle::Completed
    );
    assert_eq!(ideation.provider_harness.as_deref(), Some("codex"));
    assert_eq!(
        ideation.provider_session_id.as_deref(),
        Some("codex-run-parent")
    );

    let verification = index
        .rows
        .iter()
        .find(|row| row.kind == AgentConversationRuntimeIndexKind::Verification)
        .expect("verification row");
    assert_eq!(
        verification.parent_session_id.as_deref(),
        Some(parent_id.as_str())
    );
    assert_eq!(
        verification.child_session_id.as_deref(),
        Some(child_id.as_str())
    );
    assert_eq!(
        verification.lifecycle,
        AgentConversationRuntimeLifecycle::Completed
    );

    let task = index
        .rows
        .iter()
        .find(|row| row.kind == AgentConversationRuntimeIndexKind::Task)
        .expect("planned task row");
    assert_eq!(task.task_id.as_deref(), Some(planned_task.id.as_str()));
    assert_eq!(task.lifecycle, AgentConversationRuntimeLifecycle::Queued);
    assert_eq!(task.status_label, "Queued");
    assert_eq!(task.group, AgentConversationRuntimeIndexGroup::Pipeline);
}

#[tokio::test]
async fn agent_conversation_runtime_status_filters_task_runs_to_linked_plan_branch() {
    let state = AppState::new_sqlite_test();
    let execution_state = Arc::new(ExecutionState::new());
    let project_id = ProjectId::from_string("project-task-runtime-status".to_string());
    let conversation_id = ChatConversationId::new();
    let plan_branch_id = PlanBranchId::from_string("plan-branch-runtime-status");
    let execution_plan_id = ExecutionPlanId::from_string("execution-plan-runtime-status");
    let other_execution_plan_id = ExecutionPlanId::from_string("execution-plan-other");

    let mut plan_branch = PlanBranch::new(
        ArtifactId::from_string("artifact-runtime-status"),
        IdeationSessionId::from_string("session-runtime-status"),
        project_id.clone(),
        "ralphx/test-plan".to_string(),
        "main".to_string(),
    );
    plan_branch.id = plan_branch_id.clone();
    plan_branch.execution_plan_id = Some(execution_plan_id.clone());
    state.plan_branch_repo.create(plan_branch).await.unwrap();

    let mut workspace = workspace_for_runtime_test(&conversation_id, &project_id);
    workspace.linked_plan_branch_id = Some(plan_branch_id);
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();

    let mut owned_task = Task::new(project_id.clone(), "Owned execution task".to_string());
    owned_task.internal_status = InternalStatus::Executing;
    owned_task.execution_plan_id = Some(execution_plan_id);
    let owned_task = state.task_repo.create(owned_task).await.unwrap();

    let mut unrelated_task = Task::new(project_id.clone(), "Other execution task".to_string());
    unrelated_task.internal_status = InternalStatus::Executing;
    unrelated_task.execution_plan_id = Some(other_execution_plan_id);
    let unrelated_task = state.task_repo.create(unrelated_task).await.unwrap();

    register_runtime_context(
        &state,
        ChatContextType::TaskExecution,
        owned_task.id.as_str(),
    )
    .await;
    register_runtime_context(
        &state,
        ChatContextType::TaskExecution,
        unrelated_task.id.as_str(),
    )
    .await;

    let statuses = get_agent_conversation_runtime_statuses_for_app_state(
        &state,
        execution_state,
        vec![conversation_id.as_str().to_string()],
    )
    .await
    .unwrap();
    let conversation_key = conversation_id.as_str();
    let runtime = statuses.get(&conversation_key).unwrap();

    assert!(runtime.is_running);
    assert_eq!(runtime.summary_label.as_deref(), Some("Executing"));
    assert_eq!(
        runtime.primary_source,
        Some(AgentConversationRuntimeSource::TaskExecution)
    );
    assert_eq!(runtime.items.len(), 1);
    let item = &runtime.items[0];
    assert_eq!(item.source, AgentConversationRuntimeSource::TaskExecution);
    assert_eq!(item.task_id.as_deref(), Some(owned_task.id.as_str()));
    assert_ne!(item.task_id.as_deref(), Some(unrelated_task.id.as_str()));
    assert_eq!(item.context_type, "task_execution");
}

#[tokio::test]
async fn agent_conversation_runtime_status_reports_idle_workspace_ipr_as_waiting() {
    let state = AppState::new_sqlite_test();
    let execution_state = Arc::new(ExecutionState::new());
    let conversation_id = ChatConversationId::new();
    let run = AgentRun::new(conversation_id);
    let run_id = run.id;
    state.agent_run_repo.create(run).await.unwrap();
    state
        .running_agent_registry
        .register(
            RunningAgentKey::new(
                ChatContextType::Project.to_string(),
                conversation_id.as_str(),
            ),
            std::process::id(),
            conversation_id.as_str(),
            run_id.as_str().to_string(),
            None,
            None,
        )
        .await;
    execution_state.mark_interactive_idle(&agent_workspace_interactive_slot_key(&conversation_id));

    let statuses = get_agent_conversation_runtime_statuses_for_app_state(
        &state,
        execution_state,
        vec![conversation_id.as_str().to_string()],
    )
    .await
    .unwrap();
    let runtime = statuses.get(&conversation_id.as_str()).unwrap();

    assert!(runtime.is_running);
    assert_eq!(runtime.agent_status, AgentRuntimeStatus::WaitingForInput);
    assert_eq!(runtime.summary_label.as_deref(), Some("Awaiting input"));
    assert_eq!(runtime.items.len(), 1);
    let item = &runtime.items[0];
    assert_eq!(item.source, AgentConversationRuntimeSource::Workspace);
    assert_eq!(item.agent_status, AgentRuntimeStatus::WaitingForInput);
}

fn build_send_now_command_app(state: AppState) -> tauri::App<tauri::test::MockRuntime> {
    mock_builder()
        .manage(state)
        .manage(Arc::new(ExecutionState::new()))
        .build(mock_context(noop_assets()))
        .expect("mock app should build")
}

#[tokio::test]
async fn get_agent_run_attribution_returns_persisted_run_and_rejects_missing_id() {
    let state = AppState::new_test();
    let mut run = AgentRun::new(ChatConversationId::new());
    let run_id = run.id.as_str().to_string();
    run.agent_name = Some("ralphx-workspace-reviewer".to_string());
    run.launch_role = Some("workspace_reviewer".to_string());
    run.runtime_source = Some(RuntimeSource::RoleDefault);
    state.agent_run_repo.create(run).await.unwrap();
    let app = build_send_now_command_app(state);

    let found = get_agent_run_attribution(run_id, app.state())
        .await
        .expect("persisted run should be returned");
    assert_eq!(
        found.agent_name.as_deref(),
        Some("ralphx-workspace-reviewer")
    );
    assert_eq!(found.launch_role.as_deref(), Some("workspace_reviewer"));
    assert_eq!(found.runtime_source, Some(RuntimeSource::RoleDefault));

    let error = get_agent_run_attribution("missing-run".to_string(), app.state())
        .await
        .expect_err("missing run must return a typed not-found error");
    assert!(matches!(error, AppError::NotFound(_)));
}

#[tokio::test]
async fn get_agent_run_attributions_returns_known_runs_and_rejects_oversized_batches() {
    let state = AppState::new_test();
    let first = AgentRun::new(ChatConversationId::new());
    let first_id = first.id.as_str().to_string();
    let second = AgentRun::new(ChatConversationId::new());
    let second_id = second.id.as_str().to_string();
    state.agent_run_repo.create(first).await.unwrap();
    state.agent_run_repo.create(second).await.unwrap();
    let app = build_send_now_command_app(state);

    let found = get_agent_run_attributions(
        vec![
            first_id.clone(),
            "missing-run".to_string(),
            second_id.clone(),
        ],
        app.state(),
    )
    .await
    .expect("known runs should be returned");
    let found_ids = found
        .into_iter()
        .map(|run| run.id.as_str())
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(
        found_ids,
        std::collections::HashSet::from([first_id, second_id])
    );

    let error = get_agent_run_attributions(
        (0..=MAX_ATTRIBUTION_BATCH)
            .map(|_| AgentRunId::new().as_str())
            .collect(),
        app.state(),
    )
    .await
    .expect_err("over-limit batch must be rejected");
    assert!(matches!(error, AppError::InvalidInput(_)));

    assert!(get_agent_run_attributions(Vec::new(), app.state())
        .await
        .expect("empty batch does not require a repository read")
        .is_empty());
}

fn enable_team_capability_for_test(state: &AppState) {
    state.agent_capability_gate.replace(
        crate::application::agent_capability_gate::AgentCapabilities {
            team: true,
            workflows: false,
            autopilot: false,
        },
    );
}

fn align_managed_team_for_command_test(
    state: &mut AppState,
) -> Arc<MemoryTeamWorkspaceReservationRepository> {
    let sessions = MemoryTeamRepository::new_shared_sessions();
    let reservation_repo = Arc::new(MemoryTeamWorkspaceReservationRepository::new());
    state.managed_team = Arc::new(ManagedTeamService::new(
        Arc::new(MemoryTeamRepository::with_sessions(Arc::clone(&sessions))),
        Arc::new(MemoryTeamCoordinationTransitionRepository::with_sessions(
            sessions,
        )),
        Arc::new(MemoryTeamRunBindingRepository::new()),
        Arc::new(MemoryTeamMessageRepository::new()),
        Arc::new(MemoryTeamWakeBatchRepository::new()),
        Arc::new(MemoryQueuedMessageRepository::new()),
        Arc::clone(&state.chat_conversation_repo),
        Arc::clone(&state.agent_run_repo),
        Arc::clone(&reservation_repo) as Arc<dyn TeamWorkspaceReservationRepository>,
        Arc::clone(&state.ui_feature_flag_overrides_repo),
    ));
    reservation_repo
}

struct OpenSessionReadFailingTeamRepository;

#[async_trait]
impl TeamRepository for OpenSessionReadFailingTeamRepository {
    async fn ensure_session(&self, _session: TeamSession) -> AppResult<TeamSession> {
        panic!("unexpected Team session write")
    }

    async fn get_session(&self, _id: &TeamSessionId) -> AppResult<Option<TeamSession>> {
        panic!("unexpected Team session lookup")
    }

    async fn get_open_session_for_conversation(
        &self,
        _conversation_id: &ChatConversationId,
    ) -> AppResult<Option<TeamSession>> {
        Err(AppError::Database(
            "Team session storage unavailable".to_string(),
        ))
    }

    async fn list_open_sessions(&self) -> AppResult<Vec<TeamSession>> {
        panic!("unexpected Team session list")
    }

    async fn update_session(
        &self,
        _session: TeamSession,
        _expected_version: i64,
    ) -> AppResult<bool> {
        panic!("unexpected Team session update")
    }

    async fn create_member(&self, _member: TeamMember) -> AppResult<TeamMember> {
        panic!("unexpected Team member write")
    }

    async fn get_member(&self, _id: &TeamMemberId) -> AppResult<Option<TeamMember>> {
        panic!("unexpected Team member lookup")
    }

    async fn list_members(&self, _team_id: &TeamSessionId) -> AppResult<Vec<TeamMember>> {
        panic!("unexpected Team member list")
    }

    async fn update_member(
        &self,
        _member: TeamMember,
        _expected_generation: i64,
    ) -> AppResult<bool> {
        panic!("unexpected Team member update")
    }
}

async fn seed_rx_native_team_conversation(state: &AppState) -> (ChatConversation, TeamSession) {
    let project_id = ProjectId::from_string("project-1".to_string());
    let mut conversation = ChatConversation::new_project(project_id.clone());
    conversation.coordination_mode = CoordinationMode::RxNativeTeam;
    let conversation = state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("Team conversation should be created");
    let team = state
        .managed_team
        .ensure_team(project_id, &conversation.id)
        .await
        .expect("Team session should be ensured");
    (conversation, team)
}

fn team_test_task(title: &str) -> AgentTaskCreate {
    AgentTaskCreate {
        title: title.to_string(),
        details: format!("{title} details"),
        active_label: None,
        owner_agent: None,
        metadata: None,
        blocked_by: Vec::new(),
        blocks: Vec::new(),
    }
}

#[tokio::test]
async fn create_agent_conversation_persists_team_intent_coordination_mode() {
    let state = AppState::new_test();
    enable_team_capability_for_test(&state);
    let app = build_send_now_command_app(state);
    let project_id = ProjectId::from_string("project-1".to_string());

    let response = create_agent_conversation(
        CreateAgentConversationInput {
            context_type: ChatContextType::Project.to_string(),
            context_id: Some(project_id.as_str().to_string()),
            title: Some("Team conversation".to_string()),
            mode: None,
            team_intent: Some(TeamIntent::rx_native(None)),
        },
        app.state(),
    )
    .await
    .expect("team conversation should be created");

    assert_eq!(response.coordination_mode, "rx_native_team");
    let stored = app
        .state::<AppState>()
        .chat_conversation_repo
        .get_by_id(&ChatConversationId::from_string(response.id))
        .await
        .expect("stored conversation should load")
        .expect("stored conversation should exist");
    assert_eq!(stored.coordination_mode, CoordinationMode::RxNativeTeam);
}

/// The standalone-conversations override is process-global. Acquiring this guard serializes every
/// test that sets it and restores the ambient value on drop, so a test asserting "flag off" can
/// never observe another test's "flag on".
fn standalone_conversations_flag_override_guard(
) -> crate::infrastructure::agents::LiveFlagOverrideTestGuard {
    crate::infrastructure::agents::LiveFlagOverrideTestGuard::default()
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn create_agent_conversation_standalone_flag_on_round_trips_self_keyed() {
    let _flag_guard = standalone_conversations_flag_override_guard();
    crate::infrastructure::agents::set_standalone_conversations_override(Some(true));
    let app = build_send_now_command_app(AppState::new_test());

    let response = create_agent_conversation(
        CreateAgentConversationInput {
            context_type: ChatContextType::Standalone.to_string(),
            context_id: None,
            title: Some("Standalone chat".to_string()),
            mode: None,
            team_intent: None,
        },
        app.state(),
    )
    .await
    .expect("standalone conversation should be created");

    assert_eq!(response.context_type, "standalone");
    assert_eq!(response.context_id, response.id);

    let stored = app
        .state::<AppState>()
        .chat_conversation_repo
        .get_by_id(&ChatConversationId::from_string(response.id))
        .await
        .expect("stored conversation should load")
        .expect("stored conversation should exist");
    assert!(stored.is_valid_standalone_self_key());
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn create_agent_conversation_standalone_flag_off_is_rejected() {
    let _flag_guard = standalone_conversations_flag_override_guard();
    crate::infrastructure::agents::set_standalone_conversations_override(Some(false));
    let app = build_send_now_command_app(AppState::new_test());

    let error = create_agent_conversation(
        CreateAgentConversationInput {
            context_type: ChatContextType::Standalone.to_string(),
            context_id: None,
            title: None,
            mode: None,
            team_intent: None,
        },
        app.state(),
    )
    .await
    .expect_err("standalone creation must be rejected while the flag is off");

    assert!(error.contains("standalone_conversations"));
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn create_agent_conversation_standalone_rejects_supplied_context_id() {
    let _flag_guard = standalone_conversations_flag_override_guard();
    crate::infrastructure::agents::set_standalone_conversations_override(Some(true));
    let app = build_send_now_command_app(AppState::new_test());

    let error = create_agent_conversation(
        CreateAgentConversationInput {
            context_type: ChatContextType::Standalone.to_string(),
            context_id: Some("caller-supplied-id".to_string()),
            title: None,
            mode: None,
            team_intent: None,
        },
        app.state(),
    )
    .await
    .expect_err("standalone creation must reject a caller-supplied context_id");

    assert!(error.contains("does not accept a context_id"));
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn create_agent_conversation_standalone_rejects_team_intent() {
    let _flag_guard = standalone_conversations_flag_override_guard();
    crate::infrastructure::agents::set_standalone_conversations_override(Some(true));
    let app = build_send_now_command_app(AppState::new_test());

    let error = create_agent_conversation(
        CreateAgentConversationInput {
            context_type: ChatContextType::Standalone.to_string(),
            context_id: None,
            title: None,
            mode: None,
            team_intent: Some(TeamIntent::rx_native(None)),
        },
        app.state(),
    )
    .await
    .expect_err("standalone creation must reject team intent");

    assert_eq!(error, STANDALONE_TEAM_INTENT_REJECTED_ERROR);
    assert!(app
        .state::<AppState>()
        .chat_conversation_repo
        .list_by_context_type(ChatContextType::Standalone, true, 10)
        .await
        .expect("standalone conversations should list")
        .is_empty());
}

#[tokio::test]
async fn update_agent_conversation_coordination_mode_persists_idle_project_conversation() {
    let state = AppState::new_test();
    enable_team_capability_for_test(&state);
    let project_id = ProjectId::from_string("project-1".to_string());
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project_id))
        .await
        .expect("conversation should be created");
    let app = build_send_now_command_app(state);

    let response = update_agent_conversation_coordination_mode(
        UpdateAgentConversationCoordinationModeInput {
            conversation_id: conversation.id.as_str(),
            coordination_mode: "rx_native_team".to_string(),
            model_override: None,
        },
        app.state(),
    )
    .await
    .expect("coordination mode should update");

    assert_eq!(response.coordination_mode, "rx_native_team");
    let stored = app
        .state::<AppState>()
        .chat_conversation_repo
        .get_by_id(&conversation.id)
        .await
        .expect("stored conversation should load")
        .expect("stored conversation should exist");
    assert_eq!(stored.coordination_mode, CoordinationMode::RxNativeTeam);
}

#[tokio::test]
async fn leaving_team_mode_performs_staged_drain() {
    let mut state = AppState::new_test();
    let reservation_repo = align_managed_team_for_command_test(&mut state);
    enable_team_capability_for_test(&state);
    let app = build_send_now_command_app(state);
    let state = app.state::<AppState>();
    let (conversation, team) = seed_rx_native_team_conversation(&state).await;
    let member = state
        .managed_team
        .add_member(
            &team.id,
            ManagedTeamMemberSpec {
                name: "Writer One".to_string(),
                canonical_agent_name: "ralphx-general-worker".to_string(),
                role_summary: "writes scoped changes".to_string(),
                harness: None,
                logical_model: None,
                logical_effort: None,
            },
        )
        .await
        .expect("Team member should be added");
    let task_service = AgentTaskService::new(state.agent_task_repo.clone());
    let mut scope = AgentTaskScope::new("conversation", conversation.id.as_str());
    scope.project_id = Some(ProjectId::from_string("project-1".to_string()));
    task_service
        .create_task(&scope, team_test_task("first Team task"))
        .await
        .expect("first Team task should be created");
    task_service
        .create_task(&scope, team_test_task("second Team task"))
        .await
        .expect("second Team task should be created");
    let plan = state
        .managed_team
        .plan_member_assignment(
            &task_service,
            ManagedTeamAssignmentRequest {
                team_id: team.id.clone(),
                member_name: member.normalized_name.clone(),
                expected_member_generation: member.generation,
                caller_scope: scope,
                caller_agent_run_id: AgentRunId::new(),
                task_ref: "1".to_string(),
                delegated_session_id: DelegatedSessionId::new(),
                delegated_conversation_id: ChatConversationId::new(),
                planned_agent_run_id: AgentRunId::new(),
                work_classification: TeamWorkClassification::Write,
                workspace: Some(ManagedTeamWorkspaceRequest {
                    writable_paths: vec!["src/owned.rs".to_string()],
                    generated_outputs: Vec::new(),
                    resource_locks: Vec::new(),
                }),
            },
        )
        .await
        .expect("active Team assignment should be planned");
    assert!(
        plan.reservation.is_some(),
        "test must seed a workspace reservation"
    );
    state
        .managed_team
        .mark_member_assignment_launching(&plan)
        .await
        .expect("Team assignment should enter launching state");
    state
        .managed_team
        .complete_member_assignment_launch(&task_service, &plan)
        .await
        .expect("Team assignment should bind its active run");

    let response = update_agent_conversation_coordination_mode(
        UpdateAgentConversationCoordinationModeInput {
            conversation_id: conversation.id.as_str(),
            coordination_mode: "solo".to_string(),
            model_override: None,
        },
        app.state(),
    )
    .await
    .expect("leaving Team mode should drain before changing capability");

    assert_eq!(response.coordination_mode, "solo");
    assert_eq!(
        state
            .managed_team
            .team_repo()
            .get_session(&team.id)
            .await
            .expect("Team session should load")
            .expect("Team session should exist")
            .status,
        TeamSessionStatus::Closed
    );
    let drained_member = state
        .managed_team
        .team_repo()
        .get_member(&member.id)
        .await
        .expect("Team member should load")
        .expect("Team member should exist");
    assert_eq!(drained_member.status, TeamMemberStatus::Stopped);
    assert!(drained_member.current_run_id.is_none());
    assert!(drained_member.current_assignment_id.is_none());
    let drained_binding = state
        .managed_team
        .run_binding_repo()
        .get_by_id(&plan.binding.id)
        .await
        .expect("Team binding should load")
        .expect("Team binding should exist");
    assert_eq!(drained_binding.status, TeamRunBindingStatus::Cancelled);
    assert_eq!(
        drained_binding.last_error.as_deref(),
        Some("team_exit_drain")
    );
    let assignment = state
        .agent_task_repo
        .get_assignment_for_run(&plan.binding.agent_run_id)
        .await
        .expect("Team assignment should load")
        .expect("Team assignment should exist");
    assert_eq!(
        assignment.assignment.state,
        AgentTaskAssignmentState::Cancelled
    );
    assert_eq!(
        assignment.assignment.settlement_reason.as_deref(),
        Some("team_exit_drain")
    );
    let active_reservations = reservation_repo
        .list_active_for_assignment(plan.assignment.assignment.id.as_str())
        .await
        .expect("active Team reservations should load");
    assert!(active_reservations.is_empty());
}

#[tokio::test]
async fn leaving_team_mode_resumes_pending_suspend_exit() {
    let mut state = AppState::new_test();
    align_managed_team_for_command_test(&mut state);
    enable_team_capability_for_test(&state);
    let app = build_send_now_command_app(state);
    let state = app.state::<AppState>();
    let (conversation, team) = seed_rx_native_team_conversation(&state).await;
    let mut pending = state
        .managed_team
        .team_repo()
        .get_session(&team.id)
        .await
        .expect("Team session should load")
        .expect("Team session should exist");
    pending.pending_exit_action = Some("suspend".to_string());
    pending.version += 1;
    assert!(state
        .managed_team
        .team_repo()
        .update_session(pending, team.version)
        .await
        .expect("pending action should be stored"));

    let response = update_agent_conversation_coordination_mode(
        UpdateAgentConversationCoordinationModeInput {
            conversation_id: conversation.id.as_str(),
            coordination_mode: "solo".to_string(),
            model_override: None,
        },
        app.state(),
    )
    .await
    .expect("stored suspend action should be resumed");

    assert_eq!(response.coordination_mode, "solo");
    assert_eq!(
        state
            .managed_team
            .team_repo()
            .get_session(&team.id)
            .await
            .expect("Team session should load")
            .expect("Team session should exist")
            .status,
        TeamSessionStatus::Suspended
    );
}

#[tokio::test]
async fn leaving_team_mode_fails_closed_for_corrupt_pending_exit_action() {
    let mut state = AppState::new_test();
    align_managed_team_for_command_test(&mut state);
    enable_team_capability_for_test(&state);
    let app = build_send_now_command_app(state);
    let state = app.state::<AppState>();
    let (conversation, team) = seed_rx_native_team_conversation(&state).await;
    let mut pending = state
        .managed_team
        .team_repo()
        .get_session(&team.id)
        .await
        .expect("Team session should load")
        .expect("Team session should exist");
    pending.pending_exit_action = Some("bogus".to_string());
    pending.version += 1;
    assert!(state
        .managed_team
        .team_repo()
        .update_session(pending, team.version)
        .await
        .expect("corrupt action should be stored"));

    let error = update_agent_conversation_coordination_mode(
        UpdateAgentConversationCoordinationModeInput {
            conversation_id: conversation.id.as_str(),
            coordination_mode: "solo".to_string(),
            model_override: None,
        },
        app.state(),
    )
    .await
    .expect_err("corrupt pending action must block the capability change");

    assert!(error.contains("managed Team exit action must be suspend or drain_and_close"));
    assert_eq!(
        state
            .chat_conversation_repo
            .get_by_id(&conversation.id)
            .await
            .expect("conversation should load")
            .expect("conversation should exist")
            .coordination_mode,
        CoordinationMode::RxNativeTeam
    );
    assert_eq!(
        state
            .managed_team
            .team_repo()
            .get_session(&team.id)
            .await
            .expect("Team session should load")
            .expect("Team session should exist")
            .status,
        TeamSessionStatus::Active
    );
}

#[tokio::test]
async fn leaving_team_mode_fails_closed_when_team_session_read_fails() {
    let mut state = AppState::new_test();
    enable_team_capability_for_test(&state);
    state.managed_team = Arc::new(ManagedTeamService::new(
        Arc::new(OpenSessionReadFailingTeamRepository),
        Arc::new(MemoryTeamCoordinationTransitionRepository::new()),
        Arc::new(MemoryTeamRunBindingRepository::new()),
        Arc::new(MemoryTeamMessageRepository::new()),
        Arc::new(MemoryTeamWakeBatchRepository::new()),
        Arc::new(MemoryQueuedMessageRepository::new()),
        Arc::clone(&state.chat_conversation_repo),
        Arc::clone(&state.agent_run_repo),
        Arc::new(MemoryTeamWorkspaceReservationRepository::new()),
        Arc::clone(&state.ui_feature_flag_overrides_repo),
    ));
    let mut conversation = ChatConversation::new_project(ProjectId::new());
    conversation.coordination_mode = CoordinationMode::RxNativeTeam;
    let conversation = state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("Team conversation should be created");
    let app = build_send_now_command_app(state);

    let error = update_agent_conversation_coordination_mode(
        UpdateAgentConversationCoordinationModeInput {
            conversation_id: conversation.id.as_str(),
            coordination_mode: "solo".to_string(),
            model_override: None,
        },
        app.state(),
    )
    .await
    .expect_err("Team repository read failure must block the capability change");

    assert!(error.contains("Team session storage unavailable"));
    assert_eq!(
        app.state::<AppState>()
            .chat_conversation_repo
            .get_by_id(&conversation.id)
            .await
            .expect("conversation should load")
            .expect("conversation should exist")
            .coordination_mode,
        CoordinationMode::RxNativeTeam
    );
}

#[tokio::test]
async fn update_agent_conversation_coordination_mode_rejects_legacy_writes() {
    let state = AppState::new_test();
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(ProjectId::from_string(
            "project-1".to_string(),
        )))
        .await
        .expect("conversation should be created");
    let app = build_send_now_command_app(state);

    let error = update_agent_conversation_coordination_mode(
        UpdateAgentConversationCoordinationModeInput {
            conversation_id: conversation.id.as_str(),
            coordination_mode: "legacy_claude_team".to_string(),
            model_override: None,
        },
        app.state(),
    )
    .await
    .expect_err("legacy team writes should be rejected");

    assert!(error.contains("Invalid coordination mode 'legacy_claude_team'"));
}

#[test]
fn normalized_effort_for_supported_keeps_supported_request_or_default() {
    let supported = [
        LogicalEffort::Low,
        LogicalEffort::Medium,
        LogicalEffort::High,
    ];

    assert_eq!(
        normalized_effort_for_supported(
            Some(LogicalEffort::High),
            &supported,
            LogicalEffort::Medium,
        ),
        LogicalEffort::High
    );
    assert_eq!(
        normalized_effort_for_supported(
            Some(LogicalEffort::Max),
            &supported,
            LogicalEffort::Medium,
        ),
        LogicalEffort::Medium
    );
    assert_eq!(
        normalized_effort_for_supported(None, &supported, LogicalEffort::Low),
        LogicalEffort::Low
    );
}

#[test]
fn normalize_agent_workspace_source_pull_request_trims_and_maps_valid_metadata() {
    let normalized = normalize_agent_workspace_source_pull_request(
        Some(AgentWorkspaceSourcePullRequestInput {
            number: 123,
            url: Some(" https://github.com/owner/repo/pull/123 ".to_string()),
            title: Some(" Add PR source context ".to_string()),
            head_ref_name: " feature/source-pr ".to_string(),
            base_ref_name: Some(" main ".to_string()),
            head_ref_oid: Some(" abc123 ".to_string()),
        }),
        Some(IdeationAnalysisBaseRefKind::LocalBranch),
        Some("feature/source-pr"),
    )
    .expect("valid source PR metadata should normalize")
    .expect("source PR metadata should be present");

    assert_eq!(normalized.number, 123);
    assert_eq!(
        normalized.url.as_deref(),
        Some("https://github.com/owner/repo/pull/123")
    );
    assert_eq!(normalized.title.as_deref(), Some("Add PR source context"));
    assert_eq!(normalized.head_ref_name, "feature/source-pr");
    assert_eq!(normalized.base_ref_name.as_deref(), Some("main"));
    assert_eq!(normalized.head_ref_oid.as_deref(), Some("abc123"));
}

#[test]
fn normalize_agent_workspace_source_pull_request_validates_pr_base_contract() {
    let input = AgentWorkspaceSourcePullRequestInput {
        number: 123,
        url: None,
        title: None,
        head_ref_name: "feature/source-pr".to_string(),
        base_ref_name: None,
        head_ref_oid: None,
    };

    assert_eq!(
        normalize_agent_workspace_source_pull_request(
            Some(input.clone()),
            Some(IdeationAnalysisBaseRefKind::ProjectDefault),
            Some("main"),
        )
        .expect_err("source PR metadata must use local branch base"),
        "Source pull request metadata requires a local_branch base ref"
    );
    assert_eq!(
        normalize_agent_workspace_source_pull_request(
            Some(input.clone()),
            Some(IdeationAnalysisBaseRefKind::LocalBranch),
            Some("different-branch"),
        )
        .expect_err("source PR head must match selected base"),
        "Source pull request head branch must match the selected base ref"
    );
    assert_eq!(
        normalize_agent_workspace_source_pull_request(
            Some(AgentWorkspaceSourcePullRequestInput { number: 0, ..input }),
            Some(IdeationAnalysisBaseRefKind::LocalBranch),
            Some("feature/source-pr"),
        )
        .expect_err("source PR number must be positive"),
        "Source pull request number must be positive"
    );
}

#[tokio::test]
async fn normalize_agent_runtime_without_provider_preserves_overrides() {
    let state = AppState::new_test();

    let normalized = normalize_agent_runtime_selection(
        &state,
        None,
        Some("manual-model".to_string()),
        Some(LogicalEffort::Max),
    )
    .await
    .expect("normalization should preserve providerless overrides");

    assert_eq!(
        normalized,
        (Some("manual-model".to_string()), Some(LogicalEffort::Max))
    );
}

#[tokio::test]
async fn normalize_agent_runtime_uses_known_model_compatibility() {
    let state = AppState::new_test();

    let normalized = normalize_agent_runtime_selection(
        &state,
        Some(AgentHarnessKind::Claude),
        Some("haiku".to_string()),
        Some(LogicalEffort::Max),
    )
    .await
    .expect("known model should normalize");

    assert_eq!(
        normalized,
        (Some("haiku".to_string()), Some(LogicalEffort::Medium))
    );
}

#[tokio::test]
async fn normalize_agent_runtime_keeps_codex_provider_supported_effort_for_unknown_model() {
    let state = AppState::new_test();

    let normalized = normalize_agent_runtime_selection(
        &state,
        Some(AgentHarnessKind::Codex),
        Some("gpt-5.6".to_string()),
        Some(LogicalEffort::Max),
    )
    .await
    .expect("unknown Codex model should use provider effort defaults");

    assert_eq!(
        normalized,
        (Some("gpt-5.6".to_string()), Some(LogicalEffort::Max))
    );
}

#[tokio::test]
async fn normalize_agent_runtime_uses_registry_default_when_model_absent() {
    let state = AppState::new_test();

    let normalized = normalize_agent_runtime_selection(
        &state,
        Some(AgentHarnessKind::Codex),
        None,
        Some(LogicalEffort::Low),
    )
    .await
    .expect("missing model should use registry defaults");

    assert_eq!(normalized, (None, Some(LogicalEffort::Low)));
}

#[tokio::test]
async fn normalize_agent_runtime_falls_back_when_provider_models_disabled() {
    let state = AppState::new_test();
    for model_id in [
        "sonnet",
        "claude-sonnet-4-6",
        "claude-sonnet-5",
        "opus",
        "claude-opus-4-7",
        "claude-opus-4-8",
        "claude-opus-5",
        "haiku",
        "fable",
    ] {
        state
            .agent_model_registry_repo
            .upsert_custom_model(&AgentModelDefinition::custom(
                AgentHarnessKind::Claude,
                model_id,
                model_id,
                model_id,
                None,
                vec![LogicalEffort::Low],
                LogicalEffort::Low,
                false,
            ))
            .await
            .expect("disabled override should save");
    }

    let normalized = normalize_agent_runtime_selection(
        &state,
        Some(AgentHarnessKind::Claude),
        None,
        Some(LogicalEffort::Max),
    )
    .await
    .expect("missing enabled default should use provider fallback");

    assert_eq!(normalized, (None, Some(LogicalEffort::Medium)));
}

#[test]
fn linked_plan_branch_publication_is_projected_into_workspace_response() {
    let mut response = AgentConversationWorkspaceResponse {
        conversation_id: "conversation-1".to_string(),
        project_id: "project-1".to_string(),
        mode: AgentConversationWorkspaceMode::Ideation.to_string(),
        branch_mode: "isolated".to_string(),
        base_ref_kind: "project_default".to_string(),
        base_ref: "main".to_string(),
        base_display_name: Some("Project default (main)".to_string()),
        base_commit: None,
        branch_name: "agent-d619a9fd".to_string(),
        worktree_path: "/tmp/workspace".to_string(),
        linked_ideation_session_id: Some("session-1".to_string()),
        task_pipeline_session_id: None,
        task_pipeline_available: false,
        linked_plan_branch_id: Some("plan-branch-1".to_string()),
        source_pull_request: None,
        publication_pr_number: None,
        publication_pr_url: None,
        publication_pr_status: None,
        publication_push_status: None,
        auto_publish_enabled: true,
        auto_publish_initial_pr_enabled: false,
        auto_publish_paused_pr_autofix_enabled: None,
        auto_publish_paused_pr_auto_merge_desired: None,
        pr_autofix_enabled: false,
        pr_auto_merge_desired: false,
        pr_auto_merge_method: DEFAULT_AGENT_WORKSPACE_PR_AUTO_MERGE_METHOD.to_string(),
        pr_auto_merge_current: None,
        pr_supervision_status: None,
        pr_supervision_summary: None,
        pr_supervision_updated_at: None,
        status: "active".to_string(),
        created_at: "2026-04-28T12:00:00+00:00".to_string(),
        updated_at: "2026-04-28T12:00:00+00:00".to_string(),
        mode_switch_locked: false,
        mode_switch_lock_reason: None,
        maintenance_operation: None,
        pr_autofix_fingerprint_spend: None,
    };
    let mut plan_branch = PlanBranch::new(
        ArtifactId::from_string("artifact-1"),
        IdeationSessionId::from_string("session-1"),
        ProjectId::from_string("project-1".to_string()),
        "agent-d619a9fd".to_string(),
        "feature/agent-screen".to_string(),
    );
    plan_branch.status = PlanBranchStatus::Active;
    plan_branch.pr_number = Some(90);
    plan_branch.pr_url = Some("https://github.com/mock/project/pull/90".to_string());
    plan_branch.pr_status = Some(PrStatus::Open);
    plan_branch.pr_push_status = PrPushStatus::Pushed;

    project_plan_branch_publication_into_workspace_response(&mut response, &plan_branch);

    assert_eq!(response.publication_pr_number, Some(90));
    assert_eq!(
        response.publication_pr_url.as_deref(),
        Some("https://github.com/mock/project/pull/90")
    );
    assert_eq!(response.publication_pr_status.as_deref(), Some("open"));
    assert_eq!(response.publication_push_status.as_deref(), Some("pushed"));

    response.publication_pr_status = None;
    plan_branch.status = PlanBranchStatus::Merged;
    project_plan_branch_publication_into_workspace_response(&mut response, &plan_branch);

    assert_eq!(response.publication_pr_status.as_deref(), Some("merged"));
}

#[test]
fn linked_plan_branch_publication_overrides_stale_workspace_publication_response() {
    let mut response = AgentConversationWorkspaceResponse {
        conversation_id: "conversation-1".to_string(),
        project_id: "project-1".to_string(),
        mode: AgentConversationWorkspaceMode::Ideation.to_string(),
        branch_mode: "isolated".to_string(),
        base_ref_kind: "project_default".to_string(),
        base_ref: "main".to_string(),
        base_display_name: Some("Project default (main)".to_string()),
        base_commit: None,
        branch_name: "agent-shell-branch".to_string(),
        worktree_path: "/tmp/workspace".to_string(),
        linked_ideation_session_id: Some("session-1".to_string()),
        task_pipeline_session_id: None,
        task_pipeline_available: false,
        linked_plan_branch_id: Some("plan-branch-1".to_string()),
        source_pull_request: None,
        publication_pr_number: Some(12),
        publication_pr_url: Some("https://github.com/mock/project/pull/12".to_string()),
        publication_pr_status: Some("open".to_string()),
        publication_push_status: Some("needs_agent".to_string()),
        auto_publish_enabled: true,
        auto_publish_initial_pr_enabled: false,
        auto_publish_paused_pr_autofix_enabled: None,
        auto_publish_paused_pr_auto_merge_desired: None,
        pr_autofix_enabled: false,
        pr_auto_merge_desired: false,
        pr_auto_merge_method: DEFAULT_AGENT_WORKSPACE_PR_AUTO_MERGE_METHOD.to_string(),
        pr_auto_merge_current: None,
        pr_supervision_status: None,
        pr_supervision_summary: None,
        pr_supervision_updated_at: None,
        status: "missing".to_string(),
        created_at: "2026-04-28T12:00:00+00:00".to_string(),
        updated_at: "2026-04-28T12:00:00+00:00".to_string(),
        mode_switch_locked: true,
        mode_switch_lock_reason: Some("Plan execution is still active".to_string()),
        maintenance_operation: None,
        pr_autofix_fingerprint_spend: None,
    };
    let mut plan_branch = PlanBranch::new(
        ArtifactId::from_string("artifact-1"),
        IdeationSessionId::from_string("session-1"),
        ProjectId::from_string("project-1".to_string()),
        "plan-branch".to_string(),
        "feature/agent-screen".to_string(),
    );
    plan_branch.pr_number = Some(90);
    plan_branch.pr_url = Some("https://github.com/mock/project/pull/90".to_string());
    plan_branch.pr_status = Some(PrStatus::Closed);
    plan_branch.pr_push_status = PrPushStatus::Pushed;

    project_plan_branch_publication_into_workspace_response(&mut response, &plan_branch);

    assert_eq!(response.publication_pr_number, Some(90));
    assert_eq!(
        response.publication_pr_url.as_deref(),
        Some("https://github.com/mock/project/pull/90")
    );
    assert_eq!(response.publication_pr_status.as_deref(), Some("closed"));
    assert_eq!(response.publication_push_status.as_deref(), Some("pushed"));
}

#[test]
fn publish_repair_message_uses_effective_target_branch_and_base() {
    let mut workspace = AgentConversationWorkspace::new(
        ChatConversationId::from_string("conversation-1"),
        ProjectId::from_string("project-1".to_string()),
        AgentConversationWorkspaceMode::Ideation,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        None,
        "agent-shell-branch".to_string(),
        "/tmp/agent-shell".to_string(),
    );
    workspace.linked_plan_branch_id = Some(PlanBranchId::from_string("plan-branch-1"));
    let target = AgentConversationWorkspaceRepairTarget {
        branch_name: "plan-branch".to_string(),
        base_ref: "feature/agent-screen".to_string(),
        base_display_name: Some("Current branch (feature/agent-screen)".to_string()),
        worktree_path: Some(PathBuf::from("/tmp/project-repo")),
    };

    let message = build_agent_workspace_publish_repair_message_for_target(
        "merge conflict",
        &workspace,
        &target,
    );

    assert!(message.contains("Workspace branch: plan-branch"));
    assert!(message.contains("Base: Current branch (feature/agent-screen)"));
    assert!(message.contains("Base ref: feature/agent-screen"));
    assert!(!message.contains("agent-shell-branch"));
    assert!(!message.contains("Project default (main)"));
}

#[test]
fn update_only_repair_action_metadata_is_preserved() {
    let workspace = AgentConversationWorkspace::new(
        ChatConversationId::from_string("conversation-1"),
        ProjectId::from_string("project-1".to_string()),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        None,
        "agent-branch".to_string(),
        "/tmp/agent-worktree".to_string(),
    );
    let target = AgentConversationWorkspaceRepairTarget {
        branch_name: "agent-branch".to_string(),
        base_ref: "main".to_string(),
        base_display_name: Some("Project default (main)".to_string()),
        worktree_path: Some(PathBuf::from("/tmp/agent-worktree")),
    };

    assert_eq!(
        AgentWorkspacePostRepairAction::Publish.classification(),
        "agent_fixable:publish"
    );
    assert_eq!(
        AgentWorkspacePostRepairAction::UpdateOnly.classification(),
        "agent_fixable:update_only"
    );
    assert_eq!(
        AgentWorkspacePostRepairAction::Publish.repair_requested_summary(),
        "Workspace agent repair requested before publishing can continue"
    );
    assert_eq!(
        AgentWorkspacePostRepairAction::UpdateOnly.repair_requested_summary(),
        "Workspace agent repair requested before the base update can complete"
    );
    assert_eq!(
        AgentWorkspacePostRepairAction::Publish.repair_sent_summary(),
        "Sent publish failure to workspace agent"
    );
    assert_eq!(
        AgentWorkspacePostRepairAction::UpdateOnly.repair_sent_summary(),
        "Sent base update failure to workspace agent"
    );
    assert_eq!(
        AgentWorkspacePostRepairAction::Publish.deferred_repair_sent_summary(),
        "Sent publish failure to workspace agent after active turn completed"
    );
    assert_eq!(
        AgentWorkspacePostRepairAction::UpdateOnly.deferred_repair_sent_summary(),
        "Sent base update failure to workspace agent after active turn completed"
    );
    assert_eq!(
        AgentWorkspacePostRepairAction::Publish.repair_send_failed_summary("unavailable"),
        "Failed to send publish failure to workspace agent: unavailable"
    );
    assert_eq!(
        AgentWorkspacePostRepairAction::UpdateOnly.repair_send_failed_summary("unavailable"),
        "Failed to send base update failure to workspace agent: unavailable"
    );
    assert_eq!(
        AgentWorkspacePostRepairAction::from_classification(Some("agent_fixable:publish")),
        Some(AgentWorkspacePostRepairAction::Publish)
    );
    assert_eq!(
        AgentWorkspacePostRepairAction::from_classification(Some("agent_fixable:update_only")),
        Some(AgentWorkspacePostRepairAction::UpdateOnly)
    );
    assert_eq!(
        AgentWorkspacePostRepairAction::from_classification(Some("agent_fixable:unknown")),
        None
    );
    assert_eq!(
        AgentWorkspacePostRepairAction::from_classification(None),
        None
    );

    let message = build_agent_workspace_repair_message_for_target(
        "merge conflict",
        &workspace,
        &target,
        AgentWorkspacePostRepairAction::UpdateOnly,
    );
    assert!(message.contains("Update from base failed for this agent workspace."));
    assert!(message.contains("Please fix the workspace so the base update can be completed."));

    let events = vec![
        AgentConversationWorkspacePublicationEvent::new(
            workspace.conversation_id,
            "repair_requested",
            "started",
            "publish repair",
            Some("agent_fixable:publish".to_string()),
        ),
        AgentConversationWorkspacePublicationEvent::new(
            workspace.conversation_id,
            "repair_requested",
            "started",
            "update repair",
            Some("agent_fixable:update_only".to_string()),
        ),
    ];
    assert_eq!(
        agent_workspace_post_repair_action_from_events(&events),
        AgentWorkspacePostRepairAction::UpdateOnly
    );
}

#[tokio::test]
async fn publish_repair_message_routes_spawn_to_effective_target_worktree() {
    let service = MockChatService::new();
    let workspace = AgentConversationWorkspace::new(
        ChatConversationId::from_string("conversation-1"),
        ProjectId::from_string("project-1".to_string()),
        AgentConversationWorkspaceMode::Ideation,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        None,
        "agent-shell-branch".to_string(),
        "/tmp/agent-shell".to_string(),
    );
    let target = AgentConversationWorkspaceRepairTarget {
        branch_name: "plan-branch".to_string(),
        base_ref: "feature/agent-screen".to_string(),
        base_display_name: Some("Current branch (feature/agent-screen)".to_string()),
        worktree_path: Some(PathBuf::from("/tmp/project-repo")),
    };

    send_agent_workspace_publish_repair_message_for_target(
        &service,
        &workspace,
        "merge conflict",
        AgentWorkspaceRepairRuntimeOverrides::default(),
        &target,
    )
    .await
    .expect("repair message should send");

    let options = service.get_sent_options().await;
    assert_eq!(options.len(), 1);
    assert_eq!(options[0].harness_override, None);
    assert_eq!(options[0].model_override, None);
    assert_eq!(options[0].logical_effort_override, None);
    assert_eq!(
        options[0].queue_policy,
        crate::application::chat_service::SendQueuePolicy::RequireImmediateStart
    );
    assert_eq!(
        options[0].working_directory_override.as_deref(),
        Some(Path::new("/tmp/project-repo"))
    );
}

#[tokio::test]
async fn repair_message_defers_only_when_app_handle_available_and_workspace_agent_running() {
    let workspace = AgentConversationWorkspace::new(
        ChatConversationId::from_string("conversation-1"),
        ProjectId::from_string("project-1".to_string()),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        None,
        "agent-branch".to_string(),
        "/tmp/agent-worktree".to_string(),
    );
    let registry = Arc::new(MemoryRunningAgentRegistry::new());
    registry
        .set_running(RunningAgentKey::new(
            ChatContextType::Project.to_string(),
            workspace.conversation_id.as_str(),
        ))
        .await;
    let registry_trait: Arc<dyn RunningAgentRegistry> = registry.clone();

    assert!(
        should_defer_agent_workspace_repair_message_for_registry(
            true,
            &registry_trait,
            None,
            &workspace
        )
        .await
    );
    assert!(
        !should_defer_agent_workspace_repair_message_for_registry(
            false,
            &registry_trait,
            None,
            &workspace
        )
        .await
    );
    let execution_state = Arc::new(ExecutionState::new());
    execution_state.mark_interactive_idle(&agent_workspace_interactive_slot_key(
        &workspace.conversation_id,
    ));
    assert!(
        !should_defer_agent_workspace_repair_message_for_registry(
            true,
            &registry_trait,
            Some(&execution_state),
            &workspace
        )
        .await
    );

    let idle_registry: Arc<dyn RunningAgentRegistry> = Arc::new(MemoryRunningAgentRegistry::new());
    assert!(
        !should_defer_agent_workspace_repair_message_for_registry(
            true,
            &idle_registry,
            None,
            &workspace
        )
        .await
    );
}

#[tokio::test]
async fn repair_wait_releases_when_ipr_is_idle_or_process_exited() {
    let state = AppState::new_test();
    let workspace = command_test_workspace();
    let key = RunningAgentKey::new(
        ChatContextType::Project.to_string(),
        workspace.conversation_id.as_str(),
    );
    let interactive_slot_key = agent_workspace_interactive_slot_key(&workspace.conversation_id);
    let execution_state = Arc::new(ExecutionState::new());

    assert!(
        agent_workspace_repair_wait_released(
            &state,
            Some(&execution_state),
            &key,
            &interactive_slot_key,
        )
        .await,
        "Codex-style process exit should release the deferred repair"
    );

    state
        .running_agent_registry
        .register(
            key.clone(),
            123,
            workspace.conversation_id.as_str(),
            "run-repair-wait".to_string(),
            None,
            None,
        )
        .await;

    assert!(
        !agent_workspace_repair_wait_released(
            &state,
            Some(&execution_state),
            &key,
            &interactive_slot_key,
        )
        .await,
        "active generation should keep the repair deferred"
    );

    execution_state.mark_interactive_idle(&interactive_slot_key);
    assert!(
        agent_workspace_repair_wait_released(
            &state,
            Some(&execution_state),
            &key,
            &interactive_slot_key,
        )
        .await,
        "Claude-style reusable idle process should release the deferred repair"
    );
}

#[tokio::test]
async fn fixable_publish_failure_routes_repair_and_records_events() {
    let state = AppState::new_test();
    let (_temp, workspace, target) = command_test_workspace_with_git_target();
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");
    let service = MockChatService::new();

    mark_agent_workspace_failure_with_routing_and_action(
        &state,
        &workspace,
        "merge conflict while updating from base",
        None,
        &service,
        true,
        &target,
        AgentWorkspacePostRepairAction::Publish,
        false,
    )
    .await;

    let messages = service.get_sent_messages().await;
    assert_eq!(messages.len(), 1);
    assert!(messages[0].contains("Commit & Publish failed for this agent workspace."));
    assert!(messages[0].contains("Workspace branch: ralphx/test/agent-command"));
    let claimed = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&workspace.conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        claimed.publication_push_status.as_deref(),
        Some("needs_agent")
    );
    assert_eq!(claimed.pr_supervision_status.as_deref(), Some("fixing"));

    let events = state
        .agent_conversation_workspace_repo
        .list_publication_events(&workspace.conversation_id)
        .await
        .expect("events should list");
    assert!(events.iter().any(|event| {
        event.step == "repair_requested"
            && event.status == "started"
            && event.classification.as_deref() == Some("agent_fixable:publish")
            && event.summary.contains("publishing can continue")
    }));
    assert!(events.iter().any(|event| {
        event.step == "repair_sent"
            && event.status == "succeeded"
            && event.summary == "Sent publish failure to workspace agent"
    }));
}

#[tokio::test]
async fn publish_failure_does_not_instruct_agent_when_current_repair_owns_head_redrive() {
    let state = AppState::new_test();
    let (_temp, workspace, target) = command_test_workspace_with_git_target();
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");
    let ready = seed_ready_command_repair_attempt(
        &state,
        &workspace,
        AgentWorkspaceRepairContinuation::Publish,
    )
    .await;
    let mut redrive = ready.clone();
    redrive.repair_head_commit = Some("validated-unpublished-repair-head".to_string());
    redrive
        .pending_reasons
        .push("pr_autofix_head_redrive:validated-unpublished-repair-head".to_string());
    redrive.updated_at += chrono::Duration::microseconds(1);
    let redrive = match state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt: redrive,
            expected_phase: AgentWorkspaceRepairPhase::Ready,
            expected_updated_at: ready.updated_at,
            next_phase: AgentWorkspaceRepairPhase::Ready,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("authorize durable head redrive")
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(attempt) => attempt,
        outcome => panic!("head-redrive checkpoint must apply, got {outcome:?}"),
    };
    state
        .agent_conversation_workspace_repo
        .claim_publish_lease(
            &workspace.conversation_id,
            &format!("publish-operation:{}", workspace.conversation_id),
            "failed-redrive-token",
            chrono::Utc::now(),
            None,
            false,
        )
        .await
        .expect("failed redrive lease should seed");
    let service = MockChatService::new();

    mark_agent_workspace_failure_with_routing_and_action(
        &state,
        &workspace,
        "push transport reported an interrupted publish",
        None,
        &service,
        true,
        &target,
        AgentWorkspacePostRepairAction::Publish,
        false,
    )
    .await;

    assert!(
        service.get_sent_messages().await.is_empty(),
        "the durable publisher owns this continuation, so no agent must be told to replay it"
    );
    let current = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&workspace.conversation_id)
        .await
        .expect("load current repair")
        .expect("current head redrive remains durable");
    assert_eq!(current.id, redrive.id);
    assert_eq!(current.generation, redrive.generation);
    assert_eq!(current.updated_at, redrive.updated_at);
    let settled_workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&workspace.conversation_id)
        .await
        .expect("load settled workspace")
        .expect("settled workspace exists");
    assert_eq!(settled_workspace.publish_lease_owner_run_id, None);
    assert_eq!(settled_workspace.publish_lease_token, None);
    assert_eq!(
        settled_workspace.publication_push_status.as_deref(),
        Some("needs_agent")
    );
    assert!(
        state
            .agent_conversation_workspace_repo
            .list_publication_events(&workspace.conversation_id)
            .await
            .expect("list publication events")
            .is_empty(),
        "suppression must not append a fake repair-delivery effect"
    );
}

#[tokio::test]
async fn fixable_update_failure_records_repair_send_failure() {
    let state = AppState::new_test();
    let workspace = command_test_workspace();
    let repository = tempfile::tempdir().expect("repair dispatch repository should exist");
    let repository_path = repository.path().join("repair-dispatch");
    setup_publish_repo(&repository_path);
    let target = AgentConversationWorkspaceRepairTarget {
        branch_name: workspace.branch_name.clone(),
        base_ref: workspace.base_ref.clone(),
        base_display_name: workspace.base_display_name.clone(),
        worktree_path: Some(repository_path),
    };
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");
    let service = MockChatService::new();
    service.set_available(false).await;

    mark_agent_workspace_failure_with_routing_and_action(
        &state,
        &workspace,
        "merge conflict while updating from base",
        None,
        &service,
        true,
        &target,
        AgentWorkspacePostRepairAction::UpdateOnly,
        false,
    )
    .await;

    let events = state
        .agent_conversation_workspace_repo
        .list_publication_events(&workspace.conversation_id)
        .await
        .expect("events should list");
    let settled = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&workspace.conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(settled.publication_push_status.as_deref(), Some("failed"));
    assert_eq!(settled.pr_supervision_status.as_deref(), Some("blocked"));
    let attempt = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&workspace.conversation_id)
        .await
        .expect("durable repair attempt should load")
        .expect("terminal delivery must leave its exact generation visible");
    assert_eq!(attempt.phase, AgentWorkspaceRepairPhase::Blocked);
    assert!(attempt.next_dispatch_at.is_none());
    assert!(
        attempt.reserved_agent_run_id.is_none(),
        "a terminal pre-start failure must clear only its untrusted run reservation"
    );
    assert!(attempt.git_common_dir.is_none());
    assert!(attempt.target_ref.is_none());
    assert!(attempt.target_lease_epoch.is_none());
    assert!(events.iter().any(|event| {
        event.step == "repair_requested"
            && event.classification.as_deref() == Some("agent_fixable:update_only")
            && event.summary.contains("base update can complete")
    }));
    assert!(events.iter().any(|event| {
        event.step == "repair_sent"
            && event.status == "failed"
            && event.summary.contains("Failed to send base update failure")
    }));
    assert_eq!(
        events
            .iter()
            .filter(|event| event.step == "repair_sent" && event.status == "failed")
            .count(),
        1,
        "terminal replay must not append another repair-delivery event"
    );
    assert_eq!(
        service.get_sent_messages().await.len(),
        1,
        "the failed immediate delivery must not start a duplicate repair worker"
    );
}

#[tokio::test]
async fn fixable_update_failure_retries_an_uncertain_immediate_repair_delivery_once() {
    let state = AppState::new_test();
    let workspace = command_test_workspace();
    let repository = tempfile::tempdir().expect("repair dispatch repository should exist");
    let repository_path = repository.path().join("uncertain-repair-dispatch");
    setup_publish_repo(&repository_path);
    let target = AgentConversationWorkspaceRepairTarget {
        branch_name: workspace.branch_name.clone(),
        base_ref: workspace.base_ref.clone(),
        base_display_name: workspace.base_display_name.clone(),
        worktree_path: Some(repository_path.clone()),
    };
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");
    let service = MockChatService::new();
    service.mismatch_next_send_result_identity().await;

    mark_agent_workspace_failure_with_routing_and_action(
        &state,
        &workspace,
        "merge conflict while updating from base",
        None,
        &service,
        true,
        &target,
        AgentWorkspacePostRepairAction::UpdateOnly,
        false,
    )
    .await;

    let scheduled = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&workspace.conversation_id)
        .await
        .expect("durable repair attempt should load")
        .expect("uncertain repair delivery must remain current for retry");
    assert_eq!(scheduled.phase, AgentWorkspaceRepairPhase::Requested);
    assert_eq!(scheduled.dispatch_count, 1);
    assert!(scheduled.next_dispatch_at.is_some());
    assert!(scheduled.reserved_agent_run_id.is_none());
    let identity = GitService::canonical_target_identity(&repository_path, &workspace.branch_name)
        .await
        .expect("resolve repair target identity");
    assert!(
        !state
            .branch_update_repo
            .get_target_lease(&identity)
            .await
            .expect("load retry lease")
            .expect("retry retains its exact repair lease")
            .is_released(),
        "delivery uncertainty must preserve the exact lease for its bounded retry"
    );
    let events = state
        .agent_conversation_workspace_repo
        .list_publication_events(&workspace.conversation_id)
        .await
        .expect("retry events should load");
    assert_eq!(
        events
            .iter()
            .filter(|event| event.step == "repair_sent" && event.status == "retrying")
            .count(),
        1
    );

    mark_agent_workspace_failure_with_routing_and_action(
        &state,
        &workspace,
        "merge conflict while updating from base",
        None,
        &service,
        true,
        &target,
        AgentWorkspacePostRepairAction::UpdateOnly,
        false,
    )
    .await;
    assert_eq!(service.get_sent_messages().await.len(), 1);
    assert_eq!(
        state
            .agent_conversation_workspace_repo
            .list_publication_events(&workspace.conversation_id)
            .await
            .expect("duplicate replay events should load"),
        events,
        "a current retry generation suppresses duplicate repair delivery and audit effects"
    );
}

#[tokio::test]
async fn live_base_update_and_publish_repair_paths_coalesce_without_stale_authority_overwrite() {
    let state = AppState::new_test();
    let (_temp, mut workspace, repair_target) = command_test_workspace_with_git_target();
    workspace.base_commit = Some("base-oid-before-join".to_string());
    let conversation_id = workspace.conversation_id.clone();
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");
    let service = MockChatService::new();

    mark_agent_workspace_update_failure_with_target(
        &state,
        &workspace,
        "merge conflict while updating from base",
        None,
        &service,
        &repair_target,
    )
    .await;

    let first = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("repair attempt should load")
        .expect("base update should create a durable repair attempt");
    let reserved_run_id = first
        .reserved_agent_run_id
        .clone()
        .expect("base update must reserve the exact repair run before dispatch");
    assert_eq!(first.generation, 1);
    assert_eq!(first.source, AgentWorkspaceRepairSource::BaseUpdate);
    assert_eq!(
        first.continuation,
        AgentWorkspaceRepairContinuation::UpdateOnly
    );
    assert_eq!(first.phase, AgentWorkspaceRepairPhase::Repairing);
    assert_eq!(first.target_base_ref, workspace.base_ref);
    assert_eq!(
        first.target_base_commit.as_deref(),
        Some("base-oid-before-join")
    );
    let messages_before_joins = service.get_sent_messages().await;
    let events_before_joins = state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("events should list");

    let mut publish_observation = workspace.clone();
    publish_observation.base_commit = Some("base-oid-stale-publish-observation".to_string());
    let publish_target =
        AgentConversationWorkspaceRepairTarget::from_workspace(&publish_observation);
    mark_agent_workspace_publish_failure_with_target(
        &state,
        &publish_observation,
        "merge conflict while updating from base",
        None,
        false,
        &service,
        &publish_target,
    )
    .await;
    let mut stale_observation = publish_observation;
    stale_observation.base_ref = "release/stale-observation".to_string();
    let stale_target = AgentConversationWorkspaceRepairTarget::from_workspace(&stale_observation);
    let ((), ()) = tokio::join!(
        mark_agent_workspace_update_failure_with_target(
            &state,
            &stale_observation,
            "merge conflict while updating from base",
            None,
            &service,
            &stale_target,
        ),
        mark_agent_workspace_update_failure_with_target(
            &state,
            &stale_observation,
            "merge conflict while updating from base",
            None,
            &service,
            &stale_target,
        ),
    );

    let current = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("repair attempt should load after joins")
        .expect("the original repair generation must remain current");
    assert_eq!(current.id, first.id);
    assert_eq!(current.generation, 1);
    assert_eq!(current.source, AgentWorkspaceRepairSource::BaseUpdate);
    assert_eq!(
        current.continuation,
        AgentWorkspaceRepairContinuation::Publish,
        "the stronger publish producer must upgrade the durable continuation"
    );
    assert_eq!(
        current.reserved_agent_run_id,
        Some(reserved_run_id),
        "stale joins must not replace the initial run reservation"
    );
    assert_eq!(current.target_base_ref, workspace.base_ref);
    assert_eq!(
        current.target_base_commit.as_deref(),
        Some("base-oid-before-join"),
        "a stale producer observation must not overwrite the durable base authority"
    );
    assert_eq!(
        service.get_sent_messages().await,
        messages_before_joins,
        "joined producers must not spawn another repair agent"
    );
    assert_eq!(
        state
            .agent_conversation_workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .expect("events should list after joins"),
        events_before_joins,
        "joined producers must not emit duplicate repair events"
    );
    assert!(state
        .agent_workspace_repair_repo
        .get_open_repair_effect(&first.id)
        .await
        .expect("repair effects should load")
        .is_none());
}

#[tokio::test]
async fn repair_request_event_failure_settles_without_dispatch() {
    let mut state = AppState::new_test();
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    state.agent_conversation_workspace_repo =
        workspace_repo.clone() as Arc<dyn AgentConversationWorkspaceRepository>;
    state.agent_workspace_repair_repo = workspace_repo.clone();
    let (_temp, workspace, target) = command_test_workspace_with_git_target();
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");
    workspace_repo.fail_next_matching_publication_event(
        "repair_requested",
        "started",
        "repair request event unavailable",
    );
    let service = MockChatService::new();

    mark_agent_workspace_failure_with_routing_and_action(
        &state,
        &workspace,
        "merge conflict while updating from base",
        None,
        &service,
        true,
        &target,
        AgentWorkspacePostRepairAction::Publish,
        false,
    )
    .await;

    assert!(service.get_sent_messages().await.is_empty());
    let settled = workspace_repo
        .get_by_conversation_id(&workspace.conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(settled.pr_supervision_status.as_deref(), Some("blocked"));
    assert_eq!(settled.publication_push_status.as_deref(), Some("failed"));
    let attempt = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&workspace.conversation_id)
        .await
        .unwrap()
        .expect("failed audit persistence should leave a blocked durable attempt");
    assert_eq!(attempt.phase, AgentWorkspaceRepairPhase::Blocked);
    assert!(attempt
        .blocker
        .as_deref()
        .unwrap_or_default()
        .contains("audit could not be persisted"));
    assert!(attempt.git_common_dir.is_none());
    assert!(attempt.target_ref.is_none());
    assert!(attempt.target_lease_epoch.is_none());
    assert!(workspace_repo
        .list_publication_events(&workspace.conversation_id)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn successful_dispatch_remains_completable_when_success_event_write_fails() {
    let mut state = AppState::new_test();
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    state.agent_conversation_workspace_repo =
        workspace_repo.clone() as Arc<dyn AgentConversationWorkspaceRepository>;
    state.agent_workspace_repair_repo = workspace_repo.clone();
    let (_temp, workspace, target) = command_test_workspace_with_git_target();
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");
    workspace_repo.fail_next_matching_publication_event(
        "repair_sent",
        "succeeded",
        "repair success event unavailable",
    );
    let service = MockChatService::with_agent_run_repo(Arc::clone(&state.agent_run_repo));

    mark_agent_workspace_failure_with_routing_and_action(
        &state,
        &workspace,
        "merge conflict while updating from base",
        None,
        &service,
        true,
        &target,
        AgentWorkspacePostRepairAction::Publish,
        false,
    )
    .await;

    assert_eq!(service.get_sent_messages().await.len(), 1);
    let current = workspace_repo
        .get_by_conversation_id(&workspace.conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(current.pr_supervision_status.as_deref(), Some("fixing"));
    let attempt = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&workspace.conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(attempt.phase, AgentWorkspaceRepairPhase::Repairing);
    let run_id = attempt
        .reserved_agent_run_id
        .as_ref()
        .expect("delivered repair should retain its run authority");
    assert!(matches!(
        crate::application::agent_workspace_publish_repair_state::classify_agent_workspace_repair_completion_authority(
            Arc::clone(&state.agent_workspace_repair_repo),
            &workspace.conversation_id,
            run_id,
        )
        .await
        .unwrap(),
        crate::domain::entities::AgentWorkspaceRepairCompletionAuthority::Current(_)
    ));
    assert!(!workspace_repo
        .list_publication_events(&workspace.conversation_id)
        .await
        .unwrap()
        .iter()
        .any(|event| event.step == "repair_sent" && event.status == "succeeded"));
}

#[tokio::test]
async fn pr_supervision_enable_marks_draft_ready_and_enables_auto_merge() {
    let mut state = AppState::new_test();
    let github = Arc::new(MockGithubService::new());
    let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
    state.github_service = Some(github_trait);

    let mut workspace = command_test_workspace();
    workspace.publication_pr_number = Some(251);
    workspace.publication_pr_url = Some("https://github.com/owner/repo/pull/251".to_string());
    workspace.publication_pr_status = Some("draft".to_string());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");

    let response = set_agent_conversation_workspace_pr_supervision_for_state(
        workspace.conversation_id.as_str(),
        AgentConversationWorkspacePrSupervisionInput {
            auto_fix_enabled: true,
            auto_merge_desired: true,
            auto_merge_method: Some(" ReBase ".to_string()),
        },
        &state,
    )
    .await
    .expect("PR supervision should enable");

    assert!(response.pr_autofix_enabled);
    assert!(response.pr_auto_merge_desired);
    assert_eq!(response.pr_auto_merge_method, "rebase");
    assert_eq!(response.pr_auto_merge_current, Some(true));
    assert_eq!(
        response.pr_supervision_status.as_deref(),
        Some("monitoring")
    );
    assert!(response
        .pr_supervision_summary
        .as_deref()
        .unwrap_or_default()
        .contains("auto-merge is enabled"));

    {
        let github_state = github.state();
        assert_eq!(github_state.mark_pr_ready_calls, 1);
        assert_eq!(github_state.last_mark_pr_ready_number, Some(251));
        assert_eq!(github_state.enable_pr_auto_merge_calls, 1);
        assert_eq!(
            github_state.last_enable_pr_auto_merge_args.as_ref(),
            Some(&(251, "rebase".to_string()))
        );
    }

    let events = state
        .agent_conversation_workspace_repo
        .list_publication_events(&workspace.conversation_id)
        .await
        .expect("events should list");
    assert!(events.iter().any(|event| {
        event.step == "pr_supervision"
            && event.status == "enabled"
            && event.classification.as_deref() == Some("pr_supervision_preferences")
    }));
}

#[tokio::test]
async fn pr_supervision_enable_uses_linked_plan_branch_pr_for_ideation_workspace() {
    let mut state = AppState::new_test();
    let github = Arc::new(MockGithubService::new());
    let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
    state.github_service = Some(github_trait);

    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_publish_repo(&repo_path);
    let plan_branch_name = "ralphx/test/plan-pr-supervision";
    git(&repo_path, &["checkout", "-b", plan_branch_name]);
    git(&repo_path, &["checkout", "main"]);

    let mut project = Project::new(
        "Plan PR supervision".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
    state
        .project_repo
        .create(project.clone())
        .await
        .expect("project should persist");

    let mut plan_branch = PlanBranch::new(
        ArtifactId::from_string("artifact-plan-pr-supervision"),
        IdeationSessionId::from_string("session-plan-pr-supervision"),
        project.id.clone(),
        plan_branch_name.to_string(),
        "main".to_string(),
    );
    plan_branch.status = PlanBranchStatus::Active;
    plan_branch.pr_number = Some(377);
    plan_branch.pr_url = Some("https://github.com/owner/repo/pull/377".to_string());
    plan_branch.pr_status = Some(PrStatus::Draft);
    plan_branch.pr_push_status = PrPushStatus::Pushed;
    let plan_branch_id = plan_branch.id.clone();
    let expected_plan_worktree =
        resolve_linked_plan_branch_agent_worktree_path(&project, &plan_branch)
            .expect("plan worktree path should resolve");
    state
        .plan_branch_repo
        .create(plan_branch)
        .await
        .expect("plan branch should persist");

    let mut workspace = command_test_workspace();
    workspace.project_id = project.id.clone();
    workspace.mode = AgentConversationWorkspaceMode::Ideation;
    workspace.linked_ideation_session_id = Some(IdeationSessionId::from_string(
        "session-plan-pr-supervision",
    ));
    workspace.linked_plan_branch_id = Some(plan_branch_id);
    workspace.publication_pr_number = None;
    workspace.publication_pr_url = None;
    workspace.publication_pr_status = None;
    workspace.publication_push_status = None;
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");

    let response = set_agent_conversation_workspace_pr_supervision_for_state(
        workspace.conversation_id.as_str(),
        AgentConversationWorkspacePrSupervisionInput {
            auto_fix_enabled: true,
            auto_merge_desired: true,
            auto_merge_method: Some("squash".to_string()),
        },
        &state,
    )
    .await
    .expect("linked plan branch PR supervision should enable");

    assert_eq!(response.publication_pr_number, Some(377));
    assert_eq!(
        response.publication_pr_url.as_deref(),
        Some("https://github.com/owner/repo/pull/377")
    );
    assert_eq!(response.publication_pr_status.as_deref(), Some("draft"));
    assert_eq!(response.publication_push_status.as_deref(), Some("pushed"));
    assert!(response.pr_autofix_enabled);
    assert!(response.pr_auto_merge_desired);
    assert_eq!(response.pr_auto_merge_current, Some(true));

    let persisted = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&workspace.conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert_eq!(persisted.publication_pr_number, Some(377));
    assert_eq!(git(&repo_path, &["branch", "--show-current"]), "main");
    assert_eq!(
        GitService::get_current_branch(&expected_plan_worktree)
            .await
            .expect("plan worktree branch should be readable"),
        plan_branch_name
    );

    let github_state = github.state();
    assert_eq!(github_state.mark_pr_ready_calls, 1);
    assert_eq!(github_state.last_mark_pr_ready_number, Some(377));
    assert_eq!(github_state.enable_pr_auto_merge_calls, 1);
    assert_eq!(
        github_state.last_enable_pr_auto_merge_args.as_ref(),
        Some(&(377, "squash".to_string()))
    );
}

#[tokio::test]
async fn pr_supervision_disable_uses_linked_plan_pr_without_ensuring_locked_plan_worktree() {
    let mut state = AppState::new_test();
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(command_test_pr_health(true)));
    let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
    state.github_service = Some(github_trait);

    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let repo_path_string = repo_path.to_string_lossy().to_string();
    let worktree_parent = temp.path().join("worktrees");
    setup_publish_repo(&repo_path);
    let plan_branch_name = "ralphx/test/plan-pr-disable";
    git(&repo_path, &["checkout", "-b", plan_branch_name]);
    git(&repo_path, &["checkout", "main"]);
    let other_worktree_path = temp.path().join("active-merge-worktree");
    let other_worktree_arg = other_worktree_path.to_string_lossy().to_string();
    git(
        &repo_path,
        &[
            "worktree",
            "add",
            other_worktree_arg.as_str(),
            plan_branch_name,
        ],
    );

    let mut project = Project::new("Plan PR disable".to_string(), repo_path_string.clone());
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
    state
        .project_repo
        .create(project.clone())
        .await
        .expect("project should persist");

    let mut plan_branch = PlanBranch::new(
        ArtifactId::from_string("artifact-plan-pr-disable"),
        IdeationSessionId::from_string("session-plan-pr-disable"),
        project.id.clone(),
        plan_branch_name.to_string(),
        "main".to_string(),
    );
    plan_branch.status = PlanBranchStatus::Active;
    plan_branch.pr_number = Some(630);
    plan_branch.pr_url = Some("https://github.com/owner/repo/pull/630".to_string());
    plan_branch.pr_status = Some(PrStatus::Open);
    plan_branch.pr_push_status = PrPushStatus::Pushed;
    let plan_branch_id = plan_branch.id.clone();
    let expected_plan_worktree =
        resolve_linked_plan_branch_agent_worktree_path(&project, &plan_branch)
            .expect("plan worktree path should resolve");
    state
        .plan_branch_repo
        .create(plan_branch)
        .await
        .expect("plan branch should persist");

    let mut workspace = command_test_workspace();
    workspace.project_id = project.id.clone();
    workspace.mode = AgentConversationWorkspaceMode::Ideation;
    workspace.linked_ideation_session_id =
        Some(IdeationSessionId::from_string("session-plan-pr-disable"));
    workspace.linked_plan_branch_id = Some(plan_branch_id);
    workspace.publication_pr_number = None;
    workspace.publication_pr_url = None;
    workspace.publication_pr_status = None;
    workspace.publication_push_status = None;
    workspace.pr_autofix_enabled = true;
    workspace.pr_auto_merge_desired = true;
    workspace.pr_auto_merge_current = Some(true);
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");

    let response = set_agent_conversation_workspace_pr_supervision_for_state(
        workspace.conversation_id.as_str(),
        AgentConversationWorkspacePrSupervisionInput {
            auto_fix_enabled: false,
            auto_merge_desired: false,
            auto_merge_method: None,
        },
        &state,
    )
    .await
    .expect("linked plan branch PR supervision should disable without ensuring worktree");

    assert!(!response.pr_autofix_enabled);
    assert!(!response.pr_auto_merge_desired);
    assert_eq!(response.pr_auto_merge_current, Some(false));
    assert_eq!(response.publication_pr_number, Some(630));
    assert_eq!(
        response.publication_pr_url.as_deref(),
        Some("https://github.com/owner/repo/pull/630")
    );
    assert_eq!(response.publication_pr_status.as_deref(), Some("open"));
    assert_eq!(response.publication_push_status.as_deref(), Some("pushed"));

    let persisted = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&workspace.conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert!(!persisted.pr_auto_merge_desired);
    assert_eq!(persisted.publication_pr_number, Some(630));
    assert!(!expected_plan_worktree.exists());
    assert_eq!(git(&repo_path, &["branch", "--show-current"]), "main");

    let github_state = github.state();
    assert_eq!(github_state.fetch_pr_health_calls, 1);
    assert_eq!(github_state.disable_pr_auto_merge_calls, 1);
    assert_eq!(github_state.last_disable_pr_auto_merge_number, Some(630));
    assert_eq!(
        github_state
            .last_disable_pr_auto_merge_working_dir
            .as_deref(),
        Some(repo_path_string.as_str())
    );
}

#[tokio::test]
async fn pr_supervision_enable_rejects_locked_linked_plan_worktree_before_persisting() {
    let mut state = AppState::new_test();
    let github = Arc::new(MockGithubService::new());
    let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
    state.github_service = Some(github_trait);

    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_publish_repo(&repo_path);
    let plan_branch_name = "ralphx/test/plan-pr-enable-locked";
    git(&repo_path, &["checkout", "-b", plan_branch_name]);
    git(&repo_path, &["checkout", "main"]);
    let other_worktree_path = temp.path().join("active-merge-worktree");
    let other_worktree_arg = other_worktree_path.to_string_lossy().to_string();
    git(
        &repo_path,
        &[
            "worktree",
            "add",
            other_worktree_arg.as_str(),
            plan_branch_name,
        ],
    );

    let mut project = Project::new(
        "Plan PR enable locked".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
    state
        .project_repo
        .create(project.clone())
        .await
        .expect("project should persist");

    let mut plan_branch = PlanBranch::new(
        ArtifactId::from_string("artifact-plan-pr-enable-locked"),
        IdeationSessionId::from_string("session-plan-pr-enable-locked"),
        project.id.clone(),
        plan_branch_name.to_string(),
        "main".to_string(),
    );
    plan_branch.status = PlanBranchStatus::Active;
    plan_branch.pr_number = Some(631);
    plan_branch.pr_url = Some("https://github.com/owner/repo/pull/631".to_string());
    plan_branch.pr_status = Some(PrStatus::Open);
    plan_branch.pr_push_status = PrPushStatus::Pushed;
    let plan_branch_id = plan_branch.id.clone();
    let expected_plan_worktree =
        resolve_linked_plan_branch_agent_worktree_path(&project, &plan_branch)
            .expect("plan worktree path should resolve");
    state
        .plan_branch_repo
        .create(plan_branch)
        .await
        .expect("plan branch should persist");

    let mut workspace = command_test_workspace();
    workspace.project_id = project.id.clone();
    workspace.mode = AgentConversationWorkspaceMode::Ideation;
    workspace.linked_ideation_session_id = Some(IdeationSessionId::from_string(
        "session-plan-pr-enable-locked",
    ));
    workspace.linked_plan_branch_id = Some(plan_branch_id);
    workspace.publication_pr_number = None;
    workspace.publication_pr_url = None;
    workspace.publication_pr_status = None;
    workspace.publication_push_status = None;
    workspace.pr_autofix_enabled = false;
    workspace.pr_auto_merge_desired = false;
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");

    let error = set_agent_conversation_workspace_pr_supervision_for_state(
        workspace.conversation_id.as_str(),
        AgentConversationWorkspacePrSupervisionInput {
            auto_fix_enabled: true,
            auto_merge_desired: true,
            auto_merge_method: Some("squash".to_string()),
        },
        &state,
    )
    .await
    .expect_err("locked linked plan branch should reject enable");

    assert!(error.contains("already checked out at"));
    assert!(error.contains("refusing to move or delete another worktree"));

    let persisted = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&workspace.conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert!(!persisted.pr_autofix_enabled);
    assert!(!persisted.pr_auto_merge_desired);
    assert_eq!(persisted.publication_pr_number, None);
    assert!(!expected_plan_worktree.exists());

    let github_state = github.state();
    assert_eq!(github_state.enable_pr_auto_merge_calls, 0);
    assert_eq!(github_state.mark_pr_ready_calls, 0);
}

#[tokio::test]
async fn pr_supervision_enable_records_waiting_when_auto_merge_enable_fails() {
    let mut state = AppState::new_test();
    let github = Arc::new(MockGithubService::new());
    github.state().enable_pr_auto_merge_result = Some(Err(AppError::Infrastructure(
        "GitHub auto-merge is not ready".to_string(),
    )));
    let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
    state.github_service = Some(github_trait);

    let mut workspace = command_test_workspace();
    workspace.publication_pr_number = Some(254);
    workspace.publication_pr_url = Some("https://github.com/owner/repo/pull/254".to_string());
    workspace.publication_pr_status = Some("open".to_string());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");

    let response = set_agent_conversation_workspace_pr_supervision_for_state(
        workspace.conversation_id.as_str(),
        AgentConversationWorkspacePrSupervisionInput {
            auto_fix_enabled: true,
            auto_merge_desired: true,
            auto_merge_method: Some("squash".to_string()),
        },
        &state,
    )
    .await
    .expect("PR supervision should persist even when GitHub auto-merge waits");

    assert!(response.pr_autofix_enabled);
    assert!(response.pr_auto_merge_desired);
    assert_eq!(response.pr_auto_merge_current, Some(false));
    assert_eq!(response.pr_supervision_status.as_deref(), Some("waiting"));
    assert!(response
        .pr_supervision_summary
        .as_deref()
        .unwrap_or_default()
        .contains("could not be enabled yet"));

    {
        let github_state = github.state();
        assert_eq!(github_state.enable_pr_auto_merge_calls, 1);
    }

    let events = state
        .agent_conversation_workspace_repo
        .list_publication_events(&workspace.conversation_id)
        .await
        .expect("events should list");
    assert!(events.iter().any(|event| {
        event.step == "pr_supervision"
            && event.status == "enabled"
            && event
                .summary
                .contains("request GitHub auto-merge when possible")
    }));
}

async fn persist_command_test_auto_merge_guard(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    pr_number: i64,
) {
    let mut monitor = AgentWorkspaceReviewMonitor::new(
        workspace.conversation_id.clone(),
        workspace.project_id.clone(),
    );
    monitor.auto_merge_guard = Some(AgentWorkspaceReviewAutoMergeGuard {
        status: AgentWorkspaceReviewAutoMergeGuardStatus::PausedForReview,
        pr_number,
        merge_method: "squash".to_string(),
        target_scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        diff_fingerprint: "guarded-diff".to_string(),
        head_sha: Some("guarded-head".to_string()),
        last_error: None,
    });
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("workspace Review monitor should persist");
}

#[tokio::test]
async fn pr_supervision_guarded_enable_is_idempotent_when_remote_auto_merge_is_absent() {
    let mut state = AppState::new_test();
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(command_test_pr_health(false)));
    let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
    state.github_service = Some(github_trait);

    let mut workspace = command_test_workspace();
    workspace.publication_pr_number = Some(255);
    workspace.publication_pr_status = Some("open".to_string());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");
    persist_command_test_auto_merge_guard(&state, &workspace, 255).await;

    let response = set_agent_conversation_workspace_pr_supervision_for_state(
        workspace.conversation_id.as_str(),
        AgentConversationWorkspacePrSupervisionInput {
            auto_fix_enabled: true,
            auto_merge_desired: true,
            auto_merge_method: Some("squash".to_string()),
        },
        &state,
    )
    .await
    .expect("guarded PR supervision should preserve the desired preference");

    assert!(response.pr_auto_merge_desired);
    assert_eq!(response.pr_auto_merge_current, Some(false));
    assert_eq!(
        response.pr_supervision_status.as_deref(),
        Some("review_paused")
    );
    let github_state = github.state();
    assert_eq!(github_state.fetch_pr_health_calls, 1);
    assert_eq!(github_state.disable_pr_auto_merge_calls, 0);
    assert_eq!(github_state.enable_pr_auto_merge_calls, 0);
}

#[tokio::test]
async fn pr_supervision_guarded_enable_disables_active_remote_auto_merge_once() {
    let mut state = AppState::new_test();
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(command_test_pr_health(true)));
    let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
    state.github_service = Some(github_trait);

    let mut workspace = command_test_workspace();
    workspace.publication_pr_number = Some(256);
    workspace.publication_pr_status = Some("open".to_string());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");
    persist_command_test_auto_merge_guard(&state, &workspace, 256).await;

    let response = set_agent_conversation_workspace_pr_supervision_for_state(
        workspace.conversation_id.as_str(),
        AgentConversationWorkspacePrSupervisionInput {
            auto_fix_enabled: true,
            auto_merge_desired: true,
            auto_merge_method: Some("squash".to_string()),
        },
        &state,
    )
    .await
    .expect("guarded PR supervision should pause active remote auto-merge");

    assert!(response.pr_auto_merge_desired);
    assert_eq!(response.pr_auto_merge_current, Some(false));
    assert_eq!(
        response.pr_supervision_status.as_deref(),
        Some("review_paused")
    );
    let github_state = github.state();
    assert_eq!(github_state.fetch_pr_health_calls, 1);
    assert_eq!(github_state.disable_pr_auto_merge_calls, 1);
    assert_eq!(github_state.last_disable_pr_auto_merge_number, Some(256));
    assert_eq!(github_state.enable_pr_auto_merge_calls, 0);
}

#[tokio::test]
async fn pr_supervision_disable_turns_off_existing_auto_merge() {
    let mut state = AppState::new_test();
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(command_test_pr_health(true)));
    let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
    state.github_service = Some(github_trait);

    let mut workspace = command_test_workspace();
    workspace.publication_pr_number = Some(252);
    workspace.publication_pr_status = Some("open".to_string());
    workspace.pr_autofix_enabled = true;
    workspace.pr_auto_merge_desired = true;
    workspace.pr_auto_merge_current = Some(true);
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");

    let response = set_agent_conversation_workspace_pr_supervision_for_state(
        workspace.conversation_id.as_str(),
        AgentConversationWorkspacePrSupervisionInput {
            auto_fix_enabled: false,
            auto_merge_desired: false,
            auto_merge_method: None,
        },
        &state,
    )
    .await
    .expect("PR supervision should disable");

    assert!(!response.pr_autofix_enabled);
    assert!(!response.pr_auto_merge_desired);
    assert_eq!(
        response.pr_auto_merge_method,
        DEFAULT_AGENT_WORKSPACE_PR_AUTO_MERGE_METHOD
    );
    assert_eq!(response.pr_auto_merge_current, Some(false));
    assert_eq!(response.pr_supervision_status.as_deref(), Some("disabled"));
    assert!(response
        .pr_supervision_summary
        .as_deref()
        .unwrap_or_default()
        .contains("auto-merge is disabled"));

    {
        let github_state = github.state();
        assert_eq!(github_state.fetch_pr_health_calls, 1);
        assert_eq!(github_state.disable_pr_auto_merge_calls, 1);
        assert_eq!(github_state.last_disable_pr_auto_merge_number, Some(252));
    }

    let events = state
        .agent_conversation_workspace_repo
        .list_publication_events(&workspace.conversation_id)
        .await
        .expect("events should list");
    assert!(events.iter().any(|event| {
        event.step == "pr_supervision"
            && event.status == "disabled"
            && event.summary == "RalphX PR supervision is disabled."
    }));
}

#[tokio::test]
async fn pr_supervision_disable_treats_absent_remote_auto_merge_as_idempotent() {
    let mut state = AppState::new_test();
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(command_test_pr_health(false)));
    let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
    state.github_service = Some(github_trait);

    let mut workspace = command_test_workspace();
    workspace.publication_pr_number = Some(253);
    workspace.publication_pr_status = Some("open".to_string());
    workspace.pr_autofix_enabled = true;
    workspace.pr_auto_merge_desired = true;
    workspace.pr_auto_merge_current = Some(true);
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");

    let response = set_agent_conversation_workspace_pr_supervision_for_state(
        workspace.conversation_id.as_str(),
        AgentConversationWorkspacePrSupervisionInput {
            auto_fix_enabled: false,
            auto_merge_desired: false,
            auto_merge_method: None,
        },
        &state,
    )
    .await
    .expect("PR supervision should disable idempotently when GitHub auto-merge is absent");

    assert!(!response.pr_autofix_enabled);
    assert!(!response.pr_auto_merge_desired);
    assert_eq!(response.pr_auto_merge_current, Some(false));
    assert_eq!(response.pr_supervision_status.as_deref(), Some("disabled"));

    let persisted = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&workspace.conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert!(!persisted.pr_auto_merge_desired);
    assert_eq!(persisted.pr_auto_merge_current, Some(false));

    {
        let github_state = github.state();
        assert_eq!(github_state.fetch_pr_health_calls, 1);
        assert_eq!(github_state.disable_pr_auto_merge_calls, 0);
    }

    let events = state
        .agent_conversation_workspace_repo
        .list_publication_events(&workspace.conversation_id)
        .await
        .expect("events should list");
    assert!(events.iter().any(|event| {
        event.step == "pr_supervision"
            && event.status == "disabled"
            && event.summary == "RalphX PR supervision is disabled."
    }));
}

#[tokio::test]
async fn pr_supervision_disable_records_waiting_when_auto_merge_disable_fails() {
    let mut state = AppState::new_test();
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(command_test_pr_health(true)));
    github.state().disable_pr_auto_merge_result = Some(Err(AppError::Infrastructure(
        "GitHub auto-merge cannot be disabled yet".to_string(),
    )));
    let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
    state.github_service = Some(github_trait);

    let mut workspace = command_test_workspace();
    workspace.publication_pr_number = Some(255);
    workspace.publication_pr_status = Some("open".to_string());
    workspace.pr_autofix_enabled = true;
    workspace.pr_auto_merge_desired = true;
    workspace.pr_auto_merge_current = Some(true);
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");

    let response = set_agent_conversation_workspace_pr_supervision_for_state(
        workspace.conversation_id.as_str(),
        AgentConversationWorkspacePrSupervisionInput {
            auto_fix_enabled: false,
            auto_merge_desired: false,
            auto_merge_method: None,
        },
        &state,
    )
    .await
    .expect("PR supervision preference should persist even when GitHub disable waits");

    assert!(!response.pr_autofix_enabled);
    assert!(!response.pr_auto_merge_desired);
    assert_eq!(response.pr_auto_merge_current, Some(true));
    assert_eq!(response.pr_supervision_status.as_deref(), Some("waiting"));
    assert!(response
        .pr_supervision_summary
        .as_deref()
        .unwrap_or_default()
        .contains("could not be disabled yet"));

    {
        let github_state = github.state();
        assert_eq!(github_state.fetch_pr_health_calls, 1);
        assert_eq!(github_state.disable_pr_auto_merge_calls, 1);
    }

    let events = state
        .agent_conversation_workspace_repo
        .list_publication_events(&workspace.conversation_id)
        .await
        .expect("events should list");
    assert!(events.iter().any(|event| {
        event.step == "pr_supervision"
            && event.status == "disabled"
            && event.summary == "RalphX PR supervision is disabled."
    }));
}

#[tokio::test]
async fn auto_publish_pause_disables_and_restores_pr_supervision_preferences() {
    let state = AppState::new_test();
    let mut workspace = command_test_workspace();
    workspace.publication_pr_number = Some(256);
    workspace.publication_pr_status = Some("open".to_string());
    workspace.pr_autofix_enabled = true;
    workspace.pr_auto_merge_desired = true;
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");

    let paused = set_agent_conversation_workspace_auto_publish_for_state(
        workspace.conversation_id.as_str(),
        AgentConversationWorkspaceAutoPublishInput {
            auto_publish_enabled: false,
        },
        &state,
    )
    .await
    .expect("Auto Publish should pause");

    assert!(!paused.auto_publish_enabled);
    assert_eq!(paused.auto_publish_paused_pr_autofix_enabled, Some(true));
    assert_eq!(paused.auto_publish_paused_pr_auto_merge_desired, Some(true));
    assert!(!paused.pr_autofix_enabled);
    assert!(!paused.pr_auto_merge_desired);
    assert_eq!(paused.pr_supervision_status.as_deref(), Some("paused"));

    let resumed = set_agent_conversation_workspace_auto_publish_for_state(
        workspace.conversation_id.as_str(),
        AgentConversationWorkspaceAutoPublishInput {
            auto_publish_enabled: true,
        },
        &state,
    )
    .await
    .expect("Auto Publish should resume");

    assert!(resumed.auto_publish_enabled);
    assert_eq!(resumed.auto_publish_paused_pr_autofix_enabled, None);
    assert_eq!(resumed.auto_publish_paused_pr_auto_merge_desired, None);
    assert!(resumed.pr_autofix_enabled);
    assert!(resumed.pr_auto_merge_desired);
    assert_eq!(resumed.pr_supervision_status.as_deref(), Some("monitoring"));

    let events = state
        .agent_conversation_workspace_repo
        .list_publication_events(&workspace.conversation_id)
        .await
        .expect("events should list");
    assert!(events.iter().any(|event| {
        event.step == "auto_publish"
            && event.status == "disabled"
            && event.classification.as_deref() == Some("auto_publish_preferences")
    }));
    assert!(events
        .iter()
        .any(|event| event.step == "auto_publish" && event.status == "enabled"));
}

#[tokio::test]
async fn agent_workspace_automation_preferences_remain_mutable_during_repair() {
    let mut state = AppState::new_test();
    let github = Arc::new(MockGithubService::new());
    let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
    state.github_service = Some(github_trait);

    let mut workspace = command_test_workspace();
    workspace.publication_pr_number = Some(257);
    workspace.publication_pr_url = Some("https://github.com/owner/repo/pull/257".to_string());
    workspace.publication_pr_status = Some("open".to_string());
    workspace.publication_push_status = Some("needs_agent".to_string());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("repair workspace should persist");

    let supervised = set_agent_conversation_workspace_pr_supervision_for_state(
        workspace.conversation_id.as_str(),
        AgentConversationWorkspacePrSupervisionInput {
            auto_fix_enabled: true,
            auto_merge_desired: true,
            auto_merge_method: Some("squash".to_string()),
        },
        &state,
    )
    .await
    .expect("PR supervision should remain configurable during repair");

    assert!(supervised.pr_autofix_enabled);
    assert!(supervised.pr_auto_merge_desired);
    assert_eq!(
        supervised.publication_push_status.as_deref(),
        Some("needs_agent")
    );

    let paused = set_agent_conversation_workspace_auto_publish_for_state(
        workspace.conversation_id.as_str(),
        AgentConversationWorkspaceAutoPublishInput {
            auto_publish_enabled: false,
        },
        &state,
    )
    .await
    .expect("Auto Publish should pause during repair");

    assert!(!paused.auto_publish_enabled);
    assert_eq!(paused.auto_publish_paused_pr_autofix_enabled, Some(true));
    assert_eq!(paused.auto_publish_paused_pr_auto_merge_desired, Some(true));
    assert!(!paused.pr_autofix_enabled);
    assert!(!paused.pr_auto_merge_desired);
    assert_eq!(paused.pr_supervision_status.as_deref(), Some("paused"));
    assert_eq!(
        paused.publication_push_status.as_deref(),
        Some("needs_agent")
    );

    let resumed = set_agent_conversation_workspace_auto_publish_for_state(
        workspace.conversation_id.as_str(),
        AgentConversationWorkspaceAutoPublishInput {
            auto_publish_enabled: true,
        },
        &state,
    )
    .await
    .expect("Auto Publish should resume during repair");

    assert!(resumed.auto_publish_enabled);
    assert_eq!(resumed.auto_publish_paused_pr_autofix_enabled, None);
    assert_eq!(resumed.auto_publish_paused_pr_auto_merge_desired, None);
    assert!(resumed.pr_autofix_enabled);
    assert!(resumed.pr_auto_merge_desired);
    assert_eq!(resumed.pr_supervision_status.as_deref(), Some("monitoring"));
    assert_eq!(
        resumed.publication_push_status.as_deref(),
        Some("needs_agent")
    );

    let stored = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&workspace.conversation_id)
        .await
        .expect("repair workspace lookup should succeed")
        .expect("repair workspace should remain persisted");
    assert!(stored.auto_publish_enabled);
    assert_eq!(stored.auto_publish_paused_pr_autofix_enabled, None);
    assert_eq!(stored.auto_publish_paused_pr_auto_merge_desired, None);
    assert!(stored.pr_autofix_enabled);
    assert!(stored.pr_auto_merge_desired);
    assert_eq!(
        stored.publication_push_status.as_deref(),
        Some("needs_agent")
    );

    let github_state = github.state();
    assert_eq!(github_state.enable_pr_auto_merge_calls, 2);
    assert_eq!(github_state.fetch_pr_health_calls, 1);
    assert_eq!(github_state.disable_pr_auto_merge_calls, 0);
}

#[tokio::test]
async fn auto_publish_enable_before_pr_sets_initial_pr_opt_in() {
    let state = AppState::new_test();
    let workspace = command_test_workspace();
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");

    let updated = set_agent_conversation_workspace_auto_publish_for_state(
        workspace.conversation_id.as_str(),
        AgentConversationWorkspaceAutoPublishInput {
            auto_publish_enabled: true,
        },
        &state,
    )
    .await
    .expect("Auto Publish should enable before PR publication");

    assert!(updated.auto_publish_enabled);
    assert!(updated.auto_publish_initial_pr_enabled);

    let events = state
        .agent_conversation_workspace_repo
        .list_publication_events(&workspace.conversation_id)
        .await
        .expect("events should list");
    assert!(events.iter().any(|event| {
        event.step == "auto_publish"
            && event.status == "enabled"
            && event.summary == "Auto Publish is enabled for the first pull request."
    }));
}

#[tokio::test]
async fn auto_publish_enable_leaves_ready_manual_and_update_only_repairs_parked() {
    for continuation in [
        AgentWorkspaceRepairContinuation::Manual,
        AgentWorkspaceRepairContinuation::UpdateOnly,
    ] {
        let state = AppState::new_test();
        let workspace = command_test_workspace();
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("workspace should persist");
        let ready =
            seed_ready_command_repair_attempt(&state, &workspace, continuation.clone()).await;

        let response = set_agent_conversation_workspace_auto_publish_for_state(
            workspace.conversation_id.as_str(),
            AgentConversationWorkspaceAutoPublishInput {
                auto_publish_enabled: true,
            },
            &state,
        )
        .await
        .expect("Auto Publish preference should persist without promoting a non-Publish repair");

        assert!(response.auto_publish_initial_pr_enabled);
        let after = state
            .agent_workspace_repair_repo
            .get_current_repair_attempt(&workspace.conversation_id)
            .await
            .expect("read parked repair attempt")
            .expect("parked repair must remain current");
        assert_eq!(after.id, ready.id);
        assert_eq!(after.phase, AgentWorkspaceRepairPhase::Ready);
        assert_eq!(after.continuation, continuation);
    }
}

#[tokio::test]
async fn existing_pr_auto_publish_enable_keeps_ready_projection_when_handoff_cannot_acquire_target()
{
    let state = AppState::new_test();
    let mut workspace = command_test_workspace();
    workspace.publication_pr_number = Some(442);
    workspace.publication_pr_status = Some("open".to_string());
    workspace.auto_publish_enabled = false;
    let mut project = Project::new(
        "Auto Publish existing PR command test".to_string(),
        "/tmp/agent-workspace-auto-publish-existing-pr".to_string(),
    );
    project.id = workspace.project_id.clone();
    state
        .project_repo
        .create(project)
        .await
        .expect("unrelated project seed should succeed");
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");
    let ready = seed_ready_command_repair_attempt(
        &state,
        &workspace,
        AgentWorkspaceRepairContinuation::Publish,
    )
    .await;

    let response = set_agent_conversation_workspace_auto_publish_for_state(
        workspace.conversation_id.as_str(),
        AgentConversationWorkspaceAutoPublishInput {
            auto_publish_enabled: true,
        },
        &state,
    )
    .await
    .expect("preference enable should return the durable Ready projection");

    assert!(response.auto_publish_enabled);
    let ready_after_failed_handoff = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&workspace.conversation_id)
        .await
        .expect("read repair attempt after failed handoff")
        .expect("Ready repair must remain current");
    assert_eq!(ready_after_failed_handoff.id, ready.id);
    assert_eq!(
        ready_after_failed_handoff.phase,
        AgentWorkspaceRepairPhase::Ready
    );
    assert_eq!(
        response
            .maintenance_operation
            .as_ref()
            .map(|operation| operation.status),
        Some(crate::domain::entities::AgentWorkspaceRepairOperationStatus::Ready),
        "response must not report a successful continuation after target reacquisition fails"
    );

    let event_count = state
        .agent_conversation_workspace_repo
        .list_publication_events(&workspace.conversation_id)
        .await
        .expect("list events after blocked continuation")
        .len();
    let replay = set_agent_conversation_workspace_auto_publish_for_state(
        workspace.conversation_id.as_str(),
        AgentConversationWorkspaceAutoPublishInput {
            auto_publish_enabled: true,
        },
        &state,
    )
    .await
    .expect("already-enabled preference should not retry the Ready continuation");
    assert_eq!(
        replay
            .maintenance_operation
            .as_ref()
            .map(|operation| operation.status),
        Some(crate::domain::entities::AgentWorkspaceRepairOperationStatus::Ready)
    );
    assert_eq!(
        event_count,
        state
            .agent_conversation_workspace_repo
            .list_publication_events(&workspace.conversation_id)
            .await
            .expect("list events after replay")
            .len(),
        "repeated enable must not append another preference or continuation event"
    );
}

#[tokio::test]
async fn pr_supervision_rejects_enable_when_auto_publish_is_paused() {
    let state = AppState::new_test();
    let mut workspace = command_test_workspace();
    workspace.publication_pr_number = Some(257);
    workspace.publication_pr_status = Some("open".to_string());
    workspace.auto_publish_enabled = false;
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");

    let error = set_agent_conversation_workspace_pr_supervision_for_state(
        workspace.conversation_id.as_str(),
        AgentConversationWorkspacePrSupervisionInput {
            auto_fix_enabled: true,
            auto_merge_desired: false,
            auto_merge_method: Some("squash".to_string()),
        },
        &state,
    )
    .await
    .expect_err("PR supervision enable should be rejected while paused");

    assert!(error.contains("Auto Publish is paused"));
}

#[tokio::test]
async fn pr_supervision_rejects_terminal_pr_and_invalid_merge_method() {
    let state = AppState::new_test();
    let mut workspace = command_test_workspace();
    workspace.publication_pr_number = Some(253);
    workspace.publication_pr_status = Some("merged".to_string());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");

    let terminal_error = set_agent_conversation_workspace_pr_supervision_for_state(
        workspace.conversation_id.as_str(),
        AgentConversationWorkspacePrSupervisionInput {
            auto_fix_enabled: true,
            auto_merge_desired: false,
            auto_merge_method: Some("squash".to_string()),
        },
        &state,
    )
    .await
    .expect_err("terminal PR supervision should be rejected");
    assert!(terminal_error.contains("closed or merged PR"));

    let method_error = set_agent_conversation_workspace_pr_supervision_for_state(
        workspace.conversation_id.as_str(),
        AgentConversationWorkspacePrSupervisionInput {
            auto_fix_enabled: true,
            auto_merge_desired: true,
            auto_merge_method: Some("octopus".to_string()),
        },
        &state,
    )
    .await
    .expect_err("invalid auto-merge method should be rejected before workspace load");
    assert!(method_error.contains("Unsupported auto-merge method"));
}

#[tokio::test]
async fn deferred_repair_spawn_without_app_handle_noops() {
    let state = AppState::new_test();
    let workspace = command_test_workspace();
    let target = AgentConversationWorkspaceRepairTarget::from_workspace(&workspace);

    spawn_deferred_agent_workspace_repair_message(
        &state,
        workspace.clone(),
        "merge conflict while updating from base".to_string(),
        AgentWorkspaceRepairRuntimeOverrides::default(),
        target,
        AgentWorkspacePostRepairAction::Publish,
        None,
        None,
    )
    .await;

    let events = state
        .agent_conversation_workspace_repo
        .list_publication_events(&workspace.conversation_id)
        .await
        .expect("events should list");
    assert!(events.is_empty());
}

fn retargeted_base_resolution() -> BaseResolutionResult {
    BaseResolutionResult {
        status: BaseStatus::Retargeted,
        old_base_ref: "feature/deleted-base".to_string(),
        effective_base_ref: Some("main".to_string()),
        effective_checkout_ref: Some("origin/main".to_string()),
        effective_base_commit: Some("main-sha".to_string()),
        display_name: Some("Project default (main)".to_string()),
        block_reason: None,
        merged_source_pull_request_number: None,
    }
}

fn blocked_base_resolution(reason: &str) -> BaseResolutionResult {
    BaseResolutionResult {
        status: BaseStatus::Blocked,
        old_base_ref: "feature/deleted-base".to_string(),
        effective_base_ref: None,
        effective_checkout_ref: None,
        effective_base_commit: None,
        display_name: None,
        block_reason: Some(reason.to_string()),
        merged_source_pull_request_number: None,
    }
}

#[test]
fn normalize_explicit_publish_base_selection_trims_defaults_and_rejects_prs() {
    assert!(
        normalize_explicit_publish_base_selection(AgentConversationWorkspaceBaseSelection {
            kind: None,
            branch_mode: None,
            base_ref: Some("  ".to_string()),
            display_name: Some("ignored".to_string()),
            source_pull_request: None,
        })
        .expect("blank base ref should be allowed as no explicit selection")
        .is_none()
    );

    let local =
        normalize_explicit_publish_base_selection(AgentConversationWorkspaceBaseSelection {
            kind: None,
            branch_mode: None,
            base_ref: Some("  release/0.8  ".to_string()),
            display_name: None,
            source_pull_request: None,
        })
        .expect("local branch should normalize")
        .expect("local branch should produce a selection");
    assert_eq!(local.kind, IdeationAnalysisBaseRefKind::LocalBranch);
    assert_eq!(local.base_ref, "release/0.8");
    assert_eq!(local.display_name, "release/0.8");

    let project =
        normalize_explicit_publish_base_selection(AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
            branch_mode: None,
            base_ref: Some("main".to_string()),
            display_name: Some("  ".to_string()),
            source_pull_request: None,
        })
        .expect("project default should normalize")
        .expect("project default should produce a selection");
    assert_eq!(project.display_name, "Project default (main)");

    let current =
        normalize_explicit_publish_base_selection(AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::CurrentBranch),
            branch_mode: None,
            base_ref: Some("feature/base".to_string()),
            display_name: None,
            source_pull_request: None,
        })
        .expect("current branch should normalize")
        .expect("current branch should produce a selection");
    assert_eq!(current.display_name, "Current branch (feature/base)");

    let source_pull_request = AgentWorkspaceSourcePullRequest {
        number: 42,
        url: Some("https://github.com/mock/repo/pull/42".to_string()),
        title: Some("Add PR base".to_string()),
        head_ref_name: "feature/pr-base".to_string(),
        base_ref_name: Some("main".to_string()),
        head_ref_oid: Some("pr-head-sha".to_string()),
    };
    let pr_base =
        normalize_explicit_publish_base_selection(AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::LocalBranch),
            branch_mode: None,
            base_ref: Some("feature/pr-base".to_string()),
            display_name: Some("PR #42: Add PR base".to_string()),
            source_pull_request: Some(source_pull_request.clone()),
        })
        .expect("PR-backed local branch should normalize")
        .expect("PR-backed local branch should produce a selection");
    assert_eq!(pr_base.kind, IdeationAnalysisBaseRefKind::LocalBranch);
    assert_eq!(pr_base.base_ref, "feature/pr-base");
    assert_eq!(pr_base.display_name, "PR #42: Add PR base");
    assert_eq!(pr_base.source_pull_request, Some(source_pull_request));

    let error =
        normalize_explicit_publish_base_selection(AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::PullRequest),
            branch_mode: None,
            base_ref: Some("123".to_string()),
            display_name: None,
            source_pull_request: None,
        })
        .expect_err("pull-request bases should be rejected");
    assert!(error.contains("Pull-request base refs are not supported"));
}

#[tokio::test]
async fn validate_explicit_publish_base_ref_accepts_remote_tracking_ref() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    setup_publish_repo(&repo_path);
    let head = git(&repo_path, &["rev-parse", "HEAD"]);
    git(
        &repo_path,
        &["update-ref", "refs/remotes/origin/release/0.8", &head],
    );

    validate_explicit_publish_base_ref(&repo_path, "release/0.8")
        .await
        .expect("remote-tracking branch should validate");
    let error = validate_explicit_publish_base_ref(&repo_path, "release/missing")
        .await
        .expect_err("missing branch should fail validation");
    assert!(error.contains("Selected base branch 'release/missing' does not exist"));
}

fn command_test_workspace() -> AgentConversationWorkspace {
    AgentConversationWorkspace::new(
        ChatConversationId::from_string("conversation-command-base"),
        ProjectId::from_string("project-command-base".to_string()),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::CurrentBranch,
        "feature/deleted-base".to_string(),
        Some("Current branch (feature/deleted-base)".to_string()),
        Some("old-base-sha".to_string()),
        "ralphx/test/agent-command".to_string(),
        "/tmp/agent-command-workspace".to_string(),
    )
}

fn command_test_workspace_with_git_target() -> (
    tempfile::TempDir,
    AgentConversationWorkspace,
    AgentConversationWorkspaceRepairTarget,
) {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let worktree_path = temp.path().join("agent-workspace");
    std::fs::create_dir_all(&worktree_path).expect("worktree root should be created");
    git(&worktree_path, &["init", "-b", "ralphx/test/agent-command"]);
    git(
        &worktree_path,
        &["config", "user.email", "test@example.com"],
    );
    git(&worktree_path, &["config", "user.name", "Test User"]);
    std::fs::write(worktree_path.join("README.md"), "repair fixture\n")
        .expect("fixture file should be written");
    git(&worktree_path, &["add", "README.md"]);
    git(&worktree_path, &["commit", "-m", "repair fixture"]);

    let mut workspace = command_test_workspace();
    workspace.worktree_path = worktree_path.to_string_lossy().to_string();
    let mut target = AgentConversationWorkspaceRepairTarget::from_workspace(&workspace);
    target.worktree_path = Some(worktree_path);
    (temp, workspace, target)
}

async fn seed_ready_command_repair_attempt(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    continuation: AgentWorkspaceRepairContinuation,
) -> AgentWorkspaceRepairAttempt {
    let started = state
        .agent_workspace_repair_repo
        .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt: AgentWorkspaceRepairAttempt::new(
                workspace.conversation_id.clone(),
                AgentWorkspaceRepairSource::BaseUpdate,
                continuation,
                workspace.base_ref.clone(),
                false,
                true,
                false,
                None,
                chrono::Utc::now(),
            ),
            reason: "seed parked repair for Auto Publish command test".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("seed durable repair attempt");
    let StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(started) = started else {
        panic!("first durable repair generation should start");
    };
    let mut ready = started.clone();
    ready.phase = AgentWorkspaceRepairPhase::Ready;
    ready.summary = Some("Repair is parked at the publish boundary.".to_string());
    ready.updated_at += chrono::Duration::microseconds(1);
    match state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt: ready,
            expected_phase: started.phase,
            expected_updated_at: started.updated_at,
            next_phase: AgentWorkspaceRepairPhase::Ready,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("park durable repair at Ready")
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(ready) => ready,
        outcome => panic!("expected Ready repair attempt, got {outcome:?}"),
    }
}

async fn seed_blocked_command_repair_attempt(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
) -> AgentWorkspaceRepairAttempt {
    let started = state
        .agent_workspace_repair_repo
        .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt: AgentWorkspaceRepairAttempt::new(
                workspace.conversation_id.clone(),
                AgentWorkspaceRepairSource::BaseUpdate,
                AgentWorkspaceRepairContinuation::UpdateOnly,
                workspace.base_ref.clone(),
                false,
                true,
                false,
                None,
                chrono::Utc::now(),
            ),
            reason: "publish rejected: protected branch requires approval".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("seed durable repair attempt");
    let StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(started) = started else {
        panic!("first durable repair generation should start");
    };
    let mut blocked = started.clone();
    blocked.phase = AgentWorkspaceRepairPhase::Blocked;
    blocked
        .pending_reasons
        .push("auto_retry_blocked_repair:3".to_string());
    blocked.summary = Some(
        "Durable workspace repair delivery retry completed. Automatic repair delivery retries are exhausted."
            .to_string(),
    );
    blocked.updated_at += chrono::Duration::microseconds(1);
    match state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt: blocked,
            expected_phase: started.phase,
            expected_updated_at: started.updated_at,
            next_phase: AgentWorkspaceRepairPhase::Blocked,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("block durable repair attempt")
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(blocked) => blocked,
        outcome => panic!("expected blocked repair attempt, got {outcome:?}"),
    }
}

#[test]
fn blocked_workspace_repair_retry_context_carries_blocker_commit_and_base_retarget() {
    let mut attempt = AgentWorkspaceRepairAttempt::new(
        ChatConversationId::from_string("retry-context-blocker".to_string()),
        AgentWorkspaceRepairSource::BaseUpdate,
        AgentWorkspaceRepairContinuation::UpdateOnly,
        "ralphx/old",
        false,
        true,
        false,
        None,
        chrono::Utc::now(),
    );
    attempt.blocker = Some("old base ref was deleted after its PR merged".to_string());
    attempt.repair_head_commit = Some("bba066f".to_string());
    attempt.pending_reasons = vec![
        "real repair failure".to_string(),
        crate::application::agent_workspace_publish_repair_state::NEEDS_HUMAN_REPAIR_REASON
            .to_string(),
    ];

    let context = compose_blocked_repair_retry_context(&attempt, "main");

    assert!(context.contains("old base ref was deleted after its PR merged"));
    assert!(context.contains("bba066f"));
    assert!(context.contains("ralphx/old"));
    assert!(context.contains("main"));
    assert!(!context.contains(
        crate::application::agent_workspace_publish_repair_state::NEEDS_HUMAN_REPAIR_REASON
    ));
}

#[test]
fn blocked_workspace_repair_retry_context_uses_summary_when_no_human_reason_exists() {
    let mut attempt = AgentWorkspaceRepairAttempt::new(
        ChatConversationId::from_string("retry-context-summary".to_string()),
        AgentWorkspaceRepairSource::BaseUpdate,
        AgentWorkspaceRepairContinuation::UpdateOnly,
        "main",
        false,
        true,
        false,
        None,
        chrono::Utc::now(),
    );
    attempt.pending_reasons = vec![
        crate::application::agent_workspace_publish_repair_state::NEEDS_HUMAN_REPAIR_REASON
            .to_string(),
    ];
    attempt.summary = Some("repair summary retained for retry".to_string());

    let context = compose_blocked_repair_retry_context(&attempt, "main");

    assert!(context.contains("repair summary retained for retry"));
    assert!(!context.contains(
        crate::application::agent_workspace_publish_repair_state::NEEDS_HUMAN_REPAIR_REASON
    ));
}

#[test]
fn blocked_workspace_repair_retry_context_prefers_human_reason_over_summary() {
    let mut attempt = AgentWorkspaceRepairAttempt::new(
        ChatConversationId::from_string("retry-context-human-reason".to_string()),
        AgentWorkspaceRepairSource::BaseUpdate,
        AgentWorkspaceRepairContinuation::UpdateOnly,
        "main",
        false,
        true,
        false,
        None,
        chrono::Utc::now(),
    );
    attempt.pending_reasons = vec!["real reason".to_string()];
    attempt.summary = Some("internal delivery message".to_string());

    let context = compose_blocked_repair_retry_context(&attempt, "main");

    assert!(context.contains("real reason"));
    assert!(!context.contains("internal delivery message"));
}

#[test]
fn blocked_workspace_repair_retry_context_uses_default_without_human_context() {
    let attempt = AgentWorkspaceRepairAttempt::new(
        ChatConversationId::from_string("retry-context-default".to_string()),
        AgentWorkspaceRepairSource::BaseUpdate,
        AgentWorkspaceRepairContinuation::UpdateOnly,
        "main",
        false,
        true,
        false,
        None,
        chrono::Utc::now(),
    );

    let context = compose_blocked_repair_retry_context(&attempt, "main");

    assert!(context.contains("Retrying blocked workspace repair."));
}

#[test]
fn blocked_workspace_repair_retry_context_omits_retarget_details_for_same_base() {
    let mut attempt = AgentWorkspaceRepairAttempt::new(
        ChatConversationId::from_string("retry-context-same-base".to_string()),
        AgentWorkspaceRepairSource::BaseUpdate,
        AgentWorkspaceRepairContinuation::UpdateOnly,
        "main",
        false,
        true,
        false,
        None,
        chrono::Utc::now(),
    );
    attempt.blocker = Some("still needs a repair".to_string());

    let context = compose_blocked_repair_retry_context(&attempt, "main");

    assert!(context.contains("still needs a repair"));
    assert!(!context.contains("The base has since been updated"));
}

#[tokio::test]
async fn explicit_workspace_repair_retry_prompt_carries_predecessor_blocker_and_commit() {
    let (_temp, state, conversation_id, _github) = setup_publish_command_state(
        "retry-blocker-context",
        true,
        None,
        Arc::new(MockGithubService::new()),
    )
    .await;
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    let blocked = seed_blocked_command_repair_attempt(&state, &workspace).await;
    let mut enriched = blocked.clone();
    enriched.blocker = Some("old base ref was deleted after its PR merged".to_string());
    enriched.repair_head_commit = Some("bba066f".to_string());
    enriched.pending_reasons.push(
        crate::application::agent_workspace_publish_repair_state::NEEDS_HUMAN_REPAIR_REASON
            .to_string(),
    );
    enriched.updated_at += chrono::Duration::microseconds(1);
    match state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt: enriched,
            expected_phase: blocked.phase,
            expected_updated_at: blocked.updated_at,
            next_phase: AgentWorkspaceRepairPhase::Blocked,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("persist predecessor repair context")
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(_) => {}
        outcome => panic!("expected enriched blocked repair attempt, got {outcome:?}"),
    }
    let service = MockChatService::new();

    assert!(
        retry_blocked_agent_workspace_repair_for_explicit_user_action(
            &state,
            &workspace,
            &service,
            AgentWorkspacePostRepairAction::UpdateOnly,
        )
        .await
    );

    let messages = service.get_sent_messages().await;
    assert_eq!(messages.len(), 1);
    assert!(messages[0].contains(
        "Error: Previous repair attempt was blocked: old base ref was deleted after its PR merged"
    ));
    assert!(messages[0].contains("bba066f"));
    assert!(!messages[0].contains(
        crate::application::agent_workspace_publish_repair_state::NEEDS_HUMAN_REPAIR_REASON
    ));
}

#[tokio::test]
async fn explicit_workspace_repair_retry_prompt_uses_root_pending_reason_not_delivery_summary() {
    let (_temp, state, conversation_id, _github) = setup_publish_command_state(
        "retry-root-cause",
        true,
        None,
        Arc::new(MockGithubService::new()),
    )
    .await;
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    seed_blocked_command_repair_attempt(&state, &workspace).await;
    let service = MockChatService::new();

    assert!(
        retry_blocked_agent_workspace_repair_for_explicit_user_action(
            &state,
            &workspace,
            &service,
            AgentWorkspacePostRepairAction::UpdateOnly,
        )
        .await
    );

    let messages = service.get_sent_messages().await;
    assert_eq!(messages.len(), 1);
    assert!(messages[0].contains("publish rejected: protected branch requires approval"));
    assert!(!messages[0].contains("Automatic repair delivery retries are exhausted"));
}

#[tokio::test]
async fn base_update_retry_returns_successful_repair_started_response() {
    let (_temp, state, conversation_id, _github) = setup_publish_command_state(
        "retry-success-response",
        true,
        None,
        Arc::new(MockGithubService::new()),
    )
    .await;
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    seed_blocked_command_repair_attempt(&state, &workspace).await;

    let response = update_agent_conversation_workspace_from_base_for_app_state(
        &state,
        &Arc::new(ExecutionState::new()),
        conversation_id,
        AgentConversationWorkspaceBaseSelection {
            kind: None,
            branch_mode: None,
            base_ref: None,
            display_name: None,
            source_pull_request: None,
        },
    )
    .await
    .expect("explicit retry should return a successful repair-started response");

    assert!(response.repair_started);
    assert_eq!(response.target_ref, workspace.base_ref);
    assert_eq!(
        response.base_commit,
        workspace.base_commit.unwrap_or_default()
    );
}

#[tokio::test]
async fn workspace_response_does_not_recover_a_stranded_repair_inline() {
    let state = AppState::new_test();
    let mut workspace = command_test_workspace();
    workspace.last_blocked_pr_health_fingerprint =
        Some("github_pr_autofix:42:checks:rust-tests".to_string());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");

    let started = state
        .agent_workspace_repair_repo
        .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt: AgentWorkspaceRepairAttempt::new(
                workspace.conversation_id.clone(),
                AgentWorkspaceRepairSource::BaseUpdate,
                AgentWorkspaceRepairContinuation::Publish,
                workspace.base_ref.clone(),
                false,
                false,
                false,
                None,
                chrono::Utc::now(),
            ),
            reason: "seed stranded repair for workspace-read regression".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("seed durable repair attempt");
    let StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(started) = started else {
        panic!("first durable repair generation should start");
    };
    let mut stranded = started.clone();
    stranded.phase = AgentWorkspaceRepairPhase::Repairing;
    stranded.updated_at += chrono::Duration::microseconds(1);
    let stranded = match state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt: stranded,
            expected_phase: started.phase,
            expected_updated_at: started.updated_at,
            next_phase: AgentWorkspaceRepairPhase::Repairing,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("mark repair stranded")
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(attempt) => attempt,
        outcome => panic!("expected stranded repair attempt, got {outcome:?}"),
    };

    let response = tokio::time::timeout(
        Duration::from_millis(100),
        agent_workspace_response_for_state(&state, workspace.clone()),
    )
    .await
    .expect("workspace read should return without waiting for recovery")
    .expect("workspace response should succeed");

    assert_eq!(response.conversation_id, workspace.conversation_id.as_str());
    assert_eq!(
        response.pr_autofix_fingerprint_spend,
        Some(super::PrAutofixFingerprintSpendResponse {
            generations: 0,
            minutes: 0,
            budget_minutes: crate::infrastructure::agents::limits_config()
                .repair_fingerprint_budget_minutes,
            is_exhausted: false,
        })
    );
    assert_eq!(
        state
            .agent_workspace_repair_repo
            .get_current_repair_attempt(&workspace.conversation_id)
            .await
            .expect("read durable repair attempt"),
        Some(stranded),
        "workspace reads must not reserve, block, or otherwise recover a durable repair inline"
    );
    assert!(
        state
            .agent_conversation_workspace_repo
            .list_publication_events(&workspace.conversation_id)
            .await
            .expect("read workspace events")
            .is_empty(),
        "workspace reads must not emit recovery compatibility events inline"
    );
}

#[tokio::test]
async fn review_pr_rejects_supervision_and_auto_publish_changes_without_mutation() {
    let state = AppState::new_test();
    let mut workspace = command_test_workspace();
    workspace.mode = AgentConversationWorkspaceMode::ReviewPr;
    workspace.source_pull_request = Some(AgentWorkspaceSourcePullRequest {
        number: 77,
        url: Some("https://github.com/owner/repo/pull/77".to_string()),
        title: Some("External PR".to_string()),
        head_ref_name: "external/head".to_string(),
        base_ref_name: Some("main".to_string()),
        head_ref_oid: Some("external-head".to_string()),
    });
    workspace.publication_pr_number = Some(77);
    workspace.publication_pr_status = Some("open".to_string());
    workspace.publication_push_status = Some("pushed".to_string());
    workspace.auto_publish_enabled = false;
    workspace.auto_publish_paused_pr_autofix_enabled = Some(true);
    workspace.auto_publish_paused_pr_auto_merge_desired = Some(true);
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");
    let original = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&workspace.conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");

    let supervision_error = set_agent_conversation_workspace_pr_supervision_for_state(
        workspace.conversation_id.as_str(),
        AgentConversationWorkspacePrSupervisionInput {
            auto_fix_enabled: true,
            auto_merge_desired: true,
            auto_merge_method: None,
        },
        &state,
    )
    .await
    .expect_err("Review PR supervision should fail closed");
    let auto_publish_error = set_agent_conversation_workspace_auto_publish_for_state(
        workspace.conversation_id.as_str(),
        AgentConversationWorkspaceAutoPublishInput {
            auto_publish_enabled: true,
        },
        &state,
    )
    .await
    .expect_err("Review PR Auto Publish changes should fail closed");

    assert!(supervision_error.contains("Review PR"));
    assert!(auto_publish_error.contains("Review PR"));
    assert_eq!(
        state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&workspace.conversation_id)
            .await
            .expect("workspace lookup should succeed"),
        Some(original)
    );
    assert!(state
        .agent_conversation_workspace_repo
        .list_publication_events(&workspace.conversation_id)
        .await
        .expect("events should list")
        .is_empty());
}

fn command_test_pr_health(auto_merge_active: bool) -> PrHealth {
    PrHealth {
        sync_state: PrSyncState {
            status: GithubPrStatus::Open,
            merge_state_status: None,
            mergeable: None,
            is_draft: false,
            head_ref_name: "feature".to_string(),
            base_ref_name: "main".to_string(),
            head_ref_oid: None,
            base_ref_oid: None,
        },
        review_decision: None,
        checks: Vec::new(),
        issue_comments: Vec::new(),
        auto_merge_request: if auto_merge_active {
            Some(PrAutoMergeRequest {
                enabled_by: Some("github-user".to_string()),
                merge_method: Some("squash".to_string()),
            })
        } else {
            None
        },
    }
}

fn command_publish_target() -> AgentConversationWorkspacePublishTarget {
    AgentConversationWorkspacePublishTarget {
        worktree_path: PathBuf::from("/tmp/project-repo"),
        branch_name: "ralphx/test/agent-command".to_string(),
        base_ref: "feature/deleted-base".to_string(),
        base_display_name: Some("Current branch (feature/deleted-base)".to_string()),
        plan_branch: None,
    }
}

fn external_pr_test_project(name: &str) -> Project {
    let mut project = Project::new(name.to_string(), format!("/tmp/{name}"));
    project.base_branch = Some("main".to_string());
    project.github_pr_enabled = true;
    project
}

fn external_pr_test_workspace(project: &Project, suffix: &str) -> AgentConversationWorkspace {
    AgentConversationWorkspace::new(
        ChatConversationId::new(),
        project.id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some("base-sha".to_string()),
        format!("ralphx/test/agent-{suffix}"),
        format!("/tmp/external-pr-command-{suffix}"),
    )
}

async fn wait_for_latest_pr_lookup_calls(github: &MockGithubService, expected: u32) {
    for _ in 0..100 {
        if github.state().find_latest_pr_by_head_branch_calls >= expected {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!(
        "expected at least {expected} latest PR lookups, got {}",
        github.state().find_latest_pr_by_head_branch_calls
    );
}

async fn wait_for_pr_sync_state_calls(github: &MockGithubService, expected: u32) {
    for _ in 0..100 {
        if github.state().check_pr_sync_state_calls >= expected {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!(
        "expected at least {expected} PR sync-state lookups, got {}",
        github.state().check_pr_sync_state_calls
    );
}

#[tokio::test]
async fn workspace_load_external_pr_reconciliation_schedules_for_reconcilable_workspace() {
    let mut state = AppState::new_test();
    let project = external_pr_test_project("external-pr-command-load");
    let workspace = external_pr_test_workspace(&project, "load");
    let github = Arc::new(MockGithubService::new());
    state.github_service = Some(github.clone());
    state.project_repo.create(project).await.unwrap();
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .unwrap();

    schedule_external_pr_reconciliation_for_workspace(
        &state,
        &workspace,
        AgentWorkspaceExternalPrReconciliationTrigger::WorkspaceLoad,
        false,
    );

    wait_for_latest_pr_lookup_calls(&github, 1).await;
    assert_eq!(
        github
            .state()
            .last_find_latest_pr_by_head_branch_name
            .as_deref(),
        Some(workspace.branch_name.as_str())
    );
}

#[tokio::test]
async fn workspace_load_external_pr_reconciliation_skips_unreconcilable_workspace() {
    let mut state = AppState::new_test();
    let project = external_pr_test_project("external-pr-command-skip");
    let mut workspace = external_pr_test_workspace(&project, "skip");
    workspace.publication_pr_number = Some(77);
    workspace.publication_pr_status = Some("open".to_string());
    let github = Arc::new(MockGithubService::new());
    state.github_service = Some(github.clone());

    schedule_external_pr_reconciliation_for_workspace(
        &state,
        &workspace,
        AgentWorkspaceExternalPrReconciliationTrigger::WorkspaceLoad,
        false,
    );
    tokio::time::sleep(Duration::from_millis(25)).await;

    assert_eq!(github.state().find_latest_pr_by_head_branch_calls, 0);
}

#[tokio::test]
async fn run_completed_external_pr_reconciliation_links_terminal_pr() {
    let mut state = AppState::new_test();
    let project = external_pr_test_project("external-pr-command-run-completed");
    let workspace = external_pr_test_workspace(&project, "run-completed");
    let conversation_id = workspace.conversation_id.clone();
    let github = Arc::new(MockGithubService::new());
    github.set_find_latest_pr_by_head_branch(Ok(Some(PrBranchMatch {
        number: 123,
        url: "https://github.com/owner/repo/pull/123".to_string(),
        status: GithubPrStatus::Closed,
        is_draft: false,
        head_ref_name: workspace.branch_name.clone(),
        updated_at: Some("2026-05-14T10:00:00Z".to_string()),
        author_login: None,
    })));
    state.github_service = Some(github.clone());
    state.project_repo.create(project).await.unwrap();
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();

    schedule_external_pr_reconciliation_for_conversation_id(
        &state,
        conversation_id.clone(),
        AgentWorkspaceExternalPrReconciliationTrigger::AgentRunCompleted,
        true,
    )
    .await
    .unwrap();

    wait_for_latest_pr_lookup_calls(&github, 1).await;
    let updated = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .expect("workspace should still exist");
    assert_eq!(updated.publication_pr_number, Some(123));
    assert_eq!(updated.publication_pr_status.as_deref(), Some("closed"));

    let events = state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].step, "external_pr_closed");
}

#[tokio::test]
async fn run_completed_pr_supervision_recovery_rearms_blocked_workspace() {
    let (_temp, state, conversation_id, github) = setup_publish_command_state(
        "pr-supervision-command-recovery",
        true,
        Some(257),
        Arc::new(MockGithubService::new()),
    )
    .await;
    let mut workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    workspace.publication_push_status = Some("failed".to_string());
    workspace.pr_supervision_status = Some("blocked".to_string());
    workspace.pr_autofix_enabled = true;
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace update should persist");
    let head_sha = git(Path::new(&workspace.worktree_path), &["rev-parse", "HEAD"]);
    github.will_return_sync_state(PrSyncState {
        status: GithubPrStatus::Open,
        merge_state_status: Some(PrMergeStateStatus::Clean),
        mergeable: Some(PrMergeableState::Mergeable),
        is_draft: false,
        head_ref_name: workspace.branch_name.clone(),
        base_ref_name: "main".to_string(),
        head_ref_oid: Some(head_sha),
        base_ref_oid: None,
    });

    schedule_pr_supervision_recovery_for_conversation_id(
        &state,
        conversation_id.clone(),
        AgentWorkspacePrSupervisionRecoveryTrigger::AgentRunCompleted,
        true,
    )
    .await
    .expect("recovery scheduling should succeed");

    wait_for_pr_sync_state_calls(&github, 1).await;
    let updated = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert_eq!(updated.publication_push_status.as_deref(), Some("pushed"));
    assert_eq!(updated.pr_supervision_status.as_deref(), Some("monitoring"));
}

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git command should spawn");
    assert!(
        output.status.success(),
        "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

struct SubmittingPrDescriptionClient {
    repo: Arc<dyn AgentConversationWorkspaceRepository>,
    conversation_id: ChatConversationId,
    spawned: tokio::sync::Mutex<usize>,
    spawned_configs: tokio::sync::Mutex<Vec<AgentConfig>>,
    decisions: tokio::sync::Mutex<VecDeque<AgentWorkspacePrMetadataDecision>>,
}

impl SubmittingPrDescriptionClient {
    fn new(
        repo: Arc<dyn AgentConversationWorkspaceRepository>,
        conversation_id: ChatConversationId,
    ) -> Self {
        Self {
            repo,
            conversation_id,
            spawned: tokio::sync::Mutex::new(0),
            spawned_configs: tokio::sync::Mutex::new(Vec::new()),
            decisions: tokio::sync::Mutex::new(VecDeque::new()),
        }
    }

    async fn queue_decision(&self, decision: AgentWorkspacePrMetadataDecision) {
        self.decisions.lock().await.push_back(decision);
    }

    async fn spawned_count(&self) -> usize {
        *self.spawned.lock().await
    }

    async fn spawned_configs(&self) -> Vec<AgentConfig> {
        self.spawned_configs.lock().await.clone()
    }
}

#[async_trait]
impl AgenticClient for SubmittingPrDescriptionClient {
    async fn spawn_agent(&self, config: AgentConfig) -> AgentResult<AgentHandle> {
        *self.spawned.lock().await += 1;
        self.spawned_configs.lock().await.push(config.clone());
        Ok(AgentHandle::mock(config.role))
    }

    async fn stop_agent(&self, _handle: &AgentHandle) -> AgentResult<()> {
        Ok(())
    }

    async fn wait_for_completion(&self, _handle: &AgentHandle) -> AgentResult<AgentOutput> {
        let decision = self.decisions.lock().await.pop_front().unwrap_or(
            AgentWorkspacePrMetadataDecision::Patch {
                title: Some("Cached publication title".to_string()),
                body_markdown: Some("## Summary\n\nReady to publish.".to_string()),
            },
        );
        self.repo
            .save_pr_metadata_decision(&self.conversation_id, decision)
            .await
            .expect("test PR description should save");
        Ok(AgentOutput::success("submitted"))
    }

    async fn send_prompt(
        &self,
        _handle: &AgentHandle,
        _prompt: &str,
    ) -> AgentResult<AgentResponse> {
        Ok(AgentResponse::new(""))
    }

    fn stream_response(
        &self,
        _handle: &AgentHandle,
        _prompt: &str,
    ) -> Pin<Box<dyn Stream<Item = AgentResult<ResponseChunk>> + Send>> {
        Box::pin(stream::empty())
    }

    fn capabilities(&self) -> &ClientCapabilities {
        static CAPS: std::sync::OnceLock<ClientCapabilities> = std::sync::OnceLock::new();
        CAPS.get_or_init(ClientCapabilities::mock)
    }

    async fn is_available(&self) -> AgentResult<bool> {
        Ok(true)
    }
}

fn setup_publish_repo(repo_path: &Path) -> String {
    std::fs::create_dir_all(repo_path).expect("repo root should be created");
    git(repo_path, &["init", "-b", "main"]);
    git(repo_path, &["config", "user.email", "test@example.com"]);
    git(repo_path, &["config", "user.name", "Test User"]);
    std::fs::write(repo_path.join("README.md"), "base\n").expect("fixture file should be written");
    git(repo_path, &["add", "README.md"]);
    git(repo_path, &["commit", "-m", "base"]);
    git(repo_path, &["rev-parse", "HEAD"])
}

async fn setup_publish_command_state(
    suffix: &str,
    capture_base_commit: bool,
    publication_pr_number: Option<i64>,
    github: Arc<MockGithubService>,
) -> (
    tempfile::TempDir,
    AppState,
    ChatConversationId,
    Arc<MockGithubService>,
) {
    setup_publish_command_state_with_mode(
        suffix,
        capture_base_commit,
        publication_pr_number,
        github,
        AgentConversationWorkspaceMode::Edit,
    )
    .await
}

async fn setup_publish_command_state_with_mode(
    suffix: &str,
    capture_base_commit: bool,
    publication_pr_number: Option<i64>,
    github: Arc<MockGithubService>,
    workspace_mode: AgentConversationWorkspaceMode,
) -> (
    tempfile::TempDir,
    AppState,
    ChatConversationId,
    Arc<MockGithubService>,
) {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    let main_sha = setup_publish_repo(&repo_path);

    let mut project = Project::new(
        format!("Publish Base {suffix}"),
        repo_path.to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
    let conversation_id = ChatConversationId::from_string(uuid::Uuid::new_v4().to_string());
    let mut workspace = prepare_agent_conversation_workspace(
        &project,
        &conversation_id,
        workspace_mode,
        AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
            branch_mode: None,
            base_ref: Some("main".to_string()),
            display_name: None,
            source_pull_request: None,
        },
    )
    .await
    .expect("workspace should be prepared");
    workspace.base_ref = "feature/deleted-base".to_string();
    workspace.base_display_name = Some("Current branch (feature/deleted-base)".to_string());
    workspace.base_commit = capture_base_commit.then_some(main_sha);
    workspace.publication_pr_number = publication_pr_number;
    workspace.publication_pr_url =
        publication_pr_number.map(|number| format!("https://github.com/mock/repo/pull/{number}"));
    workspace.publication_pr_status = publication_pr_number.map(|_| "open".to_string());

    let mut state = AppState::new_test();
    let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
    state.github_service = Some(github_trait);
    state
        .project_repo
        .create(project.clone())
        .await
        .expect("project should be persisted");
    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.id = conversation_id.clone();
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("conversation should be persisted");
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should be persisted");

    (temp, state, conversation_id, github)
}

async fn published_workspace_and_project(
    state: &AppState,
    conversation_id: &ChatConversationId,
) -> (AgentConversationWorkspace, Project) {
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    let project = state
        .project_repo
        .get_by_id(&workspace.project_id)
        .await
        .expect("project lookup should succeed")
        .expect("project should exist");
    (workspace, project)
}

async fn use_main_as_publish_base(
    state: &AppState,
    conversation_id: &ChatConversationId,
) -> AgentConversationWorkspace {
    let (mut workspace, _project) = published_workspace_and_project(state, conversation_id).await;
    workspace.base_ref = "main".to_string();
    workspace.base_display_name = Some("Project default (main)".to_string());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace base should update");
    workspace
}

async fn enable_github_pr_publishing(state: &AppState, conversation_id: &ChatConversationId) {
    let (workspace, mut project) = published_workspace_and_project(state, conversation_id).await;
    git(
        Path::new(&project.working_directory),
        &["remote", "add", "origin", &project.working_directory],
    );
    git(
        Path::new(&project.working_directory),
        &[
            "config",
            "remote.origin.pushurl",
            "git@github.com:owner/repository.git",
        ],
    );
    project.github_pr_enabled = true;
    state
        .project_repo
        .update(&project)
        .await
        .expect("GitHub-capable project should persist");
    assert_eq!(workspace.project_id, project.id);
}

async fn seed_current_passing_workspace_review(
    state: &AppState,
    conversation_id: &ChatConversationId,
) {
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    let context = crate::application::agent_workspace_review::load_agent_workspace_review_context(
        state, &workspace,
    )
    .await
    .expect("review context should load");
    let target = context.target.expect("review target should exist");
    let mut monitor = context.monitor;
    crate::application::agent_workspace_review::apply_review_artifact_to_monitor(
        &mut monitor,
        target.scope,
        target.head_sha,
        target.diff_fingerprint,
        Some("seeded-passing-review".to_string()),
        ArtifactId::from_string(format!("review-artifact-{}", conversation_id.as_str())),
        1,
        chrono::Utc::now(),
        None,
    );
    monitor.review_outcome = AgentWorkspaceReviewOutcome::Passed;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Passed;
    monitor.status = AgentWorkspaceReviewMonitorStatus::Ready;
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("passing review monitor should persist");
}

fn commit_file(repo: &Path, relative_path: &str, contents: &str, message: &str) -> String {
    let path = repo.join(relative_path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("parent directory should be created");
    }
    std::fs::write(&path, contents).expect("fixture file should be written");
    git(repo, &["add", relative_path]);
    git(repo, &["commit", "-m", message]);
    git(repo, &["rev-parse", "HEAD"])
}

async fn setup_linked_plan_publish_command_state(
    suffix: &str,
    active_regular_task: bool,
    github: Arc<MockGithubService>,
) -> (
    tempfile::TempDir,
    AppState,
    ChatConversationId,
    PlanBranchId,
    Arc<MockGithubService>,
) {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    let main_sha = setup_publish_repo(&repo_path);
    let origin_path = repo_path.to_string_lossy().to_string();
    git(
        &repo_path,
        &["remote", "add", "origin", origin_path.as_str()],
    );
    let plan_branch_name = format!("feature/plan-publish-{suffix}");
    git(&repo_path, &["checkout", "-b", &plan_branch_name]);
    std::fs::write(repo_path.join("plan.txt"), "plan branch change\n")
        .expect("plan fixture should be written");
    git(&repo_path, &["add", "plan.txt"]);
    git(&repo_path, &["commit", "-m", "plan branch change"]);
    git(&repo_path, &["checkout", "main"]);

    let mut project = Project::new(
        format!("Plan Publish {suffix}"),
        repo_path.to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
    let conversation_id = ChatConversationId::from_string(uuid::Uuid::new_v4().to_string());
    let session_id = IdeationSessionId::from_string(format!("session-plan-publish-{suffix}"));
    let execution_plan = ExecutionPlan::new(session_id.clone());
    let mut plan_branch = PlanBranch::new(
        ArtifactId::from_string(format!("artifact-plan-publish-{suffix}")),
        session_id.clone(),
        project.id.clone(),
        plan_branch_name.clone(),
        "main".to_string(),
    );
    plan_branch.execution_plan_id = Some(execution_plan.id.clone());
    plan_branch.pr_number = Some(77);
    plan_branch.pr_url = Some("https://github.com/mock/repo/pull/77".to_string());
    plan_branch.pr_status = Some(PrStatus::Open);
    plan_branch.pr_push_status = PrPushStatus::Pending;
    let plan_branch_id = plan_branch.id.clone();
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id.clone(),
        project.id.clone(),
        AgentConversationWorkspaceMode::Ideation,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some(main_sha),
        "agent-shell-plan-publish".to_string(),
        temp.path()
            .join("agent-shell-plan-publish")
            .to_string_lossy()
            .to_string(),
    );
    workspace.linked_ideation_session_id = Some(session_id.clone());
    workspace.linked_plan_branch_id = Some(plan_branch_id.clone());

    let mut task = Task::new(project.id.clone(), "Plan task".to_string());
    task.ideation_session_id = Some(session_id.clone());
    task.execution_plan_id = Some(execution_plan.id.clone());
    task.internal_status = if active_regular_task {
        InternalStatus::Executing
    } else {
        InternalStatus::Merged
    };

    let mut state = AppState::new_test();
    let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
    state.github_service = Some(github_trait);
    state
        .project_repo
        .create(project.clone())
        .await
        .expect("project should be persisted");
    state
        .ideation_session_repo
        .create(
            IdeationSession::builder()
                .id(session_id.clone())
                .project_id(project.id.clone())
                .session_flow(IdeationSessionFlow::Planning)
                .source_context_type("agent_conversation")
                .source_context_id(conversation_id.as_str())
                .build(),
        )
        .await
        .expect("planning session should be persisted");
    state
        .execution_plan_repo
        .create(execution_plan)
        .await
        .expect("execution plan should be persisted");
    state
        .plan_branch_repo
        .create(plan_branch)
        .await
        .expect("plan branch should be persisted");
    state
        .task_repo
        .create(task)
        .await
        .expect("task should be persisted");
    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.id = conversation_id.clone();
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("conversation should be persisted");
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should be persisted");

    (temp, state, conversation_id, plan_branch_id, github)
}

#[tokio::test]
async fn precompute_pr_description_skips_workspace_without_reviewable_commits() {
    let (_temp, state, conversation_id, _github) = setup_publish_command_state(
        "precompute-no-commits",
        true,
        None,
        Arc::new(MockGithubService::new()),
    )
    .await;

    let response = precompute_agent_conversation_workspace_pr_description_for_app_state(
        &state,
        conversation_id,
    )
    .await
    .expect("precompute should skip without error");

    assert_eq!(response.status, "skipped");
    assert_eq!(response.reason.as_deref(), Some("no_reviewable_commits"));
    assert!(response.cache_status.is_none());
}

#[tokio::test]
async fn precompute_pr_description_skips_non_edit_workspaces() {
    let (_temp, state, conversation_id, _github) = setup_publish_command_state(
        "precompute-non-edit",
        true,
        None,
        Arc::new(MockGithubService::new()),
    )
    .await;
    let mut workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    workspace.mode = AgentConversationWorkspaceMode::Chat;
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace mode should update");

    let response = precompute_agent_conversation_workspace_pr_description_for_app_state(
        &state,
        conversation_id,
    )
    .await
    .expect("precompute should skip without error");

    assert_eq!(response.status, "skipped");
    assert_eq!(response.reason.as_deref(), Some("not_edit_workspace"));
    assert!(response.cache_status.is_none());
}

#[tokio::test]
async fn precompute_pr_description_skips_missing_review_base() {
    let (_temp, state, conversation_id, _github) = setup_publish_command_state(
        "precompute-missing-base",
        false,
        None,
        Arc::new(MockGithubService::new()),
    )
    .await;

    let response = precompute_agent_conversation_workspace_pr_description_for_app_state(
        &state,
        conversation_id,
    )
    .await
    .expect("precompute should skip without error");

    assert_eq!(response.status, "skipped");
    assert_eq!(response.reason.as_deref(), Some("missing_review_base"));
    assert!(response.cache_status.is_none());
}

#[tokio::test]
async fn precompute_pr_description_skips_dirty_workspace() {
    let (_temp, state, conversation_id, _github) = setup_publish_command_state(
        "precompute-dirty",
        true,
        None,
        Arc::new(MockGithubService::new()),
    )
    .await;
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    std::fs::write(
        PathBuf::from(workspace.worktree_path).join("dirty.txt"),
        "uncommitted\n",
    )
    .expect("dirty file should be written");

    let response = precompute_agent_conversation_workspace_pr_description_for_app_state(
        &state,
        conversation_id,
    )
    .await
    .expect("precompute should skip without error");

    assert_eq!(response.status, "skipped");
    assert_eq!(response.reason.as_deref(), Some("uncommitted_changes"));
    assert!(response.cache_status.is_none());
}

#[tokio::test]
async fn precompute_pr_description_skips_when_base_is_ahead() {
    let (_temp, state, conversation_id, _github) = setup_publish_command_state(
        "precompute-base-ahead",
        true,
        None,
        Arc::new(MockGithubService::new()),
    )
    .await;
    let workspace = use_main_as_publish_base(&state, &conversation_id).await;
    let project = state
        .project_repo
        .get_by_id(&workspace.project_id)
        .await
        .expect("project lookup should succeed")
        .expect("project should exist");
    let worktree_path = PathBuf::from(&workspace.worktree_path);
    commit_file(
        &worktree_path,
        "feature-only.txt",
        "feature\n",
        "Add feature-only change",
    );

    let repo_path = PathBuf::from(&project.working_directory);
    git(&repo_path, &["checkout", "main"]);
    commit_file(&repo_path, "base-only.txt", "base\n", "Advance base branch");

    let client = Arc::new(SubmittingPrDescriptionClient::new(
        Arc::clone(&state.agent_conversation_workspace_repo),
        conversation_id.clone(),
    ));
    let state = state.with_agent_client(client.clone());

    let response = precompute_agent_conversation_workspace_pr_description_for_app_state(
        &state,
        conversation_id,
    )
    .await
    .expect("precompute should skip behind-base workspace without error");

    assert_eq!(response.status, "skipped");
    assert_eq!(response.reason.as_deref(), Some("base_ahead"));
    assert!(response.cache_status.is_none());
    assert_eq!(client.spawned_count().await, 0);
}

#[tokio::test]
async fn precompute_pr_description_uses_current_base_when_branch_contains_target_base() {
    let (_temp, state, conversation_id, _github) = setup_publish_command_state(
        "precompute-stale-base-contained",
        true,
        None,
        Arc::new(MockGithubService::new()),
    )
    .await;
    let workspace = use_main_as_publish_base(&state, &conversation_id).await;
    let project = state
        .project_repo
        .get_by_id(&workspace.project_id)
        .await
        .expect("project lookup should succeed")
        .expect("project should exist");
    let worktree_path = PathBuf::from(&workspace.worktree_path);
    commit_file(
        &worktree_path,
        "feature-only.txt",
        "feature\n",
        "Add feature-only change",
    );

    let repo_path = PathBuf::from(&project.working_directory);
    git(&repo_path, &["checkout", "main"]);
    let current_base = commit_file(&repo_path, "base-only.txt", "base\n", "Advance base branch");
    git(&worktree_path, &["merge", "--no-edit", "main"]);

    let stored = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert_ne!(
        stored.base_commit.as_deref(),
        Some(current_base.as_str()),
        "fixture should keep the stored base commit stale"
    );

    let client = Arc::new(SubmittingPrDescriptionClient::new(
        Arc::clone(&state.agent_conversation_workspace_repo),
        conversation_id.clone(),
    ));
    let state = state.with_agent_client(client.clone());

    let response = precompute_agent_conversation_workspace_pr_description_for_app_state(
        &state,
        conversation_id,
    )
    .await
    .expect("precompute should draft from the effective current base");

    assert_eq!(response.status, "ready");
    assert_eq!(response.cache_status.as_deref(), Some("miss"));
    assert_eq!(client.spawned_count().await, 1);
    let configs = client.spawned_configs().await;
    let prompt = &configs
        .first()
        .expect("describer should have been spawned")
        .prompt;
    assert!(
        prompt.contains(&format!("<review_base>{current_base}</review_base>")),
        "prompt should use the current target base as the review base"
    );
    assert!(
        prompt.contains("feature-only.txt"),
        "feature file should remain in the PR diff context"
    );
    assert!(
        !prompt.contains("base-only.txt"),
        "base-only file must not appear in the PR diff context"
    );
}

#[tokio::test]
async fn precompute_pr_description_caches_ready_workspace_description() {
    let (_temp, state, conversation_id, _github) = setup_publish_command_state(
        "precompute-ready",
        true,
        None,
        Arc::new(MockGithubService::new()),
    )
    .await;
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    let worktree_path = PathBuf::from(&workspace.worktree_path);
    std::fs::write(worktree_path.join("publish-ready.txt"), "ready\n")
        .expect("publish fixture should be written");
    git(&worktree_path, &["add", "publish-ready.txt"]);
    git(
        &worktree_path,
        &["commit", "-m", "Add publish ready fixture"],
    );

    let client = Arc::new(SubmittingPrDescriptionClient::new(
        Arc::clone(&state.agent_conversation_workspace_repo),
        conversation_id.clone(),
    ));
    let state = state.with_agent_client(client.clone());

    let first = precompute_agent_conversation_workspace_pr_description_for_app_state(
        &state,
        conversation_id.clone(),
    )
    .await
    .expect("precompute should prepare a description");
    assert_eq!(first.status, "ready");
    assert_eq!(first.cache_status.as_deref(), Some("miss"));
    assert_eq!(first.reason, None);

    let second = precompute_agent_conversation_workspace_pr_description_for_app_state(
        &state,
        conversation_id,
    )
    .await
    .expect("precompute should reuse cached description");
    assert_eq!(second.status, "ready");
    assert_eq!(second.cache_status.as_deref(), Some("hit"));
    assert_eq!(client.spawned_count().await, 1);
}

#[test]
fn base_resolution_updates_publish_target_or_blocks_with_reason() {
    let resolution = retargeted_base_resolution();
    let mut target = command_publish_target();

    apply_base_resolution_to_publish_target(&mut target, &resolution)
        .expect("retargeted base should update publish target");

    assert_eq!(target.base_ref, "main");
    assert_eq!(
        target.base_display_name.as_deref(),
        Some("Project default (main)")
    );

    let blocked = blocked_base_resolution("cannot verify base");
    let error = apply_base_resolution_to_publish_target(&mut target, &blocked)
        .expect_err("blocked base should stop publish target update");
    assert_eq!(error, "cannot verify base");
}

#[tokio::test]
async fn persisting_retargeted_base_resolution_updates_workspace_metadata() {
    let state = AppState::new_test();
    let mut workspace = command_test_workspace();
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should be persisted");

    persist_workspace_base_resolution_if_retargeted(
        &state,
        &mut workspace,
        &retargeted_base_resolution(),
    )
    .await
    .expect("retargeted workspace metadata should persist");

    assert_eq!(
        workspace.base_ref_kind,
        IdeationAnalysisBaseRefKind::ProjectDefault
    );
    assert_eq!(workspace.base_ref, "main");
    assert_eq!(workspace.base_commit.as_deref(), Some("main-sha"));
    let stored = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&workspace.conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert_eq!(stored.base_ref, "main");
    assert_eq!(
        stored.base_display_name.as_deref(),
        Some("Project default (main)")
    );
}

#[tokio::test]
async fn retargeting_existing_workspace_pr_updates_github_base() {
    let mut state = AppState::new_test();
    let github = Arc::new(MockGithubService::new());
    let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
    state.github_service = Some(github_trait);
    let mut workspace = command_test_workspace();
    workspace.publication_pr_number = Some(123);
    let target = command_publish_target();

    retarget_existing_workspace_pr_base_if_needed(
        &state,
        &target,
        &workspace,
        &retargeted_base_resolution(),
    )
    .await
    .expect("existing PR should be retargeted");

    let mock_state = github.state();
    assert_eq!(mock_state.update_pr_base_calls, 1);
    assert_eq!(
        mock_state.last_update_pr_base_args,
        Some((123, "main".to_string()))
    );
}

#[tokio::test]
async fn retargeting_existing_workspace_pr_blocks_when_github_is_missing_or_fails() {
    let mut workspace = command_test_workspace();
    workspace.publication_pr_number = Some(123);
    let target = command_publish_target();
    let resolution = retargeted_base_resolution();

    let missing_error = retarget_existing_workspace_pr_base_if_needed(
        &AppState::new_test(),
        &target,
        &workspace,
        &resolution,
    )
    .await
    .expect_err("missing GitHub service should block existing PR retarget");
    assert_eq!(
        missing_error,
        existing_pr_retarget_block_reason(123, &resolution)
    );

    let mut state = AppState::new_test();
    let github = Arc::new(MockGithubService::new());
    {
        github.state().update_pr_base_result =
            Some(Err(AppError::Infrastructure("denied".to_string())));
    }
    let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
    state.github_service = Some(github_trait);

    let failure_error =
        retarget_existing_workspace_pr_base_if_needed(&state, &target, &workspace, &resolution)
            .await
            .expect_err("GitHub retarget failure should block existing PR");
    assert_eq!(
        failure_error,
        existing_pr_retarget_block_reason(123, &resolution)
    );
    assert_eq!(github.state().update_pr_base_calls, 1);
}

#[tokio::test]
async fn retargeting_workspace_without_existing_pr_is_a_noop() {
    let state = AppState::new_test();
    let workspace = command_test_workspace();
    let target = command_publish_target();

    retarget_existing_workspace_pr_base_if_needed(
        &state,
        &target,
        &workspace,
        &retargeted_base_resolution(),
    )
    .await
    .expect("workspace without PR should not require GitHub");
}

#[tokio::test]
async fn retargeting_terminal_publication_pr_is_a_noop() {
    let mut state = AppState::new_test();
    let github = Arc::new(MockGithubService::new());
    let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
    state.github_service = Some(github_trait);
    let mut workspace = command_test_workspace();
    workspace.publication_pr_number = Some(123);
    workspace.publication_pr_status = Some("merged".to_string());

    retarget_existing_workspace_pr_base_if_needed(
        &state,
        &command_publish_target(),
        &workspace,
        &retargeted_base_resolution(),
    )
    .await
    .expect("terminal publication PR must not be retargeted");

    assert_eq!(github.state().update_pr_base_calls, 0);
}

#[test]
fn freshness_response_includes_effective_and_blocked_base_state() {
    let status = PublishBranchFreshnessStatus {
        target_ref: "origin/main".to_string(),
        captured_base_commit: Some("old-base-sha".to_string()),
        target_base_commit: "main-sha".to_string(),
        is_base_ahead: true,
    };
    let retargeted = retargeted_base_resolution();
    let response = AgentConversationWorkspaceFreshnessResponse::from_target_status(
        "conversation-command-base".to_string(),
        AgentWorkspaceFreshnessScope::Full,
        "feature/deleted-base".to_string(),
        Some("Current branch (feature/deleted-base)".to_string()),
        Some(&retargeted),
        status.clone(),
        true,
        Some(2),
        true,
        true,
    );

    assert_eq!(response.base_status, "retargeted");
    assert_eq!(response.effective_base_ref.as_deref(), Some("main"));
    assert_eq!(
        response.effective_base_display_name.as_deref(),
        Some("Project default (main)")
    );
    assert_eq!(response.base_block_reason, None);
    assert!(response.has_uncommitted_changes);
    assert_eq!(response.unpublished_commit_count, Some(2));
    assert_eq!(response.recommended_actions, None);

    let merged_source_pr = BaseResolutionResult::retargeted_merged_source_pull_request(
        "feature/source-pr".to_string(),
        "main".to_string(),
        "origin/main".to_string(),
        "main-sha".to_string(),
        42,
    );
    let merged_response = AgentConversationWorkspaceFreshnessResponse::from_target_status(
        "conversation-command-base".to_string(),
        AgentWorkspaceFreshnessScope::Full,
        "feature/source-pr".to_string(),
        Some("feature/source-pr".to_string()),
        Some(&merged_source_pr),
        status.clone(),
        true,
        Some(2),
        true,
        true,
    );
    assert_eq!(
        merged_response.recommended_actions,
        Some(vec![
            "update_from_base".to_string(),
            "base_pr_merged".to_string(),
        ])
    );

    let fallback = AgentConversationWorkspaceFreshnessResponse::from_target_status(
        "conversation-command-base".to_string(),
        AgentWorkspaceFreshnessScope::Full,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        None,
        status,
        false,
        Some(0),
        true,
        true,
    );
    assert_eq!(fallback.base_status, "valid");
    assert_eq!(fallback.effective_base_ref.as_deref(), Some("main"));
    assert_eq!(
        fallback.effective_base_display_name.as_deref(),
        Some("Project default (main)")
    );

    let workspace = command_test_workspace();
    let blocked = blocked_base_resolution(BLOCK_REASON_MISSING_BASE_COMMIT);
    let blocked_response = AgentConversationWorkspaceFreshnessResponse::blocked(
        "conversation-command-base".to_string(),
        AgentWorkspaceFreshnessScope::Full,
        &workspace,
        &blocked,
        true,
        Some(1),
        true,
        true,
    );
    assert_eq!(blocked_response.base_status, "blocked");
    assert_eq!(
        blocked_response.base_block_reason.as_deref(),
        Some(BLOCK_REASON_MISSING_BASE_COMMIT)
    );
    assert_eq!(blocked_response.effective_base_ref, None);
    assert_eq!(blocked_response.target_ref, "");
}

#[test]
fn workspace_freshness_cache_status_labels_are_stable() {
    assert_eq!(AgentWorkspaceFreshnessCacheStatus::Hit.as_str(), "hit");
    assert_eq!(
        AgentWorkspaceFreshnessCacheStatus::Coalesced.as_str(),
        "coalesced"
    );
    assert_eq!(AgentWorkspaceFreshnessCacheStatus::Miss.as_str(), "miss");
}

#[test]
fn workspace_freshness_cache_hits_and_invalidates_recent_response() {
    let conversation_id =
        ChatConversationId::from_string("77777777-7777-4777-8777-777777777777".to_string());
    invalidate_agent_workspace_freshness_cache(&conversation_id);
    assert!(
        cached_agent_workspace_freshness(&conversation_id, AgentWorkspaceFreshnessScope::Full)
            .is_none()
    );

    let response = AgentConversationWorkspaceFreshnessResponse::from_target_status(
        conversation_id.as_str().to_string(),
        AgentWorkspaceFreshnessScope::Full,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        None,
        PublishBranchFreshnessStatus {
            target_ref: "origin/main".to_string(),
            captured_base_commit: Some("old-base-sha".to_string()),
            target_base_commit: "main-sha".to_string(),
            is_base_ahead: true,
        },
        false,
        Some(1),
        true,
        true,
    );
    store_agent_workspace_freshness(
        &conversation_id,
        AgentWorkspaceFreshnessScope::Full,
        &response,
    );

    let cached =
        cached_agent_workspace_freshness(&conversation_id, AgentWorkspaceFreshnessScope::Full)
            .expect("recent freshness response should be cached");
    assert_eq!(cached.conversation_id, response.conversation_id);
    assert_eq!(cached.target_base_commit, "main-sha");
    assert!(cached.is_base_ahead);

    invalidate_agent_workspace_freshness_cache(&conversation_id);
    assert!(
        cached_agent_workspace_freshness(&conversation_id, AgentWorkspaceFreshnessScope::Full)
            .is_none()
    );
}

#[test]
fn workspace_freshness_cache_keeps_local_and_full_scopes_separate() {
    let conversation_id =
        ChatConversationId::from_string("78777777-7777-4777-8777-777777777777".to_string());
    invalidate_agent_workspace_freshness_cache(&conversation_id);
    let local = AgentConversationWorkspaceFreshnessResponse::from_local_summary(
        conversation_id.as_str(),
        "main".to_string(),
        Some("Project default (main)".to_string()),
        "ralphx/test/workspace".to_string(),
        Some("base-sha".to_string()),
    );
    let full = AgentConversationWorkspaceFreshnessResponse::from_target_status(
        conversation_id.as_str(),
        AgentWorkspaceFreshnessScope::Full,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        None,
        PublishBranchFreshnessStatus {
            target_ref: "origin/main".to_string(),
            captured_base_commit: Some("base-sha".to_string()),
            target_base_commit: "new-main-sha".to_string(),
            is_base_ahead: true,
        },
        false,
        Some(3),
        true,
        true,
    );

    store_agent_workspace_freshness(
        &conversation_id,
        AgentWorkspaceFreshnessScope::Local,
        &local,
    );
    store_agent_workspace_freshness(&conversation_id, AgentWorkspaceFreshnessScope::Full, &full);

    let cached_local =
        cached_agent_workspace_freshness(&conversation_id, AgentWorkspaceFreshnessScope::Local)
            .expect("local response should be cached");
    let cached_full =
        cached_agent_workspace_freshness(&conversation_id, AgentWorkspaceFreshnessScope::Full)
            .expect("full response should be cached");

    assert_eq!(cached_local.freshness_scope, "local");
    assert_eq!(cached_local.target_base_commit, "base-sha");
    assert_eq!(cached_full.freshness_scope, "full");
    assert_eq!(cached_full.target_base_commit, "new-main-sha");
}

#[test]
fn workspace_freshness_cache_expires_stale_entries() {
    let conversation_id = ChatConversationId::from_string("87777777-7777-4777-8777-777777777777");
    invalidate_agent_workspace_freshness_cache(&conversation_id);
    let response = AgentConversationWorkspaceFreshnessResponse::from_target_status(
        conversation_id.as_str(),
        AgentWorkspaceFreshnessScope::Full,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        None,
        PublishBranchFreshnessStatus {
            target_ref: "origin/main".to_string(),
            captured_base_commit: Some("old-base-sha".to_string()),
            target_base_commit: "main-sha".to_string(),
            is_base_ahead: false,
        },
        false,
        Some(0),
        true,
        true,
    );
    let key =
        agent_workspace_freshness_cache_key(&conversation_id, AgentWorkspaceFreshnessScope::Full)
            .expect("conversation id should be cacheable");
    agent_workspace_freshness_cache().insert(
        key.clone(),
        AgentWorkspaceFreshnessCacheEntry {
            inserted_at: Instant::now()
                .checked_sub(Duration::from_secs(60))
                .expect("stale instant should be representable"),
            response,
        },
    );

    assert!(
        cached_agent_workspace_freshness(&conversation_id, AgentWorkspaceFreshnessScope::Full)
            .is_none()
    );
    assert!(!agent_workspace_freshness_cache().contains_key(&key));
}

#[test]
fn workspace_freshness_invalidation_guard_clears_cache_on_create_and_drop() {
    let conversation_id = ChatConversationId::from_string("97777777-7777-4777-8777-777777777777");
    let response = AgentConversationWorkspaceFreshnessResponse::from_target_status(
        conversation_id.as_str(),
        AgentWorkspaceFreshnessScope::Full,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        None,
        PublishBranchFreshnessStatus {
            target_ref: "origin/main".to_string(),
            captured_base_commit: Some("old-base-sha".to_string()),
            target_base_commit: "main-sha".to_string(),
            is_base_ahead: false,
        },
        false,
        Some(0),
        true,
        true,
    );

    store_agent_workspace_freshness(
        &conversation_id,
        AgentWorkspaceFreshnessScope::Full,
        &response,
    );
    assert!(
        cached_agent_workspace_freshness(&conversation_id, AgentWorkspaceFreshnessScope::Full)
            .is_some()
    );
    {
        let _guard = AgentWorkspaceFreshnessInvalidationGuard::new(&conversation_id);
        assert!(cached_agent_workspace_freshness(
            &conversation_id,
            AgentWorkspaceFreshnessScope::Full
        )
        .is_none());
        store_agent_workspace_freshness(
            &conversation_id,
            AgentWorkspaceFreshnessScope::Full,
            &response,
        );
        assert!(cached_agent_workspace_freshness(
            &conversation_id,
            AgentWorkspaceFreshnessScope::Full
        )
        .is_some());
    }
    assert!(
        cached_agent_workspace_freshness(&conversation_id, AgentWorkspaceFreshnessScope::Full)
            .is_none()
    );
}

#[test]
fn pr_description_invalidation_guard_can_defer_initial_invalidation() {
    let conversation_id = ChatConversationId::from_string("a7777777-7777-4777-8777-777777777777");
    let _guard = AgentWorkspacePrDescriptionInvalidationGuard::new(&conversation_id, false);
}

#[test]
fn workspace_freshness_cache_skips_nil_conversation_ids() {
    let conversation_id = ChatConversationId::from_string("not-a-uuid");
    assert!(conversation_id.as_uuid().is_nil());

    let response = AgentConversationWorkspaceFreshnessResponse::from_target_status(
        conversation_id.as_str(),
        AgentWorkspaceFreshnessScope::Full,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        None,
        PublishBranchFreshnessStatus {
            target_ref: "origin/main".to_string(),
            captured_base_commit: None,
            target_base_commit: "main-sha".to_string(),
            is_base_ahead: false,
        },
        false,
        Some(0),
        true,
        true,
    );
    store_agent_workspace_freshness(
        &conversation_id,
        AgentWorkspaceFreshnessScope::Full,
        &response,
    );

    assert!(
        cached_agent_workspace_freshness(&conversation_id, AgentWorkspaceFreshnessScope::Full)
            .is_none()
    );
}

#[tokio::test]
async fn workspace_freshness_command_blocks_stale_base_without_commit() {
    let (_temp, state, conversation_id, _github) = setup_publish_command_state(
        "freshness-blocked",
        false,
        None,
        Arc::new(MockGithubService::new()),
    )
    .await;
    let app = mock_builder()
        .manage(state)
        .build(mock_context(noop_assets()))
        .expect("mock app should build");

    let response = get_agent_conversation_workspace_freshness(
        conversation_id.as_str(),
        Some("full".to_string()),
        app.state(),
    )
    .await
    .expect("freshness should return blocked state");

    assert_eq!(response.base_status, "blocked");
    assert_eq!(response.base_ref, "feature/deleted-base");
    assert_eq!(response.effective_base_ref, None);
    assert_eq!(
        response.base_block_reason.as_deref(),
        Some(BLOCK_REASON_MISSING_BASE_COMMIT)
    );
    assert_eq!(response.target_ref, "");
}

#[tokio::test]
async fn workspace_freshness_command_reports_retargeted_base() {
    let (_temp, state, conversation_id, _github) = setup_publish_command_state(
        "freshness-retargeted",
        true,
        None,
        Arc::new(MockGithubService::new()),
    )
    .await;
    let app = mock_builder()
        .manage(state)
        .build(mock_context(noop_assets()))
        .expect("mock app should build");

    let response = get_agent_conversation_workspace_freshness(
        conversation_id.as_str(),
        Some("full".to_string()),
        app.state(),
    )
    .await
    .expect("freshness should resolve retargeted base");

    assert_eq!(response.base_status, "retargeted");
    assert_eq!(response.base_ref, "feature/deleted-base");
    assert_eq!(response.effective_base_ref.as_deref(), Some("main"));
    assert_eq!(
        response.effective_base_display_name.as_deref(),
        Some("Project default (main)")
    );
    assert_eq!(response.target_ref, "main");
    assert!(!response.is_base_ahead);
}

#[tokio::test]
async fn plan_workspace_full_freshness_reports_current_and_behind_base() {
    let (temp, state, conversation_id, _github) = setup_publish_command_state_with_mode(
        "plan-freshness",
        true,
        None,
        Arc::new(MockGithubService::new()),
        AgentConversationWorkspaceMode::Plan,
    )
    .await;

    let current = super::get_agent_conversation_workspace_freshness_for_app_state(
        &conversation_id,
        Some("full"),
        &state,
    )
    .await
    .expect("Plan workspace freshness should load when current");
    assert!(!current.is_base_ahead);

    let repo_path = temp.path().join("repo");
    commit_file(
        &repo_path,
        "base-change.txt",
        "base change\n",
        "advance base branch",
    );
    invalidate_agent_workspace_freshness_cache(&conversation_id);

    let behind = super::get_agent_conversation_workspace_freshness_for_app_state(
        &conversation_id,
        Some("full"),
        &state,
    )
    .await
    .expect("Plan workspace freshness should load when behind");
    assert!(behind.is_base_ahead);
}

#[tokio::test]
async fn workspace_freshness_rejects_chat_mode() {
    let (_temp, state, conversation_id, _github) = setup_publish_command_state(
        "freshness-chat-mode",
        true,
        None,
        Arc::new(MockGithubService::new()),
    )
    .await;
    let mut workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    workspace.mode = AgentConversationWorkspaceMode::Chat;
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("Chat workspace should persist");

    let error = super::get_agent_conversation_workspace_freshness_for_app_state(
        &conversation_id,
        Some("full"),
        &state,
    )
    .await
    .expect_err("Chat workspaces must not support freshness");

    assert!(error.contains("Only edit and plan workspaces"));
}

#[tokio::test]
async fn plan_workspace_base_update_refreshes_full_freshness() {
    let (temp, state, conversation_id, _github) = setup_publish_command_state_with_mode(
        "plan-freshness-update",
        true,
        None,
        Arc::new(MockGithubService::new()),
        AgentConversationWorkspaceMode::Plan,
    )
    .await;
    let repo_path = temp.path().join("repo");
    commit_file(
        &repo_path,
        "base-change.txt",
        "base change\n",
        "advance base branch",
    );

    let behind = super::get_agent_conversation_workspace_freshness_for_app_state(
        &conversation_id,
        Some("full"),
        &state,
    )
    .await
    .expect("Plan workspace freshness should load before updating");
    assert!(behind.is_base_ahead);

    let response = update_agent_conversation_workspace_from_base_for_app_state(
        &state,
        &Arc::new(ExecutionState::new()),
        conversation_id.clone(),
        AgentConversationWorkspaceBaseSelection {
            kind: None,
            branch_mode: None,
            base_ref: None,
            display_name: None,
            source_pull_request: None,
        },
    )
    .await
    .expect("Plan workspace base update should succeed");
    assert!(response.updated);

    let current = super::get_agent_conversation_workspace_freshness_for_app_state(
        &conversation_id,
        Some("full"),
        &state,
    )
    .await
    .expect("Plan workspace freshness should load after updating");
    assert!(!current.is_base_ahead);
}

#[tokio::test]
async fn workspace_freshness_command_caches_local_summary_after_first_lookup() {
    let (_temp, state, conversation_id, _github) = setup_publish_command_state(
        "freshness-local-cache",
        true,
        None,
        Arc::new(MockGithubService::new()),
    )
    .await;
    let app = mock_builder()
        .manage(state)
        .build(mock_context(noop_assets()))
        .expect("mock app should build");

    let first = get_agent_conversation_workspace_freshness(
        conversation_id.as_str(),
        Some("local".to_string()),
        app.state(),
    )
    .await
    .expect("local freshness should load");
    assert_eq!(first.freshness_scope, "local");
    assert_eq!(first.base_ref, "feature/deleted-base");
    assert!(first.target_ref.starts_with("ralphx/"));
    assert!(!first.remote_refreshed);
    assert!(!first.worktree_status_checked);

    let second = get_agent_conversation_workspace_freshness(
        conversation_id.as_str(),
        Some("local".to_string()),
        app.state(),
    )
    .await
    .expect("cached local freshness should load");

    assert_eq!(second.conversation_id, first.conversation_id);
    assert_eq!(second.freshness_scope, "local");
    assert_eq!(second.target_base_commit, first.target_base_commit);
    assert_eq!(second.target_ref, first.target_ref);
}

#[tokio::test]
async fn workspace_freshness_command_treats_merged_missing_workspace_as_terminal() {
    let (_temp, state, conversation_id, _github) = setup_publish_command_state(
        "freshness-merged-missing",
        true,
        Some(243),
        Arc::new(MockGithubService::new()),
    )
    .await;
    let mut workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    let worktree_path = PathBuf::from(&workspace.worktree_path);
    workspace.publication_pr_status = Some("merged".to_string());
    workspace.publication_push_status = Some("pushed".to_string());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should update");
    std::fs::remove_dir_all(&worktree_path).expect("worktree should be removed");

    let app = mock_builder()
        .manage(state)
        .build(mock_context(noop_assets()))
        .expect("mock app should build");

    let response = get_agent_conversation_workspace_freshness(
        conversation_id.as_str(),
        Some("local".to_string()),
        app.state(),
    )
    .await
    .expect("terminal workspace freshness should not require the removed worktree");

    assert_eq!(response.freshness_scope, "local");
    assert_eq!(response.base_status, "valid");
    assert_eq!(response.unpublished_commit_count, Some(0));
    assert!(!response.remote_refreshed);
    assert!(!response.worktree_status_checked);
}

#[tokio::test]
async fn update_workspace_from_explicit_base_recovers_blocked_base() {
    let (temp, state, conversation_id, _github) = setup_publish_command_state(
        "explicit-base-recovery",
        true,
        None,
        Arc::new(MockGithubService::new()),
    )
    .await;
    let repo_path = temp.path().join("repo");
    git(&repo_path, &["checkout", "-b", "release/0.8"]);
    std::fs::write(repo_path.join("release.txt"), "release\n")
        .expect("release fixture should be written");
    git(&repo_path, &["add", "release.txt"]);
    git(&repo_path, &["commit", "-m", "release base"]);
    let release_sha = git(&repo_path, &["rev-parse", "HEAD"]);
    git(&repo_path, &["checkout", "main"]);
    git(&repo_path, &["checkout", "--orphan", "rewritten-main"]);
    git(&repo_path, &["rm", "-rf", "."]);
    std::fs::write(repo_path.join("README.md"), "rewritten\n")
        .expect("rewritten fixture should be written");
    git(&repo_path, &["add", "README.md"]);
    git(&repo_path, &["commit", "-m", "rewrite main"]);
    git(&repo_path, &["branch", "-M", "main"]);

    let execution_state = Arc::new(ExecutionState::new());
    let app = mock_builder()
        .manage(state)
        .manage(execution_state)
        .build(mock_context(noop_assets()))
        .expect("mock app should build");
    let blocked = get_agent_conversation_workspace_freshness(
        conversation_id.as_str(),
        Some("full".to_string()),
        app.state(),
    )
    .await
    .expect("freshness should load");
    assert_eq!(blocked.base_status, "blocked");

    let response = update_agent_conversation_workspace_from_base_for_app_state(
        app.state::<AppState>().inner(),
        app.state::<Arc<ExecutionState>>().inner(),
        conversation_id.clone(),
        AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::LocalBranch),
            branch_mode: None,
            base_ref: Some("release/0.8".to_string()),
            display_name: Some("release/0.8".to_string()),
            source_pull_request: None,
        },
    )
    .await
    .expect("explicit base update should recover workspace");

    assert!(response.updated);
    assert_eq!(response.base_status, "valid");
    assert_eq!(response.target_ref, "release/0.8");
    assert_eq!(response.base_commit, release_sha);
    assert_eq!(response.workspace.base_ref_kind, "local_branch");
    assert_eq!(response.workspace.base_ref, "release/0.8");
    assert_eq!(
        response.workspace.base_display_name.as_deref(),
        Some("release/0.8")
    );
    assert_eq!(
        response.workspace.base_commit.as_deref(),
        Some(release_sha.as_str())
    );
}

#[tokio::test]
async fn update_workspace_from_base_running_conversation_does_not_stick_refreshing() {
    let (_temp, state, conversation_id, _github) = setup_publish_command_state(
        "update-running-conversation",
        true,
        None,
        Arc::new(MockGithubService::new()),
    )
    .await;
    let execution_state = Arc::new(ExecutionState::new());
    state
        .running_agent_registry
        .register(
            RunningAgentKey::new(
                ChatContextType::Project.to_string(),
                conversation_id.as_str(),
            ),
            123,
            conversation_id.as_str(),
            "run-update-base".to_string(),
            None,
            None,
        )
        .await;

    let result = update_agent_conversation_workspace_from_base_for_app_state(
        &state,
        &execution_state,
        conversation_id.clone(),
        AgentConversationWorkspaceBaseSelection {
            kind: None,
            branch_mode: None,
            base_ref: None,
            display_name: None,
            source_pull_request: None,
        },
    )
    .await
    .expect("running conversation should allow workspace base update");

    assert_eq!(result.workspace.conversation_id, conversation_id.as_str());
    let stored = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert_ne!(
        stored.publication_push_status.as_deref(),
        Some("refreshing")
    );
}

#[tokio::test]
async fn update_workspace_from_base_succeeds_when_agent_is_running() {
    let (_temp, state, conversation_id, _github) = setup_publish_command_state(
        "update-running-conversation-allowed",
        true,
        None,
        Arc::new(MockGithubService::new()),
    )
    .await;
    let execution_state = Arc::new(ExecutionState::new());
    state
        .running_agent_registry
        .register(
            RunningAgentKey::new(
                ChatContextType::Project.to_string(),
                conversation_id.as_str(),
            ),
            123,
            conversation_id.as_str(),
            "run-update-base".to_string(),
            None,
            None,
        )
        .await;

    let result = update_agent_conversation_workspace_from_base_for_app_state(
        &state,
        &execution_state,
        conversation_id.clone(),
        AgentConversationWorkspaceBaseSelection {
            kind: None,
            branch_mode: None,
            base_ref: None,
            display_name: None,
            source_pull_request: None,
        },
    )
    .await
    .expect("running conversation should allow workspace base update");

    assert_eq!(result.workspace.conversation_id, conversation_id.as_str());
}

#[tokio::test]
async fn update_workspace_from_base_allows_interactive_idle_conversation() {
    let (_temp, state, conversation_id, _github) = setup_publish_command_state(
        "update-interactive-idle-conversation",
        true,
        None,
        Arc::new(MockGithubService::new()),
    )
    .await;
    let execution_state = Arc::new(ExecutionState::new());
    state
        .running_agent_registry
        .register(
            RunningAgentKey::new(
                ChatContextType::Project.to_string(),
                conversation_id.as_str(),
            ),
            123,
            conversation_id.as_str(),
            "run-update-base-idle".to_string(),
            None,
            None,
        )
        .await;
    execution_state.mark_interactive_idle(&format!(
        "{}/{}",
        ChatContextType::Project,
        conversation_id.as_str()
    ));

    let result = update_agent_conversation_workspace_from_base_for_app_state(
        &state,
        &execution_state,
        conversation_id.clone(),
        AgentConversationWorkspaceBaseSelection {
            kind: None,
            branch_mode: None,
            base_ref: None,
            display_name: None,
            source_pull_request: None,
        },
    )
    .await
    .expect("interactive-idle conversation should allow workspace base update");

    assert_eq!(result.workspace.conversation_id, conversation_id.as_str());
}

#[tokio::test]
async fn update_workspace_from_base_pr_selection_persists_source_pull_request() {
    let (temp, state, conversation_id, _github) = setup_publish_command_state(
        "update-pr-base",
        true,
        None,
        Arc::new(MockGithubService::new()),
    )
    .await;
    let repo_path = temp.path().join("repo");
    let head = git(&repo_path, &["rev-parse", "HEAD"]);
    git(
        &repo_path,
        &["update-ref", "refs/heads/feature/pr-base", &head],
    );
    let execution_state = Arc::new(ExecutionState::new());
    let source_pull_request = AgentWorkspaceSourcePullRequest {
        number: 42,
        url: Some("https://github.com/mock/repo/pull/42".to_string()),
        title: Some("Add PR base".to_string()),
        head_ref_name: "feature/pr-base".to_string(),
        base_ref_name: Some("main".to_string()),
        head_ref_oid: Some("pr-head-sha".to_string()),
    };

    let result = update_agent_conversation_workspace_from_base_for_app_state(
        &state,
        &execution_state,
        conversation_id.clone(),
        AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::LocalBranch),
            branch_mode: None,
            base_ref: Some("feature/pr-base".to_string()),
            display_name: Some("PR #42: Add PR base".to_string()),
            source_pull_request: Some(source_pull_request.clone()),
        },
    )
    .await
    .expect("PR-backed base update should succeed");

    assert_eq!(result.workspace.base_ref_kind, "local_branch");
    assert_eq!(result.workspace.base_ref, "feature/pr-base");
    assert_eq!(
        result.workspace.base_display_name.as_deref(),
        Some("PR #42: Add PR base")
    );
    let response_source = result
        .workspace
        .source_pull_request
        .as_ref()
        .expect("response should include source PR metadata");
    assert_eq!(response_source.number, 42);
    assert_eq!(response_source.head_ref_name, "feature/pr-base");

    let stored = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert_eq!(stored.source_pull_request, Some(source_pull_request));
}

#[tokio::test]
async fn update_workspace_from_base_pr_selection_fetches_remote_head_before_validation() {
    let (temp, state, conversation_id, _github) = setup_publish_command_state(
        "update-pr-base-remote-only",
        true,
        None,
        Arc::new(MockGithubService::new()),
    )
    .await;
    let repo_path = temp.path().join("repo");
    let origin_path = temp.path().join("origin.git");
    git(
        &repo_path,
        &["init", "--bare", origin_path.to_str().expect("origin path")],
    );
    git(
        &repo_path,
        &[
            "remote",
            "add",
            "origin",
            origin_path.to_str().expect("origin path"),
        ],
    );
    git(&repo_path, &["push", "origin", "main"]);
    git(&repo_path, &["checkout", "-b", "feature/pr-remote-only"]);
    std::fs::write(repo_path.join("pr.txt"), "remote pr head\n")
        .expect("fixture file should be written");
    git(&repo_path, &["add", "pr.txt"]);
    git(&repo_path, &["commit", "-m", "remote pr head"]);
    let pr_head = git(&repo_path, &["rev-parse", "HEAD"]);
    git(&repo_path, &["push", "origin", "feature/pr-remote-only"]);
    git(&repo_path, &["checkout", "main"]);
    git(&repo_path, &["branch", "-D", "feature/pr-remote-only"]);
    git(
        &repo_path,
        &[
            "update-ref",
            "-d",
            "refs/remotes/origin/feature/pr-remote-only",
        ],
    );
    assert!(
        !GitService::ref_exists(&repo_path, "feature/pr-remote-only")
            .await
            .expect("local branch check should succeed")
    );
    assert!(
        !GitService::ref_exists(&repo_path, "origin/feature/pr-remote-only")
            .await
            .expect("remote tracking check should succeed")
    );
    let execution_state = Arc::new(ExecutionState::new());

    let result = update_agent_conversation_workspace_from_base_for_app_state(
        &state,
        &execution_state,
        conversation_id.clone(),
        AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::LocalBranch),
            branch_mode: None,
            base_ref: Some("feature/pr-remote-only".to_string()),
            display_name: Some("PR #43: Remote-only PR base".to_string()),
            source_pull_request: Some(AgentWorkspaceSourcePullRequest {
                number: 43,
                url: Some("https://github.com/mock/repo/pull/43".to_string()),
                title: Some("Remote-only PR base".to_string()),
                head_ref_name: "feature/pr-remote-only".to_string(),
                base_ref_name: Some("main".to_string()),
                head_ref_oid: Some(pr_head),
            }),
        },
    )
    .await
    .expect("PR-backed remote-only base update should fetch and succeed");

    assert_eq!(result.workspace.base_ref, "feature/pr-remote-only");
    assert!(
        GitService::ref_exists(&repo_path, "origin/feature/pr-remote-only")
            .await
            .expect("remote tracking check should succeed after update")
    );
}

#[tokio::test]
async fn update_workspace_from_saved_base_retargets_to_project_default() {
    let (_temp, state, conversation_id, _github) = setup_publish_command_state(
        "saved-base-retarget",
        true,
        None,
        Arc::new(MockGithubService::new()),
    )
    .await;
    let execution_state = Arc::new(ExecutionState::new());
    let app = mock_builder()
        .manage(state)
        .manage(execution_state)
        .build(mock_context(noop_assets()))
        .expect("mock app should build");

    let freshness = get_agent_conversation_workspace_freshness(
        conversation_id.as_str(),
        Some("full".to_string()),
        app.state(),
    )
    .await
    .expect("freshness should resolve retargeted base");
    assert_eq!(freshness.base_status, "retargeted");

    let response = update_agent_conversation_workspace_from_base_for_app_state(
        app.state::<AppState>().inner(),
        app.state::<Arc<ExecutionState>>().inner(),
        conversation_id.clone(),
        AgentConversationWorkspaceBaseSelection {
            kind: None,
            branch_mode: None,
            base_ref: None,
            display_name: None,
            source_pull_request: None,
        },
    )
    .await
    .expect("saved-base update should retarget workspace");

    assert!(!response.updated);
    assert_eq!(response.base_status, "retargeted");
    assert_eq!(response.target_ref, "main");
    assert_eq!(response.workspace.base_ref_kind, "project_default");
    assert_eq!(response.workspace.base_ref, "main");
    assert_eq!(
        response.effective_base_display_name.as_deref(),
        Some("Project default (main)")
    );
    assert_eq!(
        response.workspace.base_display_name.as_deref(),
        Some("Project default (main)")
    );
}

#[tokio::test]
async fn update_ideation_workspace_from_base_refuses_primary_checkout_plan_branch() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    let base_sha = setup_publish_repo(&repo_path);
    let plan_branch_name = "feature/plan-primary-checkout";

    git(&repo_path, &["checkout", "-b", plan_branch_name]);
    git(&repo_path, &["checkout", "main"]);
    std::fs::write(repo_path.join("fix.txt"), "base fix\n")
        .expect("fixture file should be written");
    git(&repo_path, &["add", "fix.txt"]);
    git(&repo_path, &["commit", "-m", "base fix"]);
    let main_sha = git(&repo_path, &["rev-parse", "HEAD"]);
    git(&repo_path, &["checkout", plan_branch_name]);

    let mut project = Project::new(
        "Primary Checkout Plan Update".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
    let conversation_id = ChatConversationId::from_string("conversation-plan-primary-checkout");
    let mut plan_branch = PlanBranch::new(
        ArtifactId::from_string("artifact-primary-checkout"),
        IdeationSessionId::from_string("session-primary-checkout"),
        project.id.clone(),
        plan_branch_name.to_string(),
        "main".to_string(),
    );
    plan_branch.status = PlanBranchStatus::Active;
    let plan_branch_id = plan_branch.id.clone();
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id.clone(),
        project.id.clone(),
        AgentConversationWorkspaceMode::Ideation,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some(base_sha.clone()),
        "agent-shell-primary-checkout".to_string(),
        temp.path()
            .join("agent-shell-primary-checkout")
            .to_string_lossy()
            .to_string(),
    );
    workspace.linked_ideation_session_id = Some(plan_branch.session_id.clone());
    workspace.linked_plan_branch_id = Some(plan_branch_id.clone());

    let state = AppState::new_test();
    state
        .project_repo
        .create(project.clone())
        .await
        .expect("project should be persisted");
    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.id = conversation_id.clone();
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("conversation should be persisted");
    state
        .plan_branch_repo
        .create(plan_branch)
        .await
        .expect("plan branch should be persisted");
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should be persisted");

    let execution_state = Arc::new(ExecutionState::new());
    let error = update_agent_conversation_workspace_from_base_for_app_state(
        &state,
        &execution_state,
        conversation_id.clone(),
        AgentConversationWorkspaceBaseSelection {
            kind: None,
            branch_mode: None,
            base_ref: None,
            display_name: None,
            source_pull_request: None,
        },
    )
    .await
    .expect_err("primary checkout plan branch should not be updated in place");

    assert!(
        error.to_ascii_lowercase().contains("primary checkout"),
        "unexpected primary checkout refusal: {error}"
    );
    assert_eq!(
        git(&repo_path, &["branch", "--show-current"]),
        plan_branch_name
    );
    assert!(!repo_path.join("fix.txt").exists());
    assert_eq!(git(&repo_path, &["rev-parse", "main"]), main_sha);
    assert_eq!(git(&repo_path, &["rev-parse", plan_branch_name]), base_sha);
    let stored = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert_eq!(stored.publication_push_status.as_deref(), Some("failed"));
}

#[tokio::test]
async fn update_ideation_workspace_from_base_updates_linked_plan_worktree() {
    let (temp, state, conversation_id, plan_branch_id, github) =
        setup_linked_plan_publish_command_state(
            "base-update",
            false,
            Arc::new(MockGithubService::new()),
        )
        .await;
    let repo_path = temp.path().join("repo");
    std::fs::write(repo_path.join("base-fix.txt"), "base fix\n")
        .expect("base fixture should be written");
    git(&repo_path, &["add", "base-fix.txt"]);
    git(&repo_path, &["commit", "-m", "base fix"]);
    let main_sha = git(&repo_path, &["rev-parse", "HEAD"]);
    assert_eq!(git(&repo_path, &["branch", "--show-current"]), "main");

    let execution_state = Arc::new(ExecutionState::new());
    let response = update_agent_conversation_workspace_from_base_for_app_state(
        &state,
        &execution_state,
        conversation_id.clone(),
        AgentConversationWorkspaceBaseSelection {
            kind: None,
            branch_mode: None,
            base_ref: None,
            display_name: None,
            source_pull_request: None,
        },
    )
    .await
    .expect("linked plan branch worktree should update from base");

    assert!(response.updated);
    assert_eq!(response.base_commit, main_sha);
    assert_eq!(response.target_ref, "origin/main");
    assert_eq!(
        response.workspace.publication_push_status.as_deref(),
        Some("pushed")
    );
    assert_eq!(github.state().push_branch_calls, 1);
    let project = state
        .project_repo
        .get_all()
        .await
        .expect("project lookup should succeed")
        .pop()
        .expect("project should exist");
    let plan_branch = state
        .plan_branch_repo
        .get_by_id(&plan_branch_id)
        .await
        .expect("plan branch lookup should succeed")
        .expect("plan branch should exist");
    assert_eq!(
        github.state().last_push_branch_name.as_deref(),
        Some(plan_branch.branch_name.as_str())
    );
    assert_eq!(plan_branch.pr_push_status, PrPushStatus::Pushed);
    let plan_worktree = ensure_linked_plan_branch_agent_worktree(&project, &plan_branch)
        .await
        .expect("linked plan worktree should resolve");
    git(
        &repo_path,
        &[
            "merge-base",
            "--is-ancestor",
            &main_sha,
            &plan_branch.branch_name,
        ],
    );
    assert_eq!(git(&repo_path, &["branch", "--show-current"]), "main");
    assert_eq!(git(&repo_path, &["status", "--short"]), "");
    assert_eq!(git(&plan_worktree, &["status", "--short"]), "");
}

#[tokio::test]
async fn update_workspace_from_saved_base_blocks_when_base_commit_is_missing() {
    let (_temp, state, conversation_id, github) = setup_publish_command_state(
        "update-missing-base",
        false,
        Some(987),
        Arc::new(MockGithubService::new()),
    )
    .await;
    let execution_state = Arc::new(ExecutionState::new());

    let error = update_agent_conversation_workspace_from_base_for_app_state(
        &state,
        &execution_state,
        conversation_id.clone(),
        AgentConversationWorkspaceBaseSelection {
            kind: None,
            branch_mode: None,
            base_ref: None,
            display_name: None,
            source_pull_request: None,
        },
    )
    .await
    .expect_err("missing saved base commit should block update");

    assert_eq!(error, BLOCK_REASON_MISSING_BASE_COMMIT);
    assert_eq!(github.state().update_pr_base_calls, 0);
    let stored = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert_eq!(stored.base_ref, "feature/deleted-base");
    assert_eq!(stored.publication_push_status.as_deref(), Some("failed"));
}

#[tokio::test]
async fn update_workspace_from_explicit_base_blocks_when_pr_retarget_fails() {
    let github = Arc::new(MockGithubService::new());
    {
        github.state().update_pr_base_result =
            Some(Err(AppError::Infrastructure("denied".to_string())));
    }
    let (temp, state, conversation_id, github) =
        setup_publish_command_state("update-explicit-retarget-fails", true, Some(988), github)
            .await;
    let repo_path = temp.path().join("repo");
    git(&repo_path, &["checkout", "-b", "release/0.8"]);
    git(&repo_path, &["checkout", "main"]);
    let execution_state = Arc::new(ExecutionState::new());

    let error = update_agent_conversation_workspace_from_base_for_app_state(
        &state,
        &execution_state,
        conversation_id.clone(),
        AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::LocalBranch),
            branch_mode: None,
            base_ref: Some("release/0.8".to_string()),
            display_name: Some("release/0.8".to_string()),
            source_pull_request: None,
        },
    )
    .await
    .expect_err("failed explicit-base PR retarget should block update");

    assert!(error.contains("Existing PR #988 targets the deleted branch"));
    assert_eq!(
        github.state().last_update_pr_base_args,
        Some((988, "release/0.8".to_string()))
    );
    let stored = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    // The explicit selection persists before the PR retarget so a later failure routes
    // repair at the user's chosen base instead of silently dropping the selection.
    assert_eq!(stored.base_ref, "release/0.8");
    assert_eq!(stored.publication_push_status.as_deref(), Some("failed"));
}

#[tokio::test]
async fn update_workspace_from_explicit_base_blocks_when_selection_is_missing() {
    let (_temp, state, conversation_id, github) = setup_publish_command_state(
        "update-explicit-missing-branch",
        true,
        Some(989),
        Arc::new(MockGithubService::new()),
    )
    .await;
    let execution_state = Arc::new(ExecutionState::new());

    let error = update_agent_conversation_workspace_from_base_for_app_state(
        &state,
        &execution_state,
        conversation_id.clone(),
        AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::LocalBranch),
            branch_mode: None,
            base_ref: Some("release/missing".to_string()),
            display_name: Some("release/missing".to_string()),
            source_pull_request: None,
        },
    )
    .await
    .expect_err("missing explicit branch should block before PR retarget");

    assert!(error.contains("Selected base branch 'release/missing' does not exist"));
    assert_eq!(github.state().update_pr_base_calls, 0);
    let stored = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert_eq!(stored.base_ref, "feature/deleted-base");
    assert_eq!(stored.publication_push_status.as_deref(), Some("failed"));
}

#[tokio::test]
async fn publish_linked_ideation_plan_branch_commits_and_pushes_existing_pr() {
    let (temp, state, conversation_id, plan_branch_id, github) =
        setup_linked_plan_publish_command_state(
            "success",
            false,
            Arc::new(MockGithubService::new()),
        )
        .await;
    let repo_path = temp.path().join("repo");
    let project = state
        .project_repo
        .get_all()
        .await
        .expect("project lookup should succeed")
        .pop()
        .expect("project should exist");
    let plan_branch = state
        .plan_branch_repo
        .get_by_id(&plan_branch_id)
        .await
        .expect("plan branch lookup should succeed")
        .expect("plan branch should exist");
    let plan_worktree = ensure_linked_plan_branch_agent_worktree(&project, &plan_branch)
        .await
        .expect("linked plan worktree should resolve");
    std::fs::write(plan_worktree.join("manual-fix.txt"), "manual follow-up\n")
        .expect("manual plan fix should be written");
    assert_eq!(git(&repo_path, &["branch", "--show-current"]), "main");
    assert_eq!(git(&repo_path, &["status", "--short"]), "");
    let execution_state = Arc::new(ExecutionState::new());

    let response = publish_agent_conversation_workspace_for_app_state(
        &state,
        &execution_state,
        conversation_id.clone(),
        false,
    )
    .await
    .expect("linked ideation plan publish should succeed");
    state
        .pr_poller_registry
        .stop_agent_workspace_polling(&conversation_id);

    assert_eq!(response.pr_number, Some(77));
    assert!(!response.created_pr);
    assert!(response.commit_sha.is_some());
    assert_eq!(
        response.workspace.publication_push_status.as_deref(),
        Some("pushed")
    );
    assert_eq!(github.state().push_branch_calls, 1);
    assert_eq!(
        github.state().last_push_branch_name.as_deref(),
        Some("feature/plan-publish-success")
    );
    assert_eq!(git(&repo_path, &["branch", "--show-current"]), "main");
    assert_eq!(git(&repo_path, &["status", "--short"]), "");
    assert_eq!(git(&plan_worktree, &["status", "--short"]), "");
    let stored_plan = state
        .plan_branch_repo
        .get_by_id(&plan_branch_id)
        .await
        .expect("plan branch lookup should succeed")
        .expect("plan branch should exist");
    assert_eq!(stored_plan.pr_push_status, PrPushStatus::Pushed);
    let stored_workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert_eq!(stored_workspace.publication_pr_number, Some(77));
    assert_eq!(
        stored_workspace.publication_push_status.as_deref(),
        Some("pushed")
    );
}

#[tokio::test]
async fn publish_linked_ideation_plan_branch_rejects_active_regular_tasks() {
    let (temp, state, conversation_id, _plan_branch_id, github) =
        setup_linked_plan_publish_command_state(
            "active-task",
            true,
            Arc::new(MockGithubService::new()),
        )
        .await;
    let repo_path = temp.path().join("repo");
    let project = state
        .project_repo
        .get_all()
        .await
        .expect("project lookup should succeed")
        .pop()
        .expect("project should exist");
    let plan_branch = state
        .plan_branch_repo
        .get_by_id(&_plan_branch_id)
        .await
        .expect("plan branch lookup should succeed")
        .expect("plan branch should exist");
    let plan_worktree = ensure_linked_plan_branch_agent_worktree(&project, &plan_branch)
        .await
        .expect("linked plan worktree should resolve");
    std::fs::write(plan_worktree.join("manual-fix.txt"), "manual follow-up\n")
        .expect("manual plan fix should be written");
    let execution_state = Arc::new(ExecutionState::new());

    let error = publish_agent_conversation_workspace_for_app_state(
        &state,
        &execution_state,
        conversation_id.clone(),
        false,
    )
    .await
    .expect_err("active regular task should retain publish ownership");

    assert!(error.contains("active task work"));
    assert_eq!(github.state().push_branch_calls, 0);
    assert_eq!(git(&repo_path, &["status", "--short"]), "");
    assert_ne!(git(&plan_worktree, &["status", "--short"]), "");
}

#[tokio::test]
async fn publish_workspace_rejects_concurrent_publish_attempt() {
    let (_temp, state, conversation_id, _github) = setup_publish_command_state(
        "concurrent-publish",
        true,
        None,
        Arc::new(MockGithubService::new()),
    )
    .await;
    let execution_state = Arc::new(ExecutionState::new());
    let _guard = try_acquire_agent_workspace_publish_guard(&conversation_id)
        .expect("test should acquire publish guard");

    let error = publish_agent_conversation_workspace_for_app_state(
        &state,
        &execution_state,
        conversation_id.clone(),
        false,
    )
    .await
    .expect_err("concurrent publish should be rejected");

    assert_eq!(error, AGENT_WORKSPACE_PUBLISH_IN_PROGRESS_MESSAGE);
}

#[tokio::test]
async fn new_pr_publish_without_origin_rejects_before_staging_or_publication_side_effects() {
    let (temp, state, conversation_id, github) = setup_publish_command_state(
        "no-origin-new-pr",
        true,
        None,
        Arc::new(MockGithubService::new()),
    )
    .await;
    let workspace = use_main_as_publish_base(&state, &conversation_id).await;
    let mut project = state
        .project_repo
        .get_by_id(&workspace.project_id)
        .await
        .expect("project lookup should succeed")
        .expect("project should exist");
    project.github_pr_enabled = true;
    state
        .project_repo
        .update(&project)
        .await
        .expect("stale preference should persist");
    let worktree = Path::new(&workspace.worktree_path);
    std::fs::write(worktree.join("pending.txt"), "must remain unstaged\n")
        .expect("workspace change should be written");
    seed_current_passing_workspace_review(&state, &conversation_id).await;
    let head_before = git(worktree, &["rev-parse", "HEAD"]);

    let error = publish_agent_conversation_workspace_for_app_state(
        &state,
        &Arc::new(ExecutionState::new()),
        conversation_id.clone(),
        true,
    )
    .await
    .expect_err("new PR publishing without origin must reject");

    assert!(
        error.contains("no GitHub origin"),
        "expected no-origin capability error, got: {error}"
    );
    assert_eq!(git(worktree, &["diff", "--cached", "--name-only"]), "");
    assert_eq!(git(worktree, &["rev-parse", "HEAD"]), head_before);
    assert_eq!(git(worktree, &["status", "--short"]), "?? pending.txt");
    assert!(state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("publication events should load")
        .is_empty());
    let stored = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert!(stored.publication_push_status.is_none());
    assert!(stored.publication_pr_status.is_none());
    assert!(stored.pr_supervision_status.is_none());
    assert!(stored.pr_supervision_summary.is_none());
    let github_state = github.state();
    assert_eq!(github_state.push_branch_calls, 0);
    assert_eq!(github_state.create_draft_pr_calls, 0);
    assert_eq!(github_state.find_pr_by_head_branch_calls, 0);
    drop(temp);
}

#[tokio::test]
async fn existing_pr_publish_bypasses_new_pr_origin_preflight() {
    let (_temp, state, conversation_id, _github) = setup_publish_command_state(
        "no-origin-existing-pr",
        true,
        Some(987),
        Arc::new(MockGithubService::new()),
    )
    .await;
    let workspace = use_main_as_publish_base(&state, &conversation_id).await;
    let mut project = state
        .project_repo
        .get_by_id(&workspace.project_id)
        .await
        .expect("project lookup should succeed")
        .expect("project should exist");
    project.github_pr_enabled = true;
    state
        .project_repo
        .update(&project)
        .await
        .expect("preference should persist");

    let error = publish_agent_conversation_workspace_for_app_state(
        &state,
        &Arc::new(ExecutionState::new()),
        conversation_id,
        false,
    )
    .await
    .expect_err("existing PR should proceed to its own origin-dependent operation");

    assert!(
        !error.contains("no GitHub origin"),
        "persisted PRs must bypass the new-PR capability gate"
    );
}

#[tokio::test]
async fn publish_workspace_clears_terminal_pr_identity_and_creates_a_fresh_draft() {
    let github = Arc::new(MockGithubService::new());
    let (temp, state, conversation_id, github) =
        setup_publish_command_state("terminal-pr", true, Some(333), github).await;
    let mut project = state
        .project_repo
        .get_all()
        .await
        .expect("projects load")
        .into_iter()
        .next()
        .expect("project exists");
    project.github_pr_enabled = true;
    state
        .project_repo
        .update(&project)
        .await
        .expect("GitHub-enabled project should persist");
    let fake_remote = temp.path().join("github-remote.git");
    git(
        Path::new(&project.working_directory),
        &[
            "clone",
            "--bare",
            &project.working_directory,
            fake_remote.to_str().expect("remote path should be UTF-8"),
        ],
    );
    let fake_ssh = temp.path().join("fake-github-ssh");
    std::fs::write(
        &fake_ssh,
        format!(
            "#!/bin/sh\nif [ \"$1\" = \"-G\" ]; then exit 0; fi\ncase \"$*\" in\n  *git-upload-pack*) exec git-upload-pack '{}' ;;\n  *git-receive-pack*) exec git-receive-pack '{}' ;;\nesac\nexit 2\n",
            fake_remote.display(),
            fake_remote.display(),
        ),
    )
    .expect("fake GitHub SSH transport should be written");
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = std::fs::metadata(&fake_ssh)
        .expect("fake GitHub SSH transport should exist")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&fake_ssh, permissions)
        .expect("fake GitHub SSH transport should be executable");
    git(
        Path::new(&project.working_directory),
        &[
            "config",
            "core.sshCommand",
            fake_ssh.to_str().expect("SSH path should be UTF-8"),
        ],
    );
    git(
        Path::new(&project.working_directory),
        &[
            "remote",
            "add",
            "origin",
            "git@github.com:ralphx/test-repository.git",
        ],
    );
    let mut workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    workspace.publication_pr_status = Some("merged".to_string());
    workspace.publication_push_status = Some("pushed".to_string());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace update should persist");
    std::fs::write(
        Path::new(&workspace.worktree_path).join("fresh-draft.txt"),
        "new work after the old PR merged\n",
    )
    .expect("workspace change should be written");
    seed_current_passing_workspace_review(&state, &conversation_id).await;
    let client = Arc::new(SubmittingPrDescriptionClient::new(
        Arc::clone(&state.agent_conversation_workspace_repo),
        conversation_id.clone(),
    ));
    let state = state.with_agent_client(client);
    let execution_state = Arc::new(ExecutionState::new());

    let response = publish_agent_conversation_workspace_for_app_state(
        &state,
        &execution_state,
        conversation_id.clone(),
        false,
    )
    .await
    .expect("publish over a terminal identity should create a fresh draft PR");
    state
        .pr_poller_registry
        .stop_agent_workspace_polling(&conversation_id);

    assert_eq!(github.state().create_draft_pr_calls, 1);
    let stored = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert_ne!(
        stored.publication_pr_number,
        Some(333),
        "the terminal identity must be replaced, not reused"
    );
    assert_ne!(
        stored.publication_pr_status.as_deref(),
        Some("merged"),
        "the fresh draft must not inherit the terminal status"
    );
    let events = state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("events should list");
    assert!(
        events
            .iter()
            .any(|event| event.step == "terminal_publication_identity_cleared"),
        "clearing the terminal identity must leave a durable event"
    );
    assert_eq!(
        response.workspace.publication_pr_number,
        stored.publication_pr_number
    );
}

#[tokio::test]
async fn publish_workspace_blocks_before_pr_mutation_when_base_commit_is_missing() {
    let (_temp, state, conversation_id, github) = setup_publish_command_state(
        "missing-base",
        false,
        Some(321),
        Arc::new(MockGithubService::new()),
    )
    .await;
    let execution_state = Arc::new(ExecutionState::new());

    let error = publish_agent_conversation_workspace_for_app_state(
        &state,
        &execution_state,
        conversation_id.clone(),
        false,
    )
    .await
    .expect_err("missing base commit should block publish");

    assert!(error.contains("missing its captured base commit"));
    assert_eq!(github.state().update_pr_base_calls, 0);
    let stored = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert_eq!(stored.base_ref, "feature/deleted-base");
}

#[tokio::test]
async fn publish_workspace_blocks_on_review_gate_before_push_when_base_is_valid() {
    let (_temp, state, conversation_id, github) = setup_publish_command_state(
        "review-required",
        true,
        Some(322),
        Arc::new(MockGithubService::new()),
    )
    .await;
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    std::fs::write(
        Path::new(&workspace.worktree_path).join("implementation.txt"),
        "change requiring review\n",
    )
    .expect("workspace change should be written");
    let execution_state = Arc::new(ExecutionState::new());

    let error = publish_agent_conversation_workspace_for_app_state(
        &state,
        &execution_state,
        conversation_id,
        false,
    )
    .await
    .expect_err("review gate should block publish");

    assert_eq!(error, "Workspace Review is required before publishing");
    assert_eq!(github.state().push_branch_calls, 0);
    assert_eq!(github.state().update_pr_base_calls, 0);
}

#[tokio::test]
async fn publish_workspace_allows_required_review_gate_when_policy_is_disabled() {
    let (_temp, state, conversation_id, github) = setup_publish_command_state(
        "review-disabled",
        true,
        Some(323),
        Arc::new(MockGithubService::new()),
    )
    .await;
    state
        .review_settings_repo
        .update_settings(&ReviewSettings {
            require_workspace_review: false,
            ..ReviewSettings::default()
        })
        .await
        .expect("review settings should update");
    let project = state
        .project_repo
        .get_all()
        .await
        .expect("projects load")
        .into_iter()
        .next()
        .expect("project exists");
    git(
        Path::new(&project.working_directory),
        &["remote", "add", "origin", &project.working_directory],
    );
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    for _ in 0..2 {
        github.queue_pr_detail(Ok(authoritative_pr_detail(
            323,
            workspace.branch_name.clone(),
            "Existing title",
            "Existing body",
        )));
    }
    std::fs::write(
        Path::new(&workspace.worktree_path).join("implementation.txt"),
        "change that would otherwise require review\n",
    )
    .expect("workspace change should be written");
    let client = Arc::new(SubmittingPrDescriptionClient::new(
        Arc::clone(&state.agent_conversation_workspace_repo),
        conversation_id.clone(),
    ));
    let state = state.with_agent_client(client);
    let execution_state = Arc::new(ExecutionState::new());

    let response = publish_agent_conversation_workspace_for_app_state(
        &state,
        &execution_state,
        conversation_id.clone(),
        false,
    )
    .await
    .expect("publish should succeed when workspace review policy is disabled");
    state
        .pr_poller_registry
        .stop_agent_workspace_polling(&conversation_id);

    assert_eq!(
        response.workspace.publication_push_status.as_deref(),
        Some("pushed")
    );
    assert_eq!(github.state().push_branch_calls, 1);
    assert_eq!(github.state().update_pr_base_calls, 1);
}

#[tokio::test]
async fn publish_workspace_blocks_when_existing_pr_base_retarget_fails() {
    let github = Arc::new(MockGithubService::new());
    {
        github.state().update_pr_base_result =
            Some(Err(AppError::Infrastructure("denied".to_string())));
    }
    let (_temp, state, conversation_id, github) =
        setup_publish_command_state("pr-retarget-fails", true, Some(654), github).await;
    let execution_state = Arc::new(ExecutionState::new());

    let error = publish_agent_conversation_workspace_for_app_state(
        &state,
        &execution_state,
        conversation_id.clone(),
        false,
    )
    .await
    .expect_err("failed PR base retarget should block publish");

    assert!(error.contains("Existing PR #654 targets the deleted branch"));
    assert_eq!(
        github.state().last_update_pr_base_args,
        Some((654, "main".to_string()))
    );
    let stored = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert_eq!(stored.base_ref, "feature/deleted-base");
}

#[test]
fn publication_event_status_helpers_include_description_states() {
    assert_eq!(
        publication_event_status_for_push_status("describing"),
        "started"
    );
    assert_eq!(
        publication_event_summary_for_push_status("describing"),
        "Drafting pull request description"
    );
    assert_eq!(
        publication_event_status_for_push_status("description_failed"),
        "failed"
    );
    assert_eq!(
        publication_event_summary_for_push_status("description_failed"),
        "Pull request description failed"
    );
}

#[tokio::test]
async fn publish_workspace_syncs_requested_auto_merge_before_returning() {
    let github = Arc::new(MockGithubService::new());
    let (temp, state, conversation_id, github) =
        setup_publish_command_state("auto-merge-publish", true, None, github).await;
    let mut project = state
        .project_repo
        .get_all()
        .await
        .expect("projects load")
        .into_iter()
        .next()
        .expect("project exists");
    project.github_pr_enabled = true;
    state
        .project_repo
        .update(&project)
        .await
        .expect("GitHub-enabled project should persist");
    let fake_remote = temp.path().join("github-remote.git");
    git(
        Path::new(&project.working_directory),
        &[
            "clone",
            "--bare",
            &project.working_directory,
            fake_remote.to_str().expect("remote path should be UTF-8"),
        ],
    );
    let fake_ssh = temp.path().join("fake-github-ssh");
    std::fs::write(
        &fake_ssh,
        format!(
            "#!/bin/sh\nif [ \"$1\" = \"-G\" ]; then exit 0; fi\ncase \"$*\" in\n  *git-upload-pack*) exec git-upload-pack '{}' ;;\n  *git-receive-pack*) exec git-receive-pack '{}' ;;\nesac\nexit 2\n",
            fake_remote.display(),
            fake_remote.display(),
        ),
    )
    .expect("fake GitHub SSH transport should be written");
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = std::fs::metadata(&fake_ssh)
        .expect("fake GitHub SSH transport should exist")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&fake_ssh, permissions)
        .expect("fake GitHub SSH transport should be executable");
    git(
        Path::new(&project.working_directory),
        &[
            "config",
            "core.sshCommand",
            fake_ssh.to_str().expect("SSH path should be UTF-8"),
        ],
    );
    git(
        Path::new(&project.working_directory),
        &[
            "remote",
            "add",
            "origin",
            "git@github.com:ralphx/test-repository.git",
        ],
    );
    let mut workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    workspace.pr_auto_merge_desired = true;
    workspace.pr_auto_merge_method = "rebase".to_string();
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace update should persist");
    std::fs::write(
        Path::new(&workspace.worktree_path).join("auto-merge.txt"),
        "ready for review\n",
    )
    .expect("workspace change should be written");
    seed_current_passing_workspace_review(&state, &conversation_id).await;
    github.state().fetch_pr_health_result = Some(Ok(PrHealth {
        sync_state: PrSyncState {
            status: GithubPrStatus::Open,
            merge_state_status: None,
            mergeable: None,
            is_draft: true,
            head_ref_name: workspace.branch_name.clone(),
            base_ref_name: "main".to_string(),
            head_ref_oid: None,
            base_ref_oid: None,
        },
        review_decision: None,
        checks: Vec::new(),
        issue_comments: Vec::new(),
        auto_merge_request: None,
    }));
    let client = Arc::new(SubmittingPrDescriptionClient::new(
        Arc::clone(&state.agent_conversation_workspace_repo),
        conversation_id.clone(),
    ));
    let state = state.with_agent_client(client);
    let execution_state = Arc::new(ExecutionState::new());

    let response = publish_agent_conversation_workspace_for_app_state(
        &state,
        &execution_state,
        conversation_id.clone(),
        false,
    )
    .await
    .expect("publish should succeed");
    state
        .pr_poller_registry
        .stop_agent_workspace_polling(&conversation_id);

    assert_eq!(
        response.workspace.publication_push_status.as_deref(),
        Some("pushed")
    );
    assert_eq!(response.workspace.pr_auto_merge_current, Some(true));
    assert_eq!(
        response.workspace.pr_supervision_status.as_deref(),
        Some("monitoring")
    );
    let github_state = github.state();
    assert!(github_state.fetch_pr_health_calls >= 1);
    assert!(github_state.mark_pr_ready_calls >= 1);
    assert!(github_state.enable_pr_auto_merge_calls >= 1);
    assert_eq!(
        github_state.last_enable_pr_auto_merge_args.as_ref(),
        Some(&(1, "rebase".to_string()))
    );
}

#[tokio::test]
async fn publish_workspace_records_waiting_when_auto_merge_sync_fails() {
    let github = Arc::new(MockGithubService::new());
    let (_temp, state, conversation_id, github) =
        setup_publish_command_state("auto-merge-publish-waiting", true, None, github).await;
    enable_github_pr_publishing(&state, &conversation_id).await;
    let mut workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    workspace.pr_auto_merge_desired = true;
    workspace.pr_auto_merge_method = "squash".to_string();
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace update should persist");
    std::fs::write(
        Path::new(&workspace.worktree_path).join("auto-merge-waiting.txt"),
        "ready for review\n",
    )
    .expect("workspace change should be written");
    seed_current_passing_workspace_review(&state, &conversation_id).await;
    github.state().fetch_pr_health_result = Some(Err(AppError::Infrastructure(
        "GitHub health unavailable".to_string(),
    )));
    let client = Arc::new(SubmittingPrDescriptionClient::new(
        Arc::clone(&state.agent_conversation_workspace_repo),
        conversation_id.clone(),
    ));
    let state = state.with_agent_client(client);
    let execution_state = Arc::new(ExecutionState::new());

    let response = publish_agent_conversation_workspace_for_app_state(
        &state,
        &execution_state,
        conversation_id.clone(),
        false,
    )
    .await
    .expect("publish should still succeed when auto-merge sync waits");
    state
        .pr_poller_registry
        .stop_agent_workspace_polling(&conversation_id);

    assert_eq!(
        response.workspace.publication_push_status.as_deref(),
        Some("pushed")
    );
    assert_eq!(response.workspace.pr_auto_merge_current, Some(false));
    assert_eq!(
        response.workspace.pr_supervision_status.as_deref(),
        Some("waiting")
    );
    assert!(response
        .workspace
        .pr_supervision_summary
        .as_deref()
        .unwrap_or_default()
        .contains("could not be refreshed yet"));
    let github_state = github.state();
    assert!(github_state.fetch_pr_health_calls >= 1);
    assert_eq!(github_state.enable_pr_auto_merge_calls, 0);
}

#[tokio::test]
async fn publish_workspace_stops_before_push_when_pr_description_fails() {
    let github = Arc::new(MockGithubService::new());
    let (_temp, state, conversation_id, github) =
        setup_publish_command_state("description-fails", true, None, github).await;
    enable_github_pr_publishing(&state, &conversation_id).await;
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    std::fs::write(
        Path::new(&workspace.worktree_path).join("implementation.txt"),
        "change that should be described\n",
    )
    .expect("workspace change should be written");
    seed_current_passing_workspace_review(&state, &conversation_id).await;
    let execution_state = Arc::new(ExecutionState::new());

    let error = publish_agent_conversation_workspace_for_app_state(
        &state,
        &execution_state,
        conversation_id.clone(),
        false,
    )
    .await
    .expect_err("missing generated PR description should block publish");

    assert!(error.contains("completed without submitting a PR description"));
    assert_eq!(github.state().push_branch_calls, 0);
    let stored = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert_eq!(
        stored.publication_push_status.as_deref(),
        Some("description_failed")
    );
    let events = state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("publication events should load");
    assert!(events.iter().any(|event| {
        event.step == "describing"
            && event.status == "started"
            && event.summary == "Drafting pull request description"
    }));
    assert!(events.iter().any(|event| {
        event.step == "description_failed"
            && event.status == "failed"
            && event.classification.as_deref() == Some("operational")
    }));
}

#[tokio::test]
async fn publish_workspace_updates_authoritative_existing_pr_metadata_after_push() {
    let github = Arc::new(MockGithubService::new());
    let (_temp, state, conversation_id, github) =
        setup_publish_command_state("existing-pr-metadata", true, Some(451), github).await;
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    for _ in 0..2 {
        github.queue_pr_detail(Ok(PrDetail {
            number: 451,
            title: "Existing title".to_string(),
            body: Some("Existing body".to_string()),
            author: Some("octocat".to_string()),
            created_at: None,
            url: Some("https://github.com/owner/repo/pull/451".to_string()),
            state: GithubPrStatus::Open,
            is_draft: true,
            head_ref_name: workspace.branch_name.clone(),
            base_ref_name: "main".to_string(),
        }));
    }
    std::fs::write(
        Path::new(&workspace.worktree_path).join("existing-pr-metadata.txt"),
        "update existing pull request metadata\n",
    )
    .expect("workspace change should be written");
    seed_current_passing_workspace_review(&state, &conversation_id).await;
    let client = Arc::new(SubmittingPrDescriptionClient::new(
        Arc::clone(&state.agent_conversation_workspace_repo),
        conversation_id.clone(),
    ));
    let state = state.with_agent_client(client.clone());

    let response = publish_agent_conversation_workspace_for_app_state(
        &state,
        &Arc::new(ExecutionState::new()),
        conversation_id.clone(),
        false,
    )
    .await
    .expect("existing PR publish should succeed");
    state
        .pr_poller_registry
        .stop_agent_workspace_polling(&conversation_id);

    assert_eq!(response.pr_number, Some(451));
    assert!(!response.created_pr);
    {
        let github_state = github.state();
        assert_eq!(github_state.push_branch_calls, 1);
        assert_eq!(github_state.fetch_pr_detail_calls, 2);
        assert_eq!(github_state.create_draft_pr_calls, 0);
        assert_eq!(github_state.patch_pr_metadata_calls, 1);
        assert_eq!(
            github_state
                .last_patch_pr_metadata_args
                .as_ref()
                .map(|args| (&args.0, &args.1)),
            Some((&451, &Some("Cached publication title".to_string())))
        );
        let patched_body = github_state
            .last_patch_pr_metadata_body
            .as_deref()
            .expect("body patch should be captured");
        assert!(patched_body.starts_with("## Summary\n\nReady to publish."));
        assert!(patched_body.contains("_Generated by [RalphX]("));
    }
    assert_eq!(client.spawned_count().await, 1);
    let stored = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert_eq!(stored.publication_push_status.as_deref(), Some("pushed"));
    let events = state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("publication events should load");
    assert!(events.iter().any(|event| {
        event.step == "pushed"
            && event.status == "succeeded"
            && event.summary == "Agent branch pushed"
    }));
    assert!(events.iter().any(|event| {
        event.step == "published"
            && event.status == "succeeded"
            && event.summary == "Draft pull request is ready"
    }));
}

#[tokio::test]
async fn publish_workspace_patches_only_editable_prefix_and_preserves_exact_managed_suffix() {
    let github = Arc::new(MockGithubService::new());
    let (_temp, state, conversation_id, github) =
        setup_publish_command_state("linked-managed-body", true, Some(888), github).await;
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    let remote_body = format!(
        "Existing editable description\n\n{RALPHX_MANAGED_PR_BODY_START}\n\
         <details>\n<summary>View full plan</summary>\n\n{}\n</details>\n\n\
         {RALPHX_GENERATED_FOOTER}\n{RALPHX_MANAGED_PR_BODY_END}\n\nCodeSmith tail  \n",
        "large plan\n".repeat(2_000)
    );
    let expected_suffix = decompose_ralphx_managed_pr_body(&remote_body)
        .preserved_suffix
        .expect("managed body should split")
        .to_string();
    for _ in 0..2 {
        github.queue_pr_detail(Ok(authoritative_pr_detail(
            888,
            workspace.branch_name.clone(),
            "Existing title",
            &remote_body,
        )));
    }
    write_publishable_workspace_change(&state, &conversation_id).await;
    let client = Arc::new(SubmittingPrDescriptionClient::new(
        Arc::clone(&state.agent_conversation_workspace_repo),
        conversation_id.clone(),
    ));
    client
        .queue_decision(
            AgentWorkspacePrMetadataDecision::patch(
                None,
                Some("Improved editable description".to_string()),
            )
            .unwrap(),
        )
        .await;
    let state = state.with_agent_client(client.clone());

    publish_agent_conversation_workspace_for_app_state(
        &state,
        &Arc::new(ExecutionState::new()),
        conversation_id.clone(),
        false,
    )
    .await
    .expect("managed existing body should patch safely");
    state
        .pr_poller_registry
        .stop_agent_workspace_polling(&conversation_id);

    {
        let github_state = github.state();
        assert_eq!(github_state.fetch_pr_detail_calls, 2);
        assert_eq!(github_state.patch_pr_metadata_calls, 1);
        let expected_body = format!("Improved editable description{expected_suffix}");
        assert_eq!(
            github_state.last_patch_pr_metadata_body.as_deref(),
            Some(expected_body.as_str())
        );
    }
    let prompt = &client.spawned_configs().await[0].prompt;
    assert!(prompt.contains("managed_suffix_preserved=\"true\""));
    assert!(prompt.contains(">Existing editable description</body>"));
    assert!(!prompt.contains("large plan"));
    assert!(!prompt.contains("CodeSmith tail"));
    assert!(!publication_events_for(&state, &conversation_id)
        .await
        .iter()
        .any(|event| event.step == "description_failed"));
}

#[tokio::test]
async fn publish_workspace_preserve_fails_closed_when_linked_target_closes_after_push() {
    let github = Arc::new(MockGithubService::new());
    let (_temp, state, conversation_id, github) =
        setup_publish_command_state("linked-preserve-closes", true, Some(889), github).await;
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    github.queue_pr_detail(Ok(authoritative_pr_detail(
        889,
        workspace.branch_name.clone(),
        "Existing title",
        "Existing body",
    )));
    github.queue_pr_detail(Ok(PrDetail {
        state: GithubPrStatus::Closed,
        ..authoritative_pr_detail(
            889,
            workspace.branch_name.clone(),
            "Existing title",
            "Existing body",
        )
    }));
    write_publishable_workspace_change(&state, &conversation_id).await;
    let client = Arc::new(SubmittingPrDescriptionClient::new(
        Arc::clone(&state.agent_conversation_workspace_repo),
        conversation_id.clone(),
    ));
    client
        .queue_decision(AgentWorkspacePrMetadataDecision::Preserve)
        .await;
    let state = state.with_agent_client(client);

    let error = publish_agent_conversation_workspace_for_app_state(
        &state,
        &Arc::new(ExecutionState::new()),
        conversation_id.clone(),
        false,
    )
    .await
    .expect_err("closed post-push target must block success");

    assert!(error.contains("is not open"));
    {
        let github_state = github.state();
        assert_eq!(github_state.push_branch_calls, 1);
        assert_eq!(github_state.fetch_pr_detail_calls, 2);
        assert_eq!(github_state.patch_pr_metadata_calls, 0);
    }
    let stored = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.publication_pr_number, Some(889));
    assert_eq!(
        stored.publication_push_status.as_deref(),
        Some("description_failed")
    );
    assert!(!publication_events_for(&state, &conversation_id)
        .await
        .iter()
        .any(|event| event.step == "published" && event.status == "succeeded"));
}

fn authoritative_pr_detail(
    number: i64,
    head_ref_name: String,
    title: &str,
    body: &str,
) -> PrDetail {
    PrDetail {
        number,
        title: title.to_string(),
        body: Some(body.to_string()),
        author: Some("octocat".to_string()),
        created_at: None,
        url: Some(format!("https://github.com/owner/repo/pull/{number}")),
        state: GithubPrStatus::Open,
        is_draft: true,
        head_ref_name,
        base_ref_name: "main".to_string(),
    }
}

async fn write_publishable_workspace_change(
    state: &AppState,
    conversation_id: &ChatConversationId,
) {
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    std::fs::write(
        Path::new(&workspace.worktree_path).join("metadata-change.txt"),
        "update pull request metadata\n",
    )
    .expect("workspace change should be written");
    seed_current_passing_workspace_review(state, conversation_id).await;
}

async fn publication_events_for(
    state: &AppState,
    conversation_id: &ChatConversationId,
) -> Vec<AgentConversationWorkspacePublicationEvent> {
    state
        .agent_conversation_workspace_repo
        .list_publication_events(conversation_id)
        .await
        .expect("publication events should load")
}

#[tokio::test]
async fn publish_workspace_preserves_linked_existing_pr_metadata_without_edit() {
    let github = Arc::new(MockGithubService::new());
    let (_temp, state, conversation_id, github) =
        setup_publish_command_state("linked-preserve", true, Some(452), github).await;
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    for _ in 0..2 {
        github.queue_pr_detail(Ok(authoritative_pr_detail(
            452,
            workspace.branch_name.clone(),
            "Existing title",
            "Existing body",
        )));
    }
    write_publishable_workspace_change(&state, &conversation_id).await;
    let client = Arc::new(SubmittingPrDescriptionClient::new(
        Arc::clone(&state.agent_conversation_workspace_repo),
        conversation_id.clone(),
    ));
    client
        .queue_decision(AgentWorkspacePrMetadataDecision::Preserve)
        .await;
    let state = state.with_agent_client(client.clone());

    let response = publish_agent_conversation_workspace_for_app_state(
        &state,
        &Arc::new(ExecutionState::new()),
        conversation_id.clone(),
        false,
    )
    .await
    .expect("preserving linked existing PR metadata should succeed");
    state
        .pr_poller_registry
        .stop_agent_workspace_polling(&conversation_id);

    {
        let github_state = github.state();
        assert_eq!(github_state.push_branch_calls, 1);
        assert_eq!(github_state.fetch_pr_detail_calls, 2);
        assert_eq!(github_state.create_draft_pr_calls, 0);
        assert_eq!(github_state.patch_pr_metadata_calls, 0);
        assert_eq!(github_state.update_pr_details_calls, 0);
    }
    assert_eq!(client.spawned_count().await, 1);
    assert_eq!(
        response.workspace.publication_push_status.as_deref(),
        Some("pushed")
    );
    assert!(publication_events_for(&state, &conversation_id)
        .await
        .iter()
        .any(|event| { event.step == "published" && event.status == "succeeded" }));
}

#[tokio::test]
async fn publish_workspace_discovers_unlinked_same_head_pr_before_create() {
    let github = Arc::new(MockGithubService::new());
    let (_temp, state, conversation_id, github) =
        setup_publish_command_state("discover-existing", true, None, github).await;
    enable_github_pr_publishing(&state, &conversation_id).await;
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    github.queue_find_pr_by_head_branch(Ok(Some((
        453,
        "https://github.com/owner/repo/pull/453".to_string(),
    ))));
    github.queue_find_pr_by_head_branch(Ok(Some((
        453,
        "https://github.com/owner/repo/pull/453".to_string(),
    ))));
    github.queue_pr_detail(Ok(authoritative_pr_detail(
        453,
        workspace.branch_name.clone(),
        "Discovered title",
        "Discovered body",
    )));
    github.queue_pr_detail(Ok(authoritative_pr_detail(
        453,
        workspace.branch_name.clone(),
        "Discovered title",
        "Discovered body",
    )));
    write_publishable_workspace_change(&state, &conversation_id).await;
    let client = Arc::new(SubmittingPrDescriptionClient::new(
        Arc::clone(&state.agent_conversation_workspace_repo),
        conversation_id.clone(),
    ));
    client
        .queue_decision(AgentWorkspacePrMetadataDecision::Preserve)
        .await;
    let state = state.with_agent_client(client);

    let response = publish_agent_conversation_workspace_for_app_state(
        &state,
        &Arc::new(ExecutionState::new()),
        conversation_id.clone(),
        false,
    )
    .await
    .expect("same-head PR discovery should publish as an existing target");
    state
        .pr_poller_registry
        .stop_agent_workspace_polling(&conversation_id);

    let github_state = github.state();
    assert_eq!(github_state.find_pr_by_head_branch_calls, 2);
    assert_eq!(github_state.fetch_pr_detail_calls, 2);
    assert_eq!(github_state.create_draft_pr_calls, 0);
    assert_eq!(github_state.patch_pr_metadata_calls, 0);
    assert_eq!(github_state.push_branch_calls, 1);
    drop(github_state);
    assert_eq!(response.pr_number, Some(453));
    assert!(!response.created_pr);
}

#[tokio::test]
async fn publish_workspace_rejects_unavailable_closed_or_wrong_head_targets_before_push() {
    for (suffix, find_result, detail_result, expected_error) in [
        (
            "find-fails",
            Err(AppError::Infrastructure("find failed".to_string())),
            None,
            "find failed",
        ),
        (
            "closed",
            Ok(Some((
                454,
                "https://github.com/owner/repo/pull/454".to_string(),
            ))),
            Some(PrDetail {
                state: GithubPrStatus::Closed,
                ..authoritative_pr_detail(454, "placeholder".to_string(), "Closed", "Body")
            }),
            "pull request #454 is not open",
        ),
        (
            "wrong-head",
            Ok(Some((
                455,
                "https://github.com/owner/repo/pull/455".to_string(),
            ))),
            Some(authoritative_pr_detail(
                455,
                "other-branch".to_string(),
                "Wrong",
                "Body",
            )),
            "pull request #455 head branch does not match workspace branch",
        ),
        (
            "number-mismatch",
            Ok(Some((
                460,
                "https://github.com/owner/repo/pull/460".to_string(),
            ))),
            Some(authoritative_pr_detail(
                999,
                "placeholder".to_string(),
                "Wrong number",
                "Body",
            )),
            "pull request lookup returned #999, expected #460",
        ),
    ] {
        let github = Arc::new(MockGithubService::new());
        let (_temp, state, conversation_id, github) =
            setup_publish_command_state(suffix, true, None, github).await;
        enable_github_pr_publishing(&state, &conversation_id).await;
        github.queue_find_pr_by_head_branch(find_result);
        if let Some(detail) = detail_result {
            github.queue_pr_detail(Ok(detail));
        }
        write_publishable_workspace_change(&state, &conversation_id).await;

        let error = publish_agent_conversation_workspace_for_app_state(
            &state,
            &Arc::new(ExecutionState::new()),
            conversation_id.clone(),
            false,
        )
        .await
        .expect_err("invalid remote target must fail before pushing");

        assert!(
            error.contains(expected_error),
            "expected {error:?} to contain {expected_error:?}"
        );
        {
            let github_state = github.state();
            assert_eq!(github_state.push_branch_calls, 0);
            assert_eq!(github_state.create_draft_pr_calls, 0);
            assert_eq!(github_state.patch_pr_metadata_calls, 0);
            assert_eq!(github_state.update_pr_details_calls, 0);
        }
        assert!(!publication_events_for(&state, &conversation_id)
            .await
            .iter()
            .any(|event| { event.step == "published" && event.status == "succeeded" }));
    }
}

#[tokio::test]
async fn publish_workspace_stops_before_push_when_existing_target_detail_read_fails() {
    let github = Arc::new(MockGithubService::new());
    let (_temp, state, conversation_id, github) =
        setup_publish_command_state("detail-fails", true, Some(459), github).await;
    github.queue_pr_detail(Err(AppError::Infrastructure("detail failed".to_string())));
    write_publishable_workspace_change(&state, &conversation_id).await;

    let error = publish_agent_conversation_workspace_for_app_state(
        &state,
        &Arc::new(ExecutionState::new()),
        conversation_id.clone(),
        false,
    )
    .await
    .expect_err("an authoritative detail read failure must block publishing");

    assert!(error.contains("detail failed"));
    {
        let github_state = github.state();
        assert_eq!(github_state.push_branch_calls, 0);
        assert_eq!(github_state.create_draft_pr_calls, 0);
        assert_eq!(github_state.patch_pr_metadata_calls, 0);
        assert_eq!(github_state.update_pr_details_calls, 0);
    }
    assert!(!publication_events_for(&state, &conversation_id)
        .await
        .iter()
        .any(|event| { event.step == "published" && event.status == "succeeded" }));
}

#[tokio::test]
async fn publish_workspace_redrafts_once_when_existing_pr_authority_drifts() {
    let github = Arc::new(MockGithubService::new());
    let (_temp, state, conversation_id, github) =
        setup_publish_command_state("authority-drifts-once", true, Some(456), github).await;
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    for (title, body) in [
        ("Initial title", "Initial body"),
        ("Changed title", "Changed body"),
        ("Changed title", "Changed body"),
    ] {
        github.queue_pr_detail(Ok(authoritative_pr_detail(
            456,
            workspace.branch_name.clone(),
            title,
            body,
        )));
    }
    write_publishable_workspace_change(&state, &conversation_id).await;
    let client = Arc::new(SubmittingPrDescriptionClient::new(
        Arc::clone(&state.agent_conversation_workspace_repo),
        conversation_id.clone(),
    ));
    client
        .queue_decision(AgentWorkspacePrMetadataDecision::Patch {
            title: Some("initial draft must not be applied".to_string()),
            body_markdown: None,
        })
        .await;
    client
        .queue_decision(AgentWorkspacePrMetadataDecision::Patch {
            title: Some("redrafted title".to_string()),
            body_markdown: None,
        })
        .await;
    let state = state.with_agent_client(client.clone());

    publish_agent_conversation_workspace_for_app_state(
        &state,
        &Arc::new(ExecutionState::new()),
        conversation_id.clone(),
        false,
    )
    .await
    .expect("one authority drift should redraft and publish");
    state
        .pr_poller_registry
        .stop_agent_workspace_polling(&conversation_id);

    {
        let github_state = github.state();
        assert_eq!(github_state.push_branch_calls, 1);
        assert_eq!(github_state.fetch_pr_detail_calls, 3);
        assert_eq!(github_state.patch_pr_metadata_calls, 1);
        assert_eq!(
            github_state.last_patch_pr_metadata_args,
            Some((456, Some("redrafted title".to_string()), None))
        );
        assert_eq!(github_state.update_pr_details_calls, 0);
    }
    assert_eq!(client.spawned_count().await, 2);
    assert!(publication_events_for(&state, &conversation_id)
        .await
        .iter()
        .any(|event| { event.step == "published" && event.status == "succeeded" }));
}

#[tokio::test]
async fn publish_workspace_fails_after_push_when_existing_pr_drifts_twice_or_final_read_fails() {
    for (suffix, confirmation) in [
        (
            "drifts-twice",
            Ok(authoritative_pr_detail(
                457,
                "placeholder".to_string(),
                "Changed again",
                "Changed again body",
            )),
        ),
        (
            "final-read-fails",
            Err(AppError::Infrastructure("final read failed".to_string())),
        ),
    ] {
        let github = Arc::new(MockGithubService::new());
        let (_temp, state, conversation_id, github) =
            setup_publish_command_state(suffix, true, Some(457), github).await;
        let workspace = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        github.queue_pr_detail(Ok(authoritative_pr_detail(
            457,
            workspace.branch_name.clone(),
            "Initial",
            "Initial body",
        )));
        github.queue_pr_detail(Ok(authoritative_pr_detail(
            457,
            workspace.branch_name.clone(),
            "Changed",
            "Changed body",
        )));
        github.queue_pr_detail(match confirmation {
            Ok(mut detail) => {
                detail.head_ref_name = workspace.branch_name.clone();
                Ok(detail)
            }
            Err(error) => Err(error),
        });
        write_publishable_workspace_change(&state, &conversation_id).await;
        let client = Arc::new(SubmittingPrDescriptionClient::new(
            Arc::clone(&state.agent_conversation_workspace_repo),
            conversation_id.clone(),
        ));
        client
            .queue_decision(AgentWorkspacePrMetadataDecision::Patch {
                title: Some("first draft".to_string()),
                body_markdown: None,
            })
            .await;
        client
            .queue_decision(AgentWorkspacePrMetadataDecision::Patch {
                title: Some("second draft".to_string()),
                body_markdown: None,
            })
            .await;
        let state = state.with_agent_client(client.clone());

        let error = publish_agent_conversation_workspace_for_app_state(
            &state,
            &Arc::new(ExecutionState::new()),
            conversation_id.clone(),
            false,
        )
        .await
        .expect_err("a second authority failure must stop metadata mutation");

        assert!(error.contains(if suffix == "drifts-twice" {
            "changed again"
        } else {
            "final read failed"
        }));
        {
            let github_state = github.state();
            assert_eq!(github_state.push_branch_calls, 1);
            assert_eq!(github_state.patch_pr_metadata_calls, 0);
            assert_eq!(github_state.update_pr_details_calls, 0);
        }
        assert_eq!(client.spawned_count().await, 2);
        let stored = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        assert_eq!(
            stored.publication_push_status.as_deref(),
            Some("description_failed")
        );
        assert!(!publication_events_for(&state, &conversation_id)
            .await
            .iter()
            .any(|event| { event.step == "published" && event.status == "succeeded" }));
    }
}

#[tokio::test]
async fn publish_workspace_recovers_duplicate_pr_with_a_redrafted_existing_patch() {
    let github = Arc::new(MockGithubService::new());
    let (_temp, state, conversation_id, github) =
        setup_publish_command_state("duplicate-pr", true, None, github).await;
    enable_github_pr_publishing(&state, &conversation_id).await;
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    github.queue_find_pr_by_head_branch(Ok(None));
    github.state().create_draft_pr_result = Some(Err(AppError::DuplicatePr));
    github.queue_find_pr_by_head_branch(Ok(Some((
        458,
        "https://github.com/owner/repo/pull/458".to_string(),
    ))));
    github.queue_find_pr_by_head_branch(Ok(Some((
        458,
        "https://github.com/owner/repo/pull/458".to_string(),
    ))));
    for _ in 0..2 {
        github.queue_pr_detail(Ok(authoritative_pr_detail(
            458,
            workspace.branch_name.clone(),
            "Existing title",
            "Existing body",
        )));
    }
    write_publishable_workspace_change(&state, &conversation_id).await;
    let client = Arc::new(SubmittingPrDescriptionClient::new(
        Arc::clone(&state.agent_conversation_workspace_repo),
        conversation_id.clone(),
    ));
    client
        .queue_decision(AgentWorkspacePrMetadataDecision::Patch {
            title: Some("new PR title must not be patched".to_string()),
            body_markdown: Some("new PR body must not be patched".to_string()),
        })
        .await;
    client
        .queue_decision(AgentWorkspacePrMetadataDecision::Patch {
            title: Some("existing PR replacement title".to_string()),
            body_markdown: None,
        })
        .await;
    let state = state.with_agent_client(client.clone());

    let response = publish_agent_conversation_workspace_for_app_state(
        &state,
        &Arc::new(ExecutionState::new()),
        conversation_id.clone(),
        false,
    )
    .await
    .expect("duplicate PR should recover through the existing metadata path");
    state
        .pr_poller_registry
        .stop_agent_workspace_polling(&conversation_id);

    {
        let github_state = github.state();
        assert_eq!(github_state.push_branch_calls, 1);
        assert_eq!(github_state.create_draft_pr_calls, 1);
        assert_eq!(github_state.find_pr_by_head_branch_calls, 3);
        assert_eq!(github_state.fetch_pr_detail_calls, 2);
        assert_eq!(github_state.patch_pr_metadata_calls, 1);
        assert_eq!(
            github_state.last_patch_pr_metadata_args,
            Some((458, Some("existing PR replacement title".to_string()), None))
        );
        assert_ne!(
            github_state.last_patch_pr_metadata_body.as_deref(),
            Some("new PR body must not be patched")
        );
        assert_eq!(github_state.update_pr_details_calls, 0);
    }
    assert_eq!(client.spawned_count().await, 2);
    assert_eq!(response.pr_number, Some(458));
    assert!(!response.created_pr);
}

#[test]
fn agent_conversation_response_derives_provider_metadata_from_legacy_claude_session() {
    let mut conversation =
        ChatConversation::new_project(ProjectId::from_string("project-1".to_string()));
    conversation.claude_session_id = Some("claude-session-123".to_string());

    let response = AgentConversationResponse::from(conversation);

    assert_eq!(
        response.claude_session_id,
        Some("claude-session-123".to_string())
    );
    assert_eq!(
        response.provider_session_id,
        Some("claude-session-123".to_string())
    );
    assert_eq!(response.provider_harness, Some("claude".to_string()));
    assert_eq!(response.coordination_mode, "solo");
}

#[test]
fn agent_conversation_response_uses_persisted_coordination_mode() {
    let mut conversation =
        ChatConversation::new_project(ProjectId::from_string("project-1".to_string()));
    conversation.set_coordination_mode(CoordinationMode::RxNativeTeam);

    let response = AgentConversationResponse::from(conversation);

    assert_eq!(response.coordination_mode, "rx_native_team");
}

#[test]
fn agent_conversation_response_keeps_codex_metadata_without_legacy_alias() {
    let mut conversation =
        ChatConversation::new_project(ProjectId::from_string("project-1".to_string()));
    conversation.set_provider_session_ref(ProviderSessionRef {
        harness: AgentHarnessKind::Codex,
        provider_session_id: "codex-thread-123".to_string(),
    });

    let response = AgentConversationResponse::from(conversation);

    assert_eq!(response.claude_session_id, None);
    assert_eq!(
        response.provider_session_id,
        Some("codex-thread-123".to_string())
    );
    assert_eq!(response.provider_harness, Some("codex".to_string()));
}

#[test]
fn agent_conversation_response_restores_legacy_alias_for_canonical_claude_provider_metadata() {
    let mut conversation =
        ChatConversation::new_project(ProjectId::from_string("project-1".to_string()));
    conversation.provider_harness = Some(AgentHarnessKind::Claude);
    conversation.provider_session_id = Some("claude-session-456".to_string());
    conversation.claude_session_id = None;

    let response = AgentConversationResponse::from(conversation);

    assert_eq!(
        response.claude_session_id,
        Some("claude-session-456".to_string())
    );
    assert_eq!(
        response.provider_session_id,
        Some("claude-session-456".to_string())
    );
    assert_eq!(response.provider_harness, Some("claude".to_string()));
}

#[test]
fn agent_conversation_response_includes_automation_ownership() {
    let mut conversation =
        ChatConversation::new_project(ProjectId::from_string("project-1".to_string()));
    conversation.automation_id = Some(AutomationId::from_string("automation-1"));
    conversation.automation_run_id = Some(AutomationRunId::from_string("run-1"));

    let response = AgentConversationResponse::from(conversation);

    assert_eq!(response.automation_id.as_deref(), Some("automation-1"));
    assert_eq!(response.automation_run_id.as_deref(), Some("run-1"));
}

#[tokio::test]
async fn agent_conversation_response_hydrates_runtime_from_copied_message_attribution() {
    let state = AppState::new_test();
    let project_id = ProjectId::from_string("project-1".to_string());
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project_id.clone()))
        .await
        .expect("conversation should be created");
    let mut message = ChatMessage::user_in_project(project_id, "assistant response");
    message.role = MessageRole::Orchestrator;
    message.conversation_id = Some(conversation.id);
    message.logical_model = Some("gpt-5.5".to_string());
    message.effective_model_id = Some("gpt-5.5".to_string());
    message.logical_effort = Some(LogicalEffort::High);
    message.effective_effort = Some("high".to_string());
    state
        .chat_message_repo
        .create(message)
        .await
        .expect("message should be created");

    let response = agent_conversation_response_for_state(&state, conversation)
        .await
        .expect("response should hydrate runtime attribution");

    assert_eq!(response.logical_model.as_deref(), Some("gpt-5.5"));
    assert_eq!(response.effective_model_id.as_deref(), Some("gpt-5.5"));
    assert_eq!(response.logical_effort.as_deref(), Some("high"));
    assert_eq!(response.effective_effort.as_deref(), Some("high"));
}

#[tokio::test]
async fn agent_conversation_response_prefers_latest_run_runtime_over_message_attribution() {
    let state = AppState::new_test();
    let project_id = ProjectId::from_string("project-1".to_string());
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project_id.clone()))
        .await
        .expect("conversation should be created");
    let mut message = ChatMessage::user_in_project(project_id, "assistant response");
    message.role = MessageRole::Orchestrator;
    message.conversation_id = Some(conversation.id);
    message.effective_model_id = Some("sonnet".to_string());
    state
        .chat_message_repo
        .create(message)
        .await
        .expect("message should be created");

    let mut run = AgentRun::new(conversation.id);
    run.logical_model = Some("opus".to_string());
    run.effective_model_id = Some("opus".to_string());
    run.logical_effort = Some(LogicalEffort::Medium);
    run.effective_effort = Some("medium".to_string());
    state
        .agent_run_repo
        .create(run)
        .await
        .expect("run should be created");

    let response = agent_conversation_response_for_state(&state, conversation)
        .await
        .expect("response should hydrate runtime attribution");

    assert_eq!(response.logical_model.as_deref(), Some("opus"));
    assert_eq!(response.effective_model_id.as_deref(), Some("opus"));
    assert_eq!(response.logical_effort.as_deref(), Some("medium"));
    assert_eq!(response.effective_effort.as_deref(), Some("medium"));
}

#[tokio::test]
async fn agent_conversation_responses_for_state_hydrates_each_conversation() {
    let state = AppState::new_test();
    let project_id = ProjectId::from_string("project-response-list".to_string());
    let first = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project_id.clone()))
        .await
        .expect("first conversation should be created");
    let second = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project_id.clone()))
        .await
        .expect("second conversation should be created");
    let first_id = first.id.as_str();
    let second_id = second.id.as_str();

    let mut first_run = AgentRun::new(first.id);
    first_run.logical_model = Some("gpt-5.5".to_string());
    first_run.effective_model_id = Some("gpt-5.5".to_string());
    first_run.logical_effort = Some(LogicalEffort::High);
    first_run.effective_effort = Some("high".to_string());
    state
        .agent_run_repo
        .create(first_run)
        .await
        .expect("first run should be created");

    let mut second_message = ChatMessage::user_in_project(project_id, "copied attribution");
    second_message.role = MessageRole::Orchestrator;
    second_message.conversation_id = Some(second.id);
    second_message.logical_model = Some("claude-sonnet".to_string());
    second_message.effective_model_id = Some("claude-sonnet-4".to_string());
    second_message.logical_effort = Some(LogicalEffort::Medium);
    second_message.effective_effort = Some("medium".to_string());
    state
        .chat_message_repo
        .create(second_message)
        .await
        .expect("second message should be created");

    let responses = agent_conversation_responses_for_state(&state, vec![first, second])
        .await
        .expect("responses should hydrate runtime attribution");

    let first_response = responses
        .iter()
        .find(|response| response.id == first_id)
        .expect("first response should be present");
    assert_eq!(first_response.logical_model.as_deref(), Some("gpt-5.5"));
    assert_eq!(
        first_response.effective_model_id.as_deref(),
        Some("gpt-5.5")
    );
    assert_eq!(first_response.logical_effort.as_deref(), Some("high"));
    assert_eq!(first_response.effective_effort.as_deref(), Some("high"));

    let second_response = responses
        .iter()
        .find(|response| response.id == second_id)
        .expect("second response should be present");
    assert_eq!(
        second_response.logical_model.as_deref(),
        Some("claude-sonnet")
    );
    assert_eq!(
        second_response.effective_model_id.as_deref(),
        Some("claude-sonnet-4")
    );
    assert_eq!(second_response.logical_effort.as_deref(), Some("medium"));
    assert_eq!(second_response.effective_effort.as_deref(), Some("medium"));
}

#[tokio::test]
async fn fork_response_for_state_includes_workspace_counts_parent_and_runtime() {
    let state = AppState::new_test();
    let project_id = ProjectId::from_string("project-fork-response".to_string());
    let mut parent = ChatConversation::new_project(project_id.clone());
    parent.set_title("[Fork] Source conversation");
    let parent_id = parent.id.as_str();
    let mut child = ChatConversation::new_project(project_id.clone());
    child.parent_conversation_id = Some(parent_id.clone());
    child.set_provider_session_ref(ProviderSessionRef {
        harness: AgentHarnessKind::Codex,
        provider_session_id: "child-thread".to_string(),
    });
    let child_id = child.id.as_str();

    let mut run = AgentRun::new(child.id);
    run.logical_model = Some("gpt-5.5".to_string());
    run.effective_model_id = Some("gpt-5.5".to_string());
    run.logical_effort = Some(LogicalEffort::High);
    run.effective_effort = Some("high".to_string());
    state
        .agent_run_repo
        .create(run)
        .await
        .expect("child run should be created");

    let workspace = AgentConversationWorkspace::new(
        child.id,
        project_id,
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some("base-sha".to_string()),
        "ralphx/test/fork-response".to_string(),
        "/tmp/fork-response".to_string(),
    );
    let result = crate::application::agent_conversation_fork::AgentConversationForkResult {
        parent_conversation: parent,
        conversation: child,
        workspace: Some(workspace),
        provider_session: Some(
            crate::application::provider_session_fork::ProviderSessionForkResult {
                session_ref: ProviderSessionRef {
                    harness: AgentHarnessKind::Codex,
                    provider_session_id: "child-thread".to_string(),
                },
                source_path: PathBuf::from("/tmp/source.jsonl"),
                destination_path: PathBuf::from("/tmp/dest.jsonl"),
            },
        ),
        copied_message_count: 2,
        copied_timeline_item_count: 3,
    };

    let response = fork_agent_conversation_response_for_state(&state, result)
        .await
        .expect("fork response should be built");

    assert_eq!(response.parent_conversation.id, parent_id);
    assert_eq!(response.conversation.id, child_id);
    assert_eq!(
        response.conversation.parent_conversation_id.as_deref(),
        Some(parent_id.as_str())
    );
    assert_eq!(
        response.conversation.provider_harness.as_deref(),
        Some("codex")
    );
    assert_eq!(
        response.conversation.provider_session_id.as_deref(),
        Some("child-thread")
    );
    assert_eq!(
        response.conversation.logical_model.as_deref(),
        Some("gpt-5.5")
    );
    assert_eq!(
        response.conversation.logical_effort.as_deref(),
        Some("high")
    );
    assert!(response.provider_session_forked);
    assert_eq!(response.copied_message_count, 2);
    assert_eq!(response.copied_timeline_item_count, 3);
    assert_eq!(
        response
            .workspace
            .as_ref()
            .map(|workspace| workspace.mode.as_str()),
        Some("edit")
    );
}

#[test]
fn emit_agent_conversation_fork_events_accepts_response_payload() {
    let project_id = ProjectId::from_string("project-fork-events".to_string());
    let parent = AgentConversationResponse::from(ChatConversation::new_project(project_id.clone()));
    let mut child_conversation = ChatConversation::new_project(project_id);
    child_conversation.parent_conversation_id = Some(parent.id.clone());
    let child = AgentConversationResponse::from(child_conversation);
    let response = ForkAgentConversationResponse {
        parent_conversation: parent,
        conversation: child,
        workspace: None,
        provider_session_forked: false,
        copied_message_count: 0,
        copied_timeline_item_count: 0,
    };
    let app = mock_builder()
        .build(mock_context(noop_assets()))
        .expect("mock app should build");

    emit_agent_conversation_fork_events(app.handle(), &response);
}

#[tokio::test]
async fn fork_terminal_agent_conversation_for_send_skips_without_terminal_workspace() {
    let state = AppState::new_test();
    let app = mock_builder()
        .build(mock_context(noop_assets()))
        .expect("mock app should build");
    assert!(
        fork_terminal_agent_conversation_for_send(&state, app.handle(), None, "", None, None)
            .await
            .expect("missing conversation id should be ignored")
            .is_none()
    );

    let project_id = ProjectId::from_string("project-terminal-fork-skip".to_string());
    let mut project = Project::new(
        "Terminal Fork Skip".to_string(),
        "/tmp/terminal-fork-skip".to_string(),
    );
    project.id = project_id.clone();
    state
        .project_repo
        .create(project)
        .await
        .expect("project should be created");
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project_id.clone()))
        .await
        .expect("conversation should be created");
    assert!(fork_terminal_agent_conversation_for_send(
        &state,
        app.handle(),
        Some(&conversation.id),
        "",
        None,
        None
    )
    .await
    .expect("missing workspace should be ignored")
    .is_none());

    let workspace = AgentConversationWorkspace::new(
        conversation.id,
        project_id,
        AgentConversationWorkspaceMode::Chat,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some("base-sha".to_string()),
        "ralphx/test/non-terminal".to_string(),
        "/tmp/non-terminal".to_string(),
    );
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should be created");

    assert!(fork_terminal_agent_conversation_for_send(
        &state,
        app.handle(),
        Some(&conversation.id),
        "",
        None,
        None
    )
    .await
    .expect("non-terminal workspace should be ignored")
    .is_none());
}

#[tokio::test]
async fn fork_terminal_agent_conversation_for_send_forks_terminal_workspace() {
    let state = AppState::new_test();
    let app = mock_builder()
        .build(mock_context(noop_assets()))
        .expect("mock app should build");
    let project_id = ProjectId::from_string("project-terminal-fork".to_string());
    let mut project = Project::new(
        "Terminal Fork".to_string(),
        "/tmp/terminal-fork".to_string(),
    );
    project.id = project_id.clone();
    state
        .project_repo
        .create(project)
        .await
        .expect("project should be created");
    let parent = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project_id.clone()))
        .await
        .expect("parent conversation should be created");
    let parent_id = parent.id.as_str();
    let mut workspace = AgentConversationWorkspace::new(
        parent.id,
        project_id,
        AgentConversationWorkspaceMode::Chat,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some("base-sha".to_string()),
        "ralphx/test/terminal".to_string(),
        "/tmp/terminal".to_string(),
    );
    workspace.publication_pr_status = Some("merged".to_string());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should be created");

    let child_id = fork_terminal_agent_conversation_for_send(
        &state,
        app.handle(),
        Some(&parent.id),
        "",
        None,
        None,
    )
    .await
    .expect("terminal workspace should fork")
    .expect("forked conversation id should be returned");
    let child = state
        .chat_conversation_repo
        .get_by_id(&child_id)
        .await
        .expect("child lookup should succeed")
        .expect("child conversation should exist");

    assert_eq!(
        child.parent_conversation_id.as_deref(),
        Some(parent_id.as_str())
    );
    assert_eq!(child.agent_mode, Some(AgentConversationWorkspaceMode::Chat));
    assert!(state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&child_id)
        .await
        .expect("workspace lookup should succeed")
        .is_none());
}

#[tokio::test]
async fn fork_terminal_agent_conversation_for_send_spawns_session_namer_with_new_message() {
    let concrete_client = Arc::new(MockAgenticClient::new());
    let agent_client: Arc<dyn AgenticClient> = concrete_client.clone();
    let state = AppState::new_test().with_agent_client(agent_client);
    let app = mock_builder()
        .build(mock_context(noop_assets()))
        .expect("mock app should build");
    let project_id = ProjectId::from_string("project-terminal-fork-namer".to_string());
    let mut project = Project::new(
        "Terminal Fork Namer".to_string(),
        "/tmp/terminal-fork-namer".to_string(),
    );
    project.id = project_id.clone();
    state
        .project_repo
        .create(project)
        .await
        .expect("project should be created");
    let mut parent = ChatConversation::new_project(project_id.clone());
    parent.set_title("Stabilize publication recovery");
    let parent = state
        .chat_conversation_repo
        .create(parent)
        .await
        .expect("parent conversation should be created");
    let mut prior_message = ChatMessage::user_in_project(
        project_id.clone(),
        "The merged workspace still reopens with stale publication state.",
    );
    prior_message.conversation_id = Some(parent.id.clone());
    state
        .chat_message_repo
        .create(prior_message)
        .await
        .expect("prior message should be created");

    let mut workspace = AgentConversationWorkspace::new(
        parent.id.clone(),
        project_id,
        AgentConversationWorkspaceMode::Chat,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some("base-sha".to_string()),
        "ralphx/test/terminal-namer".to_string(),
        "/tmp/terminal-namer".to_string(),
    );
    workspace.publication_pr_status = Some("merged".to_string());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should be created");

    let child_id = fork_terminal_agent_conversation_for_send(
        &state,
        app.handle(),
        Some(&parent.id),
        "Please continue the closed run and fix the title fallback.",
        None,
        None,
    )
    .await
    .expect("terminal workspace should fork")
    .expect("forked conversation id should be returned");

    for _ in 0..20 {
        if !concrete_client.get_spawn_calls().await.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let spawn_calls = concrete_client.get_spawn_calls().await;
    let prompt = spawn_calls
        .iter()
        .find_map(|call| match &call.call_type {
            MockCallType::Spawn { prompt, .. } => Some(prompt.as_str()),
            _ => None,
        })
        .expect("session namer should be spawned");

    assert!(prompt.contains(&format!(
        "<conversation_id>{}</conversation_id>",
        child_id.as_str()
    )));
    assert!(prompt.contains(
        "<user_message>Please continue the closed run and fix the title fallback.</user_message>"
    ));
    assert!(prompt.contains(
        "<content>The merged workspace still reopens with stale publication state.</content>"
    ));
}

#[tokio::test]
async fn fork_agent_conversation_command_returns_hydrated_child_response() {
    let state = AppState::new_test();
    let project_id = ProjectId::from_string("project-fork-command".to_string());
    let mut project = Project::new(
        "Fork Command Project".to_string(),
        "/tmp/fork-command-project".to_string(),
    );
    project.id = project_id.clone();
    state
        .project_repo
        .create(project)
        .await
        .expect("project should be created");
    let mut parent = ChatConversation::new_project(project_id.clone());
    parent.set_title("Source conversation");
    parent.set_agent_mode(Some(AgentConversationWorkspaceMode::Chat));
    let parent = state
        .chat_conversation_repo
        .create(parent)
        .await
        .expect("parent conversation should be created");
    let mut message = ChatMessage::user_in_project(project_id, "copied runtime");
    message.conversation_id = Some(parent.id);
    message.logical_model = Some("gpt-5.4".to_string());
    message.effective_model_id = Some("gpt-5.4".to_string());
    message.logical_effort = Some(LogicalEffort::Medium);
    message.effective_effort = Some("medium".to_string());
    state
        .chat_message_repo
        .create(message)
        .await
        .expect("message should be created");
    let parent_id = parent.id.as_str();
    let app = mock_builder()
        .manage(state)
        .build(mock_context(noop_assets()))
        .expect("mock app should build");

    let response = fork_agent_conversation(
        ForkAgentConversationInput {
            conversation_id: parent.id.as_str(),
        },
        app.state(),
        app.handle().clone(),
    )
    .await
    .expect("fork command should succeed");

    assert_eq!(response.parent_conversation.id, parent_id);
    assert_eq!(
        response.conversation.parent_conversation_id.as_deref(),
        Some(parent_id.as_str())
    );
    assert_eq!(
        response.conversation.title.as_deref(),
        Some("[Fork] Source conversation")
    );
    assert_eq!(response.conversation.agent_mode.as_deref(), Some("chat"));
    assert_eq!(response.copied_message_count, 1);
    assert!(response.workspace.is_none());
    assert_eq!(
        response.conversation.logical_model.as_deref(),
        Some("gpt-5.4")
    );
    assert_eq!(
        response.conversation.logical_effort.as_deref(),
        Some("medium")
    );
}

#[tokio::test]
async fn list_page_create_archive_restore_and_summary_hydrate_runtime_attribution() {
    let state = AppState::new_test();
    let project_id = ProjectId::from_string("project-command-runtime".to_string());
    let mut conversation = ChatConversation::new_project(project_id.clone());
    conversation.set_title("Runtime conversation");
    let conversation = state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("conversation should be created");
    let conversation_id = conversation.id.as_str();
    let mut run = AgentRun::new(conversation.id);
    run.logical_model = Some("gpt-5.5".to_string());
    run.effective_model_id = Some("gpt-5.5".to_string());
    run.logical_effort = Some(LogicalEffort::High);
    run.effective_effort = Some("high".to_string());
    state
        .agent_run_repo
        .create(run)
        .await
        .expect("run should be created");
    let mut child = ChatConversation::new_project(project_id.clone());
    child.parent_conversation_id = Some(conversation.id.as_str().to_string());
    child.set_title("Review workspace changes");
    let child = state
        .chat_conversation_repo
        .create(child)
        .await
        .expect("child conversation should be created");
    let app = mock_builder()
        .manage(state)
        .build(mock_context(noop_assets()))
        .expect("mock app should build");

    let page = list_agent_conversations_page(
        ChatContextType::Project.to_string(),
        project_id.as_str().to_string(),
        Some(true),
        Some(false),
        Some(0),
        Some(10),
        None,
        app.state(),
    )
    .await
    .expect("conversation page should load");
    assert_eq!(page.total, 1);
    let page_conversation = page
        .conversations
        .iter()
        .find(|conversation| conversation.id == conversation_id)
        .expect("seeded conversation should be listed");
    assert_eq!(page_conversation.logical_model.as_deref(), Some("gpt-5.5"));
    assert_eq!(page_conversation.logical_effort.as_deref(), Some("high"));
    assert!(page
        .conversations
        .iter()
        .all(|conversation| conversation.id != child.id.as_str()));

    let summary = get_agent_conversation_summary_for_app_state(
        app.state::<AppState>().inner(),
        conversation_id.clone(),
    )
    .await
    .expect("summary should load")
    .expect("summary should exist");
    assert_eq!(summary.effective_model_id.as_deref(), Some("gpt-5.5"));
    assert_eq!(summary.effective_effort.as_deref(), Some("high"));

    let created = create_agent_conversation(
        CreateAgentConversationInput {
            context_type: ChatContextType::Project.to_string(),
            context_id: Some(project_id.as_str().to_string()),
            title: Some("Created from command".to_string()),
            mode: None,
            team_intent: None,
        },
        app.state(),
    )
    .await
    .expect("conversation should be created");
    assert_eq!(created.title.as_deref(), Some("Created from command"));

    let archived = archive_agent_conversation(conversation_id.clone(), false, app.state())
        .await
        .expect("conversation should be archived");
    assert!(archived.conversation.archived_at.is_some());
    assert_eq!(
        archived.conversation.logical_model.as_deref(),
        Some("gpt-5.5")
    );

    let restored = restore_agent_conversation(conversation_id, app.state())
        .await
        .expect("conversation should be restored");
    assert!(restored.archived_at.is_none());
    assert_eq!(restored.logical_effort.as_deref(), Some("high"));
}

#[tokio::test]
async fn list_page_includes_child_conversations_with_owned_workspaces() {
    let state = AppState::new_test();
    let project_id = ProjectId::from_string("project-command-child-workspace".to_string());
    let mut parent = ChatConversation::new_project(project_id.clone());
    parent.set_title("Merged parent workspace");
    let parent = state
        .chat_conversation_repo
        .create(parent)
        .await
        .expect("parent conversation should be created");

    let mut embedded_child = ChatConversation::new_project(project_id.clone());
    embedded_child.parent_conversation_id = Some(parent.id.as_str().to_string());
    embedded_child.set_title("Embedded review child");
    let embedded_child = state
        .chat_conversation_repo
        .create(embedded_child)
        .await
        .expect("embedded child should be created");

    let mut workspace_child = ChatConversation::new_project(project_id.clone());
    workspace_child.parent_conversation_id = Some(parent.id.as_str().to_string());
    workspace_child.set_title("Continued child workspace");
    let workspace_child = state
        .chat_conversation_repo
        .create(workspace_child)
        .await
        .expect("workspace child should be created");
    let workspace = workspace_for_runtime_test(&workspace_child.id, &project_id);
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should be created");

    let app = mock_builder()
        .manage(state)
        .build(mock_context(noop_assets()))
        .expect("mock app should build");

    let page = list_agent_conversations_page(
        ChatContextType::Project.to_string(),
        project_id.as_str().to_string(),
        Some(false),
        Some(false),
        Some(0),
        Some(10),
        None,
        app.state(),
    )
    .await
    .expect("conversation page should load");

    let conversation_ids = page
        .conversations
        .iter()
        .map(|conversation| conversation.id.clone())
        .collect::<Vec<_>>();
    assert!(
        conversation_ids.contains(&workspace_child.id.as_str()),
        "child conversations with their own workspace should be listed"
    );
    assert!(
        !conversation_ids.contains(&embedded_child.id.as_str()),
        "embedded child conversations without workspaces should stay hidden"
    );
}

#[tokio::test]
async fn agent_list_filter_keeps_task_runtime_child_conversations() {
    let state = AppState::new_test();
    let task_id = TaskId::from_string("task-runtime-visible".to_string());
    let parent = state
        .chat_conversation_repo
        .create(ChatConversation::new_task_execution(task_id.clone()))
        .await
        .expect("parent task runtime conversation should be created");

    let mut child = ChatConversation::new_task_execution(task_id);
    child.parent_conversation_id = Some(parent.id.as_str().to_string());
    let child = state
        .chat_conversation_repo
        .create(child)
        .await
        .expect("child task runtime conversation should be created");

    let filtered =
        filter_agent_list_visible_conversations(&state, vec![child.clone(), parent.clone()])
            .await
            .expect("shared list filter should run");
    let filtered_ids = filtered
        .iter()
        .map(|conversation| conversation.id.as_str())
        .collect::<Vec<_>>();

    assert_eq!(
        filtered_ids,
        vec![child.id.as_str(), parent.id.as_str()],
        "task runtime attempts should stay visible even when parented"
    );
}

#[tokio::test]
async fn agent_list_endpoints_show_automation_setup_and_hide_runs_but_direct_fetch_works() {
    let state = AppState::new_test();
    let project_id = ProjectId::from_string("project-command-automation-hidden".to_string());
    let mut visible = ChatConversation::new_project(project_id.clone());
    visible.set_title("Manual agent conversation");
    let visible = state
        .chat_conversation_repo
        .create(visible)
        .await
        .expect("visible conversation should be created");

    let automation_id = AutomationId::from_string("automation-1");
    let mut setup = ChatConversation::new_project(project_id.clone());
    setup.set_title("Automation setup conversation");
    setup.automation_id = Some(automation_id.clone());
    let setup = state
        .chat_conversation_repo
        .create(setup)
        .await
        .expect("setup conversation should be created");

    let mut run = ChatConversation::new_project(project_id.clone());
    run.set_title("Automation run conversation");
    run.automation_id = Some(automation_id);
    run.automation_run_id = Some(AutomationRunId::from_string("run-1"));
    let run = state
        .chat_conversation_repo
        .create(run)
        .await
        .expect("run conversation should be created");

    let filtered = filter_agent_list_visible_conversations(
        &state,
        vec![visible.clone(), setup.clone(), run.clone()],
    )
    .await
    .expect("shared list filter should run");
    let filtered_ids = filtered
        .iter()
        .map(|conversation| conversation.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(filtered_ids, vec![visible.id.as_str(), setup.id.as_str()]);

    let setup_conversation = state
        .chat_conversation_repo
        .get_by_id(&setup.id)
        .await
        .expect("direct setup conversation fetch should load")
        .expect("direct setup conversation should exist");
    let setup_response = agent_conversation_response_for_state(&state, setup_conversation)
        .await
        .expect("setup response should hydrate");
    assert_eq!(setup_response.id, setup.id.as_str());
    assert_eq!(
        setup_response.automation_id.as_deref(),
        Some("automation-1")
    );

    let run_conversation = state
        .chat_conversation_repo
        .get_by_id(&run.id)
        .await
        .expect("direct run conversation fetch should load")
        .expect("direct run conversation should exist");
    let run_response = agent_conversation_response_for_state(&state, run_conversation)
        .await
        .expect("run response should hydrate");
    assert_eq!(run_response.id, run.id.as_str());
    assert_eq!(run_response.automation_run_id.as_deref(), Some("run-1"));

    let app = mock_builder()
        .manage(state)
        .build(mock_context(noop_assets()))
        .expect("mock app should build");

    let page = list_agent_conversations_page(
        ChatContextType::Project.to_string(),
        project_id.as_str().to_string(),
        Some(false),
        Some(false),
        Some(0),
        Some(10),
        None,
        app.state(),
    )
    .await
    .expect("conversation page should load");
    let page_ids = page
        .conversations
        .iter()
        .map(|conversation| conversation.id.clone())
        .collect::<Vec<_>>();
    let visible_id = visible.id.as_str();
    let setup_id = setup.id.as_str();
    let run_id = run.id.as_str();
    assert_eq!(page_ids.len(), 2);
    assert!(
        page_ids.contains(&visible_id),
        "manual conversations should be listed"
    );
    assert!(
        page_ids.contains(&setup_id),
        "automation setup conversations should be listed"
    );
    assert!(
        !page_ids.contains(&run_id),
        "automation run conversations should stay hidden from list endpoints"
    );
}

fn mode_lock_test_workspace(
    conversation_id: ChatConversationId,
    project_id: ProjectId,
) -> AgentConversationWorkspace {
    AgentConversationWorkspace::new(
        conversation_id,
        project_id,
        AgentConversationWorkspaceMode::Ideation,
        IdeationAnalysisBaseRefKind::CurrentBranch,
        "feature/mode-lock".to_string(),
        Some("Current branch (feature/mode-lock)".to_string()),
        Some("base-sha".to_string()),
        "ralphx/project/mode-lock".to_string(),
        "/tmp/ralphx-mode-lock".to_string(),
    )
}

#[tokio::test]
async fn workspace_response_projects_active_ideation_mode_lock() {
    let state = AppState::new_test();
    let project_id = ProjectId::from_string("project-active-mode-lock".to_string());
    let conversation_id = ChatConversationId::from_string("77777777-7777-4777-8777-777777777777");
    let session = state
        .ideation_session_repo
        .create(IdeationSession::new(project_id.clone()))
        .await
        .expect("ideation session persisted");
    let mut workspace = mode_lock_test_workspace(conversation_id, project_id);
    workspace.linked_ideation_session_id = Some(session.id);

    let response = agent_workspace_response_for_state(&state, workspace)
        .await
        .expect("workspace response resolves mode lock");

    assert!(response.mode_switch_locked);
    assert_eq!(
        response.mode_switch_lock_reason.as_deref(),
        Some("Ideation session is still active")
    );
}

#[tokio::test]
async fn workspace_response_keeps_active_planning_session_unlocked() {
    let state = AppState::new_test();
    let project_id = ProjectId::from_string("project-planning-mode-unlocked".to_string());
    let conversation_id = ChatConversationId::from_string("77777777-7777-4777-8777-777777777778");
    let session = state
        .ideation_session_repo
        .create(
            IdeationSession::builder()
                .project_id(project_id.clone())
                .session_flow(IdeationSessionFlow::Planning)
                .build(),
        )
        .await
        .expect("planning session persisted");
    let mut workspace = mode_lock_test_workspace(conversation_id, project_id);
    workspace.mode = AgentConversationWorkspaceMode::Plan;
    workspace.linked_ideation_session_id = Some(session.id);

    let response = agent_workspace_response_for_state(&state, workspace)
        .await
        .expect("workspace response resolves mode lock");

    assert!(!response.mode_switch_locked);
    assert!(response.mode_switch_lock_reason.is_none());
}

#[tokio::test]
async fn workspace_response_projects_superseded_execution_plan_as_unlocked() {
    let state = AppState::new_test();
    let project_id = ProjectId::from_string("project-superseded-mode-lock".to_string());
    let conversation_id = ChatConversationId::from_string("88888888-8888-4888-8888-888888888888");
    let session = state
        .ideation_session_repo
        .create(IdeationSession::new(project_id.clone()))
        .await
        .expect("ideation session persisted");
    let mut execution_plan = ExecutionPlan::new(session.id.clone());
    execution_plan.status = ExecutionPlanStatus::Superseded;
    let execution_plan = state
        .execution_plan_repo
        .create(execution_plan)
        .await
        .expect("execution plan persisted");
    let mut plan_branch = PlanBranch::new(
        ArtifactId::from_string("artifact-superseded-lock"),
        session.id.clone(),
        project_id.clone(),
        "plan-superseded-lock".to_string(),
        "main".to_string(),
    );
    plan_branch.execution_plan_id = Some(execution_plan.id);
    let plan_branch = state
        .plan_branch_repo
        .create(plan_branch)
        .await
        .expect("plan branch persisted");
    let mut workspace = mode_lock_test_workspace(conversation_id, project_id);
    workspace.linked_ideation_session_id = Some(session.id);
    workspace.linked_plan_branch_id = Some(plan_branch.id);

    let response = agent_workspace_response_for_state(&state, workspace)
        .await
        .expect("workspace response resolves mode lock");

    assert!(!response.mode_switch_locked);
    assert!(response.mode_switch_lock_reason.is_none());
}

#[tokio::test]
async fn workspace_response_treats_missing_mode_owner_links_as_unlocked() {
    let state = AppState::new_test();
    let project_id = ProjectId::from_string("project-missing-mode-lock".to_string());
    let plan_conversation_id =
        ChatConversationId::from_string("99999999-9999-4999-8999-999999999999");
    let mut plan_workspace = mode_lock_test_workspace(plan_conversation_id, project_id.clone());
    plan_workspace.linked_plan_branch_id =
        Some(PlanBranchId::from_string("missing-plan-branch".to_string()));

    let plan_response = agent_workspace_response_for_state(&state, plan_workspace)
        .await
        .expect("missing plan branch resolves as unlocked");
    assert!(!plan_response.mode_switch_locked);
    assert!(plan_response.mode_switch_lock_reason.is_none());

    let session_conversation_id =
        ChatConversationId::from_string("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa");
    let mut session_workspace = mode_lock_test_workspace(session_conversation_id, project_id);
    session_workspace.linked_ideation_session_id = Some(IdeationSessionId::from_string(
        "missing-ideation-session".to_string(),
    ));

    let session_response = agent_workspace_response_for_state(&state, session_workspace)
        .await
        .expect("missing ideation session resolves as unlocked");
    assert!(!session_response.mode_switch_locked);
    assert!(session_response.mode_switch_lock_reason.is_none());
}

#[tokio::test]
async fn switching_to_chat_without_existing_workspace_keeps_workspace_absent() {
    let state = AppState::new_test();
    let project_id = ProjectId::from_string("project-chat-no-workspace".to_string());
    let conversation_id = ChatConversationId::from_string("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb");
    let mut conversation = ChatConversation::new_project(project_id);
    conversation.id = conversation_id;
    conversation.set_agent_mode(Some(AgentConversationWorkspaceMode::Chat));
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("conversation persisted");

    let response = switch_agent_conversation_mode_for_state(
        SwitchAgentConversationModeInput {
            conversation_id: conversation_id.as_str(),
            mode: "chat".to_string(),
            base_ref_kind: None,
            base_branch_mode: None,
            base_ref: None,
            base_display_name: None,
            base_source_pull_request: None,
            runtime_override: None,
        },
        &state,
    )
    .await
    .expect("chat mode switch succeeds without workspace");

    assert_eq!(response.conversation.agent_mode.as_deref(), Some("chat"));
    assert!(response.workspace.is_none());
}

#[tokio::test]
async fn switching_agent_mode_with_runtime_override_persists_one_conversation_tuple() {
    let state = AppState::new_test();
    let project_id = ProjectId::from_string("project-runtime-mode-switch".to_string());
    let conversation_id = ChatConversationId::from_string("abababab-abab-4bab-8bab-abababababab");
    let mut project = Project::new(
        "Runtime Mode Switch".to_string(),
        "/tmp/runtime-mode-switch".to_string(),
    );
    project.id = project_id.clone();
    state
        .project_repo
        .create(project)
        .await
        .expect("project persisted");
    let mut conversation = ChatConversation::new_project(project_id.clone());
    conversation.id = conversation_id;
    conversation.set_agent_mode(Some(AgentConversationWorkspaceMode::Chat));
    conversation.set_coordination_mode(CoordinationMode::RxNativeTeam);
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("conversation persisted");
    let workspace = AgentConversationWorkspace::new(
        conversation_id,
        project_id,
        AgentConversationWorkspaceMode::Chat,
        IdeationAnalysisBaseRefKind::CurrentBranch,
        "main".to_string(),
        Some("Current branch (main)".to_string()),
        Some("base-sha".to_string()),
        "ralphx/project/runtime-mode-switch".to_string(),
        "/tmp/ralphx-runtime-mode-switch".to_string(),
    );
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace persisted");

    let response = switch_agent_conversation_mode_for_state(
        SwitchAgentConversationModeInput {
            conversation_id: conversation_id.as_str(),
            mode: "edit".to_string(),
            runtime_override: Some(ManualRoleRuntimeOverride {
                harness: AgentHarnessKind::Codex,
                model: None,
                effort: None,
                service_tier: ManualServiceTier::ProviderDefault,
                coordination_mode: Some(CoordinationMode::Solo),
                persona_id: None,
            }),
            base_ref_kind: None,
            base_branch_mode: None,
            base_ref: None,
            base_display_name: None,
            base_source_pull_request: None,
        },
        &state,
    )
    .await
    .expect("mode and runtime bindings persist together");

    assert_eq!(response.conversation.agent_mode.as_deref(), Some("edit"));
    assert_eq!(response.conversation.coordination_mode, "solo");
    assert!(response.conversation.persona_id.is_none());
    assert_eq!(response.workspace.expect("workspace returned").mode, "edit");

    let stored = state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .expect("conversation lookup succeeds")
        .expect("conversation exists");
    assert_eq!(
        stored.agent_mode,
        Some(AgentConversationWorkspaceMode::Edit)
    );
    assert_eq!(stored.coordination_mode, CoordinationMode::Solo);
    assert!(stored.persona_id.is_none());
}

#[tokio::test]
async fn switching_to_edit_without_existing_workspace_creates_workspace() {
    let state = AppState::new_test();
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_publish_repo(&repo_path);
    let project_id = ProjectId::from_string("project-edit-new-workspace".to_string());
    let conversation_id = ChatConversationId::from_string("cccccccc-cccc-4ccc-8ccc-cccccccccccc");
    let mut project = Project::new(
        "Mode Switch Project".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.id = project_id.clone();
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
    state
        .project_repo
        .create(project)
        .await
        .expect("project persisted");
    state
        .execution_settings_repo
        .update_settings(
            Some(&project_id),
            &ExecutionSettings {
                agent_workspace_pr_autofix_default: true,
                agent_workspace_pr_auto_merge_default: true,
                ..ExecutionSettings::default()
            },
        )
        .await
        .expect("settings persisted");
    let mut conversation = ChatConversation::new_project(project_id);
    conversation.id = conversation_id;
    conversation.set_agent_mode(Some(AgentConversationWorkspaceMode::Chat));
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("conversation persisted");

    let response = switch_agent_conversation_mode_for_state(
        SwitchAgentConversationModeInput {
            conversation_id: conversation_id.as_str(),
            mode: "edit".to_string(),
            base_ref_kind: None,
            base_branch_mode: None,
            base_ref: None,
            base_display_name: None,
            base_source_pull_request: None,
            runtime_override: None,
        },
        &state,
    )
    .await
    .expect("edit mode switch creates workspace");

    assert_eq!(response.conversation.agent_mode.as_deref(), Some("edit"));
    let workspace = response.workspace.expect("workspace should be returned");
    assert_eq!(workspace.mode.as_str(), "edit");
    assert!(workspace.pr_autofix_enabled);
    assert!(workspace.pr_auto_merge_desired);
}

#[tokio::test]
async fn switching_branchless_chat_to_edit_persists_source_pull_request_metadata() {
    let state = AppState::new_test();
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_publish_repo(&repo_path);
    git(&repo_path, &["checkout", "-b", "feature/source-pr"]);
    std::fs::write(repo_path.join("README.md"), "source pr\n")
        .expect("fixture update should be written");
    git(&repo_path, &["add", "README.md"]);
    git(&repo_path, &["commit", "-m", "source pr"]);
    let source_sha = git(&repo_path, &["rev-parse", "HEAD"]);
    git(&repo_path, &["checkout", "main"]);

    let project_id = ProjectId::from_string("project-source-pr-switch".to_string());
    let conversation_id = ChatConversationId::from_string("eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee");
    let mut project = Project::new(
        "Mode Switch Source PR".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.id = project_id.clone();
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
    state
        .project_repo
        .create(project)
        .await
        .expect("project persisted");
    let mut conversation = ChatConversation::new_project(project_id);
    conversation.id = conversation_id.clone();
    conversation.set_agent_mode(Some(AgentConversationWorkspaceMode::Chat));
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("conversation persisted");

    let response = switch_agent_conversation_mode_for_state(
        SwitchAgentConversationModeInput {
            conversation_id: conversation_id.as_str(),
            mode: "edit".to_string(),
            base_ref_kind: Some("local_branch".to_string()),
            base_branch_mode: None,
            base_ref: Some("feature/source-pr".to_string()),
            base_display_name: Some("PR #456: Source PR".to_string()),
            base_source_pull_request: Some(AgentWorkspaceSourcePullRequestInput {
                number: 456,
                url: Some("https://github.com/owner/repo/pull/456".to_string()),
                title: Some("Source PR".to_string()),
                head_ref_name: "feature/source-pr".to_string(),
                base_ref_name: Some("main".to_string()),
                head_ref_oid: Some(source_sha.clone()),
            }),
            runtime_override: None,
        },
        &state,
    )
    .await
    .expect("edit mode switch should create source PR workspace");

    let workspace = response.workspace.expect("workspace should be returned");
    assert_eq!(workspace.mode, "edit");
    assert_eq!(workspace.branch_mode, "isolated");
    assert_eq!(workspace.base_ref_kind, "local_branch");
    assert_eq!(workspace.base_ref, "feature/source-pr");
    assert_ne!(workspace.branch_name, "feature/source-pr");
    assert!(workspace.branch_name.contains("/agent-"));
    assert_eq!(workspace.publication_pr_number, None);
    assert_eq!(workspace.publication_pr_status.as_deref(), None);
    let source = workspace
        .source_pull_request
        .expect("source PR metadata should be returned");
    assert_eq!(source.number, 456);
    assert_eq!(source.head_ref_name, "feature/source-pr");
    assert_eq!(source.base_ref_name.as_deref(), Some("main"));
    assert_eq!(source.head_ref_oid.as_deref(), Some(source_sha.as_str()));

    let persisted = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup succeeds")
        .expect("workspace should persist");
    assert_eq!(
        persisted.branch_mode,
        AgentConversationWorkspaceBranchMode::Isolated
    );
    assert_eq!(
        persisted.base_ref_kind,
        IdeationAnalysisBaseRefKind::LocalBranch
    );
    assert_eq!(persisted.base_ref, "feature/source-pr");
    assert_ne!(persisted.branch_name, "feature/source-pr");
    assert!(persisted.branch_name.contains("/agent-"));
    assert_eq!(persisted.publication_pr_number, None);
    assert_eq!(persisted.publication_pr_url.as_deref(), None);
    assert_eq!(persisted.publication_pr_status.as_deref(), None);
    assert_eq!(
        persisted
            .source_pull_request
            .as_ref()
            .map(|source| source.number),
        Some(456)
    );
    assert_eq!(
        persisted
            .source_pull_request
            .as_ref()
            .and_then(|source| source.base_ref_name.as_deref()),
        Some("main")
    );
}

#[tokio::test]
async fn plan_to_edit_precommit_rejects_a_runtime_that_remains_registered_after_stop() {
    let state = AppState::new_test();
    let conversation = ChatConversation::new_project(ProjectId::new());
    let running_key = RunningAgentKey::new(
        ChatContextType::Project.to_string(),
        conversation.id.as_str(),
    );
    state
        .running_agent_registry
        .register(
            running_key.clone(),
            123,
            conversation.id.as_str(),
            "run-still-registered".to_string(),
            None,
            None,
        )
        .await;
    let service = MockChatService::new();

    let error = stop_plan_to_edit_handoff_before_commit(&state, &service, &conversation)
        .await
        .expect_err("a still-registered runtime must block the authority transition");

    assert_eq!(error, "Cannot change mode while the agent is running");
    assert_eq!(
        service.get_stop_agent_calls().await,
        vec![(
            ChatContextType::Project,
            conversation.id.as_str().to_string()
        )]
    );
    assert!(state.running_agent_registry.is_running(&running_key).await);
}

#[tokio::test]
async fn plan_to_edit_postcommit_preserves_session_when_idle_retirement_is_rejected() {
    let state = AppState::new_test();
    let mut conversation = ChatConversation::new_project(ProjectId::new());
    conversation.set_provider_session_ref(ProviderSessionRef {
        harness: AgentHarnessKind::Claude,
        provider_session_id: "planning-session".to_string(),
    });
    let conversation_id = conversation.id.as_str().to_string();
    let service = MockChatService::new();
    service
        .set_retire_idle_interactive_process_result(false)
        .await;

    let error = finish_plan_to_edit_handoff_after_commit(&state, &service, &mut conversation)
        .await
        .expect_err("unverified idle retirement must reject direct implementation");

    assert!(error.contains("runtime handoff is still active"));
    assert_eq!(
        service.get_retire_idle_interactive_process_calls().await,
        vec![(ChatContextType::Project, conversation_id)]
    );
    assert_eq!(
        conversation
            .provider_session_ref()
            .map(|session| session.provider_session_id),
        Some("planning-session".to_string())
    );
}

#[tokio::test]
async fn accepted_plan_proposal_switch_can_bypass_running_agent_guard() {
    let state = AppState::new_test();
    let project_id = ProjectId::from_string("project-running-plan-switch".to_string());
    let conversation_id = ChatConversationId::from_string("12121212-1212-4121-8121-121212121212");
    let mut conversation = ChatConversation::new_project(project_id.clone());
    conversation.id = conversation_id.clone();
    conversation.set_agent_mode(Some(AgentConversationWorkspaceMode::Edit));
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("conversation persisted");

    let workspace = AgentConversationWorkspace::new(
        conversation_id.clone(),
        project_id,
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::CurrentBranch,
        "feature/agent-screen".to_string(),
        Some("Current branch (feature/agent-screen)".to_string()),
        Some("base-sha".to_string()),
        "ralphx/project/agent-12121212".to_string(),
        "/tmp/ralphx-agent-12121212".to_string(),
    );
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace persisted");

    let running_key = RunningAgentKey::new(
        ChatContextType::Project.to_string(),
        conversation_id.as_str(),
    );
    state
        .running_agent_registry
        .register(
            running_key,
            123,
            conversation_id.as_str(),
            "run-plan-proposal".to_string(),
            None,
            None,
        )
        .await;

    let public_result = switch_agent_conversation_mode_for_state(
        SwitchAgentConversationModeInput {
            conversation_id: conversation_id.as_str(),
            mode: "plan".to_string(),
            base_ref_kind: None,
            base_branch_mode: None,
            base_ref: None,
            base_display_name: None,
            base_source_pull_request: None,
            runtime_override: None,
        },
        &state,
    )
    .await;
    assert_eq!(
        public_result.expect_err("public switch should reject running agents"),
        "Cannot change mode while the agent is running"
    );

    let response = switch_agent_conversation_mode_for_state_allowing_running(
        SwitchAgentConversationModeInput {
            conversation_id: conversation_id.as_str(),
            mode: "plan".to_string(),
            base_ref_kind: None,
            base_branch_mode: None,
            base_ref: None,
            base_display_name: None,
            base_source_pull_request: None,
            runtime_override: None,
        },
        &state,
        ModeSwitchInitiator::User,
    )
    .await
    .expect("accepted proposal switch should bypass running guard");

    assert_eq!(response.conversation.agent_mode.as_deref(), Some("plan"));
    let stored = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup succeeds")
        .expect("workspace exists");
    assert_eq!(stored.mode, AgentConversationWorkspaceMode::Plan);
}

#[tokio::test]
async fn switching_edit_to_plan_quiesces_workspace_review_authority_before_persisting_mode() {
    let state = AppState::new_test();
    let project_id = ProjectId::from_string("project-plan-review-cleanup".to_string());
    let conversation_id =
        ChatConversationId::from_string("34343434-3434-4434-8434-343434343434".to_string());
    let mut conversation = ChatConversation::new_project(project_id.clone());
    conversation.id = conversation_id.clone();
    conversation.set_agent_mode(Some(AgentConversationWorkspaceMode::Edit));
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("conversation should persist");
    let workspace = AgentConversationWorkspace::new(
        conversation_id.clone(),
        project_id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::CurrentBranch,
        "main".to_string(),
        None,
        Some("base-sha".to_string()),
        "ralphx/test/plan-review-cleanup".to_string(),
        "/tmp/ralphx-plan-review-cleanup".to_string(),
    );
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");
    let review_conversation_id = ChatConversationId::from_string("review-runtime".to_string());
    let fixer_conversation_id = ChatConversationId::from_string("fixer-runtime".to_string());
    let artifact_id = ArtifactId::from_string("historical-review-artifact".to_string());
    let mut monitor = AgentWorkspaceReviewMonitor::new(conversation_id.clone(), project_id);
    monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Reviewing;
    monitor.review_conversation_id = Some(review_conversation_id.clone());
    monitor.review_fixer_status = Some("running".to_string());
    monitor.review_fixer_conversation_id = Some(fixer_conversation_id.clone());
    monitor.review_artifact_id = Some(artifact_id.clone());
    monitor.review_artifact_version = Some(4);
    monitor.auto_merge_guard = Some(AgentWorkspaceReviewAutoMergeGuard {
        status: AgentWorkspaceReviewAutoMergeGuardStatus::PausedForReview,
        pr_number: 42,
        merge_method: "squash".to_string(),
        target_scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        diff_fingerprint: "review-fingerprint".to_string(),
        head_sha: Some("head-sha".to_string()),
        last_error: None,
    });
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("review monitor should persist");
    let service = MockChatService::new();

    let response = switch_agent_conversation_mode_for_state_stopping_running_agent(
        SwitchAgentConversationModeInput {
            conversation_id: conversation_id.as_str(),
            mode: "plan".to_string(),
            base_ref_kind: None,
            base_branch_mode: None,
            base_ref: None,
            base_display_name: None,
            base_source_pull_request: None,
            runtime_override: None,
        },
        &state,
        &service,
    )
    .await
    .expect("review cleanup should allow the PLAN transition");

    assert_eq!(response.conversation.agent_mode.as_deref(), Some("plan"));
    let calls = service.get_stop_agent_calls().await;
    assert!(calls.contains(&(ChatContextType::Project, review_conversation_id.as_str())));
    assert!(calls.contains(&(ChatContextType::Project, fixer_conversation_id.as_str())));
    let stored = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert_eq!(stored.mode, AgentConversationWorkspaceMode::Plan);
    assert_eq!(stored.pr_auto_merge_current, Some(false));
    let cleaned = state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&conversation_id)
        .await
        .expect("monitor lookup should succeed")
        .expect("monitor should exist");
    assert_eq!(cleaned.status, AgentWorkspaceReviewMonitorStatus::Blocked);
    assert_eq!(
        cleaned.review_outcome,
        AgentWorkspaceReviewOutcome::RunFailed
    );
    assert_eq!(
        cleaned.review_gate_status,
        AgentWorkspaceReviewGateStatus::Failed
    );
    assert!(cleaned.review_fixer_status.is_none());
    assert!(cleaned.review_fixer_run_id.is_none());
    assert!(cleaned.auto_merge_guard.is_none());
    assert_eq!(cleaned.review_artifact_id, Some(artifact_id));
    assert_eq!(cleaned.review_artifact_version, Some(4));
    assert!(cleaned
        .last_error
        .as_deref()
        .is_some_and(|error| error.contains("mode changed to Plan")));
}

#[tokio::test]
async fn failed_workspace_review_runtime_cleanup_keeps_workspace_out_of_plan_mode() {
    let state = AppState::new_test();
    let project_id = ProjectId::from_string("project-plan-review-cleanup-failure".to_string());
    let conversation_id =
        ChatConversationId::from_string("45454545-4545-4454-8454-454545454545".to_string());
    let mut conversation = ChatConversation::new_project(project_id.clone());
    conversation.id = conversation_id.clone();
    conversation.set_agent_mode(Some(AgentConversationWorkspaceMode::Edit));
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("conversation should persist");
    let workspace = AgentConversationWorkspace::new(
        conversation_id.clone(),
        project_id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::CurrentBranch,
        "main".to_string(),
        None,
        Some("base-sha".to_string()),
        "ralphx/test/plan-review-cleanup-failure".to_string(),
        "/tmp/ralphx-plan-review-cleanup-failure".to_string(),
    );
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");
    let mut monitor = AgentWorkspaceReviewMonitor::new(conversation_id.clone(), project_id);
    monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Reviewing;
    monitor.review_conversation_id = Some(ChatConversationId::from_string(
        "review-runtime-cleanup-failure".to_string(),
    ));
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("review monitor should persist");
    let service = MockChatService::new();
    service.fail_next_stop_agent_calls(1).await;

    let error = switch_agent_conversation_mode_for_state_stopping_running_agent(
        SwitchAgentConversationModeInput {
            conversation_id: conversation_id.as_str(),
            mode: "plan".to_string(),
            base_ref_kind: None,
            base_branch_mode: None,
            base_ref: None,
            base_display_name: None,
            base_source_pull_request: None,
            runtime_override: None,
        },
        &state,
        &service,
    )
    .await
    .expect_err("failed runtime cleanup must reject the PLAN transition");

    assert!(error.contains("failed to stop Workspace Review runtime"));
    assert_eq!(
        state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist")
            .mode,
        AgentConversationWorkspaceMode::Edit
    );
}

#[tokio::test]
async fn switching_unlocked_linked_plan_ideation_to_edit_uses_plan_worktree() {
    let state = AppState::new_test();
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    let main_sha = setup_publish_repo(&repo_path);
    let plan_branch_name = "plan/manual-agent-handoff";
    git(&repo_path, &["branch", plan_branch_name]);

    let project_id = ProjectId::from_string("project-linked-plan-mode-switch".to_string());
    let conversation_id = ChatConversationId::from_string("eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee");
    let mut project = Project::new(
        "Linked Plan Mode Switch".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.id = project_id.clone();
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
    state
        .project_repo
        .create(project.clone())
        .await
        .expect("project persisted");

    let session = state
        .ideation_session_repo
        .create(IdeationSession::new(project_id.clone()))
        .await
        .expect("ideation session persisted");
    let mut execution_plan = ExecutionPlan::new(session.id.clone());
    execution_plan.status = ExecutionPlanStatus::Superseded;
    let execution_plan = state
        .execution_plan_repo
        .create(execution_plan)
        .await
        .expect("execution plan persisted");
    let mut plan_branch = PlanBranch::new(
        ArtifactId::from_string("artifact-linked-plan-mode-switch"),
        session.id.clone(),
        project_id.clone(),
        plan_branch_name.to_string(),
        "main".to_string(),
    );
    plan_branch.status = PlanBranchStatus::Active;
    plan_branch.execution_plan_id = Some(execution_plan.id);
    plan_branch.pr_number = Some(123);
    plan_branch.pr_url = Some("https://github.com/mock/repo/pull/123".to_string());
    plan_branch.pr_status = Some(PrStatus::Open);
    plan_branch.pr_push_status = PrPushStatus::Pushed;
    let plan_branch_id = plan_branch.id.clone();
    let expected_plan_worktree =
        resolve_linked_plan_branch_agent_worktree_path(&project, &plan_branch)
            .expect("expected plan worktree path should resolve");
    state
        .plan_branch_repo
        .create(plan_branch)
        .await
        .expect("plan branch persisted");

    let mut conversation = ChatConversation::new_project(project_id.clone());
    conversation.id = conversation_id.clone();
    conversation.set_agent_mode(Some(AgentConversationWorkspaceMode::Ideation));
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("conversation persisted");

    let mut workspace = AgentConversationWorkspace::new(
        conversation_id.clone(),
        project_id,
        AgentConversationWorkspaceMode::Ideation,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some(main_sha),
        "agent-shell-linked-plan".to_string(),
        temp.path()
            .join("agent-shell-linked-plan")
            .to_string_lossy()
            .to_string(),
    );
    workspace.linked_ideation_session_id = Some(session.id);
    workspace.linked_plan_branch_id = Some(plan_branch_id);
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace persisted");

    let response = switch_agent_conversation_mode_for_state(
        SwitchAgentConversationModeInput {
            conversation_id: conversation_id.as_str(),
            mode: "edit".to_string(),
            base_ref_kind: None,
            base_branch_mode: None,
            base_ref: None,
            base_display_name: None,
            base_source_pull_request: None,
            runtime_override: None,
        },
        &state,
    )
    .await
    .expect("linked plan ideation workspace should switch to edit");

    assert_eq!(response.conversation.agent_mode.as_deref(), Some("edit"));
    let switched = response.workspace.expect("workspace should be returned");

    assert_eq!(switched.mode, "edit");
    assert_eq!(switched.branch_name, plan_branch_name);
    assert_eq!(
        switched.worktree_path,
        expected_plan_worktree.to_string_lossy()
    );
    assert_eq!(switched.linked_ideation_session_id, None);
    assert_eq!(switched.linked_plan_branch_id, None);
    assert_eq!(switched.publication_pr_number, Some(123));
    assert_eq!(
        switched.publication_pr_url.as_deref(),
        Some("https://github.com/mock/repo/pull/123")
    );
    assert_eq!(switched.publication_pr_status.as_deref(), Some("open"));
    assert_eq!(switched.publication_push_status.as_deref(), Some("pushed"));
    assert_eq!(git(&repo_path, &["branch", "--show-current"]), "main");
    assert_eq!(
        GitService::get_current_branch(&expected_plan_worktree)
            .await
            .expect("plan worktree branch should be readable"),
        plan_branch_name
    );
}

#[tokio::test]
async fn switching_to_plan_defers_session_and_edit_preserves_link_but_clears_provider() {
    let state = AppState::new_test();
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_publish_repo(&repo_path);
    let project_id = ProjectId::from_string("project-plan-new-workspace".to_string());
    let conversation_id = ChatConversationId::from_string("dddddddd-dddd-4ddd-8ddd-dddddddddddd");
    let mut project = Project::new(
        "Mode Switch Plan Project".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.id = project_id.clone();
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
    state
        .project_repo
        .create(project)
        .await
        .expect("project persisted");
    let mut conversation = ChatConversation::new_project(project_id);
    conversation.id = conversation_id.clone();
    conversation.title = Some("Review CLI gaps".to_string());
    conversation.set_agent_mode(Some(AgentConversationWorkspaceMode::Chat));
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("conversation persisted");

    let plan_response = switch_agent_conversation_mode_for_state(
        SwitchAgentConversationModeInput {
            conversation_id: conversation_id.as_str(),
            mode: "plan".to_string(),
            base_ref_kind: Some("project_default".to_string()),
            base_branch_mode: None,
            base_ref: None,
            base_display_name: None,
            base_source_pull_request: None,
            runtime_override: None,
        },
        &state,
    )
    .await
    .expect("plan mode switch creates workspace");

    let plan_workspace = plan_response
        .workspace
        .as_ref()
        .expect("plan workspace should be returned");
    assert_eq!(plan_workspace.mode, "plan");
    assert!(
        plan_workspace.linked_ideation_session_id.is_none(),
        "idle Plan mode should not create an empty planning session"
    );
    assert!(plan_workspace.linked_plan_branch_id.is_none());

    let created_for_send =
        ensure_plan_workspace_planning_session_link_for_send(&state, &conversation_id)
            .await
            .expect("first Plan send should ensure a planning session");
    assert!(created_for_send);
    let second_ensure =
        ensure_plan_workspace_planning_session_link_for_send(&state, &conversation_id)
            .await
            .expect("existing planning session should be reused");
    assert!(!second_ensure);

    state
        .chat_conversation_repo
        .update_provider_session_ref(
            &conversation_id,
            &ProviderSessionRef {
                harness: AgentHarnessKind::Claude,
                provider_session_id: "planning-provider-session".to_string(),
            },
        )
        .await
        .expect("planning provider session should persist before Edit handoff");

    let plan_workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup succeeds")
        .expect("plan workspace should persist");
    let session_id = plan_workspace
        .linked_ideation_session_id
        .as_ref()
        .expect("first Plan send should link a planning session")
        .clone();
    let session = state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .expect("planning session lookup succeeds")
        .expect("planning session should exist");
    let conversation_id_string = conversation_id.as_str();
    assert_eq!(session.session_flow, IdeationSessionFlow::Planning);
    assert_eq!(session.title.as_deref(), Some("Review CLI gaps"));
    assert_eq!(session.title_source.as_deref(), Some("auto"));
    assert_eq!(
        session.source_context_type.as_deref(),
        Some("agent_conversation")
    );
    assert_eq!(
        session.source_context_id.as_deref(),
        Some(conversation_id_string.as_str())
    );
    assert_eq!(
        session.analysis.workspace_path.as_deref(),
        Some(plan_workspace.worktree_path.as_str())
    );
    assert!(plan_workspace.linked_plan_branch_id.is_none());

    let review_artifact_id =
        ArtifactId::from_string("historical-plan-mode-review-artifact".to_string());
    let mut monitor = AgentWorkspaceReviewMonitor::new(
        conversation_id.clone(),
        plan_workspace.project_id.clone(),
    );
    monitor.status = AgentWorkspaceReviewMonitorStatus::Ready;
    monitor.review_outcome = AgentWorkspaceReviewOutcome::Passed;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Passed;
    monitor.review_artifact_id = Some(review_artifact_id.clone());
    monitor.review_artifact_version = Some(3);
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("historical Plan review monitor should persist");

    let edit_response = switch_agent_conversation_mode_for_state(
        SwitchAgentConversationModeInput {
            conversation_id: conversation_id.as_str(),
            mode: "edit".to_string(),
            base_ref_kind: None,
            base_branch_mode: None,
            base_ref: None,
            base_display_name: None,
            base_source_pull_request: None,
            runtime_override: None,
        },
        &state,
    )
    .await
    .expect("edit mode switch preserves planning link");

    let edit_workspace = edit_response
        .workspace
        .as_ref()
        .expect("edit workspace should be returned");
    assert_eq!(edit_workspace.mode, "edit");
    assert_eq!(
        edit_workspace.linked_ideation_session_id.as_deref(),
        Some(session_id.as_str())
    );
    assert!(edit_workspace.linked_plan_branch_id.is_none());
    assert!(edit_response.conversation.provider_session_id.is_none());
    let cleaned_review = state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&conversation_id)
        .await
        .expect("review monitor lookup should succeed")
        .expect("historical review monitor should remain");
    assert_eq!(
        cleaned_review.review_outcome,
        AgentWorkspaceReviewOutcome::RunFailed
    );
    assert_eq!(
        cleaned_review.review_gate_status,
        AgentWorkspaceReviewGateStatus::Failed
    );
    assert_eq!(cleaned_review.review_artifact_id, Some(review_artifact_id));
}

#[tokio::test]
async fn switching_agent_mode_preserves_provider_session_for_native_resume() {
    let state = AppState::new_test();
    let project_id = ProjectId::from_string("project-mode-switch".to_string());
    let conversation_id = ChatConversationId::from_string("11111111-1111-4111-8111-111111111111");
    let mut conversation = ChatConversation::new_project(project_id.clone());
    conversation.id = conversation_id;
    conversation.set_agent_mode(Some(AgentConversationWorkspaceMode::Edit));
    conversation.set_provider_session_ref(ProviderSessionRef {
        harness: AgentHarnessKind::Codex,
        provider_session_id: "codex-thread-existing".to_string(),
    });
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("conversation persisted");

    let workspace = AgentConversationWorkspace::new(
        conversation_id,
        project_id,
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::CurrentBranch,
        "feature/agent-screen".to_string(),
        Some("Current branch (feature/agent-screen)".to_string()),
        Some("base-sha".to_string()),
        "ralphx/project/agent-11111111".to_string(),
        "/tmp/ralphx-agent-11111111".to_string(),
    );
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace persisted");

    let response = switch_agent_conversation_mode_for_state(
        SwitchAgentConversationModeInput {
            conversation_id: conversation_id.as_str(),
            mode: "ideation".to_string(),
            base_ref_kind: None,
            base_branch_mode: None,
            base_ref: None,
            base_display_name: None,
            base_source_pull_request: None,
            runtime_override: None,
        },
        &state,
    )
    .await
    .expect("mode switch succeeds");

    assert_eq!(
        response.conversation.agent_mode.as_deref(),
        Some("ideation")
    );
    assert_eq!(
        response.conversation.provider_session_id.as_deref(),
        Some("codex-thread-existing")
    );
    assert_eq!(
        response.conversation.provider_harness.as_deref(),
        Some("codex")
    );

    let stored = state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .expect("conversation load succeeds")
        .expect("conversation exists");
    assert_eq!(
        stored
            .provider_session_ref()
            .map(|session_ref| session_ref.provider_session_id),
        Some("codex-thread-existing".to_string())
    );
}

#[test]
fn parse_wrapped_mcp_result_object_extracts_embedded_json_payload() {
    let result = json!({
        "content": [
            {
                "type": "text",
                "text": "{\"delegated_session_id\":\"delegated-1\",\"status\":\"running\"}"
            }
        ]
    });

    let parsed = parse_wrapped_mcp_result_object(&result).expect("parsed result");

    assert_eq!(
        parsed
            .get("delegated_session_id")
            .and_then(|value| value.as_str()),
        Some("delegated-1")
    );
    assert_eq!(
        parsed.get("status").and_then(|value| value.as_str()),
        Some("running")
    );
}

#[test]
fn merge_delegated_snapshot_overrides_running_result_with_terminal_runtime_state() {
    let mut result = json!({
        "delegated_session_id": "delegated-1",
        "status": "running",
        "job_status": "running"
    });
    let snapshot = DelegatedToolRuntimeSnapshot {
        session_id: "delegated-1".to_string(),
        conversation_id: Some("conversation-1".to_string()),
        agent_run_id: Some("run-1".to_string()),
        agent_name: "ralphx-general-explorer".to_string(),
        title: Some("Plan evidence review".to_string()),
        harness: "codex".to_string(),
        provider_session_id: Some("provider-1".to_string()),
        session_status: "completed".to_string(),
        session_error: None,
        created_at: "2026-04-13T10:00:00Z".to_string(),
        updated_at: "2026-04-13T10:01:00Z".to_string(),
        completed_at: Some("2026-04-13T10:01:30Z".to_string()),
        latest_run: Some(json!({
            "agent_run_id": "run-1",
            "status": "completed"
        })),
        recent_messages: vec![json!({
            "role": "assistant",
            "content": "Completeness: no critical blockers found.",
            "created_at": "2026-04-13T10:01:20Z"
        })],
    };

    merge_delegated_snapshot_into_result(&mut result, &snapshot);

    assert_eq!(
        result.get("status").and_then(|value| value.as_str()),
        Some("completed")
    );
    assert_eq!(
        result.get("job_status").and_then(|value| value.as_str()),
        Some("completed")
    );
    assert_eq!(
        result
            .get("delegated_status")
            .and_then(|value| value.get("latest_run"))
            .and_then(|value| value.get("status"))
            .and_then(|value| value.as_str()),
        Some("completed")
    );
    assert_eq!(
        result
            .get("delegated_status")
            .and_then(|value| value.get("recent_messages"))
            .and_then(|value| value.as_array())
            .map(Vec::len),
        Some(1)
    );
}

#[test]
fn merge_delegated_snapshot_updates_mcp_wrapped_result_payload() {
    let mut result = json!({
        "content": [{
            "type": "text",
            "text": "{\"delegated_session_id\":\"delegated-wrapped\",\"status\":\"running\"}"
        }]
    });
    let snapshot = DelegatedToolRuntimeSnapshot {
        session_id: "delegated-wrapped".to_string(),
        conversation_id: Some("conversation-wrapped".to_string()),
        agent_run_id: Some("run-wrapped".to_string()),
        agent_name: "ralphx-general-explorer".to_string(),
        title: None,
        harness: "codex".to_string(),
        provider_session_id: None,
        session_status: "completed".to_string(),
        session_error: None,
        created_at: "2026-04-13T10:00:00Z".to_string(),
        updated_at: "2026-04-13T10:01:00Z".to_string(),
        completed_at: Some("2026-04-13T10:01:30Z".to_string()),
        latest_run: Some(json!({
            "agent_run_id": "run-wrapped",
            "status": "completed"
        })),
        recent_messages: Vec::new(),
    };

    merge_delegated_snapshot_into_result(&mut result, &snapshot);
    let parsed = parse_wrapped_mcp_result_object(&result).expect("wrapped result parses");

    assert_eq!(
        parsed.get("status").and_then(|value| value.as_str()),
        Some("completed")
    );
    assert_eq!(
        parsed
            .get("delegated_conversation_id")
            .and_then(|value| value.as_str()),
        Some("conversation-wrapped")
    );
    assert_eq!(
        parsed
            .get("delegated_status")
            .and_then(|value| value.get("latest_run"))
            .and_then(|value| value.get("agent_run_id"))
            .and_then(|value| value.as_str()),
        Some("run-wrapped")
    );
}

async fn seed_delegated_timeline_tool(
    state: &AppState,
    status: AgentRunStatus,
) -> (
    ChatConversationId,
    ChatTimelineItemId,
    crate::domain::entities::DelegatedSessionId,
    crate::domain::entities::AgentRunId,
) {
    let project_id = ProjectId::new();
    let parent = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project_id.clone()))
        .await
        .expect("create parent conversation");
    let mut session = DelegatedSession::new(
        project_id,
        "agent_conversation",
        parent.id.as_str(),
        "ralphx-general-explorer",
        AgentHarnessKind::Codex,
    );
    session.title = Some("Inspect delegate hydration".to_string());
    session.status = status.to_string();
    session.completed_at = Some(chrono::Utc::now());
    let session = state
        .delegated_session_repo
        .create(session)
        .await
        .expect("create delegated session");

    let child = state
        .chat_conversation_repo
        .create(ChatConversation::new_delegation(session.id.clone()))
        .await
        .expect("create delegated child conversation");

    let mut run = AgentRun::new(child.id);
    run.status = status;
    run.completed_at = Some(chrono::Utc::now());
    run.error_message =
        matches!(status, AgentRunStatus::Failed).then(|| "delegated review failed".to_string());
    run.harness = Some(AgentHarnessKind::Codex);
    run.provider_session_id = Some("codex-delegated-thread".to_string());
    run.upstream_provider = Some("openai".to_string());
    run.provider_profile = Some("openai".to_string());
    run.logical_model = Some("gpt-5.4".to_string());
    run.effective_model_id = Some("gpt-5.4".to_string());
    run.input_tokens = Some(9_877_122);
    run.output_tokens = Some(31_874);
    run.cache_read_tokens = Some(9_540_224);
    run.estimated_usd = Some(0.0125);
    let run = state
        .agent_run_repo
        .create(run)
        .await
        .expect("create delegated run");

    let mut child_message =
        ChatMessage::orchestrator_in_session(IdeationSessionId::new(), "Delegated final output");
    child_message.session_id = None;
    child_message.conversation_id = Some(child.id);
    state
        .chat_message_repo
        .create(child_message)
        .await
        .expect("create delegated message");

    let mut item = ChatTimelineItem::for_message_block(
        ChatMessageId::from_string("parent-delegate-message"),
        parent.id,
        0,
        MessageRole::Orchestrator,
        ChatTimelineItemKind::ToolUse,
    );
    item.status = ChatTimelineItemStatus::Finalized;
    item.tool_call_id = Some("delegate-tool-1".to_string());
    item.tool_name = Some("mcp__ralphx__delegate_start".to_string());
    item.input_json = Some(
        json!({
            "agent_name": "ralphx-general-explorer",
            "prompt": "Inspect delegate hydration"
        })
        .to_string(),
    );
    item.result_json = Some(
        json!({
            "delegated_session_id": session.id.as_str(),
            "delegated_conversation_id": child.id.as_str(),
            "delegated_agent_run_id": run.id.as_str(),
            "status": "running"
        })
        .to_string(),
    );
    let item = state
        .chat_timeline_repo
        .upsert_item(item)
        .await
        .expect("create delegate timeline item");

    (parent.id, item.id, session.id, run.id)
}

#[tokio::test]
async fn completed_delegate_timeline_page_and_detail_reconcile_durable_runtime_state() {
    let state = AppState::new_test();
    let (conversation_id, item_id, session_id, run_id) =
        seed_delegated_timeline_tool(&state, AgentRunStatus::Completed).await;

    let page =
        get_agent_conversation_timeline_page_for_app_state(&state, conversation_id, 10, None)
            .await
            .expect("timeline page")
            .expect("conversation exists");
    let page_result = &page.items[0]
        .tool_call
        .as_ref()
        .expect("delegate tool call")["result"];

    assert_eq!(page_result["delegated_session_id"], session_id.as_str());
    assert_eq!(page_result["delegated_agent_run_id"], run_id.as_str());
    assert_eq!(page_result["status"], "completed");
    assert_eq!(
        page_result["delegated_status"]["latest_run"]["status"],
        "completed"
    );
    assert_eq!(
        page_result["delegated_status"]["latest_run"]["total_tokens"],
        9_908_996
    );
    assert_eq!(
        page_result["delegated_status"]["recent_messages"][0]["content"],
        "Delegated final output"
    );

    let detail =
        get_agent_timeline_item_tool_call_detail_for_app_state(&state, conversation_id, item_id)
            .await
            .expect("timeline detail")
            .expect("timeline detail exists");
    let detail_result = &detail.tool_call["result"];

    assert_eq!(detail_result["delegated_session_id"], session_id.as_str());
    assert_eq!(detail_result["delegated_agent_run_id"], run_id.as_str());
    assert_eq!(
        detail_result["delegated_status"]["latest_run"]["status"],
        "completed"
    );
    assert_eq!(
        detail_result["delegated_status"]["latest_run"]["total_tokens"],
        9_908_996
    );
    assert_eq!(
        detail_result["delegated_status"]["recent_messages"][0]["content"],
        "Delegated final output"
    );
}

#[tokio::test]
async fn delegate_timeline_hydration_preserves_failed_and_cancelled_statuses() {
    for expected_status in [AgentRunStatus::Failed, AgentRunStatus::Cancelled] {
        let state = AppState::new_test();
        let (conversation_id, _, _, _) =
            seed_delegated_timeline_tool(&state, expected_status).await;

        let page =
            get_agent_conversation_timeline_page_for_app_state(&state, conversation_id, 10, None)
                .await
                .expect("timeline page")
                .expect("conversation exists");
        let result = &page.items[0]
            .tool_call
            .as_ref()
            .expect("delegate tool call")["result"];

        assert_eq!(result["status"], expected_status.to_string());
        assert_eq!(
            result["delegated_status"]["latest_run"]["status"],
            expected_status.to_string()
        );
    }
}

#[tokio::test]
async fn delegate_timeline_hydration_uses_stored_run_id_after_a_newer_retry() {
    let state = AppState::new_test();
    let (conversation_id, _, _, stored_run_id) =
        seed_delegated_timeline_tool(&state, AgentRunStatus::Failed).await;
    let page_before_retry =
        get_agent_conversation_timeline_page_for_app_state(&state, conversation_id, 10, None)
            .await
            .expect("timeline page")
            .expect("conversation exists");
    let delegated_conversation_id = page_before_retry.items[0]
        .tool_call
        .as_ref()
        .expect("delegate tool call")["result"]["delegated_conversation_id"]
        .as_str()
        .expect("delegated conversation id")
        .to_string();

    let mut retry = AgentRun::new(ChatConversationId::from_string(delegated_conversation_id));
    retry.status = AgentRunStatus::Completed;
    retry.completed_at = Some(chrono::Utc::now());
    state
        .agent_run_repo
        .create(retry)
        .await
        .expect("create newer retry");

    let page_after_retry =
        get_agent_conversation_timeline_page_for_app_state(&state, conversation_id, 10, None)
            .await
            .expect("timeline page")
            .expect("conversation exists");
    let result = &page_after_retry.items[0]
        .tool_call
        .as_ref()
        .expect("delegate tool call")["result"];

    assert_eq!(result["delegated_agent_run_id"], stored_run_id.as_str());
    assert_eq!(result["status"], "failed");
}

#[tokio::test]
async fn delegate_timeline_hydration_rejects_a_run_from_another_conversation() {
    let state = AppState::new_test();
    let (_, _, session_id, stored_run_id) =
        seed_delegated_timeline_tool(&state, AgentRunStatus::Completed).await;
    let stored_run = state
        .agent_run_repo
        .get_by_id(&stored_run_id)
        .await
        .expect("load stored run")
        .expect("stored run should exist");
    let delegated_conversation = state
        .chat_conversation_repo
        .get_by_id(&stored_run.conversation_id)
        .await
        .expect("load delegated conversation")
        .expect("delegated conversation should exist");
    let foreign_conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(ProjectId::new()))
        .await
        .expect("create foreign conversation");
    let foreign_run = state
        .agent_run_repo
        .create(AgentRun::new(foreign_conversation.id))
        .await
        .expect("create foreign run");

    let snapshot = load_delegated_tool_runtime_snapshot(
        &state,
        session_id.as_str(),
        Some(&delegated_conversation.id.as_str()),
        Some(&foreign_run.id.as_str()),
    )
    .await;

    assert!(snapshot.is_none());
}

#[tokio::test]
async fn delegate_timeline_hydration_keeps_sparse_result_when_session_is_missing() {
    let state = AppState::new_test();
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(ProjectId::new()))
        .await
        .expect("create conversation");
    let mut item = ChatTimelineItem::for_message_block(
        ChatMessageId::from_string("missing-delegate-message"),
        conversation.id,
        0,
        MessageRole::Orchestrator,
        ChatTimelineItemKind::ToolUse,
    );
    item.tool_name = Some("delegate_start".to_string());
    item.result_json = Some(
        json!({
            "delegated_session_id": "missing-session",
            "status": "running"
        })
        .to_string(),
    );
    state
        .chat_timeline_repo
        .upsert_item(item)
        .await
        .expect("create sparse delegate item");

    let page =
        get_agent_conversation_timeline_page_for_app_state(&state, conversation.id, 10, None)
            .await
            .expect("timeline page")
            .expect("conversation exists");
    let result = &page.items[0]
        .tool_call
        .as_ref()
        .expect("delegate tool call")["result"];

    assert_eq!(result["delegated_session_id"], "missing-session");
    assert_eq!(result["status"], "running");
    assert!(result.get("delegated_status").is_none());
}

#[tokio::test]
async fn conversation_timeline_page_limits_visible_items_not_message_rows() {
    let state = AppState::new_test();
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(ProjectId::new()))
        .await
        .expect("create conversation");
    let message_id = ChatMessageId::from_string("assistant-message-1");

    for index in 0..3 {
        let mut item = ChatTimelineItem::for_message_block(
            message_id.clone(),
            conversation.id,
            index,
            MessageRole::Orchestrator,
            ChatTimelineItemKind::Text,
        );
        item.status = ChatTimelineItemStatus::Finalized;
        item.text = Some(format!("block {index}"));
        state
            .chat_timeline_repo
            .upsert_item(item)
            .await
            .expect("upsert timeline item");
    }

    let newest_page =
        get_agent_conversation_timeline_page_for_app_state(&state, conversation.id, 2, None)
            .await
            .expect("timeline page")
            .expect("conversation exists");

    assert_eq!(newest_page.items.len(), 2);
    assert_eq!(newest_page.total_item_count, 3);
    assert!(newest_page.has_older);
    assert_eq!(
        newest_page
            .items
            .iter()
            .map(|item| item.content.as_str())
            .collect::<Vec<_>>(),
        vec!["block 1", "block 2"]
    );

    let older_page = get_agent_conversation_timeline_page_for_app_state(
        &state,
        conversation.id,
        2,
        newest_page.oldest_loaded_sequence,
    )
    .await
    .expect("older timeline page")
    .expect("conversation exists");

    assert_eq!(older_page.items.len(), 1);
    assert!(!older_page.has_older);
    assert_eq!(older_page.items[0].content, "block 0");
}

#[test]
fn timeline_item_response_builds_text_message_block() {
    let conversation_id = ChatConversationId::new();
    let message_id = ChatMessageId::from_string("assistant-message-text");
    let mut item = ChatTimelineItem::for_message_block(
        message_id.clone(),
        conversation_id,
        0,
        MessageRole::Orchestrator,
        ChatTimelineItemKind::Text,
    );
    item.sequence = 12;
    item.status = ChatTimelineItemStatus::Finalized;
    item.text = Some("final answer".to_string());
    item.metadata = Some(r#"{"source":"test"}"#.to_string());

    let response = AgentTimelineItemResponse::from(item);

    assert_eq!(response.message_id.as_deref(), Some(message_id.as_str()));
    assert_eq!(response.content, "final answer");
    assert_eq!(response.kind, "text");
    assert!(response.tool_call.is_none());
    assert_eq!(
        response.content_blocks,
        json!([{ "type": "text", "text": "final answer" }])
    );
    assert_eq!(response.metadata.as_deref(), Some(r#"{"source":"test"}"#));
}

#[test]
fn timeline_item_response_builds_thinking_block_with_duration_and_reasoning_tokens() {
    let conversation_id = ChatConversationId::new();
    let message_id = ChatMessageId::from_string("assistant-message-thinking");
    let mut item = ChatTimelineItem::for_message_block(
        message_id,
        conversation_id,
        0,
        MessageRole::Orchestrator,
        ChatTimelineItemKind::Thinking,
    );
    item.text = Some("Considering the request".to_string());
    item.metadata = Some(r#"{"duration_ms":1234,"reasoning_tokens":321}"#.to_string());

    let response = AgentTimelineItemResponse::from(item);

    assert!(response.tool_call.is_none());
    assert_eq!(
        response.content_blocks,
        json!([{
            "type": "thinking",
            "text": "Considering the request",
            "duration_ms": 1234,
            "reasoning_tokens": 321
        }])
    );
}

#[test]
fn timeline_item_response_builds_thinking_block_without_duration_or_tool_use() {
    let conversation_id = ChatConversationId::new();
    let message_id = ChatMessageId::from_string("assistant-message-thinking-no-duration");
    let mut item = ChatTimelineItem::for_message_block(
        message_id,
        conversation_id,
        0,
        MessageRole::Orchestrator,
        ChatTimelineItemKind::Thinking,
    );
    item.text = Some("Still considering".to_string());

    let response = AgentTimelineItemResponse::from(item);
    let block = &response.content_blocks[0];

    assert!(response.tool_call.is_none());
    assert_eq!(
        block,
        &json!({ "type": "thinking", "text": "Still considering" })
    );
    assert!(block.get("duration_ms").is_none());
    assert_ne!(block["type"], "tool_use");
}

#[tokio::test]
async fn conversation_timeline_page_hydrates_persisted_thinking_item() {
    let state = AppState::new_test();
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(ProjectId::new()))
        .await
        .expect("create conversation");
    let mut item = ChatTimelineItem::for_message_block(
        ChatMessageId::from_string("assistant-message-thinking-page"),
        conversation.id,
        0,
        MessageRole::Orchestrator,
        ChatTimelineItemKind::Thinking,
    );
    item.text = Some("Persisted reasoning".to_string());
    item.metadata = Some(r#"{"duration_ms":1234,"reasoning_tokens":321}"#.to_string());
    state
        .chat_timeline_repo
        .upsert_item(item)
        .await
        .expect("upsert thinking timeline item");

    let page =
        get_agent_conversation_timeline_page_for_app_state(&state, conversation.id, 10, None)
            .await
            .expect("timeline page")
            .expect("conversation exists");

    assert_eq!(
        page.items[0].content_blocks,
        json!([{
            "type": "thinking",
            "text": "Persisted reasoning",
            "duration_ms": 1234,
            "reasoning_tokens": 321
        }])
    );
    assert!(page.items[0].tool_call.is_none());
}

#[test]
fn timeline_item_response_builds_tool_block_with_detail_ref_and_diff_context() {
    let conversation_id = ChatConversationId::new();
    let message_id = ChatMessageId::from_string("assistant-message-tool");
    let mut item = ChatTimelineItem::for_message_block(
        message_id.clone(),
        conversation_id,
        3,
        MessageRole::Orchestrator,
        ChatTimelineItemKind::ToolUse,
    );
    item.sequence = 22;
    item.status = ChatTimelineItemStatus::Finalized;
    item.tool_call_id = Some("tool-1".to_string());
    item.tool_name = Some("bash".to_string());
    item.input_json = Some(r#"{"command":"cargo test"}"#.to_string());
    item.result_json = Some(r#""ok""#.to_string());
    item.raw_block_json =
        Some(r#"{"type":"tool_use","diff_context":{"file_path":"src/lib.rs"}}"#.to_string());
    item.provider_harness = Some(AgentHarnessKind::Codex);
    item.provider_session_id = Some("thread-1".to_string());

    let response = AgentTimelineItemResponse::from(item);
    let tool = response.tool_call.expect("tool response");

    assert_eq!(response.kind, "tool_use");
    assert_eq!(response.provider_harness.as_deref(), Some("codex"));
    assert_eq!(response.provider_session_id.as_deref(), Some("thread-1"));
    assert_eq!(tool["id"], "tool-1");
    assert_eq!(tool["name"], "bash");
    assert_eq!(tool["arguments"]["command"], "cargo test");
    assert_eq!(tool["result"], "ok");
    assert_eq!(
        tool["detail_ref"]["timeline_item_id"].as_str(),
        Some(response.id.as_str())
    );
    assert_eq!(
        tool["detail_ref"]["message_id"].as_str(),
        Some(message_id.as_str())
    );
    assert_eq!(tool["diff_context"]["file_path"], "src/lib.rs");
}

#[test]
fn timeline_item_response_reconstructs_tool_block_without_raw_payload() {
    let conversation_id = ChatConversationId::new();
    let message_id = ChatMessageId::from_string("assistant-message-no-raw-payload");
    let mut item = ChatTimelineItem::for_message_block(
        message_id,
        conversation_id,
        0,
        MessageRole::Orchestrator,
        ChatTimelineItemKind::ToolUse,
    );
    item.tool_call_id = Some("tool-bash".to_string());
    item.tool_name = Some("bash".to_string());
    item.input_json = Some(r#"{"command":"cargo test"}"#.to_string());
    item.result_json = Some(r#""ok""#.to_string());

    let response = AgentTimelineItemResponse::from(item);
    let tool = response.tool_call.expect("tool response");

    assert_eq!(tool["id"], "tool-bash");
    assert_eq!(tool["name"], "bash");
    assert_eq!(tool["arguments"]["command"], "cargo test");
    assert_eq!(tool["result"], "ok");
    assert!(tool.get("diff_context").is_none());
}

#[test]
fn preview_tool_payloads_preserves_parseable_mcp_artifact_preview() {
    let artifact_content = "Detailed artifact line.\n".repeat(600);
    let artifact = json!({
        "id": "artifact-preview-1",
        "title": "Previewable Artifact",
        "artifact_type": "design_doc",
        "content": artifact_content,
        "version": 3
    });
    let tool_calls = json!([{
        "id": "tool-artifact-1",
        "name": "mcp__ralphx__get_artifact",
        "arguments": { "artifact_id": "artifact-preview-1" },
        "result": {
            "content": [{
                "type": "text",
                "text": serde_json::to_string(&artifact).expect("artifact json")
            }]
        }
    }]);

    let (tool_calls, _) =
        preview_tool_payloads_for_message("conversation-1", "message-1", Some(tool_calls), None);
    let tool_calls = tool_calls.expect("previewed tool calls");
    let tool = &tool_calls.as_array().expect("tool call array")[0];
    let preview_text = tool["result"]["content"][0]["text"]
        .as_str()
        .expect("mcp text content preview");
    let parsed_preview: serde_json::Value =
        serde_json::from_str(preview_text).expect("preview text remains valid JSON");

    assert_eq!(tool["result_preview_truncated"], true);
    assert_eq!(parsed_preview["title"], "Previewable Artifact");
    assert_eq!(parsed_preview["artifact_type"], "design_doc");
    assert_eq!(parsed_preview["version"], 3);
    assert!(
        parsed_preview["content"]
            .as_str()
            .expect("content preview string")
            .len()
            < artifact_content.len(),
        "artifact content should stay bounded in the paginated preview"
    );
    assert_eq!(
        tool["detail_ref"],
        json!({
            "conversation_id": "conversation-1",
            "message_id": "message-1",
            "tool_call_id": "tool-artifact-1",
            "content_block_index": null
        })
    );
}

#[test]
fn preview_tool_payloads_replaces_edit_arguments_with_first_diff_hunk() {
    let old_content = [
        "line 1", "line 2", "line 3", "line 4", "line 5", "line 6", "line 7", "line 8", "line 9",
        "line 10", "line 11", "line 12",
    ]
    .join("\n");
    let new_content = [
        "line 1",
        "line 2 changed",
        "line 3",
        "line 4",
        "line 5",
        "line 6",
        "line 7",
        "line 8",
        "line 9",
        "line 10 changed",
        "line 11",
        "line 12",
    ]
    .join("\n");
    let tool_calls = json!([{
        "id": "tool-edit-1",
        "name": "edit",
        "arguments": {
            "file_path": "src/example.ts",
            "old_string": old_content,
            "new_string": new_content,
            "replace_all": false
        },
        "result": { "status": "ok" }
    }]);

    let (tool_calls, _) =
        preview_tool_payloads_for_message("conversation-1", "message-1", Some(tool_calls), None);
    let tool_calls = tool_calls.expect("previewed tool calls");
    let tool = &tool_calls.as_array().expect("tool call array")[0];
    let diff_preview_text =
        serde_json::to_string(&tool["diff_preview"]).expect("diff preview serializes");

    assert_eq!(tool["arguments_preview_truncated"], true);
    assert_eq!(tool["arguments"]["file_path"], "src/example.ts");
    assert_eq!(tool["arguments"]["replace_all"], false);
    assert!(tool["arguments"]["old_string"].is_null());
    assert!(tool["arguments"]["new_string"].is_null());
    assert_eq!(
        tool["detail_ref"],
        json!({
            "conversation_id": "conversation-1",
            "message_id": "message-1",
            "tool_call_id": "tool-edit-1",
            "content_block_index": null
        })
    );
    assert_eq!(tool["diff_preview"]["file_path"], "src/example.ts");
    assert_eq!(tool["diff_preview"]["language"], "typescript");
    assert!(diff_preview_text.contains("line 2 changed"));
    assert!(!diff_preview_text.contains("line 10 changed"));
}

#[test]
fn preview_tool_payloads_replaces_write_content_and_diff_context_with_diff_preview() {
    let content_blocks = json!([{
        "type": "tool_use",
        "id": "tool-write-1",
        "name": "write",
        "arguments": {
            "file_path": "src/lib.rs",
            "content": "fn main() {\n    println!(\"new\");\n}"
        },
        "diff_context": {
            "file_path": "src/lib.rs",
            "old_content": "fn main() {\n    println!(\"old\");\n}"
        },
        "result": { "status": "ok" }
    }]);

    let (_, content_blocks) = preview_tool_payloads_for_message(
        "conversation-1",
        "message-1",
        None,
        Some(content_blocks),
    );
    let content_blocks = content_blocks.expect("previewed content blocks");
    let tool = &content_blocks.as_array().expect("content block array")[0];

    assert_eq!(tool["arguments_preview_truncated"], true);
    assert_eq!(tool["arguments"]["file_path"], "src/lib.rs");
    assert!(tool["arguments"]["content"].is_null());
    assert_eq!(tool["diff_context"]["file_path"], "src/lib.rs");
    assert!(tool["diff_context"]["old_content"].is_null());
    assert_eq!(tool["diff_preview"]["file_path"], "src/lib.rs");
    assert_eq!(
        tool["detail_ref"]["content_block_index"],
        serde_json::json!(0)
    );
}

#[test]
fn preview_tool_payloads_renders_new_write_as_added_diff() {
    let content_blocks = json!([{
        "type": "tool_use",
        "id": "tool-write-new",
        "name": "write",
        "arguments": {
            "file_path": "src/new.rs",
            "content": "pub fn new() {}\n"
        },
        "diff_context": {
            "file_path": "src/new.rs",
            "old_file_exists": false
        },
        "result": { "status": "ok" }
    }]);

    let (_, content_blocks) = preview_tool_payloads_for_message(
        "conversation-1",
        "message-1",
        None,
        Some(content_blocks),
    );
    let content_blocks = content_blocks.expect("previewed content blocks");
    let tool = &content_blocks.as_array().expect("content block array")[0];

    assert_eq!(tool["arguments_preview_truncated"], true);
    assert_eq!(tool["arguments"]["file_path"], "src/new.rs");
    assert!(tool["arguments"]["content"].is_null());
    assert_eq!(tool["diff_context"]["old_file_exists"], false);
    assert_eq!(tool["diff_preview"]["old_total_lines"], 0);
    assert_eq!(tool["diff_preview"]["new_total_lines"], 2);
    assert_eq!(
        tool["diff_preview"]["hunks"][0]["lines"][0]["kind"],
        "addition"
    );
}

#[tokio::test]
async fn timeline_item_response_previews_edit_arguments_but_detail_returns_full_payload() {
    let state = AppState::new_test();
    let conversation_id = ChatConversationId::new();
    let message_id = ChatMessageId::from_string("assistant-message-edit");
    let mut item = ChatTimelineItem::for_message_block(
        message_id.clone(),
        conversation_id,
        0,
        MessageRole::Orchestrator,
        ChatTimelineItemKind::ToolUse,
    );
    item.tool_call_id = Some("tool-edit-timeline".to_string());
    item.tool_name = Some("edit".to_string());
    item.input_json = Some(
        json!({
            "file_path": "src/example.ts",
            "old_string": "old line",
            "new_string": "new line"
        })
        .to_string(),
    );

    let response = AgentTimelineItemResponse::from(item.clone());
    let preview_tool = response.tool_call.expect("timeline tool preview");
    assert_eq!(preview_tool["arguments_preview_truncated"], true);
    assert!(preview_tool["arguments"]["old_string"].is_null());
    assert_eq!(
        preview_tool["detail_ref"]["timeline_item_id"].as_str(),
        Some(response.id.as_str())
    );

    let item = state
        .chat_timeline_repo
        .upsert_item(item)
        .await
        .expect("insert timeline edit item");
    let detail =
        get_agent_timeline_item_tool_call_detail_for_app_state(&state, conversation_id, item.id)
            .await
            .expect("timeline edit detail lookup")
            .expect("timeline edit detail");

    assert_eq!(detail.tool_call["arguments"]["old_string"], "old line");
    assert_eq!(detail.tool_call["arguments"]["new_string"], "new line");
    assert!(detail.tool_call["arguments_preview_truncated"].is_null());
}

#[test]
fn chat_timeline_domain_values_cover_all_variants_from_app_crate_tests() {
    let generated_id = ChatTimelineItemId::new();
    assert!(!generated_id.as_str().is_empty());
    assert!(!ChatTimelineItemId::default().as_str().is_empty());

    for (raw, kind) in [
        ("text", ChatTimelineItemKind::Text),
        ("tool_use", ChatTimelineItemKind::ToolUse),
        ("task", ChatTimelineItemKind::Task),
        ("system_notice", ChatTimelineItemKind::SystemNotice),
        ("error", ChatTimelineItemKind::Error),
    ] {
        assert_eq!(kind.to_string(), raw);
        assert_eq!(ChatTimelineItemKind::from_str(raw), Ok(kind));
    }

    for (raw, status) in [
        ("streaming", ChatTimelineItemStatus::Streaming),
        ("finalized", ChatTimelineItemStatus::Finalized),
        ("error", ChatTimelineItemStatus::Error),
    ] {
        assert_eq!(status.to_string(), raw);
        assert_eq!(ChatTimelineItemStatus::from_str(raw), Ok(status));
    }

    assert!(ChatTimelineItemKind::from_str("bogus").is_err());
    assert!(ChatTimelineItemStatus::from_str("bogus").is_err());
}

#[tokio::test]
async fn timeline_item_detail_returns_none_for_missing_or_mismatched_item() {
    let state = AppState::new_test();
    let conversation_id = ChatConversationId::new();

    let missing = get_agent_timeline_item_tool_call_detail_for_app_state(
        &state,
        conversation_id,
        ChatTimelineItemId::from_string("missing"),
    )
    .await
    .expect("missing detail lookup");
    assert!(missing.is_none());

    let other_conversation_id = ChatConversationId::new();
    let message_id = ChatMessageId::from_string("assistant-message-tool");
    let mut item = ChatTimelineItem::for_message_block(
        message_id,
        other_conversation_id,
        0,
        MessageRole::Orchestrator,
        ChatTimelineItemKind::ToolUse,
    );
    item.tool_call_id = Some("tool-other".to_string());
    item.tool_name = Some("Read".to_string());
    item.input_json = Some(r#"{"file_path":"src/lib.rs"}"#.to_string());
    let item = state
        .chat_timeline_repo
        .upsert_item(item)
        .await
        .expect("insert mismatched timeline item");

    let mismatched =
        get_agent_timeline_item_tool_call_detail_for_app_state(&state, conversation_id, item.id)
            .await
            .expect("mismatched detail lookup");
    assert!(mismatched.is_none());
}

#[tokio::test]
async fn timeline_item_detail_uses_preview_fallbacks_for_partial_tool_payload() {
    let state = AppState::new_test();
    let conversation_id = ChatConversationId::new();
    let mut item = ChatTimelineItem {
        id: ChatTimelineItemId::from_string("timeline-tool-preview"),
        conversation_id,
        message_id: None,
        run_id: None,
        sequence: 4,
        block_index: 2,
        role: MessageRole::Orchestrator,
        kind: ChatTimelineItemKind::ToolUse,
        status: ChatTimelineItemStatus::Streaming,
        text: None,
        tool_call_id: None,
        tool_name: None,
        tool_status: Some("pending".to_string()),
        tool_input_preview: Some(r#"{"path":"src/lib.rs"}"#.to_string()),
        tool_result_preview: Some("preview result".to_string()),
        input_json: None,
        result_json: None,
        raw_block_json: Some(r#"{"type":"tool_use","extra":true}"#.to_string()),
        metadata: None,
        provider_harness: None,
        provider_session_id: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        finalized_at: None,
    };
    item = state
        .chat_timeline_repo
        .upsert_item(item)
        .await
        .expect("insert preview timeline item");

    let detail = get_agent_timeline_item_tool_call_detail_for_app_state(
        &state,
        conversation_id,
        item.id.clone(),
    )
    .await
    .expect("preview detail lookup")
    .expect("preview detail");
    let tool = detail.tool_call;

    assert_eq!(tool["id"], item.id.to_string());
    assert_eq!(tool["name"], "unknown");
    assert_eq!(tool["arguments"]["path"], "src/lib.rs");
    assert_eq!(tool["result"], "preview result");
    assert_eq!(tool["detail_ref"]["message_id"], item.id.to_string());
    assert_eq!(tool["detail_ref"]["content_block_index"], 2);
}
